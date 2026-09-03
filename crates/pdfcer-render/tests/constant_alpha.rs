//! # `/ca` and `/CA` constant alpha are honoured (ISO 32000-1 §11.6.4.4)
//!
//! Table 58's `/ca` (non-stroking) and `/CA` (stroking) set one alpha
//! applied to everything an operation paints. Until 2026-08-09 pdfcer read
//! neither: `apply_ext_gstate` fell through to a comment saying they were
//! "deferred", **with no counter**, and `solid()` hard-coded alpha 255.
//!
//! That made the omission invisible twice over — the page rendered fully
//! opaque, and nothing told the operator a transparency instruction had
//! been dropped. Found by the benign-bucket audit as a ~120-page cluster
//! across the veraPDF PDF/A transparency and colour-space suites, where a
//! fixture's own bookmark reads *"The ExtGState contains the /ca key with
//! value 0.5"*, pdfcer painted the glyph solid black, and pdfium and
//! Acrobat both painted it 50% grey.
//!
//! Every test here compares against a control that changes only the alpha,
//! because two identical no-ops compare equal — the failure mode that
//! would let a regression pass silently.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

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

/// A page painting `content` with `/GS0` set to `gs_dict`.
fn page(gs_dict: &str, content: &str) -> Vec<u8> {
    let stream = format!("{content}\n");
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            &format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 60 60] \
                 /Resources << /ExtGState << /GS0 {gs_dict} >> >> >>"
            ),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>"),
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

/// The centre pixel, demultiplied.
fn centre(r: &RenderedPage) -> (u8, u8, u8) {
    let pm = &r.pixmap;
    let px = pm
        .pixel(pm.width() / 2, pm.height() / 2)
        .expect("in bounds")
        .demultiply();
    (px.red(), px.green(), px.blue())
}

/// A black fill covering the page.
const FILL: &str = "/GS0 gs 0 g 0 0 60 60 re f";
/// A thick black stroke across the middle.
const STROKE: &str = "/GS0 gs 0 G 20 w 0 30 m 60 30 l S";

#[test]
fn a_half_opacity_fill_is_grey_not_black() {
    // The exact shape of the shipped defect: the fixture says 0.5 and
    // pdfcer painted solid black.
    let opaque = centre(&render(page("<< >>", FILL)));
    let half = centre(&render(page("<< /ca 0.5 >>", FILL)));

    assert_eq!(opaque, (0, 0, 0), "the control must be solid black");
    assert!(
        half.0 > 100 && half.0 < 155,
        "0.5 alpha over white should land near mid-grey, got {half:?}"
    );
    assert_eq!(half.0, half.1, "and stay neutral");
}

#[test]
fn lowercase_ca_is_fill_only_and_uppercase_ca_is_stroke_only() {
    // §11.6.4.4 keeps them separate, and a single `gs` may set either.
    // Conflating them would be an easy and invisible mistake — the page
    // still looks "transparent", just on the wrong operation.
    let fill_only = centre(&render(page("<< /ca 0.5 >>", STROKE)));
    assert_eq!(fill_only, (0, 0, 0), "/ca must not lighten a STROKE");

    let stroke_only = centre(&render(page("<< /CA 0.5 >>", FILL)));
    assert_eq!(stroke_only, (0, 0, 0), "/CA must not lighten a FILL");
}

#[test]
fn uppercase_ca_lightens_a_stroke() {
    let opaque = centre(&render(page("<< >>", STROKE)));
    let half = centre(&render(page("<< /CA 0.5 >>", STROKE)));
    assert_eq!(opaque, (0, 0, 0));
    assert!(
        half.0 > 100 && half.0 < 155,
        "a 0.5-alpha stroke should be mid-grey, got {half:?}"
    );
}

#[test]
fn fully_transparent_paints_nothing_and_fully_opaque_is_unchanged() {
    // The endpoints, which is where a clamp or an off-by-one shows up.
    assert_eq!(
        centre(&render(page("<< /ca 0 >>", FILL))),
        (255, 255, 255),
        "alpha 0 must leave the paper"
    );
    assert_eq!(
        centre(&render(page("<< /ca 1 >>", FILL))),
        centre(&render(page("<< >>", FILL))),
        "alpha 1 must be identical to no /ca at all"
    );
}

#[test]
fn q_and_q_restore_the_alpha() {
    // The reason these live on the graphics state rather than beside the
    // paint: §8.4.2 save/restore must cover them, and it does so for free
    // only if they are stored there.
    let restored = centre(&render(page(
        "<< /ca 0.5 >>",
        "q /GS0 gs Q 0 g 0 0 60 60 re f",
    )));
    assert_eq!(
        restored,
        (0, 0, 0),
        "an alpha set inside q/Q must not survive the Q"
    );
}
