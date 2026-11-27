//! Evidence bounds (decision 227, Pass 780.2): a codestream declaring more
//! geometry than its bytes could encode is refused by name, before any
//! allocation sized from that geometry, and exactly at the bound.

use pdfcer_image_codec::{Codec, DeclaredGeometry, ImageCodecError, decode_image};
use pdfcer_model::object::{Dict, Name, Object};

#[cfg(feature = "jpx")]
use crate::peak_memory::j2k;
use crate::peak_memory::{coded, empty_document, jpeg, peak_of};

const MIB: usize = 1 << 20;

/// `jpeg` with its SOF `Y` and `X` rewritten (T.81 B.2.2: P, Y, X).
fn with_sof_dims(mut bytes: Vec<u8>, width: u16, height: u16) -> Vec<u8> {
    let sof = bytes
        .windows(2)
        .position(|w| w == [0xFF, 0xC0] || w == [0xFF, 0xC2])
        .expect("baseline or progressive SOF");
    bytes[sof + 5..sof + 7].copy_from_slice(&height.to_be_bytes());
    bytes[sof + 7..sof + 9].copy_from_slice(&width.to_be_bytes());
    bytes
}

fn decode(dict: &Dict, raw: &[u8]) -> (usize, Result<(), ImageCodecError>) {
    let doc = empty_document();
    let (peak, out) = peak_of(|| decode_image(&doc, dict, raw, false).map(|_| ()));
    (peak, out)
}

fn evidence(
    codec: Codec,
    declared: DeclaredGeometry,
    min_bytes: u64,
    have: usize,
) -> ImageCodecError {
    ImageCodecError::DeclaredGeometryExceedsData {
        codec,
        declared,
        min_bytes,
        have_bytes: have as u64,
    }
}

fn is_evidence(r: &Result<(), ImageCodecError>) -> bool {
    matches!(r, Err(ImageCodecError::DeclaredGeometryExceedsData { .. }))
}

#[test]
fn a_65535_square_dct_header_is_refused_before_a_mebibyte_is_allocated() {
    use jpeg_encoder::{ColorType as C, SamplingFactor as S};
    // Baseline: one interleaved scan of all three components, of which 4:2:0
    // chroma has the fewest blocks (4096^2), at 2 bits each. Progressive:
    // the encoder's first scan is luma's DC alone, 8192^2 blocks at 1 bit.
    for (label, progressive, bits) in [
        ("baseline", false, 4096u64 * 4096 * 2),
        ("progressive", true, 8192 * 8192),
    ] {
        let small = jpeg(8, 8, C::Rgb, 3, S::F_2_2, progressive, false);
        let raw = with_sof_dims(small, 65535, 65535);
        assert!(
            (400..=800).contains(&raw.len()),
            "{label}: {} bytes",
            raw.len()
        );
        let dict = coded(b"DCTDecode", 65535, 65535, 8, b"DeviceRGB");
        let (peak, out) = decode(&dict, &raw);
        let declared = DeclaredGeometry::Pixels {
            width: 65535,
            height: 65535,
        };
        assert_eq!(
            out,
            Err(evidence(Codec::Dct, declared, bits / 8, raw.len())),
            "{label}"
        );
        assert!(peak < MIB, "{label}: peak {peak} bytes");
        let msg = out.unwrap_err().to_string();
        assert!(msg.contains("65535 x 65535 pixels"), "{msg}");
        assert!(msg.contains(&(bits / 8).to_string()), "{msg}");
    }
}

#[test]
fn a_65535_square_ccitt_declaration_is_refused_before_a_mebibyte_is_allocated() {
    let raw = vec![0u8; 600];
    let mut dict = coded(b"CCITTFaxDecode", 65535, 65535, 1, b"DeviceGray");
    let mut parms = Dict::new();
    for (k, v) in [(&b"K"[..], -1), (b"Columns", 65535), (b"Rows", 65535)] {
        parms.insert(Name::from(k), Object::Integer(v));
    }
    dict.insert(Name::from(&b"DecodeParms"[..]), Object::Dict(parms));
    let (peak, out) = decode(&dict, &raw);
    assert_eq!(
        out,
        Err(evidence(
            Codec::Ccitt,
            DeclaredGeometry::Rows { rows: 65535 },
            8192,
            600
        ))
    );
    assert!(peak < MIB, "peak {peak} bytes");
}

/// [`j2k`] retiled to `x` x `y` pixels in `tx` x `ty` tiles (T.800 A.5.1,
/// offsets from SOC: 8 Xsiz, 12 Ysiz, 24 XTsiz, 28 YTsiz), padded to
/// `total` bytes with a COM segment after SIZ.
#[cfg(feature = "jpx")]
fn retiled_j2k(x: u32, y: u32, tx: u32, ty: u32, total: usize) -> Vec<u8> {
    let mut raw = j2k(16, 4, 1, 0);
    for (at, v) in [(8usize, x), (12, y), (24, tx), (28, ty)] {
        raw[at..at + 4].copy_from_slice(&v.to_be_bytes());
    }
    let siz_end = 4 + usize::from(u16::from_be_bytes([raw[4], raw[5]]));
    let added = total - raw.len();
    let lcom = u16::try_from(added - 2).expect("COM fits");
    let mut com = vec![0xFF, 0x64];
    com.extend_from_slice(&lcom.to_be_bytes());
    com.extend_from_slice(&[0, 1]);
    com.resize(added, b' ');
    raw.splice(siz_end..siz_end, com);
    assert_eq!(raw.len(), total);
    raw
}

#[cfg(feature = "jpx")]
#[test]
fn the_310_byte_jpx_declaring_65536_tiles_is_refused() {
    // 512 x 1024 in 4 x 2 tiles: the fuzz-found CPU-exhaustion shape.
    let raw = retiled_j2k(512, 1024, 4, 2, 310);
    let dict = coded(b"JPXDecode", 512, 1024, 8, b"DeviceGray");
    let (peak, out) = decode(&dict, &raw);
    assert_eq!(
        out,
        Err(evidence(
            Codec::Jpx,
            DeclaredGeometry::Tiles { count: 65_536 },
            65_536 * 14,
            310
        ))
    );
    assert!(peak < MIB, "peak {peak} bytes");
    assert!(out.unwrap_err().to_string().contains("65536 tiles"));
}

#[cfg(feature = "jpx")]
#[test]
fn jpx_tiles_boundary() {
    // 4097 = 17 x 241 one-pixel tiles: one past the no-evidence floor.
    let need = 4097 * 14;
    let dict = coded(b"JPXDecode", 17, 241, 8, b"DeviceGray");
    let short = retiled_j2k(17, 241, 1, 1, need - 1);
    assert_eq!(
        decode(&dict, &short).1,
        Err(evidence(
            Codec::Jpx,
            DeclaredGeometry::Tiles { count: 4097 },
            need as u64,
            need - 1
        ))
    );
    let exact = retiled_j2k(17, 241, 1, 1, need);
    assert!(!is_evidence(&decode(&dict, &exact).1));
    // At the floor no evidence is asked for: 4096 = 64 x 64 tiles.
    let floor = retiled_j2k(64, 64, 1, 1, 400);
    assert!(!is_evidence(
        &decode(&coded(b"JPXDecode", 64, 64, 8, b"DeviceGray"), &floor).1
    ));
}

/// A gray 8 x 8 JPEG re-declared `blocks` blocks wide and one block tall.
fn one_block_row(progressive: bool, blocks: u64) -> (Vec<u8>, Dict) {
    let small = jpeg(
        8,
        8,
        jpeg_encoder::ColorType::Luma,
        1,
        jpeg_encoder::SamplingFactor::F_1_1,
        progressive,
        false,
    );
    let width = u16::try_from(blocks * 8).expect("width fits");
    let dict = coded(b"DCTDecode", u32::from(width), 8, 8, b"DeviceGray");
    (with_sof_dims(small, width, 8), dict)
}

#[test]
fn dct_progressive_boundary_bits_admitted_bits_minus_one_refused() {
    // Progressive DC first scan: one bit per block, so `blocks` = bits.
    let len = one_block_row(true, 1).0.len() as u64;
    let (admit, dict) = one_block_row(true, 8 * len);
    assert!(!is_evidence(&decode(&dict, &admit).1), "bits = 8 x len");
    // One more block: bits = 8 x len + 1, so the data is bits - 1.
    let (refuse, dict) = one_block_row(true, 8 * len + 1);
    let declared = DeclaredGeometry::Pixels {
        width: u32::try_from(8 * (8 * len + 1)).unwrap(),
        height: 8,
    };
    assert_eq!(
        decode(&dict, &refuse).1,
        Err(evidence(Codec::Dct, declared, len + 1, len as usize))
    );
}

#[test]
fn dct_sequential_boundary() {
    // Two bits per block: 4 x len blocks is exactly the data's bits.
    let len = one_block_row(false, 1).0.len() as u64;
    let (admit, dict) = one_block_row(false, 4 * len);
    assert!(!is_evidence(&decode(&dict, &admit).1));
    let (refuse, dict) = one_block_row(false, 4 * len + 1);
    assert!(is_evidence(&decode(&dict, &refuse).1));
}

#[test]
fn dct_progressive_first_scan_must_be_dc() {
    let (mut raw, dict) = one_block_row(true, 1);
    let sos = raw.windows(2).position(|w| w == [0xFF, 0xDA]).unwrap();
    // SOS: FFDA, Ls, Ns=1, (Cs, Td|Ta), then Ss.
    raw[sos + 7] = 1;
    let err = decode(&dict, &raw).1.unwrap_err();
    assert!(
        matches!(&err, ImageCodecError::Corrupt { codec: Codec::Dct, detail } if detail.contains("G.1.1.1.1")),
        "{err:?}"
    );
}

#[test]
fn ccitt_rows_boundary() {
    let rows = |n: i64, len: usize| {
        let mut dict = coded(
            b"CCITTFaxDecode",
            16,
            u32::try_from(n).unwrap(),
            1,
            b"DeviceGray",
        );
        let mut parms = Dict::new();
        for (k, v) in [(&b"K"[..], -1), (b"Columns", 16), (b"Rows", n)] {
            parms.insert(Name::from(k), Object::Integer(v));
        }
        dict.insert(Name::from(&b"DecodeParms"[..]), Object::Dict(parms));
        decode(&dict, &vec![0u8; len]).1
    };
    // 10 bytes code at most 8 x 10 + 1 rows.
    assert!(!is_evidence(&rows(81, 10)));
    assert_eq!(
        rows(82, 10),
        Err(evidence(
            Codec::Ccitt,
            DeclaredGeometry::Rows { rows: 82 },
            11,
            10
        ))
    );
}

#[test]
fn every_evidence_refusal_has_a_stable_key() {
    for (codec, key) in [
        (Codec::Dct, "image/evidence/dct"),
        (Codec::Jpx, "image/evidence/jpx"),
        (Codec::Ccitt, "image/evidence/ccitt"),
    ] {
        let e = evidence(codec, DeclaredGeometry::Rows { rows: 2 }, 1, 0);
        assert_eq!(e.refusal_key(), Some(key));
    }
    assert_eq!(ImageCodecError::TooLarge.refusal_key(), None);
}
