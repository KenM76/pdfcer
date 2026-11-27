//! The cooperative cancel poll inside the filter chain.

use std::cell::Cell;

use super::*;
use crate::object::Name;

fn dict(filter: &[u8], parms: Option<Dict>) -> Dict {
    let mut d = Dict::new();
    d.insert(Name::from(b"Filter"), Object::Name(Name::from(filter)));
    if let Some(p) = parms {
        d.insert(Name::from(b"DecodeParms"), Object::Dict(p));
    }
    d
}

fn zlib(data: &[u8]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::Write as _;
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::fast());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

/// A poll that answers `true` from its `fire_at`th call on, counting calls.
fn counting(calls: &Cell<u32>, fire_at: u32) -> impl Fn() -> bool + '_ {
    move || {
        calls.set(calls.get() + 1);
        calls.get() >= fire_at
    }
}

#[test]
fn a_flate_decode_stops_at_the_next_chunk_once_cancelled() {
    // 4 MiB inflates in 64 chunks; cancelling at the third poll must stop
    // the decode there rather than at the end.
    let raw = zlib(&vec![0x55; 4 << 20]);
    let d = dict(b"FlateDecode", None);
    let calls = Cell::new(0);
    let poll = counting(&calls, 3);
    assert_eq!(
        decode_stream_cancellable(&d, &raw, &poll),
        Err(FilterError::Cancelled)
    );
    assert_eq!(calls.get(), 3, "no poll after the one that fired");
}

#[test]
fn an_uncancelled_decode_is_byte_identical_and_polls_per_chunk() {
    let plain: Vec<u8> = (0..(1u32 << 20)).map(|i| (i * 7 % 251) as u8).collect();
    let raw = zlib(&plain);
    let d = dict(b"FlateDecode", None);
    let calls = Cell::new(0);
    let poll = counting(&calls, u32::MAX);
    let (bytes, _) = decode_stream_cancellable(&d, &raw, &poll).unwrap();
    assert_eq!(bytes, decode_stream(&d, &raw).unwrap());
    assert_eq!(bytes, plain);
    // One stage poll plus one per 64 KiB pull (16 full chunks and the end).
    assert!(calls.get() >= 17, "polled {} times", calls.get());
}

#[test]
fn a_png_predictor_polls_every_row() {
    // 256 rows of 1024 bytes, PNG `Up` (tag 2) on every row.
    let columns = 1024;
    let rows = 256;
    let mut predicted = Vec::new();
    for _ in 0..rows {
        predicted.push(2u8);
        predicted.extend(std::iter::repeat_n(1u8, columns));
    }
    let raw = zlib(&predicted);
    let mut parms = Dict::new();
    parms.insert(Name::from(b"Predictor"), Object::Integer(12));
    parms.insert(Name::from(b"Columns"), Object::Integer(columns as i64));
    let d = dict(b"FlateDecode", Some(parms));

    let calls = Cell::new(0);
    let poll = counting(&calls, u32::MAX);
    let (bytes, _) = decode_stream_cancellable(&d, &raw, &poll).unwrap();
    assert_eq!(bytes.len(), columns * rows);
    let total = calls.get();
    assert!(total > rows as u32, "row polls missing: {total}");

    // Firing on the last few polls lands inside the predictor stage, after
    // every inflate chunk has been pulled.
    let calls = Cell::new(0);
    let poll = counting(&calls, total - 5);
    assert_eq!(
        decode_stream_cancellable(&d, &raw, &poll),
        Err(FilterError::Cancelled)
    );
    assert_eq!(calls.get(), total - 5);
}

#[test]
fn a_run_length_decode_stops_once_cancelled() {
    // 8192 runs of 128 copies: 1 MiB of output, 16 chunks.
    let raw: Vec<u8> = std::iter::repeat_n([129u8, 0x42], 8192)
        .flatten()
        .chain([128])
        .collect();
    let d = dict(b"RunLengthDecode", None);
    let calls = Cell::new(0);
    let poll = counting(&calls, 2);
    assert_eq!(
        decode_stream_cancellable(&d, &raw, &poll),
        Err(FilterError::Cancelled)
    );
    assert_eq!(calls.get(), 2);
}

#[test]
fn a_cancel_before_the_chain_starts_reads_nothing() {
    // Bytes that would fail to inflate: the poll answers first.
    let d = dict(b"FlateDecode", None);
    assert_eq!(
        decode_stream_cancellable(&d, b"not zlib", &|| true),
        Err(FilterError::Cancelled)
    );
}
