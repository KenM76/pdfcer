#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::document::Document;
use crate::edit::EditSession;
use crate::font_unembed::UnembedRequest;
use crate::graph::ObjectGraph;
use crate::object::{ObjId, Object};
use crate::optimize::resubset::{
    FontResubsetter, PresentGlyphs, ResubsetFonts, ResubsetProgram, ResubsetRefusal,
    ResubsetRequest,
};
use crate::optimize::tests::{build, id, noise};
use crate::optimize::{OptimizeError, OptimizeSettings, OptimizedFile, StreamAction};
use crate::space_audit::tests::stream;
use crate::writer::{RebuildOptions, SaveOptions};

/// Cuts every program to 50 zero bytes.
#[derive(Debug)]
struct Zeros;

impl FontResubsetter for Zeros {
    fn resubset(&self, r: &ResubsetRequest<'_, '_>) -> Result<ResubsetProgram, ResubsetRefusal> {
        let kept = r.users.iter().map(|u| u.codes.len()).sum::<usize>();
        Ok(ResubsetProgram::new(
            vec![0; 50],
            u32::try_from(kept).unwrap(),
            9,
        ))
    }
}

static ZEROS: Zeros = Zeros;

/// A page drawing "Hi" in font 5 (`ABCDEF+Arial`), descriptor 6, program 7.
fn session() -> EditSession {
    session_with("", &[])
}

/// [`session`] with `catalog_extra` in the catalog and `extra` from 8.
fn session_with(catalog_extra: &str, extra: &[Vec<u8>]) -> EditSession {
    let mut bodies = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>").into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R \
           /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_vec(),
        stream("", b"BT /F1 9 Tf 10 10 Td (Hi) Tj ET"),
        b"<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Arial /FirstChar 72 \
           /LastChar 105 /FontDescriptor 6 0 R >>"
            .to_vec(),
        b"<< /Type /FontDescriptor /FontName /ABCDEF+Arial /Flags 32 /FontBBox [0 0 1 1] \
           /ItalicAngle 0 /Ascent 1 /Descent 0 /CapHeight 1 /StemV 1 /FontFile2 7 0 R >>"
            .to_vec(),
        stream("/Length1 3000", &noise(3000)),
    ];
    bodies.extend_from_slice(extra);
    EditSession::new(Document::from_bytes(build(&bodies)).unwrap())
}

fn settings() -> OptimizeSettings {
    let mut s = OptimizeSettings::new();
    s.resubset = Some(ResubsetFonts::new(&ZEROS));
    s
}

fn written(s: &EditSession, plan: &crate::optimize::OptimizePlan) -> Document {
    let (file, _) = s
        .save_optimized(plan, &SaveOptions::default(), &RebuildOptions::none())
        .unwrap();
    match file {
        OptimizedFile::Smaller(b) | OptimizedFile::NotSmaller { bytes: b, .. } => {
            Document::from_bytes(b).unwrap()
        }
        other => panic!("expected a written file, got {other:?}"),
    }
}

fn name(d: &Document, obj: ObjId, key: &[u8]) -> String {
    let view = d.view();
    let n = view
        .value(obj)
        .and_then(Object::as_dict)
        .unwrap()
        .get(key)
        .unwrap()
        .clone();
    String::from_utf8_lossy(n.as_name().unwrap().as_bytes()).into_owned()
}

#[test]
fn the_cut_program_and_new_tag_are_saved_in_one_undo_entry() {
    let mut s = session();
    let mut set = settings();
    set.streams = StreamAction::Recompress { level: 9 };
    let plan = s.plan_optimize(&set).unwrap();
    assert!(
        plan.changes.iter().all(|c| c.id != id(7)),
        "the cut replaces any recompress"
    );
    let cut = plan.resubset.as_ref().unwrap();
    assert_eq!(cut.targets.len(), 1, "{:?}", cut.refused);
    let after = cut.targets[0].name_after.clone();
    assert!(
        plan.notes.iter().any(|n| n.contains("2 of 9 glyphs")),
        "{:?}",
        plan.notes
    );
    let undo = s.undo.len();
    s.apply_optimize(&plan).unwrap();
    assert_eq!(s.undo.len(), undo + 1);
    let d = written(&s, &plan);
    assert_eq!(name(&d, id(5), b"BaseFont"), after);
    assert_eq!(name(&d, id(6), b"FontName"), after);
    let view = d.view();
    let Some(Object::Stream(p)) = view.value(id(7)) else {
        panic!("program gone");
    };
    let data = crate::filters::decode_stream(&p.dict, view.slice(p.data_span).unwrap()).unwrap();
    assert_eq!(data, vec![0; 50]);
    assert_eq!(p.dict.get(b"Length1").and_then(Object::as_int), Some(50));
    s.undo();
    assert_eq!(s.value(id(5)), session().value(id(5)));
}

#[test]
fn a_plan_applied_twice_is_stale() {
    let mut s = session();
    let plan = s.plan_optimize(&settings()).unwrap();
    s.apply_optimize(&plan).unwrap();
    assert!(matches!(
        s.apply_optimize(&plan),
        Err(OptimizeError::StalePlan { .. })
    ));
}

#[test]
fn a_font_unembedded_in_the_same_run_is_not_cut() {
    let s = session();
    let mut set = settings();
    set.unembed = Some(UnembedRequest::all_removable());
    let plan = s.plan_optimize(&set).unwrap();
    assert!(
        !plan.unembed.as_ref().unwrap().targets.is_empty(),
        "fixture must unembed"
    );
    let cut = plan.resubset.as_ref().unwrap();
    assert!(cut.targets.is_empty());
    assert_eq!(cut.refused[0].why.token, "unembedded-this-run");
}

#[test]
fn text_drawn_after_planning_makes_the_plan_stale() {
    let mut s = session();
    let plan = s.plan_optimize(&settings()).unwrap();
    let data = b"BT /F1 9 Tf 10 10 Td (Hiz) Tj ET";
    let data_span = s.stage_bytes(data);
    let Some(Object::Stream(old)) = s.value(id(4)).cloned() else {
        panic!("no content");
    };
    let mut dict = old.dict.clone();
    dict.insert(
        crate::object::Name::from(b"Length"),
        Object::Integer(data.len() as i64),
    );
    s.commit(crate::edit::Command {
        kind: crate::edit::CommandKind::Optimize { count: 1 },
        objects: vec![crate::edit::ObjectWrite {
            id: id(4),
            before: None,
            after: Some(Object::Stream(crate::object::Stream { dict, data_span })),
        }],
        removals: Vec::new(),
        trailer: None,
    });
    assert!(matches!(
        s.apply_optimize(&plan),
        Err(OptimizeError::StalePlan { .. })
    ));
}

#[test]
fn a_pdfa_2_document_is_re_subset() {
    let xmp = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" pdfaid:part="2" pdfaid:conformance="B"/></rdf:RDF></x:xmpmeta>"#;
    let s = session_with(
        "/Metadata 8 0 R",
        &[stream("/Type /Metadata /Subtype /XML", xmp)],
    );
    let plan = s.plan_optimize(&settings()).unwrap();
    let cut = plan.resubset.unwrap();
    assert_eq!(cut.targets.len(), 1, "{:?}", cut.refused);
}

/// A descriptor held directly in the font dictionary is re-tagged with it
/// (ISO 32000-2 §9.6.4).
#[test]
fn a_direct_descriptor_is_re_tagged_with_its_font() {
    use crate::object::{Dict, Name};
    let name = |n: &[u8]| Object::Name(Name::from(n));
    let mut inner = Dict::new();
    inner.insert(Name::from(b"FontName"), name(b"ABCDEF+Arial"));
    let mut dict = Dict::new();
    dict.insert(Name::from(b"BaseFont"), name(b"ABCDEF+Arial"));
    dict.insert(Name::from(b"FontDescriptor"), Object::Dict(inner));
    let font = Object::Dict(dict);
    let out = super::retagged(&font, "QWERTY").unwrap();
    let d = out.as_dict().unwrap();
    let inner = d.get(b"FontDescriptor").and_then(Object::as_dict).unwrap();
    for (dict, key) in [(d, b"BaseFont".as_slice()), (inner, b"FontName")] {
        assert_eq!(
            dict.get(key).and_then(Object::as_name).unwrap().as_bytes(),
            b"QWERTY+Arial"
        );
    }
}

/// Cuts like [`Zeros`] and reports `present` as what the cut holds.
#[derive(Debug)]
struct Lister(PresentGlyphs);

impl FontResubsetter for Lister {
    fn resubset(&self, r: &ResubsetRequest<'_, '_>) -> Result<ResubsetProgram, ResubsetRefusal> {
        Zeros.resubset(r)
    }

    fn present_glyphs(&self, _: &[u8]) -> Result<PresentGlyphs, ResubsetRefusal> {
        Ok(self.0.clone())
    }
}

/// A PDF/A-1 document whose page draws `content` with font 5 (`font`),
/// descriptor 6 holding program 7 (`program_dict`) under `file`, metadata
/// 8; `extra` from 9.
fn pdfa1(
    font: &str,
    file: &str,
    program_dict: &str,
    content: &[u8],
    extra: &[Vec<u8>],
) -> EditSession {
    let xmp = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" pdfaid:part="1" pdfaid:conformance="B"/></rdf:RDF></x:xmpmeta>"#;
    let mut bodies = vec![
        b"<< /Type /Catalog /Pages 2 0 R /Metadata 8 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R \
           /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_vec(),
        stream("", content),
        font.as_bytes().to_vec(),
        format!(
            "<< /Type /FontDescriptor /FontName /Helv /Flags 32 /FontBBox [0 0 1 1] \
             /ItalicAngle 0 /Ascent 1 /Descent 0 /CapHeight 1 /StemV 1 /{file} 7 0 R >>"
        )
        .into_bytes(),
        stream(program_dict, &noise(3000)),
        stream("/Type /Metadata /Subtype /XML", xmp),
    ];
    bodies.extend_from_slice(extra);
    EditSession::new(Document::from_bytes(build(&bodies)).unwrap())
}

fn listing(cutter: &'static Lister) -> OptimizeSettings {
    let mut s = OptimizeSettings::new();
    s.resubset = Some(ResubsetFonts::new(cutter));
    s
}

static NAMES: std::sync::LazyLock<Lister> = std::sync::LazyLock::new(|| {
    Lister(PresentGlyphs::Names(
        [b".notdef".to_vec(), b"a".to_vec(), b"b".to_vec()].into(),
    ))
});
static COUNT: Lister = Lister(PresentGlyphs::Count(10));

/// The saved descriptor carries the `/CharSet` PDF/A-1 requires of the new
/// subset (ISO 19005-1 §6.3.5); undo takes it away again.
#[test]
fn a_pdfa1_type1_subset_is_saved_with_its_charset() {
    let font = "<< /Type /Font /Subtype /Type1 /BaseFont /Helv /FirstChar 32 /LastChar 126 \
                /FontDescriptor 6 0 R >>";
    let mut s = pdfa1(
        font,
        "FontFile3",
        "/Subtype /Type1C",
        b"BT /F1 9 Tf (a) Tj ET",
        &[],
    );
    let before = s.value(id(6)).cloned();
    let plan = s.plan_optimize(&listing(&NAMES)).unwrap();
    assert!(
        plan.notes
            .iter()
            .any(|n| n.contains("1 font(s) given the /CIDSet or /CharSet")),
        "{:?}",
        plan.notes
    );
    s.apply_optimize(&plan).unwrap();
    let d = written(&s, &plan);
    let view = d.view();
    let dict = view.value(id(6)).and_then(Object::as_dict).unwrap().clone();
    assert_eq!(
        dict.get(b"CharSet"),
        Some(&Object::String(b"/a/b".to_vec()))
    );
    assert!(name(&d, id(6), b"FontName").ends_with("+Helv"));
    s.undo();
    assert_eq!(s.value(id(6)).cloned(), before);
}

/// A CIDFont's descriptor gains a new `/CIDSet` stream with a bit per CID
/// the program holds; undo removes the stream and the reference.
#[test]
fn a_pdfa1_cidfont_subset_is_saved_with_a_new_cidset_stream() {
    let font = "<< /Type /Font /Subtype /Type0 /BaseFont /Helv /Encoding /Identity-H \
                /DescendantFonts [9 0 R] >>";
    let cid_font = b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Helv \
        /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
        /FontDescriptor 6 0 R >>"
        .to_vec();
    let mut s = pdfa1(
        font,
        "FontFile2",
        "",
        b"BT /F1 9 Tf <0001> Tj ET",
        &[cid_font],
    );
    let before = s.value(id(6)).cloned();
    let plan = s.plan_optimize(&listing(&COUNT)).unwrap();
    let cut = plan.resubset.as_ref().unwrap();
    assert_eq!(cut.targets[0].listed, [id(6)], "{:?}", cut.refused);
    s.apply_optimize(&plan).unwrap();
    let Some(Object::Reference(set)) = s
        .value(id(6))
        .and_then(|d| d.as_dict().and_then(|d| d.get(b"CIDSet")).cloned())
    else {
        panic!("no /CIDSet reference");
    };
    let d = written(&s, &plan);
    let view = d.view();
    let Some(Object::Stream(p)) = view.value(set) else {
        panic!("/CIDSet is not a stream");
    };
    let bits = crate::filters::decode_stream(&p.dict, view.slice(p.data_span).unwrap()).unwrap();
    assert_eq!(bits, [0xFF, 0xC0]);
    s.undo();
    assert_eq!(s.value(id(6)).cloned(), before);
    assert_eq!(s.value(set), None);
}

/// PDF/A-1 forbids object and cross-reference streams (ISO 19005-1 §6.1.4),
/// so a rebuild asked to pack them leaves them out and the plan says so.
#[test]
fn a_pdfa1_rebuild_packs_no_object_streams() {
    let font = "<< /Type /Font /Subtype /Type1 /BaseFont /Helv /FirstChar 32 /LastChar 126 \
                /FontDescriptor 6 0 R >>";
    let mut s = pdfa1(
        font,
        "FontFile3",
        "/Subtype /Type1C",
        b"BT /F1 9 Tf (a) Tj ET",
        &[],
    );
    let mut settings = listing(&NAMES);
    settings.structure = RebuildOptions::all();
    let plan = s.plan_optimize(&settings).unwrap();
    assert!(
        plan.notes.iter().any(|n| n.contains("PDF/A-1 forbids")),
        "{:?}",
        plan.notes
    );
    s.apply_optimize(&plan).unwrap();
    let (file, report) = s
        .save_optimized(&plan, &SaveOptions::default(), &RebuildOptions::all())
        .unwrap();
    assert_eq!(report.unwrap().object_streams, 0);
    let OptimizedFile::Smaller(bytes) = file else {
        panic!("expected a smaller file, got {file:?}");
    };
    assert!(!bytes.windows(5).any(|w| w == b"/XRef"));
}
