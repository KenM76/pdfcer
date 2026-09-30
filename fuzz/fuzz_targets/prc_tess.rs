//! Fuzz target: the PRC schema interpreter and tessellation decoder
//! (`pdfcer_3d`, PRC 8137 WD 7.3.7), reached directly.
//!
//! `prc_parse` feeds whole files, and random bytes almost never form a
//! container whose tessellation section inflates. This target builds a valid
//! container around the input instead: byte 0 picks the authoring version
//! (either side of the 7039 and 7047 gates), bytes 1-2 split the rest into
//! the globals section (whose head is the schema) and the tessellation
//! section. Sections are stored-block zlib, so no compressor is needed.
//! Invariant: never panics, never loops.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_3d::PrcFile;

const VERSIONS: [u32; 4] = [7038, 7046, 7047, 8137];

/// RFC 1950 zlib of `data` using RFC 1951 stored blocks.
fn stored_zlib(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut chunks = data.chunks(0xffff).peekable();
    if chunks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(c) = chunks.next() {
        let len = c.len() as u16;
        out.push(u8::from(chunks.peek().is_none()));
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

fn le(out: &mut Vec<u8>, vs: &[u32]) {
    for v in vs {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

/// One file structure at authoring version `v` holding `globals` and `tess`.
fn container(v: u32, globals: &[u8], tess: &[u8]) -> Vec<u8> {
    const HEADER_LEN: usize = 107;
    let sections: [&[u8]; 5] = [globals, &[0], tess, &[], &[]];
    let mut fs = b"PRC".to_vec();
    le(&mut fs, &[8137, v, 5, 6, 7, 8, 0, 0, 0, 0, 0]);
    let mut offs = Vec::new();
    for s in sections {
        offs.push((HEADER_LEN + fs.len()) as u32);
        fs.extend(stored_zlib(s));
    }
    let mf_start = (HEADER_LEN + fs.len()) as u32;
    fs.extend(stored_zlib(&[0]));
    let mf_end = (HEADER_LEN + fs.len()) as u32;
    let mut out = b"PRC".to_vec();
    le(&mut out, &[8137, v, 1, 2, 3, 4, 0, 0, 0, 0, 1]);
    le(&mut out, &[5, 6, 7, 8, 0, 6, HEADER_LEN as u32]);
    le(&mut out, &offs);
    le(&mut out, &[mf_start, mf_end, 0]);
    out.extend(fs);
    out
}

fuzz_target!(|data: &[u8]| {
    let [sel, hi, lo, rest @ ..] = data else {
        return;
    };
    let split = (usize::from(*hi) << 8 | usize::from(*lo)).min(rest.len());
    let (globals, tess) = rest.split_at(split);
    let v = VERSIONS[usize::from(*sel) % VERSIONS.len()];
    let f = PrcFile::parse_with_limit(&container(v, globals, tess), 1 << 20)
        .expect("a built container parses");
    let _ = f.file_structures[0].tessellations();
});
