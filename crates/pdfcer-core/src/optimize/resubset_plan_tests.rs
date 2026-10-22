#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::sync::Mutex;

use super::*;
use crate::document::Document;
use crate::optimize::resubset::ResubsetProgram;
use crate::optimize::tests::{build, id, noise};
use crate::space_audit::tests::stream;

/// Each request's users with their codes.
type Asked = Vec<Vec<(ObjId, Vec<u32>)>>;

/// Cuts every program to `len` zero bytes and records what it was asked.
#[derive(Debug)]
struct Fake {
    len: usize,
    asked: Mutex<Asked>,
}

impl Fake {
    fn new(len: usize) -> Self {
        Self {
            len,
            asked: Mutex::new(Vec::new()),
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
