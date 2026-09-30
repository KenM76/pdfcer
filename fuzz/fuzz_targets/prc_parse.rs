//! Fuzz target: the PRC container walk and bit primitives (`pdfcer_3d`,
//! PRC 8137 WD 6.1-6.2, 11.1-11.17).
//!
//! Parses arbitrary bytes as a PRC stream under a small inflation ceiling,
//! then, both over the raw input and over every inflated section, decodes
//! `Double`s and `UnsignedInteger`s until the data runs out; then runs the
//! schema interpreter over the raw input and the tessellation decoder over
//! every file structure. `prc_tess` reaches the decoder directly.
//! Invariant: never panics, never loops; every read either consumes at least
//! one bit or errors.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_3d::bits::BitReader;
use pdfcer_3d::{PrcFile, SectionKind};

fn drain(data: &[u8]) {
    let mut r = BitReader::new(data);
    while r.remaining() > 0 {
        let before = r.position();
        let ok = if before % 2 == 0 {
            r.double().is_ok()
        } else {
            r.unsigned_integer().is_ok()
        };
        if !ok {
            break;
        }
        assert!(r.position() > before);
    }
}

fuzz_target!(|data: &[u8]| {
    drain(data);
    let _ = pdfcer_3d::Schema::read(&mut BitReader::new(data));
    if let Ok(f) = PrcFile::parse_with_limit(data, 1 << 20) {
        for fs in &f.file_structures {
            for kind in SectionKind::ALL {
                drain(fs.section(kind));
            }
            let _ = fs.tessellations();
        }
        drain(&f.model_file);
    }
});
