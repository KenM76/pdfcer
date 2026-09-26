use super::*;

/// Warn when the visible document is a cover page for an encrypted payload.
///
/// # Why this is a warning and not a refusal
///
/// The wrapper really is a readable PDF, and everything pdfcer says about it
/// is true *of the wrapper*. Refusing to open it would withhold a document
/// the operator can legitimately read — the cover page is often the only
/// instructions they have. What must not happen is the operator taking
/// "1 page, no fields" as a fact about the protected content.
///
/// So: open it, report it, and say plainly that the counts describe the
/// cover. On stderr, so a script capturing stdout still shows a human.
pub(crate) fn disclose_wrapper(file: &Path, doc: &pdfcer_core::document::Document) {
    let info = pdfcer_core::wrapper::detect(doc);
    if let Some(message) = info.message() {
        eprintln!("pdfcer: {}: {message}", file.display());
    }
}

/// Implement `pdfcer inspect <file>`: probe the header and print the
/// declared version, or print a diagnostic and map the error to the
/// documented exit code.
pub(crate) fn cmd_inspect(file: &Path) -> u8 {
    let probe = pdfcer_core::probe_file(file);
    // A full load additionally surfaces cross-reference RECOVERY (decision
    // 013): a file whose header probes fine can still have an unparseable
    // cross-reference table that pdfcer rebuilds by scanning, and an
    // offset-start file fails the header probe but still recovers. `inspect`
    // stays lenient — a full-load *failure* does not fail the probe (a
    // header-valid-but-otherwise-broken file still reports its version) —
    // but a RECOVERED open is disclosed and gets a distinct exit status
    // (R20, fuzzy-never-sneaky).
    // The load ERROR is kept, not discarded. It used to be `.ok()`-ed away,
    // which is why an encrypted document reported the same clean line as a
    // readable one — the reason it could not be read was thrown away one
    // expression before anyone could report it.
    let full_result = std::fs::read(file)
        .map_err(pdfcer_core::document::DocError::Io)
        .and_then(open_document_bytes);
    let full = full_result.as_ref().ok();

    match (&probe, &full) {
        // Opened via recovery — disclose + distinct status, whether or not
        // the header probe itself succeeded.
        (_, Some(doc)) if doc.recovery().is_some() => {
            let version = match &probe {
                Ok(v) => v.to_string(),
                Err(_) => doc.version().to_string(),
            };
            println!("{}: PDF {version} (recovered)", file.display());
            if let Some(report) = doc.recovery() {
                disclose_recovery(file, report);
            }
            exit::OPENED_VIA_RECOVERY
        }
        // Clean header (whether or not the full body loaded) — the stable
        // probe line, unchanged.
        (Ok(version), _) => {
            println!("{}: PDF {version}", file.display());
            // What pdfcer DECIDED, before anything it merely observed.
            if let Some(doc) = full {
                disclose_load_anomalies(file, doc);
            }
            // The header probe succeeding is NOT the same as the document
            // being readable, and `inspect` used to say only the former.
            //
            // An encrypted PDF produced exactly the line above and exit 0 —
            // byte-identical in shape to a plain readable file. An operator
            // sweeping a directory to find what pdfcer can handle would have
            // been told every file was fine, and found out otherwise one
            // command later.
            //
            // That is R186's shape a third time: the refusal fires correctly
            // at the LOAD layer, and the layer a sweep actually runs first
            // never mentioned it. Reported here rather than moved, because
            // the probe line is genuinely useful on a file whose body will
            // not load — it is how you learn it is a PDF at all.
            if let Err(err) = &full_result {
                eprintln!(
                    "pdfcer: {}: the header reads as PDF {version}, but pdfcer could NOT load the document body: {err}. Anything reported above describes the header alone.",
                    file.display()
                );
                return exit_code_for_doc(err);
            }
            // ISO 32000-2 §7.6.7. Checked here rather than only in a forms
            // or attachments command because `inspect` is what a sweep runs
            // first, and the whole hazard is an operator concluding from a
            // clean result that they are looking at the document.
            if let Some(loaded) = full.as_ref() {
                disclose_wrapper(file, loaded);
                disclose_repaired_contents(file, loaded);
                disclose_actions(file, loaded);
            }
            exit::SUCCESS
        }
        // Header probe failed and recovery did not open it: not a PDF.
        (Err(err), _) => {
            eprintln!("pdfcer: {}: {err}", file.display());
            exit_code_for(err)
        }
    }
}

/// Disclose what this document would RUN in a reader that runs things
/// (`Pass 133.0`).
///
/// # Why `inspect`, and why this was the missing surface rather than a new one
///
/// `list-fields` has carried the action histogram since decision 009, and it
/// is the wrong and only place for it. `list-fields` is a FORMS command: an
/// operator reaches for it when they already believe the document has fields.
/// A document whose only hazard is a `/Launch` on a bookmark has no fields at
/// all, and the operator asking *"what is this file?"* runs `inspect` — which
/// said nothing about actions, on any document, ever.
///
/// So the same reasoning the encryption and wrapper disclosures above give
/// applies exactly: **`inspect` is what a sweep runs first**, and a hazard it
/// declines to mention is a hazard the sweep reports as absent.
///
/// # It is deliberately quiet on an ordinary interactive form
///
/// The line fires on [`FormJavaScript::reaches_outside`] or on a script, not
/// on [`FormJavaScript::any`]. A form with buttons that fill in and reset
/// itself reaches nothing and gets no warning, because a warning that fires on
/// every form is one operators learn to scroll past — and then the one that
/// matters scrolls past too.
///
/// Stderr, so the stable `path: PDF version` line a script parses is
/// undisturbed.
pub(crate) fn disclose_actions(file: &Path, doc: &pdfcer_core::document::Document) {
    let js = pdfcer_core::forms::scan_javascript(doc);
    if js.reaches_outside() {
        eprintln!(
            "pdfcer: {}: ★ THIS DOCUMENT REACHES OUTSIDE ITSELF. It carries {} action(s) that would contact the NETWORK and {} that would LAUNCH A PROCESS in Adobe Acrobat or Reader. pdfcer RECOGNISES them and NEVER runs any of them (R12/R13/R54) — this is a description of the file, not of anything pdfcer did. Where they are: {} on an annotation (something the operator clicks — a button or a link), {} on a page or navigation trigger (fires from MOVING THROUGH the document with nothing clicked, and a navigation node's /Dur fires it on a TIMER), {} on an outline item (a bookmark), and {} reachable ONLY by following an action's /Next chain — that is to say, not visible at any of the places you would look. Run `pdfcer list-fields` for the full histogram, which is printed whether or not the document has a form",
            file.display(),
            js.network_action_count,
            js.launch_action_count,
            js.annotation_actions,
            js.page_trigger_actions,
            js.outline_actions,
            js.chained_actions,
        );
    } else if js.javascript_actions > 0 || js.doc_level_scripts > 0 {
        eprintln!(
            "pdfcer: {}: this document carries {} JavaScript action(s) and {} document-level script(s) that Acrobat/Reader would run. pdfcer recognises them and runs none (R53/R54). None of them reaches the network or launches a process — the values a form calculates for itself are shown as last saved, never recomputed",
            file.display(),
            js.javascript_actions,
            js.doc_level_scripts,
        );
    }
    // SAID SEPARATELY, AND SAID EVEN WHEN NOTHING WAS FOUND, because these
    // are not the same claim: "pdfcer found no hazard" and "pdfcer stopped
    // looking" produce the identical silence otherwise, and for a
    // security-shaped disclosure that is the one confusion that must not be
    // possible.
    if js.scan_truncated {
        eprintln!(
            "pdfcer: {}: ★ THE ACTION SCAN DID NOT FINISH — it hit its own traversal ceiling after {} action(s). Whatever is reported above is a LOWER BOUND, not a total, and an absence of hazards above means pdfcer stopped looking rather than that the document is clean",
            file.display(),
            js.actions_scanned,
        );
    }
}

/// Disclose that this document carries the `/Contents` damage a pdfcer build
/// older than `Pass 111.0` wrote, and that pdfcer repaired it ON READ ONLY
/// (`Pass 111.0`).
///
/// # Why `inspect` and why at all
///
/// The repair is exact and silent, and silence is the problem: the operator's
/// file opens, renders and extracts perfectly here while **other readers may
/// still refuse it**, because the bytes on disk are unchanged. Rule 4 — pdfcer
/// inferred nothing here, but it did REPAIR something, and a repair the
/// operator cannot see is a repair they cannot act on.
///
/// `inspect` is the right home because it is what a sweep runs first, on the
/// same reasoning the encryption disclosure above it gives. Stderr, so a
/// script reading the stable `path: PDF version` line is undisturbed.
///
/// It also tells them the way out, which is the part that makes it actionable:
/// any edit that rewrites the page dictionary writes the flat form, so
/// re-saving the document through this build fixes it permanently.
pub(crate) fn disclose_repaired_contents(file: &Path, doc: &pdfcer_core::document::Document) {
    let Ok(pages) = pdfcer_core::page_tree::pages(doc) else {
        return;
    };
    let damaged: Vec<usize> = pages
        .iter()
        .enumerate()
        .filter(|(_, p)| p.contents_flattened > 0)
        .map(|(i, _)| i + 1)
        .collect();
    if damaged.is_empty() {
        return;
    }
    let list = damaged
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    eprintln!(
        "pdfcer: {}: {} page(s) carry a nested /Contents array (pages {list}) -- damage written by a pdfcer build older than Pass 111.0. pdfcer repairs this ON READ, so the document is fully usable here, but the FILE IS STILL DAMAGED and other readers may refuse it. Re-saving through this build after any edit writes the corrected form.",
        file.display(),
        damaged.len(),
    );
}

/// Print what pdfcer DECIDED about a file that contradicted itself
/// (`Pass 283.0`).
///
/// # Why this prints on a successful open
///
/// The operator's ruling: a file with errors must open, the errors must be
/// managed rather than fatal, and *"if the user can intervene in a decision
/// that should always be an option along with them not having to intervene."*
///
/// This is the "always an option" half at the CLI. The invocation IS the
/// commit here (project rule 11 — no session, no undo), so the disclosure
/// rides out with the result rather than waiting to be asked for: an operator
/// who never reads it still got a working document, and one who does can
/// re-run with `--duplicate-keys first` and compare.
///
/// Silent for a file that says nothing twice, which is nearly all of them.
pub(crate) fn disclose_load_anomalies(file: &Path, doc: &pdfcer_core::document::Document) {
    use pdfcer_core::document::LoadAnomaly;

    let anomalies = doc.load_anomalies();
    if anomalies.is_empty() {
        return;
    }
    eprintln!(
        "pdfcer: {}: this file contradicts itself in {} place(s); pdfcer decided rather than refusing, and here is what it decided:",
        file.display(),
        anomalies.len()
    );
    for a in anomalies {
        match a {
            LoadAnomaly::DuplicateDictKey {
                object,
                key,
                kept,
                discarded,
            } => {
                // The advice names the policy NOT in force. The first cut
                // always said "first", which is wrong advice the moment the
                // operator has already taken it — a remedy sentence is a claim
                // about what to do next, and one that is false half the time
                // teaches the reader to ignore all of them.
                let other = match cli_load_options().duplicate_keys {
                    pdfcer_core::parser::DuplicateKeyPolicy::KeepFirst => "keep-last",
                    _ => "keep-first",
                };
                eprintln!(
                    "  duplicate_dict_key object={} key=/{} kept={} discarded={} -- re-run with --on-malformed {other} to take the other one",
                    object.map_or_else(|| "trailer".to_owned(), |id| id.to_string()),
                    sanitize_token(&String::from_utf8_lossy(key)),
                    sanitize_token(&object_summary(kept)),
                    sanitize_token(&object_summary(discarded)),
                );
            }
            LoadAnomaly::StreamLengthRecovered { object } => {
                eprintln!(
                    "  stream_length_recovered object={object} -- its /Length was missing or unusable and the data extent was re-derived by scanning to `endstream`; there is no second reading to choose from"
                );
            }
            LoadAnomaly::MissingEndobjRecovered { object } => {
                eprintln!(
                    "  missing_endobj_recovered object={object} -- the definition had no `endobj` and was ended at the next object header"
                );
            }
            // `LoadAnomaly` is #[non_exhaustive]; a future kind must SAY
            // something rather than vanish, which is the whole point of the
            // list.
            other => eprintln!(
                "  {} -- this build of pdfcer-cli does not describe this kind in detail",
                other.kind()
            ),
        }
    }
}

/// A one-line rendering of an object for a disclosure line.
///
/// Deliberately shallow: a duplicate key's two values are usually names or
/// numbers, and a caller who needs the full value has the API. A nested
/// dictionary prints as its shape rather than its contents, because a
/// disclosure line that wraps is a disclosure line nobody reads.
pub(crate) fn object_summary(value: &pdfcer_core::object::Object) -> String {
    use pdfcer_core::object::Object;
    match value {
        Object::Name(n) => format!("/{}", String::from_utf8_lossy(n.as_bytes())),
        Object::Integer(i) => i.to_string(),
        Object::Real(r) => r.to_string(),
        Object::Boolean(b) => b.to_string(),
        Object::Null => "null".to_owned(),
        Object::String(s) => format!("({})", String::from_utf8_lossy(s)),
        Object::Reference(id) => format!("{id} R"),
        Object::Array(items) => format!("[{} item(s)]", items.len()),
        Object::Dict(d) => format!("<<{} key(s)>>", d.iter().count()),
        Object::Stream(_) => "<<stream>>".to_owned(),
        // `Object` is #[non_exhaustive] to this crate; a future kind gets a
        // shape rather than a panic or a silent blank.
        _ => "<unrecognised object kind>".to_owned(),
    }
}

/// Print the honest cross-reference-recovery disclosure (decision 013,
/// R20). One counted diagnostic line to stderr plus a save-behaviour note.
///
/// Diagnostics go to **stderr** so a script reading `inspect`'s stdout for
/// the stable `path: PDF version` line is not disturbed; the distinct
/// [`exit::OPENED_VIA_RECOVERY`] status is the machine-readable signal.
pub(crate) fn disclose_recovery(file: &Path, report: &pdfcer_core::recover::RecoveryReport) {
    eprintln!(
        "pdfcer: {}: opened via cross-reference recovery (rebuild-by-scan): \
         reason={:?}, file-level-objects={}, from-object-streams={}, \
         last-wins-collisions={}, trailer={:?}, offset-start={}, \
         stream-lengths-recovered={}, missing-endobj-recovered={}",
        file.display(),
        report.reason,
        report.file_level_objects,
        report.objstm_objects,
        report.last_wins_collisions,
        report.trailer_source,
        report.offset_start,
        report.stream_lengths_recovered,
        report.missing_endobj_recovered,
    );
    eprintln!(
        "pdfcer: {}: NOTE: the cross-reference table was rebuilt in memory; \
         saving will rewrite (normalize) the file, and incremental save is refused.",
        file.display()
    );
    if report.stream_lengths_recovered > 0 {
        eprintln!(
            "pdfcer: {}: NOTE: {} stream(s) had a /Length that did not agree with their \
             endstream keyword; their byte extents were re-derived from the keyword \
             (ISO 32000-1 \u{a7}7.3.8.2 defines /Length in terms of endstream). Those extents \
             are pdfcer's reading of the file, not the file's own claim.",
            file.display(),
            report.stream_lengths_recovered
        );
    }
    if report.missing_endobj_recovered > 0 {
        eprintln!(
            "pdfcer: {}: NOTE: {} object definition(s) had no `endobj` keyword \
             (ISO 32000-1 \u{a7}7.3.10 requires one); each was bounded at the next object \
             header instead of being dropped. Dropping is what pdfcer did before \
             2026-08-07, and when the dropped object was the page tree the saved file \
             named a /Pages object that was not in it.",
            file.display(),
            report.missing_endobj_recovered
        );
    }

    // THE LOSS ITSELF, NAMED (owed item 33; the disclosure `Pass 302.0`
    // recorded and nothing printed).
    //
    // `Pass 302.0` gave `RecoveryReport` an `objects_dropped` list so recovery
    // would stop returning a shorter document with no explanation. It wired
    // nothing to it: from a terminal the document was still silently shorter,
    // which is the half of rule 4 that actually bites. This function prints
    // every other field on that struct, and the struct's own doc comment says
    // the CLI surfaces all of them and that "none is rounded away" -- a
    // sentence that was false for exactly one field, the newest.
    //
    // The two reasons are printed SEPARATELY rather than summed, because
    // they are different news. `IdMismatch` means a definition contradicted
    // the offset that found it -- always worth a human's attention. `Unparseable`
    // is overwhelmingly binary data inside a stream that happens to spell
    // `N G obj`, and is routine; a combined count would make every ordinary
    // recovery look as alarming as a real loss, which is how a disclosure
    // trains its reader to ignore it.
    if !report.objects_dropped.is_empty() {
        use pdfcer_core::recover::DropReason;
        let mut unparseable: Vec<u32> = Vec::new();
        let mut mismatched: Vec<u32> = Vec::new();
        for d in &report.objects_dropped {
            match d.reason {
                DropReason::IdMismatch => mismatched.push(d.number),
                // `_` rather than naming `Unparseable`: `DropReason` is
                // `#[non_exhaustive]`, and a future reason must land in the
                // conservative bucket rather than stop this compiling or,
                // worse, go unreported.
                _ => unparseable.push(d.number),
            }
        }
        let list = |v: &[u32]| v.iter().map(u32::to_string).collect::<Vec<_>>().join(", ");
        if !mismatched.is_empty() {
            eprintln!(
                "pdfcer: {}: NOTE: {} scanned object(s) were NOT kept because the \
                 definition's own object number disagreed with the header that found \
                 it: {}. A definition that contradicts its offset cannot be trusted to \
                 be what the offset claimed, so it is dropped rather than guessed at.",
                file.display(),
                mismatched.len(),
                list(&mismatched),
            );
        }
        if !unparseable.is_empty() {
            eprintln!(
                "pdfcer: {}: NOTE: {} scanned object header(s) did not parse and were \
                 NOT kept: {}. Most are binary data inside a stream that happens to \
                 spell `N G obj` and cost nothing; a genuinely corrupt object looks the \
                 same from here, so if a page is blank or missing, these numbers are \
                 where it went.",
                file.display(),
                unparseable.len(),
                list(&unparseable),
            );
        }
    }
}

/// Upper bound on a single supplied font file, in bytes (pdfcer policy,
/// ARCHITECTURE.md §10 — never trust a file's size). 64 MiB comfortably
/// covers even large CJK OpenType faces while refusing a
/// pathologically-large or wrong-typed file before it is read into
/// memory. A file past this ceiling is skipped-and-noted, never fatal
/// (decision 012 acceptance).
pub(crate) const MAX_FONT_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Font-file extensions the `--font-dir` walk attempts to parse
/// (decision 012). Matched case-insensitively. A file with any other
/// extension is silently ignored (a font folder routinely also holds
/// `.txt` licences, `.json` metadata, etc.); a file WITH one of these
/// extensions that then fails to parse or is oversized IS noted, because
/// the operator meant it to be a usable font.
pub(crate) const FONT_FILE_EXTENSIONS: [&str; 7] =
    ["ttf", "otf", "ttc", "cff", "pfb", "pfa", "otc"];

/// Walk each `--font-dir` and build the [`pdfcer_render::FontEnvironment`]
/// the renderer will consult for the document's NON-embedded fonts
/// (decision 012 — the SHELL owns the filesystem; the renderer stays
/// bytes-in, R61).
///
/// For every readable font-extension file under each directory (sorted
/// for determinism, so which face wins a duplicate name is stable), the
/// bytes are parsed ONCE through `pdfcer-render`'s single skrifa parser
/// (R21) to read the face's advertised name(s), then registered via
/// [`pdfcer_render::FontEnvironment::insert_named`] under **both** every
/// advertised name AND the filename stem — so `Calibri.ttf` matches a
/// PDF `/BaseFont` of `Calibri` whether or not the program's internal
/// name agrees. A file that cannot be read, exceeds
/// [`MAX_FONT_FILE_BYTES`], or fails to parse is skipped and pushed to
/// the returned notes; it never aborts the walk or the render.
///
/// Returns the environment plus a `(registered, notes)` pair:
/// `registered` is the count of faces (name→file registrations) added,
/// `notes` are the human-readable skip/registration lines for stderr.
/// When `font_dirs` is empty the environment is exactly
/// [`pdfcer_render::FontEnvironment::bundled`] and both are empty — the
/// deterministic default path is untouched (R19/R63).
pub(crate) fn build_font_environment(
    font_dirs: &[PathBuf],
) -> (pdfcer_render::FontEnvironment, usize, Vec<String>) {
    use pdfcer_render::FontData;
    use pdfcer_render::font::program::FontProgram;

    let mut env = pdfcer_render::FontEnvironment::bundled();
    let mut registered = 0usize;
    let mut notes: Vec<String> = Vec::new();

    for dir in font_dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(err) => {
                notes.push(format!("font dir {}: {err}", dir.display()));
                continue;
            }
        };
        // Collect + sort so registration order (and therefore
        // duplicate-name precedence: last wins) is deterministic rather
        // than dependent on the OS directory-iteration order (R19 spirit,
        // even though the walk itself is shell-side).
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && has_font_extension(p))
            .collect();
        files.sort();

        for path in files {
            let meta = std::fs::metadata(&path);
            if let Ok(m) = &meta
                && m.len() > MAX_FONT_FILE_BYTES
            {
                notes.push(format!(
                    "skipped {}: {} bytes exceeds the {}-MiB font-file ceiling",
                    path.display(),
                    m.len(),
                    MAX_FONT_FILE_BYTES / (1024 * 1024)
                ));
                continue;
            }
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(err) => {
                    notes.push(format!("skipped {}: {err}", path.display()));
                    continue;
                }
            };
            // Parse ONCE (R21) to read the advertised name(s). The borrow
            // ends before `bytes` is moved into `FontData` below.
            let mut names: Vec<String> = match FontProgram::parse(&bytes) {
                Ok(program) => program.face_names(),
                Err(err) => {
                    notes.push(format!(
                        "skipped {}: not a usable font ({err})",
                        path.display()
                    ));
                    continue;
                }
            };
            // Always also register under the filename stem, so a match
            // works even when the internal name is odd or absent.
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && !names.iter().any(|n| n == stem)
            {
                names.push(stem.to_owned());
            }
            if names.is_empty() {
                notes.push(format!(
                    "skipped {}: parsed but advertises no name and has no usable filename",
                    path.display()
                ));
                continue;
            }
            let data = FontData::new(bytes);
            for name in &names {
                env.insert_named(name, data.clone());
                registered += 1;
            }
            notes.push(format!(
                "registered {} as: {}",
                path.display(),
                names.join(", ")
            ));
        }
    }

    (env, registered, notes)
}

/// Whether `path`'s extension is one of [`FONT_FILE_EXTENSIONS`]
/// (case-insensitive).
pub(crate) fn has_font_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| FONT_FILE_EXTENSIONS.contains(&e.as_str()))
}

/// Render one [`pdfcer_render::InkProbe`] as its own stdout line.
///
/// # The shape, and why absence is spelled rather than omitted
///
/// ```text
/// ink-probe: x=200 y=200 source=cmyk-buffer c=0.750 m=0.000 y=1.000 k=0.000 alpha=1.000 srgb=47,181,73
/// ink-probe: x=200 y=200 source=screen-srgb c=- m=- y=- k=- alpha=- srgb=47,180,73
/// ink-probe: x=99999 y=7 source=out-of-range c=- m=- y=- k=- alpha=- srgb=-
/// ```
///
/// The first two lines are the SAME PAGE and the SAME OPERAND, rendered
/// with and without a colorant buffer, and they differ by **one count of
/// blue**. That is real and is a property of the compositing path, not of
/// the conversion table — one path converts an 8-bit paint colour, the
/// other converts `f32` colorants at the very end. Do not read a one-count
/// blue between two probes as a disagreement.
///
/// These example values were `srgb=24,140,108` on the buffer line until
/// `Pass 174.5`, and that is worth a sentence rather than a silent edit:
/// `(24,140,108)` is the **pre-`Pass 165.0` defect value**, so the example
/// restated — in shipped operator-facing documentation — exactly the
/// *"the fallback beats the buffer"* premise that `Pass 174.0` measured
/// away, while a test twenty files over asserted `47,181,73` for the same
/// operand. A worked example is a claim, and a stale one is a wrong claim
/// that reads as an illustration.
///
/// Every key is present in every variant, with `-` where there is no
/// value. A line whose key set changes with the answer forces a parser to
/// branch before it can read anything, and — worse for a human — makes
/// *"this page was never composited in ink"* and *"this pixel has no ink
/// on it"* look like the same output. They are wholly different facts.
/// `source=` is what separates them, and it is second on the line so it is
/// read before the numbers it qualifies.
///
/// Three decimals on the tints: a colorant is authored as a decimal
/// fraction in a content stream and 0.001 is finer than any press or any
/// 8-bit channel can resolve, so more digits would publish `f32`
/// representation noise as if it were ink.
pub(crate) fn format_ink_probe(probe: &pdfcer_render::InkProbe) -> String {
    let source = match probe.source {
        pdfcer_render::InkProbeSource::CmykBuffer => "cmyk-buffer",
        pdfcer_render::InkProbeSource::ScreenSrgb => "screen-srgb",
        pdfcer_render::InkProbeSource::OutOfRange => "out-of-range",
        // `#[non_exhaustive]`, so a variant added upstream must still
        // print something rather than fail to compile a shell that has not
        // caught up. It prints the fact that it is unrecognised, which is
        // the honest report.
        _ => "unknown",
    };
    let ink = probe.cmyk.map_or_else(
        || "c=- m=- y=- k=-".to_owned(),
        |v| format!("c={:.3} m={:.3} y={:.3} k={:.3}", v[0], v[1], v[2], v[3]),
    );
    let alpha = probe
        .alpha
        .map_or_else(|| "-".to_owned(), |a| format!("{a:.3}"));
    let srgb = probe
        .srgb
        .map_or_else(|| "-".to_owned(), |c| format!("{},{},{}", c[0], c[1], c[2]));
    // SPOT PLANES, appended rather than interleaved, so the four process
    // fields keep their exact positions and every script that parses this
    // line by `c=`/`m=`/`y=`/`k=` keeps working. Absent entirely on a page
    // with no spot roster -- 98.6 % of a 4,023-file corpus -- so the common
    // line is byte-identical to what it has always been.
    //
    // Reported at all because a four-channel readout of a buffer whose
    // channel count is variable answers a DIFFERENT QUESTION from the one
    // its name asks: this probe once reported a trap mark and its surround
    // as identical while they rendered as visibly different colours,
    // because the whole difference lived in a plane it could not see.
    let spots = if probe.spots.is_empty() {
        String::new()
    } else {
        let inks: Vec<String> = probe
            .spots
            .iter()
            .map(|(name, tint)| format!("{}={tint:.3}", sanitize_token(name)))
            .collect();
        format!(" spots={}", inks.join(","))
    };
    format!(
        "ink-probe: x={} y={} source={source} {ink} alpha={alpha} srgb={srgb}{spots}",
        probe.x, probe.y
    )
}

/// Parse `--probe-ink X,Y` into a device-pixel coordinate.
///
/// # Why this is not `parse_region`'s cousin
///
/// A region is in **user space** — points, origin bottom-left, `f64`, and
/// negative values are ordinary. A probe is in **device space** — pixels,
/// origin top-left, integral, and a negative value is meaningless rather
/// than merely unusual. Sharing a parser between the two would mean one of
/// them accepting a coordinate system it cannot honour, which is a worse
/// outcome than two small functions.
///
/// # What is refused here, and what deliberately is not
///
/// Refused: a wrong field count, a non-integer, a negative. Each of those
/// is decidable from the string alone.
///
/// **Not** refused: a coordinate larger than the raster. The raster's size
/// is a function of `--scale`, `--region` and the page's own box, none of
/// which have been resolved at parse time — and reporting
/// `source=out-of-range` after the render is a strictly better answer than
/// a refusal, because it can say what the raster's size actually *was*.
///
/// # Errors
///
/// Returns a human-readable reason: a wrong field count, a field that is
/// not a whole number, or a negative coordinate.
pub(crate) fn parse_probe_ink(spec: &str) -> Result<(u32, u32), String> {
    let parts: Vec<&str> = spec.split(',').map(str::trim).collect();
    if parts.len() != 2 {
        return Err(format!(
            "expected 2 comma-separated whole numbers `X,Y` in DEVICE PIXELS (origin top-left), got {}",
            parts.len()
        ));
    }
    let mut v = [0u32; 2];
    for (i, (slot, text)) in v.iter_mut().zip(parts.iter()).enumerate() {
        *slot = text.parse::<u32>().map_err(|_| {
            format!(
                "field {} ({text:?}) is not a whole, non-negative number. These are DEVICE PIXELS at the rendered scale, not PDF points: a probe of a page rendered at --scale 2 wants twice the point coordinate, and its origin is the TOP-left, not the bottom-left a /MediaBox uses",
                ["X", "Y"][i]
            )
        })?;
    }
    Ok((v[0], v[1]))
}

/// Parse `--region llx,lly,urx,ury` into a user-space rectangle.
///
/// # Why the errors are this specific
///
/// A region is four numbers with no units and no labels, which is the
/// easiest kind of argument to get subtly wrong: swapped corners, a
/// width-and-height where a second corner belongs, a comma-separated list
/// pasted from somewhere with three values or five. Each of those has a
/// distinct message here, because the failure an operator cannot diagnose
/// is the one that renders *something* — a mirrored or empty rectangle
/// silently clamped to nothing looks exactly like a blank area of the
/// page.
///
/// # Errors
///
/// Returns a human-readable reason: a wrong field count, a field that is
/// not a number, a non-finite value, or a rectangle with zero or negative
/// extent in either axis.
pub(crate) fn parse_region(spec: &str) -> Result<pdfcer_core::page_tree::Rect, String> {
    let parts: Vec<&str> = spec.split(',').map(str::trim).collect();
    if parts.len() != 4 {
        return Err(format!(
            "expected 4 comma-separated numbers `llx,lly,urx,ury`, got {}",
            parts.len()
        ));
    }
    let mut v = [0.0f64; 4];
    for (i, (slot, text)) in v.iter_mut().zip(parts.iter()).enumerate() {
        *slot = text.parse::<f64>().map_err(|_| {
            format!(
                "field {} ({text:?}) is not a number",
                ["llx", "lly", "urx", "ury"][i]
            )
        })?;
        if !slot.is_finite() {
            return Err(format!(
                "field {} is not finite",
                ["llx", "lly", "urx", "ury"][i]
            ));
        }
    }
    if v[2] <= v[0] || v[3] <= v[1] {
        return Err(format!(
            "empty or inverted rectangle: urx must exceed llx and ury must exceed lly (got llx={} lly={} urx={} ury={}). Note these are two CORNERS in PDF user space, not an origin and a size",
            v[0], v[1], v[2], v[3]
        ));
    }
    Ok(pdfcer_core::page_tree::Rect {
        llx: v[0],
        lly: v[1],
        urx: v[2],
        ury: v[3],
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod region_tests {
    use super::parse_region;

    /// The happy path, and the reason the type is two CORNERS: a viewer
    /// scrolled past the left edge of a page legitimately asks for a
    /// region with negative coordinates, and a `/MediaBox` may have a
    /// negative origin to begin with.
    #[test]
    fn a_region_is_two_corners_and_may_be_negative() {
        let r = parse_region("-760,-437.5,840,562").unwrap();
        assert!((r.llx - -760.0).abs() < 1e-9);
        assert!((r.lly - -437.5).abs() < 1e-9);
        assert!((r.urx - 840.0).abs() < 1e-9);
        assert!((r.ury - 562.0).abs() < 1e-9);
        // Whitespace around the commas is a shell reality, not a mistake.
        assert!(parse_region(" 0 , 0 , 10 , 10 ").is_ok());
    }

    /// The error that matters most is the one an operator CANNOT see in
    /// the output: an origin-and-size quadruple parses as a rectangle and
    /// renders *something*, which is indistinguishable from a blank part
    /// of the page. So it is refused by name, and the message says which
    /// mistake it thinks was made.
    #[test]
    fn an_origin_and_size_is_refused_with_the_reason() {
        let err = parse_region("100,100,50,50").unwrap_err();
        assert!(err.contains("inverted"), "{err}");
        assert!(err.contains("CORNERS"), "{err}");
        assert!(parse_region("10,10,10,20").is_err(), "zero width is empty");
        assert!(parse_region("10,10,20,10").is_err(), "zero height is empty");
    }

    #[test]
    fn the_field_count_and_each_field_are_checked_by_name() {
        assert!(parse_region("1,2,3").unwrap_err().contains("got 3"));
        assert!(parse_region("1,2,3,4,5").unwrap_err().contains("got 5"));
        let err = parse_region("1,2,three,4").unwrap_err();
        assert!(err.contains("urx"), "names the field, not the index: {err}");
        assert!(parse_region("1,2,3,inf").unwrap_err().contains("finite"));
        assert!(parse_region("1,2,3,NaN").unwrap_err().contains("finite"));
    }
}
