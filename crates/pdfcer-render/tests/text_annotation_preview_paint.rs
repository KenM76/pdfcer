//! `paint_text_annotation_preview` paints the pixels the committed text box
//! paints: preview it, commit it, render the saved page, compare (pdfcer-gui
//! request G282).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot_author::{Color as AnnotColor, TextAnnotSpec, free_text_preview};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, MarkupNote};
use pdfcer_core::fontdata::Std14;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vartext::{Quadding, TextColor};
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::edit_preview::paint_text_annotation_preview;
use pdfcer_render::tiny_skia::{Color, Pixmap};
use pdfcer_render::{RenderOptions, page_device_geometry, render_page_with};

const WORDS: &str = "one two three four five six seven eight nine ten";

fn blank_page_doc(rotate: i32) -> Document {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] /Rotate {rotate} \
         /Resources << >> >>"
    );
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        page.as_str(),
    ];
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(buf).unwrap()
}

fn free_text(text: &str, multiline: bool) -> TextAnnotSpec {
    TextAnnotSpec::FreeText {
        rect: Rect {
            llx: 60.0,
            lly: 150.0,
            urx: 170.0,
            ury: 200.0,
        },
        text: text.to_owned(),
        font: Std14::Helvetica,
        font_size: 12.0,
        color: TextColor::Gray(0.0),
        quadding: Quadding::Left,
        multiline,
        border: Some(AnnotColor::Rgb(1.0, 0.0, 0.0)),
        border_width: 1.5,
        frame: Default::default(),
    }
}

/// The page of `s` with `authored` painted over white, and what the
/// interpreter reported.
fn painted(
    s: &EditSession,
    authored: &pdfcer_core::annot_author::AuthoredTextAnnot,
    scale: f32,
) -> (Pixmap, Vec<String>) {
    let page = s.pages().unwrap().remove(0);
    let (w, h, to_device) = page_device_geometry(&page, scale);
    let mut pixmap = Pixmap::new(w, h).unwrap();
    pixmap.fill(Color::WHITE);
    let diag = paint_text_annotation_preview(
        &s.view(),
        authored,
        &RenderOptions::default(),
        to_device,
        &mut pixmap,
    );
    (pixmap, diag.sample_ops)
}

fn saved_render(s: &EditSession, scale: f32) -> (Pixmap, Vec<String>) {
    let saved = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(saved).unwrap();
    let page = pdfcer_core::page_tree::pages(&doc).unwrap().remove(0);
    let out = render_page_with(&doc, &page, scale, &RenderOptions::default()).unwrap();
    (out.pixmap, out.diagnostics.sample_ops)
}

fn assert_same_pixels(preview: &(Pixmap, Vec<String>), committed: &(Pixmap, Vec<String>)) {
    // The interpreter reports the same things about both.
    assert_eq!(preview.1, committed.1);
    let (preview, committed) = (&preview.0, &committed.0);
    assert_eq!(
        (preview.width(), preview.height()),
        (committed.width(), committed.height())
    );
    let ink = preview
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[1] < 0xF0)
        .count();
    assert!(ink > 200, "the preview painted nothing ({ink})");
    let differing = preview
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(committed.data().as_chunks::<4>().0)
        .filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 2))
        .count();
    assert_eq!(differing, 0, "preview and committed render differ");
}

#[test]
fn a_new_text_box_preview_paints_what_the_commit_renders() {
    for (rotate, multiline) in [(0, true), (0, false), (90, true), (270, false)] {
        let mut s = EditSession::new(blank_page_doc(rotate));
        let spec = free_text(WORDS, multiline);
        let preview = free_text_preview(&spec).unwrap().unwrap();
        let before = painted(&s, &preview.authored, 2.0);

        s.add_text_annotation(0, &spec).unwrap();
        assert_same_pixels(&before, &saved_render(&s, 2.0));
    }
}

#[test]
fn a_retyped_text_box_preview_paints_what_the_commit_renders() {
    for multiline in [true, false] {
        let mut s = EditSession::new(blank_page_doc(0));
        let id = s
            .add_text_annotation(0, &free_text("first", multiline))
            .unwrap();
        let preview = s.free_text_note_preview(id, WORDS).unwrap().unwrap();

        // The preview is painted over a page without the old box, as a shell
        // hiding the annotation being edited would show it.
        let bare = EditSession::new(blank_page_doc(0));
        let before = painted(&bare, &preview.authored, 1.5);

        s.set_markup_note(id, &MarkupNote::new(WORDS)).unwrap();
        assert_same_pixels(&before, &saved_render(&s, 1.5));
    }
}
