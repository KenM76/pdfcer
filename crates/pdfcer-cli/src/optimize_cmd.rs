//! `optimize`: make a PDF smaller under presets, profiles, rules and
//! per-object overrides (Pass 616.1, decision 191).

use std::fmt::Write as _;

use pdfcer_core::edit::EditSession;
use pdfcer_core::optimize::budget::BudgetError;
use pdfcer_core::optimize::{
    DiscardOptions, OptimizeError, OptimizePlan, OptimizeSettings, OptimizedFile, Preset,
    RebuildReport, Refusal, profile,
};
use pdfcer_core::space_audit;
use pdfcer_core::writer::SaveOptions;

use super::*;
use crate::optimize_budget_cmd::BudgetSummary;
use crate::optimize_removals_report::{
    cleanup_json, cleanup_text, discard_json, discard_objects_json, discard_objects_text,
    discard_text,
};

/// clap's own code for a usage error, which `exit` reserves.
const USAGE: u8 = 2;

/// The `optimize` arguments.
#[derive(Debug, Clone, clap::Args)]
pub(crate) struct OptimizeArgs {
    /// Input PDF.
    #[arg(required_unless_present_any = ["list_presets", "write_profile"])]
    pub(crate) input: Option<PathBuf>,
    /// Output PDF. Required unless `--in-place` or `--dry-run`.
    #[arg(short, long, conflicts_with = "in_place")]
    pub(crate) output: Option<PathBuf>,
    /// Replace the input file instead of writing a new one.
    #[arg(long)]
    pub(crate) in_place: bool,
    /// Start from a built-in preset: lossless, standard, screen (alias
    /// mobile), print or archive. The default is standard, which downsamples
    /// images; lossless and archive change no image. --list-presets says
    /// what each does.
    #[arg(long, conflicts_with = "profile")]
    pub(crate) preset: Option<String>,
    /// Start from a saved profile (TOML). An `extends` in it names a preset
    /// or another profile file, relative to the naming one.
    #[arg(long, value_name = "FILE")]
    pub(crate) profile: Option<PathBuf>,
    /// Replace the default stream action: leave, recompress (level 9),
    /// recompress-0 to recompress-9, brotli (quality 11) or brotli-0 to
    /// brotli-11 (smaller, but only readers supporting the PDF 2.0 Brotli
    /// extension draw it: Acrobat Reader opens the file and shows those
    /// pages blank, with no error; adds that extension and /Version 2.0,
    /// skipped for PDF/A), or image settings for every image
    /// (other streams keep the preset's action): "image(ppi=150)" downsamples every image whose largest drawn
    /// use is above 1.5 x 150 ppi to 150 ppi there. Keys: method
    /// (off|average|subsample|bicubic|lanczos), ppi, above (the trigger),
    /// encode (keep|auto|flate|flate-N|jpeg|runlength|lzw|brotli|brotli-N|ccitt|jbig2; auto writes the smallest lossless filter the document version allows; ccitt and jbig2 need 1-bit output, jbig2 PDF 1.4; brotli, quality 0-11, needs PDF 2.0, adds the Brotli extension and is skipped for PDF/A), quality (1-100 or minimum, low,
    /// medium, high, maximum; default 80), chroma (auto|444|420),
    /// progressive, convert (keep|gray|bilevel|indexed), threshold (otsu,
    /// dither or 0-255, for bilevel), colors (2-256, for indexed; default
    /// 256), predictor (off|png|auto, default auto: Flate output is
    /// PNG-predicted when that is smaller), keyed (subsample|skip, default
    /// subsample: a colour-keyed image is downsampled by nearest pixel so
    /// its key still matches), version-raise (allow|refuse, default allow:
    /// encode=jbig2 in a document older than PDF 1.4 sets the catalog's
    /// /Version to 1.4, encode=brotli below 2.0 sets 2.0, and says so;
    /// refuse skips the image), max-pixels (an image drawn on no page has
    /// no resolution, so it is skipped unless this caps its width x height),
    /// min-saving (percent), ssim (0-1, e.g. 0.98: JPEG at the lowest
    /// quality whose structural similarity to the samples reaches it, with
    /// quality as the ceiling when set; the report gives the score), reset.
    /// "image(encode=jpeg)"
    /// rewrites every eligible image as JPEG at its size; a JPEG is never
    /// written above its own estimated quality, and one left at its size is
    /// not touched. "image(convert=gray)" writes colour images as
    /// DeviceGray by the ISO 32000-2 formulas; bilevel makes them 1 bit;
    /// indexed builds a palette (exact when the image has few enough
    /// colours, else median cut). Soft masks, colour-keyed images (for
    /// conversion and JPEG) and
    /// Separation, DeviceN or Lab images are skipped with the reason.
    /// "content(...)" compacts page content, form XObjects and tiling
    /// patterns instead; see --content.
    #[arg(long, value_name = "ACTION")]
    pub(crate) streams: Option<String>,
    /// Compact content streams (page content, form XObjects, tiling
    /// patterns), stored with the stream action (recompress-9 for leave).
    /// Keys, comma-separated, defaults first: whitespace=minimal|keep (drop
    /// separators a reader does not need, and comments), numbers=shortest|keep
    /// (0.500 becomes .5), redundant=on|off (drop empty q Q, identity cm and
    /// repeated setters), round=off|T (lossy: round the path coordinates and
    /// text line starts of page content, and of forms and tiling patterns
    /// only page content draws, no point moving over T units of 1/72 inch),
    /// reset. Bare --content takes every default. A rewrite that
    /// re-reads differently is skipped. Per object: --object "12=content(numbers=keep)".
    #[arg(long, value_name = "SPEC", num_args = 0..=1, default_missing_value = "content()")]
    pub(crate) content: Option<String>,
    /// Add a rule, after the profile's. Whitespace-separated key=value pairs
    /// using the profile's `[[rule]]` keys, list values comma-separated:
    /// "category=fonts,images pages=2- min-bytes=4096 action=leave", or
    /// "image-class=color,gray action=image(ppi=150)".
    /// Repeatable; a later rule beats an earlier one.
    #[arg(long = "rule", value_name = "SPEC")]
    pub(crate) rules: Vec<String>,
    /// Override one object, beating every rule: "12=leave",
    /// "12 0=recompress-6" or "42=image(method=off)", which keeps every
    /// other image setting. Repeatable.
    #[arg(long = "object", value_name = "ID=ACTION")]
    pub(crate) objects: Vec<String>,
    /// Acrobat's compression modes in one switch. entire: recompress
    /// streams and pack objects into object streams (the presets' default).
    /// structure: object streams only, every stream's data left as stored.
    /// leave: neither. For Acrobat's "remove compression" (decoded, larger,
    /// for inspection), use `pdfcer export-structure`.
    #[arg(
        long,
        value_enum,
        conflicts_with_all = ["streams", "no_object_streams", "keep_structure"]
    )]
    pub(crate) compression: Option<Compression>,
    /// Keep the file's structure: no unreferenced-object removal, no
    /// duplicate merging, no object streams. Same as all three --keep flags.
    #[arg(long)]
    pub(crate) keep_structure: bool,
    /// Keep objects nothing in the document refers to.
    #[arg(long)]
    pub(crate) keep_unreferenced: bool,
    /// Keep identical objects (images, fonts, embedded files) as separate
    /// copies instead of merging them.
    #[arg(long)]
    pub(crate) keep_duplicates: bool,
    /// Write a classic cross-reference table instead of packing objects into
    /// compressed object streams. Use for readers older than PDF 1.5.
    #[arg(long)]
    pub(crate) no_object_streams: bool,
    /// Write a linearized file ("Fast Web View"): page 1 first, then a hint
    /// stream telling a viewer where every other page is, so a browser can
    /// show page 1 before the download finishes. Renumbers every object,
    /// moves inherited page attributes onto each page, and does not pack
    /// object streams. The file usually grows slightly; it is written even
    /// when larger than the input.
    #[arg(long, conflicts_with = "keep_structure")]
    pub(crate) linearize: bool,
    /// Keep Link annotations whose destination is a missing page or an
    /// undefined named destination. Every preset deletes them.
    #[arg(long)]
    pub(crate) keep_invalid_links: bool,
    /// Keep bookmarks that go nowhere. Every preset deletes a broken
    /// bookmark once none of its children works.
    #[arg(long)]
    pub(crate) keep_invalid_bookmarks: bool,
    /// Keep named destinations nothing in the file names (the screen
    /// preset deletes them).
    #[arg(long, conflicts_with = "remove_unreferenced_destinations")]
    pub(crate) keep_unreferenced_destinations: bool,
    /// Delete named destinations nothing in the file names, under any
    /// preset. A link from another file to one of them stops working.
    #[arg(long)]
    pub(crate) remove_unreferenced_destinations: bool,
    /// Remove user data of these kinds, on top of the preset's: info,
    /// document-xmp, object-xmp, pieceinfo, thumbnail, javascript,
    /// attachment, comment, hidden-layer, form-data, earlier-revisions,
    /// document-id; or these document objects: bookmarks, tags (the file is
    /// no longer tagged for screen readers), print-settings,
    /// alternate-images, form-actions (submit, import and reset),
    /// external-references (actions that open another file; web links
    /// stay), object-data (the properties CAD and diagram programs attach
    /// to drawn objects), form-fields (deleted, not flattened: what they
    /// show goes too), multimedia (sound, video and rich media), 3d-models,
    /// flatten-layers (visible layers merge into the page; hidden ones stay
    /// unless hidden-layer is given too), search-index (an index Acrobat
    /// built for searching many files; the file's own text stays
    /// searchable), merge-image-fragments (a picture stored as many thin
    /// strips becomes one image that looks the same), smooth-lines (runs of
    /// short straight lines, as CAD programs draw arcs, become curves within
    /// --smooth-tolerance); or all. Comma-separated or repeated.
    /// The standard and print presets remove thumbnails; screen also
    /// pieceinfo, object-xmp and alternate-images.
    #[arg(long, value_name = "KINDS", value_delimiter = ',')]
    pub(crate) discard: Vec<String>,
    /// How far, in page points, a smooth-lines curve may stray from the
    /// lines it replaces (default 0.1; 1 pt is 1/72 inch). Smaller keeps
    /// more curves and saves less.
    #[arg(long, value_name = "PT")]
    pub(crate) smooth_tolerance: Option<f64>,
    /// Keep these kinds and objects even when the preset or --discard
    /// removes them. The same names as --discard; all keeps everything.
    #[arg(long, value_name = "KINDS", value_delimiter = ',')]
    pub(crate) keep_data: Vec<String>,
    #[command(flatten)]
    pub(crate) fonts: crate::optimize_fonts::FontArgs,
    /// Optimize a signed document anyway. Every signature is invalidated,
    /// and the report says how many.
    #[arg(long)]
    pub(crate) invalidate_signatures: bool,
    /// Fit the output under SIZE: if the settings alone miss it, colour and
    /// grey images are written as JPEG at quality 80 down to 30, then
    /// resampled to 200 down to 72 ppi, and the least lossy step that fits
    /// is kept. Your rules and --object overrides still win. SIZE is bytes,
    /// or a number with KB, MB or GB (1000s) or KiB, MiB or GiB (1024s).
    /// When even the last step misses, the closest file is still written
    /// and the exit code is 15.
    #[arg(long, value_name = "SIZE", value_parser = crate::optimize_budget_cmd::parse_size)]
    pub(crate) target_size: Option<u64>,
    #[command(flatten)]
    pub(crate) visual: crate::optimize_visual_diff::VisualDiffArgs,
    #[command(flatten)]
    pub(crate) compat: crate::optimize_compat::CompatArgs,
    /// Plan and measure without writing anything. The report shows the size
    /// the output would have.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Print the report as JSON.
    #[arg(long)]
    pub(crate) json: bool,
    /// Write the resolved settings as a complete profile to FILE (`-` for
    /// stdout). Without an input, does only that.
    #[arg(long, value_name = "FILE")]
    pub(crate) write_profile: Option<PathBuf>,
    /// List the presets and what each does, then exit.
    #[arg(long)]
    pub(crate) list_presets: bool,
}

/// `--compression`'s modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Compression {
    /// Recompress streams and use object streams.
    Entire,
    /// Object streams only; stream data left as stored.
    Structure,
    /// Neither: stream data as stored, a classic cross-reference table.
    Leave,
}

/// Run `optimize`.
pub(crate) fn cmd_optimize(args: &OptimizeArgs) -> u8 {
    if args.list_presets {
        print!("{}", presets_text());
        return exit::SUCCESS;
    }
    let settings = match resolve_settings(args) {
        Ok(s) => s,
        Err(message) => {
            eprintln!("pdfcer: optimize: {message}");
            return USAGE;
        }
    };
    if let Some(path) = &args.write_profile
        && let Err(code) = write_profile(path, &settings)
    {
        return code;
    }
    let Some(input) = &args.input else {
        return exit::SUCCESS;
    };
    let output = match (&args.output, args.in_place, args.dry_run) {
        (Some(o), _, _) => Some(o.clone()),
        (None, true, _) => Some(input.clone()),
        (None, false, true) => None,
        (None, false, false) => {
            eprintln!("pdfcer: optimize: name an output with -o, or pass --in-place or --dry-run");
            return USAGE;
        }
    };
    if output.as_deref() == Some(input.as_path()) && !args.in_place {
        eprintln!("pdfcer: optimize: -o names the input; pass --in-place to replace it");
        return USAGE;
    }
    run(
        input,
        output.filter(|_| !args.dry_run).as_deref(),
        &settings,
        args.target_size,
        &args.visual,
        &args.compat,
        args.json,
    )
}

fn presets_text() -> String {
    let mut out = String::new();
    for p in Preset::ALL {
        let _ = writeln!(out, "{:<10} {}", p.as_str(), preset_summary(p));
    }
    out
}

fn preset_summary(p: Preset) -> &'static str {
    match p {
        Preset::Lossless => "lossless only: recompress generic streams at level 9",
        Preset::Standard => "the default; images above 225 ppi to 150, quality 60",
        Preset::Screen => "smallest for on-screen reading; images above 144 ppi to 96 as JPEG",
        Preset::Print => "keeps print resolution; images above 450 ppi to 300",
        Preset::Archive => "PDF/A-safe; lossless, images kept",
        _ => "",
    }
}

fn resolve_settings(args: &OptimizeArgs) -> Result<OptimizeSettings, String> {
    let mut settings = match (&args.preset, &args.profile) {
        (Some(name), _) => Preset::from_name(name)
            .ok_or_else(|| format!("unknown preset {name:?}; --list-presets lists them"))?
            .settings(),
        (None, Some(path)) => load_profile(path)?,
        (None, None) => Preset::Standard.settings(),
    };
    if let Some(a) = &args.streams {
        // On top of a preset, image settings must not drop its
        // recompression of every other stream.
        match pdfcer_core::optimize::StreamAction::parse(a)? {
            pdfcer_core::optimize::StreamAction::Image(patch) => settings.images = patch,
            pdfcer_core::optimize::StreamAction::Content(patch) => settings.content = Some(patch),
            other => settings.streams = other,
        }
    }
    if let Some(spec) = &args.content {
        let spec = if spec.trim_start().starts_with("content(") {
            spec.clone()
        } else {
            format!("content({spec})")
        };
        settings.content = Some(pdfcer_core::optimize::ContentPatch::parse(&spec)?);
    }
    for spec in &args.rules {
        settings
            .rules
            .push(profile::parse_rule(spec).map_err(|e| e.to_string())?);
    }
    for spec in &args.objects {
        let (id, action) = profile::parse_object(spec).map_err(|e| e.to_string())?;
        settings.objects.insert(id, action);
    }
    if matches!(
        args.compression,
        Some(Compression::Structure | Compression::Leave)
    ) {
        settings.streams = pdfcer_core::optimize::StreamAction::Leave;
    }
    settings.structure.object_streams &= args.compression != Some(Compression::Leave);
    settings.allow_signed |= args.invalidate_signatures;
    let st = &mut settings.structure;
    st.remove_unreferenced &= !(args.keep_structure || args.keep_unreferenced);
    st.merge_duplicates &= !(args.keep_structure || args.keep_duplicates);
    st.object_streams &= !(args.keep_structure || args.no_object_streams);
    st.linearize |= args.linearize;
    let c = &mut settings.cleanup;
    c.invalid_links &= !args.keep_invalid_links;
    c.invalid_bookmarks &= !args.keep_invalid_bookmarks;
    c.unreferenced_destinations = (c.unreferenced_destinations
        || args.remove_unreferenced_destinations)
        && !args.keep_unreferenced_destinations;
    for name in &args.discard {
        settings.discard = settings
            .discard
            .with_named(name)
            .ok_or_else(|| unknown(name))?;
    }
    if let Some(t) = args.smooth_tolerance {
        if !(t.is_finite() && t > 0.0) {
            return Err(format!(
                "--smooth-tolerance must be a positive number of points, not {t}"
            ));
        }
        settings.smooth_tolerance = t;
    }
    for name in &args.keep_data {
        settings.discard = settings
            .discard
            .without_named(name)
            .ok_or_else(|| unknown(name))?;
    }
    crate::optimize_fonts::apply(&mut settings, &args.fonts);
    Ok(settings)
}

/// The error for a `--discard` / `--keep-data` name nothing answers to.
fn unknown(name: &str) -> String {
    let known: Vec<&str> = DiscardOptions::known_names().collect();
    format!("unknown data kind {name:?}; one of: {}", known.join(", "))
}

fn load_profile(path: &Path) -> Result<OptimizeSettings, String> {
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let text = read(path)?;
    let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut resolve = |name: &str| read(&base.join(name));
    profile::parse(&path.display().to_string(), &text, &mut resolve).map_err(|e| e.to_string())
}

fn write_profile(path: &Path, settings: &OptimizeSettings) -> Result<(), u8> {
    let text = profile::to_profile(settings);
    if path.as_os_str() == "-" {
        print!("{text}");
        return Ok(());
    }
    write_output(path, text).map_err(|e| {
        eprintln!("pdfcer: {}: {e}", path.display());
        exit::IO_ERROR
    })
}

/// What one run produced, for the report.
struct Outcome {
    plan: OptimizePlan,
    original: u64,
    rewritten: u64,
    /// The file to write, or `None` when it was neither smaller nor removed
    /// anything.
    bytes: Option<Vec<u8>>,
    /// Whether the rewrite is strictly smaller than the input.
    smaller: bool,
    /// What the structure rebuild did, when one ran.
    structure: Option<RebuildReport>,
    /// The `--target-size` search, when one ran.
    budget: Option<BudgetSummary>,
    /// The `--visual-diff` comparison, when one ran.
    visual: Option<crate::optimize_visual_diff::VisualSummary>,
    /// The `--compat` feature scan, when a target was set.
    compat: Option<crate::optimize_compat::CompatSummary>,
}

fn run(
    input: &Path,
    output: Option<&Path>,
    settings: &OptimizeSettings,
    target: Option<u64>,
    visual: &crate::optimize_visual_diff::VisualDiffArgs,
    compat: &crate::optimize_compat::CompatArgs,
    json: bool,
) -> u8 {
    let doc = match open_document(input) {
        Ok(d) => d,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let before = space_audit::audit(&doc);
    let mut settings = settings.clone();
    settings.compatibility = compat.resolve(doc.version());
    let session = EditSession::new(doc);
    let outcome = match target {
        Some(max) => fit_budget(session, &settings, max),
        None => optimize(session, &settings),
    };
    let mut outcome = match outcome {
        Ok(o) => o,
        Err(code) => return code,
    };
    if visual.requested()
        && let Ok(doc) = open_document(input)
    {
        outcome.visual = Some(crate::optimize_visual_diff::compare(
            visual,
            &doc,
            outcome.bytes.as_deref(),
        ));
    }
    let after_doc = match &outcome.bytes {
        Some(b) => open_document_bytes(b.clone()).ok(),
        None => open_document(input).ok(),
    };
    if let (Some(t), Some(d)) = (settings.compatibility, &after_doc) {
        outcome.compat = Some(crate::optimize_compat::summarize(t, d));
    }
    let after = after_doc
        .filter(|_| outcome.bytes.is_some())
        .map(|d| space_audit::audit(&d));
    let diff = after.as_ref().map(|a| space_audit::diff(&before, a));
    print!(
        "{}",
        if json {
            report_json(&outcome, output, diff.as_ref())
        } else {
            report_text(&outcome, output, diff.as_ref())
        }
    );
    let Some(bytes) = &outcome.bytes else {
        if outcome.budget.as_ref().is_some_and(|b| !b.met) {
            return exit::OVER_BUDGET;
        }
        return exit::NOT_SMALLER;
    };
    if let Some(path) = output
        && let Err(e) = write_output(path, bytes)
    {
        eprintln!("pdfcer: {}: {e}", path.display());
        return exit::IO_ERROR;
    }
    if outcome.budget.as_ref().is_some_and(|b| !b.met) {
        return exit::OVER_BUDGET;
    }
    exit::SUCCESS
}

fn optimize(mut session: EditSession, settings: &OptimizeSettings) -> Result<Outcome, u8> {
    let plan = session.plan_optimize(settings).map_err(|e| refused(&e))?;
    session.apply_optimize(&plan).map_err(|e| refused(&e))?;
    let (file, structure) = session
        .save_optimized(&plan, &SaveOptions::default(), &settings.structure)
        .map_err(|e| {
            eprintln!("pdfcer: optimize: {e}");
            exit::SAVE_REFUSED
        })?;
    Ok(outcome(plan, file, structure, None))
}

/// `optimize --target-size`: the least lossy budget step that fits.
fn fit_budget(
    mut session: EditSession,
    settings: &OptimizeSettings,
    max: u64,
) -> Result<Outcome, u8> {
    let fit = session
        .optimize_to_budget(settings, max, &SaveOptions::default())
        .map_err(|e| match e {
            BudgetError::Optimize(e) => refused(&e),
            e => {
                eprintln!("pdfcer: optimize: {e}");
                exit::SAVE_REFUSED
            }
        })?;
    let summary = BudgetSummary::of(&fit, max);
    Ok(outcome(fit.plan, fit.file, fit.structure, Some(summary)))
}

fn outcome(
    plan: OptimizePlan,
    file: OptimizedFile,
    structure: Option<RebuildReport>,
    budget: Option<BudgetSummary>,
) -> Outcome {
    let original = plan.file_bytes;
    let (rewritten, bytes) = match file {
        OptimizedFile::Smaller(bytes) => (bytes.len() as u64, Some(bytes)),
        OptimizedFile::NotSmaller { bytes, .. } => (bytes.len() as u64, Some(bytes)),
        OptimizedFile::KeptOriginal { rewritten, .. } => (rewritten, None),
        _ => (original, None),
    };
    Outcome {
        plan,
        original,
        smaller: rewritten < original,
        rewritten,
        bytes,
        structure,
        budget,
        visual: None,
        compat: None,
    }
}

fn refused(err: &OptimizeError) -> u8 {
    eprintln!("pdfcer: optimize: {err}");
    if let OptimizeError::Refused(list) = err {
        for r in list {
            match r {
                Refusal::Signed { .. } => eprintln!(
                    "pdfcer: hint: --invalidate-signatures optimizes anyway; every signature stops verifying"
                ),
                Refusal::Encrypted => eprintln!(
                    "pdfcer: hint: decrypt it, optimize the copy, then encrypt the result"
                ),
                _ => {}
            }
        }
    }
    exit::EDIT_REFUSED
}

fn report_text(
    o: &Outcome,
    output: Option<&Path>,
    diff: Option<&space_audit::SpaceDiff>,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "planned: {} stream(s) rewritten, {} B saved in stream data",
        o.plan.changes.len(),
        o.plan.stream_saving()
    );
    out.push_str(&optimize_report::changes_text(&o.plan));
    out.push_str(&cleanup_text(&o.plan.cleanup));
    out.push_str(&discard_text(&o.plan.discard));
    out.push_str(&discard_objects_text(&o.plan.discard_objects));
    out.push_str(&crate::optimize_fonts::fonts_text(&o.plan));
    out.push_str(&crate::optimize_fonts::resubset_text(&o.plan));
    for n in &o.plan.notes {
        let _ = writeln!(out, "note: {n}");
    }
    if let Some(v) = o.plan.version_raise() {
        let _ = writeln!(
            out,
            "version: raised to PDF {v} in the catalog's /Version for the filter it needs; version-raise=refuse keeps the version"
        );
    }
    if let Some(r) = &o.structure {
        out.push_str(&optimize_report::structure_text(r));
    }
    match (&o.bytes, output) {
        (None, _) => {
            let _ = writeln!(
                out,
                "not written: the rewrite would be {} B, not smaller than the input's {} B",
                o.rewritten, o.original
            );
        }
        (Some(_), Some(path)) => {
            let _ = writeln!(
                out,
                "wrote {} ({} -> {} B)",
                path.display(),
                o.original,
                o.rewritten
            );
        }
        (Some(_), None) => {
            let _ = writeln!(
                out,
                "dry run: the output would be {} -> {} B",
                o.original, o.rewritten
            );
        }
    }
    if o.bytes.is_some() && !o.smaller {
        out.push_str("not smaller: kept anyway, because it removes what was asked for\n");
    }
    if let Some(b) = &o.budget {
        out.push_str(&crate::optimize_budget_cmd::budget_text(b));
    }
    if let Some(v) = &o.visual {
        out.push_str(&crate::optimize_visual_diff::visual_text(v));
    }
    if let Some(c) = &o.compat {
        out.push_str(&crate::optimize_compat::compat_text(c));
    }
    if let Some(d) = diff {
        out.push('\n');
        out.push_str(&diff_text(d));
    }
    out
}

fn report_json(
    o: &Outcome,
    output: Option<&Path>,
    diff: Option<&space_audit::SpaceDiff>,
) -> String {
    let q = |s: &str| format!("\"{}\"", json_escape(s));
    let changes: Vec<String> = o
        .plan
        .changes
        .iter()
        .map(|c| {
            let filters: Vec<String> = c.filters_before.iter().map(|f| q(f)).collect();
            format!(
                "{{\"id\":{},\"category\":{},\"filters_before\":[{}],\"action\":{},\"before_bytes\":{},\"after_bytes\":{},\"lossy\":{},\"image\":{},\"content\":{}}}",
                q(&c.id.to_string()),
                q(c.category.as_str()),
                filters.join(","),
                q(&c.action.to_string()),
                c.before_bytes,
                c.after_bytes,
                c.lossy,
                c.image.as_ref().map_or_else(|| "null".to_owned(), optimize_report::image_json),
                c.content.as_ref().map_or_else(|| "null".to_owned(), optimize_report::content_json)
            )
        })
        .collect();
    let skipped: Vec<String> = o
        .plan
        .skipped
        .iter()
        .map(|s| {
            format!(
                "{{\"id\":{},\"action\":{},\"reason\":{}}}",
                q(&s.id.to_string()),
                q(&s.action.to_string()),
                q(&s.reason.to_string())
            )
        })
        .collect();
    let notes: Vec<String> = o.plan.notes.iter().map(|n| q(n)).collect();
    let written = match (&o.bytes, output) {
        (Some(_), Some(p)) => q(&p.display().to_string()),
        _ => "null".to_owned(),
    };
    format!(
        "{{\"original_bytes\":{},\"rewritten_bytes\":{},\"smaller\":{},\"written\":{written},\"changes\":[{}],\"skipped\":[{}],\"notes\":[{}],\"version_raise\":{},\"cleanup\":{},\"discard\":{},\"discard_objects\":{},\"fonts\":{},\"resubset\":{},\"structure\":{},\"budget\":{},\"visual_diff\":{},\"compatibility\":{},\"diff\":{}}}\n",
        o.original,
        o.rewritten,
        o.smaller,
        changes.join(","),
        skipped.join(","),
        notes.join(","),
        o.plan
            .version_raise()
            .map_or("null".to_owned(), |v| format!("\"{v}\"")),
        cleanup_json(&o.plan.cleanup),
        discard_json(&o.plan.discard),
        discard_objects_json(&o.plan.discard_objects),
        crate::optimize_fonts::fonts_json(&o.plan),
        crate::optimize_fonts::resubset_json(&o.plan),
        o.structure
            .as_ref()
            .map_or_else(|| "null".to_owned(), optimize_report::structure_json),
        o.budget.as_ref().map_or_else(
            || "null".to_owned(),
            crate::optimize_budget_cmd::budget_json
        ),
        o.visual.as_ref().map_or_else(
            || "null".to_owned(),
            crate::optimize_visual_diff::visual_json
        ),
        o.compat
            .as_ref()
            .map_or_else(|| "null".to_owned(), crate::optimize_compat::compat_json),
        diff.map_or_else(|| "null".to_owned(), |d| diff_json(d).trim_end().to_owned())
    )
}
