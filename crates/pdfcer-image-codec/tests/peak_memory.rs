//! Measured peak heap of every decode path against its predictor
//! (decision 227, Pass 780.1): `measured <= predicted <= 1.5 x measured`.
//!
//! The peak comes from a counting `GlobalAlloc` over `System`. Its counters
//! are thread-local, so tests running in parallel do not see each other's
//! allocations; every decoder measured here is single-threaded.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use pdfcer_image_codec::decode_image;
use pdfcer_image_codec::peak::predict_peak;
use pdfcer_model::document::Document;
use pdfcer_model::object::{Dict, Name, Object};

struct Counting;

thread_local! {
    static LIVE: Cell<usize> = const { Cell::new(0) };
    static PEAK: Cell<usize> = const { Cell::new(0) };
}

fn grow(n: usize) {
    let _ = LIVE.try_with(|live| {
        let now = live.get().saturating_add(n);
        live.set(now);
        let _ = PEAK.try_with(|p| p.set(p.get().max(now)));
    });
}

fn shrink(n: usize) {
    let _ = LIVE.try_with(|live| live.set(live.get().saturating_sub(n)));
}

// SAFETY: every method forwards to `System` with the caller's arguments
// unchanged; the counters are side bookkeeping that never allocate.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded verbatim; the caller upholds `alloc`'s contract.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            grow(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as `alloc`.
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            grow(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from this allocator with `layout`.
        unsafe { System.dealloc(ptr, layout) };
        shrink(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: as `dealloc`, and `new_size` is the caller's.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            // Counted as old and new held at once: a moving realloc does
            // hold both, and an in-place one is not promised.
            grow(new_size);
            shrink(layout.size());
        }
        p
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// Peak bytes live above the starting level while `f` runs.
fn peak_of<T>(f: impl FnOnce() -> T) -> (usize, T) {
    let start = LIVE.with(Cell::get);
    PEAK.with(|p| p.set(start));
    let out = f();
    let peak = PEAK.with(Cell::get);
    (peak.saturating_sub(start), out)
}

fn empty_document() -> Document {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (num, body) in [
        (1u32, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
    ] {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 3\n0000000000 65535 f\r\n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(buf).expect("synthetic document parses")
}

fn image_dict(filter: Option<&[u8]>, width: u32, height: u32, bpc: i64, cs: &[u8]) -> Dict {
    let mut d = Dict::new();
    if let Some(f) = filter {
        d.insert(Name::from(&b"Filter"[..]), Object::Name(Name::from(f)));
    }
    d.insert(Name::from(&b"Width"[..]), Object::Integer(i64::from(width)));
    d.insert(
        Name::from(&b"Height"[..]),
        Object::Integer(i64::from(height)),
    );
    d.insert(Name::from(&b"BitsPerComponent"[..]), Object::Integer(bpc));
    d.insert(Name::from(&b"ColorSpace"[..]), Object::Name(Name::from(cs)));
    d
}

/// Measures one decode, predicts it, and asserts
/// `measured <= predicted <= 1.5 * measured`. Prints one report row.
fn check(label: &str, dict: &Dict, raw: &[u8]) {
    let doc = empty_document();
    let predicted =
        predict_peak(&doc.view(), dict, raw).unwrap_or_else(|| panic!("{label}: no prediction"));
    let (measured, decoded) = peak_of(|| decode_image(&doc, dict, raw, false));
    decoded.unwrap_or_else(|e| panic!("{label}: decode failed: {e}"));
    let measured = u64::try_from(measured).expect("fits");
    let ratio = predicted as f64 / measured as f64;
    eprintln!("| {label} | {measured} | {predicted} | {ratio:.3} |");
    assert!(
        predicted >= measured,
        "{label}: predicted {predicted} under measured {measured}"
    );
    assert!(
        predicted.saturating_mul(2) <= measured.saturating_mul(3),
        "{label}: predicted {predicted} over 1.5 x measured {measured}"
    );
}

/// A deterministic, non-flat test picture of `channels` interleaved bytes.
fn picture(width: u32, height: u32, channels: usize) -> Vec<u8> {
    let mut px = Vec::with_capacity(width as usize * height as usize * channels);
    for y in 0..height {
        for x in 0..width {
            for c in 0..channels {
                let v = (x.wrapping_mul(7) ^ y.wrapping_mul(13)).wrapping_add(c as u32 * 61);
                px.push((v ^ ((x * y) >> 5)) as u8);
            }
        }
    }
    px
}

fn jpeg(
    width: u32,
    height: u32,
    colour: jpeg_encoder::ColorType,
    channels: usize,
    sampling: jpeg_encoder::SamplingFactor,
    progressive: bool,
    optimized: bool,
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = jpeg_encoder::Encoder::new(&mut out, 85);
    enc.set_sampling_factor(sampling);
    enc.set_progressive(progressive);
    enc.set_optimized_huffman_tables(optimized);
    enc.encode(
        &picture(width, height, channels),
        u16::try_from(width).expect("width fits"),
        u16::try_from(height).expect("height fits"),
        colour,
    )
    .expect("encode");
    out
}

const JPEG_W: u32 = 1200;
const JPEG_H: u32 = 1000;

/// Baseline interleaved, baseline one-scan-per-component (the optimized
/// encode, which the remux interleaves) and progressive, at every sampling
/// factor the encoder offers.
#[test]
fn dct_rgb_every_sampling_factor() {
    use jpeg_encoder::{ColorType as C, SamplingFactor as S};
    for (name, s) in [
        ("1x1", S::F_1_1),
        ("2x1", S::F_2_1),
        ("1x2", S::F_1_2),
        ("2x2", S::F_2_2),
        ("4x1", S::F_4_1),
        ("4x2", S::F_4_2),
        ("1x4", S::F_1_4),
        ("2x4", S::F_2_4),
    ] {
        for (mode, prog, opt) in DCT_MODES {
            let data = jpeg(JPEG_W, JPEG_H, C::Rgb, 3, s, prog, opt);
            let dict = image_dict(Some(b"DCTDecode"), JPEG_W, JPEG_H, 8, b"DeviceRGB");
            check(&format!("DCT RGB {name} {mode}"), &dict, &data);
        }
    }
}

/// `(label, progressive, optimized)`: an optimized sequential encode is
/// written one scan per component.
const DCT_MODES: [(&str, bool, bool); 3] = [
    ("baseline", false, false),
    ("non-interleaved", false, true),
    ("progressive", true, false),
];

/// Gray, CMYK and YCCK. Progressive four-component encodes from
/// `jpeg-encoder` are refused by zune ("Invalid slice"), so are not measured.
#[test]
fn dct_gray_cmyk_ycck() {
    use jpeg_encoder::{ColorType as C, SamplingFactor as S};
    for (name, colour, channels, cs) in [
        ("Gray", C::Luma, 1, &b"DeviceGray"[..]),
        ("CMYK", C::Cmyk, 4, b"DeviceCMYK"),
        ("YCCK", C::Ycck, 4, b"DeviceCMYK"),
    ] {
        for s in [S::F_1_1, S::F_2_2] {
            for (mode, prog, opt) in DCT_MODES {
                if prog && channels == 4 {
                    continue;
                }
                let data = jpeg(JPEG_W, JPEG_H, colour, channels, s, prog, opt);
                let dict = image_dict(Some(b"DCTDecode"), JPEG_W, JPEG_H, 8, cs);
                check(&format!("DCT {name} {s:?} {mode}"), &dict, &data);
            }
        }
    }
}

fn be32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

/// A single-tile J2K codestream (T.800 Annex A) whose packets are all
/// empty: every code-block is zero, which is what the decoder's buffers
/// are sized by regardless.
fn j2k(width: u32, height: u32, comps: u16, levels: u8) -> Vec<u8> {
    let mut o = vec![0xFF, 0x4F, 0xFF, 0x51];
    o.extend_from_slice(&(38 + 3 * comps).to_be_bytes());
    o.extend_from_slice(&[0, 0]);
    for v in [width, height, 0, 0, width, height, 0, 0] {
        be32(&mut o, v);
    }
    o.extend_from_slice(&comps.to_be_bytes());
    for _ in 0..comps {
        o.extend_from_slice(&[7, 1, 1]);
    }
    let mct = u8::from(comps >= 3);
    o.extend_from_slice(&[0xFF, 0x52, 0, 12, 0, 0, 0, 1, mct, levels, 4, 4, 0, 1]);
    let bands = 1 + 3 * u16::from(levels);
    o.extend_from_slice(&[0xFF, 0x5C]);
    o.extend_from_slice(&(3 + bands).to_be_bytes());
    o.push(0x40);
    for _ in 0..bands {
        o.push(10 << 3);
    }
    let packets = u32::from(comps) * (u32::from(levels) + 1);
    o.extend_from_slice(&[0xFF, 0x90, 0, 10, 0, 0]);
    be32(&mut o, 12 + 2 + packets);
    o.extend_from_slice(&[0, 1, 0xFF, 0x93]);
    o.extend(std::iter::repeat_n(0u8, packets as usize));
    o.extend_from_slice(&[0xFF, 0xD9]);
    o
}

/// A JBIG2 segment with a short-form header (T.88 §7.2): page 1, one-byte
/// referred-to numbers.
fn segment(out: &mut Vec<u8>, number: u32, kind: u8, refs: &[u8], data: &[u8]) {
    be32(out, number);
    out.push(kind);
    out.push(u8::try_from(refs.len()).expect("few refs") << 5);
    out.extend_from_slice(refs);
    out.push(1);
    be32(out, u32::try_from(data.len()).expect("fits"));
    out.extend_from_slice(data);
}

fn page_info(out: &mut Vec<u8>, width: u32, height: u32) {
    let mut d = Vec::new();
    for v in [width, height, 0, 0] {
        be32(&mut d, v);
    }
    d.extend_from_slice(&[0x01, 0, 0]);
    segment(out, 0, 48, &[], &d);
}

/// Region segment information field (§7.4.1).
fn region_info(d: &mut Vec<u8>, w: u32, h: u32, x: u32, y: u32, combop: u8) {
    for v in [w, h, x, y] {
        be32(d, v);
    }
    d.push(combop);
}

/// Page + 16-pattern 4x4 MMR dictionary + a halftone region over a
/// `grid_w` x `grid_h` grid (§7.4.4, §7.4.5), placed at (`x`, `y`).
fn jbig2_halftone(page_w: u32, page_h: u32, grid_w: u32, grid_h: u32, x: u32, y: u32) -> Vec<u8> {
    use pdfcer_image_codec::ccitt::encode_g4;
    let mut out = Vec::new();
    page_info(&mut out, page_w, page_h);
    let collective = picture(64, 4, 1)
        .iter()
        .step_by(8)
        .copied()
        .collect::<Vec<u8>>();
    let mut d = vec![0x01, 4, 4];
    be32(&mut d, 15);
    d.extend_from_slice(&encode_g4(&collective, 64, 4, true));
    segment(&mut out, 1, 16, &[], &d);
    let mut d = Vec::new();
    region_info(&mut d, grid_w * 4, grid_h * 4, x, y, 0);
    d.push(0x01);
    for v in [grid_w, grid_h, 0, 0] {
        be32(&mut d, v);
    }
    d.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]);
    let plane = picture(grid_w, grid_h, 1);
    let stride = grid_w.div_ceil(8) as usize;
    for k in 0..4 {
        let bits: Vec<u8> = plane
            .chunks(grid_w as usize)
            .flat_map(|row| {
                let mut packed = vec![0u8; stride];
                for (i, v) in row.iter().enumerate() {
                    if (v >> k) & 1 == 1 {
                        packed[i / 8] |= 0x80 >> (i % 8);
                    }
                }
                packed
            })
            .collect();
        d.extend_from_slice(&encode_g4(&bits, grid_w, grid_h, true));
    }
    segment(&mut out, 2, 22, &[1], &d);
    out
}

/// Page + an arithmetic text region with no symbols and no instances
/// (§7.4.3), placed at (`x`, `y`): the region bitmap with nothing on it.
fn jbig2_text(page_w: u32, page_h: u32, w: u32, h: u32, x: u32, y: u32) -> Vec<u8> {
    let mut out = Vec::new();
    page_info(&mut out, page_w, page_h);
    let mut d = Vec::new();
    region_info(&mut d, w, h, x, y, 0);
    d.extend_from_slice(&[0x00, 0x00]);
    be32(&mut d, 0);
    d.extend_from_slice(&[0x00; 16]);
    segment(&mut out, 1, 6, &[], &d);
    out
}

fn coded(filter: &[u8], width: u32, height: u32, bpc: i64, cs: &[u8]) -> Dict {
    image_dict(Some(filter), width, height, bpc, cs)
}

/// Single-tile codestreams; multi-tile files are not measured.
#[test]
fn jpx_one_to_four_components() {
    for (w, h, n, levels) in [
        (1000u32, 1000u32, 1u16, 5u8),
        (1000, 1000, 3, 5),
        (2000, 1000, 3, 5),
        (1000, 1000, 4, 2),
    ] {
        let cs: &[u8] = match n {
            1 => b"DeviceGray",
            3 => b"DeviceRGB",
            _ => b"DeviceCMYK",
        };
        let dict = coded(b"JPXDecode", w, h, 8, cs);
        check(
            &format!("JPX {w}x{h}x{n} L{levels}"),
            &dict,
            &j2k(w, h, n, levels),
        );
    }
}

#[test]
fn jbig2_generic_text_and_halftone_regions() {
    let (w, h) = (2000u32, 1500u32);
    let dict = coded(b"JBIG2Decode", w, h, 1, b"DeviceGray");
    let bits = picture(w.div_ceil(8), h, 1);
    let generic = pdfcer_image_codec::jbig2::encode_generic(&bits, w, h);
    check("JBIG2 generic", &dict, &generic);
    check(
        "JBIG2 text, whole page",
        &dict,
        &jbig2_text(w, h, w, h, 0, 0),
    );
    check(
        "JBIG2 text, inset",
        &dict,
        &jbig2_text(w, h, w - 8, h - 8, 4, 4),
    );
    let whole = jbig2_halftone(w, h, w / 4, h / 4, 0, 0);
    check("JBIG2 halftone, whole page", &dict, &whole);
    let inset = jbig2_halftone(w, h, w / 4 - 2, h / 4 - 2, 4, 4);
    check("JBIG2 halftone, inset", &dict, &inset);
}

#[test]
fn ccitt_group4() {
    let (w, h) = (2400u32, 1800u32);
    let bits = picture(w.div_ceil(8), h, 1);
    let data = pdfcer_image_codec::ccitt::encode_g4(&bits, w, h, true);
    let mut dict = coded(b"CCITTFaxDecode", w, h, 1, b"DeviceGray");
    let mut parms = Dict::new();
    for (k, v) in [
        (&b"K"[..], -1),
        (b"Columns", i64::from(w)),
        (b"Rows", i64::from(h)),
    ] {
        parms.insert(Name::from(k), Object::Integer(v));
    }
    parms.insert(Name::from(&b"BlackIs1"[..]), Object::Boolean(true));
    dict.insert(Name::from(&b"DecodeParms"[..]), Object::Dict(parms));
    check("CCITT G4", &dict, &data);
}

#[test]
fn uncoded_raw_flate_and_png_predictor() {
    let (w, h) = (1500u32, 1000u32);
    let raw = picture(w, h, 3);
    check("raw", &image_dict(None, w, h, 8, b"DeviceRGB"), &raw);
    let flate = coded(b"FlateDecode", w, h, 8, b"DeviceRGB");
    check("Flate", &flate, &pdfcer_model::filters::flate::encode(&raw));
    let mut rows = Vec::new();
    for row in raw.chunks(w as usize * 3) {
        rows.push(0u8);
        rows.extend_from_slice(row);
    }
    let mut dict = flate.clone();
    let mut parms = Dict::new();
    for (k, v) in [
        (&b"Predictor"[..], 12),
        (b"Colors", 3),
        (b"Columns", i64::from(w)),
    ] {
        parms.insert(Name::from(k), Object::Integer(v));
    }
    dict.insert(Name::from(&b"DecodeParms"[..]), Object::Dict(parms));
    check(
        "Flate + PNG predictor",
        &dict,
        &pdfcer_model::filters::flate::encode(&rows),
    );
    let gray = image_dict(None, w, h, 1, b"DeviceGray");
    check("raw 1-bit", &gray, &picture(w.div_ceil(8), h, 1));
}

/// The cases `predict_peak` documents as unbounded from headers.
#[test]
fn predictor_declines_what_headers_cannot_bound() {
    let doc = empty_document();
    let view = doc.view();
    let (w, h) = (64u32, 64u32);
    let mut symbols = Vec::new();
    page_info(&mut symbols, w, h);
    segment(&mut symbols, 1, 0, &[], &[0; 18]);
    let jbig2 = coded(b"JBIG2Decode", w, h, 1, b"DeviceGray");
    assert_eq!(
        predict_peak(&view, &jbig2, &symbols),
        None,
        "symbol dictionary"
    );

    let mut chain = coded(b"FlateDecode", w, h, 8, b"DeviceGray");
    let filters = [&b"FlateDecode"[..], b"DCTDecode"].map(|f| Object::Name(Name::from(f)));
    chain.insert(Name::from(&b"Filter"[..]), Object::Array(filters.to_vec()));
    assert_eq!(
        predict_peak(&view, &chain, b"x"),
        None,
        "filter before a codec"
    );

    let jxl = coded(b"JXLDecode", w, h, 8, b"DeviceGray");
    assert_eq!(predict_peak(&view, &jxl, b"\xFF\x0A"), None, "JPEG XL");

    let dct = coded(b"DCTDecode", w, h, 8, b"DeviceGray");
    assert_eq!(predict_peak(&view, &dct, b"not a jpeg"), None, "no SOI");
    let pattern = image_dict(Some(b"FlateDecode"), w, h, 8, b"Pattern");
    assert_eq!(predict_peak(&view, &pattern, b"x"), None, "pattern space");
}

#[test]
fn decode_limits_defaults() {
    use pdfcer_image_codec::peak::DecodeLimits;
    let limits = DecodeLimits::default();
    assert_eq!(limits.memory_budget, DecodeLimits::FALLBACK_MEMORY_BUDGET);
    assert_eq!(limits.max_pixels, None);
    assert_eq!(limits.downstream_bytes_per_pixel, 0);
    assert_eq!(
        DecodeLimits::for_physical_memory(8 << 30).memory_budget,
        4 << 30
    );
}
