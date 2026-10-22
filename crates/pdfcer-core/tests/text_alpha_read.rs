//! `G225` — the fill alpha (ExtGState `/ca`, ISO 32000-1 §8.4.5 Table 58) in
//! force on each text run, read back from the page model, on the page and in
//! form leaves, with an unreadable `gs` reported rather than guessed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::{FormatOptions, FormatRequest};
use pdfcer_core::vector::decompose::{TextRun, VectorObject};
use pdfcer_core::writer::SaveOptions;

fn pdf(objects: &[(u32, String)]) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0usize; objects.len() + 1];
    for (n, body) in objects {
        offsets[*n as usize] = out.len();
        out.extend_from_slice(format!("{n} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in &offsets[1..] {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn stream(content: &str, extra: &str) -> String {
    format!(
        "<< {extra} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

const FONT: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";

/// A page drawing `content` with `/F1`, the extra page resources `res`, and
/// a form `/Fm1` drawing `form` with its own resources `form_res`.
fn page(content: &str, res: &str, form: &str, form_res: &str) -> EditSession {
    page_with(content, res, form, form_res, "")
}

/// [`page`], with `form_extra` added to the form's dictionary.
fn page_with(
    content: &str,
    res: &str,
    form: &str,
    form_res: &str,
    form_extra: &str,
) -> EditSession {
    EditSession::new(
        Document::from_bytes(pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
            (
                3,
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
                     /Resources << /Font << /F1 5 0 R >> /XObject << /Fm1 6 0 R >> {res} >> >>"
                ),
            ),
            (4, stream(content, "")),
            (5, FONT.to_owned()),
            (
                6,
                stream(
                    form,
                    &format!(
                        "/Type /XObject /Subtype /Form /BBox [0 0 612 792] {form_extra} \
                         /Resources << /Font << /F1 5 0 R >> {form_res} >>"
                    ),
                ),
            ),
        ]))
        .unwrap(),
    )
}

fn page_runs(s: &mut EditSession) -> Vec<TextRun> {
    s.page_objects(0)
        .unwrap()
        .objects
        .iter()
        .filter_map(|o| match o {
            VectorObject::Text(t) => Some(t.runs.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

fn leaf_runs(s: &mut EditSession) -> Vec<TextRun> {
    s.page_objects(0)
        .unwrap()
        .leaves
        .iter()
        .filter_map(|l| match &l.object {
            VectorObject::Text(t) => Some(t.runs.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

fn alphas(runs: &[TextRun]) -> Vec<Option<f64>> {
    runs.iter().map(|r| r.fill_alpha).collect()
}

const GS: &str = "/ExtGState << /GS3 << /ca 0.3 >> /GS7 << /ca 0.7 /CA 0.1 >> \
                  /GSB << /ca 2.5 >> /GSN << /ca /Half >> /GSL << /LW 4 >> >>";

#[test]
fn text_with_no_gs_reads_opaque() {
    let mut s = page("BT /F1 12 Tf 72 700 Td (a) Tj ET", "", "", "");
    assert_eq!(alphas(&page_runs(&mut s)), vec![Some(1.0)]);
}

#[test]
fn each_run_reads_the_alpha_in_force_when_it_is_shown() {
    let mut s = page(
        "BT /F1 12 Tf 72 700 Td (a) Tj /GS3 gs (b) Tj /GSL gs (c) Tj /GS7 gs (d) Tj ET",
        GS,
        "",
        "",
    );
    assert_eq!(
        alphas(&page_runs(&mut s)),
        vec![Some(1.0), Some(0.3), Some(0.3), Some(0.7)]
    );
}

#[test]
fn q_and_big_q_restore_the_alpha() {
    let mut s = page(
        "q /GS3 gs BT /F1 12 Tf 72 700 Td (a) Tj ET Q BT /F1 12 Tf 72 600 Td (b) Tj ET",
        GS,
        "",
        "",
    );
    assert_eq!(alphas(&page_runs(&mut s)), vec![Some(0.3), Some(1.0)]);
}

#[test]
fn an_out_of_range_alpha_is_clamped() {
    let mut s = page("/GSB gs BT /F1 12 Tf 72 700 Td (a) Tj ET", GS, "", "");
    assert_eq!(alphas(&page_runs(&mut s)), vec![Some(1.0)]);
}

#[test]
fn an_unresolvable_gs_reads_unknown_until_a_readable_one() {
    let mut s = page(
        "BT /F1 12 Tf 72 700 Td /GSX gs (a) Tj /GSL gs (b) Tj /GS3 gs (c) Tj ET",
        GS,
        "",
        "",
    );
    assert_eq!(alphas(&page_runs(&mut s)), vec![None, None, Some(0.3)]);
}

#[test]
fn a_non_numeric_ca_reads_unknown() {
    let mut s = page("/GSN gs BT /F1 12 Tf 72 700 Td (a) Tj ET", GS, "", "");
    assert_eq!(alphas(&page_runs(&mut s)), vec![None]);
}

#[test]
fn unknown_is_restored_away_by_big_q() {
    let mut s = page(
        "q /GSX gs BT /F1 12 Tf 72 700 Td (a) Tj ET Q BT /F1 12 Tf 72 600 Td (b) Tj ET",
        GS,
        "",
        "",
    );
    assert_eq!(alphas(&page_runs(&mut s)), vec![None, Some(1.0)]);
}

#[test]
fn a_form_leaf_reads_its_own_gs_and_the_ambient_alpha() {
    let mut s = page(
        "/GS7 gs /Fm1 Do",
        GS,
        "BT /F1 12 Tf 72 700 Td (a) Tj /GF gs (b) Tj ET",
        "/ExtGState << /GF << /ca 0.25 >> >>",
    );
    assert_eq!(alphas(&leaf_runs(&mut s)), vec![Some(0.7), Some(0.25)]);
}

#[test]
fn a_form_leaf_with_an_unresolvable_gs_reads_unknown() {
    let mut s = page("/Fm1 Do", "", "BT /F1 12 Tf 72 700 Td /GX gs (a) Tj ET", "");
    assert_eq!(alphas(&leaf_runs(&mut s)), vec![None]);
}

#[test]
fn a_written_alpha_reads_back() {
    let mut s = page(
        "BT /F1 12 Tf 72 700 Td (hello) Tj 0 -20 Td (world) Tj ET",
        "",
        "",
        "",
    );
    s.format_text(
        &FormatRequest::new(0, "hello").fill_alpha(0.4),
        &FormatOptions::default(),
    )
    .unwrap();
    let before = alphas(&page_runs(&mut s));
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    let mut reopened = EditSession::new(Document::from_bytes(bytes).unwrap());
    for runs in [before, alphas(&page_runs(&mut reopened))] {
        assert!(runs.contains(&Some(0.4)), "{runs:?}");
        assert!(runs.contains(&Some(1.0)), "{runs:?}");
    }
}

#[test]
fn a_transparency_group_form_starts_opaque() {
    for (group, want) in [("/Group << /S /Transparency >>", 1.0), ("", 0.7)] {
        let mut s = page_with(
            "/GS7 gs /Fm1 Do",
            GS,
            "BT /F1 12 Tf 72 700 Td (a) Tj ET",
            "",
            group,
        );
        assert_eq!(alphas(&leaf_runs(&mut s)), vec![Some(want)], "{group}");
    }
}

fn leaf_paths(s: &mut EditSession) -> Vec<pdfcer_core::vector::decompose::PathObject> {
    s.page_objects(0)
        .unwrap()
        .leaves
        .iter()
        .filter_map(|l| match &l.object {
            VectorObject::Path(p) => Some(p.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_path_leaf_inherits_both_alphas_and_their_readability() {
    let mut s = page(
        "/GS7 gs /Fm1 Do",
        GS,
        "0 0 10 10 re B /GX gs 0 0 5 5 re B",
        "",
    );
    let p = leaf_paths(&mut s);
    assert_eq!(p.len(), 2);
    assert!((p[0].fill_alpha - 0.7).abs() < 1e-9 && (p[0].stroke_alpha - 0.1).abs() < 1e-9);
    assert!(p[0].fill_alpha_known && p[0].stroke_alpha_known);
    assert!(!p[1].fill_alpha_known && !p[1].stroke_alpha_known);
}

#[test]
fn a_page_path_after_a_non_numeric_ca_has_only_its_fill_alpha_unknown() {
    let mut s = page("/GSN gs 0 0 10 10 re B", GS, "", "");
    let objs = s.page_objects(0).unwrap().objects.clone();
    let Some(VectorObject::Path(p)) = objs.first() else {
        panic!("{objs:?}")
    };
    assert!(!p.fill_alpha_known && p.stroke_alpha_known);
}
