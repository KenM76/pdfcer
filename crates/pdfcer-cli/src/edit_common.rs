use super::*;

/// Implement `pdfcer round-trip`: save, then verify the
/// `ARCHITECTURE.md` §5 invariant that the chosen mode promises.
///
/// ## The three checks, and why they are three
///
/// Decision 007 W1/R32 names conflating these *"the single likeliest
/// source of a false green or a false red"*. Each maps to its own exit
/// code so a corpus sweep can tally them apart:
///
/// 1. **Byte identity** — whole-file for `--mode incremental` (which
///    promises it), *prefix* identity for `--mode append-identity`
///    (§7.5.6: prior bytes are left intact), and **per object
///    definition** for `--mode full`, where offsets legitimately move
///    and a whole-file comparison would fail universally.
/// 2. **Reload** — `pdfcer-core` must be able to parse what it just
///    wrote. A writer that emits an unloadable file is worse than one
///    that emits a differing file.
/// 3. **Raster** — page 1 must re-render to identical pixels. This is
///    the *semantic* oracle: byte identity is a syntactic claim, and a
///    file can satisfy it while meaning something different. It is a
///    **self**-comparison (pdfcer-before vs pdfcer-after), which needs no
///    reference renderer — deliberately NOT the outstanding
///    pdfcer-vs-pdfium pixel-parity harness, which remains owed.
///
/// A refused save ([`exit::SAVE_REFUSED`]) is a fourth, separate
/// outcome: pdfcer declining a hybrid full rewrite by name is correct
/// behaviour, and a corpus run that counted it as a failure would be
/// reporting a lie.
pub(crate) fn cmd_round_trip(
    input: &Path,
    mode: RoundTripMode,
    output: Option<&Path>,
    producer: ProducerArg,
    scale: f32,
    compare_raster: bool,
) -> u8 {
    use pdfcer_core::document::Document;
    use pdfcer_core::writer::{DirtySet, ProducerPolicy, SaveOptions};

    let source = match std::fs::read(input) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::IO_ERROR;
        }
    };
    let doc = match open_document_bytes(source.clone()) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    // Decision 013: if this file opened only via cross-reference recovery,
    // disclose it (R20). A recovered document also refuses incremental save
    // by name (`RoundTripMode::Incremental`/`AppendIdentity` → SAVE_REFUSED)
    // and its `save_full` emits a fresh valid classic xref — the demo path.
    if let Some(report) = doc.recovery() {
        disclose_recovery(input, report);
    }

    let options = SaveOptions::default().with_producer(match producer {
        ProducerArg::Set => ProducerPolicy::Set,
        ProducerArg::Preserve => ProducerPolicy::Preserve,
    });

    // The whole `round-trip` subcommand is an INSTRUMENT, not an editing
    // feature. Every arm below passes `DirtySet::empty()` or
    // `identity_reemission` — it mutates nothing, and exists to prove the
    // writer reproduces a file byte-for-byte (the content-identity harness).
    // Routing it through `EditSession` would mean opening a session to make no
    // edit, and would measure the session rather than the writer.
    //
    // bypass-exempt: identity re-emission only, mutates nothing (see above)
    let saved = match mode {
        RoundTripMode::Incremental => {
            pdfcer_core::writer::save_incremental(&doc, &DirtySet::empty(), &options)
        }
        RoundTripMode::Full => pdfcer_core::writer::save_full(&doc, &DirtySet::empty(), &options),
        RoundTripMode::AppendIdentity => {
            // Every object of the base revision, re-emitted unchanged.
            let ids: Vec<_> = doc.objects().map(|io| io.id).collect();
            // bypass-exempt: round-trip instrument — the `DirtySet` below is
            // `identity_reemission`, which re-emits every base object
            // unchanged and mutates nothing. This proves the writer reproduces
            // a file byte-for-byte; it is an instrument, not an edit.
            pdfcer_core::writer::save_incremental(
                // bypass-exempt: round-trip instrument, identity re-emission
                &doc,
                &DirtySet::identity_reemission(ids),
                &options,
            )
        }
    };
    let (bytes, report) = match saved {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("pdfcer: {}: save refused: {err}", input.display());
            hint_recovered_base(&err);
            return exit::SAVE_REFUSED;
        }
    };

    if let Some(path) = output
        && let Err(err) = std::fs::write(path, &bytes)
    {
        eprintln!("pdfcer: {}: {err}", path.display());
        return exit::IO_ERROR;
    }

    // Check 2 is evaluated BEFORE check 1, because a full rewrite's
    // byte-identity test needs the RELOADED document — see
    // `full_rewrite_is_per_object_verbatim` for why comparing through
    // the reload is both linear and a strictly stronger claim.
    //
    // --- check 2: reload, and the object graph survived ---------------
    //
    // Compared ACROSS buffers: `Object`'s derived `PartialEq` compares a
    // stream's `ByteSpan`, which a save legitimately relocates, so a
    // span-sensitive comparison reports a phantom change on every
    // stream-bearing file. See `object::equivalent_across_buffers`.
    //
    // Two objects are excluded, and both exclusions are principled
    // rather than convenient:
    //
    //  - the base file's own cross-reference-stream object, which the
    //    new section supersedes (`is_section_object`);
    //  - under `--producer set`, the document information dictionary,
    //    which the policy rewrote ON PURPOSE. Reporting an intentional
    //    metadata change as a round-trip regression would be a false
    //    red, and would make the one flag R41 requires unusable.
    let deliberately_rewritten = match producer {
        ProducerArg::Set => doc
            .trailer()
            .get(b"Info")
            .and_then(pdfcer_core::object::Object::as_reference),
        ProducerArg::Preserve => None,
    };
    let reloaded = open_document_bytes(bytes.clone());
    let reload_ok = match &reloaded {
        Ok(back) => doc.objects().all(|io| {
            is_section_object(&doc, io.id)
                || Some(io.id) == deliberately_rewritten
                || back.get(io.id).is_some_and(|b| {
                    pdfcer_core::object::equivalent_across_buffers(
                        &b.value,
                        back.bytes(),
                        &io.value,
                        doc.bytes(),
                    )
                })
        }),
        Err(_) => false,
    };

    // --- check 1: byte identity, in the shape this mode promises -----
    let (identical, identity_note) = match mode {
        RoundTripMode::Incremental => (
            bytes == source,
            "whole-file byte identity (empty dirty set)",
        ),
        RoundTripMode::AppendIdentity => (
            // The one permitted insertion is a separating EOL when the
            // base file's final byte is not one (§7.2.3's
            // comment-runs-to-end-of-line rule).
            bytes.starts_with(&source)
                || (bytes.get(..source.len()) == Some(&source[..])
                    && matches!(bytes.get(source.len()), Some(b'\n'))),
            "prior bytes unchanged (§7.5.6 append)",
        ),
        RoundTripMode::Full => (
            reloaded.as_ref().is_ok_and(|back| {
                full_rewrite_is_per_object_verbatim(&doc, back, deliberately_rewritten)
            }),
            "per-object-definition byte identity",
        ),
    };

    // --- check 2b: the saved file is still USABLE ---------------------
    //
    // `reload_ok` above asks "did every object I HAVE survive?" — a
    // survivorship test over the model. It cannot see a saved file that
    // REFERENCES something absent, because §7.3.10 makes a dangling
    // reference resolve to null rather than an error, so the model reads
    // clean while the file is broken.
    //
    // That blindness shipped. On 2026-08-07 the veraPDF parse gate found
    // pdfcer writing a catalog that said `/Pages 2 0 R` with object 2
    // absent, and `round-trip --mode full` reported SUCCESS on it — the
    // verb whose entire job is verifying the save invariant was the thing
    // that missed it. Fixing only the dropped object would have left this
    // check just as blind to the next one (R163: strengthen the gate, do
    // not write a note asking future authors to look harder).
    //
    // The criterion is COMPARATIVE, and that is deliberate. Asking "does
    // the saved file have a page tree?" would fail every round-trip over
    // a legitimately broken corpus file, where faithfully preserving the
    // damage is correct behaviour. What must never happen is a save that
    // DESTROYS a page tree the source had. §7.7.2 Table 28 makes /Pages
    // required, so a document that loses it is one no conforming reader
    // can open.
    let resolves_page_tree = |d: &Document| -> bool {
        d.catalog()
            .ok()
            .and_then(|c| c.get(b"Pages").cloned())
            .is_some_and(|p| d.resolve(&p).as_dict().is_some())
    };
    // Three outcomes, not two — the same shape `tools/verapdf-parse-gate.py`
    // settled on, and for the same reason. A save that DESTROYS a page
    // tree fails. A save that faithfully preserves an already-missing one
    // is not a failure, but it must not be SILENT either: the saved file
    // still names a /Pages object it does not contain, and an operator who
    // sees "round-trip ... identical=1" and nothing else will reasonably
    // conclude the output is usable. It is not.
    //
    // Stated plainly because it is the honest limit of this check: the
    // 2026-08-07 `bad6.pdf` defect lands in the PRESERVED bucket, not the
    // destroyed one, because pdfcer could not resolve the page tree on the
    // input either. So check 2b alone would not have caught it — the NOTE
    // below is the part that would have, by making the broken output
    // visible instead of letting a clean-looking result line speak for it.
    let page_tree_before = resolves_page_tree(&doc);
    let page_tree_after = reloaded.as_ref().is_ok_and(resolves_page_tree);
    let page_tree_kept = !page_tree_before || page_tree_after;
    if !page_tree_after && reloaded.is_ok() {
        eprintln!(
            "pdfcer: {}: NOTE: the saved file's catalog does not resolve to a page \
tree{}. \u{a7}7.7.2 Table 28 requires /Pages, so this output is not openable by a \
conforming reader.",
            input.display(),
            if page_tree_before {
                " — and the SOURCE resolved one, so the save destroyed it"
            } else {
                " — the source did not resolve one either, so this is preserved \
damage rather than new damage"
            }
        );
    }

    // --- check 3: raster self-comparison ------------------------------
    let mut raster_compared = 0u32;
    let mut raster_identical = 0u32;
    if compare_raster
        && let Ok(after) = &reloaded
        && let (Some(before_px), Some(after_px)) = (
            render_first_page(&doc, scale),
            render_first_page(after, scale),
        )
    {
        raster_compared = 1;
        raster_identical = u32::from(before_px == after_px);
    }

    if report.delinearized {
        // "Fuzzy, never sneaky": Annex F.1 makes de-linearization on
        // append normative and unavoidable, but it is still a property
        // the operator did not ask to spend.
        eprintln!(
            "pdfcer: {}: this file is linearized (Fast Web View); saving \
invalidates that. Per ISO 32000-1 Annex F.1 the result 'shall be treated as \
ordinary PDF'. pdfcer does not repair or re-linearize, and does not strip the \
/Linearized dictionary.",
            input.display()
        );
    }

    // The stable stdout line: narrative half, then `key=<integer>` pairs
    // in fixed order (module header, "stdout result-line format").
    println!(
        "round-trip {} mode={} -> {}; \
identical={} in_bytes={} out_bytes={} appended={} objects={} verbatim={} \
reserialized={} reloaded={} raster_compared={} raster_identical={} delinearized={} \
promoted={} page_tree_kept={}",
        input.display(),
        mode_name(mode),
        output.map_or_else(|| "<memory>".to_owned(), |p| p.display().to_string()),
        u32::from(identical),
        source.len(),
        report.bytes_written,
        report.bytes_appended,
        report.objects_written,
        report.objects_verbatim,
        report.objects_reserialized,
        u32::from(reload_ok),
        raster_compared,
        raster_identical,
        u32::from(report.delinearized),
        // Appended, never inserted: keys are added at the END so a
        // parser that reads by name keeps working (module docs). Only
        // ever non-zero in `append-identity` mode, where re-emitting a
        // compressed object necessarily promotes it out of its
        // container.
        report.promoted.len(),
        // Appended at the END for the same reason `promoted` was.
        u32::from(page_tree_kept),
    );

    // Ordered worst-first: an unloadable file is a more serious result
    // than a differing one, and a differing one than a raster mismatch
    // — a script branching on the code gets the most severe finding.
    match &reloaded {
        Err(err) => {
            eprintln!(
                "pdfcer: {}: the saved file did not reload: {err}",
                input.display()
            );
            return exit::RELOAD_FAILED;
        }
        Ok(_) if !reload_ok => {
            eprintln!(
                "pdfcer: {}: the saved file reloaded, but its object graph changed.",
                input.display()
            );
            return exit::RELOAD_FAILED;
        }
        Ok(_) if !page_tree_kept => {
            eprintln!(
                "pdfcer: {}: the source resolved a page tree and the saved file does \
not — the catalog names a /Pages object the file does not contain (ISO 32000-1 \
\u{a7}7.7.2 Table 28 requires it). No conforming reader can open the result.",
                input.display()
            );
            return exit::RELOAD_FAILED;
        }
        Ok(_) => {}
    }
    if !identical {
        eprintln!(
            "pdfcer: {}: {identity_note} FAILED — this is an \
ARCHITECTURE.md §5 round-trip violation.",
            input.display()
        );
        return exit::NOT_BYTE_IDENTICAL;
    }
    if raster_compared == 1 && raster_identical == 0 {
        eprintln!(
            "pdfcer: {}: page 1 re-renders differently after the save.",
            input.display()
        );
        return exit::RASTER_DIFFERS;
    }
    exit::SUCCESS
}

/// The `--mode` value as it appears on the stdout line.
pub(crate) const fn mode_name(mode: RoundTripMode) -> &'static str {
    match mode {
        RoundTripMode::Incremental => "incremental",
        RoundTripMode::Full => "full",
        RoundTripMode::AppendIdentity => "append-identity",
    }
}

/// Whether every `File`-provenance object's definition bytes are
/// **byte-identical and reachable through the new cross-reference
/// table** — the R32 per-object assertion for a full rewrite.
///
/// Compares each object's retained `ByteSpan` slice on both sides,
/// rather than searching the output for the bytes. Two reasons, and the
/// second matters more:
///
/// 1. **It is linear.** A substring search per object is
///    `objects × filesize`. The veraPDF corpus contains a deliberate
///    Annex C implementation-limits file with ~80,000 objects in 4 MB —
///    roughly 320 GB of byte comparisons, which blows any wall-clock
///    budget.
/// 2. **It is stricter.** "These bytes appear somewhere in the output"
///    is a weak claim: it passes even when the object landed at an
///    offset its own cross-reference entry does not name. Comparing the
///    span the RELOADED document resolved for that object id proves the
///    bytes are reachable *through the xref*, which is what §5 actually
///    promises.
///
/// Two objects are excluded: the base file's own cross-reference-stream
/// object (superseded by the newly generated section, so its old bytes
/// are *supposed* to be absent), and `rewritten` — the object a
/// `--producer set` policy deliberately changed. Counting an intentional
/// metadata edit as a round-trip violation would be a false red.
pub(crate) fn full_rewrite_is_per_object_verbatim(
    before: &pdfcer_core::document::Document,
    after: &pdfcer_core::document::Document,
    rewritten: Option<pdfcer_core::object::ObjId>,
) -> bool {
    use pdfcer_core::object::Provenance;

    for io in before.objects() {
        // Only `Provenance::File` promises verbatim re-emission. A
        // compressed object has no file-level bytes, and a
        // `Provenance::RecoveredFile` object has bytes that CONTRADICT its
        // parsed value (a recovered stream extent), so the writer
        // deliberately re-serializes it — asserting byte identity for
        // either would be asserting the opposite of the contract.
        let Provenance::File(span) = io.provenance else {
            continue;
        };
        if is_section_object(before, io.id) || Some(io.id) == rewritten {
            continue;
        }
        let want = span.slice(before.bytes());
        let got = after
            .get(io.id)
            .and_then(|o| o.file_span())
            .and_then(|s| s.slice(after.bytes()));
        if want.is_none() || got != want {
            eprintln!(
                "pdfcer: object {} lost its verbatim definition bytes",
                io.id
            );
            return false;
        }
    }
    true
}

/// Whether `id` is the object that *is* the base file's newest
/// cross-reference section (§7.5.8.1) rather than document content.
///
/// Excluded from every comparison: a save supersedes it, so its
/// dictionary legitimately differs (fresh `/Prev`, delta `/Index`, new
/// `/Length`) and its old bytes are supposed to be absent.
pub(crate) fn is_section_object(
    doc: &pdfcer_core::document::Document,
    id: pdfcer_core::object::ObjId,
) -> bool {
    use pdfcer_core::xref::SectionShape;
    matches!(doc.section_shape(), SectionShape::Stream { id: sid, .. } if sid == id)
}

/// Rasterize page 1 for the self-comparison oracle, or `None` if the
/// document has no renderable first page.
///
/// A render failure is not a round-trip failure — plenty of corpus files
/// are deliberately non-conformant — so this yields `None` and the
/// caller reports `raster_compared=0` rather than a false red.
pub(crate) fn render_first_page(
    doc: &pdfcer_core::document::Document,
    scale: f32,
) -> Option<Vec<u8>> {
    let pages = pdfcer_core::page_tree::pages(doc).ok()?;
    let page = pages.first()?;
    let rendered = pdfcer_render::render_page(doc, page, scale).ok()?;
    Some(rendered.pixmap.data().to_vec())
}

// ---------------------------------------------------------------------------
// Editing subcommands (Pass 3.1)
// ---------------------------------------------------------------------------

/// What an edit-and-save produced, for the stdout line.
pub(crate) struct EditOutcome {
    pub(crate) report: pdfcer_core::writer::SaveReport,
    /// Objects that currently differ from the base revision — the
    /// save-time diff, **not** a count of commands run.
    pub(crate) changed: usize,
    pub(crate) undo_verified: bool,
    pub(crate) undo_identical: bool,
}

/// Load `input` into an editing session, or print a diagnostic and
/// return the mapped exit code.
pub(crate) fn open_for_edit(input: &Path) -> Result<(Vec<u8>, pdfcer_core::edit::EditSession), u8> {
    let source = std::fs::read(input).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", input.display());
        exit::IO_ERROR
    })?;
    let doc = open_document_bytes(source.clone()).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", input.display());
        exit_code_for_doc(&err)
    })?;
    Ok((source, pdfcer_core::edit::EditSession::new(doc)))
}

/// Save an edited session, optionally verifying that undoing every edit
/// reproduces the input byte for byte.
///
/// ## The undo check always uses the incremental path, whatever `mode` is
///
/// The contract being verified is `ARCHITECTURE.md` §11.1's — *an object
/// edited and then undone must not appear in the update section* — and
/// its observable form is "zero edits means zero bytes", which only
/// incremental save promises. A full rewrite legitimately produces
/// different bytes for an unedited document (offsets move), so running
/// the check through it would assert something that is false by design.
///
/// The session is left with the edits **undone** afterwards. That is
/// deliberate rather than tidied up: the output file has already been
/// written, and re-applying the history only to throw the session away
/// would be motion without meaning.
/// With `verify_undo`, this leaves the session **UNDONE**.
///
/// The verification is `while session.undo().is_some() {}` followed by a save
/// of the emptied stack, and there is no redo afterwards — so on return the
/// session holds the document as it was **before** any edit. That is fine for
/// the verification itself, and it is a trap for the caller: **any state a
/// command reports must be read before this call, not after it.**
///
/// It bites quietly rather than loudly. `add-named-dest` printed `names=0`
/// immediately after successfully defining a destination, which is exactly
/// what a document with no destinations would print — no panic, no wrong
/// type, just a plausible number that was silently the pre-edit one. It was
/// found only by running the command with and without the flag and comparing
/// the two lines (R174).
///
/// Most commands are *accidentally* safe, because they report the verb's own
/// return value and that is computed before saving. A command that queries
/// the session for a summary figure is the shape that is not.
pub(crate) fn save_edited(
    session: &mut pdfcer_core::edit::EditSession,
    source: &[u8],
    output: &Path,
    mode: SaveMode,
    producer: ProducerArg,
    verify_undo: bool,
) -> Result<EditOutcome, u8> {
    use pdfcer_core::writer::{ProducerPolicy, SaveOptions};

    // The two BYTES-radius R169 knobs: §7.5.4's cross-reference entry
    // terminator (`EOL-A1`) and §7.5.5's trailing end-of-line (`EOL-A2`).
    // Both are genuine spec ambiguities — three legal forms and no stated
    // preference, and two self-consistent readings of the last line — and
    // both default to exactly what pdfcer has always emitted, so a machine
    // with no settings file writes byte-identical output.
    //
    // Applied to BOTH save modes, unlike `producer`: these describe bytes
    // the appended revision writes for itself, not a rewrite of anything
    // the operator did not touch, so rule 3 is untroubled. The
    // undo-verification save further down deliberately keeps pure
    // `identity()` — it is a byte comparison against the source and must
    // not acquire a dependency on a settings file.
    let (settings, settings_report) =
        pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
    report_settings(&settings_report);
    let eol = |options: SaveOptions| {
        options
            .with_xref_entry_eol(settings.xref_entry_eol)
            .with_trailing_eol(settings.trailing_eol)
    };

    let options = eol(SaveOptions::default().with_producer(match producer {
        ProducerArg::Set => ProducerPolicy::Set,
        ProducerArg::Preserve => ProducerPolicy::Preserve,
    }));
    let changed = session.dirty_set().len();

    let saved = match mode {
        SaveMode::Incremental => session.to_incremental_bytes(&eol(SaveOptions::identity())),
        SaveMode::Full => session.to_full_bytes(&options),
    };
    let (bytes, report) = saved.map_err(|err| {
        eprintln!("pdfcer: save refused: {err}");
        hint_recovered_base(&err);
        exit::SAVE_REFUSED
    })?;

    std::fs::write(output, &bytes).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", output.display());
        exit::IO_ERROR
    })?;

    let mut undo_identical = false;
    if verify_undo {
        while session.undo().is_some() {}
        let undone = session
            .to_incremental_bytes(&SaveOptions::identity())
            .map_err(|err| {
                eprintln!("pdfcer: undo verification could not save: {err}");
                exit::SAVE_REFUSED
            })?
            .0;
        undo_identical = undone == source;
    }

    Ok(EditOutcome {
        report,
        changed,
        undo_verified: verify_undo,
        undo_identical,
    })
}

/// Print the shared warnings and return the final exit code for an
/// editing subcommand.
pub(crate) fn finish_edit(input: &Path, outcome: &EditOutcome) -> u8 {
    if outcome.changed == 0 {
        // "Zero edits means zero bytes" is the writer's contract, so an
        // unchanged document produces a byte copy rather than an empty
        // revision. Saying so is the difference between a no-op the
        // operator understands and one they mistake for a failure.
        eprintln!(
            "pdfcer: {}: nothing changed — the document already had the requested \
value(s), so the output is a byte-for-byte copy of the input and no revision was appended.",
            input.display()
        );
    }
    if !outcome.report.promoted.is_empty() {
        // R38: a representation change to an object whose value the
        // operator may not have edited. Named, not just counted.
        let names: Vec<String> = outcome
            .report
            .promoted
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        eprintln!(
            "pdfcer: {}: {} object(s) were moved out of an object stream because they were \
touched (ISO 32000-1 §7.5.7 objects cannot be edited in place): {}. Their previous values remain \
inside the untouched container, which is normal for an edit and is NOT sufficient for redaction.",
            input.display(),
            outcome.report.promoted.len(),
            names.join(", ")
        );
    }
    if outcome.report.delinearized {
        eprintln!(
            "pdfcer: {}: this file is linearized (Fast Web View); saving invalidates that. \
Per ISO 32000-1 Annex F.1 the result 'shall be treated as ordinary PDF'. pdfcer does not repair \
or re-linearize, and does not strip the /Linearized dictionary.",
            input.display()
        );
    }
    if outcome.undo_verified && !outcome.undo_identical {
        eprintln!(
            "pdfcer: {}: UNDO VERIFICATION FAILED — undoing the edit did not reproduce the \
input byte for byte. This is an ARCHITECTURE.md §11.1 violation (the dirty set must be a diff \
against the base revision, not the union of every command run).",
            input.display()
        );
        return exit::NOT_BYTE_IDENTICAL;
    }
    exit::SUCCESS
}

/// Map an [`EditError`](pdfcer_core::edit::EditError) to an exit code.
///
/// Everything except a genuine structural failure of the page tree is
/// [`exit::EDIT_REFUSED`]: the file was readable and pdfcer declined the
/// operation as asked, which a batch script must be able to tell apart
/// from a broken file.
pub(crate) fn report_edit_error(input: &Path, err: &pdfcer_core::edit::EditError) -> u8 {
    use pdfcer_core::edit::EditError;
    // Translate the ONE error whose text is in the engine's units rather
    // than the operator's.
    //
    // `pdfcer-core` is 0-based throughout and `EditError::PageOutOfRange`
    // formats itself that way, correctly — it has no idea what a caller
    // typed. Every `--page` flag in this binary is **1-based**, so passing
    // that message straight through answers `--page 99` with *"page index 98
    // is out of range"*, and the operator has to work out that the tool
    // subtracted one before deciding whether they made a mistake or pdfcer
    // did. Found by reading this command's own output on a deliberate
    // out-of-range run (R174); it applies to all eight 1-based callers, so
    // it is fixed once here at the boundary rather than in each of them.
    //
    // Only `--page` is 1-based. Sub-page indices (`--index` into `/Annots`,
    // `--object`, `--run`, `--node`) are 0-based in both the flag and the
    // engine, and none of them raises this variant — so there is no caller
    // for which this translation would be wrong.
    if let EditError::PageOutOfRange { index, count } = err {
        eprintln!(
            "pdfcer: {}: page {} is out of range (the document has {count} page(s))",
            input.display(),
            index.saturating_add(1),
        );
        return exit::EDIT_REFUSED;
    }
    eprintln!("pdfcer: {}: {err}", input.display());
    match err {
        EditError::PageTree(_) => exit::RUNTIME_ERROR,
        _ => exit::EDIT_REFUSED,
    }
}
