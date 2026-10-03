//! Typing a character an `Identity-H` CIDFontType2 subset has no glyph for,
//! by appending it from the installed face the subset was cut from
//! (decision 173, composite route). `cid-subset.ttf` is
//! `tools/gen-augment-face-fixtures.py`'s cmap-less `.notdef A B C` cut from
//! `face.ttf` (`pdfcerAugFace`), which also draws D, E and É.
//!
//! Objects: 5 Type0, 6 CIDFontType2, 7 descriptor, 8 FontFile2,
//! 9 ToUnicode, 10 a `/CIDToGIDMap` stream, 11 a `/CIDSet`, 12-14 a second
//! font sharing object 8.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Read;

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

#[derive(Default, Clone, Copy)]
struct Shape {
    /// CIDs 10-12 reach GIDs 1-3 through a `/CIDToGIDMap` stream.
    map: bool,
    cid_set: bool,
    /// A second font's descriptor points at the same `FontFile2`.
    shared: bool,
    cff: bool,
    /// `/Encoding` other than `/Identity-H`.
    encoding: Option<&'static str>,
    /// The program is `subset.ttf`, which keeps its `cmap`.
    cmap: bool,
    /// The catalog's XMP claims PDF/A-2b (object 15).
    pdfa: bool,
}

const PDFA_XMP: &str = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
    xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description \
    xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\"><pdfaid:part>2</pdfaid:part>\
    <pdfaid:conformance>B</pdfaid:conformance></rdf:Description></rdf:RDF></x:xmpmeta>";

const TO_UNICODE: &str = "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
    /CMapName /pdfcer-aug def 1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
    3 beginbfchar <{a}> <0041> <{b}> <0042> <{c}> <0043> endbfchar\n\
    endcmap CMapName currentdict /CMap defineresource pop end end";

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut s = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    s.extend_from_slice(data);
    s.extend_from_slice(b"\nendstream");
    s
}

fn doc(shape: Shape) -> Document {
    let first: u16 = if shape.map { 10 } else { 1 };
    let codes: Vec<String> = (first..first + 3).map(|c| format!("{c:04X}")).collect();
    let content = format!("BT /F0 24 Tf 10 40 Td <{}> Tj ET", codes.concat());
    let to_unicode = TO_UNICODE
        .replace("{a}", &codes[0])
        .replace("{b}", &codes[1])
        .replace("{c}", &codes[2]);
    let program = fixture(if shape.cmap {
        "subset.ttf"
    } else {
        "cid-subset.ttf"
    });
    let encoding = shape.encoding.unwrap_or("/Identity-H");
    let (subtype, file) = if shape.cff {
        ("CIDFontType0", "FontFile3")
    } else {
        ("CIDFontType2", "FontFile2")
    };
    let map = if shape.map { "10 0 R" } else { "/Identity" };
    let cid_set = if shape.cid_set { "/CIDSet 11 0 R" } else { "" };
    let fonts = if shape.shared {
        "/F0 5 0 R /F1 12 0 R"
    } else {
        "/F0 5 0 R"
    };
    let mut map_bytes = vec![0u8; 2 * 13];
    for gid in 1..=3u8 {
        map_bytes[2 * (9 + usize::from(gid)) + 1] = gid;
    }
    let bits: &[u8] = if shape.map { &[0x80, 0x38] } else { &[0xF0] };
    let program_dict = if shape.cff {
        "/Subtype /CIDFontType0C".to_owned()
    } else {
        format!("/Length1 {}", program.len())
    };
    let catalog = if shape.pdfa {
        "<< /Type /Catalog /Pages 2 0 R /Metadata 15 0 R >>"
    } else {
        "<< /Type /Catalog /Pages 2 0 R >>"
    };
    let mut objs: Vec<Vec<u8>> = vec![
        catalog.as_bytes().to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] \
             /Resources << /Font << {fonts} >> >> /Contents 4 0 R >>"
        )
        .into_bytes(),
        stream("", content.as_bytes()),
        format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /ABCDEF+pdfcerAugFace \
             /Encoding {encoding} /DescendantFonts [6 0 R] /ToUnicode 9 0 R >>"
        )
        .into_bytes(),
        format!(
            "<< /Type /Font /Subtype /{subtype} /BaseFont /ABCDEF+pdfcerAugFace \
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
             /W [{first} [600 620 640]] /CIDToGIDMap {map} /FontDescriptor 7 0 R >>"
        )
        .into_bytes(),
        format!(
            "<< /Type /FontDescriptor /FontName /ABCDEF+pdfcerAugFace /Flags 4 \
             /FontBBox [0 0 600 700] /ItalicAngle 0 /Ascent 700 /Descent 0 \
             /CapHeight 700 /StemV 80 /{file} 8 0 R {cid_set} >>"
        )
        .into_bytes(),
        stream(&program_dict, &program),
        stream("", to_unicode.as_bytes()),
        stream("", &map_bytes),
        stream("", bits),
        b"<< /Type /Font /Subtype /Type0 /BaseFont /GHIJKL+pdfcerAugFace \
           /Encoding /Identity-H /DescendantFonts [13 0 R] >>"
            .to_vec(),
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /GHIJKL+pdfcerAugFace \
           /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
           /FontDescriptor 14 0 R >>"
            .to_vec(),
        b"<< /Type /FontDescriptor /FontName /GHIJKL+pdfcerAugFace /Flags 4 \
           /FontBBox [0 0 600 700] /ItalicAngle 0 /Ascent 700 /Descent 0 \
           /CapHeight 700 /StemV 80 /FontFile2 8 0 R >>"
            .to_vec(),
    ];
    if shape.pdfa {
        objs.push(stream("/Type /Metadata /Subtype /XML", PDFA_XMP.as_bytes()));
    }
    Document::from_bytes(assemble(&objs)).unwrap()
}

fn assemble(objs: &[Vec<u8>]) -> Vec<u8> {
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
    out
}

fn opts() -> EditOptions {
    let mut faces = InstalledFaceAugmenter::new();
    faces.insert("pdfcerAugFace", FontData::new(fixture("face.ttf")));
    EditOptions::default()
        .with_embedded_glyphs(&EmbeddedProgramGlyphs)
        .with_subset_augment(SubsetAugment::new(Box::leak(Box::new(faces))))
}

fn edit(doc: &Document, replace: &str) -> Result<text_edit::EditOutcome, String> {
    text_edit::edit_text(doc, &EditRequest::find_replace(0, "ABC", replace), &opts())
        .map_err(|e| e.to_string())
}

/// `ABC` → `ABD` under `mode`, and the new `FontFile2`'s data.
fn edit_program(
    shape: Shape,
    mode: text_edit::CidFontProgram,
) -> (text_edit::EditOutcome, Vec<u8>) {
    let base = doc(shape);
    let opts = opts().with_cid_font_program(mode);
    let out = text_edit::edit_text(&base, &EditRequest::find_replace(0, "ABC", "ABD"), &opts)
        .expect("D is appended");
    assert_eq!(notdefs(&out.bytes), 0, "D reaches a real glyph");
    assert!(page_text(&out.bytes).contains("ABD"));
    let descriptor = newest(
        &out.bytes,
        referent(&newest(&out.bytes, 6), "/FontDescriptor"),
    );
    let program = referent(&descriptor, "/FontFile2");
    assert_ne!(program, 8);
    let data = stream_data(&out.bytes, program);
    (out, data)
}

/// Whether the sfnt's table directory names a `cmap`.
fn has_cmap(program: &[u8]) -> bool {
    let n = usize::from(u16::from_be_bytes([program[4], program[5]]));
    (0..n).any(|i| &program[12 + 16 * i..16 + 16 * i] == b"cmap")
}

fn notdefs(bytes: &[u8]) -> usize {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let page = page_tree::pages(&doc).unwrap().remove(0);
    render_page_with(&doc, &page, 1.0, &RenderOptions::default())
        .unwrap()
        .diagnostics
        .glyphs_notdef
}

fn page_text(bytes: &[u8]) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let page = page_tree::pages(&doc).unwrap().remove(0);
    pdfcer_core::text_extract::extract_page(&doc, &page, 0, &Default::default())
        .unwrap()
        .sourced_text()
}

/// The newest revision of object `id`, as whitespace-normalised text.
fn newest(bytes: &[u8], id: u32) -> String {
    let s = String::from_utf8_lossy(bytes);
    let at = s.rfind(&format!("\n{id} 0 obj")).expect("object");
    let end = at + s[at..].find("endobj").unwrap();
    s[at..end].split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The object `key` refers to in `dict_text`, e.g. `/CIDSet 17 0 R` → 17.
fn referent(dict_text: &str, key: &str) -> u32 {
    let at = dict_text.find(&format!("{key} ")).expect(key) + key.len() + 1;
    dict_text[at..].split(' ').next().unwrap().parse().unwrap()
}

/// The decoded data of the newest revision of stream `id`, Flate or plain.
fn stream_data(bytes: &[u8], id: u32) -> Vec<u8> {
    let find = |from: usize, needle: &[u8]| {
        from + bytes[from..]
            .windows(needle.len())
            .position(|w| w == needle)
            .expect("marker")
    };
    let header = format!("\n{id} 0 obj");
    let at = bytes
        .windows(header.len())
        .rposition(|w| w == header.as_bytes())
        .expect("object");
    let start = find(at, b"stream") + "stream".len();
    let start = start + if bytes[start] == b'\r' { 2 } else { 1 };
    let raw = bytes[start..find(start, b"endstream")].trim_ascii_end();
    if !bytes[at..start].windows(11).any(|w| w == b"FlateDecode") {
        return raw.to_vec();
    }
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(raw)
        .read_to_end(&mut out)
        .unwrap();
    out
}

fn appended<'a>(out: &'a [u8], base: &Document) -> &'a [u8] {
    assert!(out.starts_with(base.bytes()), "an incremental save");
    &out[base.bytes().len()..]
}

fn redefines(section: &[u8], id: u32) -> bool {
    let needle = format!("\n{id} 0 obj");
    section
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[test]
fn an_identity_subset_gains_the_glyph_at_cid_equal_to_its_gid() {
    let base = doc(Shape::default());
    let out = edit(&base, "ABD").expect("D is appended");
    assert_eq!(notdefs(&out.bytes), 0, "D reaches a real glyph");
    assert!(page_text(&out.bytes).contains("ABD"));
    let descendant = newest(&out.bytes, 6);
    assert!(descendant.contains("4 [660]"), "{descendant}");
    assert!(
        descendant.contains("/CIDToGIDMap /Identity"),
        "{descendant}"
    );
    let section = appended(&out.bytes, &base);
    assert!(!redefines(section, 8), "the old program is untouched");
    let program = referent(
        &newest(&out.bytes, referent(&descendant, "/FontDescriptor")),
        "/FontFile2",
    );
    assert_ne!(program, 8, "a new FontFile2 object");
    let d = out.report.disclosures.join("\n");
    assert!(
        d.contains("inference") && d.contains("pdfcerAugFace"),
        "{d}"
    );
}

#[test]
fn the_type0_and_descendant_take_the_same_new_tag() {
    let base = doc(Shape::default());
    let out = edit(&base, "ABD").unwrap();
    let type0 = newest(&out.bytes, 5);
    let descendant = newest(&out.bytes, 6);
    let tag = |t: &str| {
        let at = t.find("/BaseFont /").unwrap() + "/BaseFont /".len();
        t[at..at + 6].to_owned()
    };
    assert_ne!(tag(&type0), "ABCDEF", "{type0}");
    assert_eq!(tag(&type0), tag(&descendant));
    let descriptor = newest(&out.bytes, referent(&descendant, "/FontDescriptor"));
    assert!(descriptor.contains(&format!("/FontName /{}+pdfcerAugFace", tag(&type0))));
}

#[test]
fn a_stream_map_is_extended_to_send_the_new_cid_to_its_glyph() {
    let base = doc(Shape {
        map: true,
        ..Shape::default()
    });
    let out = edit(&base, "ABD").expect("D is appended");
    assert_eq!(notdefs(&out.bytes), 0);
    assert!(page_text(&out.bytes).contains("ABD"));
    let descendant = newest(&out.bytes, 6);
    let map_id = referent(&descendant, "/CIDToGIDMap");
    assert_ne!(map_id, 10, "copy-on-write: {descendant}");
    let map = stream_data(&out.bytes, map_id);
    assert_eq!(&map[8..10], &[0, 4], "CID 4 → GID 4, the first free CID");
    assert_eq!(&map[20..26], &[0, 1, 0, 2, 0, 3], "the old entries kept");
    assert!(descendant.contains("4 [660]"), "{descendant}");
    assert!(!redefines(appended(&out.bytes, &base), 10));
    let d = out.report.disclosures.join("\n");
    assert!(d.contains("/CIDToGIDMap"), "{d}");
}

#[test]
fn a_cid_set_gains_the_new_cids_bit_in_a_new_stream() {
    let base = doc(Shape {
        cid_set: true,
        ..Shape::default()
    });
    let out = edit(&base, "ABD").unwrap();
    let descriptor = newest(
        &out.bytes,
        referent(&newest(&out.bytes, 6), "/FontDescriptor"),
    );
    let set = referent(&descriptor, "/CIDSet");
    assert_ne!(set, 11);
    assert_eq!(stream_data(&out.bytes, set), vec![0xF8], "CIDs 0-4");
    assert!(!redefines(appended(&out.bytes, &base), 11));
}

#[test]
fn a_program_another_font_shares_is_copied_not_changed() {
    let base = doc(Shape {
        shared: true,
        ..Shape::default()
    });
    let out = edit(&base, "ABD").expect("copy-on-write");
    let section = appended(&out.bytes, &base);
    for id in [8, 12, 13, 14] {
        assert!(!redefines(section, id), "object {id} is untouched");
    }
    assert_eq!(notdefs(&out.bytes), 0);
}

#[test]
fn the_new_program_leaves_out_the_subsets_cmap_by_default() {
    let cmap = Shape {
        cmap: true,
        ..Shape::default()
    };
    assert!(has_cmap(&fixture("subset.ttf")));
    let (out, program) = edit_program(cmap, text_edit::CidFontProgram::StripCmap);
    assert!(!has_cmap(&program));
    let d = out.report.disclosures.join("\n");
    assert!(d.contains("leaves out the subset's cmap table"), "{d}");
    let (out, program) = edit_program(cmap, text_edit::CidFontProgram::Off);
    assert!(!has_cmap(&program), "Off governs route B only");
    assert!(!out.report.disclosures.join("\n").contains("PDF/A"));
    let (out, _) = edit_program(Shape::default(), text_edit::CidFontProgram::StripCmap);
    let d = out.report.disclosures.join("\n");
    assert!(!d.contains("cmap table"), "no cmap, nothing to say: {d}");
}

#[test]
fn share_keeps_the_cmap_and_states_the_nonconformance() {
    let shape = Shape {
        cmap: true,
        ..Shape::default()
    };
    let (out, program) = edit_program(shape, text_edit::CidFontProgram::ShareStream);
    assert!(has_cmap(&program));
    let d = out.report.disclosures.join("\n");
    assert!(
        d.contains("keeps the subset's cmap table") && d.contains("§9.9"),
        "{d}"
    );
}

#[test]
fn a_pdfa_claim_drops_the_cmap_even_when_sharing_is_asked_for() {
    let shape = Shape {
        cmap: true,
        pdfa: true,
        ..Shape::default()
    };
    let (out, program) = edit_program(shape, text_edit::CidFontProgram::ShareStream);
    assert!(!has_cmap(&program));
    let d = out.report.disclosures.join("\n");
    assert!(d.contains("claims PDF/A"), "{d}");
}

#[test]
fn a_cff_descendant_is_refused_by_name() {
    let err = edit(
        &doc(Shape {
            cff: true,
            ..Shape::default()
        }),
        "ABD",
    )
    .unwrap_err();
    assert!(err.contains("CFF-based CIDFontType0"), "{err}");
}

#[test]
fn a_cmap_other_than_identity_h_is_refused_by_name() {
    let shape = Shape {
        encoding: Some("/UniGB-UCS2-H"),
        ..Shape::default()
    };
    let err = edit(&doc(shape), "ABD").unwrap_err();
    assert!(
        err.contains("only an /Identity-H composite font can be extended"),
        "{err}"
    );
    assert!(!err.contains("installed font"), "not offered: {err}");
}

#[test]
fn without_the_setting_the_missing_glyph_is_refused() {
    let plain = EditOptions::default().with_embedded_glyphs(&EmbeddedProgramGlyphs);
    let err = text_edit::edit_text(
        &doc(Shape::default()),
        &EditRequest::find_replace(0, "ABC", "ABD"),
        &plain,
    )
    .unwrap_err()
    .to_string();
    assert!(!err.contains("installed font"), "{err}");
}

#[test]
fn a_second_session_edit_replaces_what_the_first_minted_and_undo_nets_to_nothing() {
    let doc = doc(Shape {
        map: true,
        cid_set: true,
        ..Shape::default()
    });
    let before = doc.bytes().to_vec();
    let mut session = EditSession::new(doc);
    let o = opts();
    session
        .edit_text(&EditRequest::find_replace(0, "ABC", "ABD"), &o)
        .unwrap();
    session
        .edit_text(&EditRequest::find_replace(0, "ABD", "ABDE"), &o)
        .unwrap();
    let saved = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0;
    let s = String::from_utf8_lossy(&saved);
    assert_eq!(s.matches("/Length1").count(), 2, "one new program");
    assert_eq!(notdefs(&saved), 0);
    assert!(page_text(&saved).contains("ABDE"));
    session.undo().unwrap();
    session.undo().unwrap();
    let reverted = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0;
    assert_eq!(reverted, before, "undo nets to nothing");
}

#[test]
fn the_preview_draws_through_the_new_program_and_map() {
    use pdfcer_render::edit_preview::preview_outlines;
    use pdfcer_render::font::GlyphSource;
    for map in [false, true] {
        let session = EditSession::new(doc(Shape {
            map,
            ..Shape::default()
        }));
        let preview = session
            .edit_text_preview(&EditRequest::find_replace(0, "ABC", "ABD"), &opts())
            .unwrap();
        assert!(preview.font_program.is_some());
        assert_eq!(preview.cid_to_gid.is_some(), map);
        let out = preview_outlines(
            &session.view(),
            &preview,
            &pdfcer_render::FontEnvironment::bundled(),
        );
        assert_eq!(out.source, Some(GlyphSource::Embedded));
        assert!(out.glyphs.iter().all(Option::is_some), "map {map}");
    }
}

#[test]
fn the_repertoire_offers_what_augmentation_can_add() {
    let session = EditSession::new(doc(Shape::default()));
    let on = session
        .run_repertoire_with(0, "ABC", None, &opts())
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
