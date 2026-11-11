//! A mesh shading's triangles are charged to the page's coverage ceiling
//! (`Pass 703.1`, `X067` F079/F080): each triangle costs the pixels its
//! device bounds reach, weighted by `MESH_PIXEL_WEIGHT`, so overlapping
//! triangles and repeated `sh` paints cannot run unbounded. Past the
//! ceiling the rest of the mesh and of the page is left out and counted in
//! `content_truncated`.
//!
//! The ceiling is spent first with `Separation /None` fills, which charge
//! their reach but draw nothing (§8.6.6.4), leaving one canvas of headroom.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::RenderedPage;
use pdfcer_render::page_budget::{COVERAGE_PER_CANVAS, MESH_PIXEL_WEIGHT, MIN_COVERAGE};

/// The page side in points, rendered at scale 1.
const SIDE: u64 = 4000;

/// One `ShadingType 4` triangle per entry, each covering the same 400 x 400
/// box at user (1000, 1000), shaded green, the last one blue: a mesh drawn
/// to the end shows blue there, a cut one green. 16-bit coordinates over
/// `/Decode [0 4000 0 4000 ...]`, 8-bit RGB, an 8-bit flag.
fn mesh(triangles: usize) -> Vec<u8> {
    let coord = |v: f64| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let q = (v / 4000.0 * 65535.0).round() as u16;
        q.to_be_bytes()
    };
    let mut out = Vec::new();
    for i in 0..triangles {
        let colour = if i + 1 == triangles {
            [0, 0, 255]
        } else {
            [0, 255, 0]
        };
        for (x, y) in [(1000.0, 1000.0), (1400.0, 1000.0), (1000.0, 1400.0)] {
            out.push(0);
            out.extend_from_slice(&coord(x));
            out.extend_from_slice(&coord(y));
            out.extend_from_slice(&colour);
        }
    }
    out
}

fn pdf(content: &str, mesh: &[u8]) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {SIDE} {SIDE}] \
             /Resources << /ColorSpace << /CS0 [/Separation /None /DeviceGray \
             << /FunctionType 2 /Domain [0 1] /C0 [1] /C1 [0] /N 1 >>] >> \
             /Shading << /Sh0 5 0 R >> >> /Contents 4 0 R >>"
        )
        .into_bytes(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
        .into_bytes(),
    ];
    let mut shading = format!(
        "<< /ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 16 \
         /BitsPerComponent 8 /BitsPerFlag 8 \
         /Decode [0 4000 0 4000 0 1 0 1 0 1] /Length {} >>\nstream\n",
        mesh.len()
    )
    .into_bytes();
    shading.extend_from_slice(mesh);
    shading.extend_from_slice(b"\nendstream");
    objects.push(shading);

    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

/// Spend all but one canvas of the ceiling, paint the mesh with `sh`, then
/// a red square.
fn page(triangles: usize) -> Vec<u8> {
    let canvas = SIDE * SIDE;
    let limit = (canvas * COVERAGE_PER_CANVAS).max(MIN_COVERAGE);
    let mut content = String::from("/CS0 cs 1 scn\n");
    for _ in 0..(limit / canvas - 1) {
        content.push_str("0 0 4000 4000 re f\n");
    }
    content.push_str("/Sh0 sh\n1 0 0 rg 100 100 200 200 re f\n");
    pdf(&content, &mesh(triangles))
}

fn render(bytes: Vec<u8>) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("synthetic file parses");
    let pages = page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page(&doc, &pages[0], 1.0).expect("renders")
}

fn rgb(p: &RenderedPage, x: u64, y: u64) -> (u8, u8, u8) {
    let y = u32::try_from(SIDE - y).expect("fits");
    let px = p
        .pixmap
        .pixel(u32::try_from(x).expect("fits"), y)
        .expect("on the page");
    (px.red(), px.green(), px.blue())
}

#[test]
fn overlapping_mesh_triangles_spend_the_ceiling() {
    // One canvas of headroom buys SIDE^2 / MESH_PIXEL_WEIGHT pixels of
    // triangle bounds: about 2 of these 400 x 400 triangles, not 1000.
    let p = render(page(1000));
    assert!(p.diagnostics.content_truncated > 0);
    assert_eq!(
        rgb(&p, 1100, 1100),
        (0, 255, 0),
        "the mesh draws its first triangles and stops before its last"
    );
    assert_ne!(
        rgb(&p, 200, 200),
        (255, 0, 0),
        "the fill after the cut mesh must be left out"
    );
}

#[test]
fn a_mesh_within_the_ceiling_draws_and_the_page_continues() {
    let fits = usize::try_from(SIDE * SIDE / MESH_PIXEL_WEIGHT / (400 * 400) / 2).expect("fits");
    assert!(fits >= 1);
    let p = render(page(fits));
    assert_eq!(p.diagnostics.content_truncated, 0);
    assert_eq!(
        rgb(&p, 1100, 1100),
        (0, 0, 255),
        "drawn to its last triangle"
    );
    assert_eq!(rgb(&p, 200, 200), (255, 0, 0));
}
