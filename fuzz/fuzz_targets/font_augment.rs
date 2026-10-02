//! Fuzz target: **composite subset augmentation** (decision 173, Pass 430.1):
//! typing a character an `Identity-H` `CIDFontType2` subset lacks, appended
//! from a supplied face.
//!
//! The untrusted input is the document's side: the embedded `FontFile2`
//! program (the sfnt walk, the CID-keyed identity check and the rebuild) or
//! the `/CIDToGIDMap` stream (CID assignment and zero-extension up to
//! `MAX_MAP_BYTES`). The supplied face is the fixed fixture `face.ttf`;
//! `font_subset` already fuzzes operator-chosen font bytes.
//!
//! Contract: `edit_text` returns `Ok` or a named `Err` for any bytes, and an
//! `Ok` outcome's bytes re-open as a document.

#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::text_edit::{EditOptions, EditRequest, SubsetAugment, edit_text};
use pdfcer_render::FontData;
use pdfcer_render::font::InstalledFaceAugmenter;
use pdfcer_render::font::embedded_glyphs::EmbeddedProgramGlyphs;

const FACE: &[u8] = include_bytes!("../../fixtures/synthetic/text/augment/face.ttf");
const SUBSET: &[u8] = include_bytes!("../../fixtures/synthetic/text/augment/cid-subset.ttf");
static FACES: OnceLock<InstalledFaceAugmenter> = OnceLock::new();
const REPLACEMENTS: [&str; 4] = ["ABD", "ABE", "AB\u{c9}", "DEA"];

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut s = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    s.extend_from_slice(data);
    s.extend_from_slice(b"\nendstream");
    s
}

/// The `cid_subset_augment` test document: CIDs 1-3 (or 10-12 through a
/// map stream) showing A B C.
fn document(program: &[u8], map: Option<&[u8]>) -> Vec<u8> {
    let first: u16 = if map.is_some() { 10 } else { 1 };
    let codes: Vec<String> = (first..first + 3).map(|c| format!("{c:04X}")).collect();
    let content = format!("BT /F0 24 Tf 10 40 Td <{}> Tj ET", codes.concat());
    let to_unicode = format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap \
         1 begincodespacerange <0000> <FFFF> endcodespacerange \
         3 beginbfchar <{}> <0041> <{}> <0042> <{}> <0043> endbfchar \
         endcmap CMapName currentdict /CMap defineresource pop end end",
        codes[0], codes[1], codes[2]
    );
    let map_ref = if map.is_some() { "10 0 R" } else { "/Identity" };
    let objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] \
           /Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>"
            .to_vec(),
        stream("", content.as_bytes()),
        b"<< /Type /Font /Subtype /Type0 /BaseFont /ABCDEF+pdfcerAugFace \
           /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 9 0 R >>"
            .to_vec(),
        format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /ABCDEF+pdfcerAugFace \
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
             /W [{first} [600 620 640]] /CIDToGIDMap {map_ref} /FontDescriptor 7 0 R >>"
        )
        .into_bytes(),
        b"<< /Type /FontDescriptor /FontName /ABCDEF+pdfcerAugFace /Flags 4 \
           /FontBBox [0 0 600 700] /ItalicAngle 0 /Ascent 700 /Descent 0 \
           /CapHeight 700 /StemV 80 /FontFile2 8 0 R >>"
            .to_vec(),
        stream(&format!("/Length1 {}", program.len()), program),
        stream("", to_unicode.as_bytes()),
        stream("", map.unwrap_or(b"")),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objs.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

fuzz_target!(|data: &[u8]| {
    let Some((&steer, rest)) = data.split_first() else {
        return;
    };
    // Bit 0: the fuzz bytes are the map stream (over the real subset)
    // rather than the program (over an Identity map).
    let pdf = if steer & 1 == 1 {
        document(SUBSET, Some(rest))
    } else {
        document(rest, None)
    };
    let Ok(doc) = Document::from_bytes(pdf) else {
        return;
    };
    let faces = FACES.get_or_init(|| {
        let mut faces = InstalledFaceAugmenter::new();
        faces.insert("pdfcerAugFace", FontData::from_static(FACE));
        faces
    });
    let opts = EditOptions::default()
        .with_embedded_glyphs(&EmbeddedProgramGlyphs)
        .with_subset_augment(SubsetAugment::new(faces));
    let replace = REPLACEMENTS[usize::from(steer >> 1) % REPLACEMENTS.len()];
    if let Ok(out) = edit_text(&doc, &EditRequest::find_replace(0, "ABC", replace), &opts) {
        assert!(
            Document::from_bytes(out.bytes).is_ok(),
            "an augmented save must re-open"
        );
    }
});
