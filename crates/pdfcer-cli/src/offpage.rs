use super::*;

/// Collect the PDFs named by `paths` — files as themselves, folders one level
/// deep or recursively (`Pass 294.0`).
///
/// Sorted, so two runs over the same tree produce diffable reports.
pub(crate) fn collect_pdfs(paths: &[PathBuf], recursive: bool) -> (Vec<PathBuf>, Vec<String>) {
    let mut out = Vec::new();
    let mut problems = Vec::new();

    fn is_pdf(p: &Path) -> bool {
        p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
    }

    fn walk(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>, problems: &mut Vec<String>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(err) => {
                problems.push(format!("{}: {err}", dir.display()));
                return;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if recursive {
                    walk(&path, recursive, out, problems);
                }
            } else if is_pdf(&path) {
                out.push(path);
            }
        }
    }

    for p in paths {
        if p.is_dir() {
            walk(p, recursive, &mut out, &mut problems);
        } else if p.exists() {
            out.push(p.clone());
        } else {
            problems.push(format!("{}: not found", p.display()));
        }
    }
    out.sort();
    (out, problems)
}

/// Implement `pdfcer scan-offpage` (`Pass 294.0`).
pub(crate) fn cmd_scan_offpage(
    paths: &[PathBuf],
    recursive: bool,
    tolerance: f64,
    output: Option<&Path>,
    detail: bool,
    files_only: bool,
) -> u8 {
    let (files, problems) = collect_pdfs(paths, recursive);
    for p in &problems {
        eprintln!("pdfcer: {p}");
    }
    if files.is_empty() {
        eprintln!("pdfcer: no PDFs found in {} path(s)", paths.len());
        return exit::RUNTIME_ERROR;
    }

    let mut report = String::new();
    let mut affected_files = 0usize;
    let mut affected_pages = 0usize;
    let mut unreadable_files = 0usize;
    let mut fully = 0usize;
    let mut partial = 0usize;

    for path in &files {
        let doc = match std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|b| pdfcer_core::document::Document::from_bytes(b).map_err(|e| e.to_string()))
        {
            Ok(d) => d,
            Err(err) => {
                // A file pdfcer cannot open is REPORTED, never counted clean:
                // "nothing off-canvas" and "I could not look" are different
                // answers and must not print the same way.
                unreadable_files += 1;
                eprintln!("pdfcer: {}: {err}", path.display());
                continue;
            }
        };
        let (scans, unreadable_pages) = match pdfcer_core::offpage::scan_document(&doc, tolerance) {
            Ok(pair) => pair,
            Err(err) => {
                unreadable_files += 1;
                eprintln!("pdfcer: {}: {err}", path.display());
                continue;
            }
        };
        for (index, why) in &unreadable_pages {
            eprintln!(
                "pdfcer: {}: page {} could not be read ({why}) — not scanned",
                path.display(),
                index + 1
            );
        }

        let hits: Vec<_> = scans.iter().filter(|s| !s.is_clean()).collect();
        if hits.is_empty() {
            continue;
        }
        affected_files += 1;
        if files_only {
            report.push_str(&format!("{}\n", path.display()));
            affected_pages += hits.len();
            for h in &hits {
                fully += h.fully_off();
                partial += h.partial();
            }
            continue;
        }
        report.push_str(&format!("{}\n", path.display()));
        for h in hits {
            affected_pages += 1;
            fully += h.fully_off();
            partial += h.partial();
            report.push_str(&format!(
                "  page {} off_page={} fully_off={} partial={} page_box={:.1},{:.1},{:.1},{:.1} drawn={:.1},{:.1},{:.1},{:.1}\n",
                h.page_index + 1,
                h.objects.len(),
                h.fully_off(),
                h.partial(),
                h.page_box.llx,
                h.page_box.lly,
                h.page_box.urx,
                h.page_box.ury,
                h.drawn.min.x,
                h.drawn.min.y,
                h.drawn.max.x,
                h.drawn.max.y,
            ));
            if detail {
                for o in &h.objects {
                    let how = match o.how {
                        pdfcer_core::offpage::OffPage::Fully => "fully-off",
                        pdfcer_core::offpage::OffPage::Partial => "partial",
                        _ => "other",
                    };
                    report.push_str(&format!(
                        "      {how:<9} {:<5} bbox={:.1},{:.1},{:.1},{:.1}{}\n",
                        o.kind,
                        o.bbox.min.x,
                        o.bbox.min.y,
                        o.bbox.max.x,
                        o.bbox.max.y,
                        match &o.text {
                            Some(t) => format!("  text={t:?}"),
                            None => String::new(),
                        }
                    ));
                }
            }
        }
    }

    let summary = format!(
        "scan-offpage files={} affected_files={} affected_pages={} fully_off={} partial={} unreadable_files={} tolerance={tolerance}",
        files.len(),
        affected_files,
        affected_pages,
        fully,
        partial,
        unreadable_files,
    );

    match output {
        Some(path) => {
            let body = format!("{report}{summary}\n");
            if let Err(err) = std::fs::write(path, body) {
                eprintln!("pdfcer: {}: {err}", path.display());
                return exit::IO_ERROR;
            }
            println!("{summary}");
            println!("report written to {}", path.display());
        }
        None => {
            print!("{report}");
            println!("{summary}");
        }
    }

    // The exit code is a VERDICT ON THE WHOLE RUN, delivered here, after
    // every file has been scanned. It is not an early stop, and the operator
    // read it as one from the release notes -- which is a wording defect in
    // the notes, fixed, and a reason to say it in the help text too.
    //
    // Exit 1 when something was found, so a script can gate on it. An
    // unreadable file is a failure of a different kind and keeps its own
    // code.
    if unreadable_files > 0 && affected_files == 0 {
        return exit::RUNTIME_ERROR;
    }
    u8::from(affected_files > 0)
}

/// Drive `redact-offpage` over files and folders (`Pass 294.1`).
///
/// # Why the output policy is two flags and not one
///
/// `-o FILE` names one output and is refused for a batch, because a batch has
/// no single output to name. `--out-dir DIR` takes a batch and **preserves the
/// input tree's shape underneath it** -- `TR-0411/TR-0411.pdf` and
/// `TR-0412/TR-0411.pdf` are different drawings with the same file name, and a
/// flat output folder would silently make one of them the other. That is not a
/// hypothetical: copying this very scan's findings to a test folder hit the
/// collision on the first try.
///
/// An existing output is SKIPPED and counted, not overwritten, unless
/// `--force`. A batch over hundreds of CAD sheets takes minutes per file; the
/// useful behaviour after an interruption is to resume.
// One argument per flag, as every other command function in this file.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_redact_offpage_batch(
    paths: &[PathBuf],
    residual_scope: ResidualScopeArg,
    recursive: bool,
    output: Option<&Path>,
    out_dir: Option<&Path>,
    suffix: &str,
    force: bool,
    tolerance: f64,
    dry_run: bool,
) -> u8 {
    let (files, problems) = collect_pdfs(paths, recursive);
    for p in &problems {
        eprintln!("pdfcer: {p}");
    }
    if files.is_empty() {
        eprintln!("pdfcer: no PDFs found in {} path(s)", paths.len());
        return exit::RUNTIME_ERROR;
    }

    // The single-file shape: one input, one named output. Unchanged behaviour.
    if let Some(out) = output {
        if files.len() > 1 {
            eprintln!(
                "pdfcer: -o names ONE output but {} input file(s) were found -- use --out-dir for a batch",
                files.len()
            );
            return exit::EDIT_REFUSED;
        }
        return cmd_redact_offpage(&files[0], out, residual_scope, tolerance, dry_run);
    }

    let Some(dir) = out_dir else {
        eprintln!(
            "pdfcer: say where the output goes: -o FILE for one input, --out-dir DIR for a batch"
        );
        return exit::EDIT_REFUSED;
    };

    // The root each output's relative path is measured from: the folder the
    // operator named, so `--out-dir` mirrors what they asked for rather than
    // the drive root.
    let roots: Vec<PathBuf> = paths
        .iter()
        .filter(|p| p.is_dir())
        .map(std::path::PathBuf::from)
        .collect();
    let relative_of = |f: &Path| -> PathBuf {
        for r in &roots {
            if let Ok(rel) = f.strip_prefix(r) {
                return rel.to_path_buf();
            }
        }
        PathBuf::from(f.file_name().unwrap_or_default())
    };

    let mut cleaned = 0usize;
    let mut skipped_clean = 0usize;
    let mut skipped_exists = 0usize;
    let mut failed = 0usize;

    for f in &files {
        let rel = relative_of(f);
        let stem = rel
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output")
            .to_owned();
        let target = dir
            .join(rel.parent().unwrap_or(Path::new("")))
            .join(format!("{stem}{suffix}.pdf"));

        if target.exists() && !force && !dry_run {
            skipped_exists += 1;
            println!("skip (exists) {}", target.display());
            continue;
        }
        if !dry_run
            && let Some(parent) = target.parent()
            && let Err(err) = std::fs::create_dir_all(parent)
        {
            eprintln!("pdfcer: {}: {err}", parent.display());
            failed += 1;
            continue;
        }

        match cmd_redact_offpage(f, &target, residual_scope, tolerance, dry_run) {
            code if code == exit::SUCCESS => {
                // `cmd_redact_offpage` writes nothing when a file has no
                // off-page content, and says so. Count the two apart: "126
                // files cleaned" and "126 files looked at" are different
                // claims and the second one is not what was asked for.
                if dry_run || target.exists() {
                    cleaned += 1;
                } else {
                    skipped_clean += 1;
                }
            }
            _ => failed += 1,
        }
    }

    println!(
        "redact-offpage BATCH files={} cleaned={cleaned} already_clean={skipped_clean} skipped_existing={skipped_exists} failed={failed} dry_run={} tolerance={tolerance}",
        files.len(),
        u32::from(dry_run),
    );
    if failed > 0 {
        return exit::RUNTIME_ERROR;
    }
    exit::SUCCESS
}

/// Implement `pdfcer redact-offpage` (`Pass 294.0`).
///
/// Two acts, in one command and in this order: author a `/Redact` mark over
/// each band of off-page area, then apply them. Marking and applying are
/// separate verbs in this CLI for a good reason (a mark removes nothing), and
/// they are fused here because "delete what is off the page" is one thing an
/// operator asks for, not two.
pub(crate) fn cmd_redact_offpage(
    input: &Path,
    output: &Path,
    residual_scope: ResidualScopeArg,
    tolerance: f64,
    dry_run: bool,
) -> u8 {
    use pdfcer_core::annot_author::{Quad, RedactSpec};
    use pdfcer_core::redact::{self, RedactOptions};
    use pdfcer_core::vartext::Quadding;
    use pdfcer_core::writer::SaveOptions;

    let source = match std::fs::read(input) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::IO_ERROR;
        }
    };
    let doc = match open_document_bytes(source.clone()) {
        Ok(d) => d,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let (scans, unreadable) = match pdfcer_core::offpage::scan_document(&doc, tolerance) {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    for (index, why) in &unreadable {
        eprintln!(
            "pdfcer: {}: page {} could not be read ({why}) — its off-page content is NOT removed",
            input.display(),
            index + 1
        );
    }

    let mut marks = 0usize;
    let mut pages_marked = 0usize;
    let mut fully = 0usize;
    let mut partial = 0usize;
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    for scan in &scans {
        let bands = pdfcer_core::offpage::offpage_bands(scan, tolerance);
        if bands.is_empty() {
            continue;
        }
        pages_marked += 1;
        fully += scan.fully_off();
        partial += scan.partial();
        for band in bands {
            let spec = RedactSpec {
                quads: vec![Quad::from_rect(band)],
                // No `/IC`: Table 192 makes an absent interior colour a
                // TRANSPARENT region on apply. A black box drawn outside the
                // page would be new content in the very area being emptied.
                fill: None,
                overlay_text: None,
                quadding: Quadding::Left,
            };
            if dry_run {
                marks += 1;
                continue;
            }
            match session.add_redaction(scan.page_index, &spec) {
                Ok(_) => marks += 1,
                Err(err) => {
                    eprintln!("pdfcer: {}: {err}", input.display());
                    return exit::EDIT_REFUSED;
                }
            }
        }
    }

    if pages_marked == 0 {
        println!(
            "redact-offpage {} -> nothing to do; no content is drawn outside the page (tolerance={tolerance})",
            input.display()
        );
        return exit::SUCCESS;
    }

    if dry_run {
        println!(
            "redact-offpage {} DRY RUN pages={pages_marked} marks={marks} fully_off={fully} partial={partial} tolerance={tolerance}",
            input.display()
        );
        println!("  nothing was written. Run without --dry-run to remove it.");
        return exit::SUCCESS;
    }

    // The marks have to be IN a document before they can be applied: a
    // `/Redact` annotation is file state, and `apply_redactions` reads a
    // document rather than a session. So the marked revision is materialised
    // in memory and re-opened -- no temporary file, and the intermediate is
    // never written where anybody could mistake it for the output.
    let marked = match session.to_full_bytes(&SaveOptions::default()) {
        Ok((bytes, _)) => bytes,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::SAVE_REFUSED;
        }
    };
    let marked_doc = match open_document_bytes(marked) {
        Ok(d) => d,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let redact_options = RedactOptions::with_residual_scope(residual_scope.into());
    let (bytes, report) =
        match redact::apply_redactions_with(&marked_doc, &SaveOptions::identity(), &redact_options)
        {
            Ok(pair) => pair,
            Err(err) => {
                eprintln!("pdfcer: redaction refused: {err}");
                return exit::EDIT_REFUSED;
            }
        };
    if let Err(err) = std::fs::write(output, &bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }

    println!(
        "redact-offpage {} -> {}; pages={pages_marked} marks={marks} fully_off={fully} partial={partial} out_bytes={} tolerance={tolerance}",
        input.display(),
        output.display(),
        bytes.len(),
    );
    // The same figures `redact-apply` prints, because the surgery is the same
    // surgery -- an operator comparing the two outputs should not have to
    // translate between two vocabularies for one act.
    println!(
        "  pages_redacted={} marks_applied={} glyphs_removed={} shows_edited={} paths_cut={}",
        report.pages_redacted,
        report.marks_applied,
        report.glyphs_removed,
        report.show_operators_edited,
        report.vector_paths_cut,
    );
    println!(
        "  paths_dropped={} streams_rewritten={} images_cleared={} images_removed={} annotations_removed={}",
        report.vector_paths_dropped,
        report.content_streams_rewritten,
        report.images_cleared,
        report.images_removed,
        report.annotations_removed,
    );
    // The same sweep `redact-apply` runs, so the same figures, unconditionally.
    println!(
        "  residual_sweep: entries_scrubbed={} objects_scrubbed={} \
         content_streams_blanked={} matches_left={}",
        report.residual_sweep_entries_scrubbed,
        report.residual_sweep_objects_scrubbed,
        report.residual_content_streams_blanked,
        report.residual_matches_left,
    );
    if report.has_disclosed_residuals() {
        eprintln!(
            "pdfcer: {}: the redaction left DISCLOSED residuals -- see `redact-apply --help` for what each means",
            output.display()
        );
    }
    exit::SUCCESS
}
