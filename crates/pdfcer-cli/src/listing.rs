use super::*;

/// `verify-signatures`: the integrity + coverage verdicts, one block per
/// signature field, with trust named as not checked on every one.
pub(crate) fn cmd_verify_signatures(input: &Path, trust_from_acrobat: bool) -> u8 {
    use pdfcer_core::signature::{self, Integrity, Trust};
    let bytes = match std::fs::read(input) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::IO_ERROR;
        }
    };
    let doc = match open_document_bytes(bytes.clone()) {
        Ok(d) => d,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    // Trust is opt-in and AT THE OPERATOR'S RISK (Pass 10.3/10.4): read the
    // installed Acrobat store either when --trust-from-acrobat is passed OR when
    // the persistent `acrobat_trust_store = at_own_risk` setting is on. Absent
    // both, trust stays NotChecked.
    let (settings, _settings_report) =
        pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
    let setting_on =
        settings.acrobat_trust_store == pdfcer_core::settings::AcrobatTrustStore::AtOwnRisk;
    if setting_on && !trust_from_acrobat {
        println!("trust: acrobat_trust_store = at_own_risk (persistent setting); evaluating trust");
    }
    let anchors = if trust_from_acrobat || setting_on {
        match acrobat_trust_store_paths().into_iter().find(|p| p.exists()) {
            Some(store) => match pdfcer_core::trust_store::load_from_path(&store) {
                Ok(set) => {
                    let c = set.counts();
                    println!(
                        "trust: using {} anchors from {} (aatl={} eutl={} adbe={})",
                        c.total,
                        store.display(),
                        c.aatl,
                        c.eutl,
                        c.adbe,
                    );
                    println!(
                        "  ⚠ AT YOUR OWN RISK: read from Adobe's own downloaded file; whether relying on it fits the Adobe Reader licence is your call. A 'trusted' result checks the chain, RFC 5280 CA/key-usage constraints, and validity dates at the signing time -- but NOT revocation (CRL/OCSP), which needs the network pdfcer-core never uses."
                    );
                    Some(set)
                }
                Err(err) => {
                    eprintln!(
                        "pdfcer: could not read the Acrobat trust store {}: {err}; trust not checked",
                        store.display()
                    );
                    None
                }
            },
            None => {
                eprintln!(
                    "pdfcer: no installed Acrobat/Reader trust store found; trust not checked"
                );
                None
            }
        }
    } else {
        None
    };
    let verdicts = signature::verify_all_with_trust(&doc.view(), &bytes, anchors.as_ref());
    if verdicts.is_empty() {
        println!(
            "verify-signatures {}: 0 signature field(s)",
            input.display()
        );
        return exit::SUCCESS;
    }
    let mut failed = 0usize;
    let mut unverifiable = 0usize;
    for (i, v) in verdicts.iter().enumerate() {
        let (integrity, detail) = match &v.integrity {
            Integrity::Verified {
                digest_algorithm,
                signature_algorithm,
            } => (
                "verified",
                format!("{signature_algorithm} with {digest_algorithm}"),
            ),
            Integrity::DigestMismatch => {
                failed += 1;
                (
                    "FAILED",
                    "the bytes under the signature were ALTERED after signing".to_string(),
                )
            }
            Integrity::SignatureInvalid => {
                failed += 1;
                (
                    "FAILED",
                    "the signature value does not verify with the signer's certificate".to_string(),
                )
            }
            Integrity::Unverifiable { reason } => {
                unverifiable += 1;
                ("unverifiable", reason.clone())
            }
            // `#[non_exhaustive]`: a variant this shell does not know is
            // reported as unverifiable, never as verified.
            other => {
                unverifiable += 1;
                (
                    "unverifiable",
                    format!("a verdict this build of the CLI does not know: {other:?}"),
                )
            }
        };
        let coverage = if !v.coverage.ranges_well_formed {
            "MALFORMED_RANGE".to_string()
        } else if v.coverage.covers_to_eof() {
            "whole file".to_string()
        } else {
            format!(
                "{} byte(s) after the signed range were appended after signing",
                v.coverage.uncovered_tail
            )
        };
        let trust = match &v.trust {
            Trust::NotChecked => {
                "NOT CHECKED (no trust store, no chain, no revocation, no clock)".to_owned()
            }
            Trust::Trusted {
                anchor_subject,
                source,
                validity_checked,
            } => format!(
                "TRUSTED via {} [{}] (chain + CA/key-usage constraints{}; revocation NOT checked)",
                anchor_subject,
                source.join(","),
                if *validity_checked {
                    " + validity dates"
                } else {
                    ", but validity dates NOT checked (no signing-time clock)"
                },
            ),
            Trust::Untrusted { reason } => format!("UNTRUSTED: {reason}"),
            Trust::SignerUnknown => {
                "SIGNER UNKNOWN (the signer certificate could not be parsed)".to_owned()
            }
            other => {
                eprintln!("pdfcer: unknown trust verdict {other:?}; reported as not checked");
                "NOT CHECKED (unknown verdict)".to_owned()
            }
        };
        println!(
            "signature {} field={:?} subfilter={}",
            i + 1,
            v.field_name.as_deref().unwrap_or("-"),
            v.sub_filter.as_deref().unwrap_or("-"),
        );
        println!("  integrity: {integrity} -- {detail}");
        println!("  coverage: {coverage}");
        println!("  trust: {trust}");
        println!(
            "  claims: signer={:?} issuer={:?} valid={}..{} signing_time={} name={:?} date={:?} reason={:?}",
            v.signer_subject.as_deref().unwrap_or("-"),
            v.signer_issuer.as_deref().unwrap_or("-"),
            v.cert_not_before.as_deref().unwrap_or("-"),
            v.cert_not_after.as_deref().unwrap_or("-"),
            v.signing_time.as_deref().unwrap_or("-"),
            v.name.as_deref().unwrap_or("-"),
            v.date.as_deref().unwrap_or("-"),
            v.reason.as_deref().unwrap_or("-"),
        );
        if let Some(p) = v.certification {
            println!(
                "  certification: DocMDP P={p} ({})",
                pdfcer_core::sign::apply::MdpPermission::from_p(p)
                    .map_or("unknown level", |m| m.meaning())
            );
        }
        for n in &v.notes {
            println!("  note: {n}");
        }
    }
    println!(
        "verify-signatures {}: {} signature(s), {} verified, {} failed, {} unverifiable (trust is per-signature above; pass --trust-from-acrobat to check it)",
        input.display(),
        verdicts.len(),
        verdicts.len() - failed - unverifiable,
        failed,
        unverifiable,
    );
    if failed > 0 {
        eprintln!(
            "pdfcer: {failed} signature(s) FAILED integrity -- the signed bytes were altered or the signature is not genuine; do not treat this document as signed"
        );
        return exit::SIGNATURE_FAILED;
    }
    if unverifiable > 0 {
        eprintln!(
            "pdfcer: {unverifiable} signature(s) could not be verified (reasons above) -- that is 'pdfcer cannot say', not 'tampered'"
        );
        return exit::SIGNATURE_UNVERIFIABLE;
    }
    exit::SUCCESS
}

/// `list-signatures` — what each signature COVERS, not whether it is valid.
///
/// # The caveat is on the output, not in the help text
///
/// This is the one command in the CLI where a reader is most likely to
/// take away more than was said. "list-signatures" on a signed document,
/// printing offsets and byte counts, looks exactly like a verification
/// report — and pdfcer performs no cryptography at all.
///
/// So every run ends with a line saying so. Not a `--verbose` extra, not
/// the man page: the summary line itself, on every invocation, because
/// the operator who most needs it is the one who did not read the docs.
///
/// # What the numbers mean
///
/// `covered` is how many bytes the digest spans. `tail` is how many lie
/// PAST the end of everything it covers — the number that matters, and
/// the shape an incremental save takes when a revision is appended after
/// signing. A non-zero tail does not mean the signature is broken; it
/// means it protects less than the whole file.
///
/// §12.8.1 makes whole-file coverage a `should`, not a `shall`, so a
/// short range is reported and never called malformed. Overlapping
/// ranges violate Table 252's "exact byte range" and ARE reported as
/// malformed. The two are deliberately distinguishable in the output.
pub(crate) fn cmd_list_signatures(input: &Path) -> u8 {
    // The file's real length on disk. `/ByteRange` is a claim about
    // BYTES, and only the bytes can check it — the object model cannot
    // check a claim about the file against itself.
    let file_len = match std::fs::metadata(input) {
        Ok(m) => m.len(),
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::IO_ERROR;
        }
    };
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let session = pdfcer_core::edit::EditSession::new(doc);
    let graph = session.graph();

    let census = pdfcer_core::signature::census(&graph);
    let coverage = pdfcer_core::signature::byte_range_coverage(&graph, file_len);

    for c in &coverage {
        let ranges: Vec<String> = c.ranges.iter().map(|(o, l)| format!("{o}+{l}")).collect();
        let mut flags: Vec<&str> = Vec::new();
        if !c.ranges_well_formed {
            flags.push("MALFORMED_RANGE");
        }
        if c.uncovered_tail > 0 {
            // Named as what it is, not as an error. A short range is
            // conforming (§12.8.1's `should`).
            flags.push("does-not-cover-whole-file");
        }
        if c.pair_count == 1 {
            // One pair means /Contents sits inside its own digest, which
            // cannot verify — a different and worse problem than a short
            // range.
            flags.push("SINGLE_RANGE_CANNOT_VERIFY");
        }
        println!(
            "signature field={:?} covered={} of {} tail={} pairs={} ranges=[{}]{}",
            c.field_name.as_deref().unwrap_or("-"),
            c.covered,
            c.file_len,
            c.uncovered_tail,
            c.pair_count,
            ranges.join(" "),
            if flags.is_empty() {
                String::new()
            } else {
                format!(" {}", flags.join(" "))
            },
        );
    }

    // TWO warnings, and they must not be conflated. The first draft
    // emitted the "this is permitted, the document is not malformed"
    // reassurance for a document whose ranges OVERLAP — so one run
    // printed `MALFORMED_RANGE` on the row and a line saying nothing was
    // malformed underneath it. Found by reading the output across all
    // three fixtures rather than by any test (R174).
    let malformed = coverage.iter().any(|c| !c.ranges_well_formed);
    if malformed {
        eprintln!(
            "pdfcer: WARNING — at least one signature's /ByteRange is MALFORMED: its \
             ranges overlap or run backwards, which Table 252's \"exact byte range\" does \
             not permit. The numbers above are what the file DECLARES; a reader that \
             rejects the array will compute something else, or nothing at all."
        );
    }
    // Only reassure about conformance when there is nothing to be
    // unreassured about.
    if !malformed && coverage.iter().any(|c| c.uncovered_tail > 0) {
        eprintln!(
            "pdfcer: WARNING — at least one signature does not cover the whole file. \
             Content exists beyond what it protects. This is permitted by ISO 32000-1 \
             §12.8.1 (whole-file coverage is a \"should\"), so the document is not \
             malformed — but the signature guarantees less than its presence suggests."
        );
    }

    println!(
        "list-signatures {} signatures={} certifications={} with_byte_range={} \
         (COVERAGE ONLY — this says what each signature would protect, never \
         whether it is valid; run verify-signatures for integrity)",
        input.display(),
        census.signatures,
        census.certifications,
        coverage.len(),
    );
    exit::SUCCESS
}

/// `list-layers` — the document's optional-content groups.
///
/// # Why the default-visibility column is the interesting one
///
/// A layer's name says what it is; `visible=` says whether a reader
/// showing this document with no interaction would draw it. Those come
/// apart constantly — a "Confidential" watermark layer that is OFF by
/// default is a very different document from one where it is ON, and the
/// name alone cannot tell them apart.
///
/// The value comes from `annot.rs`'s `optional_content_default_off`, the
/// same resolver the renderer uses to decide whether an annotation is
/// drawn. Sharing it is the point: a listing that said "on" about
/// content the renderer hides would be worse than no listing.
///
/// # Read-only, and layers are not editable here
///
/// Toggling a layer in a viewer is **session-scoped with zero
/// file-format footprint** unless the operator explicitly saves
/// (`Acrobat_Features/layers__ocg_visibility_and_defaults.md`). pdfcer has
/// no save path for it, so there is nothing to offer — and offering a
/// toggle that silently did not persist would be worse than not offering
/// one (R83).
pub(crate) fn cmd_list_layers(input: &Path) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let session = pdfcer_core::edit::EditSession::new(doc);
    let read = pdfcer_core::layers::read_layers(&session.graph());

    for l in &read.layers {
        // An undeclared name is reported as `-`, never invented. Table 98
        // marks `/Name` Required, so its absence is a real malformation,
        // and a synthesised "Layer 3" would hide it behind something that
        // looks like data from the file.
        let name = if l.name_declared {
            format!("{:?}", l.name)
        } else {
            "-".to_owned()
        };
        // Only the flags that are TRUE, and only where true is the
        // interesting case. `in_order=false` matters (the layer will not
        // appear in a conforming panel); `in_order=true` is the norm and
        // says nothing.
        let mut flags: Vec<&str> = Vec::new();
        if l.locked {
            flags.push("locked");
        }
        if !l.in_default_config {
            flags.push("UNREGISTERED");
        }
        if !l.in_order {
            flags.push("not-in-order");
        }
        if !l.name_exact {
            flags.push("name-inexact");
        }
        // §8.11.2.3: a group whose `/Intent` excludes `View` does not
        // participate in visibility, so `visible=1` on it is not a
        // statement about the document's `/OFF` array — it is a
        // statement that the array does not reach this group.
        //
        // Printed only when it is NOT `View`, like every other flag
        // here: the common case says nothing, and a token on every line
        // is a token nobody reads. Without it, a `Design` layer named in
        // `/OFF` prints `visible=1` with no way to tell intent
        // filtering from a pdfcer defect.
        if !l.intent_view {
            flags.push("intent-not-view");
        }
        let rb = match l.radio_group {
            Some(g) => format!(" radio_group={g}"),
            None => String::new(),
        };
        println!(
            "layer name={name} visible={}{rb}{}{}",
            u32::from(l.visible_by_default),
            if flags.is_empty() {
                String::new()
            } else {
                format!(" {}", flags.join(" "))
            },
            // Inlined rather than a nested `format!`: clippy's
            // `format_in_format_args` is right that the inner allocation
            // is pointless when the outer macro can format it directly.
            format_args!(" via={:?}", l.discovered_via),
        );
    }

    // Only the diagnostics that fired — the struct has seventeen fields
    // and on a healthy document every one is quiet. Burying the two that
    // matter in fifteen `false`s is how a real warning gets skimmed past;
    // that lesson cost an outline listing earlier today.
    let d = &read.diagnostics;
    let mut notes: Vec<String> = Vec::new();
    for (on, name) in [
        (d.no_optional_content, "no_optional_content"),
        (d.missing_default_config, "missing_default_config"),
        (d.missing_registry, "missing_registry"),
        (d.order_node_truncation, "order_node_truncation"),
        (d.base_state_off_in_default, "base_state_off_in_default"),
        (d.base_state_unrecognised, "base_state_unrecognised"),
        (
            d.base_state_off_with_unregistered,
            "BASE_STATE_OFF_WITH_UNREGISTERED",
        ),
        (d.layer_truncation, "layer_truncation"),
        (d.resource_scan_truncated, "resource_scan_truncated"),
        (d.page_scan_failed, "page_scan_failed"),
    ] {
        if on {
            notes.push(name.to_owned());
        }
    }
    for (c, name) in [
        (d.unregistered_groups, "unregistered_groups"),
        // Not a fault — the file is fine and its state simply moves.
        // Reported because `visible=` on the rows above is the
        // /D-initial answer, and for these groups a viewer's answer
        // depends on magnification (§8.11.4.5).
        (d.auto_managed_groups, "auto_managed_groups"),
        // Decision 038: the file says two things about these groups and
        // pdfcer resolved it. The count is emitted here; the sentence
        // naming the RESOLUTION goes to stderr below, because an
        // operator who sees only a count cannot predict what they are
        // looking at.
        (d.contradictory_on_off_groups, "contradictory_on_off_groups"),
        (d.groups_without_name, "groups_without_name"),
        (d.names_inexact, "names_inexact"),
        (d.direct_group_dicts, "direct_group_dicts"),
        (d.dangling_group_references, "dangling_group_references"),
        (d.order_depth_truncations, "order_depth_truncations"),
        (d.order_cycles, "order_cycles"),
        (d.malformed_group_elements, "malformed_group_elements"),
        (d.overlapping_radio_groups, "overlapping_radio_groups"),
    ] {
        if c > 0 {
            notes.push(format!("{name}={c}"));
        }
    }
    let warnings = if notes.is_empty() {
        "clean".to_owned()
    } else {
        notes.join(" ")
    };
    // The sentence that makes the count actionable. A bare
    // `contradictory_on_off_groups=2` says a file is self-contradictory
    // and leaves the operator unable to predict which state pdfcer chose;
    // naming the rule lets them work it out for any group (decision 038).
    if read.diagnostics.contradictory_on_off_groups > 0 {
        eprintln!(
            "pdfcer: {} group(s) are listed in BOTH /D /ON and /D /OFF. Resolved per ISO 32000-1 §8.11.4.5 b): the array OPPOSITE /BaseState decides, so with the usual /BaseState ON they are OFF. The document is not malformed — nothing forbids a writer from listing a group twice.",
            read.diagnostics.contradictory_on_off_groups
        );
    }
    if read.diagnostics.auto_managed_groups > 0 {
        eprintln!(
            "pdfcer: {} group(s) have their state managed automatically by /AS usage application dictionaries (ISO 32000-1 §8.11.4.4). The visible= column above is the state the document OPENS in; a viewer re-computes it from the current magnification, so what render-page draws at a given --scale may differ. Use --print-state on render-page for the state a printing or aggregating application uses.",
            read.diagnostics.auto_managed_groups
        );
    }
    if read.diagnostics.base_state_unrecognised {
        eprintln!(
            "pdfcer: /D /BaseState is a name other than ON or OFF. Table 101 requires the default configuration's /BaseState to be ON, so this file is non-conforming; pdfcer recovers by treating it as ON, which is both the stated default and the only value /D was allowed to carry."
        );
    }
    let config = read.config_name.as_deref().unwrap_or("-");
    println!(
        "list-layers {} layers={} config={config:?} radio_groups={} {warnings}",
        input.display(),
        read.layers.len(),
        read.radio_groups.len(),
    );
    exit::SUCCESS
}

/// Render one font's `fsType` state as a single stable token.
///
/// The four states must never collapse into each other, and in particular
/// none of them may look like `0`.
///
/// `fsType == 0` genuinely **means** Installable — the most permissive value
/// the field can express — so "we could not read it" and "this format has no
/// such field" have to be visibly different from it and from one another. A
/// report that printed a blank, a dash, or a zero for all three would be
/// asserting the broadest embedding right there is on the strength of bytes
/// nobody read (`PDF_Spec/fonts/font__opentype_os2_fstype.md` N1).
///
/// The raw value is printed alongside the word so the reading can be checked
/// against the specification's own table without re-deriving it.
pub(crate) fn format_fs_type(fs: &pdfcer_core::fontinfo::FsType) -> String {
    use pdfcer_core::fontinfo::{FsType, FsTypeError};
    match fs {
        FsType::NotApplicable => "n/a-no-field".to_owned(),
        FsType::ProgramNotDecoded => "unknown-not-decoded".to_owned(),
        // The CAUSE, not just "unknown". A sweep of 3,901 corpus documents
        // found 998 of 1,560 embedded programs reading unknown here, and one
        // token could not say whether that was a subsetter stripping `OS/2`
        // (the common, benign case -- the tool that made the subset simply
        // did not carry the table forward) or a damaged font. Those are
        // different facts about the file, and an operator triaging a corpus
        // needs to bucket them apart.
        FsType::Unreadable(why) => match why {
            FsTypeError::NotSfnt => "unknown-not-sfnt",
            FsTypeError::Collection => "unknown-collection",
            FsTypeError::BadTableDirectory => "unknown-bad-directory",
            FsTypeError::NoOs2Table => "unknown-no-os2",
            FsTypeError::Os2Truncated => "unknown-os2-truncated",
            _ => "unknown-unrecognised-cause",
        }
        .to_owned(),
        // `FsType` is `#[non_exhaustive]`, so a future state compiles here
        // rather than breaking the CLI — but it must not silently render as
        // one of the existing ones, least of all as a permission. An
        // unrecognised state prints as unrecognised.
        FsType::Known(bits) => {
            let mut s = format!("{}/0x{:04X}", bits.permission.label(), bits.raw);
            if bits.no_subsetting {
                s.push_str("+nosubset");
            }
            if bits.bitmap_only {
                s.push_str("+bitmaponly");
            }
            if bits.version_gated_bits_ignored {
                s.push_str("+v0v1-bits-ignored");
            }
            if bits.reserved_bit0 {
                s.push_str("+reserved-bit0");
            }
            s
        }
        _ => "unknown-unrecognised-state".to_owned(),
    }
}

/// `list-fonts` — the document's fonts, what they cost, and what could be
/// removed.
///
/// # Why the byte size is here and nowhere else
///
/// Acrobat exposes a per-font byte size **nowhere**: Document Properties →
/// Fonts gives type, encoding and embedded status with no size at all, and
/// Audit Space Usage gives one aggregate "Fonts" bucket for the whole
/// document with no per-font attribution
/// (`Acrobat_Features/optimize__font_reporting.md`, recorded as a GAP with no
/// source found either way). An operator asking "which font is costing me the
/// most" has to infer it there by toggling fonts through the Optimizer one at
/// a time and diffing output sizes.
///
/// pdfcer computes it directly from data already parsed. This is a deliberate
/// exceed rather than a parity target — there is no Acrobat behaviour to
/// match, only a gap it leaves open.
///
/// The number is the program's **stored** size: the bytes it occupies in the
/// file, which is what removing it recovers. The decoded size is printed
/// beside it because it answers the different question of how large the font
/// actually is.
///
/// # Why the verdict is a word and not a flag
///
/// Acrobat refuses to unembed a font whose text is glyph-index-keyed, and
/// refuses **silently** — the font simply does not appear in its unembed
/// list, with no reason shown anywhere (sourced to Dov Isaacs, former Adobe
/// Principal Scientist, in `optimize__font_unembedding.md`; independently
/// corroborated by a user whose largest font was absent from the list with no
/// explanation). A shorter list is not actionable. "This font's text is
/// stored as glyph indices into this exact program" is.
///
/// So every font appears, every verdict is named, and the reasons present in
/// the document go to stderr whether or not `--reasons` was passed. That is
/// project rule 4 applied to a refusal: the inference pdfcer made is visible
/// before anyone acts on it.
///
/// # Why coverage is on the summary line
///
/// A font inventory that quietly misses a surface and prints a confident list
/// is this project's most-repeated defect (R186). The summary states which
/// surfaces were walked **and which were not**, so the listing carries the
/// shape of its own evidence. Acrobat's coverage here is an unconfirmed GAP,
/// so pdfcer states its own scope rather than assuming parity with a behaviour
/// nobody has measured.
pub(crate) fn cmd_list_fonts(input: &Path, reasons: bool, by_size: bool) -> u8 {
    use pdfcer_core::fontinfo::{self, Program, Removability};

    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let inv = fontinfo::inventory(&doc.view());

    // Borrowed, so the default order stays first-discovery — stable across
    // runs and diff-friendly — and `--by-size` is a view over it rather than
    // a different inventory.
    let mut rows: Vec<&fontinfo::FontRecord> = inv.fonts.iter().collect();
    if by_size {
        // Descending by stored bytes, ties broken by discovery order, which
        // `sort_by_key` preserves (it is stable).
        rows.sort_by_key(|f| std::cmp::Reverse(f.stored_bytes()));
    }

    for f in &rows {
        let name = f.base_font.as_deref().unwrap_or("-");
        // Printed only when it differs, because on the ~13% of embedded
        // fonts that are not subsets it would repeat the name field exactly
        // — and a token that is always present and usually redundant is a
        // token nobody reads (the lesson `list-layers` records).
        let family = match f.family_name() {
            Some(fam) if fam != name => format!(" family={fam:?}"),
            _ => String::new(),
        };
        let ty = match &f.descendant_subtype {
            Some(d) => format!("{}/{}", f.subtype.label(), d.label()),
            None => f.subtype.label().to_owned(),
        };
        let (embedded, bytes, decoded, fstype) = match &f.program {
            Program::NotEmbedded => (
                "no".to_owned(),
                "0".to_owned(),
                "-".to_owned(),
                "n/a-not-embedded".to_owned(),
            ),
            // "Declared but unreadable" is not "not embedded": the first is
            // damage, the second is a document relying on substitution.
            Program::Unreadable { key, .. } => (
                format!("{}!unreadable", key.label()),
                "0".to_owned(),
                "-".to_owned(),
                "n/a-program-unreadable".to_owned(),
            ),
            Program::Embedded(p) => (
                match &p.subtype {
                    Some(s) => format!("{}/{s}", p.key.label()),
                    None => p.key.label().to_owned(),
                },
                p.stored_bytes.to_string(),
                p.decoded_bytes
                    .map_or_else(|| "-".to_owned(), |n| n.to_string()),
                format_fs_type(&p.fs_type),
            ),
            // `Program` is `#[non_exhaustive]`. A state this build does not
            // know must not be rendered as "no" — that would report a font
            // as unembedded on the strength of not recognising it.
            _ => (
                "unrecognised".to_owned(),
                "-".to_owned(),
                "-".to_owned(),
                "unknown-unrecognised-state".to_owned(),
            ),
        };
        let surfaces = f
            .surfaces
            .iter()
            .map(|s| s.token())
            .collect::<Vec<_>>()
            .join(",");
        let names = if f.resource_names.is_empty() {
            "-".to_owned()
        } else {
            let mut joined = f.resource_names.join(",");
            if f.resource_names_truncated {
                joined.push_str(",…");
            }
            joined
        };
        let obj =
            f.id.map_or_else(|| "direct".to_owned(), |id| format!("{}", id.num));
        println!(
            "font name={name:?}{family} type={ty:?} encoding={:?} embedded={embedded} \
bytes={bytes} decoded={decoded} fstype={fstype} tounicode={} std14={} verdict={} \
pages={} surfaces={surfaces} resources={names} obj={obj}",
            f.encoding.label(),
            u32::from(f.has_to_unicode),
            u32::from(f.standard_14),
            f.removability.token(),
            pdfcer_core::fontinfo::format_page_ranges(&f.pages),
        );
        if reasons {
            println!("  reason: {}", f.removability.reason());
        }
    }

    // The document total, computed from the same per-font numbers the rows
    // above show — so the total and the listing cannot disagree. Acrobat's
    // equivalent (Audit Space Usage's aggregate "Fonts" bucket) is
    // Pro-exclusive; this ships in every build.
    let counts = inv.verdict_counts();
    let verdicts = if counts.is_empty() {
        "-".to_owned()
    } else {
        counts
            .iter()
            .map(|(token, n)| format!("{token}={n}"))
            .collect::<Vec<_>>()
            .join(" ")
    };

    let d = &inv.diagnostics;
    let mut notes: Vec<String> = Vec::new();
    for (on, name) in [
        (d.resource_scan_truncated, "RESOURCE_SCAN_TRUNCATED"),
        (d.font_limit_reached, "FONT_LIMIT_REACHED"),
        (d.page_scan_failed, "PAGE_SCAN_FAILED"),
    ] {
        if on {
            notes.push(name.to_owned());
        }
    }
    for (c, name) in [
        (d.direct_font_dicts, "direct_font_dicts"),
        (d.dangling_font_references, "dangling_font_references"),
        (d.descriptors_missing, "descriptors_missing"),
        (d.programs_unreadable, "programs_unreadable"),
        (d.programs_undecodable, "programs_undecodable"),
        (d.descendants_missing, "descendants_missing"),
    ] {
        if c > 0 {
            notes.push(format!("{name}={c}"));
        }
    }
    let warnings = if notes.is_empty() {
        "clean".to_owned()
    } else {
        notes.join(" ")
    };

    let walked = inv
        .coverage
        .walked()
        .iter()
        .map(|s| s.token())
        .collect::<Vec<_>>()
        .join(",");
    let not_walked = inv.coverage.not_walked();
    let not_walked_token = if not_walked.is_empty() {
        "-".to_owned()
    } else {
        not_walked
            .iter()
            .map(|s| s.token())
            .collect::<Vec<_>>()
            .join(",")
    };

    println!(
        "list-fonts {} fonts={} embedded={} bytes={} {verdicts} walked={walked} \
not_walked={not_walked_token} {warnings}",
        input.display(),
        inv.fonts.len(),
        inv.embedded_count(),
        inv.embedded_bytes(),
    );

    // The disclosure Acrobat does not make. One sentence per DISTINCT
    // non-removable verdict present, on stderr so stdout stays a clean
    // machine-readable listing. Deduplicated because a document with forty
    // Identity-H fonts needs the mechanism explained once, not forty times —
    // repetition is how a real warning gets skimmed past.
    let mut seen: std::collections::BTreeSet<&'static str> = std::collections::BTreeSet::new();
    for f in &rows {
        if matches!(f.removability, Removability::Removable) {
            continue;
        }
        if seen.insert(f.removability.token()) {
            eprintln!(
                "pdfcer: {}: verdict {} — {}",
                input.display(),
                f.removability.token(),
                f.removability.reason()
            );
        }
    }
    // Said unconditionally, not only when it bites. An operator reading a
    // font inventory to decide what to delete needs the shape of the
    // evidence, and "there is one place pdfcer did not look" is part of the
    // answer rather than a caveat on it.
    if !not_walked.is_empty() {
        eprintln!(
            "pdfcer: {}: NOT searched: {not_walked_token}. Font dictionaries reachable from \
none of the walked surfaces still occupy bytes in the file but do not appear above.",
            input.display(),
        );
    }
    if d.page_scan_failed {
        eprintln!(
            "pdfcer: {}: the page tree would not walk, so NO page-reachable font is in this \
listing. An empty or short list here is not a statement about the document's fonts.",
            input.display(),
        );
    }
    exit::SUCCESS
}

/// `extract-attachment` — get an embedded file OUT of a PDF.
///
/// Closes a gap that had existed since attachments first became readable:
/// `pdfcer_core::attachments::extract_attachment` could always do this and no
/// shell could ask for it — standing rule R151's exact shape, a core API with
/// no caller.
///
/// The output path is REQUIRED and never derived from the attachment's own
/// name. That name is attacker-controlled and ISO 32000-1 constrains nothing
/// about it: it may carry `..`, a NUL, a reserved device name like `CON`, or
/// a right-to-left override making `gnp.exe` render as `exe.png`. A tool that
/// wrote to a path taken from inside the document would be a path-traversal
/// primitive, so this one cannot be asked to.
pub(crate) fn cmd_extract_attachment(
    input: &Path,
    name: &str,
    output: &Path,
    cut: Option<&Path>,
    mode: SaveMode,
) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let view = doc.view();
    let items = pdfcer_core::attachments::list_attachments(&doc);
    let Some(found) = items.iter().find(|a| a.name == name) else {
        eprintln!(
            "pdfcer: {}: no attachment named {name:?}. Run `pdfcer list-attachments` to \
             see the names this document actually uses.",
            input.display()
        );
        return exit::EDIT_REFUSED;
    };
    match pdfcer_core::attachments::extract_attachment(&view, found) {
        Ok(extracted) => {
            if let Err(err) = std::fs::write(output, &extracted.data) {
                eprintln!("pdfcer: {}: {err}", output.display());
                return exit::IO_ERROR;
            }
            println!(
                "extracted name={:?} bytes={} -> {}",
                found.name,
                extracted.data.len(),
                output.display()
            );
            // `/Size` is Optional and nothing requires it to match the real
            // length (§7.11.4.1 Table 46). A mismatch is REPORTED, not treated
            // as corruption — the bytes are still the bytes.
            println!("  size_check={:?}", extracted.size_check);
            let Some(cut_output) = cut else {
                return exit::SUCCESS;
            };
            // The CUT half, through the core verb so the detach and the
            // extraction are the same gesture and one undo entry -- not two
            // CLI invocations an operator has to get the order of right.
            let (source, mut session) = match open_for_edit(input) {
                Ok(pair) => pair,
                Err(code) => return code,
            };
            if let Err(err) = session.cut_attachment(&found.name_bytes) {
                return report_edit_error(input, &err);
            }
            let outcome = match save_edited(
                &mut session,
                &source,
                cut_output,
                mode,
                ProducerArg::Preserve,
                false,
            ) {
                Ok(outcome) => outcome,
                Err(code) => return code,
            };
            println!(
                "  cut=1 cut_out={} mode={} changed={}",
                cut_output.display(),
                mode.name(),
                outcome.changed,
            );
            finish_edit(input, &outcome)
        }
        Err(err) => {
            eprintln!("pdfcer: {}: {name:?}: {err}", input.display());
            exit::EDIT_REFUSED
        }
    }
}

/// `attach-file` — embed a file into a PDF as a document-level attachment.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_attach_file(
    input: &Path,
    file: &Path,
    name: Option<&str>,
    desc: Option<&str>,
    apply: bool,
    output: Option<&Path>,
    mode: SaveMode,
) -> u8 {
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", file.display());
            return exit::IO_ERROR;
        }
    };
    // The stored name defaults to the source file's own NAME, never its full
    // path: embedding `C:\Users\Ken\...` would put the operator's directory
    // layout inside a document they hand to someone else.
    let stored = match name {
        Some(n) => n.to_string(),
        None => match file.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => {
                eprintln!(
                    "pdfcer: {}: cannot derive a name from this path; pass --name",
                    file.display()
                );
                return exit::EDIT_REFUSED;
            }
        },
    };
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    if let Err(err) = session.attach_file(&stored, &bytes, desc) {
        eprintln!("pdfcer: {}: {err}", input.display());
        return exit::EDIT_REFUSED;
    }
    println!(
        "attach-file {} name={stored:?} bytes={} mode={} applied={}",
        input.display(),
        bytes.len(),
        mode_token(mode),
        u32::from(apply)
    );
    eprintln!(
        "pdfcer: the attachment is NOT protected — it travels with the PDF, and anyone who \
         can open the PDF can extract it."
    );
    if !apply {
        eprintln!("pdfcer: dry run — pass --apply with --output to write the file.");
        return exit::SUCCESS;
    }
    finish_attachment_save(input, &mut session, output, mode)
}

/// `detach-file` — remove a document-level attachment.
pub(crate) fn cmd_detach_file(
    input: &Path,
    name: &str,
    apply: bool,
    output: Option<&Path>,
    mode: SaveMode,
) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    // Resolve the operator's name to the name-tree KEY. The key and the
    // filespec's `/F`/`/UF` are independently-authored strings a real document
    // is free to disagree on, so deleting by the DISPLAYED name would miss
    // exactly the documents where they differ.
    let items = pdfcer_core::attachments::list_attachments(&doc);
    let Some(found) = items.iter().find(|a| a.name == name) else {
        eprintln!(
            "pdfcer: {}: no attachment named {name:?}. Run `pdfcer list-attachments` to \
             see the names this document actually uses.",
            input.display()
        );
        return exit::EDIT_REFUSED;
    };
    let key = match &found.kind {
        pdfcer_core::attachments::AttachmentKind::DocumentLevel { tree_key } => tree_key.clone(),
        other => {
            eprintln!(
                "pdfcer: {}: {name:?} is {other:?} — it lives on a page as an annotation, \
                 not in the document's embedded-file tree, and is removed as an annotation.",
                input.display()
            );
            return exit::EDIT_REFUSED;
        }
    };
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    if let Err(err) = session.detach_file(&key) {
        eprintln!("pdfcer: {}: {err}", input.display());
        return exit::EDIT_REFUSED;
    }
    println!(
        "detach-file {} name={name:?} mode={} applied={}",
        input.display(),
        mode_token(mode),
        u32::from(apply)
    );
    if matches!(mode, SaveMode::Incremental) {
        eprintln!(
            "pdfcer: NOT a redaction. An incremental save keeps every prior revision by \
             design (§7.5.6), so these bytes stay recoverable from the earlier revision. Use \
             --mode full if the point was that they should not be in the file at all."
        );
    }
    if !apply {
        eprintln!("pdfcer: dry run — pass --apply with --output to write the file.");
        return exit::SUCCESS;
    }
    finish_attachment_save(input, &mut session, output, mode)
}

/// The save-mode token both attachment commands print.
pub(crate) const fn mode_token(mode: SaveMode) -> &'static str {
    match mode {
        SaveMode::Incremental => "incremental",
        SaveMode::Full => "full",
    }
}

/// Shared save tail for `attach-file` and `detach-file`.
///
/// One function rather than two copies: the recovered-base hint, the
/// output-required check and the exit codes must not drift apart between two
/// commands an operator will reasonably expect to behave identically.
pub(crate) fn finish_attachment_save(
    input: &Path,
    session: &mut pdfcer_core::edit::EditSession,
    output: Option<&Path>,
    mode: SaveMode,
) -> u8 {
    let Some(out) = output else {
        eprintln!("pdfcer: --apply needs --output <PATH>");
        return exit::RUNTIME_ERROR;
    };
    let saved = match mode {
        SaveMode::Incremental => {
            session.to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        }
        SaveMode::Full => session.to_full_bytes(&pdfcer_core::writer::SaveOptions::default()),
    };
    let (bytes, _report) = match saved {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("pdfcer: {}: save refused: {err}", input.display());
            hint_recovered_base(&err);
            return exit::SAVE_REFUSED;
        }
    };
    if let Err(err) = std::fs::write(out, &bytes) {
        eprintln!("pdfcer: {}: {err}", out.display());
        return exit::IO_ERROR;
    }
    println!("  wrote {} bytes={}", out.display(), bytes.len());
    exit::SUCCESS
}

/// `list-attachments` — embedded files, both kinds, in one list.
///
/// # Why both kinds in one list, each labelled
///
/// Document-level (`/Names /EmbeddedFiles`) and page-level
/// (`/FileAttachment` annotations) are structurally distinct and behave
/// differently on save and on page deletion, so a caller must be able to
/// tell them apart. But an operator asking "what is in this file" should
/// not have to know the distinction exists in order to get a complete
/// answer, so it is one command and one list.
///
/// # The encryption warning is a REFUSAL condition, not a note
///
/// Since PDF 1.5 an otherwise-unencrypted document can carry ENCRYPTED
/// embedded files via `/EFF` + `DefEmbeddedFile` (§7.6.5). The intuitive
/// guard — no password prompt, so plaintext — is wrong, and wrong
/// silently: the filter chain runs and returns garbage that looks like
/// success. pdfcer cannot decrypt yet, so when the flag is set this
/// command says so loudly on stderr rather than letting a caller treat
/// the listing as safe to extract from.
///
/// # What a listing means, and what it does not
///
/// Complete enumeration is impossible **by the standard's own admission**
/// (§7.11.7 NOTE 1/3), not by pdfcer's limitation: no `shall` requires an
/// embedded file to appear in `/EmbeddedFiles`. So this reports what is
/// reachable by the two standard paths, and the summary line says so
/// rather than implying exhaustiveness.
pub(crate) fn cmd_list_attachments(input: &Path) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let session = pdfcer_core::edit::EditSession::new(doc);
    let (items, notes) = pdfcer_core::attachments::list_attachments_with_notes(&session.graph());

    for a in &items {
        // The RAW name, deliberately — it is what the document says, and
        // an operator investigating a suspicious file needs pdfcer's
        // evidence rather than pdfcer's cleanup. `safe_name()` exists for
        // the moment bytes are written somewhere, which this command
        // never does.
        let safe = a.safe_name();
        let changed = if safe.changed {
            format!(
                " UNSAFE_NAME safe={:?} hazards={:?}",
                safe.value, safe.hazards
            )
        } else {
            String::new()
        };
        // A COMPACT kind, not the derived Debug. The real one prints
        // `tree_key: [254, 255, 0, 115, ...]` — UTF-16BE bytes where a
        // reader expects a name, plus object ids nobody asked for. The
        // page number is the part that matters for a page-level
        // attachment; the tree key IS the name, already printed.
        let kind = match &a.kind {
            pdfcer_core::attachments::AttachmentKind::DocumentLevel { .. } => "document".to_owned(),
            pdfcer_core::attachments::AttachmentKind::PageAnnotation { page_index, .. } => {
                format!("page:{}", page_index + 1)
            }
            // `AttachmentKind` is `#[non_exhaustive]`, so a kind added
            // later lands here. Reported as unknown rather than folded
            // into "document" — a wrong label is worse than an honest
            // gap, because only one of the two prompts anyone to look.
            _ => "unknown".to_owned(),
        };
        println!(
            "attachment name={:?} kind={kind} desc={:?} source={:?}{changed}",
            a.name,
            a.description.as_deref().unwrap_or("-"),
            a.name_source,
        );
    }

    if notes.may_be_encrypted {
        eprintln!(
            "pdfcer: WARNING — this document's embedded files may be ENCRYPTED (§7.6.5 \
             /EFF). pdfcer cannot decrypt them yet, and extracting one would produce \
             ciphertext that looks like a successful read. Do not treat these bytes as \
             the file's contents."
        );
    }
    // Same reasoning as the outline diagnostics: only what is not clean.
    let mut n: Vec<String> = Vec::new();
    if notes.page_tree_unwalkable {
        n.push("page_tree_unwalkable".to_owned());
    }
    if notes.truncated {
        n.push("truncated".to_owned());
    }
    if notes.name_tree_budget_exhausted {
        n.push("name_tree_budget_exhausted".to_owned());
    }
    for (c, name) in [
        (notes.name_tree_cycles, "name_tree_cycles"),
        (notes.malformed_tree_entries, "malformed_tree_entries"),
        (
            notes.annotations_without_filespec,
            "annotations_without_filespec",
        ),
        (notes.filespecs_without_stream, "filespecs_without_stream"),
        (notes.unresolvable_streams, "unresolvable_streams"),
    ] {
        if c > 0 {
            n.push(format!("{name}={c}"));
        }
    }
    if notes.may_be_encrypted {
        n.push("MAY_BE_ENCRYPTED".to_owned());
    }
    let warnings = if n.is_empty() {
        "clean".to_owned()
    } else {
        n.join(" ")
    };
    println!(
        "list-attachments {} attachments={} {warnings} (reachable by the two standard \
         paths; ISO 32000-1 §7.11.7 does not require completeness)",
        input.display(),
        items.len(),
    );
    exit::SUCCESS
}

/// `layer-toggle` — show/hide a dimension group's optional-content layer.
pub(crate) fn cmd_layer_toggle(
    input: &Path,
    group: u32,
    hide: bool,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    use pdfcer_core::dimension::GroupId;

    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let visible = match session.toggle_dimension_layer(GroupId(group), !hide) {
        Ok(v) => v,
        Err(err) => return report_edit_error(input, &err),
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "layer-toggle {} group={group} mode={} -> {}; visible={visible} changed={} \
objects={} appended={} out_bytes={}",
        input.display(),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
    );
    finish_edit(input, &outcome)
}
