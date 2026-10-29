// Test code: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;
use crate::Placement;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GdiFlush, PatBlt, SelectObject, WHITENESS,
};

/// A 100 x 100 pt page: a red square, a blue stroked frame, a green
/// triangle — opaque, so it takes the vector route.
fn page_pdf() -> Vec<u8> {
    let content = "1 0 0 rg 20 20 40 40 re f 0 0 1 RG 4 w 10 10 80 80 re S \
                   0 0.6 0 rg 60 60 m 90 60 l 75 90 l f";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>".to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    buf
}

/// Play `emf` for `page` onto a white `w` x `h` memory DC at `dpi`; returns
/// its pixels as BGRX, top row first.
fn play_into_memory(emf: &[u8], page: &PageBitmap, dpi: i32, w: i32, h: i32) -> Vec<u8> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: 40,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: every handle is created, used and deleted in this function;
    // `bits` points into the DIB section, read only after `GdiFlush` and
    // before the section is deleted.
    unsafe {
        let dc = CreateCompatibleDC(None);
        let mut bits = std::ptr::null_mut();
        let dib = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            .expect("DIB section");
        let old = SelectObject(dc, dib.into());
        assert!(PatBlt(dc, 0, 0, w, h, WHITENESS).as_bool());
        play_page(dc, emf, page, (dpi, dpi)).expect("plays");
        let _ = GdiFlush();
        let len = (w * h * 4) as usize;
        let out = std::slice::from_raw_parts(bits.cast::<u8>(), len).to_vec();
        SelectObject(dc, old);
        let _ = DeleteObject(dib.into());
        let _ = DeleteDC(dc);
        out
    }
}

#[test]
fn a_played_metafile_matches_the_raster_and_lands_at_the_placement() {
    let doc = pdfcer_core::document::Document::from_bytes(page_pdf()).unwrap();
    let page = pdfcer_core::page_tree::pages(&doc).unwrap().remove(0);
    let view = pdfcer_core::view::DocumentView::new(&doc, doc.bytes(), doc.version());
    let options = pdfcer_render::RenderOptions::default();
    let emf = pdfcer_render::emf::export_emf_view(
        &view,
        &page,
        &options,
        &pdfcer_render::emf::EmfOptions::default().with_raster_dpi(144.0),
    )
    .unwrap()
    .emf;
    let raster = pdfcer_render::render_page_with_view(&view, &page, 2.0, &options)
        .unwrap()
        .pixmap;
    assert_eq!((raster.width(), raster.height()), (200, 200));

    // 144 dpi = 2 px per pt; offset (5, 10) pt = (10, 20) px.
    let placed = PageBitmap {
        width: 0,
        height: 0,
        rgba: Vec::new(),
        placement: Placement {
            scale: 1.0,
            offset_x_pt: 5.0,
            offset_y_pt: 10.0,
            clipped: false,
        },
        page_pt: (100.0, 100.0),
    };
    assert_eq!(dest_rect(&placed, (144, 144)), (10, 20, 200, 200));
    let (w, h) = (220, 230);
    let played = play_into_memory(&emf, &placed, 144, w, h);
    let at = |x: i32, y: i32| {
        let i = ((y * w + x) * 4) as usize;
        [played[i + 2], played[i + 1], played[i]]
    };

    // Outside the destination rectangle stays white.
    assert_eq!(at(5, 5), [255, 255, 255]);
    assert_eq!(at(215, 225), [255, 255, 255]);
    // The red square's centre (40 pt, 60 pt from the top) is red.
    assert_eq!(at(10 + 80, 20 + 120), [255, 0, 0]);

    let mut differing = 0;
    for y in 0..200 {
        for x in 0..200 {
            let i = ((y * 200 + x) * 4) as usize;
            let px = &raster.data()[i..i + 4];
            let over = |c: u8| c.saturating_add(255 - px[3]);
            let want = [over(px[0]), over(px[1]), over(px[2])];
            let got = at(10 + x, 20 + y);
            if want.iter().zip(got).any(|(a, b)| a.abs_diff(b) > 64) {
                differing += 1;
            }
        }
    }
    // Edge antialiasing only: GDI fills are aliased, the raster is not.
    assert!(differing < 200 * 200 * 3 / 100, "{differing} pixels differ");
}
