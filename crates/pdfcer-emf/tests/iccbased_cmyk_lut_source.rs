//! # An `ICCBased /N 4` colour whose profile is a v2 lut printer profile is
//! converted through THAT profile, on every route (X074)
//!
//! ISO 32000-2 §8.6.5.5: an `ICCBased` colour's numbers mean what its
//! embedded profile says they mean, not what the output intent says. The
//! source here is a v2.2 printer-class profile whose `A2B0`/`B2A0` are
//! `lut16Type` (`mft2`, ICC.1:2022 clause 10.10) tables of a simple press
//! model with cyan and magenta SWAPPED; the output intent is the same model
//! unswapped. So `C = 1` in the source space is the output intent's
//! magenta, and every route must show it as magenta:
//!
//! * the raster (the composite through the colorant buffer),
//! * the EMF export (brush `COLORREF` for the fill, bitmap texels for the
//!   image),
//! * the separation plates (the magenta plate inks it, the cyan plate not),
//! * an additive page's raster (fill, image and shading, screen route),
//! * the print metafile (`print_export_with`).
//!
//! An embedded profile icccer cannot parse is counted in
//! `Diagnostics::icc_profiles_unmodelled` on the raster, EMF and print
//! outcomes.
//!
//! The oracle is the same page's own `DeviceCMYK 0 1 0 0` (and, with
//! `preserve_calibrated_cmyk`, `1 0 0 0`) fill, compared in CIELAB (ΔE*ab,
//! limit 2). Both profiles are written by this file; nothing is downloaded.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use std::fmt::Write as _;

use super::export_emf::{record_offsets, u32_at};
use super::minimal_pdf::build;
use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::settings::PageBlendSpaceSource;
use pdfcer_emf::{EmfOptions, export_emf, print_export_with, walk_records};
use pdfcer_render::plates::{Plates, render_plates};
use pdfcer_render::{RenderOptions, render_page_with};

const SCALE: f32 = 2.0;
const DE_LIMIT: f64 = 2.0;

// ---------------------------------------------------------------------------
// The synthetic profiles (ICC.1:2022 clauses 7.2 header, 7.3 tag table,
// 10.10 lut16Type; legacy 16-bit PCSLAB, Tables 42/43)
// ---------------------------------------------------------------------------

/// sRGB primaries adapted to D50 (Bradford): linear RGB → PCS XYZ.
const M: [[f64; 3]; 3] = [
    [0.436_074_7, 0.385_064_9, 0.143_080_4],
    [0.222_504_5, 0.716_878_6, 0.060_616_9],
    [0.013_932_2, 0.097_104_5, 0.714_173_3],
];
const D50: [f64; 3] = [0.9642, 1.0, 0.8249];

/// The press model: four tints → CIELAB (D50). Each process ink absorbs one
/// third of the spectrum, black absorbs all of it, and the paper is the PCS
/// white so relative colorimetry is the identity on the PCS.
fn model_lab(c: f64, m: f64, y: f64, k: f64) -> [f64; 3] {
    let lin = |t: f64| 0.03 + 0.97 * (1.0 - t) * (1.0 - k);
    let rgb = [lin(c), lin(m), lin(y)];
    let mut xyz = [0.0; 3];
    for (i, row) in M.iter().enumerate() {
        xyz[i] = row.iter().zip(rgb).map(|(a, b)| a * b).sum::<f64>();
    }
    // Scale so the paper (all tints 0) is exactly D50.
    let white: Vec<f64> = M.iter().map(|r| r.iter().sum::<f64>()).collect();
    for i in 0..3 {
        xyz[i] *= D50[i] / white[i];
    }
    xyz_to_lab(xyz, D50)
}

/// The model's inverse, no black (GCR 0): CIELAB → four tints.
fn model_inverse(lab: [f64; 3]) -> [f64; 4] {
    let mut xyz = lab_to_xyz(lab, D50);
    let white: Vec<f64> = M.iter().map(|r| r.iter().sum::<f64>()).collect();
    for i in 0..3 {
        xyz[i] *= white[i] / D50[i];
    }
    let rgb = solve3(M, xyz);
    let tint = |v: f64| (1.0 - (v - 0.03) / 0.97).clamp(0.0, 1.0);
    [tint(rgb[0]), tint(rgb[1]), tint(rgb[2]), 0.0]
}

fn solve3(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    let det = |a: [[f64; 3]; 3]| {
        a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
            - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
    };
    let d = det(m);
    let mut out = [0.0; 3];
    for (col, o) in out.iter_mut().enumerate() {
        let mut a = m;
        for row in 0..3 {
            a[row][col] = v[row];
        }
        *o = det(a) / d;
    }
    out
}

fn xyz_to_lab(xyz: [f64; 3], white: [f64; 3]) -> [f64; 3] {
    let f = |t: f64| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (
        f(xyz[0] / white[0]),
        f(xyz[1] / white[1]),
        f(xyz[2] / white[2]),
    );
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn lab_to_xyz(lab: [f64; 3], white: [f64; 3]) -> [f64; 3] {
    let fy = (lab[0] + 16.0) / 116.0;
    let fx = fy + lab[1] / 500.0;
    let fz = fy - lab[2] / 200.0;
    let inv = |t: f64| {
        if t.powi(3) > 216.0 / 24389.0 {
            t.powi(3)
        } else {
            (116.0 * t - 16.0) * 27.0 / 24389.0
        }
    };
    [white[0] * inv(fx), white[1] * inv(fy), white[2] * inv(fz)]
}

fn u16_unit(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// Legacy 16-bit PCSLAB (ICC.1:2022 Tables 42/43): `L* 100 = FF00h`,
/// `a*/b* 0 = 8000h`.
fn legacy_lab(lab: [f64; 3]) -> [u16; 3] {
    let l = (lab[0] * 65280.0 / 100.0).round().clamp(0.0, 65535.0) as u16;
    let ab = |v: f64| ((v + 128.0) * 256.0).round().clamp(0.0, 65535.0) as u16;
    [l, ab(lab[1]), ab(lab[2])]
}

fn legacy_lab_decode(v: [f64; 3]) -> [f64; 3] {
    let raw = |n: f64| n * 65535.0;
    [
        raw(v[0]) * 100.0 / 65280.0,
        raw(v[1]) / 256.0 - 128.0,
        raw(v[2]) / 256.0 - 128.0,
    ]
}

/// One `mft2` tag with identity curves and an identity matrix (10.10:
/// "shall be an identity matrix unless the input is in the PCSXYZ colour
/// space"), the CLUT filled by `node` per grid node, first input slowest.
fn mft2(inputs: usize, outputs: usize, grid: usize, node: impl Fn(&[f64]) -> Vec<u16>) -> Vec<u8> {
    let mut t = b"mft2\0\0\0\0".to_vec();
    t.extend_from_slice(&[inputs as u8, outputs as u8, grid as u8, 0]);
    for i in 0..9 {
        let one: i32 = if i % 4 == 0 { 0x0001_0000 } else { 0 };
        t.extend_from_slice(&one.to_be_bytes());
    }
    t.extend_from_slice(&2u16.to_be_bytes());
    t.extend_from_slice(&2u16.to_be_bytes());
    for _ in 0..inputs {
        t.extend_from_slice(&[0, 0, 0xFF, 0xFF]);
    }
    let mut idx = vec![0usize; inputs];
    for _ in 0..grid.pow(inputs as u32) {
        let at: Vec<f64> = idx.iter().map(|i| *i as f64 / (grid - 1) as f64).collect();
        for v in node(&at) {
            t.extend_from_slice(&v.to_be_bytes());
        }
        for d in (0..inputs).rev() {
            idx[d] += 1;
            if idx[d] < grid {
                break;
            }
            idx[d] = 0;
        }
    }
    for _ in 0..outputs {
        t.extend_from_slice(&[0, 0, 0xFF, 0xFF]);
    }
    t
}

fn s15(v: f64) -> [u8; 4] {
    ((v * 65536.0).round() as i32).to_be_bytes()
}

/// A v2.2 printer-class CMYK profile, Lab PCS, of the press model — with
/// cyan and magenta exchanged when `swap_cm`.
pub(crate) fn press_profile(swap_cm: bool) -> Vec<u8> {
    let perm = move |t: [f64; 4]| if swap_cm { [t[1], t[0], t[2], t[3]] } else { t };
    let a2b = mft2(4, 3, 9, |n| {
        let [c, m, y, k] = perm([n[0], n[1], n[2], n[3]]);
        legacy_lab(model_lab(c, m, y, k)).to_vec()
    });
    let b2a = mft2(3, 4, 33, |n| {
        perm(model_inverse(legacy_lab_decode([n[0], n[1], n[2]])))
            .iter()
            .map(|v| u16_unit(*v))
            .collect()
    });
    let mut wtpt = b"XYZ \0\0\0\0".to_vec();
    for v in D50 {
        wtpt.extend_from_slice(&s15(v));
    }
    let mut desc = b"desc\0\0\0\0".to_vec();
    desc.extend_from_slice(&10u32.to_be_bytes());
    desc.extend_from_slice(b"synthetic\0");
    desc.extend_from_slice(&[0u8; 8 + 2 + 1 + 67]);
    let cprt = b"text\0\0\0\0none\0".to_vec();
    assemble(&[
        (*b"desc", desc),
        (*b"cprt", cprt),
        (*b"wtpt", wtpt),
        (*b"A2B0", a2b),
        (*b"B2A0", b2a),
    ])
}

fn assemble(tags: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut header = vec![0u8; 128];
    header[8..12].copy_from_slice(&0x0220_0000u32.to_be_bytes());
    header[12..16].copy_from_slice(b"prtr");
    header[16..20].copy_from_slice(b"CMYK");
    header[20..24].copy_from_slice(b"Lab ");
    header[24..36].copy_from_slice(&[0x07, 0xEA, 0, 11, 0, 11, 0, 0, 0, 0, 0, 0]);
    header[36..40].copy_from_slice(b"acsp");
    for (i, v) in D50.iter().enumerate() {
        header[68 + 4 * i..72 + 4 * i].copy_from_slice(&s15(*v));
    }
    let mut table = (tags.len() as u32).to_be_bytes().to_vec();
    let mut body = Vec::new();
    let mut at = 128 + 4 + 12 * tags.len();
    for (sig, data) in tags {
        table.extend_from_slice(sig);
        table.extend_from_slice(&(at as u32).to_be_bytes());
        table.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        let pad = (4 - data.len() % 4) % 4;
        body.extend(std::iter::repeat_n(0u8, pad));
        at += data.len() + pad;
    }
    let mut out = header;
    out.extend_from_slice(&table);
    out.extend_from_slice(&body);
    let len = out.len() as u32;
    out[0..4].copy_from_slice(&len.to_be_bytes());
    out
}

// ---------------------------------------------------------------------------
// The page
// ---------------------------------------------------------------------------

fn hex_stream(dict: &str, bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2 + 1);
    for b in bytes {
        write!(hex, "{b:02X}").unwrap();
    }
    hex.push('>');
    format!(
        "<< {dict} /Filter /ASCIIHexDecode /Length {} >>\nstream\n{hex}\nendstream",
        hex.len()
    )
}

/// Box `i` (0..4) spans x `10 + 100 i .. 90 + 100 i` on a 400 × 100 pt page.
const BOX_ICC_FILL: usize = 0;
const BOX_DEVICE_M: usize = 1;
const BOX_ICC_IMAGE: usize = 2;
const BOX_DEVICE_C: usize = 3;

/// A 400 × 100 pt page composited in `DeviceCMYK` under a CMYK output
/// intent: the `ICCBased` `C = 1` fill, a `DeviceCMYK` magenta, a 2 × 2
/// `ICCBased` image of `C = 1` texels, a `DeviceCMYK` cyan. `src` is the
/// embedded profile's bytes.
pub(crate) fn page_bytes(src: &[u8], content: &str) -> Vec<u8> {
    page_with(
        src,
        content,
        "/Group << /S /Transparency /CS /DeviceCMYK >>",
    )
}

/// The same page with no transparency group. Rendered with
/// [`additive`] options it composites additively, so every colour reaches
/// the raster through its screen (sRGB) answer.
fn additive_page_bytes(src: &[u8], content: &str) -> Vec<u8> {
    page_with(src, content, "")
}

fn page_with(src: &[u8], content: &str, group: &str) -> Vec<u8> {
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /MediaBox [0 0 400 100] {group} \
         /Resources << /ColorSpace << /C [/ICCBased 5 0 R] >> /XObject << /Im 7 0 R >> \
         /Shading << /Sh 8 0 R >> >> >>"
    );
    let stream = format!(
        "<< /Length {} >>\nstream\n{content}endstream",
        content.len()
    );
    let image = hex_stream(
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 /BitsPerComponent 8 \
         /ColorSpace [/ICCBased 5 0 R]",
        &[0xFF, 0, 0, 0].repeat(4),
    );
    build(&[
        (
            1,
            "<< /Type /Catalog /Pages 2 0 R /OutputIntents [<< /Type /OutputIntent \
             /S /GTS_PDFX /OutputConditionIdentifier (synthetic) /DestOutputProfile 6 0 R >>] >>",
        ),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, &page),
        (4, &stream),
        (5, &hex_stream("/N 4", src)),
        (6, &hex_stream("/N 4", &press_profile(false))),
        (7, &image),
        // A constant `C = 1` axial shading in the same space (§8.7.4.5.3).
        (
            8,
            "<< /ShadingType 2 /ColorSpace [/ICCBased 5 0 R] /Coords [0 0 400 0] \
             /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0 0] /C1 [1 0 0 0] /N 1 >> >>",
        ),
    ])
}

const ALL_FOUR: &str = "/C cs 1 0 0 0 sc 10 10 80 80 re f \
    0 1 0 0 k 110 10 80 80 re f \
    q 80 0 0 80 210 10 cm /Im Do Q \
    1 0 0 0 k 310 10 80 80 re f\n";

fn load(bytes: Vec<u8>) -> (Document, page_tree::Page) {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    (doc, p)
}

fn options(preserve: bool) -> RenderOptions {
    RenderOptions::default().with_preserve_calibrated_cmyk(preserve)
}

/// `options` with the page's blending space taken from the device, not the
/// CMYK output intent (ISO 32000-1 §11.4.7), so an ungrouped page is
/// additive and its colours keep the output intent as their destination.
fn additive(preserve: bool) -> RenderOptions {
    options(preserve).with_page_blend_space_source(PageBlendSpaceSource::DeviceNative)
}

// ---------------------------------------------------------------------------
// Measurement
// ---------------------------------------------------------------------------

/// Mean sRGB over the middle half of box `i` in an RGBA pixmap's bytes.
fn box_mean(data: &[u8], width: u32, height: u32, i: usize) -> [f64; 3] {
    let x0 = ((30.0 + 100.0 * i as f32) * SCALE) as u32;
    let x1 = ((70.0 + 100.0 * i as f32) * SCALE) as u32;
    let (y0, y1) = (height * 3 / 10, height * 7 / 10);
    let mut sum = [0.0; 3];
    let mut n = 0.0;
    for y in y0..y1 {
        for x in x0..x1 {
            let o = ((y * width + x) * 4) as usize;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += f64::from(data[o + c]);
            }
            n += 1.0;
        }
    }
    sum.map(|s| s / n)
}

fn srgb_to_lab(rgb: [f64; 3]) -> [f64; 3] {
    let lin = |v: f64| {
        let v = v / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let l = rgb.map(lin);
    let d65 = [
        [0.412_456_4, 0.357_576_1, 0.180_437_5],
        [0.212_672_9, 0.715_152_2, 0.072_175_0],
        [0.019_333_9, 0.119_192_0, 0.950_304_1],
    ];
    let xyz = d65.map(|r| r[0] * l[0] + r[1] * l[1] + r[2] * l[2]);
    xyz_to_lab(xyz, [0.950_47, 1.0, 1.088_83])
}

fn delta_e(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (la, lb) = (srgb_to_lab(a), srgb_to_lab(b));
    ((la[0] - lb[0]).powi(2) + (la[1] - lb[1]).powi(2) + (la[2] - lb[2]).powi(2)).sqrt()
}

fn assert_close(what: &str, got: [f64; 3], want: [f64; 3]) {
    let de = delta_e(got, want);
    assert!(
        de <= DE_LIMIT,
        "{what}: {got:?} is ΔE {de:.2} from {want:?} (limit {DE_LIMIT})"
    );
}

fn raster_boxes(preserve: bool) -> [[f64; 3]; 4] {
    let (doc, p) = load(page_bytes(&press_profile(true), ALL_FOUR));
    let r = render_page_with(&doc, &p, SCALE, &options(preserve)).expect("render");
    assert!(
        r.diagnostics.cmyk_buffer_engaged,
        "the page composites in ink"
    );
    let px = &r.pixmap;
    [0, 1, 2, 3].map(|i| box_mean(px.data(), px.width(), px.height(), i))
}

// ---------------------------------------------------------------------------
// Raster
// ---------------------------------------------------------------------------

#[test]
fn raster_shows_the_source_profiles_cyan_as_the_output_intents_magenta() {
    let b = raster_boxes(false);
    assert_close("ICCBased fill", b[BOX_ICC_FILL], b[BOX_DEVICE_M]);
    assert_close("ICCBased image", b[BOX_ICC_IMAGE], b[BOX_DEVICE_M]);
    // The control: magenta and cyan must be far apart, or the two
    // assertions above could not fail.
    assert!(delta_e(b[BOX_DEVICE_M], b[BOX_DEVICE_C]) > 20.0);
}

#[test]
fn raster_preserved_keeps_the_authored_cyan() {
    let b = raster_boxes(true);
    assert_close("ICCBased fill", b[BOX_ICC_FILL], b[BOX_DEVICE_C]);
    assert_close("ICCBased image", b[BOX_ICC_IMAGE], b[BOX_DEVICE_C]);
}

/// Box 3 is the `ICCBased` shading here, clipped to its box; the
/// `DeviceCMYK` cyan control is the EMF test's job.
const ADDITIVE: &str = "/C cs 1 0 0 0 sc 10 10 80 80 re f \
    0 1 0 0 k 110 10 80 80 re f \
    q 80 0 0 80 210 10 cm /Im Do Q \
    q 310 10 80 80 re W n /Sh sh Q\n";

fn additive_boxes(preserve: bool) -> [[f64; 3]; 4] {
    let (doc, p) = load(additive_page_bytes(&press_profile(true), ADDITIVE));
    let r = render_page_with(&doc, &p, SCALE, &additive(preserve)).expect("render");
    assert!(!r.diagnostics.cmyk_buffer_engaged, "the page is additive");
    let px = &r.pixmap;
    [0, 1, 2, 3].map(|i| box_mean(px.data(), px.width(), px.height(), i))
}

/// The screen route: no colorant buffer, so the fill, image and shading
/// each reach the raster as sRGB, and each must still be the source
/// profile's colour (the defect X074 reported on the raster print route).
#[test]
fn an_additive_raster_shows_the_source_profiles_cyan_as_magenta() {
    let b = additive_boxes(false);
    assert_close("ICCBased fill", b[BOX_ICC_FILL], b[BOX_DEVICE_M]);
    assert_close("ICCBased image", b[BOX_ICC_IMAGE], b[BOX_DEVICE_M]);
    assert_close("ICCBased shading", b[3], b[BOX_DEVICE_M]);
}

#[test]
fn an_additive_raster_preserved_keeps_the_authored_cyan() {
    let b = additive_boxes(true);
    let c = raster_boxes(true)[BOX_DEVICE_C];
    assert_close("ICCBased fill", b[BOX_ICC_FILL], c);
    assert_close("ICCBased image", b[BOX_ICC_IMAGE], c);
    assert_close("ICCBased shading", b[3], c);
}

// ---------------------------------------------------------------------------
// EMF
// ---------------------------------------------------------------------------

/// Each `EMR_CREATEBRUSHINDIRECT`'s `COLORREF` (R, G, B on disk), in order.
fn brushes(emf: &[u8]) -> Vec<[f64; 3]> {
    let records = walk_records(emf).unwrap();
    records
        .iter()
        .zip(record_offsets(&records))
        .filter(|((t, _), _)| *t == 0x27)
        .map(|(_, off)| [0, 1, 2].map(|i| f64::from(emf[off + 16 + i])))
        .collect()
}

/// The centre texel (BGRA, premultiplied, opaque here) of each
/// `EMR_ALPHABLEND` bitmap, as RGB.
fn bitmap_centres(emf: &[u8]) -> Vec<[f64; 3]> {
    let records = walk_records(emf).unwrap();
    records
        .iter()
        .zip(record_offsets(&records))
        .filter(|((t, _), _)| *t == 0x72)
        .map(|(_, off)| {
            let (w, h) = (u32_at(emf, off + 100), u32_at(emf, off + 104));
            let at = off + 148 + (((h / 2) * w + w / 2) * 4) as usize;
            [emf[at + 2], emf[at + 1], emf[at]].map(f64::from)
        })
        .collect()
}

fn emf_of(src: &[u8], content: &str, preserve: bool) -> pdfcer_emf::EmfExport {
    let (doc, p) = load(page_bytes(src, content));
    export_emf(
        &doc,
        &p,
        &options(preserve),
        &EmfOptions::default().with_raster_dpi(144.0),
    )
    .expect("emf export")
}

/// `[ICCBased fill, DeviceCMYK magenta, DeviceCMYK cyan]` brushes and the
/// image's centre texel.
fn emf_colours(preserve: bool) -> ([[f64; 3]; 3], [f64; 3]) {
    let src = press_profile(true);
    let brush = |content: &str| {
        let b = brushes(&emf_of(&src, content, preserve).emf);
        assert_eq!(b.len(), 1, "{content}: one brush");
        b[0]
    };
    let fills = [
        brush("/C cs 1 0 0 0 sc 10 10 80 80 re f\n"),
        brush("0 1 0 0 k 10 10 80 80 re f\n"),
        brush("1 0 0 0 k 10 10 80 80 re f\n"),
    ];
    let image = emf_of(&src, "q 80 0 0 80 10 10 cm /Im Do Q\n", preserve);
    let texels = bitmap_centres(&image.emf);
    assert_eq!(texels.len(), 1, "one image bitmap");
    (fills, texels[0])
}

#[test]
fn emf_brush_and_bitmap_show_the_source_profiles_cyan_as_magenta() {
    let ([icc, m, c], image) = emf_colours(false);
    assert_close("ICCBased brush", icc, m);
    assert_close("ICCBased image bitmap", image, m);
    assert!(delta_e(m, c) > 20.0);
}

#[test]
fn emf_preserved_keeps_the_authored_cyan() {
    let ([icc, _, c], image) = emf_colours(true);
    assert_close("ICCBased brush", icc, c);
    assert_close("ICCBased image bitmap", image, c);
}

// ---------------------------------------------------------------------------
// Plates
// ---------------------------------------------------------------------------

/// The grey (0 = full ink) of box `i` on the plate named `ink`.
fn plate_grey(plates: &Plates, ink: &str, i: usize) -> f64 {
    let Plates::Separated(list) = plates else {
        panic!("the page separates");
    };
    let plate = list.iter().find(|p| p.ink == ink).expect("plate exists");
    let px = &plate.pixmap;
    box_mean(px.data(), px.width(), px.height(), i)[0]
}

fn plates(preserve: bool) -> Plates {
    let (doc, p) = load(page_bytes(&press_profile(true), ALL_FOUR));
    render_plates(&doc.view(), &p, SCALE, &options(preserve)).expect("plates")
}

#[test]
fn plates_put_the_source_profiles_cyan_on_the_magenta_plate() {
    let pl = plates(false);
    for (what, i) in [("fill", BOX_ICC_FILL), ("image", BOX_ICC_IMAGE)] {
        let (cyan, magenta) = (plate_grey(&pl, "Cyan", i), plate_grey(&pl, "Magenta", i));
        assert!(magenta < 40.0, "{what}: magenta plate grey {magenta}");
        assert!(cyan > 230.0, "{what}: cyan plate grey {cyan}");
    }
}

#[test]
fn plates_preserved_keep_the_authored_cyan_plate() {
    let pl = plates(true);
    for (what, i) in [("fill", BOX_ICC_FILL), ("image", BOX_ICC_IMAGE)] {
        let (cyan, magenta) = (plate_grey(&pl, "Cyan", i), plate_grey(&pl, "Magenta", i));
        assert!(cyan < 40.0, "{what}: cyan plate grey {cyan}");
        assert!(magenta > 230.0, "{what}: magenta plate grey {magenta}");
    }
}

// ---------------------------------------------------------------------------
// Print (the metafile a printer is sent)
// ---------------------------------------------------------------------------

fn print_of(src: &[u8], content: &str) -> pdfcer_emf::EmfExport {
    // Additive: a subtractive blending space is a reason to print as an
    // image.
    let (doc, p) = load(additive_page_bytes(src, content));
    let render = additive(false).with_print_intent(true);
    print_export_with(&doc.view(), &p, &render, &EmfOptions::default()).expect("prints as vector")
}

#[test]
fn the_print_metafile_shows_the_source_profiles_cyan_as_magenta() {
    let src = press_profile(true);
    let icc = brushes(&print_of(&src, "/C cs 1 0 0 0 sc 10 10 80 80 re f\n").emf);
    let m = brushes(&print_of(&src, "0 1 0 0 k 10 10 80 80 re f\n").emf);
    assert_close("ICCBased print brush", icc[0], m[0]);
}

// ---------------------------------------------------------------------------
// A profile icccer cannot model is counted, never silent
// ---------------------------------------------------------------------------

/// 200 bytes with no `acsp` signature: not an ICC profile (ICC.1:2022 7.2.9).
fn garbage_profile() -> Vec<u8> {
    (0u8..200).collect()
}

#[test]
fn an_unparseable_profile_is_counted_on_raster_emf_and_print() {
    let content = "/C cs 1 0 0 0 sc 10 10 80 80 re f\n";
    let (doc, p) = load(page_bytes(&garbage_profile(), content));
    let raster = render_page_with(&doc, &p, SCALE, &options(false)).expect("render");
    assert_eq!(raster.diagnostics.icc_profiles_unmodelled, 1, "raster");
    let emf = emf_of(&garbage_profile(), content, false);
    assert_eq!(emf.outcome.diagnostics.icc_profiles_unmodelled, 1, "emf");
    let print = print_of(&garbage_profile(), content);
    assert_eq!(
        print.outcome.diagnostics.icc_profiles_unmodelled, 1,
        "print"
    );
}

/// The control: the modellable profile on the same page counts nothing,
/// so the count above is the refusal and not the page.
#[test]
fn a_modellable_profile_counts_no_fallback() {
    let content = "/C cs 1 0 0 0 sc 10 10 80 80 re f q 80 0 0 80 210 10 cm /Im Do Q\n";
    let (doc, p) = load(page_bytes(&press_profile(true), content));
    let raster = render_page_with(&doc, &p, SCALE, &options(false)).expect("render");
    assert_eq!(raster.diagnostics.icc_profiles_unmodelled, 0);
    assert!(raster.diagnostics.icc_managed_paints >= 2);
    assert_eq!(raster.diagnostics.icc_unmanaged_paints, 0);
}
