#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeSet;

use pdfcer_core::document::Document;
use pdfcer_core::object::ObjId;
use pdfcer_core::optimize::resubset::FontUse;

use super::*;
use crate::font::augment::glyf::{read_loca, record};
use crate::font::sfnt::Directory;

/// `.notdef A B C D E acute Eacute`; Eacute is E + acute; short `loca`.
const FACE: &[u8] = include_bytes!("../../../../../fixtures/synthetic/text/augment/face.ttf");

const FONT: ObjId = ObjId {
    num: 2,
    generation: 0,
};

/// Object 1 a catalog, 2 `font`, then `rest` from 3.
fn doc(font: &str, rest: &[Vec<u8>]) -> Document {
    let mut bodies = vec![b"<< /Type /Catalog >>".to_vec(), font.as_bytes().to_vec()];
    bodies.extend_from_slice(rest);
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut at = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        at.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    let n = bodies.len() + 1;
    pdf.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f\r\n").as_bytes());
    for off in at {
        pdf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(pdf).unwrap()
}

const WIN_ANSI: &str =
    "<< /Type /Font /Subtype /TrueType /BaseFont /Face /Encoding /WinAnsiEncoding >>";

fn cut(d: &Document, program: &[u8], codes: &[u32]) -> Result<ResubsetProgram, ResubsetRefusal> {
    let view = d.view();
    let codes: BTreeSet<u32> = codes.iter().copied().collect();
    let users = [FontUse::new(FONT, &codes)];
    GLYPH_ZERO_RESUBSETTER.resubset(&ResubsetRequest::new(&view, program, &users))
}

/// Each glyph's record in `font`.
fn records(font: &[u8]) -> Vec<Vec<u8>> {
    let dir = Directory::parse(font).unwrap();
    let total = usize::from(crate::font::sfnt::read_u16(dir.table(*b"maxp").unwrap(), 4).unwrap());
    let loca = read_loca(dir.table(*b"loca").unwrap(), false, total).unwrap();
    let glyf = dir.table(*b"glyf").unwrap();
    (0..total)
        .map(|g| record(glyf, &loca, g).unwrap().to_vec())
        .collect()
}

#[test]
fn shown_glyphs_and_their_components_keep_their_records_and_the_rest_empty() {
    let d = doc(WIN_ANSI, &[]);
    let out = cut(&d, FACE, &[u32::from(b'A'), 0xC9]).unwrap();
    assert_eq!((out.glyphs_kept, out.glyphs_total), (5, 8));
    assert!(out.program.len() < FACE.len());
    let (before, after) = (records(FACE), records(&out.program));
    for (gid, name) in [
        (0, ".notdef"),
        (1, "A"),
        (5, "E"),
        (6, "acute"),
        (7, "Eacute"),
    ] {
        assert!(!before[gid].is_empty() || gid == 0, "{name} has an outline");
        assert_eq!(after[gid], before[gid], "{name} kept byte-identical");
    }
    for gid in [2, 3, 4] {
        assert!(
            after[gid].is_empty() && !before[gid].is_empty(),
            "glyph {gid}"
        );
    }
    let parsed = FontProgram::parse(&out.program).unwrap();
    assert_eq!(parsed.glyph_for_char('B'), Some(2), "cmap untouched");
}

#[test]
fn a_code_no_rung_resolves_is_refused() {
    let d = doc(WIN_ANSI, &[]);
    let err = cut(&d, FACE, &[u32::from(b'Z')]).unwrap_err();
    assert_eq!(err.token, "unmapped-code");
}

#[test]
fn a_composite_font_keeps_the_glyphs_its_cid_map_names() {
    let mut map = vec![0u8; 4];
    map[2..4].copy_from_slice(&3u16.to_be_bytes());
    let mut map_obj = format!("<< /Length {} >>\nstream\n", map.len()).into_bytes();
    map_obj.extend_from_slice(&map);
    map_obj.extend_from_slice(b"\nendstream");
    let d = doc(
        "<< /Type /Font /Subtype /Type0 /BaseFont /Face /Encoding /Identity-H \
         /DescendantFonts [3 0 R] >>",
        &[
            b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Face \
               /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
               /CIDToGIDMap 4 0 R >>"
                .to_vec(),
            map_obj,
        ],
    );
    let out = cut(&d, FACE, &[1, 900]).unwrap();
    assert_eq!(
        out.glyphs_kept, 2,
        ".notdef and C: CID 1 is glyph 3, CID 900 is past the map"
    );
    assert!(!records(&out.program)[3].is_empty());
    assert!(records(&out.program)[1].is_empty());
}

#[test]
fn a_collection_and_a_restricted_licence_are_refused() {
    let d = doc(WIN_ANSI, &[]);
    let ttc = [b"ttcf".as_slice(), &[0; 12]].concat();
    assert_eq!(cut(&d, &ttc, &[65]).unwrap_err().token, "collection");
    let mut restricted = FACE.to_vec();
    let dir = Directory::parse(FACE).unwrap();
    let os2 = dir.table(*b"OS/2").unwrap();
    let at = os2.as_ptr() as usize - FACE.as_ptr() as usize + 8;
    restricted[at..at + 2].copy_from_slice(&2u16.to_be_bytes());
    assert_eq!(
        cut(&d, &restricted, &[65]).unwrap_err().token,
        "fstype-restricted"
    );
}

/// `FACE` cut to `roots`, as a subset another tool might have embedded.
fn subset(roots: &[u16]) -> Vec<u8> {
    zero::zero(FACE, &roots.iter().copied().collect())
        .unwrap()
        .program
}

/// The offset of `tag`'s table in `font`.
fn table_at(font: &[u8], tag: [u8; 4]) -> usize {
    let dir = Directory::parse(font).unwrap();
    dir.table(tag).unwrap().as_ptr() as usize - font.as_ptr() as usize
}

#[test]
fn two_subsets_of_one_face_merge_into_one_holding_both() {
    let (a, b) = (subset(&[1]), subset(&[2, 7]));
    let out = GLYPH_ZERO_RESUBSETTER.merge(&[&a, &b]).unwrap();
    assert_eq!((out.glyphs_kept, out.glyphs_total), (5, 8));
    let (face, merged) = (records(FACE), records(&out.program));
    for gid in [1, 2, 5, 6, 7] {
        assert_eq!(
            merged[gid], face[gid],
            "glyph {gid} from one of the subsets"
        );
    }
    for gid in [3, 4] {
        assert!(merged[gid].is_empty(), "glyph {gid} neither subset carries");
    }
}

#[test]
fn subsets_whose_metrics_differ_are_refused() {
    let (a, mut b) = (subset(&[1]), subset(&[2]));
    let at = table_at(&b, *b"hmtx");
    b[at] ^= 1;
    let err = GLYPH_ZERO_RESUBSETTER.merge(&[&a, &b]).unwrap_err();
    assert_eq!(err.token, "different-face");
    assert!(err.reason.contains("hmtx"), "{}", err.reason);
}

#[test]
fn a_glyph_two_subsets_draw_differently_is_refused() {
    let (a, mut b) = (subset(&[1]), subset(&[1, 2]));
    let glyf = table_at(&b, *b"glyf");
    let dir = Directory::parse(&b).unwrap();
    let total = usize::from(crate::font::sfnt::read_u16(dir.table(*b"maxp").unwrap(), 4).unwrap());
    let loca = read_loca(dir.table(*b"loca").unwrap(), false, total).unwrap();
    let last = glyf + loca[2] - 1;
    b[last] ^= 1;
    let err = GLYPH_ZERO_RESUBSETTER.merge(&[&a, &b]).unwrap_err();
    assert_eq!(err.token, "different-face");
    assert!(err.reason.contains("glyph 1"), "{}", err.reason);
}
