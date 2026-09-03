//! # Clause-11 transparency: what composites, what does not, and what SAYS so
//!
//! **This file's premise changed one commit after it was written**, and it
//! is re-pointed rather than rewritten so the change stays legible. It was
//! called `transparency_is_disclosed` because NEITHER `/BM` (§11.3.5) nor
//! `/SMask` (§11.6.5) was implemented and the point was that pdfcer at least
//! said so. Blend modes now composite for real. Soft masks still do not.
//!
//! So the subjects here are now:
//!
//! - **Soft masks** — still unimplemented, still disclosed. Unchanged.
//! - **Unrecognised blend-mode names** — a name outside Tables 136/137 is
//!   composited as `Normal` and counted. Also unchanged in spirit; what
//!   changed is that "unrecognised" is now a much smaller set than
//!   "non-Normal".
//! - **The census/shortfall split** — `blend_modes_applied` counts modes
//!   pdfcer honoured, `blend_modes_ignored` counts names it did not know.
//!   Those were ONE counter an hour ago, and merging them again would make
//!   a real shortfall invisible inside an ordinary census.
//!
//! The numeric verification that pdfcer's blend modes match ISO 32000-1
//! Tables 136 and 137 lives in `blend_modes.rs`, not here. This file is
//! about DISCLOSURE; that one is about arithmetic.
//!
//! ## Why the two counters stay separate, which is the design
//!
//! Their failure directions are opposite and only one can expose content:
//!
//! - An ignored **blend mode** composites the same marks by the wrong
//!   rule. The page is not blank there, it is *wrong* there — and a
//!   Multiply that composited as Normal looks like a perfectly ordinary
//!   opaque overlay. Nobody notices.
//! - An ignored **soft mask** paints marks the document asked to be faded
//!   or masked away, so it paints MORE than was asked for. On a page whose
//!   design relies on a mask to hide something, that is the difference
//!   between a rendering artefact and showing what was meant to be hidden.
//!
//! ## What this gap actually costs, measured
//!
//! On the operator's print-conformance suite X-4 file, 2026-08-17:
//! **113 blend modes and 36 soft masks across six pages**, with page 2
//! alone accounting for 76 and 31. Page 2 had previously reported no
//! unsupported images, no unpainted patterns and no refused shadings — it
//! looked *clean*, and it was compositing wrongly the whole time. That
//! measurement was impossible to take before these counters existed, which
//! is the argument for disclosing a gap before implementing it: it
//! re-ordered the render queue.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

/// Assemble a classic single-page PDF with a correct xref table.
fn build(objects: &[(u32, &str)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
    buf.extend_from_slice(format!("xref\n0 {}\n", max_num + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..=max_num {
        match offsets.iter().find(|(n, _)| *n == num) {
            Some((_, off)) => buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => buf.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /ID [<0102> <0304>] >>\nstartxref\n{xref_at}\n%%EOF\n",
            max_num + 1
        )
        .as_bytes(),
    );
    buf
}

/// A page whose `/GS0` is `gs_dict`, painting a filled square through it.
fn page(gs_dict: &str) -> Vec<u8> {
    let content = "/GS0 gs 1 0 0 rg 10 10 40 40 re f";
    let stream = format!("{content}\n");
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 60 60] >>",
        ),
        (
            3,
            &format!(
                "<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
                 /Resources << /ExtGState << /GS0 {gs_dict} >> >> >>"
            ),
        ),
        (
            4,
            &format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ),
    ])
}

fn render(bytes: Vec<u8>) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    render_page_with(&doc, &p, 1.0, &RenderOptions::default()).expect("render")
}

/// A mode pdfcer implements is counted as APPLIED, not as ignored. Getting
/// this backwards would report a shortfall on a page pdfcer rendered
/// correctly, which is the fastest way to make a diagnostic worthless.
#[test]
fn an_implemented_blend_mode_is_counted_as_applied() {
    let r = render(page("<< /Type /ExtGState /BM /Multiply >>"));
    assert_eq!(r.diagnostics.blend_modes_applied, 1);
    assert_eq!(r.diagnostics.blend_modes_ignored, 0);
    assert_eq!(r.diagnostics.soft_masks_ignored, 0);
}

/// A name pdfcer does not apply — the shortfall case. It composites as
/// `Normal` and says so; it does NOT refuse to paint, because the marks
/// belong on the page and only the compositing rule is in doubt.
#[test]
fn an_unrecognised_blend_mode_name_is_counted_as_ignored() {
    let r = render(page("<< /Type /ExtGState /BM /NotARealMode >>"));
    assert_eq!(r.diagnostics.blend_modes_ignored, 1);
    assert_eq!(r.diagnostics.blend_modes_applied, 0);
    // The marks still landed.
    let p = r.pixmap.pixel(30, 30).expect("in bounds").demultiply();
    assert!(p.red() > 200, "an unknown /BM must not suppress the paint");
}

/// Table 58 allows `/BM` to be an ARRAY — "the first blend mode in the
/// array that the conforming reader supports". Reading only the name form
/// would miss every producer that writes the array, and miss it silently.
#[test]
fn a_blend_mode_given_as_an_array_is_honoured() {
    let r = render(page("<< /Type /ExtGState /BM [/Darken /Normal] >>"));
    assert_eq!(r.diagnostics.blend_modes_applied, 1);
    assert_eq!(r.diagnostics.blend_modes_ignored, 0);
}

/// `Normal` and `Compatible` are what pdfcer does anyway, so NEITHER
/// counter moves. Counting them in the census would put a large number on
/// ordinary documents — producers emit `/BM /Normal` constantly to reset
/// inherited state — and train every reader to ignore the counter, which is
/// how a real signal gets lost inside a true one.
#[test]
fn normal_and_compatible_move_neither_counter() {
    for gs in [
        "<< /Type /ExtGState /BM /Normal >>",
        "<< /Type /ExtGState /BM /Compatible >>",
    ] {
        let d = render(page(gs)).diagnostics;
        assert_eq!(d.blend_modes_applied, 0, "{gs}");
        assert_eq!(d.blend_modes_ignored, 0, "{gs}");
    }
}

#[test]
fn a_soft_mask_is_counted() {
    let r = render(page(
        "<< /Type /ExtGState /SMask << /S /Alpha /G 9 0 R >> >>",
    ));
    assert_eq!(r.diagnostics.soft_masks_ignored, 1);
    assert_eq!(r.diagnostics.blend_modes_ignored, 0);
}

/// `/SMask /None` is the RESET — it turns a soft mask OFF, which is
/// precisely pdfcer's behaviour already. Counting it would report a
/// shortfall on a page that asked for nothing pdfcer cannot do, and
/// producers emit `/SMask /None` constantly to clear inherited state.
#[test]
fn smask_none_is_not_counted() {
    let r = render(page("<< /Type /ExtGState /SMask /None >>"));
    assert_eq!(r.diagnostics.soft_masks_ignored, 0);
}

/// Both at once, and the marks still land: the disclosure is about how
/// they were composited, not about whether anything was drawn. A test that
/// only checked the counters would pass on a page that drew nothing.
/// Both gaps at once, and the marks still land: the disclosure is about
/// HOW they were composited, not about whether anything was drawn. A test
/// that only checked the counters would pass on a page that drew nothing.
///
/// `Multiply` is used rather than `Screen` deliberately. The first version
/// of this test used `/BM /Screen` and asserted the square was still RED —
/// which passed only because blend modes were being ignored. Screen of red
/// over a white page is WHITE, correctly, so implementing the feature broke
/// the assertion. Multiply of red over white is red, so the assertion
/// survives for the reason it was written (the paint happened) rather than
/// for the reason it originally passed (the blend did not).
#[test]
fn the_marks_are_still_painted_while_the_soft_mask_gap_is_disclosed() {
    let r = render(page(
        "<< /Type /ExtGState /BM /Multiply /SMask << /S /Luminosity /G 9 0 R >> >>",
    ));
    assert_eq!(r.diagnostics.blend_modes_applied, 1);
    assert_eq!(r.diagnostics.soft_masks_ignored, 1);
    let p = r.pixmap.pixel(30, 30).expect("in bounds").demultiply();
    assert!(
        p.red() > 200 && p.green() < 55 && p.blue() < 55,
        "Multiply of red over a white page is red, got ({}, {}, {})",
        p.red(),
        p.green(),
        p.blue()
    );
}

/// **The blend actually changes pixels**, which no counter can tell you —
/// and it blends against the RIGHT backdrop, which is the harder half.
///
/// ★ The first version of this test asserted that `Screen` over the page
/// came out WHITE, and it passed. It was asserting a BUG. §11.4.7 makes the
/// page an *isolated* transparency group whose initial backdrop is fully
/// TRANSPARENT — white is composited once at the end — and §11.4.5 says
/// blend modes inside a group "shall not be influenced by the group's
/// backdrop". pdfcer was filling the buffer opaque white and handing every
/// blend function `cb = 1.0`, which is harmless only for the four modes
/// satisfying `B(1.0, cs) = cs` (`Normal`, `Compatible`, `Multiply`,
/// `Darken`) and wrong for the other eleven.
///
/// So the honest proof needs a real backdrop object underneath. Blue, then
/// red screened over it: `Screen(cb, cs) = cb + cs − cb·cs`, componentwise
/// `(0,0,1)` with `(1,0,0)` gives `(1,0,1)` — magenta. That value can only
/// arise if the blend ran AND saw blue rather than white.
#[test]
fn screen_blends_against_the_object_beneath_not_against_the_paper() {
    // Blue square painted Normal, then a red square screened over it.
    let content = "0 0 1 rg 10 10 40 40 re f /GS0 gs 1 0 0 rg 10 10 40 40 re f";
    let stream = format!(
        "{content}
"
    );
    let bytes = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 60 60] >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources              << /ExtGState << /GS0 << /Type /ExtGState /BM /Screen >> >> >> >>",
        ),
        (
            4,
            &format!(
                "<< /Length {} >>
stream
{stream}endstream",
                stream.len()
            ),
        ),
    ]);
    let r = render(bytes);
    let p = r.pixmap.pixel(30, 30).expect("in bounds").demultiply();
    assert!(
        p.red() > 250 && p.green() < 5 && p.blue() > 250,
        "Screen of red over blue is magenta, got ({}, {}, {}) — (255,0,0) means the blend never ran, (255,255,255) means it ran against the paper instead of against the blue square",
        p.red(),
        p.green(),
        p.blue()
    );
}

/// The page group's initial backdrop is TRANSPARENT (§11.4.7), so the FIRST
/// object painted at a pixel is unblended whatever the mode says — there is
/// nothing to blend with yet. Pinned because it is the exact behaviour the
/// old white-fill got wrong, and because it looks like a bug until you read
/// the clause.
#[test]
fn the_first_object_at_a_pixel_is_unblended_because_the_page_starts_transparent() {
    let r = render(page("<< /Type /ExtGState /BM /Screen >>"));
    let p = r.pixmap.pixel(30, 30).expect("in bounds").demultiply();
    assert!(
        p.red() > 250 && p.green() < 5 && p.blue() < 5,
        "an isolated group's first object survives its own blend mode, got ({}, {}, {}) — white here means the buffer was pre-filled",
        p.red(),
        p.green(),
        p.blue()
    );
    // And the paper still arrives: outside the square is white, not
    // transparent, because the group is flattened over white at the end.
    let bg = r.pixmap.pixel(2, 2).expect("in bounds").demultiply();
    assert_eq!(
        (bg.red(), bg.green(), bg.blue(), bg.alpha()),
        (255, 255, 255, 255),
        "uncovered page must be opaque white after flattening"
    );
}

// -- §11.4.7 transparency groups -------------------------------------------

/// A page invoking form `/Fm0`, whose stream dictionary carries `extra`.
fn page_with_form(extra: &str) -> Vec<u8> {
    let content = "/Fm0 Do";
    let stream = format!("{content}\n");
    let form_body = "1 0 0 rg 10 10 40 40 re f";
    let form = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 60 60] {extra} /Length {} >>\n\
         stream\n{form_body}\nendstream",
        form_body.len()
    );
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 60 60] >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
             /Resources << /XObject << /Fm0 5 0 R >> >> >>",
        ),
        (
            4,
            &format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ),
        (5, &form),
    ])
}

/// **The finding these counters exist for, and it is now fixed.** A form
/// carrying `/Group << /S /Transparency >>` is a COMPOSITING SCOPE
/// (§11.4.7, Table 96): its contents belong in their own buffer, whose
/// RESULT is then composited with the blend mode, constant alpha and soft
/// mask in force at the `Do` (§11.4.5).
///
/// pdfcer used to paint the contents straight onto the page, applying those
/// to each object INSIDE instead. That was invisible until it was counted,
/// and the way it surfaced is the whole argument for counting a gap before
/// closing it: the suite X-4 file's blend-mode panel still showed the
/// suite's failure crosses AFTER blend modes were implemented and verified
/// correct both in isolation and against a coloured backdrop, while every
/// blend-mode counter looked healthy. The page carried 148 form XObjects
/// and `/Group` was never read.
///
/// With group compositing in, that panel renders clean — the crosses are
/// gone and the swatches match the suite's own reference sheet.
#[test]
fn a_transparency_group_is_composited_as_a_unit() {
    let r = render(page_with_form("/Group << /S /Transparency >>"));
    assert_eq!(r.diagnostics.transparency_groups_composited, 1);
    assert_eq!(
        r.diagnostics.transparency_groups_flattened, 0,
        "flattening is now the FALLBACK, taken only if the buffer cannot \
         be allocated"
    );
    let p = r.pixmap.pixel(30, 30).expect("in bounds").demultiply();
    assert!(p.red() > 200, "the group's contents are still painted");
}

/// **The composite is what carries the outer blend mode**, and this is the
/// assertion that separates a real group implementation from a counter that
/// merely says "group".
///
/// A blue square is painted, then a form containing a red square is invoked
/// under `/BM /Screen`. If the group is composited as a unit, the outer
/// Screen applies once to the group's RESULT: `Screen(blue, red)` is
/// magenta. If the group were flattened, the red square would be screened
/// against the blue directly — which happens to give the same colour here,
/// so the distinguishing half is the ALPHA: a flattened group applies the
/// outer constant alpha to each object inside as well.
#[test]
fn the_outer_blend_mode_applies_to_the_groups_result() {
    let content = "0 0 1 rg 10 10 40 40 re f /GS0 gs /Fm0 Do";
    let stream = format!("{content}\n");
    let form_body = "1 0 0 rg 10 10 40 40 re f";
    let form = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 60 60] \
         /Group << /S /Transparency >> /Length {} >>\nstream\n{form_body}\nendstream",
        form_body.len()
    );
    let bytes = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 60 60] >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources \
             << /XObject << /Fm0 5 0 R >> \
                /ExtGState << /GS0 << /Type /ExtGState /BM /Screen >> >> >> >>",
        ),
        (
            4,
            &format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ),
        (5, &form),
    ]);
    let r = render(bytes);
    assert_eq!(r.diagnostics.transparency_groups_composited, 1);
    let p = r.pixmap.pixel(30, 30).expect("in bounds").demultiply();
    assert!(
        p.red() > 250 && p.green() < 5 && p.blue() > 250,
        "Screen of the group's red result over blue is magenta, got \
         ({}, {}, {})",
        p.red(),
        p.green(),
        p.blue()
    );
}

/// The blend mode in force at the `Do` must NOT also apply to each object
/// inside the group — §11.4.5 says it applies to the group's result. So the
/// group's contents start at `Normal` however the outer state is set.
///
/// Without the reset, a group's first object would be blended once on the
/// way in and again on the way out. With a single opaque object the double
/// application is invisible for `Multiply` (idempotent against white) and
/// very visible for `Screen`, which is why the fixture uses two objects and
/// checks the one UNDERNEATH.
#[test]
fn the_outer_blend_mode_does_not_leak_into_the_groups_contents() {
    let content = "/GS0 gs /Fm0 Do";
    let stream = format!("{content}\n");
    // Inside the group: blue, then red over it, both Normal. Red must win.
    let form_body = "0 0 1 rg 10 10 40 40 re f 1 0 0 rg 10 10 40 40 re f";
    let form = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 60 60] \
         /Group << /S /Transparency >> /Length {} >>\nstream\n{form_body}\nendstream",
        form_body.len()
    );
    let bytes = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 60 60] >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources \
             << /XObject << /Fm0 5 0 R >> \
                /ExtGState << /GS0 << /Type /ExtGState /BM /Multiply >> >> >> >>",
        ),
        (
            4,
            &format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ),
        (5, &form),
    ]);
    let r = render(bytes);
    let p = r.pixmap.pixel(30, 30).expect("in bounds").demultiply();
    assert!(
        p.red() > 250 && p.green() < 5 && p.blue() < 5,
        "inside the group the two fills are Normal, so red covers blue; \
         Multiply then applies once to the result over white paper, \
         leaving red. Got ({}, {}, {})",
        p.red(),
        p.green(),
        p.blue()
    );
}

/// `/I` (isolated) and `/K` (knockout) are counted, and
/// `knockout_approximated` is now **zero** for a knockout group pdfcer
/// rendered exactly.
///
/// # ★ THIS COUNTER CHANGED MEANING IN `Pass 97.0`, and the previous
/// # expectation is kept here so the change reads as deliberate
///
/// It used to be `1` for every `/K true` group, because the whole feature
/// was an approximation: pdfcer composited a knockout group as an ordinary
/// one, which gets its outer boundary right and its internal occlusion
/// order wrong. §11.4.6 now has a real implementation
/// (`crate::canvas::KnockoutTarget`), so the counter names something
/// narrower and more useful — the ELEMENTS inside a knockout group that
/// could not be given knockout semantics, because they read the
/// destination back (a shading, an overprint composite, a per-paint
/// non-separable blend).
///
/// A silently redefined counter is worse than a renamed one: an operator
/// diffing two runs would see the number fall to zero and read it as an
/// improvement in the wrong thing. The rustdoc on the field says so, and
/// this test is the executable half of that statement.
#[test]
fn isolated_and_knockout_groups_are_counted_separately() {
    for extra in [
        "/Group << /S /Transparency /I true >>",
        "/Group << /S /Transparency /K true >>",
        "/Group << /S /Transparency /I true /K true >>",
    ] {
        let d = render(page_with_form(extra)).diagnostics;
        assert_eq!(d.transparency_groups_composited, 1, "{extra}");
        assert_eq!(d.transparency_groups_special, 1, "{extra}");
        assert_eq!(
            d.transparency_groups_knockout_approximated, 0,
            "a knockout group of plain fills is rendered exactly, so nothing \\
             is approximated: {extra}"
        );
    }
}

/// ★ **§11.4.6, measured — and the fixture sets `/ca 0.5` for the reason
/// the clause itself gives.**
///
/// Knockout and non-knockout are **identical** when every element is
/// opaque: `q_s = 1` makes `α_s = f_s`, and §11.4.8's `(1 − f_si)`
/// destination scale collapses onto §11.4.4's `(1 − α_si)` term for term.
/// So a fixture built from opaque fills passes under the correct
/// implementation *and* under the collapsed one, and proves nothing. The
/// corpus states this as a warning in so many words
/// (`iso32000__s__11.4.md` §6.5): *"A fixture built from opaque fills
/// cannot distinguish a correct knockout implementation from a wrong one.
/// Build the test with `/ca < 1`."*
///
/// The fixture: two black fills at `/ca 0.5`, exactly overlapping, inside
/// one group over white paper.
///
/// | group | what the second fill does | grey level |
/// |---|---|---|
/// | non-knockout | layers over the first ⇒ `1 − 0.5²` | **25 %** |
/// | knockout | knocks the first out ⇒ `1 − 0.5` | **50 %** |
///
/// A 64-level gap, and it is the whole visible content of the feature.
#[test]
fn knockout_erases_an_earlier_element_where_a_normal_group_layers() {
    fn grey(extra: &str) -> u8 {
        let content = "/Fm0 Do";
        let stream = format!("{content}\n");
        // Two identical half-opaque black squares. `/GS0` carries /ca 0.5,
        // set INSIDE the group so it is the element's own opacity rather
        // than the group's.
        let form_body = "/GS0 gs 0 g 10 10 40 40 re f 0 g 10 10 40 40 re f";
        let form = format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 60 60] {extra} \
             /Resources << /ExtGState << /GS0 << /Type /ExtGState /ca 0.5 >> >> >> \
             /Length {} >>\nstream\n{form_body}\nendstream",
            form_body.len()
        );
        let bytes = build(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 60 60] >>",
            ),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
                 /Resources << /XObject << /Fm0 5 0 R >> >> >>",
            ),
            (
                4,
                &format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
            ),
            (5, &form),
        ]);
        render(bytes)
            .pixmap
            .pixel(30, 30)
            .expect("in bounds")
            .demultiply()
            .red()
    }

    let normal = grey("/Group << /S /Transparency >>");
    let knocked = grey("/Group << /S /Transparency /K true >>");
    assert!(
        (normal as i32 - 64).abs() <= 3,
        "two half-opaque blacks layering reach 25% grey (~64), got {normal}"
    );
    assert!(
        (knocked as i32 - 128).abs() <= 3,
        "in a knockout group the second fill REPLACES the first, so the \\
         result stays at 50% grey (~128), got {knocked}"
    );
}

/// ★ **§11.4.5, measured — a soft mask applies to the group's RESULT, once,
/// not to each object inside it.**
///
/// # Why the fixture is two OVERLAPPING squares and not one
///
/// Because one square cannot tell the two models apart. A mask folded into
/// the clip multiplies each object's coverage; a mask applied to the result
/// multiplies the composite's alpha. With a single object those are the
/// same number. They diverge exactly where two objects overlap, and they
/// diverge by the mask value squared:
///
/// | model | first square | overlap |
/// |---|---|---|
/// | folded into the contents' clip (what pdfcer did) | `1 − M` | `(1 − M)²` |
/// | applied to the group result (§11.4.5) | `1 − M` | `1 − M` |
///
/// At `M = 0.5`, black on white: **128 either way outside the overlap, and
/// 64 versus 128 inside it.** So the assertion is taken inside the overlap
/// and nowhere else.
///
/// # Why the mask is a `/Luminosity` group painting a flat grey
///
/// Because a flat mask makes the arithmetic checkable by hand. §11.5.3's
/// device-space luminosity is `0.30 R + 0.59 G + 0.11 B` with **no** gamma
/// compensation, and those coefficients sum to 1, so a neutral `0.5 g`
/// gives a mask of exactly 0.5 whatever the coefficients are — which means
/// this test cannot fail for a reason it is not about.
#[test]
fn a_soft_mask_applies_to_the_group_result_not_to_each_object_inside_it() {
    // The mask group: flat 50% grey over the whole page.
    let mask_body = "0.5 g 0 0 60 60 re f";
    let mask_form = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 60 60] \
         /Group << /S /Transparency /CS /DeviceGray >> /Length {} >>\n\
         stream\n{mask_body}\nendstream",
        mask_body.len()
    );
    // The masked group: two black squares overlapping in the middle.
    let form_body = "0 g 5 5 30 30 re f 0 g 25 25 30 30 re f";
    let form = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 60 60] \
         /Group << /S /Transparency >> /Length {} >>\n\
         stream\n{form_body}\nendstream",
        form_body.len()
    );
    let content = "/GS0 gs /Fm0 Do";
    let stream = format!("{content}\n");
    let bytes = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 60 60] >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources \
             << /XObject << /Fm0 5 0 R >> \
                /ExtGState << /GS0 << /Type /ExtGState /SMask \
                  << /S /Luminosity /G 6 0 R >> >> >> >> >>",
        ),
        (
            4,
            &format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ),
        (5, &form),
        (6, &mask_form),
    ]);
    let r = render(bytes);
    assert_eq!(
        r.diagnostics.soft_masks_on_group_result, 1,
        "the mask must have been lifted out of the contents' clip"
    );
    assert_eq!(
        r.diagnostics.soft_masks_reset_stale, 0,
        "nothing here establishes a clip between the gs and the Do"
    );

    // (30, 30) in PDF space is inside BOTH squares; device y is flipped,
    // so the same device pixel is inside both as well.
    let overlap = r.pixmap.pixel(30, 30).expect("in bounds").demultiply();
    // (10, 50) device = (10, 10) PDF: inside the first square only.
    let single = r.pixmap.pixel(10, 50).expect("in bounds").demultiply();

    assert!(
        (i32::from(single.red()) - 128).abs() <= 4,
        "one black square under a 50% mask over white is mid grey, got {}",
        single.red()
    );
    assert!(
        (i32::from(overlap.red()) - 128).abs() <= 4,
        "the OVERLAP must be the same mid grey: the mask applies once, to the \
         group's result. Folding it into the contents' clip squares it and \
         gives ~64. Got {}",
        overlap.red()
    );
}

/// ★ **§11.3.4 / Table 147 — the blending colour space is INHERITED by a
/// non-isolated group and CHOSEN by an isolated one, and the difference
/// is what makes the whole suite transparency panel subtractive.**
///
/// Table 147's `/CS` row: *"if the group is non-isolated, `CS` shall be
/// ignored and the colour space shall be inherited from the group's
/// parent"*. ISO 32000-2 §11.6.6 says it from the other side —
/// *"non-isolated groups shall inherit their colour space from the
/// nearest ancestor isolated parent group"* — and gives the reason:
/// converting the backdrop into another space is not always possible, and
/// would be an excessive number of conversions where it is.
///
/// So a `DeviceCMYK` **page** group makes every non-isolated group on the
/// page subtractive whatever those groups declare, and that is exactly the
/// shape of `PCS3_161`: `ICCBased` RGB artwork, `DeviceCMYK` page group,
/// and all fifteen of its blends computed on the wrong side of §11.3.4.
///
/// Four cases in one test because the interesting ones are the pairs:
/// declaring RGB while non-isolated must **not** escape a CMYK page, and
/// declaring CMYK while isolated must **not** need a CMYK page.
#[test]
fn the_blending_space_is_inherited_unless_the_group_is_isolated() {
    fn subtractive_count(page_cs: &str, group_extra: &str) -> usize {
        // One `/BM /Multiply` inside the group, so `blends_in_wrong_space`
        // has something to count as well.
        let form_body = "/GS1 gs 0 g 10 10 30 30 re f";
        let form = format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 60 60] {group_extra} \
             /Resources << /ExtGState << /GS1 << /Type /ExtGState /BM /Multiply >> >> >> \
             /Length {} >>\nstream\n{form_body}\nendstream",
            form_body.len()
        );
        let content = "/Fm0 Do";
        let stream = format!("{content}\n");
        let bytes = build(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 60 60] >>",
            ),
            (
                3,
                &format!(
                    "<< /Type /Page /Parent 2 0 R /Contents 4 0 R {page_cs} \
                     /Resources << /XObject << /Fm0 5 0 R >> >> >>"
                ),
            ),
            (
                4,
                &format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
            ),
            (5, &form),
        ]);
        render(bytes).diagnostics.blend_space_subtractive
    }

    // An RGB page and an RGB group: nothing subtractive anywhere.
    assert_eq!(
        subtractive_count(
            "/Group << /S /Transparency /CS /DeviceRGB >>",
            "/Group << /S /Transparency /I true /CS /DeviceRGB >>"
        ),
        0
    );
    // A CMYK page: the page counts, and the NON-isolated group inherits it
    // — even though it declares RGB, which Table 147 says to ignore.
    assert_eq!(
        subtractive_count(
            "/Group << /S /Transparency /CS /DeviceCMYK >>",
            "/Group << /S /Transparency /CS /DeviceRGB >>"
        ),
        2,
        "a non-isolated group inherits the page's CMYK space and may not \
         escape it by declaring one of its own — Table 147's /CS row"
    );
    // The same CMYK page with an ISOLATED group declaring RGB: the group
    // genuinely escapes, so only the page counts.
    assert_eq!(
        subtractive_count(
            "/Group << /S /Transparency /CS /DeviceCMYK >>",
            "/Group << /S /Transparency /I true /CS /DeviceRGB >>"
        ),
        1,
        "an ISOLATED group's /CS is honoured, so it leaves the page's space"
    );
    // And an RGB page with an isolated CMYK group: the group alone.
    assert_eq!(
        subtractive_count(
            "/Group << /S /Transparency /CS /DeviceRGB >>",
            "/Group << /S /Transparency /I true /CS /DeviceCMYK >>"
        ),
        1
    );
}

/// `Normal` inside a subtractive space is **not** a §11.3.4 violation, and
/// this test is the reason the two counters are separate.
///
/// The complement is applied to the **blend function**, and `Normal` is
/// `c_s` on either side of it: `1 − (1 − c_s) = c_s`. So a page can be
/// entirely `DeviceCMYK`, composite hundreds of objects, and be entirely
/// correct. Counting the space alone would report every CMYK print file in
/// the world as broken.
#[test]
fn a_cmyk_page_that_only_composites_normal_is_not_counted_as_wrong() {
    let content = "0 0 0 1 k 10 10 40 40 re f 0 1 0 0 k 20 20 30 30 re f";
    let stream = format!("{content}\n");
    let bytes = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 60 60] >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
             /Group << /S /Transparency /CS /DeviceCMYK >> /Resources << >> >>",
        ),
        (
            4,
            &format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ),
    ]);
    let d = render(bytes).diagnostics;
    assert_eq!(
        d.blend_space_subtractive, 1,
        "the page group's own space is a DeviceCMYK one and is counted"
    );
    assert_eq!(
        d.blends_in_wrong_space, 0,
        "…but nothing here blends, so nothing is wrong. A page can be \
         entirely CMYK and entirely correct."
    );
}

/// **The Pass's headline claim, end to end**: a page whose group declares
/// `DeviceCMYK` is composited in ink, and the `Difference` cell that
/// motivated the whole build lands on the value §11.3.4 requires.
///
/// # Why this fixture is the suite cell and not a synthetic one
///
/// Because the arithmetic is already pinned by `compositor.rs`'s unit
/// test, and pinning it twice proves nothing. What is unproven until this
/// test runs is that a real content stream, walked by the real
/// interpreter, through the real canvas, reaches that arithmetic — which is
/// exactly the failure mode `Pass 28.0`'s `move_subpath` had for eight
/// Passes: correct, callable and never called.
///
/// Magenta `0 1 0 0 k` under black `0 0 0 1 k` with `/BM /Difference`
/// gives `1 − |c_b′ − c_s′|` = `DeviceCMYK 1 0 1 0`. Rendered additively
/// pdfcer produced `(237, 1, 140)`; the assertion below is that it no
/// longer does.
#[test]
fn a_subtractive_page_group_composites_the_difference_cell_in_ink() {
    let content = "0 0 0 1 k 0 0 20 20 re f /GS0 gs 0 1 0 0 k 0 0 20 20 re f";
    let stream = format!(
        "{content}
"
    );
    let bytes = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 20 20] >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R              /Group << /S /Transparency /CS /DeviceCMYK >>              /Resources << /ExtGState << /GS0 5 0 R >> >> >>",
        ),
        (
            4,
            &format!(
                "<< /Length {} >>
stream
{stream}endstream",
                stream.len()
            ),
        ),
        (5, "<< /Type /ExtGState /BM /Difference >>"),
    ]);
    let r = render(bytes);
    assert!(
        r.diagnostics.cmyk_buffer_engaged,
        "a DeviceCMYK page group must engage the colorant buffer"
    );
    assert_eq!(
        r.diagnostics.blends_in_wrong_space, 0,
        "the blend ran in the space the page declared, so nothing is owed"
    );
    assert_eq!(
        r.diagnostics.cmyk_groups_approximated, 0,
        "no groups here beyond the page group itself"
    );

    // `DeviceCMYK 1 0 1 0` is the answer; compare against the SAME
    // conversion the renderer collapses through rather than a hard-coded
    // RGB triple, so that a future re-calibration of the CMYK table moves
    // the fixture and the renderer together instead of breaking this test
    // for a reason that has nothing to do with compositing.
    let want = pdfcer_core::color::cmyk_to_srgb_with(
        pdfcer_core::settings::CmykIntent::default(),
        1.0,
        0.0,
        1.0,
        0.0,
    );
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let px = r.pixmap.pixels()[(10 * 20 + 10) as usize];
    for (got, want) in [px.red(), px.green(), px.blue()]
        .iter()
        .zip([q(want[0]), q(want[1]), q(want[2])].iter())
    {
        assert!(
            got.abs_diff(*want) <= 1,
            "expected the subtractive Difference result, got ({}, {}, {})",
            px.red(),
            px.green(),
            px.blue()
        );
    }
}

/// The ceiling is the OPERATOR'S, and the public predicate does not lie
/// about where it falls (`Pass 132.0`).
///
/// # Why this is one test and not three
///
/// Three claims are only worth anything together:
///
/// 1. A ceiling too small for the raster **refuses** — the same disclosed
///    fallback a huge page has always taken, now reachable deliberately.
/// 2. Raising the ceiling **engages** it again at the same raster size, so
///    the setting is a real permission and not a decoration.
/// 3. [`pdfcer_render::will_composite_in_cmyk`] **predicted both**.
///
/// The third is the one the request from the shell was actually about. A
/// caller sizing a raster has to decide BEFORE rendering whether the
/// colours it gets back will be the exact ones, and it cannot do that by
/// looking at `cmyk_buffer_refused` afterwards. If the predicate and the
/// renderer ever disagree, the shell's tier choice is wrong in exactly the
/// band it was written to fix — silently, because both answers still look
/// like a page.
#[test]
fn the_operator_s_ceiling_decides_the_path_and_the_predicate_agrees() {
    let content = "0 0 0 1 k 0 0 20 20 re f /GS0 gs 0 1 0 0 k 0 0 20 20 re f";
    let stream = format!(
        "{content}
"
    );
    let bytes = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 20 20] >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R              /Group << /S /Transparency /CS /DeviceCMYK >>              /Resources << /ExtGState << /GS0 5 0 R >> >> >>",
        ),
        (
            4,
            &format!(
                "<< /Length {} >>
stream
{stream}endstream",
                stream.len()
            ),
        ),
        (5, "<< /Type /ExtGState /BM /Difference >>"),
    ]);
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);

    // The raster is 20x20 = 400 px, so 8,000 bytes of colorant buffer.
    // Both ceilings below are chosen against that number rather than
    // against a round one, so the test says WHY it expects each answer.
    for (ceiling, expect_ink) in [
        (Some(4_000_usize), false),
        (Some(8_000), true),
        (None, true),
        (Some(0), false),
    ] {
        let options = RenderOptions::default().with_max_cmyk_buffer_bytes(ceiling);
        let r = render_page_with(&doc, &p, 1.0, &options).expect("render");
        assert_eq!(
            r.diagnostics.cmyk_buffer_engaged,
            expect_ink,
            "ceiling {ceiling:?} should {} the colorant buffer",
            if expect_ink { "engage" } else { "refuse" }
        );
        assert_eq!(
            r.diagnostics.cmyk_buffer_refused,
            usize::from(!expect_ink),
            "a refusal must be DISCLOSED, never silent"
        );
        assert_eq!(
            pdfcer_render::will_composite_in_cmyk(20, 20, ceiling),
            expect_ink,
            "the public predicate disagreed with the renderer at {ceiling:?} —              a caller sizing a raster would be told the wrong thing"
        );
    }
}

/// The other half of the switch, and the one that protects every ordinary
/// document: a page with **no** subtractive group keeps the sRGB path.
///
/// ISO 32000-1 §8.6.6.4 makes that the *specified* behaviour on an
/// additive device rather than merely the safe one, so a regression here
/// would be a conformance failure in the opposite direction — and it would
/// be invisible, because the picture would still look like a page.
#[test]
fn an_additive_page_does_not_engage_the_colorant_buffer() {
    let r = render(page("<< /Type /ExtGState /BM /Multiply >>"));
    assert!(!r.diagnostics.cmyk_buffer_engaged);
    assert_eq!(r.diagnostics.cmyk_bridged_pixels, 0);
}

/// A form with NO `/Group` is an ordinary reusable content stream and is
/// not a compositing scope. Counting it would put a large number on
/// ordinary documents — forms are how every producer factors repeated
/// content — and bury the real signal.
#[test]
fn a_plain_form_xobject_is_not_a_transparency_group() {
    let d = render(page_with_form("")).diagnostics;
    assert_eq!(d.transparency_groups_composited, 0);
    assert_eq!(d.transparency_groups_flattened, 0);
    assert_eq!(d.transparency_groups_special, 0);
}

/// `/Group` exists for more than transparency — Table 95 allows other
/// subtypes, and only `/S /Transparency` makes a compositing scope.
#[test]
fn a_group_that_is_not_a_transparency_group_is_not_counted() {
    let d = render(page_with_form("/Group << /S /SomethingElse >>")).diagnostics;
    assert_eq!(d.transparency_groups_composited, 0);
    assert_eq!(d.transparency_groups_flattened, 0);
}

/// The FOUR NON-SEPARABLE modes are APPLIED, and applied by **pdfcer's own**
/// Table 137 rather than by the rasteriser.
///
/// ★ THIS TEST USED TO ASSERT THE OPPOSITE, and the history is the point.
/// It was `the_non_separable_modes_are_refused_not_silently_wrong`, and it
/// pinned decision 066's refusal: `tiny_skia` HAS these four modes, mapping
/// to them is one line, and they are wrong by up to 107/255 on 9.4–15.5 % of
/// colour pairs (its `clip_color` gates the low-gamut rescale on `mx >= 0`
/// where the standard uses `mn < 0`, leaving the branch dead).
///
/// **Decision 066 refused TRUSTING the dependency. It never refused the
/// feature**, and the modes are now computed in `pdfcer_render::blend_nonsep`
/// against the clause. So the assertion flips — but *what it protects does
/// not*: these must never be **silently wrong**, and "applied" is only an
/// improvement on "refused" if the thing applied is right.
///
/// That second half is why this test checks a counter and
/// `tests/nonseparable_blend_differential.rs` checks the pixels: a counter
/// saying `applied` while the composite is wrong is precisely the outcome
/// decision 066 exists to prevent, and it is exactly what the old
/// one-line-mapping would have produced.
#[test]
fn the_non_separable_modes_are_applied_by_pdfces_own_table_137() {
    for name in ["Hue", "Saturation", "Color", "Luminosity"] {
        let d = render(page(&format!("<< /Type /ExtGState /BM /{name} >>"))).diagnostics;
        assert_eq!(
            d.blend_modes_applied, 1,
            "/BM /{name} must be APPLIED now that pdfcer computes Table 137"
        );
        assert_eq!(
            d.blend_modes_ignored, 0,
            "/BM /{name} must no longer land in the shortfall counter"
        );
    }
}

/// **Isolated and non-isolated groups differ, and this is the fixture that
/// can tell them apart.**
///
/// The page paints blue. A group then paints red with `/BM /Screen` INSIDE
/// it. What the red screens against depends entirely on `/I` (Table 96):
///
/// - `/I true` (isolated): the group's initial backdrop is TRANSPARENT, so
///   the red has nothing to blend with and stays red. The group's result is
///   then composited over the blue normally — still red.
/// - `/I false` (the DEFAULT, non-isolated): the group's initial backdrop
///   is the page, so the red screens against BLUE and comes out magenta.
///
/// pdfcer buffers a group only when buffering changes the answer — a
/// non-isolated group under a neutral outer state is painted inline, which
/// IS the non-isolated semantics rather than an approximation of them. That
/// optimisation and this correctness property are the same decision, so
/// this test guards both: break the buffering condition in either direction
/// and one of these two assertions fails.
#[test]
fn an_isolated_group_blends_against_transparency_a_non_isolated_one_against_the_page() {
    let render_with = |iso: &str| {
        let content = "0 0 1 rg 10 10 40 40 re f /Fm0 Do";
        let stream = format!(
            "{content}
"
        );
        let form_body = "/GS0 gs 1 0 0 rg 10 10 40 40 re f";
        let form = format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 60 60]              /Group << /S /Transparency {iso} >> /Resources              << /ExtGState << /GS0 << /Type /ExtGState /BM /Screen >> >> >>              /Length {} >>
stream
{form_body}
endstream",
            form_body.len()
        );
        let bytes = build(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 60 60] >>",
            ),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources                  << /XObject << /Fm0 5 0 R >> >> >>",
            ),
            (
                4,
                &format!(
                    "<< /Length {} >>
stream
{stream}endstream",
                    stream.len()
                ),
            ),
            (5, &form),
        ]);
        let r = render(bytes);
        let p = r.pixmap.pixel(30, 30).expect("in bounds").demultiply();
        (p.red(), p.green(), p.blue())
    };

    let (ir, ig, ib) = render_with("/I true");
    assert!(
        ir > 250 && ig < 5 && ib < 5,
        "an ISOLATED group's contents see a transparent backdrop, so the red survives its own Screen: expected red, got ({ir}, {ig}, {ib})"
    );

    let (nr, ng, nb) = render_with("");
    assert!(
        nr > 250 && ng < 5 && nb > 250,
        "a NON-isolated group's contents see the page, so the red screens against blue: expected magenta, got ({nr}, {ng}, {nb})"
    );
}
