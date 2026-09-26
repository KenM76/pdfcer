use super::*;

/// `--format` for `sign`.
#[cfg(feature = "signing")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SignFormatArg {
    /// /SubFilter /ETSI.CAdES.detached — PAdES (ISO 32000-2 §12.8.3.4).
    Cades,
    /// /SubFilter /adbe.pkcs7.detached — ISO 32000-1 §12.8.3.3.
    Pkcs7,
}

/// `--mdp-level` for `sign --certify` (`Pass 10.12`): Table 254's three
/// values in Acrobat's own vocabulary.
#[cfg(feature = "signing")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum MdpLevelArg {
    /// P = 1 — no changes permitted.
    None,
    /// P = 2 — form fill-in and signing (the standard's default).
    FormFill,
    /// P = 3 — form fill-in, signing and annotations.
    Annotate,
}

#[cfg(feature = "signing")]
impl MdpLevelArg {
    /// The DocMDP permission level (ISO 32000-1 Table 254) this word names.
    pub(crate) const fn permission(self) -> pdfcer_core::sign::apply::MdpPermission {
        use pdfcer_core::sign::apply::MdpPermission as M;
        match self {
            Self::None => M::NoChanges,
            Self::FormFill => M::FormFillAndSign,
            Self::Annotate => M::FormFillSignAnnotate,
        }
    }
}

/// `--algorithm` for `sign`.
#[cfg(feature = "signing")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SignAlgorithmArg {
    /// RSASSA-PKCS1-v1_5 with SHA-256 (RSA keys; the default for them).
    RsaPkcs1,
    /// RSASSA-PSS with SHA-256 (RSA keys; PAdES-preferred).
    RsaPss,
    /// ECDSA with SHA-256 over P-256 (EC P-256 keys; their default).
    EcdsaP256,
    /// ECDSA with SHA-384 over P-384 (EC P-384 keys; their default).
    EcdsaP384,
}

/// Arguments of `sign`, bundled to stay inside clippy's argument limit.
#[cfg(feature = "signing")]
pub(crate) struct SignArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) cert: &'a Path,
    pub(crate) password: &'a str,
    pub(crate) output: &'a Path,
    pub(crate) format: SignFormatArg,
    pub(crate) algorithm: Option<SignAlgorithmArg>,
    pub(crate) signing_time: Option<&'a str>,
    pub(crate) name: Option<&'a str>,
    pub(crate) reason: Option<&'a str>,
    pub(crate) location: Option<&'a str>,
    pub(crate) contact: Option<&'a str>,
    /// `--certify` / `--mdp-level` (`Pass 10.12`).
    pub(crate) certify: bool,
    pub(crate) mdp_level: Option<MdpLevelArg>,
    pub(crate) field_name: Option<&'a str>,
    pub(crate) visible: Option<&'a str>,
    pub(crate) page: usize,
    pub(crate) reserve: usize,
}

/// The current UTC time as a PDF date string (`D:YYYYMMDDHHmmSSZ`, ISO
/// 32000-1 §7.9.4), from the system clock.
///
/// This is the ONE place in the CLI that reads a clock for a value that
/// ends up inside a document, and it exists only because PAdES requires
/// `/M` and a batch signer has no operator to type one. The caller prints
/// that the value was derived (rule 4). Howard Hinnant's `civil_from_days`
/// does the calendar arithmetic; there is no timezone — the `Z` is honest.
#[cfg(feature = "signing")]
pub(crate) fn pdf_date_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    // civil_from_days: shift the epoch to 0000-03-01 so leap days fall last.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "D:{y:04}{m:02}{d:02}{:02}{:02}{:02}Z",
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

/// `sign` (`Pass 10.9`): PKCS#12 in, signed PDF out, every derived or
/// disclosed fact printed (rules 4 and 11).
#[cfg(feature = "signing")]
pub(crate) fn cmd_sign(args: &SignArgs<'_>) -> u8 {
    use pdfcer_core::sign::apply::{SignApplyError, SignRequest};
    use pdfcer_core::sign::cms_build::SubFilter;
    use pdfcer_core::sign::pkcs12::Pkcs12Signer;
    use pdfcer_core::sign::{SignatureAlgorithm, Signer as _};

    // --- the digital ID -----------------------------------------------------
    let pfx = match std::fs::read(args.cert) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.cert.display());
            return exit::IO_ERROR;
        }
    };
    let signer = match Pkcs12Signer::from_der(&pfx, args.password) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.cert.display());
            return exit::SIGNATURE_FAILED;
        }
    };
    let id = signer.report();
    // Rule 4: what the container was made of, and whether its integrity was
    // actually checked.
    match &id.mac {
        Some(mac) => eprintln!(
            "pdfcer: {}: PKCS#12 integrity MAC ({mac}, {} iterations) verified; key bag {}; {} certificate(s) in the chain{}",
            args.cert.display(),
            id.mac_iterations.unwrap_or(1),
            id.key_scheme,
            id.chain_length,
            if id.unrelated_certificates > 0 {
                format!(
                    "; {} unrelated certificate(s) ignored",
                    id.unrelated_certificates
                )
            } else {
                String::new()
            }
        ),
        None => eprintln!(
            "pdfcer: {}: the PKCS#12 container carries NO integrity MAC, so the password could not be checked before decryption and the file's integrity is unverified",
            args.cert.display()
        ),
    }

    // --- the request ---------------------------------------------------------
    let (signing_time, derived) = match args.signing_time {
        Some(t) => (t.to_owned(), false),
        None => (pdf_date_now(), true),
    };
    if derived {
        eprintln!(
            "pdfcer: --signing-time not given; /M {signing_time} was DERIVED from this machine's clock (UTC). Pass --signing-time to state it yourself."
        );
    }
    let visible = match args.visible {
        None => None,
        Some(text) => {
            let nums: Vec<f64> = text
                .split(',')
                .map(|t| t.trim().parse::<f64>())
                .collect::<Result<_, _>>()
                .unwrap_or_default();
            let [x0, y0, x1, y1] = nums.as_slice() else {
                eprintln!(
                    "pdfcer: {}: --visible must be `x0,y0,x1,y1` in points",
                    args.input.display()
                );
                return exit::EDIT_REFUSED;
            };
            if args.page == 0 {
                eprintln!(
                    "pdfcer: {}: --page is 1-based; 0 is not a page",
                    args.input.display()
                );
                return exit::EDIT_REFUSED;
            }
            Some((
                args.page - 1,
                pdfcer_core::page_tree::Rect {
                    llx: x0.min(*x1),
                    lly: y0.min(*y1),
                    urx: x0.max(*x1),
                    ury: y0.max(*y1),
                },
            ))
        }
    };
    let mut request = SignRequest::at(signing_time.clone());
    request.sub_filter = match args.format {
        SignFormatArg::Cades => SubFilter::EtsiCadesDetached,
        SignFormatArg::Pkcs7 => SubFilter::AdbePkcs7Detached,
    };
    request.algorithm = args.algorithm.map(|a| match a {
        SignAlgorithmArg::RsaPkcs1 => SignatureAlgorithm::RsaPkcs1v15Sha256,
        SignAlgorithmArg::RsaPss => SignatureAlgorithm::RsaPssSha256,
        SignAlgorithmArg::EcdsaP256 => SignatureAlgorithm::EcdsaP256Sha256,
        SignAlgorithmArg::EcdsaP384 => SignatureAlgorithm::EcdsaP384Sha384,
    });
    request.name = args.name.map(str::to_owned);
    request.reason = args.reason.map(str::to_owned);
    request.location = args.location.map(str::to_owned);
    request.contact_info = args.contact.map(str::to_owned);
    // `Pass 10.12`: `--mdp-level` implies `--certify`; `--certify` alone
    // takes Table 254's default (P=2) and SAYS so below.
    let mdp_defaulted = args.certify && args.mdp_level.is_none();
    request.certify = if args.certify || args.mdp_level.is_some() {
        Some(args.mdp_level.map_or(
            pdfcer_core::sign::apply::MdpPermission::FormFillAndSign,
            |l| l.permission(),
        ))
    } else {
        None
    };
    request.field_name = args.field_name.map(str::to_owned);
    request.visible = visible;
    request.reserve = args.reserve;

    // --- sign ----------------------------------------------------------------
    let (_source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let (bytes, report) = match session.sign(
        &signer,
        &request,
        &pdfcer_core::writer::SaveOptions::identity(),
    ) {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.input.display());
            return match err {
                SignApplyError::AlreadyCertified { .. }
                | SignApplyError::CertificationNotFirst { .. }
                | SignApplyError::FieldNotSignature { .. }
                | SignApplyError::FieldAlreadySigned { .. }
                | SignApplyError::FieldHasKids { .. }
                | SignApplyError::RectRefusedForExistingField { .. }
                | SignApplyError::SeedValueViolated { .. }
                | SignApplyError::SeedValueUnevaluable { .. } => exit::EDIT_REFUSED,
                SignApplyError::Sign(_)
                | SignApplyError::Cms(_)
                | SignApplyError::ReservationTooSmall { .. }
                | SignApplyError::SelfVerificationFailed { .. }
                | SignApplyError::PlaceholderNotFound { .. } => exit::SIGNATURE_FAILED,
                SignApplyError::Write(_) => exit::SAVE_REFUSED,
                _ => exit::EDIT_REFUSED,
            };
        }
    };
    if let Err(err) = std::fs::write(args.output, &bytes) {
        eprintln!("pdfcer: {}: {err}", args.output.display());
        return exit::IO_ERROR;
    }

    let sub_filter = String::from_utf8_lossy(report.sub_filter.name()).into_owned();
    println!(
        "sign {} -> {}; field={} ({}) subtype={} algorithm={:?} key={} signer={} serial={} certificates={} byte_range={},{},{},{} cms_bytes={} reserved={} signing_time={} time_derived={} level={} prior_signatures={} self_verified={} out_bytes={}",
        args.input.display(),
        args.output.display(),
        quoted_token(&report.field_name),
        if report.field_reused {
            "existing"
        } else {
            "created"
        },
        sub_filter,
        report.algorithm,
        signer.key_label(),
        quoted_token(&report.signer_subject),
        report.signer_serial_hex,
        report.certificates,
        report.byte_range[0],
        report.byte_range[1],
        report.byte_range[2],
        report.byte_range[3],
        report.cms_bytes,
        report.reserved_bytes,
        report.signing_time,
        u8::from(derived),
        report.pades_level,
        report.prior_signatures,
        u8::from(report.self_verified),
        bytes.len(),
    );
    // `Pass 10.13`: what signing INTO the author's field carried with it.
    if let Some(lock) = &report.field_lock {
        println!("  field_lock: /FieldMDP {lock} (copied from the field's /Lock, Table 233)");
    }
    for n in &report.notes {
        println!("  note: {n}");
    }
    if let Some(level) = report.certification {
        println!(
            "  certification: DocMDP P={} ({}){}",
            level.p(),
            level.meaning(),
            if mdp_defaulted {
                " -- --mdp-level not given; this is Table 254's default"
            } else {
                ""
            }
        );
    }
    // Rule 4: the operator cannot read the appearance back without a viewer,
    // so say what the box shows.
    if !report.appearance_lines.is_empty() {
        println!(
            "  appearance: {}",
            report
                .appearance_lines
                .iter()
                .map(|l| quoted_token(l))
                .collect::<Vec<_>>()
                .join(" | ")
        );
    }
    exit::SUCCESS
}

/// Emit the standard "not implemented yet" message for a stub subcommand
/// and return [`exit::UNIMPLEMENTED`].
pub(crate) fn unimplemented_stub(name: &str) -> u8 {
    eprintln!(
        "pdfcer: `{name}` is not implemented yet — it ships in a later Pass \
(see docs/ROADMAP.md). This is a Pass 0 scaffold stub."
    );
    exit::UNIMPLEMENTED
}

// ---------------------------------------------------------------------------
// Structural page operations (Pass 3.2)
// ---------------------------------------------------------------------------

/// Parse a 1-based page specification into 0-based indices.
///
/// Accepted: `all`, a comma-separated list of single pages (`3`) and
/// inclusive ranges (`3-7`), with optional surrounding whitespace. A
/// descending range (`7-3`) is accepted and expands **descending**, so
/// `--order 3-1` is a legitimate way to reverse three pages.
///
/// ## Why this refuses instead of skipping
///
/// A page number past the end of the document, a zero, or an
/// unparseable token is an **error**, not something to drop quietly. A
/// batch script that asks for pages 1-50 of a 30-page file has made a
/// mistake, and silently handing back 30 pages is how that mistake ships
/// to a thousand documents. This is the CLI half of the R27 fail-clean
/// posture.
///
/// `count` is the document's page count, used both to bound the numbers
/// and to expand `all`.
pub(crate) fn parse_pages(spec: &str, count: usize) -> Result<Vec<usize>, String> {
    let trimmed = spec.trim();
    if trimmed.eq_ignore_ascii_case("all") {
        return Ok((0..count).collect());
    }
    let mut out: Vec<usize> = Vec::new();
    for token in trimmed.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let (first, last) = match token.split_once('-') {
            Some((a, b)) => (parse_page_number(a, count)?, parse_page_number(b, count)?),
            None => {
                let single = parse_page_number(token, count)?;
                (single, single)
            }
        };
        if first <= last {
            out.extend(first..=last);
        } else {
            // Descending on purpose: `--order 3-1` reverses.
            out.extend((last..=first).rev());
        }
    }
    if out.is_empty() {
        return Err("the page specification selected no pages".to_owned());
    }
    Ok(out)
}

/// Parse one 1-based page number into a 0-based index, bounded by
/// `count`.
pub(crate) fn parse_page_number(token: &str, count: usize) -> Result<usize, String> {
    let token = token.trim();
    let number: usize = token
        .parse()
        .map_err(|_| format!("`{token}` is not a page number"))?;
    if number == 0 {
        return Err("page numbers are 1-based, so 0 is not a page".to_owned());
    }
    if number > count {
        return Err(format!(
            "page {number} is past the end of the document, which has {count} page(s)"
        ));
    }
    Ok(number - 1)
}

/// The `signature=` token on a stdout line.
///
/// Lives in the **narrative** half of the line, alongside `mode=`,
/// because it is a name rather than a non-negative integer and the
/// metrics half's contract is `key=<integer>`.
pub(crate) const fn signature_token(impact: SignatureImpact) -> &'static str {
    match impact {
        SignatureImpact::None => "none",
        // Named for the fact it establishes (§12.8.2.2.2's stage 1), NOT
        // for validity. See `pdfcer_core::signature`.
        SignatureImpact::ByteRangePreserved => "byte-range-preserved",
        SignatureImpact::Invalidated => "invalidated",
        // `SignatureImpact` is #[non_exhaustive]; an unmapped future
        // verdict must not silently print as "none".
        _ => "unknown",
    }
}

/// Print the honest expansion of a signature verdict to stderr.
///
/// The `ByteRangePreserved` wording is the load-bearing one, and it is
/// deliberately not reassuring. `iso32000__s__12.8.md`'s VALIDATION MODEL
/// puts it plainly: *"Reporting stage-1 success as 'the signature is
/// still valid' is the specific error this section exists to prevent."*
/// So the message states what was preserved, states what was not
/// established, and stops.
pub(crate) fn report_signature(input: &Path, impact: SignatureImpact) {
    match impact {
        SignatureImpact::None => {}
        SignatureImpact::ByteRangePreserved => eprintln!(
            "pdfcer: {}: this document is signed. The save appends a revision, so each \
signature's signed byte range is preserved (ISO 32000-1 §12.8.1 NOTE 1) — but that is only the \
FIRST of two validation stages. Whether the changes are ones the signer permitted (§12.8.2.2.2) \
is a separate question pdfcer does not answer here, and a validator may still report the document \
as altered since signing.",
            input.display()
        ),
        SignatureImpact::Invalidated => eprintln!(
            "pdfcer: {}: this document is signed, and this save INVALIDATES that signature. \
Keep a copy of the original if the signature matters.",
            input.display()
        ),
        _ => eprintln!(
            "pdfcer: {}: this document is signed and pdfcer cannot classify this save's effect \
on it. Treat the signature as suspect.",
            input.display()
        ),
    }
}

/// Report an [`AssembleReport`]'s honesty counters to stderr.
///
/// Every one of these is something the operator **cannot see by looking
/// at the output file**, which is the test for whether it belongs on
/// stderr rather than only in the machine line.
pub(crate) fn report_assemble(output: &Path, report: &pdfcer_core::pageops::AssembleReport) {
    if report.dangling_references > 0 {
        eprintln!(
            "pdfcer: {}: {} reference(s) pointed at a page that was not copied and were \
dropped — links and destinations that led outside the selection now lead nowhere. The \
annotations themselves were kept.",
            output.display(),
            report.dangling_references
        );
    }
    if report.outline_items_dropped > 0 {
        eprintln!(
            "pdfcer: {}: {} bookmark(s) were dropped because their destination page was not \
copied; {} were carried and repointed.",
            output.display(),
            report.outline_items_dropped,
            report.outline_items_kept
        );
    }
    // Rule 4: re-pointing a cross-file bookmark is pdfcer INFERRING that
    // `chapter1.pdf` in a /Launch means the `chapter1.pdf` in this merge.
    // That inference is almost always right and is never silent.
    if report.outline_items_relinked > 0 {
        eprintln!(
            "pdfcer: {}: {} bookmark(s) pointed at another file and were re-pointed to that \
file's pages inside this document; they would otherwise have been dropped, because the file \
they named no longer exists once everything is one document.",
            output.display(),
            report.outline_items_relinked
        );
    }
    if report.form_fields_renamed > 0 {
        eprintln!(
            "pdfcer: {}: {} form field(s) were renamed with a Doc<N>_ prefix because their \
names collided across sources. Without this, same-named fields become one logical field and \
typing in either fills both.",
            output.display(),
            report.form_fields_renamed
        );
    }
    if report.form_fields_dropped > 0 {
        eprintln!(
            "pdfcer: {}: {} form field(s) were dropped because their widgets straddle the \
copied/not-copied boundary. pdfcer does not copy half a field — a field is identified by name \
across the whole document, so a partial copy would apply a value to controls that no longer exist.",
            output.display(),
            report.form_fields_dropped
        );
    }
    if report.named_destinations_dropped > 0 {
        eprintln!(
            "pdfcer: {}: {} named destination(s) were not carried. Bookmarks that used them \
were rewritten to explicit destinations; links inside the copied pages that used them by name \
will not resolve.",
            output.display(),
            report.named_destinations_dropped
        );
    }
    if report.page_labels_stale {
        eprintln!(
            "pdfcer: {}: this document has a page-label tree (/PageLabels) and it was carried across unchanged, so its numbering is now stale for the pages after the insertion point. Acrobat leaves it stale too; pdfcer says so.",
            output.display()
        );
    }
    if report.page_labels_dropped {
        eprintln!(
            "pdfcer: {}: the source's page-label tree (/PageLabels) was NOT carried. Its \
numbering describes a different set of pages, so carrying it would produce labels that are \
confidently wrong.",
            output.display()
        );
    }
    if report.struct_tree_dropped {
        eprintln!(
            "pdfcer: {}: the source's tagged-PDF structure tree (/StructTreeRoot) was NOT \
carried. Subsetting a structure tree to a page selection is not implemented; copying it whole \
would leave dangling references and a file that claims to be tagged but is not.",
            output.display()
        );
    }
    report_separations(output, &report.separations);
}

/// Translate the one save refusal that has a **typeable remedy** into that
/// remedy, at the shell boundary.
///
/// WHY this exists at all, and why it lives in the CLI rather than the core:
/// [`WriteError::RecoveredBaseForbidsIncremental`]'s own message ends with
/// "(save_full)". That is the correct name for the core's audience — a Rust
/// caller reaching for [`pdfcer_core::writer::save_full`] — and it must stay
/// that way, because the core is a library first and its errors are read by
/// API consumers, not only by this binary.
///
/// But an operator at a shell prompt cannot type `save_full`. They were told
/// what was refused and given a symbol that appears in no `--help` output,
/// which is the failure mode standing rule **R174** names: a diagnostic is
/// only finished when it is read as its actual audience would read it. The
/// same refusal reaches the operator here through `embed-font`, `redact`,
/// `unembed-font` and every other mutating subcommand, so the translation
/// belongs once at the shell's save boundary rather than in each of them.
///
/// The hint deliberately states the COST as well as the flag. `--mode full`
/// is not a free retry: it rewrites the file as a single revision and so
/// destroys any existing digital signature (ISO 32000-1 §12.8.1 NOTE 1).
/// Offering the flag without that consequence would be exactly the "sneaky"
/// half of project rule 4 — pdfcer steering the operator into a destructive
/// path because it was the one that made the command succeed.
///
/// Every other [`WriteError`] variant is left alone: they describe conditions
/// with no single-flag remedy, and inventing a suggestion for them would be
/// worse than silence.
pub(crate) fn hint_recovered_base(err: &pdfcer_core::writer::WriteError) {
    if matches!(
        err,
        pdfcer_core::writer::WriteError::RecoveredBaseForbidsIncremental
    ) {
        eprintln!(
            "pdfcer: retry with `--mode full` to write this file. That rewrites it as a \
             single revision, which is what a recovered base requires — and note it drops \
             superseded revisions and invalidates any existing digital signature."
        );
    }
}

/// Report anything the settings load wants the operator to know.
///
/// Goes to stderr, never changes the exit code, and is silent on the
/// normal case (no file, or a file read cleanly). The settings store's
/// fail-soft contract is that a configuration problem must not stop the
/// work — but a typo at a known line number is exactly the thing a
/// command-line operator can fix in ten seconds if told, and never notices
/// if not.
pub(crate) fn report_settings(report: &pdfcer_core::settings::LoadReport) {
    use pdfcer_core::settings::{SettingNote, StoreKind};

    if report.location.kind == StoreKind::PlatformFallback
        && let Some(dir) = report.location.directory()
    {
        eprintln!(
            "pdfcer: settings are being read from {} because pdfcer's own folder is not writable; they do not travel with the program folder.",
            dir.display()
        );
    }
    for note in &report.notes {
        // Spelled out rather than `{:?}`-printed. A `Debug` dump is a
        // developer's view of a struct; the operator's question is "what
        // did I get wrong and on which line", and the answer has to be a
        // sentence they can act on without reading pdfcer's source.
        let line = match note {
            SettingNote::Unreadable { path, reason } => format!(
                "the settings file at {} could not be read ({reason}); defaults are in use",
                path.display()
            ),
            SettingNote::UnknownKey { key, line } => format!(
                "line {line}: \"{key}\" is not a setting pdfcer knows. It was left in the file, not removed"
            ),
            SettingNote::BadValue {
                key,
                value,
                line,
                using,
            } => format!(
                "line {line}: \"{value}\" is not a value \"{key}\" accepts, so \"{using}\" is being used instead; every other setting in the file still applies"
            ),
            SettingNote::Clamped {
                key,
                value,
                line,
                using,
            } => format!(
                "line {line}: \"{key} = {value}\" is outside the accepted range, so {using} is being used"
            ),
            SettingNote::Malformed { line } => format!(
                "line {line} is not a setting (it needs the form: name = value) and was skipped"
            ),
            SettingNote::Duplicate { key, line } => {
                format!("\"{key}\" is set more than once; the one on line {line} is in effect")
            }
            // `SettingNote` is `#[non_exhaustive]`: a note a future pdfcer
            // adds must still reach the operator, even unspelled.
            _ => "something in the settings file was not applied as written".to_owned(),
        };
        eprintln!("pdfcer: settings: {line}.");
    }
}

/// Render `/DeviceColorant` byte strings as a readable list.
///
/// Table 364 types the value as "name **or** string", so it arrives as
/// bytes with no declared encoding. Lossy UTF-8 is right for a diagnostic
/// line: a colorant is almost always ASCII (`Cyan`, `PANTONE 485 C`), and
/// a mangled byte should still print something an operator can match
/// against their plate list rather than suppressing the whole message.
pub(crate) fn colorant_list(colorants: &[Vec<u8>]) -> String {
    colorants
        .iter()
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect::<Vec<String>>()
        .join(", ")
}

/// Disclose what an operation did to preseparated page sets (§14.11.4).
///
/// Shared by the document producers and the in-place editors because the
/// operator's question is the same either way: *which plates do I still
/// have?* Silent on the overwhelmingly common non-preseparated document.
pub(crate) fn report_separations(output: &Path, impact: &pdfcer_core::pageops::SeparationImpact) {
    if impact.sets_split > 0 {
        // The sentence must match the POLICY rather than assume one. The
        // first version of this said arrays had been "rewritten" under
        // every policy, which is false for `Discard` — that removes the
        // dictionary outright. A true count with a false sentence
        // attached is a worse disclosure than no sentence, because it
        // gets believed.
        let did = match impact.policy {
            pdfcer_core::pageops::SeparationPolicy::Discard => {
                "surviving page(s) had their /SeparationInfo removed entirely, so they are now \
ordinary pages carrying no record of which plate they were"
            }
            _ => {
                "surviving page(s) had their /SeparationInfo /Pages array rewritten to name only \
the plates that are still here"
            }
        };
        eprintln!(
            "pdfcer: {}: this is a PRESEPARATED document (ISO 32000-1 §14.11.4) — several \
page objects are one logical page, one per printing plate. The selection split {} set(s), so \
{} {did}. Removed: {}. Kept: {}.",
            output.display(),
            impact.sets_split,
            impact.pages_changed,
            colorant_list(&impact.colorants_removed),
            colorant_list(&impact.colorants_kept),
        );
    }
    if impact.malformed > 0 {
        eprintln!(
            "pdfcer: {}: {} page(s) carry a /SeparationInfo with no usable /Pages array, \
which §14.11.4 Table 364 makes REQUIRED. They were already non-conforming on arrival and were \
left exactly as they were — repairing one would mean guessing which pages belonged to the set.",
            output.display(),
            impact.malformed
        );
    }
}

/// The `separations=` metrics fragment shared by both metric tails.
///
/// Colorant names are deliberately absent from the machine line: they are
/// unescaped operator-supplied bytes and the metrics format is
/// whitespace-delimited `key=value`. The counts are the machine-readable
/// part; the names go to the stderr prose above, where they can be
/// arbitrary.
pub(crate) fn separation_metrics(impact: &pdfcer_core::pageops::SeparationImpact) -> String {
    format!(
        "sep_sets_split={} sep_pages_changed={} sep_malformed={}",
        impact.sets_split, impact.pages_changed, impact.malformed
    )
}

/// The metrics tail every document-producing subcommand shares.
pub(crate) fn assemble_metrics(
    report: &pdfcer_core::pageops::AssembleReport,
    out_bytes: usize,
) -> String {
    format!(
        "pages={} objects={} dangling={} outline_kept={} outline_dropped={} \
outline_relinked={} fields_renamed={} fields_dropped={} dests_dropped={} labels_dropped={} \
labels_stale={} struct_tree_dropped={} ocg_carried={} {} out_bytes={out_bytes}",
        report.pages,
        report.objects_copied,
        report.dangling_references,
        report.outline_items_kept,
        report.outline_items_dropped,
        report.outline_items_relinked,
        report.form_fields_renamed,
        report.form_fields_dropped,
        report.named_destinations_dropped,
        u32::from(report.page_labels_dropped),
        u32::from(report.page_labels_stale),
        u32::from(report.struct_tree_dropped),
        u32::from(report.optional_content_carried),
        separation_metrics(&report.separations),
    )
}

/// The metrics tail every in-place editing subcommand shares.
pub(crate) fn edit_metrics(outcome: &EditOutcome) -> String {
    format!(
        "changed={} objects={} verbatim={} reserialized={} promoted={} deleted={} \
appended={} out_bytes={} undo_verified={} undo_identical={} delinearized={}",
        outcome.changed,
        outcome.report.objects_written,
        outcome.report.objects_verbatim,
        outcome.report.objects_reserialized,
        outcome.report.promoted.len(),
        outcome.report.objects_deleted,
        outcome.report.bytes_appended,
        outcome.report.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
        u32::from(outcome.report.delinearized),
    )
}

/// Map a [`PageOpError`] to an exit code and print it.
///
/// [`exit::EDIT_REFUSED`] for every named refusal: the documents were
/// readable and pdfcer declined the operation as asked, which a batch
/// script must be able to tell apart from a broken file.
pub(crate) fn report_page_op_error(err: &PageOpError) -> u8 {
    eprintln!("pdfcer: {err}");
    exit::EDIT_REFUSED
}

/// Load a document for a read-only structural operation.
pub(crate) fn open_for_read(path: &Path) -> Result<Document, u8> {
    open_document(path).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", path.display());
        exit_code_for_doc(&err)
    })
}

// =====================================================================
// Pass 12.M2 dimensioning subcommands (decision 011 §2.3/§2.4)
// =====================================================================
