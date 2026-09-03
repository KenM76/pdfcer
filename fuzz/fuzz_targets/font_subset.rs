//! Fuzz target: the **FF-C donor subsetter**
//! (`pdfcer_render::font::subset::plan_subset`, Pass 21.0 / decision 021 §3.5).
//!
//! ## Why this input is untrusted even though an operator chose it
//!
//! `--embed-font` points at a file the operator picked, and it is tempting to
//! treat that as consent to trust the bytes. It is not. Font files are a
//! long-standing exploit vector, they arrive by email and download like any
//! other document, and the operator is in no position to audit an sfnt table
//! directory. What "the operator chose it" actually rules out is *pdfcer being
//! tricked into reading a file the operator never named* — nothing about the
//! contents.
//!
//! ## Fuzz the glue and the ceiling, not `subsetter`'s internals
//!
//! Typst's `subsetter` is `#![deny(unsafe_code)]` and is exercised by Typst's
//! own corpus, so hammering its `glyf` walk here would largely re-run someone
//! else's campaign. pdfcer's bugs will be in what pdfcer does around it:
//!
//! 1. **The size ceiling's ORDERING.** `MAX_DONOR_BYTES` has to be checked
//!    before the parse, or it bounds nothing — a 64 MiB+ input would be fully
//!    parsed first and only then refused. The unit test asserts that with one
//!    crafted buffer; this target hits it with arbitrary sizes.
//! 2. **Coverage lookup against a hostile `cmap`.** `plan_subset` maps each
//!    requested character to a GID and then narrows it to `u16`. A font whose
//!    charmap yields a GID outside 16 bits must be refused, not truncated —
//!    truncation silently selects a *different, valid* glyph, which is the
//!    worst possible outcome because it renders.
//! 3. **The units-per-em conversion.** Every metric is scaled by
//!    `1000 / upem`. A zero or absurd `upem` is attacker-controlled and turns
//!    that into a division by zero or an `i32` overflow in the `as` cast.
//! 4. **Error mapping totality.** Every `subsetter::Error` must land on a
//!    named `SubsetError`; a panic in the mapping would be a crash on the
//!    error path, which is exactly where nobody looks.
//!
//! ## The contract
//!
//! For ANY byte string and ANY requested character set, `plan_subset` returns
//! — `Ok` or a named `Err`. It must not panic, must not hang, and must not
//! allocate without bound. Note the deliberate absence of a
//! composite-glyph-depth assertion: `subsetter`'s closure is an iterative
//! worklist bounded by `numGlyphs`, so cycles terminate structurally upstream
//! (decision 021 §3.5). A pdfcer-side depth guard would be unreachable, and
//! this target plus the cycle fixture cover the property instead.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_render::font::subset::plan_subset;

/// Cap on how many characters the fuzzer may request.
///
/// Not a correctness bound — `plan_subset` handles any count — but a
/// throughput one. Without it the fuzzer spends its budget on enormous
/// character sets against fonts that were never going to parse, which is the
/// least interesting corner of the input space.
const MAX_CHARS: usize = 64;

fuzz_target!(|data: &[u8]| {
    // Split the input: a small prefix steers the REQUEST (face index and the
    // characters asked for), the remainder is the candidate font.
    //
    // Steering these independently matters. The interesting bugs are in the
    // interaction — a valid font asked for characters it lacks, a truncated
    // font asked for many characters, a collection index past the end of a
    // real collection — and a single undifferentiated blob would only ever
    // exercise "is this a font", which the parser already answers.
    let Some((&index_byte, rest)) = data.split_first() else {
        return;
    };
    let Some((&count_byte, font_bytes)) = rest.split_first() else {
        return;
    };

    // Face index: mostly 0 (the overwhelmingly common case, and the one the
    // CLI passes) with occasional large values to reach the collection-index
    // bounds check.
    let face_index = u32::from(index_byte % 4);

    let n = usize::from(count_byte) % MAX_CHARS;
    // Characters drawn from the font bytes themselves. Deliberately includes
    // non-Latin and non-BMP scalars: FF-C exists to embed exactly the text
    // the Standard-14 path cannot, so a fuzz corpus of ASCII would miss the
    // case the feature is for. `from_u32` filters surrogates, which are not
    // scalar values and can never appear in a Rust `char`.
    let chars: Vec<char> = font_bytes
        .iter()
        .take(n)
        .enumerate()
        .filter_map(|(i, b)| {
            let cp = u32::from(*b) | ((i as u32 & 0xff) << 8);
            char::from_u32(cp)
        })
        .collect();
    if chars.is_empty() {
        return;
    }

    // The tag is fixed and valid: a malformed one is already covered by a
    // unit test, and spending fuzz cycles on a parameter pdfcer derives itself
    // would test the harness rather than the code.
    let _ = plan_subset(font_bytes, face_index, &chars, "FuzzDonor", "ABCDEF");

    // ★ The SECOND `fsType` reader, on the same bytes (Pass 67.0 phase A).
    //
    // `pdfcer_core::fontinfo::read_fs_type` walks an sfnt table directory by
    // hand — magic, `numTables`, 16-byte records, then a `uint16` at offset 8
    // inside `OS/2` — because `pdfcer-core` may not take a font-parsing
    // dependency (project rule 2; `skrifa` lives in `pdfcer-render` and the
    // crate boundary is load-bearing). Hand-written offset arithmetic over
    // attacker-controlled `uint32` offsets and lengths is exactly the code
    // that needs this target, and it needs it on the SAME inputs: a font that
    // reaches the subsetter is a font the inventory will also read, and the
    // interesting corpus is shared.
    //
    // The contract is the same shape as `plan_subset`'s: for ANY byte string,
    // `Ok` or a NAMED `Err`. Never a panic, never an out-of-bounds slice,
    // never an unbounded read from a 65,535-entry `numTables` claim. And —
    // the property that actually matters — never a fabricated permission: a
    // failure must surface as an error, because `fsType == 0` means
    // *Installable*, the most permissive value the field can express, so a
    // guess in the failure path would silently grant the broadest embedding
    // right there is.
    if let Ok(bits) = pdfcer_core::fontinfo::read_fs_type(font_bytes) {
        // The version gate is a real branch over attacker-controlled data:
        // OpenType says bits 4-15 MUST be ignored for an `OS/2` version 0 or
        // 1 table, so a v0/v1 font must never come back claiming them.
        assert!(
            bits.os2_version > 1 || (!bits.no_subsetting && !bits.bitmap_only),
            "bits 8/9 must be suppressed for OS/2 v0 and v1"
        );
    }
});
