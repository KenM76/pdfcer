#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;

fn face(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic/resubset")
            .join(name),
    )
    .unwrap()
}

fn set(gids: &[u16]) -> BTreeSet<u16> {
    gids.iter().copied().collect()
}

#[test]
fn integers_decode_in_every_tn5176_form() {
    assert_eq!(int(&[139]), Some(0));
    assert_eq!(int(&[32]), Some(-107));
    assert_eq!(int(&[247, 0]), Some(108));
    assert_eq!(int(&[254, 255]), Some(-1131));
    assert_eq!(int(&[28, 0x80, 0]), Some(-32768));
    assert_eq!(int(&[29, 0, 1, 0, 0]), Some(65536));
    assert_eq!(int(&[30, 0x1F]), None, "a real is not an integer");
    assert_eq!(int(&[29, 0, 1]), None, "truncated");
}

#[test]
fn a_dict_splits_into_operators_and_operand_spans() {
    // 100 200 Private; 1.5 (real) 12 6 CharstringType; trailing 5
    let dict = [239, 247, 92, 18, 30, 0x1A, 0x5F, 12, 6];
    let e = entries(&dict).unwrap();
    assert_eq!(
        (e[0].op, e[0].operands.clone()),
        (PRIVATE, vec![0..1, 1..3])
    );
    assert_eq!(
        (e[1].op, e[1].operands.clone()),
        (CHARSTRING_TYPE, vec![Range { start: 4, end: 7 }])
    );
    assert!(
        entries(&[139]).is_none(),
        "an operand after the last operator"
    );
    assert!(entries(&[255]).is_none(), "byte 255 is reserved");
}

#[test]
fn an_index_reads_its_spans_and_refuses_bad_offsets() {
    let data = [0, 0, 0, 2, 1, 1, 2, 4, b'a', b'b', b'c', 9];
    let i = read_index(&data, 2).unwrap();
    assert_eq!(i.span, 2..11);
    assert_eq!(i.items, vec![8..9, 9..11]);
    assert_eq!(read_index(&[0, 0], 0).unwrap().span, 0..2);
    assert!(
        read_index(&[0, 1, 1, 2, 1], 0).is_none(),
        "first offset is not 1"
    );
    assert!(read_index(&[0, 1, 1, 1, 9], 0).is_none(), "past the data");
    assert!(read_index(&[0, 1, 5, 1, 1], 0).is_none(), "offSize 5");
}

#[test]
fn an_offset_moves_by_the_rewrites_before_it() {
    let replaced = [(10..20, 15), (30..40, 4)];
    assert_eq!(moved(5, &replaced), Some(5));
    assert_eq!(moved(10, &replaced), Some(10), "the start of a rewrite");
    assert_eq!(moved(20, &replaced), Some(25));
    assert_eq!(moved(50, &replaced), Some(49));
    assert_eq!(moved(15, &replaced), None, "inside a rewrite");
}

/// Each glyph's charstring in `cff`.
fn charstrings(cff: &[u8]) -> Vec<Vec<u8>> {
    let font = skrifa::raw::ps::cff::CffFontRef::new_cff(cff, 0, None).unwrap();
    (0..font.num_glyphs())
        .map(|g| font.charstrings().get(g as usize).unwrap().to_vec())
        .collect()
}

#[test]
fn a_bare_cff_cut_empties_the_unused_charstrings_only() {
    let old = face("face.cff");
    let cut = zero(&old, &set(&[1, 3])).unwrap();
    assert_eq!((cut.kept, cut.total), (3, 7));
    assert!(cut.program.len() < old.len());
    let (a, b) = (charstrings(&old), charstrings(&cut.program));
    for g in 0..7 {
        if [0, 1, 3].contains(&g) {
            assert_eq!(a[g], b[g], "glyph {g} kept");
        } else {
            assert_eq!(b[g], ENDCHAR, "glyph {g} emptied");
        }
    }
}

#[test]
fn a_seac_glyph_keeps_its_base_and_accent() {
    let cut = zero(&face("face.cff"), &set(&[6])).unwrap();
    assert_eq!(cut.kept, 4, ".notdef, E, acute and Eacute");
    let b = charstrings(&cut.program);
    assert_eq!(b[1], ENDCHAR);
    assert_ne!(b[4], ENDCHAR, "E is the seac base");
    assert_ne!(b[5], ENDCHAR, "acute is the seac accent");
}

#[test]
fn an_otto_cut_rewrites_its_cff_table() {
    let old = face("face-cff.otf");
    let cut = zero(&old, &set(&[2])).unwrap();
    assert_eq!(cut.kept, 2);
    assert!(cut.program.len() < old.len());
    let dir = Directory::parse(&cut.program).unwrap();
    let b = charstrings(dir.table(*b"CFF ").unwrap());
    assert_eq!(b[1], ENDCHAR);
    assert_ne!(b[2], ENDCHAR);
}

#[test]
fn a_root_past_the_glyph_count_is_ignored() {
    let cut = zero(&face("face.cff"), &set(&[2, 900])).unwrap();
    assert_eq!(cut.kept, 2);
}
