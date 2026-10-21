#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::encode_generic;
use super::mq::MqEncoder;
use crate::bilevel::BilevelSink;

/// Decodes an embedded stream through the in-tree decoder.
fn decode(stream: &[u8]) -> (Vec<u8>, u32, u32) {
    let image = hayro_jbig2::Image::new_embedded(stream, None).unwrap();
    let (w, h) = (image.width(), image.height());
    let mut sink = BilevelSink::new(w, h);
    image.decode(&mut sink).unwrap();
    let (samples, rows) = sink.finish();
    assert_eq!(rows, h);
    (samples, w, h)
}

/// Packed rows, each padded to a byte, from a per-pixel predicate on the
/// PDF sample value.
fn image(width: u32, height: u32, mut one: impl FnMut(u32, u32) -> bool) -> Vec<u8> {
    let stride = width.div_ceil(8) as usize;
    let mut out = vec![0u8; stride * height as usize];
    for y in 0..height {
        for x in 0..width {
            if one(x, y) {
                out[y as usize * stride + (x / 8) as usize] |= 0x80 >> (x % 8);
            }
        }
    }
    out
}

/// xorshift, `per_mille` ones.
fn noise(seed: u32, per_mille: u32) -> impl FnMut(u32, u32) -> bool {
    let mut s = seed | 1;
    move |_, _| {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        s % 1000 < per_mille
    }
}

fn round_trips(px: &[u8], width: u32, height: u32) {
    let stream = encode_generic(px, width, height);
    let (back, w, h) = decode(&stream);
    assert_eq!((w, h), (width, height));
    assert_eq!(back, px, "{width}x{height}");
}

/// T.88 H.2: 32 bytes coded MSB first in one context.
#[test]
fn the_mq_coder_reproduces_the_t88_h2_test_sequence() {
    let input = [
        0x00, 0x02, 0x00, 0x51, 0x00, 0x00, 0x00, 0xC0, 0x03, 0x52, 0x87, 0x2A, 0xAA, 0xAA, 0xAA,
        0xAA, 0x82, 0xC0, 0x20, 0x00, 0xFC, 0xD7, 0x9E, 0xF6, 0xBF, 0x7F, 0xED, 0x90, 0x4F, 0x46,
        0xA3, 0xBF,
    ];
    let expected = [
        0x84, 0xC7, 0x3B, 0xFC, 0xE1, 0xA1, 0x43, 0x04, 0x02, 0x20, 0x00, 0x00, 0x41, 0x0D, 0xBB,
        0x86, 0xF4, 0x31, 0x7F, 0xFF, 0x88, 0xFF, 0x37, 0x47, 0x1A, 0xDB, 0x6A, 0xDF, 0xFF, 0xAC,
    ];
    let mut mq = MqEncoder::new(1);
    for byte in input {
        for bit in (0..8).rev() {
            mq.encode(0, (byte >> bit) & 1 == 1);
        }
    }
    assert_eq!(mq.finish(), expected);
}

/// T.88 H.1 segment 11, moved to (0, 0) on a page of its own size:
/// decoding it and encoding the result gives the same bytes.
#[test]
fn the_t88_h1_generic_region_re_encodes_byte_for_byte() {
    let data = [0x04, 0xEE, 0xED, 0x87, 0xFB, 0xCB, 0x2B, 0xFF, 0xAC];
    let mut stream = vec![0, 0, 0, 0, 0x30, 0x00, 0x01, 0, 0, 0, 0x13];
    stream.extend_from_slice(&[
        0, 0, 0, 0x36, 0, 0, 0, 0x2C, 0, 0, 0, 0, 0, 0, 0, 0, 0x01, 0, 0,
    ]);
    stream.extend_from_slice(&[0, 0, 0, 1, 0x27, 0x00, 0x01, 0, 0, 0, 0x23]);
    stream.extend_from_slice(&[0, 0, 0, 0x36, 0, 0, 0, 0x2C, 0, 0, 0, 0, 0, 0, 0, 0]);
    stream.extend_from_slice(&[0x00, 0x08, 0x03, 0xFF, 0xFD, 0xFF, 0x02, 0xFE, 0xFE, 0xFE]);
    stream.extend_from_slice(&data);
    let (px, w, h) = decode(&stream);
    assert_eq!((w, h), (54, 44));
    let black: u32 = px.iter().map(|b| b.count_zeros()).sum::<u32>() - 44 * 2;
    assert_eq!(black, 376, "H.1 says 376 black pixels");
    assert_eq!(encode_generic(&px, 54, 44), stream);
}

#[test]
fn plain_pictures_round_trip_at_awkward_widths() {
    for width in [1, 2, 7, 8, 9, 31, 33, 100] {
        for height in [1, 2, 5] {
            round_trips(&image(width, height, |_, _| false), width, height);
            round_trips(&image(width, height, |_, _| true), width, height);
            round_trips(
                &image(width, height, |x, y| (x + y) % 3 == 0),
                width,
                height,
            );
            round_trips(
                &image(width, height, noise(width * 7 + height, 500)),
                width,
                height,
            );
        }
    }
}

/// Repeated rows make typical prediction switch on and off, including
/// an all-0 first row (typical against the all-0 row above the bitmap).
#[test]
fn repeated_rows_round_trip_through_typical_prediction() {
    let mut rows = noise(9, 300);
    let fresh: Vec<Vec<bool>> = (0..6)
        .map(|_| (0..40).map(|x| rows(x, 0)).collect())
        .collect();
    let pick = [0, 0, 1, 1, 1, 2, 0, 0, 3, 4, 4, 5];
    round_trips(
        &image(40, 12, |x, y| fresh[pick[y as usize]][x as usize]),
        40,
        12,
    );
    round_trips(
        &image(40, 12, |x, y| y < 3 || fresh[pick[y as usize]][x as usize]),
        40,
        12,
    );
}

/// The template 0 context of every pixel, computed directly from
/// Figure 3 and RAG §3.5 rather than by the encoder's windows.
fn contexts(px: &[u8], width: u32, height: u32) -> Vec<u32> {
    let stride = width.div_ceil(8) as i64;
    let at = |x: i64, y: i64| -> u32 {
        if x < 0 || y < 0 || x >= i64::from(width) {
            return 0;
        }
        let byte = px[(y * stride + x / 8) as usize];
        u32::from((byte >> (7 - x % 8)) & 1 == 0)
    };
    let mut out = Vec::new();
    for y in 0..i64::from(height) {
        for x in 0..i64::from(width) {
            let mut cx = 0;
            for (dx, dy) in [(-2, -2), (-1, -2), (0, -2), (1, -2), (2, -2)]
                .into_iter()
                .chain((-3..=3).map(|dx| (dx, -1)))
                .chain((-4..=-1).map(|dx| (dx, 0)))
            {
                cx = (cx << 1) | at(x + dx, y + dy);
            }
            out.push(cx);
        }
    }
    out
}

/// A picture with pixels whose context is Figure 8's: SLTP and those
/// pixels share one adaptive bin, and a decoder that shares it (as T.88
/// requires) reads the picture back only if the encoder shares it too.
#[test]
fn pixels_in_the_sltp_context_share_its_bin() {
    let (w, h) = (64, 64);
    // Figure 8 in JBIG2 values (1 = black = PDF sample 0), anchored at
    // the pixel being coded: row -2 at x-2..=x+2, row -1 at x-3..=x+3,
    // row 0 at x-4..=x-1.
    let figure_8: [(i32, &[u8]); 3] = [
        (-2, &[1, 0, 0, 1, 1]),
        (-3, &[0, 1, 1, 0, 0, 1, 0]),
        (-4, &[0, 1, 0, 1]),
    ];
    let anchors = [(10, 10), (30, 20), (50, 40), (20, 60)];
    let mut n = noise(77, 500);
    let px = image(w, h, |x, y| {
        let planted = anchors.iter().find_map(|&(ax, ay)| {
            let dy = y as i32 - ay;
            let (start, values) = figure_8.get(usize::try_from(dy + 2).ok()?)?;
            let i = usize::try_from(x as i32 - ax - start).ok()?;
            values.get(i).map(|&v| v == 0)
        });
        planted.unwrap_or_else(|| n(x, y))
    });
    let hits = contexts(&px, w, h).iter().filter(|&&c| c == 0x9B25).count();
    assert!(hits > 0, "the fixture must reach context 0x9B25");
    round_trips(&px, w, h);
}

#[test]
fn bits_past_the_end_of_short_samples_read_as_0() {
    let stream = encode_generic(&[0xF0], 8, 3);
    let (back, _, _) = decode(&stream);
    assert_eq!(back, [0xF0, 0x00, 0x00]);
}
