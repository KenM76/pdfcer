#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::pipeline_tests::{doc, grey};
use crate::document::Document;
use crate::edit::EditSession;
use crate::object::{ObjId, Object};
use crate::optimize::{
    OptimizePlan, OptimizeSettings, OptimizedFile, RebuildOptions, StreamAction,
};
use crate::writer::SaveOptions;

/// Two byte-identical images: `/Im5` drawn at 100 pt (216 ppi), `/Im6` at
/// 200 pt (108 ppi).
fn twins() -> Vec<u8> {
    doc(
        "q 100 0 0 100 0 0 cm /Im5 Do Q q 200 0 0 200 0 0 cm /Im6 Do Q",
        &[grey(""), grey("")],
    )
}

fn plan(merge: bool) -> (EditSession, OptimizePlan) {
    let mut settings = OptimizeSettings::new();
    settings.streams = StreamAction::parse("image(ppi=50)").unwrap();
    settings.structure = if merge {
        RebuildOptions::all()
    } else {
        RebuildOptions::none()
    };
    let session = EditSession::new(Document::from_bytes(twins()).unwrap());
    let plan = session.plan_optimize(&settings).unwrap();
    (session, plan)
}

fn after(p: &OptimizePlan, n: u32) -> ((u32, u32), Vec<ObjId>) {
    let img = p.changes.iter().find(|c| c.id.num == n).unwrap();
    let img = img.image.as_ref().unwrap();
    (img.size_after, img.copies.clone())
}

#[test]
fn identical_images_are_measured_over_every_copy_and_name_each_other() {
    let (_, p) = plan(true);
    // The 108 ppi use binds both: 300 x 50 / 108 = 139.
    assert_eq!(after(&p, 5), ((139, 139), vec![ObjId::new(6, 0)]));
    assert_eq!(after(&p, 6), ((139, 139), vec![ObjId::new(5, 0)]));
    let b = p.changes.iter().find(|c| c.id.num == 5).unwrap();
    assert_eq!(
        b.image.as_ref().unwrap().binding.as_ref().unwrap().width_pt,
        200.0
    );
}

#[test]
fn without_merge_duplicates_each_copy_is_measured_alone() {
    let (_, p) = plan(false);
    assert_eq!(after(&p, 5), ((69, 69), vec![]));
    assert_eq!(after(&p, 6), ((139, 139), vec![]));
}

#[test]
fn the_rebuilt_file_keeps_one_image() {
    let (mut s, p) = plan(true);
    s.apply_optimize(&p).unwrap();
    let OptimizedFile::Smaller(bytes) = s
        .save_optimized(&p, &SaveOptions::default(), &RebuildOptions::all())
        .unwrap()
        .0
    else {
        panic!("expected a smaller file")
    };
    let d = Document::from_bytes(bytes).unwrap();
    let images = d
        .objects()
        .filter(|o| {
            matches!(&o.value, Object::Stream(s)
                if matches!(s.dict.get(b"Subtype"), Some(Object::Name(n)) if n.as_bytes() == b"Image"))
        })
        .count();
    assert_eq!(images, 1);
}
