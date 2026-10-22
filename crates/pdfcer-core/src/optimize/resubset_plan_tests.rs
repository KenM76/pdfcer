#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::sync::Mutex;

use super::*;
use crate::document::Document;
use crate::optimize::resubset::{PresentGlyphs, ResubsetProgram};
use crate::optimize::resubset_list::ListEntry;
use crate::optimize::tests::{build, id, noise};
use crate::space_audit::tests::stream;

/// Each request's users with their codes.
type Asked = Vec<Vec<(ObjId, Vec<u32>)>>;

/// Cuts every program to `len` zero bytes and records what it was asked.
#[derive(Debug)]
struct Fake {
    len: usize,
    asked: Mutex<Asked>,
    present: Option<PresentGlyphs>,
}

impl Fake {
    fn new(len: usize) -> Self {
        Self {
            len,
            asked: Mutex::new(Vec::new()),
            present: None,
        }
    }

    /// A cutter that reports `present` as what every program holds.
    fn listing(present: PresentGlyphs) -> Self {
        Self {
            present: Some(present),
            ..Self::new(10)
        }
    }
}

impl FontResubsetter for Fake {
    fn resubset(
        &self,
        request: &ResubsetRequest<'_, '_>,
    ) -> Result<ResubsetProgram, ResubsetRefusal> {
        let uses = request
            .users
            .iter()
            .map(|u| (u.font, u.codes.iter().copied().collect()))
            .collect();
        self.asked.lock().unwrap().push(uses);
        Ok(ResubsetProgram::new(vec![0; self.len], 3, 10))
    }

    fn present_glyphs(&self, program: &[u8]) -> Result<PresentGlyphs, ResubsetRefusal> {
        assert_eq!(program, vec![0; self.len], "the cut program, decoded");
        self.present
            .clone()
            .ok_or_else(|| ResubsetRefusal::new("list-unsupported", "no lister"))
    }
}

/// 1 catalog (`catalog_extra`), 2 pages, 3 page drawing 4 with font 5
/// (`/BaseFont /name`), 6 its descriptor, 7 its `/FontFile2`; `extra` from 8.
fn doc(catalog_extra: &str, name: &str, content: &[u8], extra: &[Vec<u8>]) -> Document {
    let mut bodies = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>").into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R \
           /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_vec(),
        stream("", content),
        format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /{name} /FirstChar 32 \
             /LastChar 126 /FontDescriptor 6 0 R >>"
        )
        .into_bytes(),
        format!(
            "<< /Type /FontDescriptor /FontName /{name} /Flags 32 /FontBBox [0 0 1 1] \
             /ItalicAngle 0 /Ascent 1 /Descent 0 /CapHeight 1 /StemV 1 /FontFile2 7 0 R >>"
        )
        .into_bytes(),
        stream("/Length1 2000", &noise(2000)),
    ];
    bodies.extend_from_slice(extra);
    Document::from_bytes(build(&bodies)).unwrap()
}

fn run(d: &Document, fake: &Fake) -> ResubsetPlan {
    let view = d.view();
    let live: BTreeSet<ObjId> = (1..=20)
        .map(id)
        .filter(|i| view.value(*i).is_some())
        .collect();
    plan_with(&view, &live, fake, true)
}

fn tokens(p: &ResubsetPlan) -> Vec<&'static str> {
    p.refused.iter().map(|r| r.why.token).collect()
}

#[test]
fn a_drawn_truetype_font_is_cut_for_its_shown_codes() {
    let fake = Fake::new(10);
    let d = doc("", "ABCDEF+Arial", b"BT /F1 9 Tf (Hi) Tj ET", &[]);
    let p = run(&d, &fake);
    assert!(p.refused.is_empty(), "{:?}", p.refused);
    let t = &p.targets[0];
    assert_eq!((t.program, t.fonts.clone()), (id(7), vec![id(5)]));
    assert_eq!(t.before_bytes, 2000);
    assert!(t.after_bytes < 2000 && p.saving() == 2000 - t.after_bytes);
    assert_eq!((t.glyphs_kept, t.glyphs_total), (3, 10));
    assert_eq!(
        fake.asked.lock().unwrap()[0],
        vec![(id(5), vec![u32::from(b'H'), u32::from(b'i')])]
    );
}

#[test]
fn the_new_name_has_a_fresh_six_letter_tag_over_the_old_stem() {
    let d = doc("", "ABCDEF+Arial", b"BT /F1 9 Tf (a) Tj ET", &[]);
    let t = run(&d, &Fake::new(10)).targets.remove(0);
    assert_eq!(t.name_before, "ABCDEF+Arial");
    let (tag, stem) = t.name_after.split_once('+').unwrap();
    assert_eq!(stem, "Arial");
    assert!(tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()));
    assert_ne!(tag, "ABCDEF");
    let untagged = doc("", "Arial", b"BT /F1 9 Tf (a) Tj ET", &[]);
    let u = run(&untagged, &Fake::new(10)).targets.remove(0);
    assert!(u.name_after.ends_with("+Arial") && u.name_after.len() == 12);
}

#[test]
fn fresh_tag_never_reuses_a_taken_tag() {
    let mut taken = BTreeSet::new();
    let first = fresh_tag(b"same", &mut taken);
    let second = fresh_tag(b"same", &mut taken);
    assert_ne!(first, second);
    assert!(taken.contains(&first) && taken.contains(&second));
}

#[test]
fn a_cut_no_smaller_than_the_stored_program_is_refused() {
    #[derive(Debug)]
    struct Noisy;
    impl FontResubsetter for Noisy {
        fn resubset(
            &self,
            _: &ResubsetRequest<'_, '_>,
        ) -> Result<ResubsetProgram, ResubsetRefusal> {
            Ok(ResubsetProgram::new(noise(4000), 1, 2))
        }
    }
    let d = doc("", "Arial", b"BT /F1 9 Tf (a) Tj ET", &[]);
    let view = d.view();
    let live: BTreeSet<ObjId> = (1..=7).map(id).collect();
    let p = plan_with(&view, &live, &Noisy, true);
    assert!(p.targets.is_empty());
    assert_eq!(tokens(&p), vec!["not-smaller"]);
}

#[test]
fn a_program_whose_every_glyph_is_used_is_not_cut() {
    #[derive(Debug)]
    struct Whole;
    impl FontResubsetter for Whole {
        fn resubset(
            &self,
            _: &ResubsetRequest<'_, '_>,
        ) -> Result<ResubsetProgram, ResubsetRefusal> {
            Ok(ResubsetProgram::new(Vec::new(), 4, 4))
        }
    }
    let d = doc("", "Arial", b"BT /F1 9 Tf (a) Tj ET", &[]);
    let view = d.view();
    let live: BTreeSet<ObjId> = (1..=7).map(id).collect();
    let p = plan_with(&view, &live, &Whole, true);
    assert!(p.targets.is_empty());
    assert_eq!(tokens(&p), vec!["every-glyph-used"]);
}

#[test]
fn a_font_in_the_form_default_resources_is_refused() {
    let d = doc(
        "/AcroForm << /Fields [] /DR << /Font << /F1 5 0 R >> >> >>",
        "Arial",
        b"BT /F1 9 Tf (a) Tj ET",
        &[],
    );
    let p = run(&d, &Fake::new(10));
    assert!(p.targets.is_empty());
    assert_eq!(tokens(&p), vec!["acroform-dr"]);
}

#[test]
fn a_font_held_by_an_object_the_walk_did_not_see_is_refused() {
    // Object 8 holds the font but nothing draws it.
    let extra = vec![b"<< /Font << /F1 5 0 R >> >>".to_vec()];
    let d = doc("/Extra 8 0 R", "Arial", b"BT /F1 9 Tf (a) Tj ET", &extra);
    let p = run(&d, &Fake::new(10));
    assert!(p.targets.is_empty());
    assert_eq!(tokens(&p), vec!["unreferenced-user"]);
}

#[test]
fn a_program_held_by_another_descriptor_is_refused() {
    // Font 9 (not on any page) shares the program through descriptor 8.
    let extra = vec![
        b"<< /Type /FontDescriptor /FontName /Other /Flags 32 /FontFile2 7 0 R >>".to_vec(),
        b"<< /Type /Font /Subtype /TrueType /BaseFont /Other /FontDescriptor 8 0 R >>".to_vec(),
    ];
    let d = doc("/Extra 9 0 R", "Arial", b"BT /F1 9 Tf (a) Tj ET", &extra);
    let p = run(&d, &Fake::new(10));
    assert!(p.targets.is_empty());
    assert_eq!(tokens(&p), vec!["unreferenced-user"]);
}

#[test]
fn a_content_fault_refuses_every_font() {
    let d = doc("", "Arial", b"BT (a) Tj /F1 9 Tf (b) Tj ET", &[]);
    let p = run(&d, &Fake::new(10));
    assert!(p.targets.is_empty());
    assert_eq!(tokens(&p), vec!["unreadable-content"]);
}

/// A zeroed glyph is still present in the program, so `/CharSet` stays.
#[test]
fn a_descriptor_with_a_charset_is_cut() {
    let d = doc("", "Arial /CharSet (/a)", b"BT /F1 9 Tf (a) Tj ET", &[]);
    let p = run(&d, &Fake::new(10));
    assert_eq!(p.targets.len(), 1, "{:?}", p.refused);
}

/// A `/Metadata` stream (object 8) claiming PDF/A `part`, B conformance.
fn pdfa(part: &str) -> Vec<u8> {
    let xmp = format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" pdfaid:part="{part}" pdfaid:conformance="B"/></rdf:RDF></x:xmpmeta>"#
    );
    stream("/Type /Metadata /Subtype /XML", xmp.as_bytes())
}

/// [`doc`] with a Type 1 font over a `/FontFile3 /Type1C` program, in a
/// document claiming PDF/A `part`.
fn type1(part: &str, name: &str) -> Document {
    let mut bodies = vec![
        b"<< /Type /Catalog /Pages 2 0 R /Metadata 8 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R \
           /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_vec(),
        stream("", b"BT /F1 9 Tf (a) Tj ET"),
        format!(
            "<< /Type /Font /Subtype /Type1 /BaseFont /{name} /FirstChar 32 \
             /LastChar 126 /FontDescriptor 6 0 R >>"
        )
        .into_bytes(),
        format!(
            "<< /Type /FontDescriptor /FontName /{name} /Flags 32 /FontBBox [0 0 1 1] \
             /ItalicAngle 0 /Ascent 1 /Descent 0 /CapHeight 1 /StemV 1 /FontFile3 7 0 R >>"
        )
        .into_bytes(),
        stream("/Subtype /Type1C", &noise(2000)),
    ];
    bodies.push(pdfa(part));
    Document::from_bytes(build(&bodies)).unwrap()
}

/// PDF/A-1 requires `/CharSet` on a Type 1 subset (ISO 19005-1 §6.3.5);
/// the cut would make an untagged font a subset without one, so it gains
/// one naming every glyph the program holds, `.notdef` left out.
#[test]
fn a_pdfa1_untagged_type1_font_gains_a_charset() {
    let names = [b".notdef".to_vec(), b"a".to_vec(), b"b".to_vec()];
    let fake = Fake::listing(PresentGlyphs::Names(names.into()));
    let p = run(&type1("1", "Helv"), &fake);
    assert!(p.refused.is_empty(), "{:?}", p.refused);
    let t = &p.targets[0];
    assert_eq!(t.listed, [id(6)]);
    assert_eq!(t.lists[0].entry, ListEntry::CharSet(b"/a/b".to_vec()));
}

/// A cutter that cannot say what a program holds refuses the font.
#[test]
fn a_pdfa1_list_the_cutter_cannot_read_is_refused() {
    let p = run(&type1("1", "Helv"), &Fake::new(10));
    assert!(p.targets.is_empty());
    assert_eq!(tokens(&p), ["list-unsupported"]);
}

/// A name-keyed list is never written from CIDs.
#[test]
fn a_type1_font_over_a_cid_program_is_refused() {
    let fake = Fake::listing(PresentGlyphs::Cids([0, 1].into()));
    let p = run(&type1("1", "Helv"), &fake);
    assert_eq!(tokens(&p), ["pdfa1-list-owed"]);
}

/// A name a `/CharSet` cannot carry refuses rather than writing a broken one.
#[test]
fn a_glyph_name_with_a_delimiter_is_refused() {
    let names = [b"a".to_vec(), b"b(c".to_vec()];
    let fake = Fake::listing(PresentGlyphs::Names(names.into()));
    let p = run(&type1("1", "Helv"), &fake);
    assert_eq!(tokens(&p), ["pdfa1-list-owed"]);
}

/// A part pdfcer cannot read is held to the PDF/A-1 rule.
#[test]
fn an_unreadable_pdfa_part_is_held_to_the_pdfa1_rule() {
    let fake = Fake::listing(PresentGlyphs::Names([b"a".to_vec()].into()));
    let p = run(&type1("x1", "Helv"), &fake);
    assert_eq!(p.targets[0].listed, [id(6)], "{:?}", p.refused);
}

#[test]
fn a_pdfa1_type1_font_with_its_charset_is_cut() {
    let p = run(&type1("1", "Helv /CharSet (/a)"), &Fake::new(10));
    assert_eq!(p.targets.len(), 1, "{:?}", p.refused);
}

/// A tagged font is a subset already; the cut leaves its lists as they were.
#[test]
fn a_pdfa1_tagged_type1_font_is_cut() {
    let p = run(&type1("1", "ABCDEF+Helv"), &Fake::new(10));
    assert_eq!(p.targets.len(), 1, "{:?}", p.refused);
}

/// PDF/A-2, -3 and -4 require no list (ISO 19005-2 §6.2.11.4.2).
#[test]
fn a_pdfa2_3_or_4_untagged_type1_font_is_cut() {
    for part in ["2", "3", "4"] {
        let p = run(&type1(part, "Helv"), &Fake::new(10));
        assert_eq!(p.targets.len(), 1, "part {part}: {:?}", p.refused);
        assert!(p.targets[0].listed.is_empty(), "part {part}");
    }
}

/// A simple TrueType font needs no list under any part.
#[test]
fn a_pdfa1_untagged_truetype_font_is_cut() {
    let d = doc(
        "/Metadata 8 0 R",
        "Arial",
        b"BT /F1 9 Tf (a) Tj ET",
        &[pdfa("1")],
    );
    let p = run(&d, &Fake::new(10));
    assert_eq!(p.targets.len(), 1, "{:?}", p.refused);
}

/// A PDF/A-1 document drawing CID 1 through Type0 font 5 over a CIDFont
/// (object 9, `/Subtype cid_font`), descriptor 6 holding program 7 under
/// `file`, metadata 8; `extra` from 10.
fn type0(cid_font: &str, file: &str, extra: &[Vec<u8>]) -> Document {
    let program = if file.starts_with("FontFile3") {
        "/Subtype /CIDFontType0C"
    } else {
        ""
    };
    let mut bodies = vec![
        b"<< /Type /Catalog /Pages 2 0 R /Metadata 8 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R \
           /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_vec(),
        stream("", b"BT /F1 9 Tf <0001> Tj ET"),
        b"<< /Type /Font /Subtype /Type0 /BaseFont /Arial /Encoding /Identity-H \
           /DescendantFonts [9 0 R] >>"
            .to_vec(),
        format!(
            "<< /Type /FontDescriptor /FontName /Arial /Flags 4 /FontBBox [0 0 1 1] \
             /ItalicAngle 0 /Ascent 1 /Descent 0 /CapHeight 1 /StemV 1 /{file} >>"
        )
        .into_bytes(),
        stream(program, &noise(2000)),
        pdfa("1"),
        format!(
            "<< /Type /Font /Subtype {cid_font} /BaseFont /Arial \
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
             /FontDescriptor 6 0 R >>"
        )
        .into_bytes(),
    ];
    bodies.extend_from_slice(extra);
    Document::from_bytes(build(&bodies)).unwrap()
}

fn cid_set(p: &ResubsetPlan) -> Vec<u8> {
    assert!(p.refused.is_empty(), "{:?}", p.refused);
    let t = &p.targets[0];
    assert_eq!(t.listed, [id(6)]);
    match &t.lists[0].entry {
        ListEntry::CidSet(bits) => bits.clone(),
        other => panic!("expected a /CIDSet, got {other:?}"),
    }
}

/// Under an identity map a CIDFontType2 holds every CID below `numGlyphs`
/// (spec RAG `pdfa__ref__font_subset_cidset_charset.md` §3.2), MSB first.
#[test]
fn a_pdfa1_identity_cidfonttype2_gains_every_cid_below_num_glyphs() {
    for map in ["", "/CIDToGIDMap /Identity"] {
        let d = type0(&format!("/CIDFontType2 {map}"), "FontFile2 7 0 R", &[]);
        let p = run(&d, &Fake::listing(PresentGlyphs::Count(10)));
        assert_eq!(cid_set(&p), [0xFF, 0xC0], "map {map:?}");
    }
}

/// A map stream: a CID whose glyph exists is present, even glyph 0; one
/// mapped past `numGlyphs` is not, nor is a CID past the map's end.
#[test]
fn a_pdfa1_mapped_cidfonttype2_lists_the_cids_whose_glyph_exists() {
    let map = stream("", &[0, 0, 0, 3, 0, 0, 0, 10, 0, 1]);
    let d = type0(
        "/CIDFontType2 /CIDToGIDMap 10 0 R",
        "FontFile2 7 0 R",
        &[map],
    );
    let p = run(&d, &Fake::listing(PresentGlyphs::Count(10)));
    assert_eq!(cid_set(&p), [0b1110_1000]);
}

/// A CID-keyed CFF lists its charset CIDs; a name-keyed one under a
/// CIDFontType0 holds CID 0 only (§3.3).
#[test]
fn a_pdfa1_cidfonttype0_lists_its_charset_or_cid_zero() {
    let d = type0("/CIDFontType0", "FontFile3 7 0 R", &[]);
    let cids = Fake::listing(PresentGlyphs::Cids([0, 1, 9].into()));
    assert_eq!(cid_set(&run(&d, &cids)), [0xC0, 0x40]);
    let names = Fake::listing(PresentGlyphs::Names([b"a".to_vec()].into()));
    assert_eq!(cid_set(&run(&d, &names)), [0x80]);
}

/// A program kind that disagrees with the CIDFont's type refuses.
#[test]
fn a_cidfonttype2_over_a_cff_program_is_refused() {
    let d = type0("/CIDFontType2", "FontFile2 7 0 R", &[]);
    let p = run(&d, &Fake::listing(PresentGlyphs::Cids([0].into())));
    assert_eq!(tokens(&p), ["pdfa1-list-owed"]);
}

/// A CIDFont that already has a `/CIDSet` keeps it as it is.
#[test]
fn a_pdfa1_cidfont_with_a_cidset_gains_nothing() {
    let set = stream("", &[0xFF]);
    let d = type0("/CIDFontType2", "FontFile2 7 0 R /CIDSet 10 0 R", &[set]);
    let p = run(&d, &Fake::listing(PresentGlyphs::Count(10)));
    assert!(p.targets[0].listed.is_empty(), "{:?}", p.refused);
}
