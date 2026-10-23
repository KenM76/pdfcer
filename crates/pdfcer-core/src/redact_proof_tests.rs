#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;
use crate::annot_author::{Quad, RedactSpec};
use crate::edit::EditSession;
use crate::optimize::tests::build;
use crate::page_tree::Rect;
use crate::redact::apply_redactions;
use crate::space_audit::tests::stream;
use crate::vartext::Quadding;
use crate::writer::SaveOptions;

const PAGE: &str = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] \
                    /Resources << /Font << /F1 5 0 R >> /XObject << /X 6 0 R >> >> /Contents 4 0 R >>";
const FONT: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";

/// 1 catalog, 2 pages, 3 page drawing `content` (4), 5 Helvetica, then
/// `extra` from 6.
fn pdf(content: &str, extra: &[Vec<u8>]) -> Vec<u8> {
    let mut bodies = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        PAGE.as_bytes().to_vec(),
        stream("", content.as_bytes()),
        FONT.as_bytes().to_vec(),
    ];
    bodies.extend(extra.iter().cloned());
    build(&bodies)
}

fn drawing(text: &str) -> Vec<u8> {
    pdf(&format!("BT /F1 12 Tf 20 100 Td ({text}) Tj ET"), &[])
}

fn one(s: &str) -> Vec<String> {
    vec![s.to_owned()]
}

fn residual(text: &str, site: ResidualSite) -> Vec<Residual> {
    vec![Residual {
        text: text.to_owned(),
        site,
    }]
}

#[test]
fn a_string_in_page_content_refuses() {
    let proof = verify_absence(&drawing("KEEP THIS SECRET"), &one("SECRET"));
    assert_eq!(
        proof.residuals,
        residual("SECRET", ResidualSite::DrawnContent)
    );
    assert_eq!(proof.verdict(), AbsenceVerdict::Refuse);
    assert_eq!(proof.drawn_survivors().collect::<Vec<_>>(), ["SECRET"]);
}

#[test]
fn an_absent_string_is_verified() {
    let proof = verify_absence(&drawing("ordinary"), &one("SECRET"));
    assert_eq!(proof.strings_checked, 1);
    assert!(proof.is_clean());
    assert_eq!(proof.verdict(), AbsenceVerdict::Verified);
}

#[test]
fn a_raw_byte_hit_outside_every_stream_discloses() {
    let mut bytes = drawing("ordinary");
    // A dictionary string, in no stream.
    let at = bytes.windows(4).position(|w| w == b"xref").unwrap();
    bytes.splice(at..at, b"% MARGARETHALE\n".iter().copied());
    let proof = verify_absence(&bytes, &one("MARGARETHALE"));
    assert_eq!(
        proof.residuals,
        residual("MARGARETHALE", ResidualSite::RawBytes)
    );
    assert_eq!(proof.verdict(), AbsenceVerdict::Disclose);
}

#[test]
fn short_strings_are_counted_and_never_refuse() {
    let needles: Vec<String> = "3.5 TYP".chars().map(String::from).collect();
    let proof = verify_absence(&drawing("SHEET 3 OF 5, TYP."), &needles);
    assert_eq!(proof.strings_too_short, 7);
    assert_eq!(proof.strings_checked, 0);
    assert_eq!(proof.verdict(), AbsenceVerdict::Verified);
    // At the floor, drawn, it refuses.
    let proof = verify_absence(&drawing("abcd drawn"), &one("abcd"));
    assert_eq!(proof.verdict(), AbsenceVerdict::Refuse);
}

#[test]
fn empty_and_duplicate_needles_are_not_counted() {
    let proof = verify_absence(
        &drawing("x"),
        &[String::new(), "SECRET".into(), "SECRET".into()],
    );
    assert_eq!(proof.strings_checked, 1);
    assert_eq!(proof.strings_too_short, 0);
}

#[test]
fn unparsable_output_discloses_and_still_searches_raw() {
    let proof = verify_absence(b"junk MARGARETHALE junk", &one("MARGARETHALE"));
    assert!(proof.output_unparsed);
    assert_eq!(
        proof.residuals,
        residual("MARGARETHALE", ResidualSite::RawBytes)
    );
    assert_eq!(proof.verdict(), AbsenceVerdict::Disclose);
    // Unparsable and absent: still not verified.
    assert_eq!(
        verify_absence(b"junk", &one("MARGARETHALE")).verdict(),
        AbsenceVerdict::Disclose
    );
}

#[test]
fn every_opaque_site_is_named_and_discloses() {
    let cases: &[(&str, ResidualSite)] = &[
        ("/Length1 3400", ResidualSite::FontProgram),
        ("/Subtype /Type1C", ResidualSite::FontProgram),
        ("/Subtype /OpenType", ResidualSite::FontProgram),
        (
            "/Type /XObject /Subtype /Image /Width 4 /Height 4",
            ResidualSite::ImageSamples,
        ),
        ("/Type /ObjStm /N 0 /First 0", ResidualSite::ObjectContainer),
        ("/Type /EmbeddedFile", ResidualSite::Attachment),
        ("/Type /Metadata /Subtype /XML", ResidualSite::Metadata),
        ("/Some /Thing", ResidualSite::OtherStream),
    ];
    for (dict, site) in cases {
        let bytes = pdf(
            "BT (ordinary) Tj ET",
            &[stream(dict, b"MARGARETHALE lives here")],
        );
        let proof = verify_absence(&bytes, &one("MARGARETHALE"));
        assert_eq!(proof.residuals, residual("MARGARETHALE", *site), "{dict}");
        assert_eq!(proof.verdict(), AbsenceVerdict::Disclose, "{dict}");
    }
}

#[test]
fn form_xobject_tiling_pattern_and_type3_glyph_are_drawn() {
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 9 9]",
        b"BT (MARGARETHALE) Tj ET",
    );
    let pattern = stream(
        "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 8 8] /XStep 8 /YStep 8 /Resources << >>",
        b"BT (MARGARETHALE) Tj ET",
    );
    for extra in [form, pattern] {
        let proof = verify_absence(&pdf("q /X Do Q", &[extra]), &one("MARGARETHALE"));
        assert_eq!(proof.verdict(), AbsenceVerdict::Refuse);
    }
    // A glyph procedure is a bare stream, recognised only via the font.
    let type3 = b"<< /Type /Font /Subtype /Type3 /FontBBox [0 0 8 8] /FontMatrix [0.001 0 0 0.001 0 0] \
                  /CharProcs << /a 7 0 R >> /Encoding << >> /FirstChar 97 /LastChar 97 /Widths [8] >>";
    let bytes = pdf(
        "BT (ordinary) Tj ET",
        &[
            type3.to_vec(),
            stream("", b"0 0 d0 BT (MARGARETHALE) Tj ET"),
        ],
    );
    let proof = verify_absence(&bytes, &one("MARGARETHALE"));
    assert_eq!(
        proof.residuals,
        residual("MARGARETHALE", ResidualSite::DrawnContent)
    );
}

#[test]
fn drawn_content_outranks_an_earlier_opaque_hit() {
    // Object 6 (a font program) is swept before object 7 (a form).
    let bytes = pdf(
        "BT (ordinary) Tj ET",
        &[
            stream("/Length1 10", b"MARGARETHALE"),
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 9 9]",
                b"(MARGARETHALE) Tj",
            ),
        ],
    );
    let proof = verify_absence(&bytes, &one("MARGARETHALE"));
    assert_eq!(
        proof.residuals,
        residual("MARGARETHALE", ResidualSite::DrawnContent)
    );
}

#[test]
fn a_flate_stream_is_searched_decoded() {
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut enc, b"BT (MARGARETHALE) Tj ET").unwrap();
    let body = enc.finish().unwrap();
    let bytes = pdf(
        "q /X Do Q",
        &[stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 9 9] /Filter /FlateDecode",
            &body,
        )],
    );
    assert!(
        !super::contains(&bytes, b"MARGARETHALE"),
        "fixture must hide it"
    );
    assert_eq!(
        verify_absence(&bytes, &one("MARGARETHALE")).verdict(),
        AbsenceVerdict::Refuse
    );
}

#[test]
fn an_undecodable_stream_forfeits_verified() {
    let bytes = pdf(
        "BT (ordinary) Tj ET",
        &[stream("/Filter /NoSuchDecode", b"opaque")],
    );
    let proof = verify_absence(&bytes, &one("MARGARETHALE"));
    assert_eq!(proof.streams_undecoded, 1);
    assert!(proof.residuals.is_empty());
    assert_eq!(proof.verdict(), AbsenceVerdict::Disclose);
}

#[test]
fn an_image_codec_stream_is_not_counted_as_undecoded() {
    let img = stream(
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /DCTDecode",
        b"\xFF\xD8",
    );
    let proof = verify_absence(&pdf("BT (ordinary) Tj ET", &[img]), &one("MARGARETHALE"));
    assert_eq!(proof.streams_undecoded, 0);
    assert_eq!(proof.verdict(), AbsenceVerdict::Verified);
}

/// Mark `rect` on page 0 of `input`, save, reload, apply.
fn apply_over(input: Vec<u8>, rect: [f64; 4]) -> (Vec<u8>, Vec<String>) {
    let mut session = EditSession::new(Document::from_bytes(input).unwrap());
    let spec = RedactSpec {
        quads: vec![Quad::from_rect(Rect::from_corners(
            rect[0], rect[1], rect[2], rect[3],
        ))],
        fill: None,
        overlay_text: None,
        quadding: Quadding::Left,
    };
    session.add_redaction(0, &spec).unwrap();
    let (marked, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap();
    let doc = Document::from_bytes(marked).unwrap();
    let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
    (out, report.redacted_text)
}

#[test]
fn a_real_apply_is_verified() {
    let (out, text) = apply_over(drawing("CLASSIFIED"), [10.0, 90.0, 390.0, 130.0]);
    assert!(text.iter().any(|t| t.contains("CLASSIFIED")), "{text:?}");
    let proof = verify_absence(&out, &text);
    assert_eq!(proof.verdict(), AbsenceVerdict::Verified, "{proof:?}");
}

/// The refuse path end to end: the same word drawn outside the marked region
/// survives the apply, and the proof refuses on the real output.
#[test]
fn a_real_apply_with_an_unmarked_repeat_refuses() {
    let content =
        "BT /F1 12 Tf 20 100 Td (CLASSIFIED) Tj ET BT /F1 12 Tf 20 20 Td (CLASSIFIED) Tj ET";
    let (out, text) = apply_over(pdf(content, &[]), [10.0, 90.0, 390.0, 130.0]);
    let proof = verify_absence(&out, &text);
    assert_eq!(proof.verdict(), AbsenceVerdict::Refuse, "{proof:?}");
    assert!(proof.drawn_survivors().any(|s| s.contains("CLASSIFIED")));
}
