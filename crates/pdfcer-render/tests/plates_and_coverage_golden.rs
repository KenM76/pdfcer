//! `render_plates` and `coverage_warning_masks` give the same pixels as they
//! did before the ink arithmetic moved to `pdfcer-ink`: a digest per fixture
//! directory, captured at the pin the move started from.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use pdfcer_core::document::Document;
use pdfcer_render::plates::{Plates, render_plates};
use pdfcer_render::{CoverageWarning, RenderOptions, coverage_warning_masks};

const DIRS: [&str; 7] = [
    "overprint",
    "transparency",
    "ink-probe",
    "devicen-image",
    "cmyk-variants",
    "mesh-ink",
    "lab-ink",
];

const RULES: [CoverageWarning; 4] = [
    CoverageWarning::TotalAreaCoverage { limit_percent: 250 },
    CoverageWarning::TotalAreaCoverage { limit_percent: 100 },
    CoverageWarning::RichBlack {
        black_start_percent: 95,
    },
    CoverageWarning::RichBlack {
        black_start_percent: 0,
    },
];

/// FNV-1a, 64-bit.
struct Fnv(u64);

impl Fnv {
    fn eat(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

fn digest(dir: &str) -> u64 {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic");
    let mut files: Vec<_> = std::fs::read_dir(root.join(dir))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "pdf"))
        .collect();
    files.sort();
    let mut h = Fnv(0xcbf2_9ce4_8422_2325);
    let opts = RenderOptions::default();
    for file in files {
        let Ok(doc) = Document::from_bytes(std::fs::read(&file).unwrap()) else {
            continue;
        };
        let Ok(pages) = pdfcer_core::page_tree::pages(&doc) else {
            continue;
        };
        let view = doc.view();
        for page in &pages {
            h.eat(file.file_name().unwrap().as_encoded_bytes());
            match render_plates(&view, page, 0.5, &opts) {
                Ok(Plates::Separated(plates)) => {
                    for p in plates {
                        h.eat(p.ink.as_bytes());
                        h.eat(p.pixmap.data());
                    }
                }
                Ok(Plates::Composite(px)) => h.eat(px.data()),
                Ok(_) => h.eat(b"other"),
                Err(e) => h.eat(e.to_string().as_bytes()),
            }
            match coverage_warning_masks(&view, page, 0.5, None, &opts, &RULES) {
                Ok(masks) => {
                    for m in masks {
                        h.eat(&m.flagged.to_le_bytes());
                        h.eat(&m.mask);
                    }
                }
                Err(e) => h.eat(e.to_string().as_bytes()),
            }
        }
    }
    h.0
}

/// The digests at `117bcb59`, before the move.
const GOLDEN: [(&str, u64); 7] = [
    ("overprint", 0x3e55_c26c_dff5_584c),
    ("transparency", 0xb953_a514_979b_6101),
    ("ink-probe", 0x3363_0be5_fbb5_dda3),
    ("devicen-image", 0x62f2_5921_4da3_9848),
    ("cmyk-variants", 0xc2c2_2de0_ccc0_7285),
    ("mesh-ink", 0x88ea_4eee_e6cc_b826),
    ("lab-ink", 0x0395_cd90_5745_2c7a),
];

#[test]
fn plates_and_coverage_masks_match_the_pin() {
    let mut wrong = Vec::new();
    for (dir, want) in GOLDEN {
        assert!(DIRS.contains(&dir));
        let got = digest(dir);
        if got != want {
            wrong.push(format!("(\"{dir}\", 0x{got:016x}),"));
        }
    }
    assert!(wrong.is_empty(), "digests differ:\n{}", wrong.join("\n"));
}
