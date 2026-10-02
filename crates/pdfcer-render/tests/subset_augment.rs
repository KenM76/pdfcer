//! Typing a character an embedded TrueType subset has no outline for, by
//! appending it from the installed face the subset was cut from
//! (decision 173). The fixtures are `tools/gen-augment-face-fixtures.py`'s:
//! `subset.ttf` holds A B C cut from `face.ttf` (`pdfcerAugFace`), which also
//! draws D and É; `face-b-differs.ttf` draws a shared glyph differently.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{self, EditOptions, EditRequest, SubsetAugment};
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::font::InstalledFaceAugmenter;
use pdfcer_render::font::embedded_glyphs::EmbeddedProgramGlyphs;
use pdfcer_render::{FontData, RenderOptions, render_page_with};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../fixtures/synthetic/text/augment/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture; run tools/gen-augment-face-fixtures.py")
}

/// One page showing `content` in `/F0`, a WinAnsi TrueType embedding
/// `subset.ttf` as `ABCDEF+pdfcerAugFace`.
fn doc_with(content: &str) -> Document {
    doc_encoded(content, "/WinAnsiEncoding")
}

fn doc_encoded(content: &str, encoding: &str) -> Document {
    let program = fixture("subset.ttf");
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] \
           /Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>"
            .to_vec(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len() + 1
        )
        .into_bytes(),
        format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+pdfcerAugFace \
             /FirstChar 65 /LastChar 67 /Widths [600 600 600] \
             /Encoding {encoding} /FontDescriptor 6 0 R >>"
        )
        .into_bytes(),
        b"<< /Type /FontDescriptor /FontName /ABCDEF+pdfcerAugFace /Flags 32 \
           /FontBBox [0 0 600 700] /ItalicAngle 0 /Ascent 700 /Descent 0 \
           /CapHeight 700 /StemV 80 /MaxWidth 600 /FontFile2 7 0 R >>"
            .to_vec(),
    ];
    let mut stream =
        format!("<< /Length {0} /Length1 {0} >>\nstream\n", program.len()).into_bytes();
    stream.extend_from_slice(&program);
    stream.extend_from_slice(b"\nendstream");
    objs.push(stream);

    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    Document::from_bytes(out).unwrap()
}

fn augmenter(face: &str) -> SubsetAugment {
    let mut faces = InstalledFaceAugmenter::new();
    faces.insert("pdfcerAugFace", FontData::new(fixture(face)));
    SubsetAugment::new(Box::leak(Box::new(faces)))
}

fn opts(face: &str) -> EditOptions {
    EditOptions::default()
        .with_embedded_glyphs(&EmbeddedProgramGlyphs)
        .with_subset_augment(augmenter(face))
}

fn notdefs(bytes: &[u8]) -> usize {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let page = page_tree::pages(&doc).unwrap().remove(0);
    render_page_with(&doc, &page, 1.0, &RenderOptions::default())
        .unwrap()
        .diagnostics
        .glyphs_notdef
}

/// Every `/BaseFont /XXXXXX+pdfcerAugFace` tag in `bytes`, in file order.
fn tags(bytes: &[u8]) -> Vec<String> {
    let s = String::from_utf8_lossy(bytes);
    s.match_indices("/BaseFont /")
        .map(|(i, m)| s[i + m.len()..i + m.len() + 6].to_owned())
        .collect()
}

#[test]
fn a_letter_the_subset_lacks_is_appended_from_the_installed_face() {
    let doc = doc_with("BT /F0 24 Tf 10 40 Td (ABC) Tj ET");
    let base = doc.bytes().to_vec();
    let out = text_edit::edit_text(
        &doc,
        &EditRequest::find_replace(0, "ABC", "ABD"),
        &opts("face.ttf"),
    )
    .expect("D is appended from the face");
    assert!(out.bytes.starts_with(&base), "an incremental save");
    assert_eq!(notdefs(&out.bytes), 0, "D reaches a real glyph");
    let all = tags(&out.bytes);
    let new = all.last().unwrap();
    assert_ne!(new, "ABCDEF", "a new program takes a new tag");
    assert!(new.bytes().all(|b| b.is_ascii_uppercase()));
    let s = String::from_utf8_lossy(&out.bytes);
    assert!(
        s.contains(&format!("/FontName /{new}+pdfcerAugFace")),
        "/FontName follows /BaseFont"
    );
    let d = out.report.disclosures.join("\n");
    assert!(
        d.contains("inference") && d.contains("pdfcerAugFace"),
        "{d}"
    );
}

#[test]
fn without_the_setting_the_missing_outline_is_refused() {
    let doc = doc_with("BT /F0 24 Tf 10 40 Td (ABC) Tj ET");
    let plain = EditOptions::default().with_embedded_glyphs(&EmbeddedProgramGlyphs);
    let err = text_edit::edit_text(&doc, &EditRequest::find_replace(0, "ABC", "ABD"), &plain)
        .unwrap_err()
        .to_string();
    assert!(err.contains("no outline"), "{err}");
    assert!(!err.contains("installed font"), "{err}");
}

#[test]
fn a_face_that_draws_a_shared_glyph_differently_is_refused() {
    let doc = doc_with("BT /F0 24 Tf 10 40 Td (ABC) Tj ET");
    let err = text_edit::edit_text(
        &doc,
        &EditRequest::find_replace(0, "ABC", "ABD"),
        &opts("face-b-differs.ttf"),
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("cannot be added from an installed font"),
        "{err}"
    );
    assert!(err.contains("differently"), "{err}");
}

#[test]
fn a_shown_code_that_would_change_is_refused() {
    let doc = doc_encoded(
        "BT /F0 24 Tf 10 40 Td (ABC) Tj 0 30 Td (\\310) Tj ET",
        "<< /BaseEncoding /WinAnsiEncoding /Differences [200 /D] >>",
    );
    let err = text_edit::edit_text(
        &doc,
        &EditRequest::find_replace(0, "ABC", "ABD"),
        &opts("face.ttf"),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("would change"), "{err}");
}

#[test]
fn a_second_session_edit_replaces_the_program_it_minted() {
    let doc = doc_with("BT /F0 24 Tf 10 40 Td (ABC) Tj ET");
    let before = doc.bytes().to_vec();
    let mut session = EditSession::new(doc);
    let o = opts("face.ttf");
    session
        .edit_text(&EditRequest::find_replace(0, "ABC", "ABD"), &o)
        .unwrap();
    session
        .edit_text(&EditRequest::find_replace(0, "ABD", "ABD\u{C9}"), &o)
        .unwrap();
    let saved = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0;
    let s = String::from_utf8_lossy(&saved);
    assert_eq!(
        s.matches("/Length1").count(),
        2,
        "the original program and one new one"
    );
    assert_eq!(notdefs(&saved), 0);

    session.undo().unwrap();
    session.undo().unwrap();
    let reverted = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0;
    assert_eq!(reverted, before, "undo nets to nothing");
}

#[test]
fn the_preview_draws_the_appended_glyph_from_the_new_program() {
    use pdfcer_render::edit_preview::preview_outlines;
    use pdfcer_render::font::GlyphSource;
    let doc = doc_with("BT /F0 24 Tf 10 40 Td (ABC) Tj ET");
    let session = EditSession::new(doc);
    let preview = session
        .edit_text_preview(
            &EditRequest::find_replace(0, "ABC", "ABD"),
            &opts("face.ttf"),
        )
        .unwrap();
    assert!(preview.font_program.is_some());
    let descriptor = preview
        .font
        .get(b"FontDescriptor")
        .and_then(|o| o.as_dict());
    let name = descriptor
        .and_then(|d| d.get(b"FontName"))
        .and_then(|o| o.as_name())
        .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned());
    assert_eq!(Some(preview.base_font.clone()), name, "inline descriptor");
    let out = preview_outlines(
        &session.view(),
        &preview,
        &pdfcer_render::FontEnvironment::bundled(),
    );
    assert_eq!(out.source, Some(GlyphSource::Embedded));
    assert!(out.glyphs.iter().all(Option::is_some), "D has an outline");
}

#[test]
fn the_repertoire_offers_what_augmentation_can_add_and_nothing_else() {
    let doc = doc_with("BT /F0 24 Tf 10 40 Td (ABC) Tj ET");
    let session = EditSession::new(doc);
    let on = session
        .run_repertoire_with(0, "ABC", None, &opts("face.ttf"))
        .unwrap()
        .accepted;
    assert!(on.contains(&'D') && on.contains(&'E'), "{on:?}");
    assert!(!on.contains(&'Z'), "the face has no Z: {on:?}");
    let plain = EditOptions::default().with_embedded_glyphs(&EmbeddedProgramGlyphs);
    let off = session
        .run_repertoire_with(0, "ABC", None, &plain)
        .unwrap()
        .accepted;
    assert!(!off.contains(&'D'), "{off:?}");
}
