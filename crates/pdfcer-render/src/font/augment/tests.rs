use super::identity::OutlineCheck;
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

const FACE_B_DIFFERS: &[u8] =
    include_bytes!("../../../../../fixtures/synthetic/text/augment/face-b-differs.ttf");

#[test]
fn the_cut_from_face_passes_the_identity_check() {
    identity::check(SUBSET, FACE, 0, &['D', '\u{C9}'], OutlineCheck::AllShared).unwrap();
}

#[test]
fn a_differing_shared_outline_refuses_unless_it_is_out_of_scope() {
    assert_eq!(
        identity::check(SUBSET, FACE_B_DIFFERS, 0, &['D'], OutlineCheck::AllShared),
        Err(AugmentError::OutlineMismatch { ch: 'B', gid: 2 })
    );
    identity::check(
        SUBSET,
        FACE_B_DIFFERS,
        0,
        &['D'],
        OutlineCheck::ShownOnly(&['A']),
    )
    .unwrap();
    assert_eq!(
        identity::check(
            SUBSET,
            FACE_B_DIFFERS,
            0,
            &['D'],
            OutlineCheck::ShownOnly(&[' '])
        ),
        Err(AugmentError::IdentityUnproven)
    );
}

#[test]
fn a_character_the_face_lacks_refuses_before_any_surgery() {
    assert_eq!(
        identity::check(SUBSET, FACE, 0, &['Z'], OutlineCheck::AllShared),
        Err(AugmentError::FaceLacksCharacter { ch: 'Z' })
    );
}

#[test]
fn a_face_is_a_candidate_only_under_the_untagged_name() {
    assert_eq!(
        identity::candidate_index(FACE, "ABCDEF+pdfcerAugFace"),
        Some(0)
    );
    assert_eq!(identity::candidate_index(FACE, "pdfcerAugFace"), Some(0));
    assert_eq!(identity::candidate_index(FACE, "ABCDEF+Other"), None);
    assert_eq!(
        identity::candidate_index(FACE, "abcdef+pdfcerAugFace"),
        None
    );
}

#[test]
fn an_empty_slot_is_no_evidence_either_way() {
    identity::check(EMPTY_SLOT, FACE, 0, &['D'], OutlineCheck::AllShared).unwrap();
}

#[test]
fn a_tag_must_be_six_capitals() {
    assert_eq!(identity::candidate_index(FACE, "ABC+pdfcerAugFace"), None);
}

#[test]
fn a_differing_shared_advance_refuses() {
    let dir = Directory::parse(FACE).unwrap();
    let hmtx = dir.table(*b"hmtx").unwrap();
    let at = hmtx.as_ptr() as usize - FACE.as_ptr() as usize + 2 * 4;
    let mut face = FACE.to_vec();
    face[at] ^= 0x01;
    assert_eq!(
        identity::check(SUBSET, &face, 0, &['D'], OutlineCheck::AllShared),
        Err(AugmentError::AdvanceMismatch { ch: 'B', gid: 2 })
    );
}

#[test]
fn a_restricted_face_or_subset_refuses_under_r109() {
    for restricted in [true, false] {
        let base = if restricted { FACE } else { SUBSET };
        let dir = Directory::parse(base).unwrap();
        let os2 = dir.table(*b"OS/2").unwrap();
        let at = os2.as_ptr() as usize - base.as_ptr() as usize + 8;
        let mut bytes = base.to_vec();
        bytes[at..at + 2].copy_from_slice(&2u16.to_be_bytes());
        let (subset, face) = if restricted {
            (SUBSET, &bytes[..])
        } else {
            (&bytes[..], FACE)
        };
        assert!(matches!(
            identity::check(subset, face, 0, &['D'], OutlineCheck::AllShared),
            Err(AugmentError::EmbeddingNotPermitted { .. })
        ));
    }
}
