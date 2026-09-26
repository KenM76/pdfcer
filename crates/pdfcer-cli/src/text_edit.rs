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
    use pdfcer_core::text_edit::{
        EditError, EditGlyphSource, EditOptions, EditRequest, FollowerDisposition,
    };

    // The shell owns font discovery (R61): `--font-dir` supplies operator
    // faces for a NON-embedded run's preview/coverage (decision 012).
    let (font_env, supplied_registered, font_notes) = build_font_environment(args.font_dirs);
    for note in &font_notes {
        eprintln!("pdfcer: font-dir: {note}");
    }

    // Parse the pin before any file I/O, and refuse an empty `--find` with no
    // pin here rather than in core, so the message names the FLAG the
    // operator would have to add — which core cannot know about.
    let pin_span = match args.pin_span {
        Some(spec) => match parse_pin_span(spec) {
            Ok(span) => Some(span),
            Err(msg) => {
                eprintln!("pdfcer: {msg}");
                return exit::EDIT_REFUSED;
            }
        },
        None => None,
    };
    if args.find.is_empty() && pin_span.is_none() {
        eprintln!(
            "pdfcer: edit-text needs --find TEXT, or --pin-span START:LEN with an empty \
             --find to mean the whole pinned show operator"
        );
        return exit::EDIT_REFUSED;
    }

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
        EditRequest::find_replace(args.page - 1, args.find, args.replace).with_target(target);
    if let Some(span) = pin_span {
        req.pinned_span = Some(span);
        // Only meaningful with a pin, and clap already refuses the flag
        // without one (`requires = "pin_span"`), so this cannot silently
        // set a flag the resolver would then ignore.
        req.span_from_pin = args.span_from_pin;
    }
    let opts = EditOptions::default().with_disposition(if args.pin {
        FollowerDisposition::Pin
    } else {
        FollowerDisposition::Reflow
    });

    let outcome = match pdfcer_core::text_edit::edit_text(&doc, &req, &opts) {
        Ok(o) => o,
        Err(err) => {
            eprintln!("pdfcer: edit-text refused: {err}");
            return match err {
                EditError::Refused(_)
                | EditError::NoMatch(_)
                | EditError::Unsupported(_)
                | EditError::PageIndex(_)
                | EditError::Encrypted => exit::EDIT_REFUSED,
                EditError::Write(_) => exit::SAVE_REFUSED,
                EditError::Content(_) | EditError::PageTree(_) => exit::RUNTIME_ERROR,
                _ => exit::RUNTIME_ERROR,
            };
        }
    };

    if let Err(err) = std::fs::write(args.output, &outcome.bytes) {
        eprintln!("pdfcer: {}: {err}", args.output.display());
        return exit::IO_ERROR;
    }

    let report = &outcome.report;
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
    // The disposition ACTUALLY USED and the sibling-stream collapse count.
    // Both were computed and printed by nobody until `check-outcome-disclosed`
    // was pointed at `EditReport` -- which it had never covered, because its
    // struct list was written from `edit.rs` alone and every report type in a
    // submodule sat outside it while the summary line read "clean".
    //
    // `disposition` is not merely an echo of `--pin`: it is what the surgery
    // settled on, and a run that cannot be pinned reports the difference here
    // rather than leaving the operator to infer it from the geometry.
    // `extra_objects_emptied` says the page had several `/Contents` streams
    // and the edit collapsed them -- a structural change to the file that the
    // operator did not ask for and should not learn about from a diff.
    println!(
        "  disposition={:?} extra_objects_emptied={}",
        report.disposition, report.extra_objects_emptied
    );
    // `Pass 119.0`: WHERE the edit landed, and how far it reaches. In the CLI
    // the invocation IS the commit -- there is no session and no undo -- so
    // rule 11's obligation is to print what an interactive shell would show
    // off-canvas, on the way past. The fan-out line is the one that matters:
    // a form XObject may be painted from several pages and no clause binds it
    // to one, so an operator running a batch rename needs the count in the
    // same output as the success.
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
    println!("  disclosures:");
    for d in &report.disclosures {
        println!("    - {d}");
    }
    exit::SUCCESS
}

/// Named arguments for [`cmd_add_text`] (grouped to dodge clippy's
/// `too_many_arguments`, matching [`EditTextArgs`]).
pub(crate) struct AddTextArgs<'a> {
    pub(crate) input: &'a Path,
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
    /// `--render-mode`, `0..=7` (G034).
    pub(crate) render_mode: u8,
    pub(crate) font_dirs: &'a [PathBuf],
    /// Path to a donor font file to SUBSET AND EMBED (FF-C, decision 021).
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
/// §7.7.3.3), leaving every ORIGINAL content stream byte-identical (R32/R46).
/// No glyph embedding (R79): the run is written by `/BaseFont` name + code, so
/// a character the face cannot represent is REFUSED by name (the F-refuse gate,
/// R71) — a clean, named non-zero exit ([`exit::EDIT_REFUSED`]), never a crash
/// or a faked glyph. This is genuine page content, NOT a `/FreeText`
/// annotation (R78): the result is editable/formattable/reflowable like the
/// page's own text. The save is INCREMENTAL. Font provenance
/// (`Bundled`/`Supplied`), the tagged-untagged disclosure (R73), and the
/// inheritance-safe `/Resources` note (§7.7.3.4) are surfaced verbatim.
pub(crate) fn cmd_add_text(args: &AddTextArgs<'_>) -> u8 {
    use pdfcer_core::fontdata::{Std14, std14_base_font_name, std14_by_base_font};
    use pdfcer_core::text_edit::{
        AddTextError, AddTextRequest, BlockAlignment, FontProvenance, NewTextColor, add_text,
    };

    if args.page == 0 {
        eprintln!("pdfcer: --page is 1-based; 0 is not a valid page number");
        return exit::EDIT_REFUSED;
    }

    // Resolve the placement mode up front (R27 fail-clean): exactly one of
    // `--at` (point, 16.0) or `--box` (boxed, 16.1). Parsing the geometry and
    // the alignment before any document work means a typo fails cleanly.
    enum Placement {
        Point {
            origin: (f64, f64),
        },
        Boxed {
            rect: (f64, f64, f64, f64),
            align: BlockAlignment,
        },
    }
    let placement = match (args.at, args.wrap_box) {
        (Some(_), Some(_)) => {
            eprintln!("pdfcer: --at and --box are mutually exclusive; pass exactly one");
            return exit::EDIT_REFUSED;
        }
        (None, None) => {
            eprintln!(
                "pdfcer: add-text needs a placement — pass --at \"x,y\" (point text) or \
                 --box \"x,y,w,h\" (boxed, wrapped text)"
            );
            return exit::EDIT_REFUSED;
        }
        (Some(at), None) => match parse_at_pair(at) {
            Some(origin) => Placement::Point { origin },
            None => {
                eprintln!(
                    "pdfcer: --at expects two comma-separated numbers \"x,y\" (points), got {at:?}"
                );
                return exit::EDIT_REFUSED;
            }
        },
        (None, Some(bx)) => {
            let rect = match parse_box_quad(bx) {
                Some(r) => r,
                None => {
                    eprintln!(
                        "pdfcer: --box expects four comma-separated numbers \"x,y,w,h\" \
                         (points), got {bx:?}"
                    );
                    return exit::EDIT_REFUSED;
                }
            };
            let align = match args.align {
                None => BlockAlignment::Left,
                Some(s) => match BlockAlignment::parse(s) {
                    Some(a) => a,
                    None => {
                        eprintln!("pdfcer: --align {s:?}: expected left|center|right|justify");
                        return exit::EDIT_REFUSED;
                    }
                },
            };
            Placement::Boxed { rect, align }
        }
    };

    // `--font auto` = Helvetica (pdfcer's documented default; Acrobat's is a
    // GAP, decision 016 §3.3); otherwise an EXACT §9.6.2.2 spelling.
    let font = if args.font.eq_ignore_ascii_case("auto") {
        Std14::Helvetica
    } else {
        match std14_by_base_font(args.font) {
            Some(f) => f,
            None => {
                eprintln!(
                    "pdfcer: --font {:?} is not a Standard-14 BaseFont name \
                     (e.g. Helvetica, Times-Roman, Courier-Bold, Symbol, ZapfDingbats)",
                    args.font
                );
                return exit::EDIT_REFUSED;
            }
        }
    };

    let color = match args.color {
        None => NewTextColor::Black,
        Some(s) => match parse_rgb_triple(s) {
            Some((r, g, b)) => NewTextColor::Rgb(r, g, b),
            None => {
                eprintln!(
                    "pdfcer: --color expects three comma-separated components in 0..=1 \
                     \"r,g,b\", got {s:?}"
                );
                return exit::EDIT_REFUSED;
            }
        },
    };

    // The shell owns font discovery (R61): a `--font-dir` face registered for
    // the chosen name lifts the disclosed provenance to `Supplied` (decision
    // 012). The WRITTEN dict is identical either way (no embedding, R79).
    let (font_env, supplied_registered, font_notes) = build_font_environment(args.font_dirs);
    for note in &font_notes {
        eprintln!("pdfcer: font-dir: {note}");
    }
    let base_font_name = std14_base_font_name(font);
    let provenance = match font_env.classify_nonembedded(base_font_name) {
        pdfcer_render::GlyphSource::Supplied => FontProvenance::Supplied,
        _ => FontProvenance::Bundled,
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

    // The `origin` a point add uses; a boxed add supersedes it via `with_box`.
    let base_origin = match placement {
        Placement::Point { origin } => origin,
        Placement::Boxed { .. } => (0.0, 0.0),
    };
    let mut req = AddTextRequest::new(args.page - 1, base_origin, args.text.to_owned())
        .with_font(font)
        .with_provenance(provenance)
        .with_size(args.size)
        .with_color(color);

    // FF-C (decision 021 / Pass 21.0): --embed-font subsets a donor face and
    // embeds it, so the saved file carries its own glyphs.
    //
    // The subset is computed HERE, before anything is written, and its real
    // numbers are printed. That is R108/R98 applied: subsetting is a pure
    // function, so there is no reason to describe the outcome in the future
    // tense. "will add roughly N KB" is a prediction; "added 11,240 bytes for
    // 14 glyph(s)" is a measurement, and only one of them can be wrong.
    if let Some(donor_path) = args.embed_font {
        let donor = match std::fs::read(donor_path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!(
                    "pdfcer: cannot read the font file {}: {e}",
                    donor_path.display()
                );
                return exit::IO_ERROR;
            }
        };
        // Deduplicated and ordered: the plan only needs each distinct
        // character once, and `plan_subset` reports coverage gaps against
        // exactly what it was asked for — passing "AAB" would otherwise
        // report 'A' missing twice.
        let mut wanted: Vec<char> = args.text.chars().collect();
        wanted.sort_unstable();
        wanted.dedup();

        let stem = donor_path.file_stem().map_or_else(
            || "EmbeddedFont".to_owned(),
            |s| s.to_string_lossy().into_owned(),
        );
        // A subset tag must be exactly six uppercase ASCII letters (§9.6.4).
        // Derived from the face name so repeated runs over the same font are
        // reproducible — a random tag would make byte-comparison of two
        // otherwise identical outputs impossible, which the round-trip
        // harness depends on.
        let tag = pdfcer_render::font::subset::subset_tag_for(&stem);

        let plan = match pdfcer_render::font::subset::plan_subset(&donor, 0, &wanted, &stem, &tag) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("pdfcer: add-text refused: {e}");
                return exit::EDIT_REFUSED;
            }
        };

        // One line, no continuation. A `\` line-continuation inside the
        // format string looked tidier in source and printed a run of
        // spaces to the terminal, because the leading indentation of the
        // continued line is only stripped when nothing follows the
        // backslash. Readable source is not worth unreadable output.
        println!(
            "embedding a subset of '{}': {} glyph(s), {} byte(s) of font program, covering {} character(s) — subset tag {}",
            plan.base_name,
            plan.glyphs.len(),
            plan.program.len(),
            wanted.len(),
            plan.subset_tag
        );
        req = req.with_embedded_face(plan);
    }
    if let Placement::Boxed {
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
    let outcome = match add_text(&doc, &req) {
        Ok(o) => o,
        Err(err) => {
            eprintln!("pdfcer: add-text refused: {err}");
            return match err {
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
                // Both FF-C refusals are operator-facing: something about
                // the request cannot be honoured, and the operator can
                // change it. They belong with the refusals, not in the
                // `_ => RUNTIME_ERROR` catch-all, which would have told a
                // script that pdfcer had crashed rather than declined.
                | AddTextError::EmbeddedBoxedUnsupported
                | AddTextError::EmbeddedPlanIncomplete { .. }
                | AddTextError::Embed(_)
                | AddTextError::Unsupported(_) => exit::EDIT_REFUSED,
                AddTextError::Write(_) => exit::SAVE_REFUSED,
                AddTextError::PageTree(_) => exit::RUNTIME_ERROR,
                _ => exit::RUNTIME_ERROR,
            };
        }
    };

    if let Err(err) = std::fs::write(args.output, &outcome.bytes) {
        eprintln!("pdfcer: {}: {err}", args.output.display());
        return exit::IO_ERROR;
    }

    let report = &outcome.report;
    println!(
        "add-text {} -> {}",
        args.input.display(),
        args.output.display()
    );
    match placement {
        Placement::Point { origin } => println!(
            "  mode=point page={} at={},{} text={:?}",
            args.page, origin.0, origin.1, args.text
        ),
        Placement::Boxed {
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
    println!("  disclosures:");
    for d in &report.disclosures {
        println!("    - {d}");
    }
    exit::SUCCESS
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
#[allow(
    clippy::too_many_lines,
    reason = "argument validation, the two document shapes, and the report print-out are one linear command; the validation half is a sequence of independent named refusals that reads better in order than split across helpers that each take the same args struct"
)]
pub(crate) fn cmd_place_text(args: &PlaceTextArgs<'_>) -> u8 {
    use pdfcer_core::fontdata::{Std14, std14_by_base_font};
    use pdfcer_core::page_tree::Rect;
    use pdfcer_core::paper::{Orientation, PaperSize};
    use pdfcer_core::text_edit::{
        BlockAlignment, NewTextColor, PageTemplate, PlaceTextError, Unmappable, blank_document,
    };

    // --- everything the operator typed, validated before any file is read.
    let media = match args.page_size {
        Some(s) => match parse_at_pair(s) {
            Some((w, h)) if w > 0.0 && h > 0.0 => Rect::from_corners(0.0, 0.0, w, h),
            _ => {
                eprintln!(
                    "pdfcer: --page-size expects two positive comma-separated numbers \"W,H\" \
                     (points), got {s:?}"
                );
                return exit::EDIT_REFUSED;
            }
        },
        None => match PaperSize::from_id(args.paper) {
            Some(p) => p.rect_with(if args.landscape {
                Orientation::Landscape
            } else {
                Orientation::Portrait
            }),
            None => {
                eprintln!(
                    "pdfcer: --paper {:?} is not a known sheet size (letter, legal, a0..a6, \
                     tabloid, executive, ansi-a..ansi-e)",
                    args.paper
                );
                return exit::EDIT_REFUSED;
            }
        },
    };

    let font = if args.font.eq_ignore_ascii_case("auto") {
        Std14::Helvetica
    } else {
        match std14_by_base_font(args.font) {
            Some(f) => f,
            None => {
                eprintln!(
                    "pdfcer: --font {:?} is not a Standard-14 BaseFont name \
                     (e.g. Helvetica, Times-Roman, Courier-Bold)",
                    args.font
                );
                return exit::EDIT_REFUSED;
            }
        }
    };

    let align = match args.align {
        None => BlockAlignment::Left,
        Some(s) => match BlockAlignment::parse(s) {
            Some(a) => a,
            None => {
                eprintln!("pdfcer: --align {s:?}: expected left|center|right|justify");
                return exit::EDIT_REFUSED;
            }
        },
    };

    let color = match args.color {
        None => NewTextColor::Black,
        Some(s) => match parse_rgb_triple(s) {
            Some((r, g, b)) => NewTextColor::Rgb(r, g, b),
            None => {
                eprintln!(
                    "pdfcer: --color expects three comma-separated components in 0..=1 \
                     \"r,g,b\", got {s:?}"
                );
                return exit::EDIT_REFUSED;
            }
        },
    };

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

    let text = match std::fs::read_to_string(args.text_file) {
        Ok(t) => t,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.text_file.display());
            return exit::IO_ERROR;
        }
    };

    // --- the two document shapes.
    let creating = args.input.is_none();
    let (source, mut session) = match args.input {
        Some(input) => match open_for_edit(input) {
            Ok(pair) => pair,
            Err(code) => return code,
        },
        None => {
            // One blank page to splice beside, removed again below. See this
            // function's docs for why the engine verb does not do this itself.
            let doc = match blank_document(media, 1) {
                Ok(d) => d,
                Err(err) => {
                    eprintln!("pdfcer: could not create a document: {err}");
                    return exit::RUNTIME_ERROR;
                }
            };
            let bytes = doc.bytes().to_vec();
            (bytes, pdfcer_core::edit::EditSession::new(doc))
        }
    };

    let position = if creating {
        pdfcer_core::pageops::InsertPosition::End
    } else {
        // The SAME parser `insert-pages --at` uses. A second one here would be
        // a second spelling of `before:N` that agrees today.
        match parse_insert_position(args.position) {
            Ok(p) => p,
            Err(message) => {
                eprintln!("pdfcer: --position {:?}: {message}", args.position);
                return exit::EDIT_REFUSED;
            }
        }
    };

    let report = match session.place_text(&text, &template, position) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("pdfcer: place-text refused: {err}");
            return match err {
                PlaceTextError::Scaffold(_) => exit::RUNTIME_ERROR,
                _ => exit::EDIT_REFUSED,
            };
        }
    };

    if creating {
        // The scaffold page is now the LAST page (the import went after it is
        // false — `End` inserted after it, so it is index 0). Removing it is
        // what makes `place-text` with no `--input` produce a document of
        // exactly the pages the text needed.
        match session.delete_pages(&[0]) {
            Ok(out) if out.pages_removed == 1 => {}
            Ok(out) => {
                eprintln!(
                    "pdfcer: internal: removing the scaffold page removed {} page(s), not 1. \
                     This is a bug; refusing rather than writing a document with a stray page",
                    out.pages_removed
                );
                return exit::RUNTIME_ERROR;
            }
            Err(err) => {
                eprintln!("pdfcer: internal: the scaffold page could not be removed: {err}");
                return exit::RUNTIME_ERROR;
            }
        }
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

    println!(
        "place-text {} -> {}",
        args.text_file.display(),
        args.output.display()
    );
    println!(
        "  pages_created={} first_page={} blank_pages={} lines_placed={} lines_per_page={}",
        report.pages_created,
        // 1-based for the operator, and re-based on the finished document: with
        // no `--input` the scaffold page in front of them is gone by now.
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
        // Named, not just counted: the count says something is missing, this
        // says what, which is the difference between a disclosure the operator
        // can act on and one they can only worry about.
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
    finish_edit(args.text_file, &outcome)
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
    use pdfcer_core::text_edit::{BlockAlignment, ReflowApplyError, ReflowRequest, apply_reflow};

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
            return match err {
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
            };
        }
    };

    if let Err(err) = std::fs::write(output, &outcome.bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }

    let report = &outcome.report;
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
    if let Some(mcid) = report.tagged_mcid {
        println!("  tagged_mcid={mcid}");
    }
    println!("  disclosures:");
    for d in &report.disclosures {
        println!("    - {d}");
    }
    exit::SUCCESS
}

/// Arguments for [`cmd_format_text`], grouped to stay under the clippy
/// `too_many_arguments` bound (same pattern as [`EditTextArgs`]).
pub(crate) struct FormatTextArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) output: &'a Path,
    /// 1-based page number.
    pub(crate) page: usize,
    pub(crate) find: &'a str,
    /// `--pin-span START:LEN`, unparsed. Parsed inside `cmd_format_text` so
    /// a malformed span fails before any file is opened.
    pub(crate) pin_span: Option<&'a str>,
    pub(crate) set_size: Option<f64>,
    /// `MODEL:comps` as passed on the command line, e.g. `rgb:1,0,0`.
    pub(crate) set_color: Option<&'a str>,
    /// A target font resource key or `/BaseFont`.
    pub(crate) set_font: Option<&'a str>,
    /// `--char-spacing` as passed, e.g. `0.5`, `0.5pt`, `20em`.
    pub(crate) char_spacing: Option<&'a str>,
    /// `--word-spacing` as passed, e.g. `2`, `2pt`, `200em` (Pass 19.4).
    pub(crate) word_spacing: Option<&'a str>,
    /// `--h-scale` percentage (100 = normal).
    pub(crate) h_scale: Option<f64>,
    /// `--render-mode`, `0..=7` (G034).
    pub(crate) render_mode: Option<u8>,
    /// The baseline toggle, already resolved from the three exclusive flags.
    pub(crate) script: Option<pdfcer_core::text_edit::ScriptPosition>,
    /// `--rise` as passed, e.g. `3.25`, `3.25pt`, `280em` (Pass 19.2).
    pub(crate) rise: Option<&'a str>,
    /// The synthetic styles asked for, already folded from the two flags.
    /// [`StyleSynthesis::None`] means none were, which is the default and
    /// the only state in which nothing is synthesized (R90).
    pub(crate) synthetic: pdfcer_core::text_edit::StyleSynthesis,
    /// `--bold` / `--italic`: the automatic ladder (`Pass 179.0`).
    pub(crate) style: pdfcer_core::text_edit::StyleSynthesis,
    /// `--style-policy`, or `None` to use the stored setting. Overrides the
    /// setting for this invocation only; nothing is persisted.
    pub(crate) style_policy: Option<StylePolicyArg>,
    pub(crate) pin: bool,
    /// The `--target` selector, unparsed — see `parse_edit_target`.
    pub(crate) target: &'a str,
    pub(crate) font_dirs: &'a [PathBuf],
}

/// Parse a text-space metric argument into a [`MetricSpec`]
/// (`0.5` / `0.5pt` → absolute; `20em` → 20 thousandths of an em).
///
/// `flag` is the option's own spelling (`--char-spacing`, `--word-spacing`,
/// `--rise`), used only so the error message names the flag the operator
/// actually typed instead of whichever one happened to be implemented
/// first. It is a parameter rather than three near-copies of this function
/// precisely because the grammar must not drift between the flags: `Tc`,
/// `Tw` and `Ts` are all in unscaled text-space units (§9.3 Table 105's
/// closing note) and all governed by R89's Absolute/Relative discrimination,
/// so there is exactly one set of suffixes to learn.
///
/// The `em` suffix means **‰ of an em**, not ems — the typographic tracking
/// convention, and the same unit space `TJ`'s own adjustments live in
/// (§9.4.3). That is a genuine trap, so it is spelled out in `--help`, in
/// the error text below, and in the save report's disclosure. It is also
/// self-refuting in practice: a `Tc` of 20 *ems* would be a 240 pt gap
/// between every pair of glyphs at 12 pt.
///
/// Returns a human-readable error string (surfaced on stderr) rather than
/// panicking, exactly as [`parse_set_color`] does.
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

/// Parse a `--set-color MODEL:C,..` argument into a [`NewFill`]
/// (`rgb:1,0,0`, `cmyk:0,1,1,0`, `gray:0.5`). Returns a human-readable
/// error string (surfaced on stderr) rather than panicking on bad input.
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

/// Parse a `--pin-span START:LEN` argument (`Pass 145.0`).
///
/// `START:LEN`, not `START:END`, because that is the shape the value has
/// everywhere else it exists: `ByteSpan` carries `start` + `len`, and
/// `extract-text --json --spans` emits `op_start` / `op_len`. Asking an
/// operator to convert between two spellings of the same number is a bug
/// waiting to be reported as a wrong edit.
///
/// # Errors
///
/// A message naming what was wrong with the value. Both numbers must parse
/// and `LEN` must be non-zero — a zero-length span names no operator and
/// would fail later with a location error that did not explain itself.
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

/// `format-text`: Pass 14.2 in-place formatting (size / fill colour /
/// font-family-style).
///
/// Locates `--find` on `--page` — inside one show operator, or (`Pass 256.0`) across CONSECUTIVE show operators that share font resource, size and baseline, the shape a producer writes when it emits one glyph per operator, applies the
/// requested formatting via the shared advance-preserving surgery, relayouts
/// the line (reflow by default; `--pin` compensates), and saves
/// INCREMENTALLY. Every gate — a coverage refusal on a family change, a
/// missing target font, an invalid colour, an unresolvable (outlined) run —
/// is a clean, named non-zero exit ([`exit::EDIT_REFUSED`]), never a crash.
/// All disclosures (three-trust-level, incremental/prior-state, colour
/// narrowing, tagged-stale, relayout overflow) are surfaced verbatim.
pub(crate) fn cmd_format_text(args: &FormatTextArgs<'_>) -> u8 {
    use pdfcer_core::text_edit::{
        EditGlyphSource, FollowerDisposition, FontSelector, FormatError, FormatOptions,
        FormatRequest,
    };

    // The shell owns font discovery (R61): `--font-dir` supplies operator
    // faces for a NON-embedded target's preview/trust level (decision 012).
    let (font_env, supplied_registered, font_notes) = build_font_environment(args.font_dirs);
    for note in &font_notes {
        eprintln!("pdfcer: font-dir: {note}");
    }

    // Parse the pin up front, alongside the colour, so a malformed span
    // fails before the input file is even opened.
    let pin_span = match args.pin_span {
        Some(spec) => match parse_pin_span(spec) {
            Ok(span) => Some(span),
            Err(msg) => {
                eprintln!("pdfcer: {msg}");
                return exit::EDIT_REFUSED;
            }
        },
        None => None,
    };
    // An empty `--find` means "the whole pinned operator" and is meaningless
    // without a pin. Refused here rather than in core so the message names
    // the FLAG the operator would have to add, which core cannot know.
    if args.find.is_empty() && pin_span.is_none() {
        eprintln!(
            "pdfcer: format-text needs --find TEXT, or --pin-span START:LEN with an empty \
             --find to mean the whole pinned show operator"
        );
        return exit::EDIT_REFUSED;
    }

    // Parse the colour up front so a bad spec fails before any file I/O.
    let fill = match args.set_color {
        Some(spec) => match parse_set_color(spec) {
            Ok(f) => Some(f),
            Err(msg) => {
                eprintln!("pdfcer: {msg}");
                return exit::EDIT_REFUSED;
            }
        },
        None => None,
    };
    // Likewise the character-spacing spec — a bad unit suffix must fail
    // before the input file is even opened.
    // …and the three text-space metric specs, which share ONE parser
    // because they share one unit model: `Tc`, `Tw` and `Ts` are all in
    // unscaled text-space units and all governed by R89's
    // Absolute/Relative discrimination (§9.3 Table 105's closing note).
    // One parser, one set of suffixes to learn, no chance of three
    // spellings drifting apart.
    let metric = |flag: &str, spec: Option<&str>| match spec {
        Some(s) => match parse_text_metric(flag, s) {
            Ok(m) => Ok(Some(m)),
            Err(msg) => {
                eprintln!("pdfcer: {msg}");
                Err(exit::EDIT_REFUSED)
            }
        },
        None => Ok(None),
    };
    let char_spacing = match metric("--char-spacing", args.char_spacing) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let word_spacing = match metric("--word-spacing", args.word_spacing) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let rise = match metric("--rise", args.rise) {
        Ok(v) => v,
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
            eprintln!("pdfcer: format-text refused: {message}");
            return exit::EDIT_REFUSED;
        }
    };
    let mut req = FormatRequest::new(args.page - 1, args.find).target(target);
    if let Some(span) = pin_span {
        req = req.pinned(span);
    }
    if let Some(size) = args.set_size {
        req = req.size(size);
    }
    if let Some(f) = fill {
        req = req.fill(f);
    }
    if let Some(name) = args.set_font {
        req = req.font(FontSelector::new(name));
    }
    if let Some(spec) = char_spacing {
        req = req.char_spacing(spec);
    }
    if let Some(spec) = word_spacing {
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
    if let Some(spec) = rise {
        req = req.rise(spec);
    }
    // Passing the request through even when it is `None` would be harmless,
    // but doing it explicitly keeps "nothing was asked for" and "nothing was
    // applied" the same statement (R90: never silent, never a default).
    if !args.synthetic.is_none() {
        req = req.synthetic(args.synthetic);
    }
    if !args.style.is_none() {
        req = req.style(args.style);
    }
    // The bold/italic fallback posture (`Pass 179.0`, decision 106).
    //
    // Resolved HERE, in the shell, and handed to core as a value -- the
    // established convention for every ambiguity setting, and the thing that
    // keeps an engine call from depending on machine state (`wasm32`).
    //
    // `--style-policy` overrides the stored setting for this invocation only,
    // the same shape `render-page --max-cmyk-buffer-bytes` uses. Nothing is
    // persisted by passing it.
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

    let outcome = match pdfcer_core::text_edit::set_format(&doc, &req, &opts) {
        Ok(o) => o,
        Err(err) => {
            eprintln!("pdfcer: format-text refused: {err}");
            return match err {
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
                | FormatError::ConflictingRenderMode
                | FormatError::RealFaceAvailable { .. }
                | FormatError::SynthesisRefusedByPosture { .. }
                | FormatError::ShearUnsupported(_)
                | FormatError::Encrypted => exit::EDIT_REFUSED,
                FormatError::Write(_) => exit::SAVE_REFUSED,
                FormatError::Content(_) | FormatError::PageTree(_) => exit::RUNTIME_ERROR,
                _ => exit::RUNTIME_ERROR,
            };
        }
    };

    if let Err(err) = std::fs::write(args.output, &outcome.bytes) {
        eprintln!("pdfcer: {}: {err}", args.output.display());
        return exit::IO_ERROR;
    }

    let report = &outcome.report;
    println!(
        "format-text {} -> {}",
        args.input.display(),
        args.output.display()
    );
    println!("  page={} find={:?}", args.page, args.find);
    // The formatting summary — one field per operation, all present so the
    // line is diffable even when an operation was not requested.
    let size_str = report
        .size_change
        .map_or_else(|| "none".to_owned(), |(o, n)| format!("{o}->{n}"));
    let color_str = report.fill_space.unwrap_or("none");
    let font_str = report
        .font_change
        .as_ref()
        .map_or_else(|| "none".to_owned(), |(o, n)| format!("{o}->{n}"));
    println!("  set_size={size_str} set_color={color_str} set_font={font_str}");
    // Pass 19.1's three controls, printed as `ambient->emitted` so the line
    // is diffable and so the RATIOS pdfcer chose are visible by value rather
    // than buried in the code (rule 4).
    let tc_str = report
        .char_spacing_change
        .map_or_else(|| "none".to_owned(), |(o, n)| format!("{o}->{n}"));
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
    // Pass 19.4. `word_spacing` prints `ambient->emitted` like its siblings
    // AND the number of code-32s it reaches, because "how many spaces did
    // that touch" is the single question the control's §9.3.3 scope makes
    // load-bearing — a script that widened one gap and moved four is a
    // silent surprise otherwise. A count of 0 prints as 0.
    let tw_str = report.word_spacing_change.map_or_else(
        || "none".to_owned(),
        |(o, n)| {
            let spaces = report
                .word_spacing_affected_codes
                .map_or_else(String::new, |c| format!(" spaces={c}"));
            format!("{o}->{n}{spaces}")
        },
    );
    println!("  char_spacing={tc_str} word_spacing={tw_str} h_scale={tz_str} script={script_str}");
    if let Some((o, n)) = report.render_mode_change {
        println!("  render_mode={o}->{n}");
    }
    // Pass 19.2. `rise` is printed whenever the baseline moved — by the
    // free-form control OR by the toggle — because "where is the baseline
    // now" is one question, not two. `synthesis` prints the mechanism's own
    // numbers (stroke width, shear, and the Trise x tan(theta) displacement)
    // so nothing pdfcer chose is invisible to the operator (rule 4).
    let rise_str = report
        .rise_change
        .map_or_else(|| "none".to_owned(), |(o, n)| format!("{o}->{n}"));
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
    println!("  rise={rise_str} synthesis={synth_str}");

    // THE DISCLOSURE A NON-REFUSING POSTURE OWES (`Pass 179.0`, decision
    // 106).
    //
    // Under `refuse` this situation is the error and the command already
    // failed with the sentence below. Under `auto` and `warn` the edit
    // HAPPENS, so rule 4's obligation lands here instead: the operator asked
    // to fake a weight, pdfcer did it, and a real face was sitting there. That
    // must not be silent -- "the user shouldn't have to intervene" removed the
    // GATE, not the disclosure.
    //
    // The two postures differ only in loudness and in the stream:
    //
    //   auto -> a `note:` on stdout, a reported fact beside the others
    //   warn -> a `warning:` on STDERR, so a script that only reads stdout
    //           still surfaces it and a human running the command sees it
    //           separated from the result
    //
    // The engine's own sentence is printed VERBATIM. It already names the
    // face, the resource, whether the family differs and the exact
    // `--set-font` to retry with; re-wording it here would be a second
    // description of one fact, and the two would drift.
    // `Pass 179.0`: which rung the automatic ladder took. The full sentence
    // is also among the disclosures below; this line is the machine-readable
    // summary a script keys on.
    if let Some(l) = &report.style_ladder {
        println!(
            "  style_ladder: requested={} rung={:?} bound={} synthesised={} passed_over={}",
            l.requested.axes(),
            l.rung,
            l.bound
                .as_deref()
                .map_or_else(|| "-".to_owned(), quoted_token),
            l.synthesised.axes(),
            l.passed_over.len()
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
    // `Pass 119.2`: WHERE the restyle landed and how far it reaches. Same
    // obligation as `edit-text`'s: in the CLI the invocation IS the commit, so
    // a fan-out an interactive shell would show off-canvas is printed here on
    // the way past.
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

    // Refine the core's Embedded/NonEmbedded into the three decision-012
    // trust levels (identical to `edit-text`, via the shared classifier).
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
    exit::SUCCESS
}

/// Grouped arguments for `text-run-delete` (`Pass 32.0`, `--leaf` added by
/// `G017`).
pub(crate) struct TextRunDeleteArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    /// The page paint-order index, when addressing a page object.
    pub(crate) object: Option<usize>,
    /// The index into this page's form leaves, when addressing a text object
    /// INSIDE a form XObject. Exactly one of this and `object` is set;
    /// `object_or_leaf` enforces it.
    pub(crate) leaf: Option<usize>,
    pub(crate) run: usize,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `text-run-delete` — remove one show operator from a text object
/// (`Pass 32.0`; reaches inside form XObjects since `G017`).
///
/// ## Contract
///
/// - One `text-run-delete …` line with the usual save-report fields, then
///   the exit code from [`finish_edit`].
/// - Refusals — an out-of-range run, and the §9.4.2 guard when the next run
///   inherits its position — go through [`report_edit_error`] before any
///   mutation. The guard's message names its own remedy.
/// - With `--leaf`, the form's reach is printed by [`report_form_reach`]
///   because the stream is shared and the delete lands on every page the form
///   is drawn on.
pub(crate) fn cmd_text_run_delete(args: &TextRunDeleteArgs<'_>) -> u8 {
    let page_index = (args.page.max(1) - 1) as usize;
    let target = match object_or_leaf(args.input, args.object, args.leaf) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let result = match target {
        GeometryTarget::Page(object) => session
            .delete_text_run(page_index, object, args.run)
            .map(|d| (d, None)),
        GeometryTarget::Leaf(leaf) => session
            .delete_text_run_in_form(page_index, leaf, args.run)
            .map(|o| (o.disclosures.clone(), Some(o))),
    };
    match result {
        Err(err) => return report_edit_error(args.input, &err),
        Ok((disclosures, form)) => {
            report_disclosures(&disclosures);
            report_form_reach(form.as_ref());
        }
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "text-run-delete {} page {} {} run={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page.max(1),
        target_token(args.object, args.leaf),
        args.run,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// Grouped arguments for `text-object-split` (`Pass 306.0`).
pub(crate) struct TextObjectSplitArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    pub(crate) object: usize,
    pub(crate) granularity: SplitGranularityArg,
    /// Explicit cut points; overrides `granularity` when non-empty.
    pub(crate) before: &'a [usize],
    pub(crate) dry_run: bool,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `text-object-split` — cut one `BT`…`ET` into several (`Pass 306.0`).
///
/// ## Contract
///
/// - `--dry-run` prints one `text-object-split-plan …` line naming the cut
///   points and writes nothing. That is the honest shape for the `line`
///   granularity, which infers where the lines are: the operator can see the
///   cuts before committing to them.
/// - Otherwise one `text-object-split …` line with the usual save-report
///   fields, then the exit code from [`finish_edit`].
/// - The `line` granularity's inference disclosure goes to **stderr** via
///   [`report_disclosures`], so stdout stays machine-parseable — rule 4's
///   "report separately", in the shell where the invocation IS the commit
///   (rule 11: there is no session to disclose into, so pdfcer prints on the
///   way past).
/// - Every §9.4.2/§14.6 refusal goes through [`report_edit_error`] before any
///   mutation, with the same sentences the GUI shows. One core, one answer.
pub(crate) fn cmd_text_object_split(args: &TextObjectSplitArgs<'_>) -> u8 {
    let page_index = (args.page.max(1) - 1) as usize;
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // Explicit cuts win over a granularity, and skip the inference entirely —
    // so they also carry no disclosure, because nothing was guessed.
    let (points, disclosures): (Vec<usize>, Vec<String>) = if args.before.is_empty() {
        match session.text_object_split_plan(page_index, args.object, args.granularity.to_core()) {
            Ok(pair) => pair,
            Err(err) => return report_edit_error(args.input, &err),
        }
    } else {
        (args.before.to_vec(), Vec::new())
    };
    report_disclosures(&disclosures);

    if args.dry_run {
        println!(
            "text-object-split-plan {} page {} object={} granularity={} cuts={} runs_before={:?}",
            args.input.display(),
            args.page.max(1),
            args.object,
            if args.before.is_empty() {
                args.granularity.name()
            } else {
                "explicit"
            },
            points.len(),
            points,
        );
        return 0;
    }

    let Some(output) = args.output else {
        eprintln!("pdfcer: text-object-split needs --output unless --dry-run is given");
        return 2;
    };

    match session.split_text_object(page_index, args.object, &points) {
        Err(err) => return report_edit_error(args.input, &err),
        Ok(d) => report_disclosures(&d),
    }

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
        "text-object-split {} page {} object={} granularity={} cuts={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page.max(1),
        args.object,
        if args.before.is_empty() {
            args.granularity.name()
        } else {
            "explicit"
        },
        points.len(),
        args.mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// Grouped arguments for `text-run-move` (`G017`).
pub(crate) struct TextRunMoveArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    /// The page paint-order index, when addressing a page object.
    pub(crate) object: Option<usize>,
    /// The index into this page's form leaves, when addressing a text object
    /// INSIDE a form XObject.
    pub(crate) leaf: Option<usize>,
    /// One run uses `move_text_run`; several use the set verb (`G030`).
    pub(crate) run: &'a [usize],
    pub(crate) dx: f64,
    pub(crate) dy: f64,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `text-run-move` — translate one show operator, or a set of them as one
/// edit, inside a text object (`G017`, `G030`).
///
/// ## Contract
///
/// - One `text-run-move …` line with the usual save-report fields, then the
///   exit code from [`finish_edit`].
/// - The two §9.4.2 refusals — this run has no position of its own, or the
///   run after it has none — go through [`report_edit_error`] before any
///   mutation, with the same sentences the GUI shows. One core, one answer,
///   whichever shell the operator came through.
/// - Where a positioning operator had to be ADDED (a `TD`, whose second
///   operand is the leading, or a bare `T*`), the disclosure goes to stderr
///   via [`report_disclosures`] so stdout stays machine-parseable. Rule 4:
///   the page is unchanged and the bytes are not, and the operator does not
///   find that out from a diff.
pub(crate) fn cmd_text_run_move(args: &TextRunMoveArgs<'_>) -> u8 {
    let page_index = (args.page.max(1) - 1) as usize;
    let target = match object_or_leaf(args.input, args.object, args.leaf) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let result = match (target, args.run) {
        (GeometryTarget::Page(object), &[run]) => session
            .move_text_run(page_index, object, run, args.dx, args.dy)
            .map(|d| (d, None)),
        (GeometryTarget::Page(object), runs) => session
            .move_text_runs(page_index, object, runs, args.dx, args.dy)
            .map(|d| (d, None)),
        (GeometryTarget::Leaf(leaf), &[run]) => session
            .move_text_run_in_form(page_index, leaf, run, args.dx, args.dy)
            .map(|o| (o.disclosures.clone(), Some(o))),
        (GeometryTarget::Leaf(leaf), runs) => session
            .move_text_runs_in_form(page_index, leaf, runs, args.dx, args.dy)
            .map(|o| (o.disclosures.clone(), Some(o))),
    };
    match result {
        Err(err) => return report_edit_error(args.input, &err),
        Ok((disclosures, form)) => {
            report_disclosures(&disclosures);
            report_form_reach(form.as_ref());
        }
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "text-run-move {} page {} {} run={} dx={} dy={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page.max(1),
        target_token(args.object, args.leaf),
        args.run
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(","),
        args.dx,
        args.dy,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// Grouped arguments for `text-run-merge` (`G035`).
pub(crate) struct TextRunMergeArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    pub(crate) object: usize,
    pub(crate) runs: &'a [usize],
    pub(crate) separator: &'a str,
    pub(crate) fit: MergeFitArg,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `text-run-merge` — join consecutive show operators into one (`G035`).
///
/// One `text-run-merge …` line with the merged text, the scale written and
/// the usual save-report fields, then the exit code from [`finish_edit`].
/// Disclosures go to stderr. A refusal exits `EDIT_REFUSED` before anything
/// is written.
pub(crate) fn cmd_text_run_merge(args: &TextRunMergeArgs<'_>) -> u8 {
    use pdfcer_core::text_edit::{FormatError, MergeFit, MergeOptions, MergeSeparator};
    let page_index = (args.page.max(1) - 1) as usize;
    let separator = match args.separator {
        "none" => MergeSeparator::None,
        "space" => MergeSeparator::Space,
        other => MergeSeparator::Text(other.to_owned()),
    };
    let fit = match args.fit {
        MergeFitArg::Span => MergeFit::Span,
        MergeFitArg::Natural => MergeFit::Natural,
    };
    let opts = MergeOptions::default().separator(separator).fit(fit);
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let report = match session.merge_text_runs(page_index, args.object, args.runs, &opts) {
        Ok(r) => r,
        Err(err) => {
            eprintln!(
                "pdfcer: text-run-merge refused on {}: {err}",
                args.input.display()
            );
            return match err {
                FormatError::Write(_) => exit::SAVE_REFUSED,
                FormatError::Content(_) | FormatError::PageTree(_) => exit::RUNTIME_ERROR,
                _ => exit::EDIT_REFUSED,
            };
        }
    };
    report_disclosures(&report.disclosures);
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    let runs: Vec<String> = args.runs.iter().map(ToString::to_string).collect();
    println!(
        "text-run-merge {} page {} object={} runs={} merged={} text={:?} h_scale={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page.max(1),
        args.object,
        runs.join(","),
        report.runs_merged,
        report.text,
        report
            .h_scale_change
            .map_or_else(|| "unchanged".to_owned(), |(_, pct)| format!("{pct:.4}")),
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// Grouped arguments for `text-run-width` (`G038`).
pub(crate) struct TextRunWidthArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    pub(crate) object: usize,
    pub(crate) run: usize,
    pub(crate) width: f64,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `text-run-width` — fit one show operator to a page width through `Tz`
/// (`G038`).
///
/// One `text-run-width …` line with the usual save-report fields and the
/// scale that was written, then the exit code from [`finish_edit`]. The
/// disclosure of the scale goes to stderr. A refusal exits
/// `EDIT_REFUSED` before anything is written.
pub(crate) fn cmd_text_run_width(args: &TextRunWidthArgs<'_>) -> u8 {
    use pdfcer_core::text_edit::FormatError;
    let page_index = (args.page.max(1) - 1) as usize;
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let report = match session.set_text_run_width(page_index, args.object, args.run, args.width) {
        Ok(r) => r,
        Err(err) => {
            eprintln!(
                "pdfcer: text-run-width refused on {}: {err}",
                args.input.display()
            );
            return match err {
                FormatError::Write(_) => exit::SAVE_REFUSED,
                FormatError::Content(_) | FormatError::PageTree(_) => exit::RUNTIME_ERROR,
                _ => exit::EDIT_REFUSED,
            };
        }
    };
    report_disclosures(&report.disclosures);
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "text-run-width {} page {} object={} run={} width={} h_scale={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page.max(1),
        args.object,
        args.run,
        args.width,
        report
            .h_scale_change
            .map_or_else(|| "unchanged".to_owned(), |(_, pct)| format!("{pct:.4}")),
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}
