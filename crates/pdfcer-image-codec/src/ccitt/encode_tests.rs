#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::encode_g4;
use crate::bilevel::BilevelSink;
use hayro_ccitt::{DecodeSettings, DecoderContext, EncodingMode};

/// Decodes through the in-tree decoder with `/K -1`, `/Columns`, `/Rows`
/// and `/BlackIs1 black_is_1`, every other Table 11 key at its default.
fn decode(data: &[u8], width: u32, height: u32, black_is_1: bool) -> Vec<u8> {
    let mut sink = BilevelSink::new(width, height);
    let mut ctx = DecoderContext::new(DecodeSettings {
        columns: width,
        rows: height,
        end_of_block: true,
        end_of_line: false,
        rows_are_byte_aligned: false,
        encoding: EncodingMode::Group4,
        invert_black: black_is_1,
    });
    hayro_ccitt::decode(data, &mut sink, &mut ctx).unwrap();
    let (samples, rows) = sink.finish();
    assert_eq!(rows, height);
    samples
}

/// Packed rows, each padded to a byte, from a per-pel predicate.
fn image(width: u32, height: u32, mut pel: impl FnMut(u32, u32) -> bool) -> Vec<u8> {
    let stride = width.div_ceil(8) as usize;
    let mut out = vec![0u8; stride * height as usize];
    for y in 0..height {
        for x in 0..width {
            if pel(x, y) {
                out[y as usize * stride + (x / 8) as usize] |= 0x80 >> (x % 8);
            }
        }
    }
    out
}

/// A deterministic pseudo-random pel stream (xorshift), `per_mille` ones.
fn noise(width: u32, height: u32, per_mille: u32, seed: u32) -> Vec<u8> {
    let mut s = seed;
    image(width, height, |_, _| {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        s % 1000 < per_mille
    })
}

fn round_trips(samples: &[u8], width: u32, height: u32) {
    for one_is_black in [true, false] {
        let g4 = encode_g4(samples, width, height, one_is_black);
        assert_eq!(
            decode(&g4, width, height, one_is_black),
            samples,
            "{width}x{height}, one_is_black={one_is_black}"
        );
    }
}

/// The cases the spec RAG's libtiff verification ran (§8), against the
/// decoder pdfcer actually ships.
#[test]
fn the_spec_verification_cases_round_trip_through_the_shipped_decoder() {
    round_trips(&noise(37, 20, 500, 1), 37, 20);
    round_trips(&noise(200, 60, 50, 2), 200, 60);
    round_trips(&image(64, 30, |x, y| (x / 5 + y / 3) % 2 == 0), 64, 30);
    round_trips(&image(31, 9, |_, _| true), 31, 9);
    round_trips(&image(31, 9, |_, _| false), 31, 9);
    round_trips(&image(1, 7, |_, y| y.is_multiple_of(2)), 1, 7);
}

/// Runs past 2560 use repeated M(2560), and 1792-2496 the shared
/// extended make-up codes, in both colours.
#[test]
fn long_runs_use_the_extended_make_up_codes() {
    let bands = |x: u32, y: u32| match y {
        0 => x < 2600,
        1 => x >= 1800,
        2 => (x / 2560) % 2 == 1,
        _ => x % 64 == 63,
    };
    round_trips(&image(5000, 4, bands), 5000, 4);
    round_trips(
        &image(5200, 3, |x, y| (x + y * 1793) % 5199 < 2600),
        5200,
        3,
    );
}

/// Every vertical offset and pass mode: a one-pel-per-row diagonal
/// shift by each of 0..=4 pels, then a gap that triggers pass mode.
#[test]
fn every_mode_round_trips() {
    for shift in 0..=4 {
        let px = |x: u32, y: u32| (x + 40 - y * shift) % 40 < 7;
        round_trips(&image(120, 9, px), 120, 9);
    }
    let pass = |x: u32, y: u32| {
        if y.is_multiple_of(2) {
            x % 10 < 3
        } else {
            x > 100
        }
    };
    round_trips(&image(120, 8, pass), 120, 8);
}

/// Widths that are not a multiple of 8 leave pad bits in each row; they
/// are not coded, and the decoder writes them as 0.
#[test]
fn row_pad_bits_are_not_coded() {
    let mut samples = image(13, 5, |x, y| (x * y) % 3 == 0);
    round_trips(&samples, 13, 5);
    for row in samples.chunks_mut(2) {
        row[1] |= 0x07;
    }
    let g4 = encode_g4(&samples, 13, 5, true);
    let clean = image(13, 5, |x, y| (x * y) % 3 == 0);
    assert_eq!(decode(&g4, 13, 5, true), clean);
}

/// The polarity choice changes size, not content, and which choice is
/// smaller depends on the picture: here short ink runs are cheaper coded
/// as white, the opposite of what a "white background" rule would pick.
#[test]
fn polarity_changes_size_not_content() {
    let text = image(800, 60, |x, y| y % 12 < 2 && x % 9 < 4);
    let one_black = encode_g4(&text, 800, 60, true);
    let one_white = encode_g4(&text, 800, 60, false);
    assert!(
        one_white.len() < one_black.len(),
        "{} vs {}",
        one_white.len(),
        one_black.len()
    );
    round_trips(&text, 800, 60);
}

#[test]
fn an_empty_image_is_eofb_alone() {
    assert_eq!(encode_g4(&[], 8, 0, true), [0x00, 0x10, 0x01]);
}

/// libtiff coded the checked-in fixture; pdfcer's encoder must produce
/// the same bits from the same picture. `/BlackIs1` is omitted there, so
/// a 1 sample is CCITT white.
#[test]
fn the_libtiff_fixture_is_reproduced_bit_for_bit() {
    use crate::fixtures_bilevel::{BILEVEL_16X4_SAMPLES, CCITT_G4_16X4};
    let g4 = encode_g4(BILEVEL_16X4_SAMPLES, 16, 4, false);
    assert_eq!(g4, CCITT_G4_16X4);
}
