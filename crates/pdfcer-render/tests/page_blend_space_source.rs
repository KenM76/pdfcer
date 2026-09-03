//! ★ `Pass 122.5` — where a page's blending colour space comes from when the
//! page group declares none, and the disclosure that says which.
//!
//! # The claim under test, and why it is a setting rather than a fix
//!
//! **ISO 32000-1 is determinate here and determinate AGAINST consulting the
//! output intent.** §11.4.7 and §11.6.3 each state independently that *"if not
//! otherwise specified, the page group's colour space **shall** be inherited
//! from the native colour space of the output device"* — `shall`, no hedge —
//! and `/OutputIntent` is absent from the 1.7 transparency model entirely.
//!
//! **ISO 32000-2 opens it, and only informatively.** Annex P offers *"from the
//! output device, **or** from the output intent"* with no ranking, no
//! condition and no precedence, so two conformant PDF 2.0 processors render
//! the same file in two different blending spaces and both cite the same
//! annex. There is no reading under which one answer is simply wrong, which is
//! exactly what makes this a setting.
//!
//! # What actually turns on it
//!
//! Overprint, and not by a matter of degree. §11.7.4.3's second bullet makes
//! `B(c_b, c_s)` equal `c_s` for every component *"specified in the current
//! colour space"*; in sRGB every source colour has already been converted to
//! all three components, so every component is specified and `B = c_s`
//! **everywhere**. Overprint in an additive space is therefore
//! **unrepresentable**, not merely unsimulated — no compositing work recovers
//! it, only an n-colorant buffer does.
//!
//! # Why the assertions are written as a PAIR
//!
//! A test that only checked the new default would pass against an
//! implementation that ignored the setting and always used ink. A test that
//! only checked `DeviceNative` would pass against one that never used it. Each
//! variant is therefore asserted to produce the *other* answer on the same
//! file — the same non-vacuity discipline `R162` asks for.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use pdfcer_core::document::Document;
use pdfcer_core::settings::PageBlendSpaceSource;
use pdfcer_render::{RenderOptions, RenderedPage};

/// Resolve a patch id (`PCS1_011`) to a file, through the private map.
///
/// # Why an id and an environment variable rather than a path
///
/// Operator ruling 2026-08-25: the licensed print-conformance suite is not
/// named anywhere in this repository, and neither are its file names — the
/// repository is public, and the suite's licence carries an affirmative-notice
/// requirement and a commercial-context restriction. So this test knows a
/// stable **id** and nothing else.
///
/// The resolution chain, in order, and it **never fails, only skips**:
///
/// 1. `PDFCER_SUITE_DIR` — the corpus directory, set by whoever has a licensed
///    copy. Absent on every fresh clone, which is the normal case.
/// 2. `<dir>/pdfcer-manifest.txt` — `id=filename`, one per line, `#` comments.
///    It lives *with the corpus*, outside the repository, because the file
///    names are as much the suite's material as the artwork is.
///
/// A missing variable, a missing manifest, an id the manifest does not carry,
/// and a named file that is not on disk are **all skips**, each announced on
/// stdout. A silently-skipped test is indistinguishable from a passing one,
/// which is the failure mode this project keeps re-learning.
fn resolve(id: &str) -> Option<PathBuf> {
    let Ok(dir) = std::env::var("PDFCER_SUITE_DIR") else {
        println!("SKIP: {id} — PDFCER_SUITE_DIR is unset (external corpus absent)");
        return None;
    };
    let manifest = PathBuf::from(&dir).join("pdfcer-manifest.txt");
    let Ok(text) = std::fs::read_to_string(&manifest) else {
        println!("SKIP: {id} — no manifest at {}", manifest.display());
        return None;
    };
    let name = text.lines().find_map(|line| {
        let line = line.trim();
        if line.starts_with('#') {
            return None;
        }
        let (key, value) = line.split_once('=')?;
        (key.trim() == id).then(|| value.trim().to_owned())
    });
    let Some(name) = name else {
        println!("SKIP: {id} — not listed in {}", manifest.display());
        return None;
    };
    let path = PathBuf::from(dir).join(name);
    if path.exists() {
        Some(path)
    } else {
        println!("SKIP: {id} — listed but not present on disk");
        None
    }
}

/// Render page 1 of a suite patch under an explicit blend-space source.
///
/// The corpus lives outside the repository (`docs/LEGAL.md` §5 — it is not
/// redistributable here), so every test in this file **skips** rather than
/// fails when it is absent. See [`resolve`] for the chain and for why the
/// patch is addressed by id.
fn render(name: &str, source: PageBlendSpaceSource) -> Option<RenderedPage> {
    let path = resolve(name)?;
    if !path.exists() {
        println!("SKIP: {} not present (external corpus)", path.display());
        return None;
    }
    let doc = Document::from_bytes(std::fs::read(&path).ok()?).expect("fixture parses");
    let pages = pdfcer_core::page_tree::pages(&doc).expect("page tree");
    let options = RenderOptions::default().with_page_blend_space_source(source);
    Some(pdfcer_render::render_page_with(&doc, &pages[0], 2.0, &options).expect("renders"))
}

/// The patch this Pass was found on: PDF/X-3, no page group `/CS`, a CMYK
/// output intent, and overprint content.
const OVERPRINT_PATCH: &str = "PCS1_011";

/// Under the shipped default, a CMYK output intent supplies the blending
/// space, the colorant buffer engages, and the provenance says so.
#[test]
fn a_cmyk_output_intent_supplies_the_space_by_default() {
    let Some(page) = render(
        OVERPRINT_PATCH,
        PageBlendSpaceSource::OutputIntentIfSubtractive,
    ) else {
        return;
    };
    assert_eq!(
        page.diagnostics.blend_space_from, "output_intent",
        "the default must take the space from the output intent on a file \
         whose page group declares none"
    );
    assert!(
        page.diagnostics.blend_space_subtractive > 0,
        "a CMYK output intent must yield a subtractive page"
    );
}

/// ★ The other half of the pair. `DeviceNative` is ISO 32000-1 to the letter,
/// and must produce the *opposite* answer on the same file — otherwise the
/// setting is decorative and the test above proves nothing about it.
#[test]
fn device_native_reproduces_the_iso_32000_1_answer() {
    let Some(page) = render(OVERPRINT_PATCH, PageBlendSpaceSource::DeviceNative) else {
        return;
    };
    assert_eq!(
        page.diagnostics.blend_space_from, "device_native",
        "DeviceNative must not consult the output intent"
    );
    assert_eq!(
        page.diagnostics.blend_space_subtractive, 0,
        "pdfcer's output device is an RGBA8 pixmap, so §11.4.7's answer for \
         this file is additive"
    );
}

/// A page that DECLARES its group `/CS` is answered by Table 147, and no
/// setting reaches it.
///
/// This is the guard against the fix over-reaching: the setting exists to fill
/// a silence, and a file that is not silent must be unaffected by it. Asserted
/// across BOTH variants, because "unaffected" is a claim about the whole
/// setting rather than about one of its values.
#[test]
fn a_declared_page_group_is_immune_to_the_setting() {
    const DECLARED: &str = "PCS1_160";
    let Some(native) = render(DECLARED, PageBlendSpaceSource::DeviceNative) else {
        return;
    };
    let Some(intent) = render(DECLARED, PageBlendSpaceSource::OutputIntentIfSubtractive) else {
        return;
    };
    for (label, page) in [("device_native", &native), ("output_intent", &intent)] {
        assert_eq!(
            page.diagnostics.blend_space_from, "page_group",
            "{label}: a declared /Group /CS is its own answer"
        );
        assert!(
            page.diagnostics.blend_space_subtractive > 0,
            "{label}: this patch declares /Group /CS /DeviceCMYK"
        );
    }
    // And the pixels agree, which is the assertion that would actually catch
    // a regression: the provenance string could be right while the space
    // silently differed.
    assert_eq!(
        native.pixmap.data(),
        intent.pixmap.data(),
        "a declared page group must render identically under every value of \
         the setting"
    );
}

/// The provenance is never empty for a page that painted content.
///
/// Reporting it only when it is *interesting* would make its absence ambiguous
/// between "not inferred" and "not recorded", and an ambiguous disclosure is
/// worse than none — a reader cannot tell which question it answered.
#[test]
fn the_provenance_is_always_reported() {
    let Some(page) = render(
        OVERPRINT_PATCH,
        PageBlendSpaceSource::OutputIntentIfSubtractive,
    ) else {
        return;
    };
    assert!(
        !page.diagnostics.blend_space_from.is_empty(),
        "a page that painted content must always disclose where its blending \
         space came from"
    );
}

/// ★ A `DeviceN` shading under overprint is painted in ink and honours it.
///
/// Found 2026-08-25 when the operator read `PCS 1.0` cells `e` and `j` and
/// said they carry no trap X but are *"the wrong colour … always have been"*.
/// He was right: `shading.rs` had no mention of overprint at all.
///
/// **This assertion was inverted on the day it was written**, and deliberately
/// left in the record that way. It first read `overprint_shadings_unsupported
/// == 2` and pinned the *gap* — two shadings that could not overprint. `Pass
/// 122.6` closed the gap, the test failed, and that failure is exactly what a
/// disclosure test is for: the counter it watches is the one the fix must
/// empty. Rewriting it to `== 0` is the fix being observed, not the test being
/// weakened.
///
/// Zero alone would be a weak claim, though — a counter is also zero when
/// nothing was painted at all. So the paint is asserted beside it.
#[test]
fn a_devicen_shading_under_overprint_is_painted_in_ink() {
    let Some(page) = render("PCS1_010", PageBlendSpaceSource::OutputIntentIfSubtractive) else {
        return;
    };
    assert_eq!(
        page.diagnostics.shading.painted, 4,
        "PCS 1.0 paints four shadings; a zero disclosure below means nothing \
         unless they were actually painted"
    );
    assert_eq!(
        page.diagnostics.overprint_shadings_unsupported, 0,
        "both of PCS 1.0's overprinting shadings are /DeviceN over a \
         DeviceCMYK alternate, so both take the native ink route and neither \
         needs disclosing"
    );
}
