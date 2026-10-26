//! `redact-apply`: true removal, the redaction report and the absence proof.

use super::*;
use pdfcer_core::redact::{
    self, AbsenceVerdict, AbsenceVerification, RedactError, RedactOptions, RedactionReport,
};
use pdfcer_core::writer::SaveOptions;

/// The operator's acknowledgements for `redact-apply`, one per safety
/// property so no flag stands in for another.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RedactAcks {
    /// `--acknowledge-residuals`: carriers the sweep disclosed but could not
    /// scrub.
    pub(crate) residuals: bool,
    /// `--allow-drawn-survivors`: write even though a removed string still
    /// occurs in drawn content.
    pub(crate) drawn_survivors: bool,
    /// `--acknowledge-unverified`: the absence proof could not say
    /// *verified*.
    pub(crate) unverified: bool,
}

/// `redact-apply`: TRULY REMOVE the marked content (the destructive R35
/// phase), run the absence proof over the produced bytes **before writing**,
/// and print the redaction report.
///
/// Exit codes: `EDIT_REFUSED` with **no output file** when a removed string
/// survives in drawn content (unless `drawn_survivors`) or core refuses;
/// `REDACTION_RESIDUALS` with the file written when a carrier was disclosed
/// but not scrubbed (unless `residuals`) or the proof could not verify
/// (unless `unverified`).
pub(crate) fn cmd_redact_apply(
    input: &Path,
    output: &Path,
    residual_scope: ResidualScopeArg,
    overlay_off_page: bool,
    acks: RedactAcks,
) -> u8 {
    let options = RedactOptions::with_residual_scope(residual_scope.into())
        .with_overlay_off_page(overlay_off_page);
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
    let (bytes, report) =
        match redact::apply_redactions_with(&doc, &SaveOptions::identity(), &options) {
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

    // The absence proof runs on the bytes BEFORE they reach the disk.
    let proof = redact::verify_absence(&bytes, &report.redacted_text);
    if proof.verdict() == AbsenceVerdict::Refuse && !acks.drawn_survivors {
        refuse_drawn_survivors(&proof, output);
        return exit::EDIT_REFUSED;
    }
    if let Err(err) = write_output(output, &bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    println!("redact-apply {} -> {}", input.display(), output.display());
    print_redaction_report(&report, residual_scope, bytes.len());
    print_absence(&proof);
    redact_exit(&report, &proof, acks)
}

/// The refusal message for a drawn-content survivor; nothing was written.
fn refuse_drawn_survivors(proof: &AbsenceVerification, output: &Path) {
    eprintln!(
        "pdfcer: redaction REFUSED, nothing written to {}: removed text still occurs in content \
         the document draws.",
        output.display()
    );
    for s in proof.drawn_survivors() {
        eprintln!("  survivor: {s:?}");
    }
    eprintln!(
        "  This is usually the same words drawn outside the marked area. Mark those too, or \
         re-run with --allow-drawn-survivors to write the file with them left in place."
    );
}

/// The absence proof's lines. `verified` is printed only when
/// [`AbsenceVerification::is_clean`] holds.
fn print_absence(proof: &AbsenceVerification) {
    let word = match proof.verdict() {
        AbsenceVerdict::Verified => "verified",
        AbsenceVerdict::Disclose => "NOT verified",
        AbsenceVerdict::Refuse => "NOT verified (drawn survivors allowed)",
    };
    println!(
        "  absence: {word} strings_checked={} strings_too_short={} residuals={} \
         streams_undecoded={} output_unparsed={} text_unextracted={}",
        proof.strings_checked,
        proof.strings_too_short,
        proof.residuals.len(),
        proof.streams_undecoded,
        u8::from(proof.output_unparsed),
        u8::from(proof.text_unextracted),
    );
    for r in &proof.residuals {
        println!("    residual site={} text={:?}", r.site.as_str(), r.text);
    }
    if proof.strings_too_short > 0 {
        println!(
            "    note: {} removed string(s) are under {} characters and were not checked",
            proof.strings_too_short,
            redact::MIN_VERIFIABLE_LEN
        );
    }
}

/// The exit code once the file is written: the carrier gate, then the proof
/// gate, each with its own acknowledgement.
fn redact_exit(report: &RedactionReport, proof: &AbsenceVerification, acks: RedactAcks) -> u8 {
    if report.marks_retained > 0 {
        eprintln!(
            "pdfcer: {} redaction mark(s) were RETAINED in the output, unapplied — each touches \
             an image whose samples pdfcer could not destroy (see the notes above). The output is \
             NOT fully redacted; the retained marks are still visible in it.",
            report.marks_retained
        );
    }
    if report.has_disclosed_residuals() && !acks.residuals {
        eprintln!(
            "pdfcer: the covered content WAS removed, but one or more diligence carriers could \
             not be scrubbed and were DISCLOSED above (see action=DISCLOSED_NOT_SCRUBBED). Review \
             them, then re-run with --acknowledge-residuals to exit 0."
        );
        return exit::REDACTION_RESIDUALS;
    }
    if proof.verdict() == AbsenceVerdict::Disclose && !acks.unverified {
        eprintln!(
            "pdfcer: the file was written, but the absence check could not verify it (see \
             `absence:` above). Review the residuals, then re-run with --acknowledge-unverified \
             to exit 0."
        );
        return exit::REDACTION_RESIDUALS;
    }
    exit::SUCCESS
}

/// The REDACTION REPORT — exactly what was removed and what was left.
///
/// Every counter is printed, zeroes included, because a script reads counters
/// and "the sweep found nothing" is itself the answer (R151).
fn print_redaction_report(report: &RedactionReport, scope: ResidualScopeArg, out_bytes: usize) {
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
        out_bytes,
    );
    // The Table 192 overlay-marking ladder.
    println!(
        "  overlay_text_burned={} overlay_ro_not_drawn={} overlay_transparent={} overlay_fill_not_rgb={} \
         overlay_trimmed_to_page={} overlay_off_page_skipped={}",
        report.overlay_text_burned,
        report.overlay_ro_not_drawn,
        report.overlay_transparent,
        report.overlay_fill_not_rgb,
        report.overlay_trimmed_to_page,
        report.overlay_off_page_skipped,
    );
    // Image surgery (§12.5.6.23). `marks_retained` non-zero means a /Redact
    // mark is still IN the output, unapplied; the notes name it.
    println!(
        "  images_cleared={} images_removed={} images_cloned_shared={} images_overcovered={} \
         marks_retained={} content_streams_decoupled={}",
        report.images_cleared,
        report.images_removed,
        report.images_cloned_shared,
        report.images_overcovered,
        report.marks_retained,
        report.content_streams_decoupled,
    );
    // Vector paths (§8.5): `vector_paths_intersecting` must read zero for the
    // region to be fully redacted.
    println!(
        "  vector_paths_cut={} vector_paths_dropped={} vector_clips_kept={} \
         vector_paths_intersecting={} shadings_cut={} shadings_intersecting={}",
        report.vector_paths_cut,
        report.vector_paths_dropped,
        report.vector_clips_kept,
        report.vector_paths_intersecting,
        report.shadings_cut,
        report.shadings_intersecting,
    );
    print_sweep_and_notes(report, scope);
}

/// The residual sweep's figures, the carrier table and the notes.
///
/// `matches_left` counts what pdfcer found and was told not to touch.
fn print_sweep_and_notes(report: &RedactionReport, scope: ResidualScopeArg) {
    println!(
        "  residual_sweep: scope={} entries_scrubbed={} objects_scrubbed={} \
         content_streams_blanked={} matches_left={}",
        match scope {
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
    // Separate from the exit gate: "could not scrub" and "was told not to"
    // are different facts.
    if report.has_unscrubbed_matches() {
        println!(
            "  note: {} object(s) quote the redacted text outside the marked regions and were \
             LEFT UNCHANGED under --residual-scope. Re-run with a wider scope to remove them.",
            report.residual_matches_left
        );
    }
}
