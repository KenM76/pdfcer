use super::*;

/// Map a CLI permission-bit name to a [`PermissionBit`].
pub(crate) fn parse_permission_bit(
    name: &str,
) -> Result<pdfcer_core::crypto::PermissionBit, String> {
    use pdfcer_core::crypto::PermissionBit as B;
    Ok(match name.trim().to_ascii_lowercase().as_str() {
        "print" => B::Print,
        "print-high-quality" | "print-hq" => B::PrintHighQuality,
        "modify-contents" | "modify" => B::ModifyContents,
        "copy" | "extract" => B::Copy,
        "annotate" => B::Annotate,
        "fill-forms" | "fill" => B::FillForms,
        "accessibility-extract" | "accessibility" => B::AccessibilityExtract,
        "assemble" => B::Assemble,
        other => {
            return Err(format!(
                "unknown permission bit {other:?} (expected one of: print, print-high-quality, \
                 modify-contents, copy, annotate, fill-forms, accessibility-extract, assemble)"
            ));
        }
    })
}

/// Resolve `--allow`/`--deny` into the granted [`PermissionBit`] set. Default
/// (no `--allow`) is ALL granted; `--deny` removes bits.
pub(crate) fn resolve_permissions(
    allow: &[String],
    deny: &[String],
) -> Result<Vec<pdfcer_core::crypto::PermissionBit>, String> {
    use pdfcer_core::crypto::PermissionBit;
    let mut granted: Vec<PermissionBit> = if allow.is_empty() {
        PermissionBit::all().to_vec()
    } else {
        let mut v = Vec::new();
        for a in allow {
            let bit = parse_permission_bit(a)?;
            if !v.contains(&bit) {
                v.push(bit);
            }
        }
        v
    };
    for d in deny {
        let bit = parse_permission_bit(d)?;
        granted.retain(|g| *g != bit);
    }
    Ok(granted)
}

/// Arguments for [`cmd_encrypt`], shared by `encrypt` and `set-permissions`.
pub(crate) struct EncryptCliArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) output: &'a Path,
    pub(crate) user_password: Option<String>,
    pub(crate) user_password_file: Option<PathBuf>,
    pub(crate) owner_password: Option<String>,
    pub(crate) owner_password_file: Option<PathBuf>,
    pub(crate) allow: &'a [String],
    pub(crate) deny: &'a [String],
    pub(crate) encrypt_metadata: bool,
    /// `true` for `set-permissions` (re-key an already-encrypted document,
    /// owner-only); `false` for `encrypt` (a plaintext document).
    pub(crate) rekey: bool,
}

/// Implement `pdfcer encrypt` and `pdfcer set-permissions` (`Pass 5.4`).
pub(crate) fn cmd_encrypt(args: &EncryptCliArgs<'_>) -> u8 {
    use pdfcer_core::edit::{EditSession, EncryptError, EncryptionSettings};
    use pdfcer_core::writer::SaveOptions;

    let user =
        match resolve_cli_password(args.user_password.clone(), args.user_password_file.clone()) {
            Ok(p) => p.unwrap_or_default(),
            Err(msg) => {
                eprintln!("pdfcer: {msg}");
                return exit::IO_ERROR;
            }
        };
    let owner = match resolve_cli_password(
        args.owner_password.clone(),
        args.owner_password_file.clone(),
    ) {
        Ok(p) => p.unwrap_or_default(),
        Err(msg) => {
            eprintln!("pdfcer: {msg}");
            return exit::IO_ERROR;
        }
    };
    let permissions = match resolve_permissions(args.allow, args.deny) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("pdfcer: {msg}");
            return exit::RUNTIME_ERROR;
        }
    };

    let doc = match open_document(args.input) {
        Ok(d) => d,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.input.display());
            return exit_code_for_doc(&err);
        }
    };
    let mut session = EditSession::new(doc);
    let mut settings = EncryptionSettings::new(user, owner);
    settings.permissions = permissions;
    settings.encrypt_metadata = args.encrypt_metadata;

    let result = if args.rekey {
        session.set_permissions(&settings, &SaveOptions::identity())
    } else {
        session.set_encryption(&settings, &SaveOptions::identity())
    };
    let (bytes, _report) = match result {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("pdfcer: encryption refused: {err}");
            return match err {
                EncryptError::Write(_) => exit::SAVE_REFUSED,
                EncryptError::Rng(_) => exit::RUNTIME_ERROR,
                _ => exit::EDIT_REFUSED,
            };
        }
    };

    if let Err(err) = std::fs::write(args.output, &bytes) {
        eprintln!("pdfcer: {}: {err}", args.output.display());
        return exit::IO_ERROR;
    }

    let verb = if args.rekey {
        "set-permissions"
    } else {
        "encrypt"
    };
    println!(
        "{verb} {} -> {}",
        args.input.display(),
        args.output.display()
    );
    println!(
        "  scheme=AES-256/R6 permissions_granted={} encrypt_metadata={} out_bytes={}",
        settings.permissions.len(),
        settings.encrypt_metadata,
        bytes.len(),
    );
    // Disclose the SASLprep gap when a password could be affected (rule 4, W20).
    if settings.has_non_ascii_password() {
        println!("  note: {}", EncryptionSettings::SASLPREP_GAP);
    }
    // The permissions notice, verbatim (criterion 9).
    println!("  {PERMISSIONS_NOTICE}");
    exit::SUCCESS
}

/// Implement `pdfcer remove-encryption` (`Pass 5.4`). Owner-only.
pub(crate) fn cmd_remove_encryption(input: &Path, output: &Path) -> u8 {
    use pdfcer_core::edit::{EditSession, EncryptError};
    use pdfcer_core::writer::SaveOptions;

    let doc = match open_document(input) {
        Ok(d) => d,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let mut session = EditSession::new(doc);
    let (bytes, _report) = match session.remove_encryption(&SaveOptions::identity()) {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("pdfcer: remove-encryption refused: {err}");
            return match err {
                EncryptError::Write(_) => exit::SAVE_REFUSED,
                _ => exit::EDIT_REFUSED,
            };
        }
    };
    if let Err(err) = std::fs::write(output, &bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    println!(
        "remove-encryption {} -> {}",
        input.display(),
        output.display()
    );
    println!("  the output is a plaintext document that opens with no password");
    exit::SUCCESS
}

/// Auto-locate an installed Acrobat/Reader `addressbook.acrodata` across the
/// track directories a real install may use. Platform-neutral by construction:
/// `%APPDATA%` is Windows-only, so `env::var` simply returns `Err` elsewhere
/// and the list is empty (no `cfg(windows)`, so every target still compiles and
/// lints — the CLI-cross-target rule).
pub(crate) fn acrobat_trust_store_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        for track in ["DC", "2020", "2017", "11.0"] {
            out.push(
                PathBuf::from(&appdata)
                    .join("Adobe")
                    .join("Acrobat")
                    .join(track)
                    .join("Security")
                    .join("addressbook.acrodata"),
            );
        }
    }
    out
}

/// Implement `pdfcer trust-store-list` (`Pass 10.2`) — READ-ONLY.
pub(crate) fn cmd_trust_store_list(file: Option<&Path>, source: &str, identities: bool) -> u8 {
    use pdfcer_core::trust_store::{self, SourceFilter};

    let filter = match source.trim().to_ascii_lowercase().as_str() {
        "all" => SourceFilter::All,
        "aatl" => SourceFilter::Aatl,
        "eutl" => SourceFilter::Eutl,
        "adbe" => SourceFilter::Adbe,
        other => {
            eprintln!("pdfcer: unknown --source {other:?} (expected all, aatl, eutl or adbe)");
            return exit::RUNTIME_ERROR;
        }
    };

    // Resolve the store path: explicit --file, else the first auto-located one.
    let path = match file {
        Some(p) => p.to_path_buf(),
        None => match acrobat_trust_store_paths().into_iter().find(|p| p.exists()) {
            Some(p) => p,
            None => {
                eprintln!(
                    "pdfcer: no installed Acrobat/Reader trust store found under \
                     %APPDATA%\\Adobe\\Acrobat\\<track>\\Security\\addressbook.acrodata. \
                     Pass --file to read a specific one."
                );
                return exit::IO_ERROR;
            }
        },
    };

    let set = match trust_store::load_from_path(&path) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", path.display());
            return exit::RUNTIME_ERROR;
        }
    };

    // Freshness disclosure: the store is only as current as Acrobat's last
    // refresh, so name the file's own mtime (rule 4).
    let mtime = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or_else(
            || "unknown".to_owned(),
            |d| format!("{}s since epoch", d.as_secs()),
        );

    let c = set.counts();
    println!("trust-store {}", path.display());
    println!(
        "  anchors={} aatl={} eutl={} adbe={} other={} undecodable={} last_refresh={}",
        c.total, c.aatl, c.eutl, c.adbe, c.other, set.undecodable, mtime,
    );
    // The provisional-/Trust disclosure (rule 4): the bit meanings are
    // Adobe-unpublished, so we show the raw integer and never interpret it.
    println!(
        "  note: /Trust bit meanings are not published by Adobe; the raw integer is shown, \
         not an interpreted privilege set. /Source is the authoritative provenance."
    );

    if identities {
        for a in set.filter(filter) {
            let src = if a.sources.is_empty() {
                "(none)".to_owned()
            } else {
                a.sources.join(",")
            };
            println!(
                "  [{src}] trust={} serial={} subject={}",
                a.trust_bits, a.serial_hex, a.subject
            );
        }
    } else {
        let shown = set.filter(filter).len();
        println!("  {shown} anchor(s) match --source {source} (use --identities to list them)");
    }
    exit::SUCCESS
}

/// `redact-apply`: TRULY REMOVE the marked content (the destructive R35
/// phase). Prints the redaction report; the refusal-acknowledgement gate
/// (ui-spec §4.4) forces a non-zero exit on un-scrubbed carrier residuals
/// unless `--acknowledge-residuals` is passed.
pub(crate) fn cmd_redact_apply(
    input: &Path,
    output: &Path,
    residual_scope: ResidualScopeArg,
    acknowledge_residuals: bool,
) -> u8 {
    use pdfcer_core::redact::{self, RedactError, RedactOptions};
    use pdfcer_core::writer::SaveOptions;

    let source = match std::fs::read(input) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::IO_ERROR;
        }
    };
    let doc = match open_document_bytes(source) {
        Ok(d) => d,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };

    let redact_options = RedactOptions::with_residual_scope(residual_scope.into());
    let (bytes, report) =
        match redact::apply_redactions_with(&doc, &SaveOptions::identity(), &redact_options) {
            Ok(pair) => pair,
            Err(err) => {
                eprintln!("pdfcer: redaction refused: {err}");
                return match err {
                    RedactError::ImageUndestroyable { .. }
                    | RedactError::NothingToApply
                    | RedactError::Encrypted => exit::EDIT_REFUSED,
                    RedactError::Write(_) => exit::SAVE_REFUSED,
                    _ => exit::RUNTIME_ERROR,
                };
            }
        };

    if let Err(err) = std::fs::write(output, &bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }

    // The REDACTION REPORT — exactly what was removed and what was left.
    println!("redact-apply {} -> {}", input.display(), output.display());
    println!(
        "  pages_redacted={} marks_applied={} glyphs_removed={} show_operators_edited={}",
        report.pages_redacted,
        report.marks_applied,
        report.glyphs_removed,
        report.show_operators_edited,
    );
    println!(
        "  content_streams_rewritten={} annotations_removed={} info_strings_scrubbed={}",
        report.content_streams_rewritten, report.annotations_removed, report.info_strings_scrubbed,
    );
    println!(
        "  containers_decomposed={} objects_promoted={} estimated_width_fonts={} out_bytes={}",
        report.containers_decomposed,
        report.objects_promoted,
        report.estimated_width_fonts,
        bytes.len(),
    );
    // The Table 192 overlay-marking ladder. Printed as counters as well as
    // appearing in `notes` below, because a counter is what a script can
    // read: `Pass 84.0` exists because twelve colour counters were computed,
    // merged and unit-tested while no shell ever printed one (R151).
    println!(
        "  overlay_text_burned={} overlay_ro_not_drawn={} overlay_transparent={}",
        report.overlay_text_burned, report.overlay_ro_not_drawn, report.overlay_transparent,
    );
    // The image surgery (§12.5.6.23 "that portion of the image data shall be
    // destroyed"). `marks_retained` is the one a batch caller must read: a
    // non-zero value means a /Redact mark is still IN the output, unapplied,
    // and the notes below name it and say why.
    println!(
        "  images_cleared={} images_removed={} images_cloned_shared={} images_overcovered={} \
         marks_retained={}",
        report.images_cleared,
        report.images_removed,
        report.images_cloned_shared,
        report.images_overcovered,
        report.marks_retained,
    );
    // Vector paths (§8.5): `vector_paths_intersecting` is the residual — a
    // path that crossed a region and could NOT be cut; it must read zero
    // for the region to be fully redacted.
    println!(
        "  vector_paths_cut={} vector_paths_dropped={} vector_clips_kept={} \
         vector_paths_intersecting={} shadings_intersecting={}",
        report.vector_paths_cut,
        report.vector_paths_dropped,
        report.vector_clips_kept,
        report.vector_paths_intersecting,
        report.shadings_intersecting,
    );
    // THE RESIDUAL SWEEP'S OWN FIGURES. Computed since `Pass 284.0` and
    // never printed by any shell until `Pass 310.1` -- exactly the R151 shape
    // the comment above warns about, in the same function. Printed
    // UNCONDITIONALLY, including the zeroes: "the sweep found nothing
    // elsewhere" is the sentence an operator actually wants after a redaction,
    // and a line that appears only on a non-zero count cannot say it.
    //
    // `matches_left` is the one to read after narrowing the scope: it counts
    // what pdfcer FOUND and was TOLD NOT TO TOUCH. The notes below name the
    // objects.
    println!(
        "  residual_sweep: scope={} entries_scrubbed={} objects_scrubbed={} \
         content_streams_blanked={} matches_left={}",
        match residual_scope {
            ResidualScopeArg::MarkedOnly => "marked-only",
            ResidualScopeArg::HiddenCarriers => "hidden-carriers",
            ResidualScopeArg::WholeDocument => "whole-document",
        },
        report.residual_sweep_entries_scrubbed,
        report.residual_sweep_objects_scrubbed,
        report.residual_content_streams_blanked,
        report.residual_matches_left,
    );
    println!("  carriers (diligence sweep, ISO 32000-1 §12.5.6.23):");
    for c in &report.carriers {
        println!(
            "    {:<24} present={} action={}",
            c.carrier,
            u32::from(c.present),
            c.action.as_str()
        );
    }
    if !report.notes.is_empty() {
        println!("  notes:");
        for note in &report.notes {
            println!("    - {note}");
        }
    }

    // The refusal-acknowledgement gate.
    if report.marks_retained > 0 {
        eprintln!(
            "pdfcer: {} redaction mark(s) were RETAINED in the output, unapplied — each touches \
             an image whose samples pdfcer could not destroy (see the notes above). The output is \
             NOT fully redacted; the retained marks are still visible in it.",
            report.marks_retained
        );
    }
    // ⚠️ SEPARATE FROM THE EXIT GATE BELOW, DELIBERATELY. "pdfcer could not
    // scrub this" and "pdfcer was told not to" are different facts, and
    // folding the second into the first would make every ordinary redaction
    // of a phrase that also appears elsewhere exit non-zero.
    if report.has_unscrubbed_matches() {
        println!(
            "  note: {} object(s) quote the redacted text outside the marked regions and were \
             LEFT UNCHANGED under --residual-scope. Re-run with a wider scope to remove them.",
            report.residual_matches_left
        );
    }
    if report.has_disclosed_residuals() && !acknowledge_residuals {
        eprintln!(
            "pdfcer: the covered content WAS removed, but one or more diligence carriers could \
             not be scrubbed and were DISCLOSED above (see action=DISCLOSED_NOT_SCRUBBED). Review \
             them, then re-run with --acknowledge-residuals to exit 0."
        );
        return exit::REDACTION_RESIDUALS;
    }
    exit::SUCCESS
}

/// `list-redactions`: report the `/Redact` marks awaiting apply, from the
/// document's own annotations (never a session counter).
pub(crate) fn cmd_list_redactions(input: &Path) -> u8 {
    let source = match std::fs::read(input) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::IO_ERROR;
        }
    };
    let doc = match open_document_bytes(source) {
        Ok(d) => d,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let count = pdfcer_core::redact::count_redaction_marks(&doc);
    println!(
        "list-redactions {}: {count} unapplied /Redact mark(s)",
        input.display()
    );
    if count > 0 {
        eprintln!(
            "pdfcer: WARNING — this document carries {count} UNAPPLIED redaction mark(s); its \
             content is NOT yet redacted. Run `redact-apply` before sharing it."
        );
    }
    exit::SUCCESS
}
