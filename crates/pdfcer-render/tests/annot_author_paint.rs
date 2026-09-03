//! # R44 round-trip: an authored appearance actually paints
//!
//! The end-to-end proof of decision 008's R44 (*"any appearance pdfcer
//! generates is written into the file, never rendered privately"*) plus
//! the Pass 6.1 authoring path: **author → save → reload → paint**. A
//! geometric-markup annotation is authored through the core
//! [`EditSession`], saved incrementally, reloaded as a fresh
//! [`Document`], and rendered through the *existing* Pass 6.0 read path.
//! If the authored `/AP` were a private pdfcer-only rendering (the R44
//! failure), the reloaded file would carry no appearance and the render
//! would be blank of it; this test fails in exactly that case.
//!
//! It is a `pdfcer-render` integration test because that is where the
//! consuming raster lives — the same self-comparison oracle Pass 6.0
//! uses, now closing the authoring loop.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot_author::{Color, MarkupSpec, StampName, TextAnnotSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::fontdata::Std14;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::vartext::{Quadding, TextColor};
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::{RenderOptions, render_page_with};

/// A one-page, classic-xref document with a blank page.
fn blank_page_doc() -> Document {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 120] /Resources << >> >>",
        "<< /Type /Page /Parent 2 0 R >>",
    ];
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    Document::from_bytes(buf).unwrap()
}

/// Count pixels that are not pure white (the page background).
fn non_white(pixmap: &pdfcer_render::tiny_skia::Pixmap) -> usize {
    pixmap
        .data()
        .chunks_exact(4)
        .filter(|px| px[0] != 0xFF || px[1] != 0xFF || px[2] != 0xFF)
        .count()
}

/// Count pixels that are predominantly red (R high, G/B low) — the
/// authored square's border colour.
fn reddish(pixmap: &pdfcer_render::tiny_skia::Pixmap) -> usize {
    pixmap
        .data()
        .chunks_exact(4)
        .filter(|px| px[0] > 0x80 && px[1] < 0x80 && px[2] < 0x80)
        .count()
}

#[test]
fn authored_square_paints_after_save_and_reload_r44() {
    // Author a red square through the core editing path.
    let mut session = EditSession::new(blank_page_doc());
    session
        .add_markup(
            0,
            &MarkupSpec::Square {
                rect: Rect {
                    llx: 20.0,
                    lly: 20.0,
                    urx: 180.0,
                    ury: 100.0,
                },
                border: Some(Color::Rgb(1.0, 0.0, 0.0)),
                interior: None,
                border_width: 4.0,
                border_effect: None,
            },
        )
        .unwrap();
    let (bytes, _report) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap();

    // Reload as a fresh document — nothing pdfcer-private survives this.
    let reloaded = Document::from_bytes(bytes).unwrap();
    let page = page_tree::pages(&reloaded).unwrap().remove(0);

    // Render with and without annotations.
    let on = render_page_with(
        &reloaded,
        &page,
        2.0,
        &RenderOptions::default().with_annotations(true),
    )
    .unwrap();
    let off = render_page_with(
        &reloaded,
        &page,
        2.0,
        &RenderOptions::default().with_annotations(false),
    )
    .unwrap();

    // With annotations OFF the page is blank; with them ON the authored
    // square paints red pixels. This is R44 end to end.
    assert_eq!(non_white(&off.pixmap), 0, "the base page must be blank");
    assert!(
        non_white(&on.pixmap) > 0,
        "the authored appearance must paint something"
    );
    assert!(
        reddish(&on.pixmap) > 50,
        "the authored square's red border must be visible (got {} red px)",
        reddish(&on.pixmap)
    );
    // Pass 6.0 counted it as painted, not merely surveyed.
    assert_eq!(on.diagnostics.annotations_total, 1);
    assert_eq!(on.diagnostics.annotations_painted, 1);
}

/// Count pixels that are predominantly dark (all channels low) — the
/// black text glyphs of an authored FreeText/Stamp label.
fn dark(pixmap: &pdfcer_render::tiny_skia::Pixmap) -> usize {
    pixmap
        .data()
        .chunks_exact(4)
        .filter(|px| px[0] < 0x60 && px[1] < 0x60 && px[2] < 0x60)
        .count()
}

/// Author `spec` on a blank page, save incrementally, and reload as a
/// fresh document — the R44 "nothing pdfcer-private survives" boundary.
fn author_text_and_reload(spec: &TextAnnotSpec) -> (Document, pdfcer_core::page_tree::Page) {
    let mut session = EditSession::new(blank_page_doc());
    session.add_text_annotation(0, spec).unwrap();
    let (bytes, _report) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap();
    let reloaded = Document::from_bytes(bytes).unwrap();
    let page = page_tree::pages(&reloaded).unwrap().remove(0);
    (reloaded, page)
}

#[test]
fn authored_freetext_paints_glyph_pixels_after_reload_r44() {
    // Author black centred text; save; reload; render through the Pass 6.0
    // read path. This exercises the *text* render pipeline on a
    // pdfcer-authored variable-text appearance — the R44 proof for text.
    let (reloaded, page) = author_text_and_reload(&TextAnnotSpec::FreeText {
        rect: Rect {
            llx: 10.0,
            lly: 45.0,
            urx: 190.0,
            ury: 80.0,
        },
        text: "Reviewed".to_owned(),
        font: Std14::Helvetica,
        font_size: 24.0,
        color: TextColor::Gray(0.0),
        quadding: Quadding::Center,
        multiline: false,
        border: None,
        border_width: 0.0,
    });

    let on = render_page_with(
        &reloaded,
        &page,
        2.0,
        &RenderOptions::default().with_annotations(true),
    )
    .unwrap();
    let off = render_page_with(
        &reloaded,
        &page,
        2.0,
        &RenderOptions::default().with_annotations(false),
    )
    .unwrap();

    // The base page is blank; the authored text paints actual dark glyph
    // pixels (asserting real glyphs, not merely "an appearance exists").
    assert_eq!(non_white(&off.pixmap), 0, "the base page must be blank");
    assert!(
        dark(&on.pixmap) > 100,
        "the authored FreeText must paint black glyph pixels (got {} dark px)",
        dark(&on.pixmap)
    );
    assert_eq!(on.diagnostics.annotations_total, 1);
    assert_eq!(on.diagnostics.annotations_painted, 1);
}

#[test]
fn bare_standard14_font_dict_renders_with_no_embedded_program() {
    // The authored appearance's font dict must be the bare standard-14
    // form (no /FontDescriptor, hence no embedded program — §9.6.2.1) AND
    // still paint glyphs. This is the "standard-14-bare-font-dict renders"
    // gate.
    let (reloaded, page) = author_text_and_reload(&TextAnnotSpec::FreeText {
        rect: Rect {
            llx: 10.0,
            lly: 45.0,
            urx: 190.0,
            ury: 80.0,
        },
        text: "Bare".to_owned(),
        font: Std14::Helvetica,
        font_size: 24.0,
        color: TextColor::Gray(0.0),
        quadding: Quadding::Left,
        multiline: false,
        border: None,
        border_width: 0.0,
    });

    // Walk to the appearance stream's font dict and assert it is program-free.
    let annots = pdfcer_core::annot::page_annotations(&reloaded, page.id);
    let ap_id = match &annots[0].appearance {
        pdfcer_core::annot::Appearance::Normal {
            stream_id: Some(id),
        } => *id,
        other => panic!("expected a normal appearance stream, got {other:?}"),
    };
    let Object::Stream(ap) = &reloaded.get(ap_id).unwrap().value else {
        panic!("appearance is not a stream");
    };
    let font_dict = resolve_font_dict(&reloaded, &ap.dict);
    assert_eq!(
        font_dict
            .get(b"BaseFont")
            .unwrap()
            .as_name()
            .unwrap()
            .as_bytes(),
        b"Helvetica"
    );
    assert!(
        font_dict.get(b"FontDescriptor").is_none() && font_dict.get(b"Widths").is_none(),
        "the standard-14 dict must carry no descriptor or widths (no embedded program)"
    );

    // And it renders glyphs regardless.
    let on = render_page_with(
        &reloaded,
        &page,
        2.0,
        &RenderOptions::default().with_annotations(true),
    )
    .unwrap();
    assert!(dark(&on.pixmap) > 50, "the bare-dict text must paint");
}

/// Resolve `/Resources /Font /Helv` (following the one indirect the writer
/// may introduce) to the font dictionary.
fn resolve_font_dict<'a>(
    doc: &'a Document,
    ap_dict: &'a pdfcer_core::object::Dict,
) -> &'a pdfcer_core::object::Dict {
    let resources = ap_dict.get(b"Resources").unwrap().as_dict().unwrap();
    let fonts = resources.get(b"Font").unwrap().as_dict().unwrap();
    let helv = fonts.get(b"Helv").unwrap();
    match helv {
        Object::Dict(d) => d,
        Object::Reference(id) => {
            let Object::Dict(d) = &doc.get(*id).unwrap().value else {
                panic!("font is not a dict");
            };
            d
        }
        other => panic!("unexpected font entry: {other:?}"),
    }
}

#[test]
fn authored_stamp_paints_framed_label_after_reload() {
    let (reloaded, page) = author_text_and_reload(&TextAnnotSpec::Stamp {
        rect: Rect {
            llx: 20.0,
            lly: 30.0,
            urx: 180.0,
            ury: 90.0,
        },
        name: StampName::Draft,
        label: None,
        color: Color::Rgb(0.8, 0.1, 0.1),
    });
    let on = render_page_with(
        &reloaded,
        &page,
        2.0,
        &RenderOptions::default().with_annotations(true),
    )
    .unwrap();
    // Both the red frame and the red label glyphs paint.
    assert!(
        reddish(&on.pixmap) > 100,
        "the stamp frame + DRAFT label must paint red (got {} px)",
        reddish(&on.pixmap)
    );
    assert_eq!(on.diagnostics.annotations_painted, 1);
}

#[test]
fn authored_sticky_note_marker_paints_but_popup_does_not() {
    let (reloaded, page) = author_text_and_reload(&TextAnnotSpec::Sticky {
        rect: Rect {
            llx: 80.0,
            lly: 50.0,
            urx: 110.0,
            ury: 80.0,
        },
        icon: pdfcer_core::annot_author::StickyIcon::Note,
        contents: "hidden note body".to_owned(),
        color: Color::Rgb(1.0, 0.9, 0.2),
        open: false,
    });
    let on = render_page_with(
        &reloaded,
        &page,
        2.0,
        &RenderOptions::default().with_annotations(true),
    )
    .unwrap();
    // The note marker paints; the /Popup companion is present but never
    // painted (Pass 6.0 X4) — so exactly one of the two annots is painted.
    assert!(non_white(&on.pixmap) > 0, "the note marker must paint");
    assert_eq!(on.diagnostics.annotations_total, 2, "note + popup");
    assert_eq!(
        on.diagnostics.annotations_painted, 1,
        "popup is not painted"
    );
}

#[test]
fn authored_highlight_paints_with_multiply_and_survives_reload() {
    use pdfcer_core::annot_author::{Quad, TextMarkupKind};

    let mut session = EditSession::new(blank_page_doc());
    session
        .add_markup(
            0,
            &MarkupSpec::TextMarkup {
                kind: TextMarkupKind::Highlight,
                quads: vec![Quad::from_rect(Rect {
                    llx: 20.0,
                    lly: 40.0,
                    urx: 180.0,
                    ury: 80.0,
                })],
                color: Color::Rgb(1.0, 1.0, 0.0),
            },
        )
        .unwrap();
    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap();
    let reloaded = Document::from_bytes(bytes).unwrap();
    let page = page_tree::pages(&reloaded).unwrap().remove(0);
    let on = render_page_with(
        &reloaded,
        &page,
        2.0,
        &RenderOptions::default().with_annotations(true),
    )
    .unwrap();
    // The highlight wash paints (yellow over white → still non-white).
    assert!(non_white(&on.pixmap) > 0, "the highlight must paint");
    assert_eq!(on.diagnostics.annotations_painted, 1);
}
