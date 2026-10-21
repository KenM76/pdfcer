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
    FontResubsetter, ResubsetFonts, ResubsetProgram, ResubsetRefusal, ResubsetRequest,
};
use crate::optimize::tests::{build, id, noise};
use crate::optimize::{OptimizeError, OptimizeSettings, OptimizedFile, StreamAction};
use crate::space_audit::tests::stream;
use crate::writer::{RebuildOptions, SaveOptions};

/// Cuts every program to 50 zero bytes.
#[derive(Debug)]
struct Zeros;

impl FontResubsetter for Zeros {
    fn resubset(&self, _: &ResubsetRequest<'_, '_>) -> Result<ResubsetProgram, ResubsetRefusal> {
        Ok(ResubsetProgram::new(vec![0; 50], 2, 9))
    }
}

static ZEROS: Zeros = Zeros;

/// A page drawing "Hi" in font 5 (`ABCDEF+Arial`), descriptor 6, program 7.
fn session() -> EditSession {
    let bodies = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
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
