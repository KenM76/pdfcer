use super::*;

/// `unembed-font` — remove embedded font programs, dry-run by default.
///
/// # Why the dry run is the default
///
/// `print` requires `--send` before paper moves; this requires `--apply`
/// before bytes move, for the same reason and one more. Unembedding is not
/// reversible from the output file: the program is gone from it, and the
/// only copy of the original is the input the operator still has. A default
/// that writes would make "I wanted to see what it would do" and "do it"
/// the same command.
///
/// The dry run runs the **whole** operation — the inventory, the plan, the
/// sharing census, the PDF/A detection — and prints exactly what `--apply`
/// would print, minus the save. Nothing is estimated.
///
/// # Why every refusal is printed, and Acrobat prints none
///
/// Acrobat refuses a font whose character codes are glyph indices into its
/// own embedded program by leaving it out of the unembed list, with no
/// reason shown anywhere (sourced to a former Adobe Principal Scientist in
/// `Acrobat_Features/optimize__font_unembedding.md`). A shorter list is not
/// actionable. Refusal is also the *majority* path here — 52 % of embedded
/// fonts across a 400-file corpus — so a command that quietly did less than
/// asked would be the normal experience of using it.
///
/// # Why two byte figures are printed and not one
///
/// `reclaim_on_full` is what a full rewrite drops. `reclaim_now` is what
/// this save actually drops, which for the default incremental mode is
/// **zero** — §7.5.6's update section is appended, so the freed program's
/// bytes are still in the prior revision and the output is larger than the
/// input. An operator whose whole goal is a smaller file is exactly the
/// operator most likely to read one number and stop, so both are on the
/// line and the difference is stated on stderr when it bites.
pub(crate) fn cmd_unembed_font(args: &UnembedArgs<'_>) -> u8 {
    use pdfcer_core::edit::EditError;
    use pdfcer_core::font_unembed::{PdfaClaim, UnembedRequest, UnembedSelection};

    if args.apply && args.output.is_none() {
        eprintln!("pdfcer: --apply needs --output <PATH>; a dry run needs neither");
        return exit::EDIT_REFUSED;
    }

    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let selection = if args.all_removable {
        UnembedSelection::AllRemovable
    } else {
        UnembedSelection::Named(args.fonts.to_vec())
    };
    // Built through the constructors rather than a struct literal:
    // `UnembedRequest` is `#[non_exhaustive]`, which is the API-guidelines
    // posture for a request type that will grow options.
    let request = match selection {
        UnembedSelection::AllRemovable => UnembedRequest::all_removable(),
        UnembedSelection::Named(names) => UnembedRequest::named(names),
        _ => UnembedRequest::all_removable(),
    };
    let request = if args.keep_subset_tag {
        request.keeping_subset_tag()
    } else {
        request
    };

    // The plan is computed ONCE and printed before anything is decided, so
    // the dry run and the apply are looking at the same evidence.
    let plan = session.unembed_preview(&request);

    println!("unembed-font {}", args.input.display());
    for t in &plan.targets {
        let name = t.base_font.as_deref().unwrap_or("-");
        let rename = t
            .rename
            .as_deref()
            .map_or_else(|| "unchanged".to_owned(), |n| format!("{n:?}"));
        let shared = if t.program_shared_with.is_empty() {
            String::new()
        } else {
            let ids: Vec<String> = t
                .program_shared_with
                .iter()
                .map(|id| format!("{}", id.num))
                .collect();
            format!(" program_shared_with={}", ids.join(","))
        };
        println!(
            "  unembed name={name:?} obj={} key={} bytes={} freed={} rename={rename} \
charset_removed={} cidset_removed={} pages={}{shared}",
            t.id.num,
            t.program_key.label(),
            t.stored_bytes,
            u32::from(t.program_freed),
            u32::from(t.char_set_removed),
            u32::from(t.cid_set_removed),
            pdfcer_core::fontinfo::format_page_ranges(&t.pages),
        );
    }
    // The disclosure Acrobat does not make. Every refused font, by name,
    // on stdout with the rest of the report — not hidden on stderr and not
    // omitted, because a font that is missing from both lists is the exact
    // silence this command exists to break.
    for b in &plan.blocked {
        let name = b.base_font.as_deref().unwrap_or("-");
        let obj =
            b.id.map_or_else(|| "direct".to_owned(), |id| format!("{}", id.num));
        println!(
            "  refused name={name:?} obj={obj} bytes={} verdict={}",
            b.stored_bytes,
            b.blocker.token(),
        );
        println!("    reason: {}", b.blocker.reason());
    }
    for name in &plan.unmatched {
        println!("  unmatched {name:?}");
    }

    let reclaim_on_full = plan.bytes_reclaimable();
    let reclaim_now = if matches!(args.mode, SaveMode::Full) {
        reclaim_on_full
    } else {
        0
    };
    println!(
        "  fonts={} refused={} unmatched={} reclaim_on_full={reclaim_on_full} \
reclaim_now={reclaim_now} pdfa={} mode={} applied={}",
        plan.targets.len(),
        plan.blocked.len(),
        plan.unmatched.len(),
        plan.pdfa.token(),
        args.mode.name(),
        u32::from(args.apply),
    );

    // Appearance change, stated as a fact and not a risk, whether or not
    // this run writes anything. It is the consequence an operator is least
    // likely to have thought about and the one they cannot see in a report.
    if !plan.targets.is_empty() {
        eprintln!(
            "pdfcer: {}: the pages using these fonts WILL LOOK DIFFERENT. Each glyph keeps its \
exact advance (/Widths is preserved), but the substituted face's own shapes and widths are not \
those numbers, so letters sit differently inside correctly-placed cells.",
            args.input.display()
        );
    }
    if plan.renames_any() {
        eprintln!(
            "pdfcer: {}: the six-letter subset tag is being removed from /BaseFont and \
/FontName (ISO 32000-1 §9.6.4, Table 122), because a name like ABCDEF+Arial matches no installed \
font once the program is gone. Pass --keep-subset-tag to leave both alone.",
            args.input.display()
        );
    }
    if !matches!(args.mode, SaveMode::Full) && reclaim_on_full > 0 {
        eprintln!(
            "pdfcer: {}: an incremental save RECLAIMS NOTHING — ISO 32000-1 §7.5.6's update \
section is appended, so the removed program's {reclaim_on_full} byte(s) stay in the prior \
revision and the output is LARGER than the input. Use --mode full to drop them.",
            args.input.display()
        );
    }
    for t in &plan.targets {
        if t.program_freed {
            continue;
        }
        eprintln!(
            "pdfcer: {}: {:?}'s font program is also reached by {} other font(s) that are NOT \
being unembedded, so the program object stays in the file. This font is unembedded; its bytes \
are not recovered.",
            args.input.display(),
            t.base_font.as_deref().unwrap_or("-"),
            t.program_shared_with.len(),
        );
    }

    // PDF/A: refused before anything is written unless acknowledged. Unlike
    // redaction's residuals — which are only knowable after the removal —
    // this is knowable in advance, so the operator gets the choice rather
    // than the news.
    if let PdfaClaim::Identified { part, conformance } = &plan.pdfa {
        let level = format!(
            "PDF/A-{}{}",
            part.as_deref().unwrap_or("?"),
            conformance.as_deref().unwrap_or("")
        );
        eprintln!(
            "pdfcer: {}: this document identifies itself as {level} (XMP pdfaid). EVERY part \
of ISO 19005 requires fonts to be embedded, so unembedding breaks that conformance, and pdfcer \
does not remove or correct the claim for you.",
            args.input.display()
        );
        if args.apply && !args.acknowledge_pdfa {
            eprintln!(
                "pdfcer: refusing to write: pass --acknowledge-pdfa to proceed anyway. \
Nothing has been changed."
            );
            return exit::EDIT_REFUSED;
        }
    } else if matches!(plan.pdfa, PdfaClaim::MetadataUnreadable) {
        eprintln!(
            "pdfcer: {}: this document's XMP metadata could not be read, so pdfcer could NOT \
check whether it claims PDF/A conformance. That is not the same as finding no claim.",
            args.input.display()
        );
    }

    if !plan.unmatched.is_empty() {
        eprintln!(
            "pdfcer: {}: {} --font name(s) matched no font in this document. Run `list-fonts` \
to see the names it actually carries.",
            args.input.display(),
            plan.unmatched.len()
        );
        return exit::EDIT_REFUSED;
    }

    if !args.apply {
        if plan.targets.is_empty() {
            // Mirrors `embed-font`'s dry-run wording, and for the same
            // reason: the "printed above" half is a claim about this
            // report, so it is made only when something was printed.
            if plan.blocked.is_empty() {
                eprintln!(
                    "pdfcer: {}: DRY RUN — nothing would be unembedded, and nothing was \
refused: this document carries no font program that can be removed.",
                    args.input.display()
                );
            } else {
                eprintln!(
                    "pdfcer: {}: DRY RUN — nothing would be unembedded. Every refusal is \
printed above with its reason.",
                    args.input.display()
                );
            }
            return exit::EDIT_REFUSED;
        }
        eprintln!(
            "pdfcer: {}: DRY RUN — no file was written. Re-run with --apply --output <PATH> \
to perform this.",
            args.input.display()
        );
        return exit::SUCCESS;
    }

    let applied = match session.unembed_fonts(&request) {
        Ok(applied) => applied,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.input.display());
            return match err {
                EditError::PageTree(_) => exit::RUNTIME_ERROR,
                _ => exit::EDIT_REFUSED,
            };
        }
    };
    // The plan the operator read and the plan that ran are the same value,
    // produced by the same function. Saying so costs one comparison and
    // makes a future divergence a test failure rather than a surprise.
    debug_assert_eq!(applied.targets.len(), plan.targets.len());

    // A signed document: disclosed, never silently broken. `impact_of` is
    // asked AFTER the edit and immediately before the save, because §11.1
    // makes the dirty set a save-time diff — the answer is not knowable at
    // edit time.
    let impact = session.signature_impact_of_save(match args.mode {
        SaveMode::Incremental => CoreSaveMode::Incremental,
        SaveMode::Full => CoreSaveMode::FullRewrite,
    });
    println!("  signature_impact={impact:?}");

    let Some(output) = args.output else {
        eprintln!("pdfcer: --apply needs --output <PATH>");
        return exit::EDIT_REFUSED;
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "  wrote {} objects={} verbatim={} reserialized={} appended={} out_bytes={} \
in_bytes={} undo_verified={} undo_identical={}",
        output.display(),
        r.objects_written,
        r.objects_verbatim,
        r.objects_reserialized,
        r.bytes_appended,
        r.bytes_written,
        source.len(),
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// `embed-font` — add the font programs a document is missing, dry-run by
/// default.
///
/// # Why a dry run is the default here too, when nothing is destroyed
///
/// `unembed-font` defaults to a dry run because it removes something the
/// output cannot get back. This command removes nothing, so that argument
/// does not transfer — and the default is the same anyway, for a different
/// reason.
///
/// **Embedding is an inference.** pdfcer is choosing font programs the
/// document did not carry, and on a typical run some of those choices are
/// stand-ins rather than the face the file names. The operator has to be
/// able to see WHICH before it becomes document state (project rule 4), and
/// a command that wrote on the first invocation would make "show me what you
/// would pick" and "pick it" the same act. It is also the shape `print`
/// (`--send`) and `unembed-font` (`--apply`) already established, and a
/// third font command with a different default would be the surprise.
///
/// # What the report says, and why each column is on it
///
/// Per resolved font: the face chosen, the file it came from, and
/// `match=exact|alias|bundled` — the disclosure rule 4 requires. Per refused
/// font: its name and a reason that says what would satisfy it. Then, on the
/// summary line, **`not_embedded_after`** — the number the operator is
/// actually trying to drive to zero. A report that showed only what pdfcer
/// managed to do would read as success over a file a print service will
/// still reject.
pub(crate) fn cmd_embed_font(args: &EmbedArgs<'_>) -> u8 {
    use pdfcer_core::edit::EditError;
    use pdfcer_core::font_embed_missing::{EmbedRequest, EmbedSelection, FontMatch, SuppliedFont};
    use pdfcer_core::fontinfo::Program;
    use pdfcer_render::font::EmbedMatch;

    if args.apply && args.output.is_none() {
        eprintln!("pdfcer: --apply needs --output <PATH>; a dry run needs neither");
        return exit::EDIT_REFUSED;
    }

    // The SHELL owns the filesystem: the environment is built here and
    // `pdfcer-core` is handed bytes (project rule 2). Reuses `--font-dir`'s
    // one walker rather than a second one (R171).
    let (font_env, supplied_registered, font_notes) = build_font_environment(args.font_dirs);
    for note in &font_notes {
        eprintln!("pdfcer: font-dir: {note}");
    }
    if supplied_registered == 0 && !args.use_bundled_fonts {
        eprintln!(
            "pdfcer: no font folder supplied any usable face. Pass --font-dir <DIR> pointing \
at a folder of font files (on Windows, C:\\Windows\\Fonts), or --use-bundled-fonts to offer \
pdfcer's own standard-14 substitutes."
        );
    }

    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // Resolve a donor for every font the document is missing. The inventory
    // is phase A's, consumed rather than re-derived.
    let inventory = pdfcer_core::fontinfo::inventory(&session.view());
    let selection = if args.all_missing {
        EmbedSelection::AllMissing
    } else {
        EmbedSelection::Named(args.fonts.to_vec())
    };
    let mut request = match &selection {
        EmbedSelection::Named(names) => EmbedRequest::named(names.clone()),
        _ => EmbedRequest::all_missing(),
    };
    let mut resolutions: Vec<(String, String, String, FontMatch)> = Vec::new();
    for record in &inventory.fonts {
        if !matches!(record.program, Program::NotEmbedded) {
            continue;
        }
        let Some(base_font) = record.base_font.as_deref() else {
            continue;
        };
        let Some(donor) = font_env.resolve_for_embedding(base_font, args.use_bundled_fonts) else {
            continue;
        };
        let matched = match donor.quality {
            EmbedMatch::Exact => FontMatch::Exact,
            EmbedMatch::Alias => FontMatch::Alias,
            EmbedMatch::Bundled => FontMatch::Bundled,
        };
        // A bundled face has no path; the source string says so in words
        // rather than printing an empty field.
        let source_label = if matches!(matched, FontMatch::Bundled) {
            format!("bundled: {}", donor.face_name)
        } else {
            format!("--font-dir face {:?}", donor.face_name)
        };
        resolutions.push((
            base_font.to_owned(),
            donor.face_name.clone(),
            source_label.clone(),
            matched,
        ));
        request = request.with_font(
            base_font,
            SuppliedFont::new(
                donor.data.bytes().to_vec(),
                donor.face_name.clone(),
                source_label,
                matched,
            ),
        );
    }

    // Computed ONCE and printed before anything is decided, so the dry run
    // and the apply are looking at the same evidence.
    let plan = session.embed_preview(&request);

    println!("embed-font {}", args.input.display());
    println!(
        "  supplied_registered={supplied_registered} resolved={} bundled_allowed={}",
        resolutions.len(),
        u32::from(args.use_bundled_fonts),
    );
    for t in &plan.targets {
        let name = t.base_font.as_deref().unwrap_or("-");
        println!(
            "  embed name={name:?} obj={} shape={} key={} subtype={} format={} face={:?} \
match={} bytes={} redeclared={} widths={} encoding={} descriptor={} rename={} pages={}",
            t.id.num,
            t.shape.token(),
            t.program_key.label(),
            t.stream_subtype.unwrap_or("-"),
            t.format.token(),
            t.face_name,
            t.matched.token(),
            t.program_bytes,
            u32::from(t.redeclared_truetype),
            t.widths_written,
            u32::from(t.encoding_written),
            u32::from(t.descriptor_written),
            t.rename.as_deref().unwrap_or("unchanged"),
            pdfcer_core::fontinfo::format_page_ranges(&t.pages),
        );
        println!("    source: {}", t.source);
    }
    // Every refused font, by name, on stdout with the rest of the report —
    // the same disclosure posture `unembed-font` takes, and for the same
    // reason: a font missing from both lists is a silence the operator
    // cannot act on.
    for b in &plan.blocked {
        // "Already embedded" is not a refusal an operator needs a paragraph
        // about; it is the answer to "why is this row not in the list".
        let name = b.base_font.as_deref().unwrap_or("-");
        let obj =
            b.id.map_or_else(|| "direct".to_owned(), |id| format!("{}", id.num));
        println!(
            "  refused name={name:?} obj={obj} reason={}",
            b.blocker.token()
        );
        if b.blocker.token() != "already-embedded" {
            println!("    reason: {}", b.blocker.reason());
        }
    }
    for name in &plan.unmatched {
        println!("  unmatched {name:?}");
    }

    let substitutes = plan
        .targets
        .iter()
        .filter(|t| t.matched.is_substitute())
        .count();
    println!(
        "  fonts={} exact={} substitute={} refused={} unmatched={} bytes_added_max={} \
not_embedded_before={} not_embedded_after={} pdfa={} mode={} applied={}",
        plan.targets.len(),
        plan.targets.len() - substitutes,
        substitutes,
        plan.blocked.len(),
        plan.unmatched.len(),
        plan.bytes_added_uncompressed(),
        plan.missing_before,
        plan.missing_after(),
        plan.pdfa.token(),
        args.mode.name(),
        u32::from(args.apply),
    );

    // The two disclosures rule 4 requires, stated as facts rather than
    // implied by the report's shape.
    if !plan.targets.is_empty() {
        eprintln!(
            "pdfcer: {}: character POSITIONS do not change — a PDF spaces text from its own \
/Widths array, which is preserved or written from the standard metrics a reader was already \
using. The LETTERFORMS will differ wherever the face embedded is not the one the document \
names.",
            args.input.display()
        );
    }
    if substitutes > 0 {
        eprintln!(
            "pdfcer: {}: {substitutes} of these use a STAND-IN face, not the one the document \
names. Each is printed above with match=alias or match=bundled and the face actually used.",
            args.input.display()
        );
    }
    if plan.redeclares_any() {
        eprintln!(
            "pdfcer: {}: one or more fonts are being re-declared from a PostScript font to a \
TrueType font, because the face supplied for them carries TrueType outlines and ISO 32000-1 \
§9.9 Table 126 admits no other way to attach one. The character mapping is written out \
explicitly at the same time, so the text is unchanged.",
            args.input.display()
        );
    }
    // The "listed above" half of this is a CLAIM ABOUT THIS REPORT, and it
    // is only true for the fonts that actually got a `refused` row. Under
    // `--font <name>` a font the operator did not name is neither embedded
    // nor refused — `plan` omits it on purpose — so it is counted here and
    // explained nowhere. Saying "every one is listed above" then sends the
    // operator looking for reasons that were never printed, which is the
    // tool misdescribing its own output (project rule 4). The two groups are
    // therefore stated separately, and the "listed above" sentence is
    // printed only for the group it is true of.
    if plan.missing_after() > 0 {
        eprintln!(
            "pdfcer: {}: {} font(s) will STILL have no embedded program. A service that \
requires embedded fonts will still reject this file.",
            args.input.display(),
            plan.missing_after()
        );
        if plan.explained_missing() > 0 {
            eprintln!(
                "pdfcer: {}: {} of those are listed above as `refused`, each with its reason.",
                args.input.display(),
                plan.explained_missing()
            );
        }
        if plan.unexplained_missing() > 0 {
            eprintln!(
                "pdfcer: {}: {} of those were NOT part of this operation and carry no reason \
above — this run only considered the font name(s) you passed with --font. Re-run with \
--all-missing to have every missing font considered and reported.",
                args.input.display(),
                plan.unexplained_missing()
            );
        }
    }

    if !plan.unmatched.is_empty() {
        eprintln!(
            "pdfcer: {}: {} --font name(s) matched no font in this document. Run `list-fonts` \
to see the names it actually carries.",
            args.input.display(),
            plan.unmatched.len()
        );
        return exit::EDIT_REFUSED;
    }

    if !args.apply {
        if plan.targets.is_empty() {
            // Same discipline as the `missing_after` disclosure above: point
            // at the reasons only when there ARE reasons. "Every refusal is
            // printed above" over an empty list is vacuously true and reads
            // as though the operator missed something, when the real answer
            // is that this document had nothing to embed into.
            if plan.blocked.is_empty() {
                eprintln!(
                    "pdfcer: {}: DRY RUN — nothing would be embedded, and nothing was \
refused: this document has no font that is missing its program.",
                    args.input.display()
                );
            } else {
                eprintln!(
                    "pdfcer: {}: DRY RUN — nothing would be embedded. Every refusal is \
printed above with its reason.",
                    args.input.display()
                );
            }
            return exit::EDIT_REFUSED;
        }
        eprintln!(
            "pdfcer: {}: DRY RUN — no file was written. Re-run with --apply --output <PATH> \
to perform this.",
            args.input.display()
        );
        return exit::SUCCESS;
    }

    let applied = match session.embed_fonts(&request) {
        Ok(applied) => applied,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.input.display());
            return match err {
                EditError::PageTree(_) => exit::RUNTIME_ERROR,
                _ => exit::EDIT_REFUSED,
            };
        }
    };
    // The plan the operator read and the plan that ran are the same value,
    // produced by the same function.
    debug_assert_eq!(applied.targets.len(), plan.targets.len());

    // ATTRIBUTION FOR pdfcer's OWN BUNDLED FACES, written into the document
    // that will carry them.
    //
    // The bundled standard-14 substitutes are BSD-3-Clause (pdfium, over
    // Foxit-origin code). That licence places no restriction on embedding —
    // it is permissive — but it does attach one condition to redistribution
    // in binary form: the copyright notice, the conditions and the disclaimer
    // must be reproduced "in the documentation and/or other materials
    // provided with the distribution". A font program copied INTO a PDF that
    // the operator then sends to a printer, a client, or a store is exactly
    // such a redistribution.
    //
    // Leaving that obligation entirely to the operator is defensible — the
    // flag is opt-in and its help text states it — but it means the condition
    // is discharged only if they remember, every time, for every file. So
    // pdfcer discharges it itself, mechanically, at the one moment it knows
    // for certain that a bundled face has been embedded.
    //
    // WHY AN ATTACHMENT rather than XMP metadata: an embedded file is
    // literally a "material provided with the distribution" — a named,
    // extractable file that travels inside the PDF and is visible in any
    // reader's attachments pane. XMP would have been the other candidate,
    // but pdfcer's spec corpus has NO XMP coverage (`xmp__* = 0 files`), and
    // writing a metadata format from training-data recall is precisely what
    // project rule 1 forbids. §7.11 is fully sourced; XMP is not.
    //
    // ONLY when a bundled face was actually embedded. A run that resolved
    // everything from the operator's own font folder adds nothing — attaching
    // a licence for fonts the document does not contain would be noise, and
    // would make the attachment meaningless as a signal.
    let used_bundled = applied
        .targets
        .iter()
        .any(|t| matches!(t.matched, FontMatch::Bundled));
    if used_bundled {
        match session.attach_file(
            BUNDLED_FONT_NOTICE_NAME,
            bundled_font_notice().as_bytes(),
            Some("Licence notice for font programs embedded by pdfcer"),
        ) {
            Ok(_) => {
                println!("  attached={BUNDLED_FONT_NOTICE_NAME} reason=bundled-face-embedded");
                eprintln!(
                    "pdfcer: {}: this file now carries pdfcer's own substitute font \
                     face(s), which are BSD-3-Clause. The licence notice that condition \
                     requires has been attached to the PDF as `{BUNDLED_FONT_NOTICE_NAME}`, \
                     so it travels with the file. Do not strip it if you redistribute this \
                     document.",
                    args.input.display()
                );
            }
            Err(err) => {
                // Reported, never swallowed. If the notice could not be
                // written, the operator is about to distribute a file
                // carrying the faces WITHOUT it, and that is the one thing
                // they must not learn later.
                eprintln!(
                    "pdfcer: {}: the fonts were embedded, but the required BSD-3-Clause \
                     licence notice could NOT be attached: {err}. The bundled faces are in \
                     this file; the notice is not. Supply the attribution yourself if you \
                     redistribute it, or re-run with --font-dir so no bundled face is used.",
                    args.input.display()
                );
            }
        }
    }

    let impact = session.signature_impact_of_save(match args.mode {
        SaveMode::Incremental => CoreSaveMode::Incremental,
        SaveMode::Full => CoreSaveMode::FullRewrite,
    });
    println!("  signature_impact={impact:?}");

    let Some(output) = args.output else {
        eprintln!("pdfcer: --apply needs --output <PATH>");
        return exit::EDIT_REFUSED;
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "  wrote {} objects={} verbatim={} reserialized={} appended={} out_bytes={} \
in_bytes={} undo_verified={} undo_identical={}",
        output.display(),
        r.objects_written,
        r.objects_verbatim,
        r.objects_reserialized,
        r.bytes_appended,
        r.bytes_written,
        source.len(),
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// The parsed `embed-font` flags, gathered for the same reason
/// [`UnembedArgs`] is.
pub(crate) struct EmbedArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) fonts: &'a [String],
    pub(crate) all_missing: bool,
    pub(crate) font_dirs: &'a [PathBuf],
    pub(crate) use_bundled_fonts: bool,
    pub(crate) apply: bool,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// The parsed `unembed-font` flags, gathered so the implementation takes one
/// parameter rather than nine — `clippy::too_many_arguments` is a real
/// readability signal here and not a formality.
pub(crate) struct UnembedArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) fonts: &'a [String],
    pub(crate) all_removable: bool,
    pub(crate) apply: bool,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
    pub(crate) keep_subset_tag: bool,
    pub(crate) acknowledge_pdfa: bool,
}

/// The permissions notice printed by every encryption subcommand, verbatim
/// (rule 4, criterion 9 — the same wording sent to `pdfceGUI` and carried by
/// [`pdfcer_core::edit::EncryptionSettings::PERMISSIONS_DISCLOSURE`]).
pub(crate) const PERMISSIONS_NOTICE: &str = "PDF permissions are a request, not a lock. A conforming reader honours them; any program that ignores the flag can print, copy or change this document freely. Only the password protects the content -- and only the user password, which controls opening it.";

/// The filename the bundled-font licence notice is attached under.
///
/// Prefixed so it sorts and reads as tooling output rather than as one of the
/// operator's own attachments, and named for what it IS rather than for pdfcer,
/// because the person who opens the PDF later may have never heard of pdfcer
/// and needs to know at a glance why a licence file is in their document.
pub(crate) const BUNDLED_FONT_NOTICE_NAME: &str = "FONT-LICENSE-NOTICE.txt";

/// The BSD-3-Clause notice for pdfcer's bundled standard-14 substitute faces.
///
/// # Why this text is not paraphrased, summarised, or regenerated
///
/// BSD-3-Clause's condition is specifically that the copyright notice, "this
/// list of conditions" and "the following disclaimer" be REPRODUCED. A
/// summary does not satisfy a reproduction requirement, and a plausible
/// rewording of a licence is worse than useless — it is a claim about legal
/// terms that nobody checked. The body below is lifted verbatim from
/// `crates/pdfcer-render/assets/fonts/PROVENANCE.md`, which in turn records it
/// from pdfium's own LICENSE with the comment markers stripped.
///
/// The surrounding explanation is pdfcer's, and is deliberately separated from
/// the licence text by a rule, so a reader can see where our words stop and
/// the licence begins.
pub(crate) fn bundled_font_notice() -> String {
    format!(
        "\
FONT LICENCE NOTICE
===================

This PDF contains one or more embedded font programs that were supplied by
pdfcer's own bundled set of standard-14 substitute faces, rather than by the
document's author.

They were embedded because the document named a font it did not carry, and a
program was needed so that the text displays and prints the same way
everywhere -- for example, to satisfy a print service that requires all fonts
to be embedded.

The faces come from the Chromium pdfium project and are BSD-3-Clause
licensed. That licence permits this use. It also requires that the notice
below travel with any redistribution in binary form, which is why this file
is attached to the document rather than left somewhere else.

If you redistribute this PDF, keep this attachment.

Faces that may be present: FoxitSans, FoxitSerif, FoxitFixed (each in
regular, bold, italic and bold-italic), FoxitSymbol and FoxitDingbats.

----------------------------------------------------------------------
{}
----------------------------------------------------------------------

Attached automatically by pdfcer. Source of the faces:
https://pdfium.googlesource.com/pdfium/
",
        PDFIUM_BSD_LICENSE.trim()
    )
}

/// pdfium's BSD-3-Clause text, verbatim, comment markers removed.
///
/// Kept as its own constant so the reproduction requirement is satisfied by a
/// single unbroken block that can be diffed against
/// `crates/pdfcer-render/assets/fonts/PROVENANCE.md`. Interpolating pdfcer's own
/// prose into it would make that check impossible.
pub(crate) const PDFIUM_BSD_LICENSE: &str = r#"
Copyright 2014 The PDFium Authors

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

   * Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.
   * Redistributions in binary form must reproduce the above
copyright notice, this list of conditions and the following disclaimer
in the documentation and/or other materials provided with the
distribution.
   * Neither the name of Google Inc. nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
"#;
