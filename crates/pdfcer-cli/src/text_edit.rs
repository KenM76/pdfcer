use super::*;

/// Parse the `--target` selector of `edit-text` (`Pass 119.0`).
///
/// Accepted: `auto`, `page`, `form:N`. A bad value returns the message to
/// print rather than an error type, because there is exactly one caller and
/// what it needs is a sentence naming the accepted spellings — a refusal that
/// only says "invalid value" makes the operator go looking for the help text.
///
/// # Errors
///
/// The operator-facing message, ready to print.
pub(crate) fn parse_edit_target(raw: &str) -> Result<pdfcer_core::text_edit::EditTarget, String> {
    use pdfcer_core::text_edit::EditTarget;
    match raw {
        "auto" => Ok(EditTarget::Auto),
        "page" => Ok(EditTarget::PageContents),
        other => match other.strip_prefix("form:") {
            Some(digits) => digits.parse::<u32>().map(|object| EditTarget::Form { object }).map_err(|_| {
                format!("--target {other:?} names no object number -- use form:N where N is a form XObject's object number (pdfcer inspect --forms lists them)")
            }),
            None => Err(format!(
                "--target {other:?} is not a target -- use auto (the page's content then every form it paints), page (the page's own content only), or form:N"
            )),
        },
    }
}

/// Arguments for [`cmd_edit_text`], grouped so the handler stays under the
/// clippy `too_many_arguments` bound (same pattern as `RedactMarkArgs`).
pub(crate) struct EditTextArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) output: &'a Path,
    /// 1-based page number.
    pub(crate) page: usize,
    pub(crate) find: &'a str,
    /// `--pin-span START:LEN`, unparsed. Parsed inside `cmd_edit_text` so a
    /// malformed span fails before any file is opened.
    pub(crate) pin_span: Option<&'a str>,
    /// `--span-from-pin`: let `find` BEGIN at the pinned operator and run on,
    /// rather than being confined to it (`Pass 272.0`). Clap already refuses
    /// it without a pin, so the handler does not re-check.
    pub(crate) span_from_pin: bool,
    pub(crate) replace: &'a str,
    pub(crate) pin: bool,
    pub(crate) font_dirs: &'a [PathBuf],
    /// `--augment-subset`'s `(--augment-check, --augment-hinting)`, or `None`
    /// without the flag.
    pub(crate) augment: Option<(&'a str, &'a str)>,
    /// `--sibling-fonts`.
    pub(crate) sibling_fonts: bool,
    /// `--cid-font-program`.
    pub(crate) cid_font_program: pdfcer_core::text_edit::CidFontProgram,
    /// `--fallback-font NAME`.
    pub(crate) fallback_font: Option<&'a str>,
    /// `--fallback-font-file PATH`.
    pub(crate) fallback_font_file: Option<&'a Path>,
    /// `--workaround`: apply the workaround a refusal offers (also on when
    /// the settings file says `workarounds = always`).
    pub(crate) workaround: bool,
    /// The `--target` selector, unparsed. Parsed inside the handler so a
    /// malformed value is a named refusal with the accepted spellings printed,
    /// rather than a clap error that only says "invalid value".
    pub(crate) target: &'a str,
}

/// `edit-text`: Pass 14.1 in-place text editing.
///
/// Locates `--find` on `--page` — inside one show operator, or (`Pass 256.0`) across CONSECUTIVE show operators that share font resource, size and baseline, the shape a producer writes when it emits one glyph per operator, re-encodes
/// `--replace` in that run's OWN font encoding (inverting `/Encoding`, never
/// `/ToUnicode` — §9.6.6 / `iso32000__ref__inverse_encoding.md`), preserves
/// the §9.4.4 advance so un-edited text stays put, relayouts the line
/// (reflow by default; `--pin` compensates instead), and saves
/// INCREMENTALLY. The font-on-edit gate REFUSES by name any character the
/// run's font cannot provide (rule 4 / R71) — a refusal is a clean, named
/// non-zero exit ([`exit::EDIT_REFUSED`]), never a crash. All disclosures
/// (three-trust-level, incremental/prior-text, tagged-stale, relayout
/// overflow, R-INV-5 ambiguity) are surfaced verbatim.
pub(crate) fn cmd_edit_text(args: &EditTextArgs<'_>) -> u8 {
    // The shell owns font discovery (R61): `--font-dir` supplies operator
    // faces for a NON-embedded run's preview/coverage (decision 012).
    let (font_env, supplied_registered, font_notes) = build_font_environment(args.font_dirs);
    for note in &font_notes {
        eprintln!("pdfcer: font-dir: {note}");
    }

    let pin_span = match edit_text_pin(args) {
        Ok(span) => span,
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

    if args.page == 0 {
        eprintln!("pdfcer: --page is 1-based; 0 is not a valid page number");
        return exit::EDIT_REFUSED;
    }
    let target = match parse_edit_target(args.target) {
        Ok(t) => t,
        Err(message) => {
            eprintln!("pdfcer: edit-text refused: {message}");
            return exit::EDIT_REFUSED;
        }
    };
    let mut req =
        pdfcer_core::text_edit::EditRequest::find_replace(args.page - 1, args.find, args.replace)
            .with_target(target);
    if let Some(span) = pin_span {
        req.pinned_span = Some(span);
        // Clap refuses `--span-from-pin` without a pin, so this never sets a
        // flag the resolver would ignore.
        req.span_from_pin = args.span_from_pin;
    }
    let apply_workaround =
        args.workaround || crate::settings::workarounds() == crate::settings::Workarounds::Always;
    let opts = match edit_text_options(args, &font_env, apply_workaround) {
        Ok(o) => o,
        Err(code) => return code,
    };

    let outcome = match pdfcer_core::text_edit::edit_text(&doc, &req, &opts) {
        Ok(o) => o,
        Err(err) => return edit_text_error_exit(&err, apply_workaround),
    };

    if let Err(err) = write_output(args.output, &outcome.bytes) {
        eprintln!("pdfcer: {}: {err}", args.output.display());
        return exit::IO_ERROR;
    }
    print_edit_text_report(args, &outcome.report, &font_env, supplied_registered);
    exit::SUCCESS
}

/// Parse `--pin-span` before any file I/O, and refuse an empty `--find` with no
/// pin here rather than in core, so the message names the FLAG the operator
/// would have to add, which core cannot know about.
fn edit_text_pin(args: &EditTextArgs<'_>) -> Result<Option<pdfcer_core::span::ByteSpan>, u8> {
    let pin_span = match args.pin_span {
        Some(spec) => match parse_pin_span(spec) {
            Ok(span) => Some(span),
            Err(msg) => {
                eprintln!("pdfcer: {msg}");
                return Err(exit::EDIT_REFUSED);
            }
        },
        None => None,
    };
    if args.find.is_empty() && pin_span.is_none() {
        eprintln!(
            "pdfcer: edit-text needs --find TEXT, or --pin-span START:LEN with an empty \
             --find to mean the whole pinned show operator"
        );
        return Err(exit::EDIT_REFUSED);
    }
    Ok(pin_span)
}

fn edit_text_options(
    args: &EditTextArgs<'_>,
    font_env: &pdfcer_render::FontEnvironment,
    apply_workaround: bool,
) -> Result<pdfcer_core::text_edit::EditOptions, u8> {
    use pdfcer_core::text_edit::{EditOptions, FollowerDisposition, WorkaroundPolicy};
    let opts = EditOptions::default()
        .with_disposition(if args.pin {
            FollowerDisposition::Pin
        } else {
            FollowerDisposition::Reflow
        })
        .with_embedded_glyphs(&pdfcer_render::font::embedded_glyphs::EmbeddedProgramGlyphs)
        .with_sibling_fonts(args.sibling_fonts)
        .with_cid_font_program(args.cid_font_program)
        .with_workarounds(if apply_workaround {
            WorkaroundPolicy::Apply
        } else {
            WorkaroundPolicy::Refuse
        });
    let opts = match args.augment {
        Some((check, hinting)) => {
            opts.with_subset_augment(subset_augment(font_env, check, hinting))
        }
        None => opts,
    };
    let fallback_font =
        crate::fallback_font::effective_name(args.fallback_font, args.fallback_font_file);
    let opts = match crate::fallback_font::fallback_face(
        fallback_font,
        args.fallback_font_file,
        args.replace,
    ) {
        Ok(Some(face)) => opts.with_fallback(face),
        Ok(None) if fallback_font == Some(crate::fallback_font::AUTO) => {
            opts.with_replacement_faces(crate::fallback_font::installed_faces(args.font_dirs))
        }
        Ok(None) => opts,
        Err(code) => return Err(code),
    };
    Ok(opts)
}

/// Print the refusal (and the `--workaround` hint) and map it to an exit code.
fn edit_text_error_exit(err: &pdfcer_core::text_edit::EditError, apply_workaround: bool) -> u8 {
    use pdfcer_core::text_edit::EditError;
    eprintln!("pdfcer: edit-text refused: {err}");
    if !apply_workaround && err.workaround().is_some() {
        eprintln!("pdfcer: re-run with --workaround to apply it");
    }
    match err {
        EditError::Refused(_)
        | EditError::NoMatch { .. }
        | EditError::Unsupported(_)
        | EditError::PageIndex(_)
        | EditError::WorkaroundRefused { .. }
        | EditError::Encrypted => exit::EDIT_REFUSED,
        EditError::Write(_) => exit::SAVE_REFUSED,
        EditError::Content(_) | EditError::PageTree(_) => exit::RUNTIME_ERROR,
        _ => exit::RUNTIME_ERROR,
    }
}

fn print_edit_text_report(
    args: &EditTextArgs<'_>,
    report: &pdfcer_core::text_edit::EditReport,
    font_env: &pdfcer_render::FontEnvironment,
    supplied_registered: usize,
) {
    use pdfcer_core::text_edit::EditGlyphSource;
    println!(
        "edit-text {} -> {}",
        args.input.display(),
        args.output.display()
    );
    println!(
        "  page={} find={:?} replace={:?}",
        args.page, args.find, args.replace
    );
    println!(
        "  base_font={} content_object={} advance_delta={:.3} followers_repositioned={} operators_spanned={}",
        report.base_font,
        report.content_object,
        report.advance_delta,
        report.followers_repositioned,
        report.operators_spanned
    );
    // `disposition` is what the surgery settled on, not an echo of `--pin`;
    // `extra_objects_emptied` discloses a `/Contents` collapse the operator
    // did not ask for.
    println!(
        "  disposition={:?} extra_objects_emptied={}",
        report.disposition, report.extra_objects_emptied
    );
    // A form XObject may be painted from several pages; the invocation is the
    // commit, so the fan-out is printed beside the success.
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

    // Refine the core's Embedded/NonEmbedded into the three decision-012
    // trust levels via the ONE shared classifier on `FontEnvironment`
    // (Pass 14.3 §7 hoist): a NON-embedded run is Supplied when a `--font-dir`
    // face is registered for its name, else Bundled (shapes only; positions
    // still come from `/Widths`).
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
    crate::fallback_font::print_fallback(report);
    if let Some(used) = &report.workaround {
        println!("  workaround={}", used.workaround.label());
    }
    println!("  disclosures:");
    for d in &report.disclosures {
        println!("    - {d}");
    }
}

/// Named arguments for [`cmd_add_text`] (grouped to dodge clippy's
/// `too_many_arguments`, matching [`EditTextArgs`]).
pub(crate) struct AddTextArgs<'a> {
    pub(crate) input: &'a Path,
    /// `--layer` / `--layer-id` (`Pass 358.5`): the layer what is added
    /// goes on, or `None` for none.
    pub(crate) layer: Option<LayerPick>,
    /// `--hand-signature`: the signature field what is added is the hand
    /// signature for, or `None`.
    pub(crate) hand_signature: Option<&'a str>,
    pub(crate) output: &'a Path,
    /// 1-based page number.
    pub(crate) page: usize,
    /// POINT mode origin `"x,y"` in points, or `None` in boxed mode.
    pub(crate) at: Option<&'a str>,
    /// BOXED mode rectangle `"x,y,w,h"` in points, or `None` in point mode.
    pub(crate) wrap_box: Option<&'a str>,
    /// BOXED mode alignment keyword, or `None` (defaults to left).
    pub(crate) align: Option<&'a str>,
    /// BOXED mode leading (points), or `None` for the derived default.
    pub(crate) leading: Option<f64>,
    pub(crate) text: &'a str,
    /// Standard-14 `BaseFont` name or `auto`.
    pub(crate) font: &'a str,
    pub(crate) size: f64,
    /// `"r,g,b"` fill colour, or `None` for black.
    pub(crate) color: Option<&'a str>,
    /// `--render-mode`, `0..=7`.
    pub(crate) render_mode: u8,
    pub(crate) font_dirs: &'a [PathBuf],
    /// Path to a donor font file to SUBSET AND EMBED.
    ///
    /// `None` keeps the shipped R79 behaviour: a Standard-14 face written by
    /// name with no embedding. Embedding is never inferred from anything
    /// else — not from `--font-dir`, not from the text containing non-Latin
    /// characters — because R108 makes it an explicit per-action choice, and
    /// an "I noticed you needed this" default is exactly the silent
    /// file-size and font-redistribution change that rule exists to prevent.
    pub(crate) embed_font: Option<&'a Path>,
}

/// `add-text`: Pass 16.0 add NEW page text (decision 016 / FF-D).
///
/// Synthesizes a single-line `BT…ET` run at `--at "x,y"` in the chosen
/// Standard-14 face and APPENDS it as a new content stream (ISO 32000-1
/// §7.7.3.3), leaving every ORIGINAL content stream byte-identical.
/// No glyph embedding: the run is written by `/BaseFont` name + code, so
/// a character the face cannot represent is REFUSED by name (the F-refuse gate,
/// R71) — a clean, named non-zero exit ([`exit::EDIT_REFUSED`]), never a crash
/// or a faked glyph. This is genuine page content, NOT a `/FreeText`
/// annotation: the result is editable/formattable/reflowable like the
/// page's own text. The save is INCREMENTAL. Font provenance
/// (`Bundled`/`Supplied`), the tagged-untagged disclosure, and the
/// inheritance-safe `/Resources` note (§7.7.3.4) are surfaced verbatim.
pub(crate) fn cmd_add_text(args: &AddTextArgs<'_>) -> u8 {
    use pdfcer_core::fontdata::std14_base_font_name;
    use pdfcer_core::text_edit::{AddTextRequest, FontProvenance};

    if args.page == 0 {
        eprintln!("pdfcer: --page is 1-based; 0 is not a valid page number");
        return exit::EDIT_REFUSED;
    }
    // Every flag is parsed before any document work, so a typo fails cleanly.
    let placement = match add_text_placement(args) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let (font, color) =
        match add_text_font(args.font).and_then(|f| add_text_color(args.color).map(|c| (f, c))) {
            Ok(fc) => fc,
            Err(code) => return code,
        };
    // A `--font-dir` face registered for the name lifts the disclosed
    // provenance to `Supplied` (decision 012); the written dict is the same.
    let (font_env, supplied_registered, font_notes) = build_font_environment(args.font_dirs);
    for note in &font_notes {
        eprintln!("pdfcer: font-dir: {note}");
    }
    let provenance = match font_env.classify_nonembedded(std14_base_font_name(font)) {
        pdfcer_render::GlyphSource::Supplied => FontProvenance::Supplied,
        _ => FontProvenance::Bundled,
    };
    let doc = match std::fs::read(args.input) {
        Ok(source) => match open_document_bytes(source) {
            Ok(d) => d,
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", args.input.display());
                return exit_code_for_doc(&err);
            }
        },
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.input.display());
            return exit::IO_ERROR;
        }
    };
    let origin = match placement {
        AddTextPlacement::Point { origin } => origin,
        AddTextPlacement::Boxed { .. } => (0.0, 0.0),
    };
    let mut req = AddTextRequest::new(args.page - 1, origin, args.text.to_owned())
        .with_font(font)
        .with_provenance(provenance)
        .with_size(args.size)
        .with_color(color);
    if let Some(donor_path) = args.embed_font {
        req = match add_text_embed(req, donor_path, args.text) {
            Ok(r) => r,
            Err(code) => return code,
        };
    }
    if let AddTextPlacement::Boxed {
        rect: (x, y, w, h),
        align,
    } = placement
    {
        req = req
            .with_box(x, y, w, h)
            .with_alignment(align)
            .with_leading(args.leading);
    }
    req = req.with_render_mode(args.render_mode);
    let (report, session_outcome) = match add_text_run(args, &doc, req) {
        Ok(r) => r,
        Err(code) => return code,
    };
    print_add_text_report(args, &placement, &report, supplied_registered);
    match &session_outcome {
        Some(outcome) => finish_edit(args.input, outcome),
        None => exit::SUCCESS,
    }
}

/// Where `add-text` puts its run: exactly one of `--at` or `--box`.
enum AddTextPlacement {
    Point {
        origin: (f64, f64),
    },
    Boxed {
        rect: (f64, f64, f64, f64),
        align: pdfcer_core::text_edit::BlockAlignment,
    },
}

/// Parse `--at` / `--box` / `--align`, or the exit code after printing why not.
fn add_text_placement(args: &AddTextArgs<'_>) -> Result<AddTextPlacement, u8> {
    match (args.at, args.wrap_box) {
        (Some(_), Some(_)) => {
            eprintln!("pdfcer: --at and --box are mutually exclusive; pass exactly one");
            Err(exit::EDIT_REFUSED)
        }
        (None, None) => {
            eprintln!(
                "pdfcer: add-text needs a placement — pass --at \"x,y\" (point text) or \
                 --box \"x,y,w,h\" (boxed, wrapped text)"
            );
            Err(exit::EDIT_REFUSED)
        }
        (Some(at), None) => parse_at_pair(at)
            .map(|origin| AddTextPlacement::Point { origin })
            .ok_or_else(|| {
                eprintln!(
                    "pdfcer: --at expects two comma-separated numbers \"x,y\" (points), got {at:?}"
                );
                exit::EDIT_REFUSED
            }),
        (None, Some(bx)) => {
            let rect = parse_box_quad(bx).ok_or_else(|| {
                eprintln!(
                    "pdfcer: --box expects four comma-separated numbers \"x,y,w,h\" \
                     (points), got {bx:?}"
                );
                exit::EDIT_REFUSED
            })?;
            let align = parse_block_align(args.align)?;
            Ok(AddTextPlacement::Boxed { rect, align })
        }
    }
}

/// `--font`: `auto` is Helvetica (decision 016 §3.3), otherwise an exact
/// §9.6.2.2 Standard-14 spelling.
fn add_text_font(name: &str) -> Result<pdfcer_core::fontdata::Std14, u8> {
    use pdfcer_core::fontdata::{Std14, std14_by_base_font};
    if name.eq_ignore_ascii_case("auto") {
        return Ok(Std14::Helvetica);
    }
    std14_by_base_font(name).ok_or_else(|| {
        eprintln!(
            "pdfcer: --font {name:?} is not a Standard-14 BaseFont name \
             (e.g. Helvetica, Times-Roman, Courier-Bold, Symbol, ZapfDingbats)"
        );
        exit::EDIT_REFUSED
    })
}

/// `--color "r,g,b"`, black when absent.
fn add_text_color(raw: Option<&str>) -> Result<pdfcer_core::text_edit::NewTextColor, u8> {
    use pdfcer_core::text_edit::NewTextColor;
    let Some(s) = raw else {
        return Ok(NewTextColor::Black);
    };
    parse_rgb_triple(s)
        .map(|(r, g, b)| NewTextColor::Rgb(r, g, b))
        .ok_or_else(|| {
            eprintln!(
                "pdfcer: --color expects three comma-separated components in 0..=1 \
                 \"r,g,b\", got {s:?}"
            );
            exit::EDIT_REFUSED
        })
}

/// `--embed-font` (FF-C, decision 021): subset the donor now and print the
/// measured result before anything is written (R108/R98).
fn add_text_embed(
    req: pdfcer_core::text_edit::AddTextRequest,
    donor_path: &Path,
    text: &str,
) -> Result<pdfcer_core::text_edit::AddTextRequest, u8> {
    let plan = subset_donor(donor_path, text, "add-text")?;
    println!(
        "embedding a subset of '{}': {} glyph(s), {} byte(s) of font program, covering {} character(s) — subset tag {}",
        plan.base_name,
        plan.glyphs.len(),
        plan.program.len(),
        distinct_chars(text).len(),
        plan.subset_tag
    );
    Ok(req.with_embedded_face(plan))
}

/// Run the add: through a session when `--layer` or `--hand-signature` needs
/// its undoable marking writes, else one-shot and written here.
fn add_text_run(
    args: &AddTextArgs<'_>,
    doc: &pdfcer_core::document::Document,
    req: pdfcer_core::text_edit::AddTextRequest,
) -> Result<(pdfcer_core::text_edit::AddTextReport, Option<EditOutcome>), u8> {
    if args.layer.is_some() || args.hand_signature.is_some() {
        let (report, outcome) = add_text_in_session(args, req)?;
        return Ok((report, Some(outcome)));
    }
    let outcome = pdfcer_core::text_edit::add_text(doc, &req).map_err(|err| {
        eprintln!("pdfcer: add-text refused: {err}");
        add_text_exit(&err)
    })?;
    write_output(args.output, &outcome.bytes).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", args.output.display());
        exit::IO_ERROR
    })?;
    Ok((outcome.report, None))
}

fn print_add_text_report(
    args: &AddTextArgs<'_>,
    placement: &AddTextPlacement,
    report: &pdfcer_core::text_edit::AddTextReport,
    supplied_registered: usize,
) {
    use pdfcer_core::text_edit::FontProvenance;
    println!(
        "add-text {} -> {}",
        args.input.display(),
        args.output.display()
    );
    match placement {
        AddTextPlacement::Point { origin } => println!(
            "  mode=point page={} at={},{} text={:?}",
            args.page, origin.0, origin.1, args.text
        ),
        AddTextPlacement::Boxed {
            rect: (x, y, w, h),
            align,
        } => println!(
            "  mode=boxed page={} box={x},{y},{w},{h} align={} text={:?}",
            args.page,
            align.as_str(),
            args.text
        ),
    }
    if let Some(n) = report.wrapped_lines {
        println!(
            "  wrapped_lines={n} box_overflow_lines={} page_overflow_pt={:.2}",
            report.box_overflow_lines, report.page_overflow_pt
        );
    }
    let provenance = match report.provenance {
        FontProvenance::Bundled => "Bundled",
        FontProvenance::Supplied => "Supplied",
        _ => "unknown",
    };
    println!(
        "  base_font={} provenance={provenance} font_resource=/{} size={}",
        report.base_font, report.font_resource_name, args.size
    );
    println!(
        "  content_object={} font_object={} gave_page_own_resources={} tagged_untagged={} \
         supplied_registered={}",
        report.content_object,
        report.font_object,
        report.gave_page_own_resources,
        report.tagged_untagged,
        supplied_registered
    );
    if let Some(pick) = &args.layer {
        match (&pick.name, pick.id) {
            (Some(name), _) => println!("  layer={name:?}"),
            (None, Some(id)) => println!("  layer_id={id}"),
            (None, None) => {}
        }
    }
    if let Some(field) = args.hand_signature {
        println!("  hand_signature={field:?}");
    }
    println!("  disclosures:");
    for d in &report.disclosures {
        println!("    - {d}");
    }
}

/// `add-text` through an edit session, for the marks only a session can
/// write (`--layer`, `--hand-signature`); saved incrementally.
fn add_text_in_session(
    args: &AddTextArgs<'_>,
    mut req: pdfcer_core::text_edit::AddTextRequest,
) -> Result<(pdfcer_core::text_edit::AddTextReport, EditOutcome), u8> {
    let (source, mut session) = open_for_edit(args.input)?;
    if let Some(layer) = resolve_add_layer(args.input, &session, args.layer.as_ref())? {
        req = req.on_layer(layer);
    }
    if let Some(field) = args.hand_signature {
        req = req.with_hand_signature(field);
    }
    let report = session.add_text(&req).map_err(|err| {
        eprintln!("pdfcer: add-text refused: {err}");
        add_text_exit(&err)
    })?;
    let outcome = save_edited(
        &mut session,
        &source,
        args.output,
        SaveMode::Incremental,
        ProducerArg::Preserve,
        false,
    )?;
    Ok((report, outcome))
}

/// The exit code for an `add-text` refusal.
fn add_text_exit(err: &pdfcer_core::text_edit::AddTextError) -> u8 {
    use pdfcer_core::text_edit::AddTextError;
    match err {
        AddTextError::Refused(_)
        | AddTextError::PageIndex(_)
        | AddTextError::EmptyText
        | AddTextError::InvalidRenderMode { .. }
        | AddTextError::InvalidSize(_)
        | AddTextError::InvalidBox(..)
        | AddTextError::NoWordsToWrap
        | AddTextError::Encrypted
        | AddTextError::CertificationForbidsChange { .. }
        | AddTextError::HiddenObjects { .. }
        | AddTextError::ObjectNumbersExhausted
        // Both FF-C refusals are operator-facing: something about the
        // request cannot be honoured, and the operator can change it. The
        // `_ => RUNTIME_ERROR` catch-all would tell a script pdfcer crashed.
        | AddTextError::EmbeddedBoxedUnsupported
        | AddTextError::EmbeddedPlanIncomplete { .. }
        | AddTextError::Embed(_)
        | AddTextError::LayerNeedsSession
        | AddTextError::HandSignatureNeedsSession
        | AddTextError::Layer(_)
        | AddTextError::HandSignature(_)
        | AddTextError::Unsupported(_) => exit::EDIT_REFUSED,
        AddTextError::Write(_) => exit::SAVE_REFUSED,
        _ => exit::RUNTIME_ERROR,
    }
}

/// Named arguments for [`cmd_place_text`] (grouped to dodge clippy's
/// `too_many_arguments`, matching [`AddTextArgs`]).
pub(crate) struct PlaceTextArgs<'a> {
    /// The plain-text file to import.
    pub(crate) text_file: &'a Path,
    pub(crate) output: &'a Path,
    /// The PDF to insert into, or `None` to have pdfcer create the document.
    pub(crate) input: Option<&'a Path>,
    /// `end` | `start` | `before:N` | `after:N`, N 1-based.
    pub(crate) position: &'a str,
    /// Named sheet size id (`letter`, `a4`, …).
    pub(crate) paper: &'a str,
    pub(crate) landscape: bool,
    /// Explicit `"W,H"` sheet size in points, overriding `paper`/`landscape`.
    pub(crate) page_size: Option<&'a str>,
    /// `(all, left, right, top, bottom)` — the per-side values override `all`.
    ///
    /// A tuple rather than five fields because they are one input with one
    /// resolution rule, and splitting them invites a caller to apply four of
    /// them and forget the fifth.
    pub(crate) margins: (f64, Option<f64>, Option<f64>, Option<f64>, Option<f64>),
    /// Standard-14 `BaseFont` name or `auto`.
    pub(crate) font: &'a str,
    pub(crate) size: f64,
    /// Leading in points, or `None` for the derived `1.2 x size`.
    pub(crate) leading: Option<f64>,
    /// Alignment keyword, or `None` (defaults to left).
    pub(crate) align: Option<&'a str>,
    /// `"r,g,b"` fill colour, or `None` for black.
    pub(crate) color: Option<&'a str>,
    /// Place the text and drop what the face cannot encode, instead of
    /// refusing the whole import.
    pub(crate) drop_unmappable: bool,
    pub(crate) mode: SaveMode,
    pub(crate) producer: ProducerArg,
}

/// `place-text`: import a plain-text file as PDF pages.
///
/// The batch half of `EditSession::place_text` (rule 11 — every feature ships
/// its `pdfcer` equivalent in the same session as the engine verb). The
/// operator's real input is a `.txt` on disk, so this reads a file rather than
/// taking a `--text` string the way `add-text` does: a shell that has to
/// inline a 40 KB document into an argument list has not been given a batch
/// tool.
///
/// ## Two shapes, one verb
///
/// With `--input`, pages are inserted into that document at `--position`.
/// Without it there is no document, and `EditSession::place_text` deliberately
/// refuses to insert beside nothing — so this builds a ONE-page scaffold with
/// `blank_document`, imports after it, and deletes the scaffold. That is three
/// engine calls rather than a fourth code path, and the CLI is the right place
/// for it: the invocation IS the commit here (rule 11), so the extra undo entry
/// the delete costs is not observable, whereas a "create a document" mode
/// inside the engine verb would be.
///
/// ## What it prints
///
/// Every field of the report, in `key=value` form for a script, then the
/// verbatim disclosures. The counts that matter most are the ones describing
/// what did NOT survive the import — dropped characters, collapsed tabs,
/// blank pages — because those are the ones nothing in the output file can
/// tell the operator (rule 4).
pub(crate) fn cmd_place_text(args: &PlaceTextArgs<'_>) -> u8 {
    // Everything the operator typed is validated before any file is read.
    let (template, media, align) = match place_text_template(args) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let text = match std::fs::read_to_string(args.text_file) {
        Ok(t) => t,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.text_file.display());
            return exit::IO_ERROR;
        }
    };
    let creating = args.input.is_none();
    let (source, mut session) = match place_text_session(args.input, media) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let position = match place_text_position(args.position, creating) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let report = match session.place_text(&text, &template, position) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("pdfcer: place-text refused: {err}");
            return match err {
                pdfcer_core::text_edit::PlaceTextError::Scaffold(_) => exit::RUNTIME_ERROR,
                _ => exit::EDIT_REFUSED,
            };
        }
    };
    if creating && let Err(code) = remove_scaffold_page(&mut session) {
        return code;
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        args.producer,
        false,
    ) {
        Ok(o) => o,
        Err(code) => return code,
    };
    print_place_text_report(args, &report, align, creating);
    finish_edit(args.text_file, &outcome)
}

/// The page template from `--page-size`/`--paper`, margins, font, size,
/// leading, alignment, colour and `--drop-unmappable`.
fn place_text_template(
    args: &PlaceTextArgs<'_>,
) -> Result<
    (
        pdfcer_core::text_edit::PageTemplate,
        pdfcer_core::page_tree::Rect,
        pdfcer_core::text_edit::BlockAlignment,
    ),
    u8,
> {
    use pdfcer_core::text_edit::{PageTemplate, Unmappable};
    let media = place_text_media(args)?;
    let font = place_text_font(args.font)?;
    let align = parse_block_align(args.align)?;
    let color = add_text_color(args.color)?;
    let (all, left, right, top, bottom) = args.margins;
    let template = PageTemplate::new()
        .with_media_box(media)
        .with_margins(
            left.unwrap_or(all),
            right.unwrap_or(all),
            top.unwrap_or(all),
            bottom.unwrap_or(all),
        )
        .with_font(font)
        .with_size(args.size)
        .with_leading(args.leading)
        .with_alignment(align)
        .with_color(color)
        .with_unmappable(if args.drop_unmappable {
            Unmappable::Drop
        } else {
            Unmappable::Refuse
        });
    Ok((template, media, align))
}

/// The sheet: an explicit `--page-size "W,H"`, else the named `--paper`.
fn place_text_media(args: &PlaceTextArgs<'_>) -> Result<pdfcer_core::page_tree::Rect, u8> {
    use pdfcer_core::page_tree::Rect;
    use pdfcer_core::paper::{Orientation, PaperSize};
    if let Some(s) = args.page_size {
        return match parse_at_pair(s) {
            Some((w, h)) if w > 0.0 && h > 0.0 => Ok(Rect::from_corners(0.0, 0.0, w, h)),
            _ => {
                eprintln!(
                    "pdfcer: --page-size expects two positive comma-separated numbers \"W,H\" \
                     (points), got {s:?}"
                );
                Err(exit::EDIT_REFUSED)
            }
        };
    }
    let orientation = if args.landscape {
        Orientation::Landscape
    } else {
        Orientation::Portrait
    };
    PaperSize::from_id(args.paper)
        .map(|p| p.rect_with(orientation))
        .ok_or_else(|| {
            eprintln!(
                "pdfcer: --paper {:?} is not a known sheet size (letter, legal, a0..a6, \
                 tabloid, executive, ansi-a..ansi-e)",
                args.paper
            );
            exit::EDIT_REFUSED
        })
}

/// `--font` for `place-text`: `auto` is Helvetica, otherwise a Standard-14
/// spelling.
fn place_text_font(name: &str) -> Result<pdfcer_core::fontdata::Std14, u8> {
    use pdfcer_core::fontdata::{Std14, std14_by_base_font};
    if name.eq_ignore_ascii_case("auto") {
        return Ok(Std14::Helvetica);
    }
    std14_by_base_font(name).ok_or_else(|| {
        eprintln!(
            "pdfcer: --font {name:?} is not a Standard-14 BaseFont name \
             (e.g. Helvetica, Times-Roman, Courier-Bold)"
        );
        exit::EDIT_REFUSED
    })
}

/// `--align`, left when absent.
fn parse_block_align(raw: Option<&str>) -> Result<pdfcer_core::text_edit::BlockAlignment, u8> {
    use pdfcer_core::text_edit::BlockAlignment;
    let Some(s) = raw else {
        return Ok(BlockAlignment::Left);
    };
    BlockAlignment::parse(s).ok_or_else(|| {
        eprintln!("pdfcer: --align {s:?}: expected left|center|right|justify");
        exit::EDIT_REFUSED
    })
}

/// The document to import into: `--input`, or a one-page scaffold to splice
/// beside (see [`cmd_place_text`] for why the engine verb does not do this).
fn place_text_session(
    input: Option<&Path>,
    media: pdfcer_core::page_tree::Rect,
) -> Result<(Vec<u8>, pdfcer_core::edit::EditSession), u8> {
    if let Some(input) = input {
        return open_for_edit(input);
    }
    let doc = pdfcer_core::text_edit::blank_document(media, 1).map_err(|err| {
        eprintln!("pdfcer: could not create a document: {err}");
        exit::RUNTIME_ERROR
    })?;
    let bytes = doc.bytes().to_vec();
    Ok((bytes, pdfcer_core::edit::EditSession::new(doc)))
}

/// `--position`, through the same parser `insert-pages --at` uses; a created
/// document always imports after its scaffold.
fn place_text_position(
    raw: &str,
    creating: bool,
) -> Result<pdfcer_core::pageops::InsertPosition, u8> {
    if creating {
        return Ok(pdfcer_core::pageops::InsertPosition::End);
    }
    parse_insert_position(raw).map_err(|message| {
        eprintln!("pdfcer: --position {raw:?}: {message}");
        exit::EDIT_REFUSED
    })
}

/// Delete the scaffold, page 0 (`End` imported after it), so a created
/// document holds exactly the pages the text needed.
fn remove_scaffold_page(session: &mut pdfcer_core::edit::EditSession) -> Result<(), u8> {
    match session.delete_pages(&[0]) {
        Ok(out) if out.pages_removed == 1 => Ok(()),
        Ok(out) => {
            eprintln!(
                "pdfcer: internal: removing the scaffold page removed {} page(s), not 1. \
                 This is a bug; refusing rather than writing a document with a stray page",
                out.pages_removed
            );
            Err(exit::RUNTIME_ERROR)
        }
        Err(err) => {
            eprintln!("pdfcer: internal: the scaffold page could not be removed: {err}");
            Err(exit::RUNTIME_ERROR)
        }
    }
}

fn print_place_text_report(
    args: &PlaceTextArgs<'_>,
    report: &pdfcer_core::text_edit::PlaceTextReport,
    align: pdfcer_core::text_edit::BlockAlignment,
    creating: bool,
) {
    println!(
        "place-text {} -> {}",
        args.text_file.display(),
        args.output.display()
    );
    println!(
        "  pages_created={} first_page={} blank_pages={} lines_placed={} lines_per_page={}",
        report.pages_created,
        // 1-based, on the finished document: a created one has lost its scaffold.
        if creating {
            1
        } else {
            report.first_page_index + 1
        },
        report.blank_pages,
        report.lines_placed,
        report.lines_per_page
    );
    println!(
        "  chars_input={} chars_placed={} whitespace_normalised={} controls_dropped={} \
         unmappable_dropped={}",
        report.chars_input,
        report.chars_placed,
        report.whitespace_normalised,
        report.chars_dropped_control,
        report.chars_dropped_unmappable
    );
    println!(
        "  bom_stripped={} crlf_normalised={} tabs_collapsed={} page_breaks={} \
         overlong_words={} paragraphs_split={}",
        report.bom_stripped,
        report.crlf_normalised,
        report.tabs_collapsed,
        report.explicit_page_breaks,
        report.overlong_words,
        report.paragraphs_split_across_pages
    );
    println!(
        "  leading={:.2}{} alignment={} box_overflow_lines={} undo_entries={} coalesced={}",
        report.leading,
        if report.leading_derived {
            " (derived)"
        } else {
            ""
        },
        align.as_str(),
        report.box_overflow_lines,
        report.undo_entries,
        report.coalesced
    );
    if !report.dropped_unmappable_chars.is_empty() {
        // Named, not just counted, so the operator can act on it.
        let named: Vec<String> = report
            .dropped_unmappable_chars
            .iter()
            .map(|(c, n)| format!("U+{:04X} x{n}", *c as u32))
            .collect();
        println!("  dropped_characters: {}", named.join(", "));
    }
    if creating && args.mode == SaveMode::Incremental {
        eprintln!(
            "pdfcer: {}: pdfcer created this document, so its base revision is the one blank \
scaffold page the import was placed beside. Under --mode incremental that page's object stays in \
the file's revision history (ISO 32000-1 §7.5.6 appends; it does not erase). --mode full writes \
the finished document without it.",
            args.output.display()
        );
    }
    println!("  disclosures:");
    for d in &report.disclosures {
        println!("    - {d}");
    }
}

/// Parse `"x,y"` into two `f64` points, or `None` on any malformed input.
pub(crate) fn parse_at_pair(s: &str) -> Option<(f64, f64)> {
    let (x, y) = s.split_once(',')?;
    let x: f64 = x.trim().parse().ok()?;
    let y: f64 = y.trim().parse().ok()?;
    if x.is_finite() && y.is_finite() {
        Some((x, y))
    } else {
        None
    }
}

/// Parse `"x,y,w,h"` into four `f64` points for the boxed add, or `None` on
/// any malformed input. Width/height positivity is enforced by the core
/// ([`pdfcer_core::text_edit::AddTextError::InvalidBox`]); this only checks the
/// shape and finiteness so a typo fails before any document work.
pub(crate) fn parse_box_quad(s: &str) -> Option<(f64, f64, f64, f64)> {
    let mut it = s.split(',');
    let x: f64 = it.next()?.trim().parse().ok()?;
    let y: f64 = it.next()?.trim().parse().ok()?;
    let w: f64 = it.next()?.trim().parse().ok()?;
    let h: f64 = it.next()?.trim().parse().ok()?;
    if it.next().is_some() {
        return None; // too many components
    }
    if [x, y, w, h].iter().all(|v| v.is_finite()) {
        Some((x, y, w, h))
    } else {
        None
    }
}

/// Parse `"r,g,b"` into three `f64` components (each finite; clamped to
/// `0..=1` by the core), or `None` on any malformed input.
pub(crate) fn parse_rgb_triple(s: &str) -> Option<(f64, f64, f64)> {
    let mut it = s.split(',');
    let r: f64 = it.next()?.trim().parse().ok()?;
    let g: f64 = it.next()?.trim().parse().ok()?;
    let b: f64 = it.next()?.trim().parse().ok()?;
    if it.next().is_some() {
        return None; // too many components
    }
    if [r, g, b].iter().all(|v| v.is_finite()) {
        Some((r, g, b))
    } else {
        None
    }
}

/// `reflow`: Pass 15.1 within-block reflow surgery.
///
/// Recognises the block model on `--page` (with first-line-indent splitting
/// relaxed, matching `inspect --reflow-preview`), re-wraps the paragraph
/// `--block` under the requested width/alignment/leading via the 14.1
/// advance-preserving machinery, and saves INCREMENTALLY — only the block's
/// own content-stream object changes. Every gate (a composite/CJK block, a
/// rotated/skewed or shared/non-contiguous block, a missing-provenance or
/// bad-index/width condition) is a clean, named non-zero exit
/// ([`exit::EDIT_REFUSED`]), never a crash. All disclosures (derived-layout,
/// justify, page-overflow-emitted-not-clipped, tagged-stale, incremental/
/// prior-text) are surfaced verbatim.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_reflow(
    input: &Path,
    page: usize,
    block: usize,
    width: Option<f64>,
    align: Option<&str>,
    leading: Option<f64>,
    output: &Path,
) -> u8 {
    use pdfcer_core::text_edit::{BlockAlignment, ReflowRequest, apply_reflow};

    // Parse the alignment override up front so a typo fails cleanly before any
    // document work (the R27 fail-clean posture; identical to the preview
    // path's parse).
    let align_override = match align {
        None => None,
        Some(s) => match BlockAlignment::parse(s) {
            Some(a) => Some(a),
            None => {
                eprintln!(
                    "pdfcer: {}: --align {s}: expected left|right|center|justified",
                    input.display()
                );
                return exit::EDIT_REFUSED;
            }
        },
    };

    if page == 0 {
        eprintln!("pdfcer: --page is 1-based; 0 is not a valid page number");
        return exit::EDIT_REFUSED;
    }

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

    let req = ReflowRequest::new()
        .with_wrap_width_opt(width)
        .with_alignment_opt(align_override)
        .with_leading_opt(leading);

    let outcome = match apply_reflow(&doc, page - 1, block, &req) {
        Ok(o) => o,
        Err(err) => {
            eprintln!("pdfcer: reflow refused: {err}");
            return reflow_error_exit(&err);
        }
    };

    if let Err(err) = write_output(output, &outcome.bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    print_reflow_report(input, output, page, block, &outcome.report);
    exit::SUCCESS
}

/// Print the recoverable-refusal hint and map a reflow error to its exit code.
fn reflow_error_exit(err: &pdfcer_core::text_edit::ReflowApplyError) -> u8 {
    use pdfcer_core::text_edit::ReflowApplyError;
    // Rule 4: the invocation IS the commit here, so what pdfcer knows
    // about the operator's options is printed on the way past rather
    // than being available to ask for. `is_recoverable()` is the
    // engine's own answer -- not this shell's reading of the sentence
    // above, which is exactly the coupling `pdfcer-gui` refused.
    if err.is_recoverable() {
        eprintln!(
            "pdfcer: this one you CAN clear -- save the document and reopen it, then reflow. \
             Every other reason reflow declines is a property of how the page was drawn."
        );
    }
    // A refusal is a clean named non-zero; a save/runtime failure is a
    // distinct class. The `_` arm keeps this exhaustive as
    // `ReflowApplyError` grows (it is `#[non_exhaustive]`).
    //
    // `PageEditedThisSession` is listed EXPLICITLY rather than left
    // to the `_` arm, which would have called it a RUNTIME_ERROR. It
    // is a refusal -- the cleanest, most recoverable one there is --
    // and a new variant silently inheriting the catch-all is how a
    // correct engine change becomes a wrong exit code.
    match err {
        ReflowApplyError::Refused(_)
        | ReflowApplyError::Preview(_)
        | ReflowApplyError::NoProvenance
        | ReflowApplyError::Unsupported(_)
        | ReflowApplyError::PageEditedThisSession
        | ReflowApplyError::PageIndex(_)
        | ReflowApplyError::Encrypted => exit::EDIT_REFUSED,
        ReflowApplyError::Write(_) => exit::SAVE_REFUSED,
        ReflowApplyError::Extract(_)
        | ReflowApplyError::Content(_)
        | ReflowApplyError::PageTree(_) => exit::RUNTIME_ERROR,
        _ => exit::RUNTIME_ERROR,
    }
}

fn print_reflow_report(
    input: &Path,
    output: &Path,
    page: usize,
    block: usize,
    report: &pdfcer_core::text_edit::ReflowApplyReport,
) {
    println!("reflow {} -> {}", input.display(), output.display());
    println!(
        "  page={page} block={block} align={} lines_before={} lines_after={} \
justified_lines={} height_delta={:.1}",
        report.alignment.as_str(),
        report.lines_before,
        report.lines_after,
        report.justified_lines,
        report.height_delta,
    );
    println!(
        "  base_font={} glyph_source={} content_object={}",
        report.base_font,
        match report.glyph_source {
            pdfcer_core::text_edit::EditGlyphSource::Embedded => "Embedded",
            _ => "NonEmbedded",
        },
        report.content_object,
    );
    if let Some(ov) = report.overflow {
        println!(
            "  overflow: past_bottom={:.1}pt lines_outside={} (EMITTED off-page, not clipped)",
            ov.past_bottom_pt, ov.lines_outside
        );
    }
    if let Some(co) = report.cell_overflow {
        println!(
            "  cell_overflow: past_bottom={:.1}pt past_right={:.1}pt lines_outside={} (cell not resized)",
            co.past_bottom_pt, co.past_right_pt, co.lines_outside
        );
    }
    if let Some(mcid) = report.tagged_mcid {
        println!("  tagged_mcid={mcid}");
    }
    println!("  disclosures:");
    for d in &report.disclosures {
        println!("    - {d}");
    }
}

/// Subset the donor at `path` for the characters of `find` (`format-text
/// --embed-font`), or the exit code after printing why not.
pub(crate) fn donor_plan(
    path: &std::path::Path,
    find: &str,
) -> Result<pdfcer_core::font_embed::FontEmbedPlan, u8> {
    if find.is_empty() {
        eprintln!(
            "pdfcer: format-text refused: --embed-font needs --find, because the subset is built for exactly the characters of the run"
        );
        return Err(exit::EDIT_REFUSED);
    }
    subset_donor(path, find, "format-text")
}

/// Subset the donor at `path` for the distinct characters of `text`, or the
/// exit code after printing why not (`verb` names the refusing command).
/// The tag derives from the file stem (§9.6.4), so repeated runs over one
/// font are byte-reproducible.
fn subset_donor(
    path: &Path,
    text: &str,
    verb: &str,
) -> Result<pdfcer_core::font_embed::FontEmbedPlan, u8> {
    let donor = std::fs::read(path).map_err(|e| {
        eprintln!("pdfcer: cannot read the font file {}: {e}", path.display());
        exit::IO_ERROR
    })?;
    let stem = path.file_stem().map_or_else(
        || "EmbeddedFont".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    );
    let tag = pdfcer_render::font::subset::subset_tag_for(&stem);
    pdfcer_render::font::subset::plan_subset(&donor, 0, &distinct_chars(text), &stem, &tag).map_err(
        |e| {
            eprintln!("pdfcer: {verb} refused: {e}");
            exit::EDIT_REFUSED
        },
    )
}

/// Each character of `text` once, sorted: a subset plan reports coverage
/// gaps against exactly what it was asked for.
fn distinct_chars(text: &str) -> Vec<char> {
    let mut wanted: Vec<char> = text.chars().collect();
    wanted.sort_unstable();
    wanted.dedup();
    wanted
}

/// One `--X` / `--no-X` flag pair as a [`StyleTarget`] axis; clap's
/// `conflicts_with` keeps both from being set.
///
/// [`StyleTarget`]: pdfcer_core::text_edit::StyleTarget
pub(crate) const fn axis_target(on: bool, off: bool) -> Option<bool> {
    if on {
        Some(true)
    } else if off {
        Some(false)
    } else {
        None
    }
}

/// Rung-3 candidates for `format-text --embed-styled-face` (`Pass 142.3`):
/// every TrueType face in `font_dirs` whose PostScript name claims every
/// axis asked on and none asked off, subset for the characters of `find`.
///
/// Only the axis claim is checked here, to avoid subsetting a whole system
/// font folder; which candidate is of the run's family and carries exactly
/// the right style is decided by the ladder in core, the one selector. A face
/// that cannot be subset (CFF, licence, missing glyph) is skipped with a
/// `font-dir:` note.
pub(crate) fn style_donor_plans(
    font_dirs: &[PathBuf],
    find: &str,
    style: pdfcer_core::text_edit::StyleTarget,
) -> Result<Vec<pdfcer_core::font_embed::FontEmbedPlan>, u8> {
    use pdfcer_core::text_edit::synth::{name_claims_bold, name_claims_italic};
    use pdfcer_render::font::program::FontProgram;

    if style.is_keep() {
        eprintln!(
            "pdfcer: format-text refused: --embed-styled-face needs --bold, --italic, --no-bold \
             or --no-italic"
        );
        return Err(exit::EDIT_REFUSED);
    }
    if find.is_empty() {
        eprintln!(
            "pdfcer: format-text refused: --embed-styled-face needs --find, because the \
             subset is built for exactly the characters of the run"
        );
        return Err(exit::EDIT_REFUSED);
    }
    let mut wanted: Vec<char> = find.chars().collect();
    wanted.sort_unstable();
    wanted.dedup();

    let mut plans = Vec::new();
    for dir in font_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue; // `build_font_environment` already reported it.
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && has_font_extension(p))
            .collect();
        files.sort();
        for path in files {
            if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_FONT_FILE_BYTES) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Some(name) = FontProgram::parse(&bytes)
                .ok()
                .and_then(|p| p.face_names().into_iter().next())
            else {
                continue;
            };
            let fits = |want: Option<bool>, claims: bool| want.is_none_or(|w| w == claims);
            if !(fits(style.bold, name_claims_bold(&name))
                && fits(style.italic, name_claims_italic(&name)))
            {
                continue;
            }
            let tag = pdfcer_render::font::subset::subset_tag_for(&name);
            match pdfcer_render::font::subset::plan_subset(&bytes, 0, &wanted, &name, &tag) {
                Ok(plan) => plans.push(plan),
                Err(e) => eprintln!(
                    "pdfcer: font-dir: {} ({name}) cannot be a style donor: {e}",
                    path.display()
                ),
            }
        }
    }
    Ok(plans)
}

/// `--augment-subset`'s settings over the `--font-dir` faces. The augmenter
/// is leaked because core holds it as `&'static`; the process runs one edit.
fn subset_augment(
    env: &pdfcer_render::FontEnvironment,
    check: &str,
    hinting: &str,
) -> pdfcer_core::text_edit::SubsetAugment {
    use pdfcer_core::text_edit::{HintingMismatch, OutlineCheck, SubsetAugment};
    let faces = pdfcer_render::font::InstalledFaceAugmenter::from_environment(env);
    SubsetAugment::new(Box::leak(Box::new(faces)))
        .with_outline_check(if check == "shown-only" {
            OutlineCheck::ShownOnly
        } else {
            OutlineCheck::AllShared
        })
        .with_hinting_mismatch(if hinting == "refuse" {
            HintingMismatch::Refuse
        } else {
            HintingMismatch::Strip
        })
}
