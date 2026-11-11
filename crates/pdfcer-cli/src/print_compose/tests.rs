// Test code: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use pdfcer_core::document::Document;
use pdfcer_core::object::Object;

use super::*;

/// Letter with a 18 pt margin on every side.
pub(crate) fn letter() -> Paper {
    Paper::portrait((612.0, 792.0), (576.0, 756.0), (18.0, 18.0))
}

#[test]
fn landscape_caps_turn_portrait() {
    // Landscape Letter, printable inset 10 left, 20 top, 30 right, 40 bottom.
    let p = Paper::portrait((792.0, 612.0), (752.0, 552.0), (10.0, 20.0));
    assert_eq!(p.physical_pt, (612.0, 792.0));
    assert_eq!(p.printable_pt, (552.0, 752.0));
    assert_eq!(p.offset_pt, (20.0, 30.0));
}

#[test]
fn each_turn_frames_the_printable_area_on_the_paper() {
    // Insets: left 10, top 20, right 30, bottom 40.
    let p = Paper::portrait((612.0, 792.0), (572.0, 732.0), (10.0, 20.0));
    assert_eq!(
        p.frame(0),
        ((612.0, 792.0), Rect::new(10.0, 20.0, 572.0, 732.0))
    );
    // Turned clockwise onto the paper: the media's top edge is the paper's
    // right edge, so its top inset is the paper's right inset.
    assert_eq!(
        p.frame(90),
        ((792.0, 612.0), Rect::new(20.0, 30.0, 732.0, 572.0))
    );
    assert_eq!(
        p.frame(180),
        ((612.0, 792.0), Rect::new(30.0, 40.0, 572.0, 732.0))
    );
    assert_eq!(
        p.frame(270),
        ((792.0, 612.0), Rect::new(40.0, 10.0, 732.0, 572.0))
    );
}

#[test]
fn strokes_become_one_path_and_none_without_any() {
    let mut sheet = Sheet::default();
    assert_eq!(strokes_svg(&sheet, (612.0, 792.0)), None);
    sheet.lines.push(((1.0, 2.0), (3.0, 4.0)));
    sheet.boxes.push(Rect::new(10.0, 20.0, 30.0, 40.0));
    let svg = strokes_svg(&sheet, (612.0, 792.0)).unwrap();
    assert!(svg.contains("viewBox=\"0 0 612 792\""), "{svg}");
    assert!(
        svg.contains("M1.000 2.000L3.000 4.000M10.000 20.000H40.000V60.000H10.000Z"),
        "{svg}"
    );
}

/// The `a d e f` of every `a 0 0 d e f cm` in `page`'s content streams.
pub(crate) fn placements(session: &EditSession, page: usize) -> Vec<[f64; 4]> {
    let pages = session.pages().unwrap();
    let view = session.view();
    let mut out = Vec::new();
    for id in &pages[page].contents {
        let Some(Object::Stream(s)) = view.graph().value(*id) else {
            continue;
        };
        let text =
            String::from_utf8_lossy(view.slice(s.data_span).unwrap_or_default()).into_owned();
        let words: Vec<&str> = text.split_whitespace().collect();
        for (i, w) in words.iter().enumerate() {
            if *w == "cm" && i >= 6 {
                let n = |k: usize| words[i - k].parse::<f64>().unwrap();
                out.push([n(6), n(3), n(2), n(1)]);
            }
        }
    }
    out
}

fn source(pages: usize) -> EditSession {
    let bytes = crate::print_remote::tests::pages_pdf(pages);
    EditSession::new(Document::from_bytes(bytes).unwrap())
}

#[test]
fn two_up_turns_the_sheet_and_keeps_pages_upright_in_it() {
    let mut src = source(3);
    let spec = NUpSpec {
        grid: imposition::NUpGrid::Count(2),
        auto_rotate: true,
        ..NUpSpec::default()
    };
    let req = Request {
        paper: letter(),
        shape: Shape::NUp(spec),
        mode: ScaleMode::Fit,
        orientation: Orientation::Auto,
        label: "a.pdf",
    };
    let composed = compose(&mut src, &[0, 1, 2], &req).unwrap();
    assert_eq!(composed.pages, 2);
    let out = EditSession::new(Document::from_bytes(composed.pdf).unwrap());
    let pages = out.pages().unwrap();
    assert_eq!(pages.len(), 2);
    for p in &pages {
        // Two portrait pages side by side on a landscape media box, turned
        // a quarter onto the portrait paper.
        assert_eq!(p.rotate, 90);
        assert_eq!((p.media_box.width(), p.media_box.height()), (792.0, 612.0));
    }
    let first = placements(&out, 0);
    assert_eq!(first.len(), 2, "{first:?}");
    assert_eq!(placements(&out, 1).len(), 1);
    let [a, d, e0, f0] = first[0];
    assert!((a - d).abs() < 1e-9 && a < 1.0, "{a} {d}");
    let [_, _, e1, f1] = first[1];
    // Side by side, the second to the right, both inside the margins.
    assert!(e1 >= e0 + 612.0 * a - 1e-6, "{e0} {e1}");
    assert!((f0 - f1).abs() < 1e-6);
    for [s, _, e, f] in first {
        assert!(
            e >= 18.0 - 1e-6 && e + 612.0 * s <= 792.0 - 18.0 + 1e-6,
            "{e}"
        );
        assert!(
            f >= 18.0 - 1e-6 && f + 792.0 * s <= 612.0 - 18.0 + 1e-6,
            "{f}"
        );
    }
}

#[test]
fn a_page_landscape_by_request_is_turned_counter_clockwise() {
    let mut src = source(1);
    let req = Request {
        paper: letter(),
        shape: Shape::Pages,
        mode: ScaleMode::ActualSize,
        orientation: Orientation::Landscape,
        label: "a.pdf",
    };
    let composed = compose(&mut src, &[0], &req).unwrap();
    let out = EditSession::new(Document::from_bytes(composed.pdf).unwrap());
    assert_eq!(out.pages().unwrap()[0].rotate, 270);
    let [s, _, e, f] = placements(&out, 0)[0];
    assert!((s - 1.0).abs() < 1e-9);
    // Actual size on the turned 756x576 printable area: centred across, and
    // too tall, so its top meets the area's top as on the driver route.
    assert!((e - (18.0 + (756.0 - 612.0) / 2.0)).abs() < 1e-6, "{e}");
    assert!((f - (612.0 - 18.0 - 792.0)).abs() < 1e-6, "{f}");
}

#[test]
fn mixed_rotations_cannot_be_composed_into_n_up() {
    let mut src = source(2);
    src.set_page_rotation(1, 90).unwrap();
    let req = Request {
        paper: letter(),
        shape: Shape::NUp(NUpSpec {
            grid: imposition::NUpGrid::Count(2),
            ..NUpSpec::default()
        }),
        mode: ScaleMode::Fit,
        orientation: Orientation::Auto,
        label: "a.pdf",
    };
    let err = compose(&mut src, &[0, 1], &req).err().unwrap();
    assert!(err.contains("different rotations"), "{err}");
}

#[test]
fn a_poster_tile_shows_its_part_of_the_page() {
    let mut src = source(1);
    let spec = PosterSpec {
        tile_scale: 2.0,
        overlap_pt: 0.0,
        cut_marks: false,
        labels: false,
        tile_only_large_pages: false,
        max_tiles: 100,
    };
    let req = Request {
        paper: letter(),
        shape: Shape::Poster(spec),
        mode: ScaleMode::Fit,
        orientation: Orientation::Auto,
        label: "a.pdf",
    };
    let composed = compose(&mut src, &[0], &req).unwrap();
    assert!(composed.pages >= 4, "{}", composed.pages);
    let out = EditSession::new(Document::from_bytes(composed.pdf).unwrap());
    for i in 0..composed.pages {
        let [s, _, ..] = placements(&out, i)[0];
        assert!((s - 2.0).abs() < 1e-9, "tile {i}: {s}");
    }
}
