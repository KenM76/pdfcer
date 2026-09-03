//! Fuzz target: parse → write → parse → compare (`pdfcer_core::writer`).
//!
//! The write-direction counterpart to `load_document`, and the executable
//! form of the `ARCHITECTURE.md` §5 round-trip invariant (decision 007
//! R40). Any input that loads at all is put through **all three** save
//! paths, and each one's own contract is asserted — never a weaker
//! shared one.
//!
//! ## The three contracts, and why they must not be merged
//!
//! Decision 007 W1 names conflating these *"the single likeliest source
//! of a false green or a false red in this Pass"*:
//!
//! 1. **`save_incremental` with an empty dirty set ⇒ the output IS the
//!    input, byte for byte.** Zero edits means zero bytes — not "the
//!    input plus an empty revision". This is the strongest assertion
//!    available and the cheapest, so it runs first.
//! 2. **`save_incremental` with a dirty set ⇒ every prior byte is
//!    unchanged** (§7.5.6: *"changes shall be appended to the end of the
//!    file, leaving its original contents intact"*). Asserted as
//!    `output.starts_with(input)`, modulo the one permitted insertion:
//!    a separating EOL when the base file's final byte is not one
//!    (§7.2.3's comment-to-end-of-line rule would otherwise swallow the
//!    first appended token). That single-byte allowance is exactly why
//!    the check is written out rather than expressed as a bare prefix
//!    test.
//! 3. **`save_full` ⇒ per-OBJECT-DEFINITION byte identity**, never per
//!    file. A full rewrite moves object offsets, so the cross-reference
//!    section must differ; a whole-file assertion here would fail
//!    universally, and an assertion of mere reloadability would pass
//!    vacuously.
//!
//! ## The semantic assertion: reload and compare the object graph
//!
//! Byte-level checks cannot catch a writer that emits a *different but
//! valid* document — a truncated `/Size` that silently deletes objects
//! (§7.5.5: objects at or above it *"shall be ignored and defined to be
//! missing"*), a mis-chained `/Prev` that skips a revision, an
//! off-by-one xref offset that resolves to the neighbouring object. So
//! every produced file is loaded back and its object graph compared
//! value-for-value against the original.
//!
//! The base file's own cross-reference-stream object is excluded from
//! that comparison: it is *superseded* by the newly written section, so
//! its dictionary legitimately differs (fresh `/Prev`, delta `/Index`,
//! new `/Length`). §7.5.6's most-recent-copy rule makes that the point,
//! not a regression.
//!
//! ## What is deliberately NOT asserted
//!
//! A **refusal** is not a failure. `save_full` declines a §7.5.8.4
//! hybrid-reference file by name rather than flattening it (which would
//! destroy its pre-1.5 readability), and a classic table refuses a
//! type-2 entry it cannot express. Those are correct outcomes; the
//! target skips them rather than reporting a crash, because a fuzzer
//! that treats principled refusals as bugs trains the implementation to
//! guess.
//!
//! Nor is *reloadability* of a full rewrite asserted unconditionally
//! when the input itself was structurally exotic — see the guard on
//! `save_full` below.
//!
//! ## Contract 4: random edit/undo/redo sequences (Pass 3.1)
//!
//! The three contracts above exercise the writer over arbitrary
//! *documents*. This one exercises it over arbitrary *edit histories*,
//! which is where `ARCHITECTURE.md` §11.1's bug lives: a dirty set
//! computed as "every object any command touched" instead of "what
//! currently differs from the base" is correct for a single edit and
//! wrong the moment undo enters the sequence.
//!
//! A short script of edits, undos and redos is derived deterministically
//! from the input bytes (see [`Script`]), applied through
//! `pdfcer_core::edit::EditSession`, and then three things are asserted:
//!
//! 1. **prior bytes are intact** after saving the edited document
//!    (§7.5.6) — the mutation form of contract 2;
//! 2. **the edited document reloads**, so no edit can produce an
//!    unparseable file;
//! 3. **undoing everything and saving yields the input, byte for byte**
//!    — §11.1's contract, over an arbitrary history rather than the one
//!    a fixture happens to encode.
//!
//! Deriving the script from the input rather than taking it as a
//! separate `Arbitrary` field keeps the corpus one-file-per-input, which
//! is what makes a crash reproducible with a plain
//! `cargo fuzz run writer_roundtrip <file>`. The cost is that the script
//! and the document are correlated; that is acceptable because the
//! interesting variation here is the *history shape*, and the mixer
//! below decorrelates them well enough that neighbouring mutations of
//! one input explore different histories.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, InfoField};
use pdfcer_core::object::{ObjId, Provenance, equivalent_across_buffers};
use pdfcer_core::writer::{DirtySet, SaveOptions, save_full, save_incremental};
use pdfcer_core::xref::SectionShape;

/// Cap on the number of objects put through the identity-append path.
///
/// Re-emitting every object of a 10,000-object document is quadratic
/// against libFuzzer's time budget without adding coverage: the append
/// writer's branches are all exercised within the first handful of
/// objects. Bounding it keeps executions-per-second high, which is what
/// actually finds bugs.
const MAX_APPEND_OBJECTS: usize = 64;

/// How many operations a derived edit script may contain.
///
/// Long enough for undo and redo to interleave in every order that
/// matters (the shortest failing history for the §11.1 bug is
/// edit-undo-save, three steps), short enough that the extra saves do
/// not dominate the per-execution cost.
const MAX_SCRIPT_LEN: usize = 12;

/// One step in a derived edit history.
#[derive(Debug, Clone, Copy)]
enum Step {
    /// Turn a page by a quarter turn. The page index and the direction
    /// come from the mixer.
    Rotate {
        page: usize,
        clockwise: bool,
    },
    /// Set one metadata field to one of a few fixed strings, chosen so
    /// both the ASCII and the UTF-16BE encoding paths are reachable.
    SetInfo {
        field: InfoField,
        value: u8,
    },
    /// Remove one metadata field.
    ClearInfo {
        field: InfoField,
    },
    Undo,
    Redo,
}

/// Derive a deterministic edit script from the input bytes.
///
/// `splitmix64` over an FNV-1a-64 of the whole input: a well-mixed
/// stream where flipping one byte of the PDF produces an unrelated
/// history, which is what lets libFuzzer's mutations explore the
/// history space at all.
struct Script(u64);

impl Script {
    fn new(data: &[u8]) -> Self {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in data {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Self(h)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn steps(&mut self, page_count: usize) -> Vec<Step> {
        let len = (self.next() as usize % MAX_SCRIPT_LEN) + 1;
        let fields = InfoField::all();
        (0..len)
            .map(|_| {
                let r = self.next();
                let field = fields[(r >> 8) as usize % fields.len()];
                match r % 5 {
                    0 if page_count > 0 => Step::Rotate {
                        page: (r >> 16) as usize % page_count,
                        clockwise: r & 0x1_0000_0000 != 0,
                    },
                    1 => Step::ClearInfo { field },
                    2 => Step::Undo,
                    3 => Step::Redo,
                    _ => Step::SetInfo {
                        field,
                        value: (r >> 24) as u8,
                    },
                }
            })
            .collect()
    }
}

/// The strings a `SetInfo` step may write.
///
/// Deliberately covers both §7.9.2 encodings — pure ASCII takes the
/// direct byte path, the rest take UTF-16BE with a BOM — plus the empty
/// string, which is the value most likely to be confused with "absent"
/// somewhere in the stack.
fn probe_value(selector: u8) -> &'static str {
    match selector % 4 {
        0 => "",
        1 => "probe",
        2 => "Café — 日本語",
        _ => "a much longer value, with (parens) and \\backslashes and a ) in it",
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = Document::from_bytes(data.to_vec()) else {
        // Not a loadable PDF — `load_document` already fuzzes that path.
        return;
    };
    let opts = SaveOptions::identity();

    // --- contract 1: zero edits, zero bytes --------------------------
    match save_incremental(&doc, &DirtySet::empty(), &opts) {
        Ok((out, report)) => {
            assert_eq!(
                out, data,
                "empty-dirty-set incremental save was not byte-identical"
            );
            assert!(report.byte_identical);
            assert_eq!(report.bytes_appended, 0);
        }
        Err(e) => panic!("an empty-dirty-set save must never fail: {e}"),
    }

    // --- contract 2: an append leaves prior bytes intact --------------
    let ids: Vec<ObjId> = doc
        .objects()
        .map(|io| io.id)
        .take(MAX_APPEND_OBJECTS)
        .collect();
    if !ids.is_empty()
        && let Ok((out, report)) =
            save_incremental(&doc, &DirtySet::identity_reemission(ids), &opts)
    {
        // The one permitted insertion: a separating EOL when the base
        // file's last byte is not one (§7.2.3).
        let prefix_ok = out.starts_with(data)
            || (out.len() > data.len()
                && out.get(..data.len()) == Some(data)
                && matches!(out.get(data.len()), Some(b'\n')));
        assert!(
            prefix_ok,
            "an incremental append modified bytes below the original EOF"
        );
        assert!(report.bytes_appended > 0);
        assert_reloads_to_the_same_graph(&doc, &out, "append");
    }

    // --- contract 3: per-object-definition identity -------------------
    if let Ok((out, _)) = save_full(&doc, &DirtySet::empty(), &opts) {
        let back = assert_reloads_to_the_same_graph(&doc, &out, "full rewrite");
        for io in doc.objects() {
            let Provenance::File(span) = io.provenance else {
                // A compressed object has no file-level bytes; its
                // container carries them and is checked in its own turn.
                continue;
            };
            if is_section_object(&doc, io.id) {
                continue;
            }
            let Some(want) = span.slice(doc.bytes()) else {
                continue;
            };
            // Compare the span the RELOADED document resolved for this
            // id, rather than searching the whole output for the bytes.
            // Linear instead of quadratic, and a strictly stronger
            // claim: it proves the bytes are reachable *through the new
            // cross-reference table*, not merely present somewhere.
            let got = back
                .get(io.id)
                .and_then(|o| o.file_span())
                .and_then(|s| s.slice(back.bytes()));
            assert!(
                got == Some(want),
                "object {} lost its verbatim definition bytes in a full rewrite",
                io.id
            );
        }
    }

    // --- contract 4: an arbitrary edit history, then undo -------------
    exercise_edit_history(data);
});

/// Apply a derived edit/undo/redo script and assert the three mutation
/// contracts (module docs).
///
/// The document is re-parsed rather than moved in, because
/// `EditSession::new` consumes it and the caller's checks above still
/// need theirs. A second parse of a buffer that already loaded cannot
/// fail, so the `else` arm is a return rather than an assertion.
fn exercise_edit_history(data: &[u8]) {
    let Ok(doc) = Document::from_bytes(data.to_vec()) else {
        return;
    };
    let mut session = EditSession::new(doc);
    // A page tree that will not walk yields zero pages, which simply
    // means the script gets no rotation steps — the metadata steps
    // still exercise the writer, so such a file is not skipped.
    let page_count = session.pages().map_or(0, |pages| pages.len());
    let mut script = Script::new(data);

    let mut applied_anything = false;
    for step in script.steps(page_count) {
        // Every refusal here is a NAMED one — a rotation that is not a
        // multiple of 90 cannot arise from these steps, so what remains
        // is a malformed page or `/Info` object, which the engine is
        // supposed to decline. Declining is correct behaviour and must
        // not be asserted against; a fuzzer that treats principled
        // refusals as bugs trains the implementation to guess.
        match step {
            Step::Rotate { page, clockwise } => {
                let delta = if clockwise { 90 } else { -90 };
                applied_anything |= session.rotate_page_by(page, delta).is_ok();
            }
            Step::SetInfo { field, value } => {
                applied_anything |= session
                    .set_info_field(field, Some(probe_value(value)))
                    .is_ok();
            }
            Step::ClearInfo { field } => {
                applied_anything |= session.set_info_field(field, None).is_ok();
            }
            Step::Undo => {
                session.undo();
            }
            Step::Redo => {
                session.redo();
            }
        }
    }
    if !applied_anything {
        return;
    }

    let opts = SaveOptions::identity();
    let modified = session.is_modified();
    if let Ok((out, report)) = session.to_incremental_bytes(&opts) {
        // The state and the bytes must agree about whether anything
        // changed. A report claiming byte identity for a modified
        // document — or an appended revision on an unmodified one —
        // is the two halves of the writer disagreeing, which is worse
        // than either being wrong alone.
        assert_eq!(
            report.byte_identical, !modified,
            "the save report disagrees with the session about whether anything changed"
        );
        if modified {
            let prefix_ok = out.starts_with(data)
                || (out.len() > data.len()
                    && out.get(..data.len()) == Some(data)
                    && matches!(out.get(data.len()), Some(b'\n')));
            assert!(
                prefix_ok,
                "an edited save modified bytes below the original EOF"
            );
            if let Err(e) = Document::from_bytes(out) {
                panic!("pdfcer could not reload a file it produced from an edit: {e}");
            }
        } else {
            assert_eq!(out, data, "an unmodified session must save the input");
        }
    }

    // THE contract: undo everything, and the save is the input again.
    while session.undo().is_some() {}
    assert!(
        !session.is_modified(),
        "undoing every command left the session modified"
    );
    match session.to_incremental_bytes(&opts) {
        Ok((undone, _)) => assert_eq!(
            undone, data,
            "edit -> undo -> save was not byte-identical (ARCHITECTURE.md §11.1)"
        ),
        Err(e) => panic!("the post-undo save must never fail: {e}"),
    }
}

/// Load `out` and assert it carries the same object graph as `doc`.
///
/// A reload failure is a hard error: pdfcer must be able to parse what
/// pdfcer just wrote. That is a stronger claim than "some reader can",
/// and deliberately so — pdfcer's own loader is the strictest one
/// available here, so it is the right oracle.
///
/// Returns the reloaded document so the caller can compare byte spans
/// against it without paying for a second load.
fn assert_reloads_to_the_same_graph(doc: &Document, out: &[u8], what: &str) -> Document {
    let back = match Document::from_bytes(out.to_vec()) {
        Ok(back) => back,
        Err(e) => panic!("pdfcer could not reload its own {what} output: {e}"),
    };
    for io in doc.objects() {
        if is_section_object(doc, io.id) {
            continue;
        }
        // Cross-buffer comparison: `Stream` stores a span, so the
        // derived `PartialEq` would flag every relocated stream.
        let same = back.get(io.id).is_some_and(|b| {
            equivalent_across_buffers(&b.value, back.bytes(), &io.value, doc.bytes())
        });
        assert!(same, "object {} changed across a {what}", io.id);
    }
    back
}

/// Whether `id` is the object that *is* the base file's newest
/// cross-reference section (§7.5.8.1) rather than document content.
fn is_section_object(doc: &Document, id: ObjId) -> bool {
    matches!(doc.section_shape(), SectionShape::Stream { id: sid, .. } if sid == id)
}
