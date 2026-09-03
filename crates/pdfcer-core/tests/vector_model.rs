//! # Pass 9a acceptance — vector object model over the synthetic fixtures
//!
//! Drives `pdfcer_core::vector` end-to-end (load → decode → decompose) on
//! the committed `fixtures/synthetic/vector/` PDFs, pinning the decision
//! 011 Appendix A Pass 9a acceptance criteria that live in `pdfcer-core`:
//!
//! - path/text/image objects are decomposed with the right shapes and
//!   token ranges (the byte span slices back to the object's source text);
//! - hit-testing selects the right object (click) and marquee encloses the
//!   right set;
//! - the filled-rectangle **centerline** derivation fires on thin bars
//!   (rotation-correct) and NOT on a genuine square / a below-threshold bar
//!   (the Z3 false-positive guard);
//! - the decomposition is **read-only** — it borrows the document
//!   immutably and cannot change a byte (the corpus-wide byte-inert proof
//!   is the separate content-identity gate; this asserts the API shape).
//!
//! The geometry-matches-the-renderer cross-check (Z2) is the companion
//! test in `pdfcer-render/tests/vector_cross_check.rs`.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{Page, pages};
use pdfcer_core::vector::{
    Bounds, MarqueeMode, Matrix, PageObjects, Point, TextPreview, VectorObject, decompose_page,
    hit_test_point, hit_test_point_all, hit_test_rect, page_candidates,
};

fn fixture(name: &str) -> Document {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/vector")
        .join(name);
    Document::from_bytes(std::fs::read(&path).expect("fixture file")).expect("fixture parses")
}

fn model(doc: &Document) -> (PageObjects, Page) {
    let page = pages(doc)
        .expect("page tree")
        .into_iter()
        .next()
        .expect("one page");
    let m = decompose_page(&doc.view(), &page, Matrix::IDENTITY).expect("decompose");
    (m, page)
}

fn count(m: &PageObjects) -> (usize, usize, usize) {
    let p = m
        .objects
        .iter()
        .filter(|o| matches!(o, VectorObject::Path(_)))
        .count();
    let t = m
        .objects
        .iter()
        .filter(|o| matches!(o, VectorObject::Text(_)))
        .count();
    let i = m
        .objects
        .iter()
        .filter(|o| matches!(o, VectorObject::Image(_)))
        .count();
    (p, t, i)
}

#[test]
fn paths_fixture_decomposes_every_shape_and_offers_one_centerline() {
    let doc = fixture("paths.pdf");
    let (m, _page) = model(&doc);
    // 8 path objects: polyline, filled rect, thin bar, triangle, donut,
    // cubic, v, y.
    assert_eq!(count(&m), (8, 0, 0));
    // Exactly one centerline candidate — the thin filled bar.
    let cands = page_candidates(&m);
    assert_eq!(cands.len(), 1);
    // The bar is 20 150 100 4 re -> midline along y = 152.
    assert!((cands[0].start.y - 152.0).abs() < 1e-6);
    assert!((cands[0].length - 100.0).abs() < 1e-6);

    // Every object's byte span slices back to real source content (the
    // editing handle is a genuine index into the decoded stream).
    for obj in &m.objects {
        let content = pdfcer_core::content::ContentStream::from_page(&doc.view(), &_page).unwrap();
        assert!(obj.bytes().slice(&content.buf).is_some());
    }
}

#[test]
fn click_selects_the_filled_rectangle_and_marquee_encloses_all() {
    let doc = fixture("paths.pdf");
    let (m, _page) = model(&doc);
    // The filled rectangle is 150 40 60 40 re f -> center (180, 60).
    let hit = hit_test_point(&m, Point::new(180.0, 60.0), 1.0).expect("hits the rectangle");
    match &m.objects[hit] {
        VectorObject::Path(p) => assert!(p.style.fill.is_some(), "the hit object is filled"),
        other => panic!("expected a path, got {other:?}"),
    }

    // A marquee over the whole page encloses all 8 objects.
    let whole = Bounds {
        min: Point::new(-10.0, -10.0),
        max: Point::new(310.0, 310.0),
    };
    let all = hit_test_rect(&m, whole, MarqueeMode::Enclosed);
    assert_eq!(all.len(), 8);

    // A tight marquee around only the filled rectangle encloses just it.
    let tight = Bounds {
        min: Point::new(148.0, 38.0),
        max: Point::new(212.0, 82.0),
    };
    let one = hit_test_rect(&m, tight, MarqueeMode::Enclosed);
    assert_eq!(one, vec![hit]);
}

#[test]
fn centerline_fixture_offers_a_candidate_per_thin_bar_and_none_for_the_square() {
    let doc = fixture("centerline.pdf");
    let (m, _page) = model(&doc);
    assert_eq!(count(&m), (5, 0, 0));
    let cands = page_candidates(&m);
    // Three thin bars (horizontal, vertical, rotated); NOT the square,
    // NOT the aspect-4 bar (Z3 false-positive guard).
    assert_eq!(cands.len(), 3);

    // The horizontal bar's midline is horizontal; the vertical bar's is
    // vertical; the rotated bar's is neither (rotation-correct).
    let horizontal = cands.iter().any(|c| (c.start.y - c.end.y).abs() < 1e-6);
    let vertical = cands.iter().any(|c| (c.start.x - c.end.x).abs() < 1e-6);
    let diagonal = cands
        .iter()
        .any(|c| (c.start.x - c.end.x).abs() > 1.0 && (c.start.y - c.end.y).abs() > 1.0);
    assert!(
        horizontal && vertical && diagonal,
        "one midline of each orientation"
    );

    // The genuine 60x60 square (250 300 60 60 re) is still selectable even
    // though it is NOT offered a centerline.
    assert!(hit_test_point(&m, Point::new(280.0, 330.0), 1.0).is_some());
}

#[test]
fn mixed_fixture_has_a_path_a_text_and_an_image_object_bbox_selectable() {
    let doc = fixture("mixed.pdf");
    let (m, _page) = model(&doc);
    assert_eq!(count(&m), (1, 1, 1));

    // The image (q 60 0 0 40 30 250 cm /Im0 Do) fills [30,250]..[90,290];
    // a click at its center selects the image object.
    let hit = hit_test_point(&m, Point::new(60.0, 270.0), 1.0).expect("hits the image");
    assert!(
        matches!(&m.objects[hit], VectorObject::Image(_)),
        "the topmost object at the image is the image"
    );

    // The text object is flagged approximate (bbox-only, not node-editable).
    let text = m
        .objects
        .iter()
        .find_map(|o| match o {
            VectorObject::Text(t) => Some(t),
            _ => None,
        })
        .expect("a text object");
    assert!(text.approximate);
    // Its bbox covers the (30,150) show origin.
    assert!(text.page_bbox.contains(Point::new(30.0, 150.0)));

    // ui-spec §B.4 #1, through the real `decompose_page` path (which
    // supplies a `DocumentFonts` resolver): the object carries the string it
    // shows and the typeface that shows it, decoded through
    // `text_extract`'s §9.10.2 ladder rather than a decoder written twice.
    assert_eq!(
        text.preview,
        TextPreview::Decoded {
            text: "Vector".to_owned(),
            truncated: false,
            lossy: false,
        }
    );
    let font = text.font.as_ref().expect("a /Tf was in effect");
    assert_eq!(font.resource, "F1");
    assert_eq!(font.base_font.as_deref(), Some("Helvetica"));
    assert_eq!(font.size, 14.0);

    // ui-spec §B.4 #2: the image's SAMPLE count (§8.9.5 Table 89), which is
    // a different thing from its 60x40 pt placement — both are carried, and
    // the pair is what answers "at what effective resolution is this
    // placed?".
    let image = m
        .objects
        .iter()
        .find_map(|o| match o {
            VectorObject::Image(i) => Some(i),
            _ => None,
        })
        .expect("an image object");
    assert_eq!(image.pixel_size, Some((2, 2)));
    assert_eq!(image.page_bbox.max.x - image.page_bbox.min.x, 60.0);
}

/// **The all-hits query over a real file** — three concentric filled
/// squares, so the front-to-back list is genuine overlap rather than a
/// tolerance artefact.
///
/// The load-bearing assertion is the LAST one: objects 1 and 0 are
/// unreachable through `hit_test_point` at any tolerance, because object 2
/// covers the point. Without `hit_test_point_all` there is no click that can
/// ever select them, which is the gap ui-spec §C.3 named.
#[test]
fn overlap_fixture_reports_every_object_under_the_point_front_most_first() {
    let doc = fixture("overlap.pdf");
    let (m, _page) = model(&doc);
    assert_eq!(count(&m), (3, 0, 0));

    // Centre: inside all three, front-most (last-painted) first.
    let centre = Point::new(150.0, 150.0);
    assert_eq!(hit_test_point_all(&m, centre, 1.0), vec![2, 1, 0]);
    // Between the outer two squares' edges: a two-deep stack.
    assert_eq!(
        hit_test_point_all(&m, Point::new(85.0, 85.0), 1.0),
        vec![1, 0]
    );
    // Inside the outermost only.
    assert_eq!(hit_test_point_all(&m, Point::new(35.0, 35.0), 1.0), vec![0]);

    // The topmost query is exactly the head, and — the point of the whole
    // feature — it is the ONLY answer it can give, at every tolerance.
    for tolerance in [0.0_f64, 1.0, 5.0, 50.0] {
        assert_eq!(hit_test_point(&m, centre, tolerance), Some(2));
        assert_eq!(
            hit_test_point(&m, centre, tolerance),
            hit_test_point_all(&m, centre, tolerance).first().copied()
        );
    }
}

/// A text object whose font defeats §9.10.2's ladder must come back
/// `Undecodable`, never as a decoded string of replacement characters.
///
/// Uses `text/identity-h-no-tounicode.pdf`, the corpus's designated honesty
/// metric: its PROVENANCE entry states that "a test that ever sees real text
/// come out of this file has found a fabrication". This is the object
/// model's version of that guard.
#[test]
fn a_font_whose_encoding_defeats_decoding_yields_no_preview_text() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text/identity-h-no-tounicode.pdf");
    let doc =
        Document::from_bytes(std::fs::read(&path).expect("fixture file")).expect("fixture parses");
    let (m, _page) = model(&doc);

    let text = m
        .objects
        .iter()
        .find_map(|o| match o {
            VectorObject::Text(t) => Some(t),
            _ => None,
        })
        .expect("a text object");
    assert_eq!(text.preview, TextPreview::Undecodable);
    // The font is still identified — which font cannot be read is most of
    // the value of the disclosure.
    assert_eq!(
        text.font.as_ref().and_then(|f| f.base_font.as_deref()),
        Some("ABCDEF+TestCID")
    );
}

#[test]
fn decomposition_is_read_only_over_the_document() {
    // The decomposition borrows the document immutably; there is no path
    // by which it could change a byte (the corpus-wide byte-inert proof is
    // the content-identity gate). Re-decomposing the same document twice
    // yields byte-identical object spans, and the document's own bytes are
    // untouched across the calls.
    let doc = fixture("paths.pdf");
    let before = doc.bytes().to_vec();
    let page = pages(&doc).unwrap().into_iter().next().unwrap();
    let a = decompose_page(&doc.view(), &page, Matrix::IDENTITY).unwrap();
    let b = decompose_page(&doc.view(), &page, Matrix::IDENTITY).unwrap();
    assert_eq!(a.objects.len(), b.objects.len());
    for (x, y) in a.objects.iter().zip(&b.objects) {
        assert_eq!(x.bytes(), y.bytes());
    }
    assert_eq!(
        doc.bytes(),
        before.as_slice(),
        "the document bytes are untouched"
    );
}

// ---------------------------------------------------------------------------
// Pass 220.0 -- `gs` (ExtGState). The operator had NO arm at all.
// ---------------------------------------------------------------------------

/// A page whose `/ExtGState` sets `/LW`, `/Font` and `/ca`, applied by `gs`.
fn page_with_extgstate(content: &str) -> Vec<u8> {
    let mut objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources \
         << /ExtGState << /GSw 5 0 R /GSinvis 6 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
        "<< /Type /ExtGState /LW 7.5 /Font [/F1 22] >>".to_owned(),
        "<< /Type /ExtGState /ca 0 >>".to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.drain(..).enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = offsets.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fn model_of(bytes: &[u8]) -> PageObjects {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    model(&doc).0
}

/// ★★★ A line width set by `gs` must reach the model.
///
/// # The defect
///
/// `decompose.rs` had no `gs` arm, so `/LW` was ignored and `line_width` kept
/// whatever the last `w` operator had set. That value feeds
/// `vector::hit::stroke_half_width`, which widens the stroke-proximity band —
/// so a wrong width means **the operator clicks a visible line and nothing
/// selects**, a symptom this project has had reported before from a different
/// cause.
///
/// # The `2 w` is in the fixture on purpose
///
/// Without it the stale value would be the §8.4.3.2 initial 1.0, which is also
/// what a plausible-but-wrong implementation reports — so the test would pass
/// on the bug. `2 w` makes the stale answer distinctive.
#[test]
fn a_line_width_set_by_an_extgstate_reaches_the_model() {
    let m = model_of(&page_with_extgstate("2 w /GSw gs 10 10 m 50 50 l S\n"));
    let VectorObject::Path(p) = &m.objects[0] else {
        panic!("expected a path")
    };
    assert!(
        (p.line_width - 7.5).abs() < 1e-9,
        "★ /LW 7.5 must win over the earlier `2 w`; got {} — the stale 2.0 is \
         the defect and 1.0 would mean the fixture never applied either",
        p.line_width
    );
}

/// The same operator's `/Font` size must reach the model too — and this is the
/// half that actually occurs.
///
/// Measured over 300 files of a 4,023-file corpus: `/Font` is the most common
/// `/ExtGState` entry (115 occurrences) and `/LW` appears **zero** times. A
/// stale font size gives a text object the wrong bounding box, which is the
/// same click-selects-nothing symptom one object kind over.
#[test]
fn a_font_size_set_by_an_extgstate_reaches_the_model() {
    let tall = model_of(&page_with_extgstate("BT /GSw gs 10 50 Td (Hi) Tj ET\n"));
    let short = model_of(&page_with_extgstate("BT /F1 8 Tf 10 50 Td (Hi) Tj ET\n"));
    let th = tall
        .objects
        .first()
        .map(|o| o.page_bbox().max.y - o.page_bbox().min.y);
    let sh = short
        .objects
        .first()
        .map(|o| o.page_bbox().max.y - o.page_bbox().min.y);
    match (th, sh) {
        (Some(a), Some(b)) => assert!(
            a > b,
            "/Font [.. 22] must produce a TALLER text box than `8 Tf`; got {a} vs {b}"
        ),
        _ => panic!("both fixtures must produce a text object"),
    }
}

/// A path made invisible by `/ca 0` is still listed — and is now COUNTED.
///
/// Not corrected: the object genuinely is in the content stream and a shell
/// may legitimately want to select it. What was missing was any way to know,
/// so an operator who clicks apparently empty space and selects something has
/// an explanation available.
#[test]
fn a_fully_transparent_path_is_disclosed_rather_than_hidden() {
    let m = model_of(&page_with_extgstate("/GSinvis gs 0 0 20 20 re f\n"));
    assert_eq!(m.objects.len(), 1, "still selectable");
    assert_eq!(
        m.diagnostics.paths_invisible_by_alpha, 1,
        "and disclosed as invisible"
    );
}

/// `q`/`Q` must restore an ExtGState-set width like any other state.
#[test]
fn an_extgstate_width_is_undone_by_a_grestore() {
    let m = model_of(&page_with_extgstate(
        "2 w q /GSw gs 0 0 m 5 5 l S Q 10 10 m 20 20 l S\n",
    ));
    let widths: Vec<f64> = m
        .objects
        .iter()
        .filter_map(|o| match o {
            VectorObject::Path(p) => Some(p.line_width),
            _ => None,
        })
        .collect();
    assert_eq!(widths.len(), 2);
    assert!((widths[0] - 7.5).abs() < 1e-9, "inside q/Q: {widths:?}");
    assert!(
        (widths[1] - 2.0).abs() < 1e-9,
        "★ after Q the width must return to the `2 w`, not stay at 7.5: {widths:?}"
    );
}

/// A `sh` shading produces no object, and says so.
///
/// The renderer paints it; this model does not list it. Before the counter,
/// an operator who could not select a visible gradient had no way to tell
/// "pdfcer does not model this" from "I missed it".
#[test]
fn a_shading_operator_is_counted_rather_than_silently_absent() {
    let m = model_of(&page_with_extgstate(
        "/Sh0 sh
",
    ));
    assert!(
        m.objects.is_empty(),
        "no object is produced for a shading -- that is the known gap"
    );
    assert_eq!(
        m.diagnostics.shadings_unmodelled, 1,
        "★ and the gap is DISCLOSED, which is the whole change"
    );
}

/// An optional-content section is counted, so a shell can tell a page has
/// layers whose visibility this model does not resolve.
#[test]
fn an_optional_content_section_is_counted() {
    let m = model_of(&page_with_extgstate(
        "/OC /MC0 BDC 0 0 10 10 re f EMC
",
    ));
    assert_eq!(m.diagnostics.oc_sections, 1);
    assert_eq!(
        m.objects.len(),
        1,
        "the content is still listed -- it is a disagreement with the renderer, not a reason to hide the object"
    );
}
