//! `Pass 289.0` — an annotation that NAMES a standard icon and carries no
//! `/AP` is drawn.
//!
//! # The report
//!
//! An operator supplied `Annotations_output.pdf` (PDFsharp 1.3): a page whose
//! entire visible content is three annotations, **none with an `/AP`** — two
//! `/Text` (`/Note`, `/Help`) and one `/Stamp` (`/TopSecret`). **Acrobat draws
//! all three; pdfcer rendered a blank page.**
//!
//! # ★★★ Why `R43` was being applied outside its territory
//!
//! `R43` says an annotation is rendered from its `/AP` or not at all, and
//! nothing in `pdfcer-render` synthesises an appearance. Right for a `/Square`
//! or a `/Line`, where a reader would have to **invent geometry**.
//!
//! Wrong for the subtypes that **name an icon**, and the standard says so with
//! a `shall` **addressed to the reader** — §12.5.6.4 Table 172:
//!
//! > *"Conforming readers **shall** provide predefined icon appearances for at
//! > least the following standard names: Comment, Key, Note, Help,
//! > NewParagraph, Paragraph, Insert."*
//!
//! §12.5.6.12 Table 181 carries the identical formula for `/Stamp`'s fourteen
//! names, `TopSecret` among them. And §12.5.2's `/AP` row settles that `/AP`
//! was never the only route: *"Individual annotation handlers **may ignore
//! this entry and provide their own appearances**."*
//!
//! ⇒ **Drawing a named icon is not synthesis — it is the reader discharging a
//! duty the standard assigned to it.** The discriminator is the grammatical
//! subject: the icon clauses address *conforming readers*; §12.5.6.8's
//! square/circle clause addresses *the annotation*. `R43` is untouched for the
//! second class, and these tests assert both halves.
//!
//! ★ The obligation is on **pixels, not objects** — no clause asks a reader to
//! materialise an `/AP` into the file — so this changes no bytes and neither
//! rule 3 nor `R44` is in play.

use pdfcer_core::document::Document;
use std::path::PathBuf;

/// A one-page PDF whose annotations carry no `/AP`, built from parts so the
/// test does not depend on a file outside the repository.
fn no_ap_pdf(annots: &str, extra_objects: &str, count: u32) -> Vec<u8> {
    let bodies: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_string()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string()),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << >> \
                 /Annots [{annots}] >>"
            ),
        ),
    ];

    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in &bodies {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    // The annotation objects, numbered from 4.
    for (i, body) in extra_objects.split('|').enumerate() {
        if body.trim().is_empty() {
            continue;
        }
        let num = 4 + i as u32;
        offsets.push((num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }

    let xref_at = buf.len();
    let high = 4 + count;
    buf.extend_from_slice(format!("xref\n0 {high}\n").as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for n in 1..high {
        let off = offsets
            .iter()
            .find(|(num, _)| *num == n)
            .map(|(_, o)| *o)
            .unwrap_or(0);
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {high} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fn render(bytes: Vec<u8>) -> pdfcer_render::RenderedPage {
    let doc = Document::from_bytes(bytes).expect("the fixture loads");
    let pages = pdfcer_core::page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page(&doc, &pages[0], 1.0).expect("the page renders")
}

/// How many pixels are not the paper colour — "did anything get drawn?".
///
/// Counted off the pixmap rather than a decoded PNG so the test needs no
/// image decoder: the render's own buffer is the thing under test anyway.
fn ink(page: &pdfcer_render::RenderedPage) -> usize {
    page.pixmap
        .pixels()
        .iter()
        .filter(|p| p.alpha() > 0 && (p.red(), p.green(), p.blue()) != (255, 255, 255))
        .count()
}

// ------------------------------------------------- 1. the reported defect

/// ★★★ A `/Text` naming `/Note` with no `/AP` is DRAWN.
#[test]
fn a_text_annotation_naming_an_icon_is_drawn() {
    let out = render(no_ap_pdf(
        "4 0 R",
        "<< /Type /Annot /Subtype /Text /Name /Note /Rect [30 250 60 280] >>",
        1,
    ));

    assert_eq!(
        out.diagnostics.annotations_icon_painted, 1,
        "the icon was painted"
    );
    assert!(ink(&out) > 0, "and the page is no longer blank");
}

/// ★★ A `/Stamp` naming `/TopSecret` with no `/AP` is DRAWN — the operator's
/// own third annotation.
#[test]
fn a_stamp_annotation_naming_an_icon_is_drawn() {
    let out = render(no_ap_pdf(
        "4 0 R",
        "<< /Type /Annot /Subtype /Stamp /Name /TopSecret /C [1 1 0] \
         /Rect [40 120 260 200] >>",
        1,
    ));

    assert_eq!(out.diagnostics.annotations_icon_painted, 1);
    assert!(ink(&out) > 0, "the stamp is on the page");
}

// ------------------------------------- 2. R43 survives for the other class

/// ★★★ THE CONTROL THAT KEEPS `R43` INTACT: a `/Square` with no `/AP` is still
/// NOT drawn.
///
/// This is the whole reason the change is a narrowing of `R43`'s scope rather
/// than its repeal. §12.5.6.8 addresses *the annotation* ("Square and circle
/// annotations shall display…"), not the reader, and pdfcer will not invent
/// geometry. Without this test, "draw what has no `/AP`" could quietly become
/// a blanket synthesis switch — which is precisely what the spec research
/// warned must not happen (`AG-A1` and `AG-A3` are different tiers).
#[test]
fn a_square_with_no_ap_is_still_not_drawn() {
    let out = render(no_ap_pdf(
        "4 0 R",
        "<< /Type /Annot /Subtype /Square /IC [1 0 0] /Rect [40 40 260 260] >>",
        1,
    ));

    assert_eq!(
        out.diagnostics.annotations_icon_painted, 0,
        "a /Square names no icon and must not be drawn"
    );
    assert_eq!(ink(&out), 0, "the page stays blank, which is R43 working");
    assert_eq!(
        out.diagnostics.annotations_without_ap.get("Square"),
        Some(&1),
        "and it is still disclosed by subtype"
    );
}

// ------------------------------------------------- 3. the disclosure

/// ★★ BOTH counters are reported, and they answer DIFFERENT questions.
///
/// `annotations_without_ap` is a fact about the **file**; `annotations_icon_painted`
/// is what the operator **saw**. Folding them together would make one of the
/// two a lie — the old note said these were "NOT painted", which became false
/// the moment the icon class started drawing.
#[test]
fn the_file_fact_and_the_painted_fact_are_counted_separately() {
    let out = render(no_ap_pdf(
        "4 0 R 5 0 R",
        "<< /Type /Annot /Subtype /Text /Name /Help /Rect [30 250 60 280] >>|\
         << /Type /Annot /Subtype /Square /IC [1 0 0] /Rect [40 40 200 200] >>",
        2,
    ));

    let without: usize = out.diagnostics.annotations_without_ap.values().sum();
    assert_eq!(without, 2, "the FILE left two annotations without an /AP");
    assert_eq!(
        out.diagnostics.annotations_icon_painted, 1,
        "and the operator saw one of them"
    );
}

/// ★ An annotation that already HAS an `/AP` is untouched by this path.
///
/// Without this, a change that drew the icon unconditionally would override
/// the file's own artwork — the opposite defect, and a far worse one, since
/// the file's appearance is authoritative when it exists.
#[test]
fn an_annotation_with_an_ap_is_not_second_guessed() {
    let path: PathBuf =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf");
    let doc = Document::load(&path).expect("hello.pdf loads");
    let pages = pdfcer_core::page_tree::pages(&doc).expect("pages");
    let out = pdfcer_render::render_page(&doc, &pages[0], 1.0).expect("renders");
    assert_eq!(
        out.diagnostics.annotations_icon_painted, 0,
        "a document with no /AP-less icon annotations paints no icons"
    );
}

/// ★★★ THE CONTROL THAT ACTUALLY MEASURES THE SUBTYPE RESTRICTION, and it
/// exists because a sabotage survived without it.
///
/// Deleting the `subtype != "Text" && subtype != "Stamp"` guard left all five
/// earlier tests green. The `/Square` control passes for the WRONG REASON:
/// `text_spec_from_dict` cannot describe a `/Square` at all, so the icon path
/// bails one line later regardless. The guard was protected by a different
/// guard, and nothing measured it.
///
/// A `/FreeText` is the case that separates them: `text_spec_from_dict`
/// **does** describe it, so only the subtype restriction stops it being
/// drawn — and it must be stopped, because §12.5.6.19 puts no `shall` on the
/// reader and a `/FreeText`'s look comes from its `/DA` and `/Contents`, which
/// is authoring, not an icon.
#[test]
fn a_free_text_with_no_ap_is_not_drawn_even_though_pdfcer_could_author_one() {
    let out = render(no_ap_pdf(
        "4 0 R",
        "<< /Type /Annot /Subtype /FreeText /Contents (hello) /DA (/Helv 12 Tf 0 g) \
         /Rect [40 40 260 120] >>",
        1,
    ));

    assert_eq!(
        out.diagnostics.annotations_icon_painted, 0,
        "a /FreeText names no icon: the standard puts no drawing duty on the reader for it, \
         and pdfcer being ABLE to author one is not permission to invent this document's"
    );
    assert_eq!(ink(&out), 0, "the page stays blank");
}
