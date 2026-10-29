//! Fuzz target: glTF 2.0 / GLB import (`pdfcer_3d::read_gltf`), then the
//! PRC writer and reader over whatever it read.
//!
//! Byte 0 picks the form: even reads the rest as-is (a `.gltf` text or a
//! GLB), odd wraps it as a GLB whose bytes 1-2 split JSON from BIN, so the
//! accessor and primitive code is reached without the fuzzer finding the
//! container. Every external buffer resolves to the BIN part.
//! Invariant: never panics, never loops, never overflows the stack; a
//! scene `write_prc` accepts reads back with the same triangle count.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_3d::{GltfOptions, read_gltf, write_prc};

fn glb(json: &[u8], bin: &[u8]) -> Vec<u8> {
    let mut j = json.to_vec();
    while j.len() % 4 != 0 {
        j.push(b' ');
    }
    let total = 12 + 8 + j.len() + 8 + bin.len();
    let mut out = b"glTF".to_vec();
    for v in [2, total as u32, j.len() as u32, 0x4E4F_534A] {
        out.extend(v.to_le_bytes());
    }
    out.extend(&j);
    out.extend((bin.len() as u32).to_le_bytes());
    out.extend(0x004E_4942u32.to_le_bytes());
    out.extend(bin);
    out
}

fuzz_target!(|data: &[u8]| {
    let [sel, hi, lo, rest @ ..] = data else {
        return;
    };
    let split = (usize::from(*hi) << 8 | usize::from(*lo)).min(rest.len());
    let (json, bin) = rest.split_at(split);
    let input = if sel % 2 == 0 { rest.to_vec() } else { glb(json, bin) };
    let mut resolve = |_: &str| Some(bin.to_vec());
    let Ok(import) = read_gltf(&input, &mut resolve, GltfOptions::default()) else {
        return;
    };
    if let Ok(report) = write_prc(&import.scene) {
        let model = pdfcer_3d::assemble(&report.prc).expect("written PRC reads back");
        assert_eq!(model.triangles, report.triangles);
    }
});
