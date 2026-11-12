//! # Spot colorants in the vector flattener (`Pass 700.21`)
//!
//! On a page whose group blends in DeviceCMYK, flat transparency and
//! overprint over `Separation`, `DeviceN` and `/All` paints are written as
//! opaque regions whose colour folds each spot plane's ink in as the
//! raster's colorant buffer does. The oracle is GDI's `PlayEnhMetaFile`
//! (Windows only) against the white-backed raster.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_emf::{EmfExport, EmfOptions, export_emf, walk_records};
use pdfcer_render::{PageBackdrop, RenderOptions, render_page_with};
use tiny_skia::Pixmap;

use crate::export_emf::{DPI, assert_well_formed, build, count};
use crate::export_emf_patches::fnv1a;
#[cfg(windows)]
use crate::export_emf_patches::{ROUNDING, playback_against_raster};

/// `/A`: tint type 2 onto magenta. `/B`: type 0, sampled, onto yellow.
/// `/C`: type 4 onto cyan with a fifth of black. `/D`: five spot inks
/// whose type 4 transform drops the fifth. `/L`: `/All`.
const SPACES: &str = "/ColorSpace << \
    /A [/Separation /SpotA /DeviceCMYK << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 1 0 0] /N 1 >>] \
    /B [/Separation /SpotB /DeviceCMYK 6 0 R] \
    /C [/Separation /SpotC /DeviceCMYK 7 0 R] \
    /D [/DeviceN [/S1 /S2 /S3 /S4 /S5] /DeviceCMYK 8 0 R] \
    /L [/Separation /All /DeviceCMYK << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [1 1 1 1] /N 1 >>] \
    >>";

/// A 100 x 80 pt page whose group blends in DeviceCMYK, with [`SPACES`],
/// `/X` as `x` and `/G` an isolated group form holding `form`.
fn ink_page(x: &str, content: &str, form: &str) -> Vec<u8> {
    grouped_page(x, content, form, "/I true")
}

/// [`ink_page`] with the form's group dictionary entries `group`.
fn grouped_page(x: &str, content: &str, form: &str, group: &str) -> Vec<u8> {
    let content = format!("{content}\n");
    let form = format!("{form}\n");
    let sampled = "00000000 0000FF00>";
    let ps = "{ dup 0 0 4 -1 roll 0.2 mul }";
    let dn = "{ pop }";
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] /Resources << {SPACES} /ExtGState << /X << {x} >> >> /XObject << /G 5 0 R >> >> >>"
            ),
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Group << /S /Transparency /CS /DeviceCMYK >> >>"
                .into(),
        ),
        (
            4,
            format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
        ),
        (
            5,
            format!(
                "<< /Type /XObject /Subtype /Form /BBox [0 0 100 80] /Group << /S /Transparency {group} >> /Resources << {SPACES} /ExtGState << /X << {x} >> >> >> /Length {} >>\nstream\n{form}endstream",
                form.len()
            ),
        ),
        (
            6,
            format!(
                "<< /FunctionType 0 /Domain [0 1] /Range [0 1 0 1 0 1 0 1] /Size [2] /BitsPerSample 8 /Filter /ASCIIHexDecode /Length {} >>\nstream\n{sampled}\nendstream",
                sampled.len() + 1
            ),
        ),
        (
            7,
            format!(
                "<< /FunctionType 4 /Domain [0 1] /Range [0 1 0 1 0 1 0 1] /Length {} >>\nstream\n{ps}\nendstream",
                ps.len() + 1
            ),
        ),
        (
            8,
            format!(
                "<< /FunctionType 4 /Domain [0 1 0 1 0 1 0 1 0 1] /Range [0 1 0 1 0 1 0 1] /Length {} >>\nstream\n{dn}\nendstream",
                dn.len() + 1
            ),
        ),
    ])
}

/// A lower square in `under`, then `/X` and an upper square in `over`.
fn pair(x: &str, under: &str, over: &str) -> Vec<u8> {
    ink_page(
        x,
        &format!("{under} 10 10 50 40 re f /X gs {over} 30 20 40 40 re f"),
        "",
    )
}

const OVERPRINT: &str = "/OP true /op true /OPM 1";
const SPOT_A: &str = "/A cs 1 scn";
const SPOT_A3: &str = "/A cs 0.3 scn";
const SPOT_B: &str = "/B cs 0.6 scn";
const SPOT_C: &str = "/C cs 0.7 scn";
const PROCESS: &str = "0 0.2 1 0 k";

/// Every resolvable case: a name and its fixture.
fn cases() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("spot-over-spot-overprint", pair(OVERPRINT, SPOT_A, SPOT_B)),
        (
            "spot-over-spot-overprint-alpha",
            pair("/OP true /op true /OPM 1 /ca 0.6", SPOT_C, SPOT_A),
        ),
        (
            "process-over-spot-overprint",
            pair(OVERPRINT, SPOT_A, "0.4 0 0 0 k"),
        ),
        (
            "spot-over-process-overprint",
            pair(OVERPRINT, PROCESS, SPOT_C),
        ),
        ("spot-over-process-alpha", pair("/ca 0.5", PROCESS, SPOT_A)),
        (
            "process-over-spot-multiply",
            pair("/BM /Multiply", SPOT_B, "0.9 0.3 0 0.1 k"),
        ),
        ("type-0-alpha", pair("/ca 0.6", SPOT_A, SPOT_B)),
        ("type-2-alpha", pair("/ca 0.6", SPOT_B, SPOT_A)),
        ("type-4-multiply", pair("/BM /Multiply", SPOT_B, SPOT_C)),
        (
            "spot-difference",
            pair("/BM /Difference", SPOT_A3, "/A cs 0.5 scn"),
        ),
        ("all-alpha", pair("/ca 0.5", SPOT_A, "/L cs 0.3 scn")),
        (
            "spot-in-isolated-group",
            ink_page(
                "/ca 0.5",
                &format!("{SPOT_A} 10 10 50 40 re f /X gs /G Do"),
                &format!("{SPOT_C} 30 20 40 40 re f /X gs {SPOT_B} 40 30 40 40 re f"),
            ),
        ),
        (
            "all-in-isolated-group",
            ink_page(
                "/ca 0.5",
                &format!("{SPOT_A} 10 10 50 40 re f /X gs /G Do"),
                &format!("{SPOT_B} 30 20 40 40 re f /L cs 0.4 scn 40 30 40 40 re f"),
            ),
        ),
    ]
}

/// The white-backed raster and the export with the switch as given.
fn export(bytes: &[u8], vector: bool) -> (Pixmap, EmfExport) {
    let doc = Document::from_bytes(bytes.to_vec()).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    let reference = render_page_with(
        &doc,
        &p,
        DPI / 72.0,
        &RenderOptions::default().with_backdrop(PageBackdrop::White),
    )
    .expect("render");
    let options = EmfOptions::default()
        .with_raster_dpi(DPI)
        .with_vector_transparency(vector);
    let emf = export_emf(&doc, &p, &RenderOptions::default(), &options).expect("emf export");
    assert_well_formed(&emf.emf);
    (reference.pixmap, emf)
}

#[test]
fn every_spot_case_is_written_as_regions_and_no_bitmap() {
    for (name, bytes) in cases() {
        let (_, on) = export(&bytes, true);
        let o = &on.outcome;
        let v = o.vectorised;
        assert!(v.ops > 0 && v.regions > 0, "{name}: {v:?}");
        assert_eq!(v.patched(), 0, "{name}: {v:?}");
        assert_eq!(o.patches.cases(), 0, "{name}: {:?}", o.patches);
        let records = walk_records(&on.emf).unwrap();
        assert_eq!(count(&records, 0x72), 0, "{name}: no EMR_ALPHABLEND");
    }
}

#[cfg(windows)]
#[test]
fn every_spot_case_plays_back_like_the_raster() {
    for (name, bytes) in cases() {
        let (reference, emf) = export(&bytes, true);
        let (bad, worst) =
            playback_against_raster(&format!("spot-{name}"), &reference, &emf.emf, ROUNDING);
        assert_eq!(bad, 0, "{name}: worst {worst}");
    }
}

#[test]
fn five_spot_inks_on_a_page_stay_a_patch_and_are_counted() {
    let bytes = pair("/ca 0.5", PROCESS, "/D cs 0.2 0.4 0.6 0.8 1 scn");
    let (_, on) = export(&bytes, true);
    let v = on.outcome.vectorised;
    assert_eq!((v.ops, v.patched_not_flat), (0, 1), "{v:?}");
    assert_eq!(v.not_flat.spot_roster, 1, "{v:?}");
}

#[test]
fn an_overflowing_page_patches_even_a_planed_spot() {
    // `/A` takes a plane first, but the page names six colorants: the
    // raster's per-buffer roster is not modelled, so `/A` is a patch too.
    let bytes = ink_page(
        "/ca 0.5",
        &format!(
            "{SPOT_A} 10 10 50 40 re f /X gs {SPOT_A3} 30 20 40 40 re f \
             /D cs 0.2 0.4 0.6 0.8 1 scn 90 70 5 5 re f"
        ),
        "",
    );
    let (_, on) = export(&bytes, true);
    let v = on.outcome.vectorised;
    assert_eq!(v.ops, 0, "{v:?}");
    assert!(v.not_flat.spot_roster >= 1, "{v:?}");
}

#[test]
fn process_paints_on_an_overflowing_page_are_still_vector() {
    // The overflow only touches paints that name a spot colorant.
    let bytes = ink_page(
        "/ca 0.5",
        &format!(
            "{PROCESS} 10 10 50 40 re f /X gs 0.4 0 0 0 k 30 20 40 40 re f \
             /D cs 0.2 0.4 0.6 0.8 1 scn 90 70 5 5 re f"
        ),
        "",
    );
    let (_, on) = export(&bytes, true);
    let v = on.outcome.vectorised;
    assert!(v.ops > 0 && v.regions > 0, "{v:?}");
}

#[test]
fn all_inside_a_non_isolated_group_stays_a_patch_and_is_counted() {
    let bytes = grouped_page(
        "/ca 0.5",
        &format!("{SPOT_A} 10 10 50 40 re f /X gs /G Do"),
        "/L cs 0.4 scn 30 20 40 40 re f",
        "/I false",
    );
    let (_, on) = export(&bytes, true);
    let v = on.outcome.vectorised;
    assert_eq!(v.not_flat.spot_all_group, 1, "{v:?}");
}

#[test]
fn with_the_switch_off_the_metafile_is_unchanged() {
    // Length and FNV-1a of the bytes the writer produced before this Pass,
    // measured on the parent commit.
    let pins = [
        (
            "spot-over-spot-overprint",
            pair(OVERPRINT, SPOT_A, SPOT_B),
            516,
            4_718_598_760_434_759_274_u64,
        ),
        (
            "all-alpha",
            pair("/ca 0.5", SPOT_A, "/L cs 0.3 scn"),
            26_116,
            17_250_285_755_542_759_906,
        ),
    ];
    for (name, bytes, len, hash) in pins {
        let (_, off) = export(&bytes, false);
        assert_eq!(off.outcome.vectorised, pdfcer_emf::EmfVectorised::default());
        assert_eq!((off.emf.len(), fnv1a(&off.emf)), (len, hash), "{name}");
    }
}
