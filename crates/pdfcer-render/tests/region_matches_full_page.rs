//! # `render_page_region` — a region must be the same pixels as the crop
//!
//! Answers the request in `D:\Dev\FeatureRequests\pdfce_FeatureRequests\`
//! (`request_region_rasterisation.md`, from the `pdfcer-gui` session): rasterise
//! a sub-rectangle of a page so a viewer at high magnification pays for the
//! pixels it shows rather than for the whole sheet.
//!
//! ## The oracle, and why it is the only one worth having
//!
//! A region render is trivially "correct" against itself — it produces *some*
//! pixmap of the right size, and eyeballing it proves nothing. The real
//! contract is **differential**: rendering region *R* at scale *s* must
//! produce exactly the pixels that cropping a full-page render at scale *s* to
//! *R* would produce. Anything less and a tiled viewer shows seams, doubled
//! strokes, or content shifted by a pixel per tile — defects that look like
//! rendering bugs and are actually transform bugs.
//!
//! So every test here compares against a full-page render. That is a slow
//! oracle and an exact one, which is the right trade for a transform.
//!
//! ## What each test pins
//!
//! | test | the failure it catches |
//! |---|---|
//! | `a_region_matches_the_same_crop_of_the_full_page` | any error in the device-space translation — the whole point |
//! | `four_tiles_reassemble_into_the_whole_page` | seams: an off-by-one in the floor/ceil that would lose or double a row between adjacent tiles |
//! | `a_region_is_bounded_by_its_own_size_not_the_pages` | the actual feature: that the guard now applies to the region, i.e. that deep zoom is reachable at all |
//! | `an_empty_region_refuses_by_name` | a degenerate rect producing a zero-sized pixmap rather than a named error |
//!
//! ## Pass 75.0 — the same oracle, applied to the display list
//!
//! `Pass 75.0` adds a second way to produce a region: interpret the page once
//! into a [`DisplayList`](pdfcer_render::DisplayList) and replay it. That is a
//! *cache*, and a cache is exactly the kind of thing that is right on the
//! fixtures somebody happened to try and wrong on everything else — so it is
//! held to the **same differential oracle** rather than to a new one.
//!
//! Extending this file rather than starting another was deliberate: the crop
//! helper, the y-flip reasoning and the tiling policy are all already argued
//! here, and a second file would have to restate them or silently assume them.
//!
//! | test | the failure it catches |
//! |---|---|
//! | `a_replayed_region_is_byte_identical_to_a_fresh_one` | ★ criterion 3 — any drift between the recorded ops and what the interpreter would have painted |
//! | `a_replayed_region_survives_a_clip_and_a_layer` | clips recorded as device masks (invalid after a pan) and layers composited in the wrong order |
//! | `culling_never_drops_a_mark` | a bounds computation that is too tight — the one cull failure that is invisible until a stroke goes missing at a viewport edge |
//! | `a_stale_epoch_is_refused_by_name` | ★ criterion 2 — a display list serving a document's PREVIOUS state while reporting success |
//! | `a_different_scale_is_refused_by_name` | the scale narrowing being silently ignored rather than enforced |
//! | `an_unrecordable_page_refuses_by_name` | a page with a shading yielding a list that renders NEARLY right |

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_render::{
    DisplayListKey, MAX_PIXMAP_EDGE, PoisonReason, RenderError, RenderOptions, record_page,
    render_page_region, render_page_view,
};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

/// The sub-rectangle of a full-page pixmap corresponding to a device-space
/// origin and size, as a flat RGBA vector — the oracle's crop.
fn crop(pixmap: &pdfcer_render::tiny_skia::Pixmap, x0: u32, y0: u32, w: u32, h: u32) -> Vec<u8> {
    let stride = pixmap.width() as usize * 4;
    let data = pixmap.data();
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in 0..h {
        let src = (y0 + row) as usize * stride + x0 as usize * 4;
        out.extend_from_slice(&data[src..src + (w as usize) * 4]);
    }
    out
}

/// ★ The contract: a region is the crop.
#[test]
fn a_region_matches_the_same_crop_of_the_full_page() {
    let doc = Document::load(&fixture("addtext/plain.pdf")).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree");
    let page = &pages[0];
    let scale = 2.0;

    let full = render_page_view(&doc.view(), page, scale).expect("full page rasterises");

    // A region well inside the page, on integral page-space coordinates so the
    // expected device origin is exact at this scale and the comparison is not
    // testing the rounding policy by accident (that is the tiling test's job).
    let cb = page.crop_box;
    let region = Rect::from_corners(cb.llx + 50.0, cb.lly + 60.0, cb.llx + 250.0, cb.lly + 220.0);

    let got = render_page_region(&doc.view(), page, scale, region, &RenderOptions::default())
        .expect("region rasterises");

    let w = (200.0 * scale) as u32;
    let h = (160.0 * scale) as u32;
    assert_eq!(got.pixmap.width(), w, "region width");
    assert_eq!(got.pixmap.height(), h, "region height");

    // Device y is flipped: the region's TOP edge in page space (ury) is the
    // small device y. Getting this backwards is the single most likely defect,
    // and it would still produce a plausible-looking picture of the wrong part
    // of the page.
    let x0 = (50.0 * scale) as u32;
    let y0 = ((cb.ury - (cb.lly + 220.0)) * f64::from(scale)) as u32;

    let expected = crop(&full.pixmap, x0, y0, w, h);
    let differing = expected
        .iter()
        .zip(got.pixmap.data().iter())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing,
        0,
        "a region must be byte-identical to the corresponding crop of the full \
         page; {differing} of {} bytes differ. A large count with the right \
         SIZE means the translation is wrong, not the scale.",
        expected.len()
    );
}

/// ★ Four tiles reassemble into the whole page, with no seam and no overlap.
///
/// This is the test a tiled viewer actually depends on. The floor/ceil policy
/// on the region's device bounds is chosen so a requested region is fully
/// covered rather than cropped; a policy that rounded instead would lose a row
/// between adjacent tiles and show a hairline seam that reads as a rendering
/// artefact.
#[test]
fn four_tiles_reassemble_into_the_whole_page() {
    let doc = Document::load(&fixture("addtext/plain.pdf")).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree");
    let page = &pages[0];
    let scale = 1.0;
    let full = render_page_view(&doc.view(), page, scale).expect("full page");

    let cb = page.crop_box;
    let (mx, my) = ((cb.llx + cb.urx) / 2.0, (cb.lly + cb.ury) / 2.0);
    let quadrants = [
        Rect::from_corners(cb.llx, my, mx, cb.ury), // top-left in page space
        Rect::from_corners(mx, my, cb.urx, cb.ury), // top-right
        Rect::from_corners(cb.llx, cb.lly, mx, my), // bottom-left
        Rect::from_corners(mx, cb.lly, cb.urx, my), // bottom-right
    ];

    let mut covered = 0u64;
    for (i, q) in quadrants.iter().enumerate() {
        let tile = render_page_region(&doc.view(), page, scale, *q, &RenderOptions::default())
            .unwrap_or_else(|e| panic!("quadrant {i} rasterises: {e}"));

        let x0 = ((q.llx - cb.llx) * f64::from(scale)).floor() as u32;
        let y0 = ((cb.ury - q.ury) * f64::from(scale)).floor() as u32;
        let (w, h) = (tile.pixmap.width(), tile.pixmap.height());

        // Only compare the part that lies inside the full-page raster: the
        // ceil policy can push a tile one pixel past the page edge, which is
        // correct (the region was covered) and simply has no oracle there.
        let w = w.min(full.pixmap.width().saturating_sub(x0));
        let h = h.min(full.pixmap.height().saturating_sub(y0));
        assert!(w > 0 && h > 0, "quadrant {i} must overlap the page");

        let expected = crop(&full.pixmap, x0, y0, w, h);
        let stride = tile.pixmap.width() as usize * 4;
        let mut actual = Vec::with_capacity(expected.len());
        for row in 0..h {
            let src = row as usize * stride;
            actual.extend_from_slice(&tile.pixmap.data()[src..src + (w as usize) * 4]);
        }
        assert_eq!(
            expected
                .iter()
                .zip(actual.iter())
                .filter(|(a, b)| a != b)
                .count(),
            0,
            "quadrant {i} must match the full-page raster exactly — a mismatch \
             here is the seam a tiled viewer would show"
        );
        covered += u64::from(w) * u64::from(h);
    }

    let page_px = u64::from(full.pixmap.width()) * u64::from(full.pixmap.height());
    assert_eq!(
        covered, page_px,
        "the four quadrants must cover every pixel exactly once — {covered} vs \
         {page_px} means a gap (seam) or an overlap (doubled strokes)"
    );
}

// The `/Rotate` axis-swap case is NOT here, deliberately.
//
// It lives as a unit test in `crates/pdfcer-render/src/lib.rs`
// (`a_region_of_a_rotated_page_is_the_crop_of_the_rotated_page`) because
// **no fixture in `fixtures/synthetic/` carries a `/Rotate` key at all** —
// the first draft of that test lived here, found nothing to load, and
// skipped, i.e. it reported success while testing nothing. The in-memory
// `doc_with_content` helper can set `/Rotate 90` directly, so the coverage is
// real there and unreachable here. Recorded rather than silently moved,
// because "no fixture exercises page rotation" is a corpus gap that outlives
// this feature.

/// ★ The feature itself: the guard now bounds the REGION, not the page.
///
/// This is what makes deep zoom reachable. At a scale that would make the
/// whole page exceed `MAX_PIXMAP_EDGE` — and therefore fail outright — a
/// modest region must still render.
#[test]
fn a_region_is_bounded_by_its_own_size_not_the_pages() {
    let doc = Document::load(&fixture("addtext/plain.pdf")).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree");
    let page = &pages[0];
    let cb = page.crop_box;

    // A scale at which the full page is definitively over the guard.
    let scale = (f64::from(MAX_PIXMAP_EDGE) / (cb.ury - cb.lly) * 1.5) as f32;

    assert!(
        matches!(
            render_page_view(&doc.view(), page, scale),
            Err(RenderError::BadRasterSize { .. })
        ),
        "the premise of this test is that the WHOLE page is over the guard at \
         this scale"
    );

    // A 40x30 pt region at the same scale is small, and must succeed.
    let region = Rect::from_corners(cb.llx + 10.0, cb.lly + 10.0, cb.llx + 50.0, cb.lly + 40.0);
    let got = render_page_region(&doc.view(), page, scale, region, &RenderOptions::default())
        .expect("a small region must render at a zoom the whole page cannot");
    assert!(got.pixmap.width() > 0 && got.pixmap.height() > 0);
    assert!(
        got.pixmap.width() <= MAX_PIXMAP_EDGE && got.pixmap.height() <= MAX_PIXMAP_EDGE,
        "and it is still bounded — by its own size"
    );
}

/// A degenerate region is a named refusal, not a zero-sized pixmap.
#[test]
fn an_empty_region_refuses_by_name() {
    let doc = Document::load(&fixture("addtext/plain.pdf")).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree");
    let page = &pages[0];
    let cb = page.crop_box;
    let empty = Rect::from_corners(cb.llx + 10.0, cb.lly + 10.0, cb.llx + 10.0, cb.lly + 10.0);
    assert!(matches!(
        render_page_region(&doc.view(), page, 1.0, empty, &RenderOptions::default()),
        Err(RenderError::BadRasterSize { .. })
    ));
}

// ---------------------------------------------------------------------------
// Pass 75.0 — the display list, held to the same differential oracle
// ---------------------------------------------------------------------------

/// Build a self-contained PDF from `(object number, body)` pairs.
///
/// A local copy of the builder several tests in this crate carry, rather than
/// a shared helper, because integration tests cannot see each other's private
/// items and a `tests/common/` module would make this file stop being
/// readable on its own — which is the property that made extending it the
/// right call in the first place.
fn build(objects: &[(u32, &str)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
    buf.extend_from_slice(format!("xref\n0 {}\n", max_num + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..=max_num {
        match offsets.iter().find(|(n, _)| *n == num) {
            Some((_, off)) => buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => buf.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /ID [<0102> <0304>] >>\nstartxref\n{xref_at}\n%%EOF\n",
            max_num + 1
        )
        .as_bytes(),
    );
    buf
}

/// A one-page document whose content stream is `content`, with `resources`
/// spliced into the page tree's inherited `/Resources`.
fn one_page(resources: &str, content: &str) -> Vec<u8> {
    let stream = format!("{content}\n");
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            &format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 120 120] \
                 /Resources << {resources} >> >>"
            ),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>"),
        (
            4,
            &format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ),
    ])
}

/// Assert that replaying `region` from a recorded list is byte-identical to
/// rendering it directly, and say something useful when it is not.
fn assert_replay_matches(
    doc: &pdfcer_core::document::Document,
    page: &pdfcer_core::page_tree::Page,
    scale: f32,
    region: Rect,
    what: &str,
) {
    let options = RenderOptions::default();
    let fresh = render_page_region(&doc.view(), page, scale, region, &options)
        .unwrap_or_else(|e| panic!("{what}: fresh region render: {e}"));
    let list = record_page(&doc.view(), page, scale, 7, &options)
        .unwrap_or_else(|e| panic!("{what}: recording: {e}"));
    let replayed = list
        .replay_region(list.key(), region)
        .unwrap_or_else(|e| panic!("{what}: replay: {e}"));

    assert_eq!(
        (replayed.pixmap.width(), replayed.pixmap.height()),
        (fresh.pixmap.width(), fresh.pixmap.height()),
        "{what}: a replay must land on the same raster as a fresh render — a \
         size mismatch means the region arithmetic diverged, not the drawing"
    );
    let differing = fresh
        .pixmap
        .data()
        .iter()
        .zip(replayed.pixmap.data().iter())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing,
        0,
        "{what}: a replayed region must be BYTE-identical to a fresh one; \
         {differing} of {} bytes differ. A handful of differing bytes on \
         object edges is anti-aliasing drift (a transform composed in a \
         different order); a large contiguous count is a missing or \
         mis-clipped op.",
        fresh.pixmap.data().len()
    );
}

/// ★ Criterion 3, on real fixtures: the replay IS the render.
#[test]
fn a_replayed_region_is_byte_identical_to_a_fresh_one() {
    // Text, vector geometry and an annotated page — the three shapes of
    // content the recorder handles by three different routes (glyph fills,
    // path fills/strokes, and a nested `run_form_at` for the appearance).
    for rel in [
        "addtext/plain.pdf",
        "annot/demo-annotated.pdf",
        "annot/placement-matrix-rotate.pdf",
    ] {
        let doc = Document::load(&fixture(rel)).expect("fixture loads");
        let pages = page_tree::pages(&doc).expect("page tree");
        let page = &pages[0];
        let cb = page.crop_box;
        let w = cb.urx - cb.llx;
        let h = cb.ury - cb.lly;

        // Four regions, chosen to exercise different failure modes: the whole
        // page (nothing culled), an interior window (everything culled at the
        // edges), a corner (the region origin is not the page origin), and a
        // sliver (almost everything culled — where a too-tight bound shows).
        let regions = [
            ("whole page", cb),
            (
                "interior window",
                Rect::from_corners(
                    cb.llx + w * 0.25,
                    cb.lly + h * 0.25,
                    cb.llx + w * 0.75,
                    cb.lly + h * 0.75,
                ),
            ),
            (
                "top-right corner",
                Rect::from_corners(cb.llx + w * 0.6, cb.lly + h * 0.6, cb.urx, cb.ury),
            ),
            (
                "horizontal sliver",
                Rect::from_corners(cb.llx, cb.lly + h * 0.45, cb.urx, cb.lly + h * 0.55),
            ),
        ];
        for scale in [1.0_f32, 2.5] {
            for (name, region) in regions {
                assert_replay_matches(
                    &doc,
                    page,
                    scale,
                    region,
                    &format!("{rel} @{scale} [{name}]"),
                );
            }
        }
    }
}

/// A clip and a `/CA` layer are the two constructs a naive recorder gets
/// wrong: the first because a mask is device-sized, the second because
/// per-operator alpha and per-layer alpha differ wherever a drawing overlaps
/// itself.
#[test]
fn a_replayed_region_survives_a_clip_and_a_layer() {
    // Two overlapping rectangles inside a clip, drawn at half alpha as ONE
    // transparency group. Per-operator alpha would darken the overlap; the
    // clip must cut the right corner off both.
    let content = "q 20 20 70 70 re W n \
                   /GS0 gs \
                   1 0 0 rg 10 10 60 60 re f \
                   0 0 1 rg 40 40 60 60 re f \
                   Q";
    let bytes = one_page("/ExtGState << /GS0 << /ca 0.5 /BM /Normal >> >>", content);
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let page = page_tree::pages(&doc).expect("page tree").remove(0);
    let cb = page.crop_box;

    for region in [
        cb,
        Rect::from_corners(cb.llx + 15.0, cb.lly + 15.0, cb.llx + 95.0, cb.lly + 95.0),
        Rect::from_corners(cb.llx + 60.0, cb.lly + 60.0, cb.urx, cb.ury),
    ] {
        assert_replay_matches(&doc, &page, 2.0, region, "clip + layer");
    }
}

/// ★ The cull must never drop a mark.
///
/// A bounds computation that is too generous costs a paint that would have
/// been skipped; one that is too tight **loses content**, and only at a
/// viewport edge, on documents nobody thought to check. So the case that
/// matters is a thick, mitred stroke whose ink reaches well outside its
/// path's own bounding box, viewed through a region that contains the ink but
/// NOT the path.
#[test]
fn culling_never_drops_a_mark() {
    // A 20-unit-wide mitre at (60,60): the join spike reaches far past the
    // two segments' own bounds.
    let content = "0 0 0 RG 20 w 10 j 0 M 30 60 m 60 60 l 60 30 l S";
    let bytes = one_page("", content);
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let page = page_tree::pages(&doc).expect("page tree").remove(0);
    let cb = page.crop_box;

    // A band just outside the polyline's own bounds, where only the stroke's
    // half-width and its mitre reach.
    for region in [
        Rect::from_corners(cb.llx + 62.0, cb.lly + 25.0, cb.llx + 80.0, cb.lly + 80.0),
        Rect::from_corners(cb.llx + 20.0, cb.lly + 62.0, cb.llx + 80.0, cb.lly + 80.0),
        Rect::from_corners(cb.llx + 55.0, cb.lly + 55.0, cb.llx + 75.0, cb.lly + 75.0),
    ] {
        assert_replay_matches(&doc, &page, 3.0, region, "mitre outside path bounds");
    }
}

/// ★ Criterion 2: a stale handle refuses rather than serving old pixels.
#[test]
fn a_stale_epoch_is_refused_by_name() {
    let doc = Document::load(&fixture("addtext/plain.pdf")).expect("fixture loads");
    let page = page_tree::pages(&doc).expect("page tree").remove(0);
    let list = record_page(&doc.view(), &page, 1.0, 4, &RenderOptions::default())
        .expect("plain text records");

    let mut moved_on = list.key();
    moved_on.epoch += 1;

    match list.replay_region(moved_on, page.crop_box) {
        Err(RenderError::DisplayListStale {
            expected_epoch,
            recorded_epoch,
            ..
        }) => {
            assert_eq!((expected_epoch, recorded_epoch), (5, 4));
        }
        Err(other) => panic!("wrong error for a stale epoch: {other}"),
        Ok(_) => panic!(
            "a display list recorded at epoch 4 replayed as epoch 5 and REPORTED \
             SUCCESS. That is the failure this criterion exists to prevent: the \
             caller gets a picture of the document's previous state with nothing \
             to distinguish it from a current one."
        ),
    }
}

/// The scale narrowing is enforced, not merely documented.
#[test]
fn a_different_scale_is_refused_by_name() {
    let doc = Document::load(&fixture("addtext/plain.pdf")).expect("fixture loads");
    let page = page_tree::pages(&doc).expect("page tree").remove(0);
    let list = record_page(&doc.view(), &page, 1.0, 0, &RenderOptions::default())
        .expect("plain text records");

    let mut zoomed = list.key();
    zoomed.scale = 2.0;

    assert!(
        matches!(
            list.replay_region(zoomed, page.crop_box),
            Err(RenderError::DisplayListStale { .. })
        ),
        "a list recorded at scale 1 must refuse to serve scale 2 — every \
         device-dependent decision in it (hairlines, image filters, edge \
         anti-aliasing) was made at the recorded scale"
    );
}

/// ★ A page that cannot be recorded says so, rather than recording something
/// that renders NEARLY right.
#[test]
fn an_unrecordable_page_refuses_by_name() {
    // `sh` — evaluated per destination pixel, so it has no recordable form.
    let bytes = one_page(
        "/Shading << /Sh0 << /ShadingType 2 /ColorSpace /DeviceRGB \
         /Coords [0 0 120 120] /Extend [true true] \
         /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >> >>",
        "/Sh0 sh",
    );
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let page = page_tree::pages(&doc).expect("page tree").remove(0);

    // The control: the page renders perfectly well. Refusing to RECORD it is
    // not refusing to draw it, and a test that did not check this could pass
    // against a page that simply failed.
    let direct = render_page_region(
        &doc.view(),
        &page,
        1.0,
        page.crop_box,
        &RenderOptions::default(),
    )
    .expect("a shading page still renders");
    assert!(
        direct.diagnostics.shading.painted > 0,
        "the control must actually paint a shading, or this test proves nothing"
    );

    match record_page(&doc.view(), &page, 1.0, 0, &RenderOptions::default()) {
        Err(RenderError::PageNotRecordable { reason }) => {
            assert_eq!(reason, PoisonReason::Shading);
        }
        Err(other) => panic!("wrong error for an unrecordable page: {other}"),
        Ok(list) => panic!(
            "a page with a shading produced a display list of {} ops. Replaying \
             it would draw everything EXCEPT the shading, and report success.",
            list.op_count()
        ),
    }
}

/// The memory figure criterion 4 asks for is reachable and plausible.
#[test]
fn a_list_reports_its_own_size() {
    let doc = Document::load(&fixture("addtext/plain.pdf")).expect("fixture loads");
    let page = page_tree::pages(&doc).expect("page tree").remove(0);
    let list = record_page(&doc.view(), &page, 1.0, 0, &RenderOptions::default()).expect("records");

    assert!(list.op_count() > 0, "a page with text records ops");
    assert!(
        list.memory_bytes() >= list.op_count() * std::mem::size_of::<usize>(),
        "the reported size must at least account for the ops it holds"
    );
    let (w, h) = list.page_device_size();
    assert!(w > 0 && h > 0, "a recorded page has a device size");
}

/// A key is usable as a map key, which is how a shell will actually hold
/// these — and which is why `Eq`/`Hash` are hand-written over the scale's
/// bit pattern rather than derived.
#[test]
fn a_key_survives_a_hash_map_round_trip() {
    use std::collections::HashMap;
    let page = pdfcer_core::object::ObjId::new(3, 0);
    let mut map: HashMap<DisplayListKey, &str> = HashMap::new();
    let key = DisplayListKey {
        page,
        epoch: 9,
        scale: 2.5,
    };
    map.insert(key, "held");
    assert_eq!(
        map.get(&DisplayListKey {
            page,
            epoch: 9,
            scale: 2.5
        }),
        Some(&"held")
    );
    assert!(
        !map.contains_key(&DisplayListKey {
            page,
            epoch: 9,
            scale: 2.500_001
        }),
        "a scale that differs at all is a different key"
    );
}

/// A recording scale past `f32`'s usable range is refused BY NAME, and one
/// below it still records.
///
/// ★ This is a `R211` boundary test, not a resource-limit test, and the
/// second half is the load-bearing one. `Pass 74.7` gave the DIRECT render
/// path an `f64` CTM; a display list is still `f32` throughout. If both
/// were allowed to operate above the point where that difference shows,
/// pdfcer would have two rendering paths that quietly disagree at deep zoom
/// — the exact hazard `R211` names, and the thing this module's own docs
/// call "strictly worse than no display list".
///
/// The refusal threshold is `Mat64::needs_precise_paths`, the SAME
/// predicate the interpreter uses to switch to its precise route. So below
/// it both paths are `f32` and identical; above it one goes `f64` and the
/// other refuses. There is no scale at which they differ.
#[test]
fn a_recording_scale_past_f32_precision_is_refused_by_name() {
    let doc = Document::load(&fixture("addtext/plain.pdf")).expect("fixture loads");
    let page = page_tree::pages(&doc).expect("page tree").remove(0);

    // A US-Letter page's transform carries `ury * scale`, so the ceiling
    // lands near 530. Recording is deliberately NOT bounded by
    // MAX_PIXMAP_EDGE -- a list allocates no raster -- so nothing else
    // would have stopped this.
    match record_page(&doc.view(), &page, 5_000.0, 1, &RenderOptions::default()) {
        Err(RenderError::PageNotRecordable {
            reason: PoisonReason::ScaleBeyondF32,
        }) => {}
        Err(other) => panic!("wrong refusal for an out-of-range scale: {other}"),
        Ok(_) => panic!(
            "a display list recorded at scale 5 000, where its own f32 transforms are half a device pixel out. It would have replayed a picture that disagrees with a direct render of the same page, with nothing at the call site to say so."
        ),
    }

    // And the half that keeps the ceiling from being a silent feature
    // removal: an ordinary scale still records.
    record_page(&doc.view(), &page, 4.0, 1, &RenderOptions::default()).expect(
        "an ordinary scale must still record -- a guard that refuses everything is not a guard",
    );
}
