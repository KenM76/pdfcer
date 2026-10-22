//! `visual_diff`: before/after page comparison for the optimize report.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_render::{VisualDiffError, VisualDiffOptions, diff_heatmap, render_page, visual_diff};

use super::separation_preview::build;

/// A document of 100 pt pages, one per entry, each painting its content.
fn doc(pages: &[&str]) -> Document {
    let n = u32::try_from(pages.len()).unwrap();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 3 + 2 * i)).collect();
    let mut objects = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [{}] /Count {n} /MediaBox [0 0 100 100] >>",
                kids.join(" ")
            )
            .into_bytes(),
        ),
    ];
    for (i, content) in (0..n).zip(pages) {
        let (p, c) = (3 + 2 * i, 4 + 2 * i);
        objects.push((
            p,
            format!("<< /Type /Page /Parent 2 0 R /Contents {c} 0 R >>").into_bytes(),
        ));
        objects.push((
            c,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            )
            .into_bytes(),
        ));
    }
    Document::from_bytes(build(&objects)).unwrap()
}

const SQUARE: &str = "0 0 1 rg 20 20 30 30 re f";

#[test]
fn an_identical_page_scores_one_and_changes_nothing() {
    let report = visual_diff(
        &doc(&[SQUARE]),
        &doc(&[SQUARE]),
        &VisualDiffOptions::default(),
    )
    .unwrap();
    let page = report[0].as_ref().unwrap();
    assert!((page.ssim - 1.0).abs() < 1e-12, "{}", page.ssim);
    assert_eq!(
        (page.max_delta, page.changed_pixels, page.changed_region),
        (0, 0, None)
    );
    assert_eq!(page.pixels, 100 * 100);
}

#[test]
fn a_moved_square_is_counted_and_boxed() {
    let moved = "0 0 1 rg 30 20 30 30 re f";
    let report = visual_diff(
        &doc(&[SQUARE]),
        &doc(&[moved]),
        &VisualDiffOptions::default(),
    )
    .unwrap();
    let page = report[0].as_ref().unwrap();
    assert!(page.changed() && page.ssim < 0.99, "{page:?}");
    // Columns 20..30 lost the square and 50..60 gained it; rows 50..80 (top-left origin).
    assert_eq!(page.changed_pixels, 2 * 10 * 30);
    let r = page.changed_region.unwrap();
    assert_eq!((r.x, r.y, r.width, r.height), (20, 50, 40, 30));
    assert_eq!(page.max_delta, 255);
}

#[test]
fn a_change_under_the_threshold_is_scored_but_not_counted() {
    let faint = "0.98 0.98 0.98 rg 20 20 30 30 re f";
    let options = VisualDiffOptions::default().with_threshold(8);
    let report = visual_diff(&doc(&[""]), &doc(&[faint]), &options).unwrap();
    let page = report[0].as_ref().unwrap();
    assert!(page.max_delta > 0 && page.max_delta <= 8, "{page:?}");
    assert_eq!(page.changed_pixels, 0);
}

#[test]
fn a_page_only_one_side_has_is_reported_missing() {
    let report = visual_diff(
        &doc(&[SQUARE, SQUARE]),
        &doc(&[SQUARE]),
        &VisualDiffOptions::default(),
    )
    .unwrap();
    assert_eq!(report.len(), 2);
    assert!(report[0].is_ok());
    assert!(matches!(
        report[1],
        Err(VisualDiffError::MissingPage { side: "after" })
    ));
}

#[test]
fn the_heatmap_paints_changed_pixels_red_and_the_rest_grey() {
    let (a, b) = (doc(&[SQUARE]), doc(&["0 0 1 rg 30 20 30 30 re f"]));
    let pages = |d: &Document| pdfcer_core::page_tree::pages(d).unwrap();
    let ra = render_page(&a, &pages(&a)[0], 1.0).unwrap();
    let rb = render_page(&b, &pages(&b)[0], 1.0).unwrap();
    let map = diff_heatmap(&ra, &rb, 8).unwrap();
    let at = |x: u32, y: u32| map.pixel(x, y).unwrap();
    let changed = at(25, 60);
    assert!(changed.red() > 200 && changed.green() < 80, "{changed:?}");
    let unchanged = at(5, 5);
    assert_eq!(unchanged.red(), unchanged.green());
}
