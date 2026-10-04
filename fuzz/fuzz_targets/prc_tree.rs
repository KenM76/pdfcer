//! Fuzz target: the PRC product tree, reference coordinate systems and
//! occurrence walk (`PrcFile::placements`, `PrcFile::model_tree`, PRC 8137
//! WD 7.3.6, 7.3.10).
//!
//! As `prc_tess`, the input is wrapped in a valid container: byte 0 picks
//! the authoring version, bytes 1-2 split the rest into the globals section
//! (schema first) and the tree section, and byte 3 is the root occurrence
//! the built model file names, so the walk is reached without the fuzzer
//! having to find a matching file-structure id. Bytes 4-5 split off a
//! trailing header uncompressed file, which a texture picture can name, and
//! the whole model is then assembled (`pdfcer_3d::assemble`, texture
//! pictures decoded, PRC 8137 WD 7.5.5).
//! Invariant: never panics, never loops, never overflows the stack; every
//! tree node's placement range lies within the placements, parents first;
//! every mesh's texture index names a decoded texture.

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

/// One file structure at authoring version `v` holding `globals` and `tree`,
/// under `model`, with `file` as the header's one uncompressed file.
fn container(v: u32, globals: &[u8], tree: &[u8], model: &[u8], file: &[u8]) -> Vec<u8> {
    let header_len = 107 + 4 + file.len();
    let sections: [&[u8]; 5] = [globals, tree, &[0], &[], &[]];
    let mut fs = b"PRC".to_vec();
    le(&mut fs, &[8137, v, 5, 6, 7, 8, 0, 0, 0, 0, 0]);
    let mut offs = Vec::new();
    for s in sections {
        offs.push((header_len + fs.len()) as u32);
        fs.extend(stored_zlib(s));
    }
    let mf_start = (header_len + fs.len()) as u32;
    fs.extend(stored_zlib(model));
    let mf_end = (header_len + fs.len()) as u32;
    let mut out = b"PRC".to_vec();
    le(&mut out, &[8137, v, 1, 2, 3, 4, 0, 0, 0, 0, 1]);
    le(&mut out, &[5, 6, 7, 8, 0, 6, header_len as u32]);
    le(&mut out, &offs);
    le(&mut out, &[mf_start, mf_end, 1, file.len() as u32]);
    out.extend_from_slice(file);
    out.extend(fs);
    out
}

/// MSB-first bit sink for the model file.
#[derive(Default)]
struct Bits(Vec<bool>);

impl Bits {
    fn put(&mut self, v: u32, n: u32) -> &mut Self {
        for i in (0..n).rev() {
            self.0.push((v >> i) & 1 == 1);
        }
        self
    }

    /// A PRC UnsignedInteger.
    fn uint(&mut self, mut v: u32) -> &mut Self {
        while v != 0 {
            self.put(1, 1).put(v & 0xff, 8);
            v >>= 8;
        }
        self.put(0, 1)
    }

    fn bytes(&self) -> Vec<u8> {
        self.0
            .chunks(8)
            .map(|c| {
                c.iter()
                    .enumerate()
                    .fold(0, |a, (i, &b)| a | u8::from(b) << (7 - i))
            })
            .collect()
    }
}

/// An empty schema, then a `ModelFile` (301) whose one root is occurrence
/// `root` of the file structure [`container`] writes.
fn model_file(root: u8) -> Vec<u8> {
    let mut b = Bits::default();
    b.uint(0).uint(301).uint(0).put(1, 1).put(0, 1);
    b.put(0b01, 2); // units_in_mm = 0.0
    b.uint(1);
    for id in [5, 6, 7, 8] {
        b.uint(id);
    }
    b.uint(u32::from(root)).put(1, 1);
    b.bytes()
}

fuzz_target!(|data: &[u8]| {
    let [sel, hi, lo, root, phi, plo, rest @ ..] = data else {
        return;
    };
    let picture = (usize::from(*phi) << 8 | usize::from(*plo)).min(rest.len());
    let (rest, file) = rest.split_at(rest.len() - picture);
    let split = (usize::from(*hi) << 8 | usize::from(*lo)).min(rest.len());
    let (globals, tree) = rest.split_at(split);
    let v = VERSIONS[usize::from(*sel) % VERSIONS.len()];
    let bytes = container(v, globals, tree, &model_file(*root), file);
    let f = PrcFile::parse_with_limit(&bytes, 1 << 20).expect("a built container parses");
    let placed = f.placements().map(|p| p.len());
    if let (Ok(nodes), Ok(placed)) = (f.model_tree(), placed) {
        for (i, n) in nodes.iter().enumerate() {
            assert!(n.placements.start <= n.placements.end && n.placements.end <= placed);
            assert!(n.parent.is_none_or(|p| p < i));
        }
    }
    if let Ok(m) = pdfcer_3d::assemble(&bytes) {
        assert!(
            m.mesh_textures
                .iter()
                .flatten()
                .all(|&t| t < m.textures.len())
        );
    }
});
