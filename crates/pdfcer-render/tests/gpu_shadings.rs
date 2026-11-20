//! Shadings in a [`pdfcer_render::GpuScene`]: evaluated the way the shader
//! evaluates them, byte-identical to the CPU replay.
#![cfg(feature = "gpu")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, record_page};

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

/// Object 5: a free-form Gouraud mesh, two triangles.
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

/// Object 6: a PostScript calculator function of `(x, y)`.
fn calculator() -> Vec<u8> {
    stream(
        "/FunctionType 4 /Domain [0 120 0 120] /Range [0 1 0 1 0 1]",
        b"{ 120 div exch 120 div exch 2 copy add 2 div }",
    )
}

/// Resources naming every shading kind, `/G0` a `/ca 0.5` state.
fn resources() -> String {
    format!(
        "<< /Shading << /A0 {AXIAL} /R0 {RADIAL} /M0 5 0 R /F0 << /ShadingType 1 \
         /ColorSpace /DeviceRGB /Domain [0 120 0 120] /Function 6 0 R >> >> \
         /Pattern << /P0 << /PatternType 2 /Shading {AXIAL} >> \
         /P1 << /PatternType 2 /Shading 5 0 R /Matrix [1 0 0 1 -10 -5] >> >> \
         /ExtGState << /G0 << /ca 0.5 >> /G1 << /ca 0.37 >> >> >>"
    )
}

fn assert_matches(content: &str, what: &str) {
    let doc = page(content, &resources(), &[mesh(), calculator()]);
    let page = page_tree::pages(&doc).expect("page tree").remove(0);
    for scale in [1.0_f32, 2.37, 300.0 / 72.0] {
        let list = record_page(&doc.view(), &page, scale, 1, &RenderOptions::default())
            .unwrap_or_else(|e| panic!("{what}: recording: {e}"));
        let cpu = list
            .replay_region(list.key(), page.crop_box)
            .unwrap_or_else(|e| panic!("{what}: replay: {e}"));
        let scene = list
            .gpu_scene(list.key(), page.crop_box)
            .unwrap_or_else(|e| panic!("{what} at {scale}: refused: {e}"));
        let gpu = scene.evaluate();
        let want = cpu.pixmap.data();
        assert!(want.chunks(4).any(|p| p != [255; 4]), "{what}: paints");
        if let Some(i) = gpu.iter().zip(want).position(|(a, b)| a != b) {
            let (p, w) = (i / 4, scene.width as usize);
            panic!(
                "{what} at {scale}: pixel ({}, {}) is {:?}, replay {:?}",
                p % w,
                p / w,
                &gpu[p * 4..p * 4 + 4],
                &want[p * 4..p * 4 + 4]
            );
        }
    }
}

#[test]
fn axial_and_radial() {
    assert_matches("/A0 sh", "axial");
    assert_matches(
        "q 20 15 m 100 30 l 60 105 l h W n /R0 sh Q",
        "radial clipped",
    );
}

#[test]
fn function_based_and_mesh() {
    assert_matches("/F0 sh", "function-based");
    assert_matches(
        "0 1 0 rg 0 0 120 120 re f q 10 10 100 90 re W n /G0 gs /M0 sh Q",
        "mesh at half alpha",
    );
}

/// Translucent shadings over a translucent backdrop, where the composite's
/// rounding meets a destination alpha below one.
#[test]
fn translucent_over_translucent() {
    assert_matches(
        "q /G1 gs 0 0.4 0.8 rg 5 5 110 110 re f /A0 sh /G0 gs /R0 sh /M0 sh Q",
        "stacked alphas",
    );
}

#[test]
fn shading_pattern_fills_and_strokes() {
    assert_matches(
        "/Pattern cs /P0 scn 10 10 60 80 re f \
         q 50 20 m 110 40 l 80 110 l h W n /G0 gs /P1 scn 30 30 85 85 re f Q \
         q 3 w /Pattern CS /P0 SCN 15 100 m 105 60 l S Q",
        "pattern fill",
    );
}
