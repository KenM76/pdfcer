//! Fuzz target: the glyph reads behind a PDF/A-1 `/CIDSet` or `/CharSet`
//! (`GlyphZeroResubsetter::present_glyphs`) and the CFF charset lookups
//! bounded against read-fonts' unbounded format-0 search
//! (`FontProgram::cff_cid_to_gid`, `glyph_for_builtin_code`).
//!
//! The program is an embedded `/FontFile2` or `/FontFile3` stream, so every
//! byte is the file author's. For any input, `present_glyphs` returns `Ok` or
//! a named refusal, and every GID either lookup returns is below the
//! program's glyph count or is `.notdef`: a phantom GID past it would draw
//! no glyph, or the wrong one.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::optimize::resubset::FontResubsetter;
use pdfcer_render::font::program::FontProgram;
use pdfcer_render::font::resubset::GLYPH_ZERO_RESUBSETTER;

fuzz_target!(|data: &[u8]| {
    if data.len() > 64 * 1024 {
        return;
    }
    let _ = GLYPH_ZERO_RESUBSETTER.present_glyphs(data);
    let Ok(program) = FontProgram::parse(data) else {
        return;
    };
    let count = program.num_glyphs();
    for cid in [0u16, 1, 2, 3, 100, 1000, u16::MAX] {
        let gid = program.cff_cid_to_gid(cid);
        assert!(
            gid == 0 || gid < count || !program.is_cid_cff(),
            "CID {cid} -> phantom GID {gid} of {count}"
        );
    }
    if let FontProgram::Cff(_) = program {
        for code in 0..=255u8 {
            if let Some(gid) = program.glyph_for_builtin_code(code) {
                assert!(gid < count, "code {code} -> phantom GID {gid} of {count}");
            }
        }
    }
});
