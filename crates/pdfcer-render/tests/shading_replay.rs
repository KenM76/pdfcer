//! A shading on the cache recorder: the page records, and replaying any
//! region is byte-identical to rendering that region directly.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_render::{
    BandBlocker, PoisonReason, RenderError, RenderOptions, record_page, render_page_region,
};

/// A 120 x 120 pt page painting `content` with `resources`; `extra` are
/// objects 5 on, as raw bytes so a mesh stream may carry binary.
fn page(content: &str, resources: &str, extra: &[Vec<u8>]) -> Document {
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 120 120] >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources {resources} >>")
            .into_bytes(),
        stream("", content.as_bytes()),
    ];
    objects.extend_from_slice(extra);
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = out.len();
    let n = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(out).expect("fixture must load")
}

/// A stream object whose dictionary holds `keys` and a `/Length`.
fn stream(keys: &str, data: &[u8]) -> Vec<u8> {
    let mut s = format!("<< {keys} /Length {} >>\nstream\n", data.len()).into_bytes();
    s.extend_from_slice(data);
    s.extend_from_slice(b"\nendstream");
    s
}

const AXIAL: &str = "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [10 10 110 90] \
    /Extend [true true] /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] \
    /C1 [0 0 1] /N 1 >> >>";
const RADIAL: &str = "<< /ShadingType 3 /ColorSpace /DeviceRGB /Coords [40 50 5 70 60 50] \
    /Extend [true false] /Function << /FunctionType 2 /Domain [0 1] /C0 [1 1 0] \
    /C1 [0 0.5 0] /N 1.5 >> >>";

/// Object 5: a free-form Gouraud mesh (§8.7.4.5.5), two triangles, 8-bit
/// coordinates, components and flags.
fn mesh() -> Vec<u8> {
    let data: [u8; 36] = [
        0, 0, 0, 255, 0, 0, //
        0, 255, 0, 0, 255, 0, //
        0, 0, 255, 0, 0, 255, //
        0, 255, 255, 255, 255, 0, //
        0, 40, 200, 0, 255, 255, //
        0, 200, 40, 255, 0, 255, //
    ];
    stream(
        "/ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 8 \
         /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 120 0 120 0 1 0 1 0 1]",
        &data,
    )
}

/// Object 6: a PostScript calculator function of `(x, y)` (§7.10.5).
fn calculator() -> Vec<u8> {
    stream(
        "/FunctionType 4 /Domain [0 120 0 120] /Range [0 1 0 1 0 1]",
        b"{ 120 div exch 120 div exch 2 copy add 2 div }",
    )
}

/// Every region and scale of `doc` replays exactly as it renders.
fn assert_replays(doc: &Document, what: &str) {
    let page = page_tree::pages(doc).expect("page tree").remove(0);
    let options = RenderOptions::default();
    let regions = [
        ("whole page", page.crop_box),
        ("interior", Rect::from_corners(30.0, 25.0, 95.0, 80.0)),
        ("corner", Rect::from_corners(70.0, 70.0, 120.0, 120.0)),
    ];
    for scale in [1.0_f32, 2.37] {
        let list = record_page(&doc.view(), &page, scale, 3, &options)
            .unwrap_or_else(|e| panic!("{what} @{scale}: recording: {e}"));
        assert_eq!(list.band_exact(), Err(BandBlocker::Shading), "{what}");
        for (name, region) in regions {
            let at = format!("{what} @{scale} [{name}]");
            let fresh = render_page_region(&doc.view(), &page, scale, region, &options)
                .unwrap_or_else(|e| panic!("{at}: fresh: {e}"));
            assert!(
                fresh.diagnostics.shading.painted > 0,
                "{at}: control paints"
            );
            let replay = list
                .replay_region(list.key(), region)
                .unwrap_or_else(|e| panic!("{at}: replay: {e}"));
            let (a, b) = (fresh.pixmap.data(), replay.pixmap.data());
            assert_eq!(a.len(), b.len(), "{at}: size");
            assert!(a.iter().any(|&v| v != 0), "{at}: something painted");
            let differ = a.iter().zip(b).filter(|(x, y)| x != y).count();
            assert_eq!(differ, 0, "{at}: {differ} of {} bytes differ", a.len());
        }
    }
}

#[test]
fn sh_of_every_kind_replays_exactly() {
    for (what, shading) in [("axial", AXIAL), ("radial", RADIAL)] {
        let res = format!("<< /Shading << /S0 {shading} >> >>");
        assert_replays(&page("/S0 sh", &res, &[]), what);
        let clipped = "q 20 15 m 100 30 l 60 105 l h W n /S0 sh Q";
        assert_replays(&page(clipped, &res, &[]), &format!("{what} clipped"));
    }
    let res = "<< /Shading << /M0 5 0 R /F0 << /ShadingType 1 /ColorSpace /DeviceRGB \
               /Domain [0 120 0 120] /Function 6 0 R >> >> >>";
    let objects = [mesh(), calculator()];
    assert_replays(&page("/F0 sh", res, &objects), "function-based");
    let alpha = format!(
        "{} /ExtGState << /G0 << /ca 0.5 >> >> >>",
        &res[..res.len() - 2]
    );
    let content = "0 1 0 rg 0 0 120 120 re f q 10 10 100 90 re W n /G0 gs /M0 sh Q";
    assert_replays(&page(content, &alpha, &objects), "mesh");
}

#[test]
fn a_shading_pattern_fill_replays_exactly() {
    let res = format!(
        "<< /Pattern << /P0 << /PatternType 2 /Shading {AXIAL} >> \
         /P1 << /PatternType 2 /Shading 5 0 R /Matrix [1 0 0 1 -10 -5] >> >> \
         /ExtGState << /G0 << /ca 0.6 >> >> >>"
    );
    let content = "/Pattern cs /P0 scn 10 10 60 80 re f \
                   q 50 20 m 110 40 l 80 110 l h W n /G0 gs /P1 scn 30 30 85 85 re f Q \
                   q 3 w /Pattern CS /P0 SCN 15 100 m 105 60 l S Q";
    assert_replays(&page(content, &res, &[mesh()]), "pattern fill");
}

/// What still cannot be recorded refuses by name.
#[test]
fn a_tiling_pattern_still_refuses_by_name() {
    let tile = stream(
        "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 \
         /YStep 10 /Resources << >>",
        b"0 0 5 5 re f",
    );
    let doc = page(
        "/Pattern cs /T0 scn 0 0 120 120 re f",
        "<< /Pattern << /T0 5 0 R >> >>",
        &[tile],
    );
    let page = page_tree::pages(&doc).expect("page tree").remove(0);
    match record_page(&doc.view(), &page, 1.0, 0, &RenderOptions::default()) {
        Err(RenderError::PageNotRecordable { reason }) => {
            assert_eq!(reason, PoisonReason::TilingPattern);
        }
        Err(other) => panic!("wrong error: {other}"),
        Ok(_) => panic!("a tiling pattern recorded"),
    }
}
