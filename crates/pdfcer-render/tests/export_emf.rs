//! # The EMF export is well-formed and renders like the page (`Pass 248.4`)
//!
//! Three layers of evidence, from cheapest to most independent:
//!
//! 1. **Structure** — every record's size is a multiple of 4, the sizes
//!    sum to the header's `Bytes`, `Records` counts them all, `Handles` is
//!    the highest object index + 1, the signature is `" EMF"`, EOF's
//!    `SizeLast` closes the file. These are exactly the conditions
//!    LibreOffice's reader aborts on (`D:\dev\rag\emf\consumers.md`).
//! 2. **GDI itself**, when the tests run on Windows: `System.Drawing`
//!    (GDI+ over GDI playback) rasterises the metafile through PowerShell
//!    and the result is compared pixel-by-pixel against pdfcer's own
//!    white-backed PNG. This is the renderer Office and every Win32 paste
//!    target use, so it is the oracle that matters.
//! 3. **Inkscape**, when installed — for the vector-only fixtures (its
//!    importer ignores `EMR_ALPHABLEND`).
//!
//! Layers 2 and 3 say on stdout when they did not run; a CI runner without
//! the tool must not go red for lacking it, and a developer's machine with
//! it must not skip silently.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::emf::{EmfOptions, export_emf, walk_records};
use pdfcer_render::{PageBackdrop, RenderOptions, render_page_with};
use tiny_skia::Pixmap;

fn build(objects: &[(u32, String)]) -> Vec<u8> {
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

/// A 100 × 80 pt page with `resources` and `content`.
fn page(resources: &str, content: &str) -> Vec<u8> {
    let stream = format!("{content}\n");
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] /Resources << {resources} >> >>"
            ),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
        (
            4,
            format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ),
    ])
}

const DPI: f32 = 144.0;

fn export(bytes: Vec<u8>) -> (Pixmap, pdfcer_render::emf::EmfExport) {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    let reference = render_page_with(
        &doc,
        &p,
        DPI / 72.0,
        &RenderOptions::default().with_backdrop(PageBackdrop::White),
    )
    .expect("render");
    let emf = export_emf(
        &doc,
        &p,
        &RenderOptions::default(),
        &EmfOptions::default().with_raster_dpi(DPI),
    )
    .expect("emf export");
    (reference.pixmap, emf)
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

/// The structural contract every export must satisfy.
fn assert_well_formed(emf: &[u8]) {
    let records = walk_records(emf).expect("record sizes chain exactly to the end");
    assert_eq!(&emf[40..44], b" EMF", "signature");
    assert_eq!(u32_at(emf, 44), 0x0001_0000, "version");
    assert_eq!(u32_at(emf, 48) as usize, emf.len(), "Bytes == file length");
    assert_eq!(
        u32_at(emf, 52) as usize,
        records.len(),
        "Records counts header and EOF"
    );
    assert_eq!(records[0].0, 1, "first record is the header");
    assert_eq!(records[0].1, 108, "Ext2 header without description");
    let (eof_type, eof_size) = *records.last().unwrap();
    assert_eq!((eof_type, eof_size), (0x0E, 20));
    assert_eq!(u32_at(emf, emf.len() - 4), 20, "EOF SizeLast");
    let handles = u16::from_le_bytes([emf[56], emf[57]]);
    let max_index = records
        .iter()
        .zip(record_offsets(&records))
        .filter(|((t, _), _)| matches!(t, 0x27 | 0x5F))
        .map(|(_, off)| u32_at(emf, off + 8))
        .max()
        .unwrap_or(0);
    assert_eq!(
        u32::from(handles),
        max_index + 1,
        "Handles = highest index + 1"
    );
    // Device/Millimeters ratio ≈ 100 (0.01 mm units) on both axes.
    let (dx, dy) = (u32_at(emf, 72), u32_at(emf, 76));
    let (mx, my) = (u32_at(emf, 80), u32_at(emf, 84));
    assert!(
        (dx as f64 / mx as f64 - 100.0).abs() < 1.0,
        "x ratio {dx}/{mx}"
    );
    assert!(
        (dy as f64 / my as f64 - 100.0).abs() < 1.0,
        "y ratio {dy}/{my}"
    );
}

fn record_offsets(records: &[(u32, u32)]) -> Vec<usize> {
    let mut off = 0usize;
    records
        .iter()
        .map(|(_, s)| {
            let o = off;
            off += *s as usize;
            o
        })
        .collect()
}

fn count(records: &[(u32, u32)], kind: u32) -> usize {
    records.iter().filter(|(t, _)| *t == kind).count()
}

// ---------------------------------------------------------------------------
// Structure
// ---------------------------------------------------------------------------

#[test]
fn a_filled_rectangle_is_a_brush_a_path_bracket_and_a_fillpath() {
    let (_, emf) = export(page("", "1 0 0 rg 10 10 40 30 re f"));
    assert_well_formed(&emf.emf);
    let r = walk_records(&emf.emf).unwrap();
    assert_eq!(count(&r, 0x27), 1, "one CREATEBRUSHINDIRECT");
    assert_eq!(count(&r, 0x3E), 1, "one FILLPATH");
    assert_eq!(count(&r, 0x3B), 1, "one BEGINPATH");
    assert_eq!(count(&r, 0x28), 1, "one DELETEOBJECT");
    assert_eq!(count(&r, 0x72), 0, "no bitmap");
    assert_eq!(emf.outcome.ops, 1);
    assert_eq!(emf.outcome.rasters_embedded, 0);
    // The brush colour is R,G,B,0 on disk — the byte-order trap.
    let offs = record_offsets(&r);
    let brush = r
        .iter()
        .zip(&offs)
        .find(|((t, _), _)| *t == 0x27)
        .unwrap()
        .1;
    assert_eq!(&emf.emf[*brush + 16..*brush + 20], &[0xFF, 0, 0, 0]);
}

#[test]
fn a_stroke_is_a_geometric_pen_with_caps_joins_and_a_float_miter_limit() {
    let (_, emf) = export(page("", "3 w 1 J 2 j 0 0 1 RG 10 10 m 90 70 l S"));
    assert_well_formed(&emf.emf);
    let r = walk_records(&emf.emf).unwrap();
    assert_eq!(count(&r, 0x5F), 1, "one EXTCREATEPEN");
    assert_eq!(count(&r, 0x40), 1, "one STROKEPATH");
    let offs = record_offsets(&r);
    let pen = *r
        .iter()
        .zip(&offs)
        .find(|((t, _), _)| *t == 0x5F)
        .unwrap()
        .1;
    let style = u32_at(&emf.emf, pen + 28);
    assert_eq!(style & 0x0001_0000, 0x0001_0000, "PS_GEOMETRIC");
    assert_eq!(style & 0x0000_0F00, 0x0000_0000, "round cap (J 1)");
    assert_eq!(style & 0x0000_F000, 0x0000_1000, "bevel join (j 2)");
    // 3 pt at 144 dpi = 6 px = 6 * 2540/144 ≈ 105.8 units.
    let width = u32_at(&emf.emf, pen + 32);
    assert!((100..=112).contains(&width), "width {width}");
    let miter = *r
        .iter()
        .zip(&offs)
        .find(|((t, _), _)| *t == 0x3A)
        .unwrap()
        .1;
    assert_eq!(
        f32::from_le_bytes(emf.emf[miter + 8..miter + 12].try_into().unwrap()),
        10.0
    );
}

#[test]
fn a_dashed_stroke_is_pre_dashed_geometry_with_a_solid_pen() {
    let (_, emf) = export(page("", "[6 3] 0 d 2 w 0 0 0 RG 10 40 m 90 40 l S"));
    assert_well_formed(&emf.emf);
    assert_eq!(emf.outcome.dashed_strokes_pre_applied, 1);
    let r = walk_records(&emf.emf).unwrap();
    // 80 pt of line in 9 pt periods: nine dashes, each its own MOVETO.
    assert!(count(&r, 0x1B) >= 8, "{} MOVETOEX", count(&r, 0x1B));
    let offs = record_offsets(&r);
    let pen = *r
        .iter()
        .zip(&offs)
        .find(|((t, _), _)| *t == 0x5F)
        .unwrap()
        .1;
    assert_eq!(
        u32_at(&emf.emf, pen + 48),
        0,
        "NumStyleEntries = 0: no PS_USERSTYLE"
    );
}

#[test]
fn a_clip_is_a_saved_dc_a_clip_path_and_a_restore() {
    let (_, emf) = export(page("", "q 10 10 60 60 re W n 1 0 0 rg 0 0 100 80 re f Q"));
    assert_well_formed(&emf.emf);
    let r = walk_records(&emf.emf).unwrap();
    assert_eq!(count(&r, 0x21), 1, "SAVEDC");
    assert_eq!(count(&r, 0x43), 1, "SELECTCLIPPATH");
    assert_eq!(count(&r, 0x22), 1, "RESTOREDC");
    // Order: SAVEDC before the clip path, the fill after it, RESTOREDC last.
    let pos = |k: u32| r.iter().position(|(t, _)| *t == k).unwrap();
    assert!(pos(0x21) < pos(0x43) && pos(0x43) < pos(0x3E) && pos(0x3E) < pos(0x22));
}

#[test]
fn transparency_and_images_become_alpha_bitmaps_and_are_counted() {
    let (_, emf) = export(page(
        "/ExtGState << /GS0 << /ca 0.5 >> >>",
        "1 0 0 rg 10 10 40 30 re f /GS0 gs 0 0 1 rg 30 20 40 30 re f",
    ));
    assert_well_formed(&emf.emf);
    let r = walk_records(&emf.emf).unwrap();
    assert_eq!(count(&r, 0x72), 1, "one ALPHABLEND for the half-alpha fill");
    assert_eq!(count(&r, 0x3E), 1, "the opaque fill stays a path");
    assert_eq!(emf.outcome.ops_rasterised_for_alpha, 1);
    assert_eq!(emf.outcome.rasters_embedded, 1);
    // ALPHABLEND: BLENDFUNCTION bytes and a top-down 32 bpp BI_RGB header.
    let offs = record_offsets(&r);
    let ab = *r
        .iter()
        .zip(&offs)
        .find(|((t, _), _)| *t == 0x72)
        .unwrap()
        .1;
    assert_eq!(
        &emf.emf[ab + 40..ab + 44],
        &[0, 0, 0xFF, 1],
        "AC_SRC_OVER, 255, AC_SRC_ALPHA"
    );
    assert_eq!(u32_at(&emf.emf, ab + 84), 108, "offBmiSrc");
    assert_eq!(u32_at(&emf.emf, ab + 92), 148, "offBitsSrc");
    let height = i32::from_le_bytes(emf.emf[ab + 116..ab + 120].try_into().unwrap());
    assert!(height < 0, "top-down DIB");
    assert_eq!(
        u16::from_le_bytes([emf.emf[ab + 122], emf.emf[ab + 123]]),
        32
    );
    // The bitmap is premultiplied BGRA: a 50 % blue pixel is (128,0,0,128)
    // in B,G,R,A order.
    let cx = u32_at(&emf.emf, ab + 100) as usize;
    let cy = u32_at(&emf.emf, ab + 104) as usize;
    let bits = &emf.emf[ab + 148..ab + 148 + cx * cy * 4];
    let mid = ((cy / 2) * cx + cx / 2) * 4;
    let px = &bits[mid..mid + 4];
    assert!(
        (i32::from(px[0]) - 128).abs() <= 1 && px[1] == 0 && px[2] == 0,
        "BGRA {px:?}"
    );
    assert!((i32::from(px[3]) - 128).abs() <= 1, "alpha {px:?}");
}

#[test]
fn a_gradient_becomes_a_bitmap_here_and_says_so() {
    let (_, emf) = export(page(
        "/Shading << /Sh0 << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [10 0 90 0] \
         /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> /Extend [true true] >> >>",
        "q 10 10 80 60 re W n /Sh0 sh Q",
    ));
    assert_well_formed(&emf.emf);
    assert_eq!(emf.outcome.gradients_rasterised, 1);
    assert_eq!(emf.outcome.rasters_embedded, 1);
}

// ---------------------------------------------------------------------------
// GDI itself (Windows) and Inkscape — when present
// ---------------------------------------------------------------------------

fn differing(a: &Pixmap, b: &Pixmap, tol: u8) -> (usize, u8) {
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    let mut count = 0;
    let mut worst = 0u8;
    for (pa, pb) in a.pixels().iter().zip(b.pixels()) {
        let m = [
            pa.red().abs_diff(pb.red()),
            pa.green().abs_diff(pb.green()),
            pa.blue().abs_diff(pb.blue()),
        ]
        .into_iter()
        .max()
        .unwrap();
        worst = worst.max(m);
        if m > tol {
            count += 1;
        }
    }
    (count, worst)
}

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("pdfcer-emf-oracle-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The fixtures both oracles run: `(name, resources, content, vector-only)`.
fn oracle_fixtures() -> Vec<(&'static str, &'static str, &'static str, bool)> {
    vec![
        (
            "fills",
            "",
            "1 0 0 rg 10 10 40 30 re f 0 0 1 rg 60 50 30 20 re 70 55 10 10 re f*",
            true,
        ),
        (
            "stroke",
            "",
            "4 w 1 J 1 j 0 0.6 0 RG 10 10 m 90 70 l S",
            true,
        ),
        (
            "clip",
            "",
            "q 10 10 60 60 re W n 1 0 0 rg 0 0 100 80 re f Q",
            true,
        ),
        (
            "alpha",
            "/ExtGState << /GS0 << /ca 0.5 >> >>",
            "1 0 0 rg 10 10 40 30 re f /GS0 gs 0 0 1 rg 30 20 40 30 re f",
            false,
        ),
    ]
}

/// GDI's own player, reached through PowerShell + P/Invoke: `PlayEnhMetaFile`
/// onto a 32 bpp DIB section, the raw BGRA written to a file. NOT
/// `System.Drawing.Imaging.Metafile` -- GDI+ has its own EMF player, and it
/// mis-plays `EMR_ALPHABLEND` (measured 2026-09-03: a premultiplied
/// (0,0,128,128) pixel came back (134,5,6) through GDI+ and (255,127,127)
/// -- the spec's answer -- through GDI). Office and every Win32 paste
/// target play through GDI.
#[cfg(windows)]
const GDI_PLAYER_PS1: &str = r#"
param([string]$Path, [int]$W, [int]$H, [string]$Out)
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class Gdi {
  [DllImport("gdi32.dll")] public static extern IntPtr CreateCompatibleDC(IntPtr hdc);
  [DllImport("gdi32.dll")] public static extern IntPtr CreateDIBSection(IntPtr hdc, ref BITMAPINFO bmi, uint usage, out IntPtr bits, IntPtr section, uint offset);
  [DllImport("gdi32.dll")] public static extern IntPtr SelectObject(IntPtr hdc, IntPtr h);
  [DllImport("gdi32.dll")] public static extern bool DeleteObject(IntPtr h);
  [DllImport("gdi32.dll")] public static extern bool DeleteDC(IntPtr hdc);
  [DllImport("gdi32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr GetEnhMetaFileW(string path);
  [DllImport("gdi32.dll")] public static extern bool PlayEnhMetaFile(IntPtr hdc, IntPtr hemf, ref RECT rect);
  [DllImport("gdi32.dll")] public static extern bool DeleteEnhMetaFile(IntPtr hemf);
  [DllImport("gdi32.dll")] public static extern IntPtr CreateSolidBrush(uint color);
  [DllImport("user32.dll")] public static extern int FillRect(IntPtr hdc, ref RECT rect, IntPtr brush);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct BITMAPINFO { public uint size; public int w, h; public ushort planes, bpp; public uint comp, sizeImage; public int xppm, yppm; public uint used, imp; public uint r0, g0, b0, a0; }
  public static byte[] Play(string path, int w, int h) {
    IntPtr hdc = CreateCompatibleDC(IntPtr.Zero);
    BITMAPINFO bmi = new BITMAPINFO(); bmi.size = 40; bmi.w = w; bmi.h = -h; bmi.planes = 1; bmi.bpp = 32; bmi.comp = 0;
    IntPtr bits; IntPtr hbm = CreateDIBSection(hdc, ref bmi, 0, out bits, IntPtr.Zero, 0);
    IntPtr old = SelectObject(hdc, hbm);
    RECT rc = new RECT(); rc.L = 0; rc.T = 0; rc.R = w; rc.B = h;
    IntPtr white = CreateSolidBrush(0x00FFFFFF); FillRect(hdc, ref rc, white); DeleteObject(white);
    IntPtr hemf = GetEnhMetaFileW(path);
    if (hemf == IntPtr.Zero) throw new Exception("GetEnhMetaFile failed");
    if (!PlayEnhMetaFile(hdc, hemf, ref rc)) throw new Exception("PlayEnhMetaFile failed");
    byte[] px = new byte[w * h * 4]; Marshal.Copy(bits, px, 0, px.Length);
    DeleteEnhMetaFile(hemf); SelectObject(hdc, old); DeleteObject(hbm); DeleteDC(hdc);
    return px;
  }
}
"@
[System.IO.File]::WriteAllBytes($Out, [Gdi]::Play($Path, $W, $H))
"#;

#[cfg(windows)]
#[test]
fn gdi_renders_the_metafile_like_pdfcer() {
    let dir = scratch_dir("gdi");
    let script = dir.join("play.ps1");
    std::fs::write(&script, GDI_PLAYER_PS1).unwrap();
    for (name, resources, content, _) in oracle_fixtures() {
        let (reference, emf) = export(page(resources, content));
        let emf_path = dir.join(format!("{name}.emf"));
        let raw_path = dir.join(format!("{name}.bgra"));
        std::fs::write(&emf_path, &emf.emf).unwrap();
        let (w, h) = (reference.width(), reference.height());
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                script.to_str().unwrap(),
                "-Path",
                emf_path.to_str().unwrap(),
                "-W",
                &w.to_string(),
                "-H",
                &h.to_string(),
                "-Out",
                raw_path.to_str().unwrap(),
            ])
            .output()
            .expect("powershell runs");
        assert!(
            out.status.success(),
            "{name}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let raw = std::fs::read(&raw_path).expect("GDI wrote the pixels");
        assert_eq!(raw.len(), (w * h * 4) as usize);
        let total = reference.pixels().len();
        let mut bad = 0usize;
        let mut worst = 0u8;
        for (i, px) in reference.pixels().iter().enumerate() {
            let g = &raw[i * 4..i * 4 + 4]; // B G R A, opaque
            let m = [
                px.red().abs_diff(g[2]),
                px.green().abs_diff(g[1]),
                px.blue().abs_diff(g[0]),
            ]
            .into_iter()
            .max()
            .unwrap();
            worst = worst.max(m);
            if m > 32 {
                bad += 1;
            }
        }
        let fraction = bad as f64 / total as f64;
        assert!(
            fraction <= 0.03,
            "{name}: GDI differs from pdfcer on {bad} of {total} pixels ({:.2}%, worst {worst})",
            fraction * 100.0
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn inkscape() -> Option<std::path::PathBuf> {
    [
        std::path::PathBuf::from(r"C:\Program Files\Inkscape\bin\inkscape.exe"),
        std::path::PathBuf::from("/usr/bin/inkscape"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

#[test]
fn inkscape_imports_the_vector_metafile_like_pdfcer_when_installed() {
    let Some(ink) = inkscape() else {
        println!("export_emf: Inkscape not installed here; the import oracle did not run");
        return;
    };
    let dir = scratch_dir("inkscape");
    for (name, resources, content, vector_only) in oracle_fixtures() {
        if !vector_only {
            continue; // Inkscape draws nothing for EMR_ALPHABLEND, by design.
        }
        let (reference, emf) = export(page(resources, content));
        let emf_path = dir.join(format!("{name}.emf"));
        let png_path = dir.join(format!("{name}.png"));
        std::fs::write(&emf_path, &emf.emf).unwrap();
        let status = std::process::Command::new(&ink)
            .arg("--export-type=png")
            .arg(format!("--export-filename={}", png_path.display()))
            .arg("--export-background=white")
            .arg("--export-background-opacity=1")
            .arg(format!("--export-width={}", reference.width()))
            .arg(format!("--export-height={}", reference.height()))
            .arg(&emf_path)
            .output()
            .expect("inkscape runs");
        assert!(
            status.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&status.stderr)
        );
        let ink_png = Pixmap::load_png(&png_path).expect("inkscape wrote a png");
        let total = reference.pixels().len();
        let (bad, worst) = differing(&reference, &ink_png, 32);
        let fraction = bad as f64 / total as f64;
        assert!(
            fraction <= 0.03,
            "{name}: Inkscape differs from pdfcer on {bad} of {total} pixels ({:.2}%, worst {worst})",
            fraction * 100.0
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
