//! A [`GpuScene`] evaluated the way the shader evaluates it is the CPU
//! replay's raster, byte for byte (decision 223, fills and strokes part).
#![cfg(feature = "gpu")]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{EdgeAntialiasing, GpuRefusal, RenderOptions, record_page};

/// A 100 x 100 pt page painting `content` with `resources`.
fn page_with(content: &str, resources: &str) -> Document {
    page_sized(content, resources, (100, 100))
}

/// A `size` pt page painting `content` with `resources`.
fn page_sized(content: &str, resources: &str, size: (u32, u32)) -> Document {
    page_objects(content, resources, size, &[])
}

/// A `size` pt page painting `content` with `resources`, objects 5 on being
/// `extra`.
fn page_objects(content: &str, resources: &str, (w, h): (u32, u32), extra: &[String]) -> Document {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!("<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 {w} {h}] >>"),
        format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources {resources} >>"),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
    ];
    objects.extend_from_slice(extra);
    let mut out = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
    }
    let xref_at = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for off in offsets {
        out.push_str(&format!("{off:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
        objects.len() + 1
    ));
    Document::from_bytes(out.into_bytes()).expect("fixture must load")
}

/// Asserts the scene's evaluation equals the replay at each scale.
fn assert_matches(content: &str, resources: &str, what: &str) {
    assert_matches_at(
        &page_with(content, resources),
        &[1.0, 2.37, 300.0 / 72.0],
        what,
    );
}

fn assert_matches_at(doc: &Document, scales: &[f32], what: &str) {
    assert_matches_with(doc, scales, what, &RenderOptions::default());
}

fn assert_matches_with(doc: &Document, scales: &[f32], what: &str, options: &RenderOptions) {
    let page = page_tree::pages(doc).expect("page tree walks").remove(0);
    for &scale in scales {
        let list = record_page(&doc.view(), &page, scale, 1, options)
            .unwrap_or_else(|e| panic!("{what}: recording: {e}"));
        let cpu = list
            .replay_region(list.key(), page.crop_box)
            .unwrap_or_else(|e| panic!("{what}: replay: {e}"));
        let scene = list
            .gpu_scene(list.key(), page.crop_box)
            .unwrap_or_else(|e| panic!("{what} at {scale}: refused: {e}"));
        assert_eq!(
            (scene.width, scene.height),
            (cpu.pixmap.width(), cpu.pixmap.height())
        );
        let gpu = scene.evaluate();
        let want = cpu.pixmap.data();
        if let Some(i) = gpu.iter().zip(want).position(|(a, b)| a != b) {
            let p = i / 4;
            let w = scene.width as usize;
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

/// Over 8,191 pixels a side the CPU fills tile by tile, the path moved to
/// each tile and back; fills, a stroke and a clip crossing the seams.
#[test]
fn rasters_wider_and_taller_than_one_fill_tile() {
    let wide = "1 0 0 rg 1940.3 2 30.6 6 re f 0 0 1 rg 1900 1 m 2000 9 l 1950 9.5 l h f
                q 1945 0 m 1980 10 l 1930 10 l h W n 0 0.6 0 rg 1800 0 300 10 re f Q
                0.3 g 0.7 w 1700 8 m 2050 2 l S
";
    assert_matches_at(&page_sized(wide, "<< >>", (2010, 10)), &[4.2, 4.37], "wide");
    let tall = "1 0 0 rg 2 1940.3 6 30.6 re f 0 0 1 rg 1 1900 m 9 2000 l 9.5 1950 l h f
                q 0 1945 m 10 1980 l 10 1930 l h W n 0 0.6 0 rg 0 1800 10 300 re f Q
                0.3 g 0.7 w 8 1700 m 2 2050 l S
";
    assert_matches_at(&page_sized(tall, "<< >>", (10, 2010)), &[4.2], "tall");
}

#[test]
fn rectangles_at_fractional_positions() {
    assert_matches(
        "1 0 0 rg 10.3 10.7 40.2 30.1 re f 0 0.5 1 rg 30.15 25.6 50 50.33 re f\n",
        "<< >>",
        "rectangles",
    );
}

#[test]
fn curves_and_even_odd() {
    assert_matches(
        "0.2 0.6 0.3 rg 50 90 m 61 60 l 95 60 l 67 42 l 78 10 l 50 30 l 22 10 l 33 42 l 5 60 l 39 60 l h f*\n\
         0 0 0.8 rg 50 50 m 50 72 32 90 10 90 c 32 70 30 52 50 50 c f\n",
        "<< >>",
        "curves",
    );
}

#[test]
fn strokes_with_joins_and_dashes() {
    assert_matches(
        "0 0 0 RG 3.3 w 10 10 m 80 20 l 30 70 l S 1 j 0.7 w [4 2] 0 d 15 90 m 85 85 l 60 40 l S\n\
         0.3 w 5 50 m 95 51 l S\n",
        "<< >>",
        "strokes",
    );
}

#[test]
fn nested_clips() {
    assert_matches(
        "q 20 20 60 60 re W n q 50 50 30 0 360 m 10 10 m 90 30 l 40 95 l h W* n \
         0 0.4 0.9 rg 0 0 100 100 re f Q 1 0 0 rg 0 0 30 100 re f Q 0 1 0 rg 70 0 30 30 re f\n",
        "<< >>",
        "clips",
    );
}

#[test]
fn translucent_paint() {
    assert_matches(
        "/A gs 1 0 0 rg 10 10 50 50 re f 0 0 1 rg 40 40 50 50 re f q 30 30 40 40 re W n 0 1 0 RG 4 w 0 0 m 100 100 l S Q\n",
        "<< /ExtGState << /A << /ca 0.45 /CA 0.6 >> >> >>",
        "alpha",
    );
}

#[test]
fn text_glyph_outlines() {
    assert_matches(
        "BT /F1 18 Tf 8 60 Td (Glyphs &ag) Tj 0 -30 Td 0.5 0 0 rg (Wavy 0129) Tj ET\n",
        "<< /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >>",
        "text",
    );
}

#[test]
fn aliased_fills_strokes_and_clips() {
    let doc = page_with(
        "1 0 0 rg 10.3 10.6 50.2 49.9 re f 0 0 1 rg 40 40 m 95 52 l 60 90 l h f q 20 20 60 60 re W n 0 1 0 RG 3.3 w 0 0 m 100 100 l S Q /A gs 0 0 0 rg 5 70 80 20 re f\n",
        "<< /ExtGState << /A << /ca 0.45 >> >> >>",
    );
    let options = RenderOptions::default().with_edge_antialiasing(EdgeAntialiasing::Off);
    assert_matches_with(&doc, &[1.0, 2.37, 300.0 / 72.0], "aliased", &options);
}

/// Each low-precision blend mode over translucent and opaque paint, with
/// partial coverage, a clip and a stroke.
#[test]
fn separable_blend_modes() {
    for mode in [
        "Multiply",
        "Screen",
        "Overlay",
        "Darken",
        "Lighten",
        "HardLight",
        "Difference",
        "Exclusion",
    ] {
        let content = "1 0 0 rg 10 10 50 50 re f /A gs 0 0.5 0.8 rg 30.3 5 40 60.6 re f \
             /B gs 0.9 0.8 0.1 rg 20 40 m 95 52 l 50 95 l h f \
             q 15 15 70 70 re W n 0.2 0.9 0.4 RG 3.3 w 0 0 m 100 90 l S Q /N gs 0 0 1 rg 5 80 30 10 re f\n";
        let res = format!(
            "<< /ExtGState << /A << /ca 0.6 >> /B << /BM /{mode} >> /N << /BM /Normal /ca 0.7 >> >> >>"
        );
        assert_matches(content, &res, mode);
        let aliased = RenderOptions::default().with_edge_antialiasing(EdgeAntialiasing::Off);
        assert_matches_with(&page_with(content, &res), &[1.0, 2.37], mode, &aliased);
    }
}

#[test]
fn a_high_precision_blend_mode_is_refused() {
    let doc = page_with(
        "/B gs 1 0 0 rg 10 10 50 50 re f\n",
        "<< /ExtGState << /B << /BM /ColorBurn >> >> >>",
    );
    let page = page_tree::pages(&doc).expect("page tree walks").remove(0);
    let list = record_page(&doc.view(), &page, 1.0, 1, &RenderOptions::default()).expect("records");
    let r = list.gpu_scene(list.key(), page.crop_box);
    assert!(matches!(r, Err(GpuRefusal::Transparency)), "{r:?}");
}

/// A 5 x 3 RGB inline image, `/I` `interpolate`, as hexadecimal content.
fn inline_image(interpolate: bool) -> String {
    let px: String = (0..15)
        .map(|k| {
            format!(
                "{:02x}{:02x}{:02x}",
                (k * 53) % 256,
                (k * 97 + 40) % 256,
                (k * 31 + 200) % 256
            )
        })
        .collect();
    format!("BI /W 5 /H 3 /CS /RGB /BPC 8 /I {interpolate} /F /AHx ID {px}> EI")
}

#[test]
fn images_nearest_and_bilinear() {
    for interpolate in [false, true] {
        let img = inline_image(interpolate);
        let content = format!(
            "q 60 0 0 40 10.3 20.6 cm {img} Q q 30 20 -25 35 70 10 cm {img} Q \
             q 0.5 0.5 m 90 40 l 40 95 l h W n 80 0 0 80 10 10 cm {img} Q \
             q 0 0 1 rg 20 20 30 30 re f /A gs 50 0 0 50 25 25 cm {img} Q\n"
        );
        let what = if interpolate { "bilinear" } else { "nearest" };
        assert_matches(&content, "<< /ExtGState << /A << /ca 0.5 >> >> >>", what);
    }
}

#[test]
fn an_image_across_fill_tiles() {
    let img = inline_image(true);
    let content = format!("q 1990 0 0 9 10 0.5 cm {img} Q q 0 0 1 rg 1930 2 40 5 re f Q\n");
    assert_matches_at(
        &page_sized(&content, "<< >>", (2010, 10)),
        &[4.2],
        "wide image",
    );
}

/// A form XObject painted under `gs`, as `/Fm`, isolated group `group`.
fn group_page(content: &str, form: &str, group: &str, gs: &str) -> Document {
    let res = format!("<< /ExtGState << {gs} >> /XObject << /Fm 5 0 R >> >>");
    let fm = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Group << /S /Transparency {group} >> /Resources {res} /Length {} >>\nstream\n{form}endstream",
        form.len()
    );
    page_objects(content, &res, (100, 100), &[fm])
}

/// Transparency groups: group opacity, a group blend mode, nesting, a clip
/// inside and a translucent member, at every scale, anti-aliased or not.
#[test]
fn transparency_groups() {
    let form = "1 0 0 rg 10.3 10 50 50 re f /M gs 0 0 1 rg 30 30.6 50 50 re f \
                q 20 20 60 60 re W n 0 1 0 RG 3.3 w 0 0 m 100 100 l S Q\n";
    for (gs, what) in [
        ("/G << /ca 0.5 >>", "opacity"),
        ("/G << /BM /Multiply /ca 0.8 >>", "multiply group"),
        ("/G << /BM /Screen >>", "screen group"),
        ("/G << /BM /HardLight /ca 0.6 >>", "hard-light group"),
        ("/G << /BM /Difference >>", "difference group"),
    ] {
        let gs = format!("{gs} /M << /ca 0.6 >>");
        let content = "0.9 0.8 0.1 rg 0 40 100 20 re f /G gs /Fm Do 0 0.6 0.3 rg 70 5 20 90 re f\n";
        let doc = group_page(content, form, "/I true", &gs);
        assert_matches_at(&doc, &[1.0, 2.37, 300.0 / 72.0], what);
        let aliased = RenderOptions::default().with_edge_antialiasing(EdgeAntialiasing::Off);
        assert_matches_with(&doc, &[1.0, 2.37], what, &aliased);
    }
}

#[test]
fn nested_transparency_groups() {
    // The form paints itself again inside a translucent group (the outer
    // `/Fm` is the same object, so the recursion guard stops it).
    let form = "0 0 1 rg 20.5 20 40 40 re f /G gs 0.5 0 0 0.5 25 25 cm /Fm Do\n";
    let doc = group_page(
        "0.9 0.8 0.1 rg 0 40 100 20 re f /G gs /Fm Do\n",
        form,
        "/I true",
        "/G << /ca 0.7 >>",
    );
    assert_matches_at(&doc, &[1.0, 2.37], "nested");
}

#[test]
fn a_cancelled_pack_is_cancelled_not_refused() {
    let doc = page_with("1 0 0 rg 10 10 50 50 re f\n", "<< >>");
    let page = page_tree::pages(&doc).expect("page tree walks").remove(0);
    let list = record_page(&doc.view(), &page, 1.0, 1, &RenderOptions::default()).expect("records");
    let cancel = pdfcer_render::cancel::RenderCancel::new();
    let live = list.gpu_scene_cancellable(list.key(), page.crop_box, &cancel);
    assert_eq!(live.ok(), list.gpu_scene(list.key(), page.crop_box).ok());
    cancel.cancel();
    let r = list.gpu_scene_cancellable(list.key(), page.crop_box, &cancel);
    assert!(matches!(r, Err(GpuRefusal::Cancelled)), "{r:?}");
}
