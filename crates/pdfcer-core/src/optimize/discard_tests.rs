#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code: a failed lookup should fail the test loudly

use super::tests::{build, only, session};
use super::*;
use crate::writer::SaveOptions;

/// One page (3) with a thumbnail (5); private application data (6) on the
/// catalog; an information dictionary (7).
fn sample() -> Vec<u8> {
    let b = |s: &str| s.as_bytes().to_vec();
    let bytes = build(&[
        b("<< /Type /Catalog /Pages 2 0 R /PieceInfo << /App << /LastModified (D:2020) /Private 6 0 R >> >> >>"),
        b("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        b("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Thumb 5 0 R >>"),
        b("<< /Length 16 >>\nstream\n0 0 m 10 10 l S\n\nendstream"),
        format!(
            "<< /Width 20 /Height 20 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 400 >>\nstream\n{}\nendstream",
            "x".repeat(400)
        )
        .into_bytes(),
        b("<< /Data (private application data) >>"),
        b("<< /Title (A title) /Author (An author) >>"),
    ]);
    let text = String::from_utf8_lossy(&bytes).into_owned();
    text.replace("/Root 1 0 R >>", "/Root 1 0 R /Info 7 0 R >>")
        .into_bytes()
}

fn kinds(plan: &OptimizePlan) -> Vec<MetadataKind> {
    plan.discard.iter().map(|i| i.kind).collect()
}

fn present(s: &crate::edit::EditSession) -> Vec<MetadataKind> {
    let mut v: Vec<MetadataKind> = s
        .metadata_inventory()
        .items
        .iter()
        .map(|i| i.kind)
        .collect();
    v.dedup();
    v
}

#[test]
fn a_plan_lists_only_the_selected_kinds() {
    let s = session(sample());
    assert!(
        s.plan_optimize(&only(DiscardOptions::none()))
            .unwrap()
            .discard
            .is_empty()
    );
    let thumbs = s
        .plan_optimize(&only(DiscardOptions::none().with(MetadataKind::Thumbnail)))
        .unwrap();
    assert_eq!(kinds(&thumbs), [MetadataKind::Thumbnail]);
    let info = s
        .plan_optimize(&only(DiscardOptions::none().with(MetadataKind::InfoEntry)))
        .unwrap();
    assert_eq!(kinds(&info), [MetadataKind::InfoEntry; 2]);
    let standard = s.plan_optimize(&Preset::Standard.settings()).unwrap();
    assert_eq!(kinds(&standard), [MetadataKind::Thumbnail]);
    let screen = s.plan_optimize(&Preset::Screen.settings()).unwrap();
    assert_eq!(
        kinds(&screen),
        [MetadataKind::PieceInfo, MetadataKind::Thumbnail]
    );
    assert!(
        s.plan_optimize(&Preset::Archive.settings())
            .unwrap()
            .discard
            .is_empty()
    );
}

#[test]
fn apply_removes_them_as_one_undo_and_undo_restores_them() {
    let mut s = session(sample());
    let before = present(&s);
    let all = DiscardOptions::none()
        .with(MetadataKind::Thumbnail)
        .with(MetadataKind::PieceInfo)
        .with(MetadataKind::InfoEntry);
    let plan = s.plan_optimize(&only(all)).unwrap();
    assert_eq!(s.apply_optimize(&plan).unwrap(), 4);
    assert!(present(&s).is_empty(), "{:?}", present(&s));
    assert_eq!(s.undo_depth(), 1);
    assert_eq!(
        s.undo_kinds().next(),
        Some(crate::edit::CommandKind::Optimize { count: 4 })
    );
    s.undo();
    assert_eq!(present(&s), before);
}

#[test]
fn the_saved_file_no_longer_holds_the_discarded_data() {
    let mut s = session(sample());
    let mut settings = Preset::Standard.settings();
    settings.discard = settings.discard.with(MetadataKind::PieceInfo);
    let plan = s.plan_optimize(&settings).unwrap();
    s.apply_optimize(&plan).unwrap();
    let (file, _) = s
        .save_optimized(&plan, &SaveOptions::default(), &RebuildOptions::all())
        .unwrap();
    let OptimizedFile::Smaller(bytes) = file else {
        panic!("expected a smaller file");
    };
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("private application data"));
    assert!(!text.contains("/Thumb"));
    let left = present(&session(bytes));
    assert!(left.contains(&MetadataKind::InfoEntry), "{left:?}");
    assert!(!left.contains(&MetadataKind::Thumbnail) && !left.contains(&MetadataKind::PieceInfo));
}

#[test]
fn all_contains_every_kind() {
    let all = DiscardOptions::all();
    assert_eq!(all.kinds().collect::<Vec<_>>(), MetadataKind::ALL);
    assert!(DiscardOptions::none().is_none() && !all.is_none());
}
