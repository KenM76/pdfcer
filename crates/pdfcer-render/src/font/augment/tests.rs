use super::*;
use crate::font::program::FontProgram;

const FACE: &[u8] = include_bytes!("../../../../../fixtures/synthetic/text/augment/face.ttf");
const SUBSET: &[u8] = include_bytes!("../../../../../fixtures/synthetic/text/augment/subset.ttf");
const OTHER_FPGM: &[u8] =
    include_bytes!("../../../../../fixtures/synthetic/text/augment/subset-other-fpgm.ttf");
const EMPTY_SLOT: &[u8] =
    include_bytes!("../../../../../fixtures/synthetic/text/augment/subset-empty-slot.ttf");

fn table<'a>(font: &'a [u8], tag: &[u8; 4]) -> &'a [u8] {
    Directory::parse(font).and_then(|d| d.table(*tag)).unwrap()
}

#[test]
fn a_composite_arrives_with_its_components_and_hinting_kept() {
    let r = append_glyphs(SUBSET, FACE, 0, &['D', '\u{C9}'], Hinting::Refuse).unwrap();
    assert_eq!(
        r.added.iter().map(|a| (a.ch, a.gid)).collect::<Vec<_>>(),
        [('D', 4), ('\u{C9}', 5)]
    );
    let p = FontProgram::parse(&r.program).unwrap();
    assert_eq!(p.num_glyphs(), 8, "4 old + D, Eacute, E, acute");
    assert!(!r.instructions_stripped);
    assert_eq!(table(&r.program, b"fpgm"), table(SUBSET, b"fpgm"));
    assert_eq!(r.added[0].advance, 660);
    assert_eq!(read_u16(table(&r.program, b"post"), 32), Some(8));
    assert_eq!(
        read_u16(table(&r.program, b"maxp"), 10),
        Some(10),
        "maxCompositePoints: E 6 + acute 4"
    );
}

#[test]
fn differing_hinting_strips_by_default_and_refuses_on_request() {
    let r = append_glyphs(OTHER_FPGM, FACE, 0, &['D'], Hinting::Strip).unwrap();
    assert!(r.instructions_stripped);
    let parts = Parts::read(&r.program, 0, "result").unwrap();
    assert_eq!(glyf::instruction_len(record(&parts, 4)), 0);
    assert_eq!(
        glyf::instruction_len(record(&parts, 1)),
        3,
        "old glyphs keep theirs"
    );
    assert_eq!(
        append_glyphs(OTHER_FPGM, FACE, 0, &['D'], Hinting::Refuse),
        Err(AugmentError::HintingDiffers)
    );
}

#[test]
fn an_empty_slot_is_remapped_to_the_new_glyph() {
    let r = append_glyphs(EMPTY_SLOT, FACE, 0, &['D'], Hinting::Strip).unwrap();
    assert_eq!(r.added.iter().map(|a| a.gid).collect::<Vec<_>>(), [5]);
    let p = FontProgram::parse(&r.program).unwrap();
    assert_eq!(p.glyph_for_char('D'), Some(5));
    assert!(
        p.outline(4).unwrap().is_none(),
        "the old empty slot is kept as it was"
    );
    assert_eq!(p.glyph_for_name("D"), Some(4));
    assert_eq!(
        p.glyph_for_name("uni0044"),
        Some(5),
        "the new glyph's name is unique"
    );
}

#[test]
fn held_characters_add_nothing_and_missing_ones_refuse() {
    let r = append_glyphs(SUBSET, FACE, 0, &['A', 'B'], Hinting::Strip).unwrap();
    assert!(r.added.is_empty());
    assert_eq!(
        append_glyphs(SUBSET, FACE, 0, &['Z'], Hinting::Strip),
        Err(AugmentError::FaceLacksCharacter { ch: 'Z' })
    );
}

#[test]
fn output_is_deterministic_and_head_widens() {
    let a = append_glyphs(SUBSET, FACE, 0, &['\u{C9}'], Hinting::Strip).unwrap();
    let b = append_glyphs(SUBSET, FACE, 0, &['\u{C9}'], Hinting::Strip).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.bbox[3], 900, "the acute raises yMax");
}

#[test]
fn an_unknown_table_is_refused_by_name() {
    let dir = Directory::parse(SUBSET).unwrap();
    let mut tables: Vec<([u8; 4], Vec<u8>)> =
        dir.tables.iter().map(|(t, d)| (*t, d.to_vec())).collect();
    tables.push((*b"CBDT", vec![0; 8]));
    let odd = assemble(dir.flavor, tables);
    assert_eq!(
        append_glyphs(&odd, FACE, 0, &['D'], Hinting::Strip),
        Err(AugmentError::UnsupportedTable { tag: "CBDT".into() })
    );
}

#[test]
fn verification_catches_a_same_shape_wrong_outline() {
    let r = append_glyphs(SUBSET, FACE, 0, &['D'], Hinting::Strip).unwrap();
    let parts = Parts::read(&r.program, 0, "result").unwrap();
    let (glyf_bytes, offsets) =
        glyf::append(parts.glyf, &parts.loca[..5], &[record(&parts, 3).to_vec()]);
    let dir = Directory::parse(&r.program).unwrap();
    let mut tables: Vec<([u8; 4], Vec<u8>)> =
        dir.tables.iter().map(|(t, d)| (*t, d.to_vec())).collect();
    for (tag, data) in &mut tables {
        match &*tag {
            b"glyf" => *data = glyf_bytes.clone(),
            b"loca" => *data = glyf::write_loca(&offsets, false).0,
            _ => {}
        }
    }
    let tampered = assemble(dir.flavor, tables);
    let err = verify::check(SUBSET, &tampered, FACE, 0, &r.added, 5).unwrap_err();
    assert!(
        matches!(&err, AugmentError::VerificationFailed { detail } if detail.contains("outline")),
        "{err}"
    );
}
