//! `format-text`: restyle a found string in place (size, fill, font, spacing,
//! scale, render mode, baseline, synthetic and automatic bold/italic,
//! decoration) and save incrementally.

use super::*;
use pdfcer_core::text_edit::{FormatError, FormatOutcome, FormatReport, FormatRequest};

/// Arguments for [`cmd_format_text`], grouped to stay under the clippy
/// `too_many_arguments` bound (same pattern as [`EditTextArgs`]).
pub(crate) struct FormatTextArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) output: &'a Path,
    /// 1-based page number.
    pub(crate) page: usize,
    pub(crate) find: &'a str,
    /// `--occurrence`, 1-based.
    pub(crate) occurrence: usize,
    /// `--underline` / `--strikethrough` / `--no-decoration`; `None` when
    /// none was given.
    pub(crate) decoration: Option<pdfcer_core::text_edit::decoration::DecorationSet>,
    /// `--decoration-metrics`.
    pub(crate) decoration_metrics: pdfcer_core::text_edit::decoration::DecorationMetrics,
    /// `--pin-span START:LEN`, unparsed. Parsed inside `cmd_format_text` so
    /// a malformed span fails before any file is opened.
    pub(crate) pin_span: Option<&'a str>,
    pub(crate) set_size: Option<f64>,
    /// `MODEL:comps` as passed on the command line, e.g. `rgb:1,0,0`.
    pub(crate) set_color: Option<&'a str>,
    /// A target font resource key or `/BaseFont`.
    pub(crate) set_font: Option<&'a str>,
    /// `--embed-font FILE`: a donor to subset for `find`.
    pub(crate) embed_font: Option<&'a std::path::Path>,
    /// `--char-spacing` as passed, e.g. `0.5`, `0.5pt`, `20em`.
    pub(crate) char_spacing: Option<&'a str>,
    /// `--word-spacing` as passed, e.g. `2`, `2pt`, `200em`.
    pub(crate) word_spacing: Option<&'a str>,
    /// `--h-scale` percentage (100 = normal).
    pub(crate) h_scale: Option<f64>,
    /// `--render-mode`, `0..=7`.
    pub(crate) render_mode: Option<u8>,
    /// The baseline toggle, already resolved from the three exclusive flags.
    pub(crate) script: Option<pdfcer_core::text_edit::ScriptPosition>,
    /// `--rise` as passed, e.g. `3.25`, `3.25pt`, `280em`.
    pub(crate) rise: Option<&'a str>,
    /// The synthetic styles asked for, already folded from the two flags.
    /// `StyleSynthesis::None` means none were, which is the default and
    /// the only state in which nothing is synthesized.
    pub(crate) synthetic: pdfcer_core::text_edit::StyleSynthesis,
    /// `--bold` / `--italic` / `--no-bold` / `--no-italic`: the automatic
    /// ladder.
    pub(crate) style: pdfcer_core::text_edit::StyleTarget,
    /// `--embed-styled-face`: offer `--font-dir`'s faces to rung 3.
    pub(crate) embed_styled_face: bool,
    /// `--style-policy`, or `None` to use the stored setting. Overrides the
    /// setting for this invocation only; nothing is persisted.
    pub(crate) style_policy: Option<StylePolicyArg>,
    pub(crate) pin: bool,
    /// The `--target` selector, unparsed — see `parse_edit_target`.
    pub(crate) target: &'a str,
    pub(crate) font_dirs: &'a [PathBuf],
}

/// Parse a text-space metric argument into a `MetricSpec`
/// (`0.5` / `0.5pt` → absolute; `20em` → 20 thousandths of an em).
///
/// `flag` is the option's own spelling (`--char-spacing`, `--word-spacing`,
/// `--rise`), used only in the error message. One parser serves all three
/// because `Tc`, `Tw` and `Ts` share one unit model: unscaled text-space units
/// (ISO 32000-2 §9.3, Table 105's closing note), Absolute/Relative per R89.
///
/// The `em` suffix means **‰ of an em**, not ems — the tracking convention and
/// the unit `TJ` adjustments use (§9.4.3). The error text says so too.
///
/// # Errors
///
/// The operator-facing message, ready to print.
pub(crate) fn parse_text_metric(
    flag: &str,
    spec: &str,
) -> Result<pdfcer_core::text_edit::MetricSpec, String> {
    use pdfcer_core::text_edit::MetricSpec;
    let raw = spec.trim();
    let (number, relative) = match raw {
        r if r.len() > 2 && r.to_ascii_lowercase().ends_with("em") => (&r[..r.len() - 2], true),
        r if r.len() > 2 && r.to_ascii_lowercase().ends_with("pt") => (&r[..r.len() - 2], false),
        r => (r, false),
    };
    let value: f64 = number.trim().parse().map_err(|_| {
        format!(
            "{flag} {spec:?}: expected a number optionally suffixed `pt` (absolute, \
             unscaled text-space units) or `em` (RELATIVE — thousandths of an em, the tracking \
             unit; `20em` is 20/1000 em, NOT 20 ems)"
        )
    })?;
    if !value.is_finite() {
        return Err(format!("{flag} {spec:?}: not a finite number"));
    }
    Ok(if relative {
        MetricSpec::Relative(value)
    } else {
        MetricSpec::Absolute(value)
    })
}

/// Parse a `--set-color MODEL:C,..` argument into a `NewFill`
/// (`rgb:1,0,0`, `cmyk:0,1,1,0`, `gray:0.5`).
///
/// # Errors
///
/// The operator-facing message, ready to print.
pub(crate) fn parse_set_color(spec: &str) -> Result<pdfcer_core::text_edit::NewFill, String> {
    use pdfcer_core::text_edit::{FillModel, NewFill};
    let (model_str, comps_str) = spec
        .split_once(':')
        .ok_or_else(|| format!("--set-color {spec:?}: expected MODEL:comps, e.g. rgb:1,0,0"))?;
    let model = match model_str.trim().to_ascii_lowercase().as_str() {
        "rgb" => FillModel::Rgb,
        "cmyk" => FillModel::Cmyk,
        "gray" | "grey" => FillModel::Gray,
        other => {
            return Err(format!(
                "--set-color: unknown model {other:?} (expected rgb, cmyk, or gray)"
            ));
        }
    };
    let mut comps = Vec::new();
    for part in comps_str.split(',') {
        let v: f64 = part
            .trim()
            .parse()
            .map_err(|_| format!("--set-color: {part:?} is not a number"))?;
        comps.push(v);
    }
    NewFill::new(model, comps).map_err(|e| e.to_string())
}

/// Parse a `--pin-span START:LEN` argument.
///
/// `START:LEN`, not `START:END`, because that is the shape `ByteSpan` and
/// `extract-text --json --spans` (`op_start` / `op_len`) already use.
///
/// # Errors
///
/// A message naming what was wrong with the value. Both numbers must parse
/// and `LEN` must be non-zero — a zero-length span names no operator.
pub(crate) fn parse_pin_span(spec: &str) -> Result<pdfcer_core::span::ByteSpan, String> {
    let (a, b) = spec.split_once(':').ok_or_else(|| {
        format!("--pin-span {spec:?} is not START:LEN (get the numbers from `extract-text --json --spans`)")
    })?;
    let start: usize = a
        .trim()
        .parse()
        .map_err(|_| format!("--pin-span start {a:?} is not a byte offset"))?;
    let len: usize = b
        .trim()
        .parse()
        .map_err(|_| format!("--pin-span length {b:?} is not a byte count"))?;
    if len == 0 {
        return Err("--pin-span length is 0, which names no operator".to_owned());
    }
    Ok(pdfcer_core::span::ByteSpan { start, len })
}

/// `format-text`: in-place formatting of `--find` on `--page`.
///
/// The match lies inside one show operator, or across consecutive show
/// operators sharing font resource, size and baseline (a producer that emits
/// one glyph per operator). The formatting goes through the shared
/// advance-preserving surgery, the line is relaid out (reflow by default;
/// `--pin` compensates), and the result is saved INCREMENTALLY.
///
/// Exit codes: every refusal — a malformed flag, a coverage failure on a
/// family change, a missing target font, an invalid colour, an outlined run —
/// is [`exit::EDIT_REFUSED`]; a write refusal is [`exit::SAVE_REFUSED`]; I/O is
/// [`exit::IO_ERROR`]. All disclosures (trust level, incremental/prior state,
/// colour narrowing, tagged-stale, relayout overflow) are printed verbatim.
pub(crate) fn cmd_format_text(args: &FormatTextArgs<'_>) -> u8 {
    // The shell owns font discovery (R61): `--font-dir` supplies operator
    // faces for a NON-embedded target's preview/trust level (decision 012).
    let (font_env, supplied_registered, font_notes) = build_font_environment(args.font_dirs);
    for note in &font_notes {
        eprintln!("pdfcer: font-dir: {note}");
    }
    let flags = match parse_format_flags(args) {
        Ok(f) => f,
        Err(code) => return code,
    };
    let source = match std::fs::read(args.input) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.input.display());
            return exit::IO_ERROR;
        }
    };
    let doc = match open_document_bytes(source) {
        Ok(d) => d,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.input.display());
            return exit_code_for_doc(&err);
        }
    };
    let req = match build_format_request(args, flags) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let (policy, opts) = format_options(args);
    let outcome = match pdfcer_core::text_edit::set_format(&doc, &req, &opts) {
        Ok(o) => o,
        Err(err) => {
            eprintln!("pdfcer: format-text refused: {err}");
            return format_error_exit(&err);
        }
    };
    if let Err(err) = write_output(args.output, &outcome.bytes) {
        eprintln!("pdfcer: {}: {err}", args.output.display());
        return exit::IO_ERROR;
    }
    report_format_outcome(args, &outcome, policy, &font_env, supplied_registered);
    exit::SUCCESS
}

/// The flags parsed before any file is opened, so a malformed one fails first.
struct FormatFlags {
    pin_span: Option<pdfcer_core::span::ByteSpan>,
    fill: Option<pdfcer_core::text_edit::NewFill>,
    char_spacing: Option<pdfcer_core::text_edit::MetricSpec>,
    word_spacing: Option<pdfcer_core::text_edit::MetricSpec>,
    rise: Option<pdfcer_core::text_edit::MetricSpec>,
}

fn parse_flag<T>(
    spec: Option<&str>,
    parse: impl FnOnce(&str) -> Result<T, String>,
) -> Result<Option<T>, u8> {
    match spec.map(parse).transpose() {
        Ok(v) => Ok(v),
        Err(msg) => {
            eprintln!("pdfcer: {msg}");
            Err(exit::EDIT_REFUSED)
        }
    }
}

fn parse_format_flags(args: &FormatTextArgs<'_>) -> Result<FormatFlags, u8> {
    let pin_span = parse_flag(args.pin_span, parse_pin_span)?;
    // An empty `--find` means "the whole pinned operator" and is meaningless
    // without a pin; refused here so the message names the flag to add.
    if args.find.is_empty() && pin_span.is_none() {
        eprintln!(
            "pdfcer: format-text needs --find TEXT, or --pin-span START:LEN with an empty \
             --find to mean the whole pinned show operator"
        );
        return Err(exit::EDIT_REFUSED);
    }
    Ok(FormatFlags {
        pin_span,
        fill: parse_flag(args.set_color, parse_set_color)?,
        char_spacing: parse_flag(args.char_spacing, |s| {
            parse_text_metric("--char-spacing", s)
        })?,
        word_spacing: parse_flag(args.word_spacing, |s| {
            parse_text_metric("--word-spacing", s)
        })?,
        rise: parse_flag(args.rise, |s| parse_text_metric("--rise", s))?,
    })
}

/// Validate the 1-based numbers and the target, then start the request.
fn format_request_base(args: &FormatTextArgs<'_>) -> Result<FormatRequest, u8> {
    if args.page == 0 {
        eprintln!("pdfcer: --page is 1-based; 0 is not a valid page number");
        return Err(exit::EDIT_REFUSED);
    }
    let target = match parse_edit_target(args.target) {
        Ok(t) => t,
        Err(message) => {
            eprintln!("pdfcer: format-text refused: {message}");
            return Err(exit::EDIT_REFUSED);
        }
    };
    if args.occurrence == 0 {
        eprintln!("pdfcer: --occurrence is 1-based; 0 is not a valid occurrence");
        return Err(exit::EDIT_REFUSED);
    }
    Ok(FormatRequest::new(args.page - 1, args.find)
        .target(target)
        .occurrence(args.occurrence - 1))
}

fn build_format_request(
    args: &FormatTextArgs<'_>,
    flags: FormatFlags,
) -> Result<FormatRequest, u8> {
    use pdfcer_core::text_edit::FontSelector;
    let mut req = format_request_base(args)?;
    if let Some(set) = args.decoration {
        req = req
            .decoration(set)
            .decoration_metrics(args.decoration_metrics);
    }
    if let Some(span) = flags.pin_span {
        req = req.pinned(span);
    }
    if let Some(size) = args.set_size {
        req = req.size(size);
    }
    if let Some(f) = flags.fill {
        req = req.fill(f);
    }
    if let Some(name) = args.set_font {
        req = req.font(FontSelector::new(name));
    }
    if let Some(donor_path) = args.embed_font {
        req = req.embedded_font(donor_plan(donor_path, args.find)?);
    }
    if let Some(spec) = flags.char_spacing {
        req = req.char_spacing(spec);
    }
    if let Some(spec) = flags.word_spacing {
        req = req.word_spacing(spec);
    }
    if let Some(pct) = args.h_scale {
        req = req.h_scale(pct);
    }
    if let Some(mode) = args.render_mode {
        req = req.render_mode(mode);
    }
    if let Some(pos) = args.script {
        req = req.script(pos);
    }
    if let Some(spec) = flags.rise {
        req = req.rise(spec);
    }
    with_style_requests(args, req)
}

fn with_style_requests(
    args: &FormatTextArgs<'_>,
    mut req: FormatRequest,
) -> Result<FormatRequest, u8> {
    if !args.synthetic.is_none() {
        req = req.synthetic(args.synthetic);
    }
    if !args.style.is_keep() {
        req = req.style(args.style);
    }
    if args.embed_styled_face {
        for plan in style_donor_plans(args.font_dirs, args.find, args.style)? {
            req = req.style_donor(plan);
        }
    }
    Ok(req)
}

/// The bold/italic fallback posture (decision 106), resolved in the shell and
/// handed to core as a value; `--style-policy` overrides the stored setting
/// for this invocation only.
fn format_options(
    args: &FormatTextArgs<'_>,
) -> (
    pdfcer_core::settings::StylePolicy,
    pdfcer_core::text_edit::FormatOptions,
) {
    use pdfcer_core::text_edit::{FollowerDisposition, FormatOptions};
    let (settings, settings_report) =
        pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
    report_settings(&settings_report);
    let policy = args
        .style_policy
        .map_or(settings.style_policy, StylePolicyArg::to_core);
    let opts = FormatOptions::default()
        .with_style_policy(policy)
        .with_disposition(if args.pin {
            FollowerDisposition::Pin
        } else {
            FollowerDisposition::Reflow
        });
    (policy, opts)
}

fn format_error_exit(err: &FormatError) -> u8 {
    match err {
        FormatError::Refused(_)
        | FormatError::CoverageFailure(_)
        | FormatError::NoOp
        | FormatError::BadColor(_)
        | FormatError::TargetFontMissing(_)
        | FormatError::NoMatch(_)
        | FormatError::Unsupported(_)
        | FormatError::PageIndex(_)
        | FormatError::AmbientUnrestorable(_)
        | FormatError::BadHorizScale(_)
        | FormatError::WordSpacingComposite { .. }
        | FormatError::ConflictingRise
        | FormatError::InvalidRenderMode { .. }
        | FormatError::DecorationOnInvisibleText { .. }
        | FormatError::ConflictingRenderMode
        | FormatError::RealFaceAvailable { .. }
        | FormatError::SynthesisRefusedByPosture { .. }
        | FormatError::NoFaceWithoutStyle { .. }
        | FormatError::ShearUnsupported(_)
        | FormatError::Encrypted => exit::EDIT_REFUSED,
        FormatError::Write(_) => exit::SAVE_REFUSED,
        _ => exit::RUNTIME_ERROR,
    }
}

fn change_str<T: std::fmt::Display>(change: Option<(T, T)>) -> String {
    change.map_or_else(|| "none".to_owned(), |(o, n)| format!("{o}->{n}"))
}

fn report_format_outcome(
    args: &FormatTextArgs<'_>,
    outcome: &FormatOutcome,
    policy: pdfcer_core::settings::StylePolicy,
    font_env: &pdfcer_render::FontEnvironment,
    supplied_registered: usize,
) {
    let report = &outcome.report;
    println!(
        "format-text {} -> {}",
        args.input.display(),
        args.output.display()
    );
    println!("  page={} find={:?}", args.page, args.find);
    print_format_controls(report);
    print_style_disclosures(report, policy);
    print_format_placement(report);
    print_format_trust(report, font_env, supplied_registered);
}

/// One field per operation, all present (`none` when not requested), so the
/// lines diff cleanly; changes print as `ambient->emitted`.
fn print_format_controls(report: &FormatReport) {
    let font_str = report
        .font_change
        .as_ref()
        .map_or_else(|| "none".to_owned(), |(o, n)| format!("{o}->{n}"));
    println!(
        "  set_size={} set_color={} set_font={font_str}",
        change_str(report.size_change),
        report.fill_space.unwrap_or("none")
    );
    let tz_str = report
        .h_scale_change
        .map_or_else(|| "none".to_owned(), |(o, n)| format!("{o}%->{n}%"));
    let script_str = report.script.map_or_else(
        || "none".to_owned(),
        |p| {
            let rise = report
                .rise_change
                .map_or_else(|| "0".to_owned(), |(_, n)| format!("{n}"));
            let size = report
                .script_size
                .map_or_else(|| "unchanged".to_owned(), |(b, e)| format!("{b}->{e}"));
            format!("{}(Ts={rise} Tf={size})", p.label())
        },
    );
    // `spaces=` counts the code-32s `Tw` reaches (§9.3.3), so a widened gap
    // that also moved four others is visible.
    let tw_str = report.word_spacing_change.map_or_else(
        || "none".to_owned(),
        |(o, n)| {
            let spaces = report
                .word_spacing_affected_codes
                .map_or_else(String::new, |c| format!(" spaces={c}"));
            format!("{o}->{n}{spaces}")
        },
    );
    println!(
        "  char_spacing={} word_spacing={tw_str} h_scale={tz_str} script={script_str}",
        change_str(report.char_spacing_change)
    );
    if let Some((o, n)) = report.render_mode_change {
        println!("  render_mode={o}->{n}");
    }
    let synth_str = if report.synthesis.is_none() {
        "none".to_owned()
    } else {
        let bold = report
            .synthetic_bold_width
            .map_or_else(String::new, |w| format!(" stroke_w={w}"));
        let ital = report.synthetic_italic.map_or_else(String::new, |(t, o)| {
            format!(" shear_tan={t} rise_offset={o}")
        });
        format!("{}{bold}{ital}", report.synthesis)
    };
    println!(
        "  rise={} synthesis={synth_str}",
        change_str(report.rise_change)
    );
}

/// The automatic ladder's rung, and the disclosure a non-refusing posture
/// owes when it synthesised a style while a real face was available
/// (decision 106): `auto` notes it on stdout, `warn` warns on stderr. The
/// engine's sentence is printed verbatim.
fn print_style_disclosures(report: &FormatReport, policy: pdfcer_core::settings::StylePolicy) {
    if let Some(l) = &report.style_ladder {
        println!(
            "  style_ladder: requested={} rung={:?} bound={} synthesised={} passed_over={} \
             removed={} unsynthesised={}",
            l.requested.axes(),
            l.rung,
            l.bound
                .as_deref()
                .map_or_else(|| "-".to_owned(), quoted_token),
            l.synthesised.axes(),
            l.passed_over.len(),
            l.removed.axes(),
            l.unsynthesised.axes()
        );
    }
    if let Some(passed_over) = &report.real_face_passed_over {
        match policy {
            pdfcer_core::settings::StylePolicy::Warn => {
                eprintln!("pdfcer: warning: {passed_over}");
            }
            // `Refuse` cannot reach here -- the call returned `Err`.
            _ => println!("  note: {passed_over}"),
        }
    }
}

/// Where the restyle landed and how far it reaches (a shared form's fan-out
/// is printed because in the CLI the invocation is the commit).
fn print_format_placement(report: &FormatReport) {
    if !report.restore_narrowed.is_empty() {
        let names: Vec<String> = report
            .restore_narrowed
            .iter()
            .map(ToString::to_string)
            .collect();
        println!("  restore_narrowed={}", names.join(","));
    }
    if report.justify_slack_invalidated {
        println!("  justify_slack_invalidated=1");
    }
    if let Some(object) = report.form_object {
        println!(
            "  form_object={object} form_invocations={} form_pages={}",
            report.form_invocations,
            report
                .form_pages
                .iter()
                .map(|p| (p + 1).to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    println!(
        "  base_font={} content_object={} advance_delta={:.3} followers_repositioned={} fill_narrowed={}",
        report.base_font,
        report.content_object,
        report.advance_delta,
        report.followers_repositioned,
        u8::from(report.fill_narrowed),
    );
}

/// The decision-012 trust level (as `edit-text` prints it), the tag and the
/// disclosures.
fn print_format_trust(
    report: &FormatReport,
    font_env: &pdfcer_render::FontEnvironment,
    supplied_registered: usize,
) {
    use pdfcer_core::text_edit::EditGlyphSource;
    let trust = match report.glyph_source {
        EditGlyphSource::Embedded => "Embedded",
        EditGlyphSource::NonEmbedded => match font_env.classify_nonembedded(&report.base_font) {
            pdfcer_render::GlyphSource::Supplied => "Supplied",
            _ => "Bundled",
        },
        _ => "unknown",
    };
    println!(
        "  glyph_source={trust} subset={} supplied_registered={}",
        report.subset, supplied_registered
    );
    if let Some(mcid) = report.tagged_mcid {
        println!("  tagged_mcid={mcid}");
    }
    println!("  disclosures:");
    for d in &report.disclosures {
        println!("    - {d}");
    }
}
