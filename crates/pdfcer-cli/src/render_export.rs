use super::*;

/// The render-affecting flags `render-page` and `export-image` share, as
/// one value (`Pass 248.0`).
///
/// # Why a struct, and why one resolver
///
/// `render-page` resolved its `RenderOptions` inline for 230 lines — the
/// settings file, three per-invocation overrides that must never write
/// back, the subset-standard preset and its disclosures, the colorant-
/// buffer ceiling, the font environment, the ink probe, the viewer-versus-
/// print `/AS` decision, and the layer overrides. `export-image` needs
/// every one of those, identically. Two copies would be R92's shape: the
/// second one is the one that goes stale, and the symptom is a flag that
/// `render-page` honours and `export-image` parses and ignores
/// (`feedback_a_shell_flag_can_be_parsed_and_never_used`). So the block
/// moved here whole, and both verbs call it.
///
/// `verb` is only used in messages ("`pdfcer: export-image: …`"), so an
/// operator reading stderr sees the command they typed.
pub(crate) struct RenderFlags<'a> {
    /// The subcommand name, for diagnostics.
    pub(crate) verb: &'static str,
    /// The document path, for diagnostics.
    pub(crate) input: &'a Path,
    /// `--standard`.
    pub(crate) standard: Option<&'a str>,
    /// `--overprint-zero-tint-scope`.
    pub(crate) overprint_zero_tint_scope: Option<&'a str>,
    /// `--spot-colorant-device-model`.
    pub(crate) spot_colorant_device_model: Option<&'a str>,
    /// `--max-cmyk-buffer-bytes`.
    pub(crate) max_cmyk_buffer_bytes: Option<&'a str>,
    /// `!--no-annotations`.
    pub(crate) annotations: bool,
    /// The font environment `build_font_environment` produced from
    /// `--font-dir`. Moved in: it becomes `RenderOptions::fonts`.
    pub(crate) font_env: pdfcer_render::FontEnvironment,
    /// `--fast-subpixel`.
    pub(crate) fast_subpixel: bool,
    /// `--probe-ink X,Y`.
    pub(crate) probe_ink: Option<&'a str>,
    /// `--print-state`.
    pub(crate) print_state: bool,
    /// The scale the render will run at — the `/AS` viewer magnification.
    pub(crate) scale: f32,
    /// `--show-layer`, by name.
    pub(crate) show_layers: &'a [String],
    /// `--hide-layer`, by name.
    pub(crate) hide_layers: &'a [String],
}

/// Resolve every render-affecting flag into the `RenderOptions` the engine
/// takes, reporting each override and preset on stderr exactly as
/// `render-page` always has.
///
/// # Errors
///
/// The exit code to return — always `RUNTIME_ERROR` — after the message
/// naming the flag that could not be honoured has been printed. A malformed
/// operator value is refused by name rather than defaulted, because a
/// mistyped token that renders under the default looks exactly like the
/// flag working.
pub(crate) fn resolve_render_options(
    doc: &Document,
    flags: RenderFlags<'_>,
) -> Result<pdfcer_render::RenderOptions, u8> {
    let RenderFlags {
        verb,
        input,
        standard,
        overprint_zero_tint_scope,
        spot_colorant_device_model,
        max_cmyk_buffer_bytes,
        annotations,
        font_env,
        fast_subpixel,
        probe_ink,
        print_state,
        scale,
        show_layers,
        hide_layers,
    } = flags;
    // Annotation painting is on by default (§12.5); `--no-annotations`
    // clears it to reproduce the pre-6.0 content-only raster. The font
    // environment carries any `--font-dir` supplied faces (decision 012);
    // with no `--font-dir` it is the bundled default (R63).
    // §8.6.4.4 mandates no CMYK conversion, so the operator's persisted
    // choice governs (R169). Read from the same `userdata/` store the GUI
    // uses, so `render-page` and the canvas cannot disagree about what
    // black looks like. Loading cannot fail — a missing or broken file
    // yields defaults plus notes, which are reported and never fatal.
    let (mut settings, settings_report) =
        pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
    report_settings(&settings_report);

    // The §8.6.7 zero-tint scope, applied over the saved settings and never
    // written back — same discipline as `--standard` below, and for the same
    // reason: one diagnostic render must not change how every later render
    // behaves.
    //
    // Parsed by handing the token to the SETTINGS PARSER rather than by
    // matching the three strings here. `OverprintZeroTintScope::parse` is the
    // same function the settings FILE parser calls, so a token the file
    // accepts and a token this flag accepts cannot diverge. A `match` here
    // would be a second spelling of one enum, and the second spelling is
    // always the one that goes stale.
    if let Some(token) = overprint_zero_tint_scope {
        use pdfcer_core::settings::OverprintZeroTintScope as Scope;
        match Scope::parse(token) {
            Some(scope) => {
                settings.overprint_zero_tint_scope = scope;
                eprintln!(
                    "pdfcer: {verb}: overprint_zero_tint_scope = {} for this render only; your saved setting is unchanged",
                    scope.as_str()
                );
            }
            None => {
                // Refuse by name rather than falling back silently. A mistyped
                // token that renders under the DEFAULT looks exactly like the
                // flag working — and the operator reached for the flag
                // precisely because they wanted the non-default.
                eprintln!(
                    "pdfcer: {verb}: unknown --overprint-zero-tint-scope {token:?} — known: device_cmyk_only, grey_as_k_only, all_process_spaces"
                );
                return Err(exit::RUNTIME_ERROR);
            }
        }
    }

    // Same shape as the block above, and deliberately not folded into it:
    // the two settings answer different questions (`OP-A5` vs `OP-A7`) and a
    // shared parser would have to know which enum a token belongs to.
    if let Some(token) = spot_colorant_device_model {
        use pdfcer_core::settings::SpotColorantDeviceModel as Model;
        match Model::parse(token) {
            Some(model) => {
                settings.spot_colorant_device_model = model;
                eprintln!(
                    "pdfcer: {verb}: spot_colorant_device_model = {} for this render only; your saved setting is unchanged",
                    model.as_str()
                );
            }
            None => {
                // Refused by name, for the reason the block above gives: a
                // mistyped token rendering under the default looks exactly
                // like the flag working, and the operator reached for it
                // precisely because they wanted the non-default.
                eprintln!(
                    "pdfcer: {verb}: unknown --spot-colorant-device-model {token:?} — known: simulate_separations, alternate_space_substitution"
                );
                return Err(exit::RUNTIME_ERROR);
            }
        }
    }

    // The subset-standard preset, applied OVER the operator's saved settings
    // and never written back. A render flag must not mutate a settings file:
    // one `--standard pdf-x4` render would otherwise silently change how every
    // later render behaved, which is the shape of surprise rule 4 exists to
    // stop.
    if let Some(token) = standard {
        use pdfcer_core::settings::presets::{RenderPreset, RenderStandard};
        match RenderStandard::parse(token) {
            Ok(std) => {
                let preset = RenderPreset::for_standard(std);
                let changed = preset.apply(&mut settings);
                // Rule 4: what the preset MOVED, by name. "4 settings changed"
                // is not actionable; knowing it was `image_minify` is.
                if changed.is_empty() {
                    eprintln!(
                        "pdfcer: render preset {}: your settings already match it; \
                         nothing changed",
                        std.as_str()
                    );
                } else {
                    eprintln!(
                        "pdfcer: render preset {}: changed {}",
                        std.as_str(),
                        changed
                            .iter()
                            .map(|k| k.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                for line in preset.disclosures() {
                    eprintln!("pdfcer: {line}");
                }
            }
            Err(bad) => {
                eprintln!(
                    "pdfcer: {verb}: unknown --standard {bad:?} — known: {}",
                    RenderStandard::all()
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                return Err(exit::RUNTIME_ERROR);
            }
        }
    }

    // The colorant-buffer ceiling: the flag overrides the setting, the
    // setting overrides the built-in default, and an unreadable flag value
    // is REPORTED and then ignored rather than quietly meaning zero -- the
    // same shape as every other bad value in the settings file, because a
    // silent zero here would read as "pdfcer stopped compositing in ink".
    let max_cmyk_buffer_bytes = match max_cmyk_buffer_bytes {
        Some(text) => match pdfcer_core::settings::parse_byte_size(text) {
            Ok(parsed) => {
                eprintln!(
                    "pdfcer: note: --max-cmyk-buffer-bytes {} overrides the `max_cmyk_buffer_bytes` setting for this render ({} pixel(s) may composite in ink)",
                    pdfcer_core::settings::format_byte_size(parsed),
                    pdfcer_render::max_cmyk_composite_pixels(parsed)
                );
                parsed
            }
            Err(bad) => {
                eprintln!("pdfcer: note: {bad} -- using the setting instead");
                settings.max_cmyk_buffer_bytes
            }
        },
        None => settings.max_cmyk_buffer_bytes,
    };

    let mut render_options = pdfcer_render::RenderOptions::default()
        .with_annotations(annotations)
        .with_cmyk_intent(settings.cmyk_intent)
        // The subtractive compositing ceiling. Not a spec ambiguity and not
        // a policy default: it is a MEMORY budget, and the right number is
        // a function of the operator's machine and their tolerance, neither
        // of which is knowable from inside the renderer.
        .with_max_cmyk_buffer_bytes(max_cmyk_buffer_bytes)
        .with_page_blend_space_source(settings.page_blend_space_source)
        // Which colour spaces get OPM 1's zero-tint rule. The default
        // preserves a spot backdrop under a DeviceGray fill, which is a
        // DIVERGENCE from ISO 32000-1 (and matches Acrobat only over a spot
        // backdrop -- NOT over process components, measured `Pass 206.0`);
        // `device_cmyk_only` is the conforming one. Edition-gated: 32000-2
        // deletes two of the three provisions that settle it in 1.7.
        //
        // SURVIVOR 7. This said "The 8.6.7 ambiguity" and was missed by the
        // 330th filing's own sweep, which grepped `§8.6.7 ambiguity` — with
        // the section sign. This line has no `§`. A sweep for a CLAIM is only
        // as good as its spelling of the claim, which is the same failure the
        // sweep existed to catch, one level up.
        .with_overprint_zero_tint_scope(settings.overprint_zero_tint_scope)
        .with_spot_colorant_device_model(settings.spot_colorant_device_model)
        // `MSH-A1`: what a type 6/7 mesh-shading PATCH record pads to.
        // The clause states the rule for a VERTEX and the patch clauses
        // point back at it without redefining the unit, in both editions.
        .with_mesh_patch_padding(settings.mesh_patch_padding)
        // The other four R169 rendering knobs, all spec silences the
        // standard declines to fill: the mask resampling filter
        // (`SM-A1`, §8.9.6.3), the minification filter (`IM-A1`,
        // §8.9.5.3), the CMYK-JPEG polarity rule (`DCT-A1`, §7.4.8) and
        // the missing-`/AS` policy (`AS-A1`, §12.5.5). Every default is
        // the behaviour pdfcer shipped before the setting existed, so a
        // machine with no settings file renders exactly as it always did.
        .with_mask_resample(settings.mask_resample)
        .with_image_minify(settings.image_minify)
        .with_cmyk_jpeg_polarity(settings.cmyk_jpeg_polarity)
        .with_missing_as(settings.missing_as);
    render_options.fonts = font_env;
    // `--fast-subpixel`. Assigned rather than set through a builder for
    // the reason `RenderOptions`'s own docs give: the type is
    // `#[non_exhaustive]`, so field assignment is the documented way in.
    render_options.subpixel_culling = fast_subpixel;
    // `--probe-ink X,Y`. A malformed pair is refused HERE, before the
    // render, because it is an operator mistake and nothing about the
    // document can fix it -- unlike a coordinate that is well-formed but
    // outside the raster, which cannot be judged until the page geometry
    // has been resolved and is therefore reported rather than refused.
    if let Some(spec) = probe_ink {
        match parse_probe_ink(spec) {
            Ok((x, y)) => render_options.ink_probe = Some((x, y)),
            Err(msg) => {
                eprintln!("pdfcer: --probe-ink: {msg}");
                return Err(exit::RUNTIME_ERROR);
            }
        }
    }
    // A raster export is for LOOKING AT, so the verb is a viewer
    // under §8.11.4.5 and applies `View`-event `/AS` usage at the
    // requested scale. The print path is the one the clause forbids this
    // on, and pdfcer's printing does not come through here.
    // §8.11.4.5: only a viewer examines `/AS`; printing and aggregating
    // applications "shall not apply the changes based on usage
    // application dictionaries". `--print-state` is that mode, and NOTE 2
    // licenses offering it.
    if !print_state {
        render_options.view_magnification = Some(scale);
    }

    // §8.11 layer overrides. Resolved by NAME against the document's own
    // registry, because a name is what an operator has (`list-layers`
    // prints them) and an object number is not.
    if !show_layers.is_empty() || !hide_layers.is_empty() {
        match resolve_layer_override(doc, show_layers, hide_layers) {
            Ok((visibility, unmatched)) => {
                for name in unmatched {
                    eprintln!(
                        "pdfcer: no layer named {name:?} in {} — the other --show-layer/--hide-layer names were still applied",
                        input.display()
                    );
                }
                render_options.layers = Some(visibility);
            }
            Err(name) => {
                eprintln!(
                    "pdfcer: layer {name:?} was given to both --show-layer and --hide-layer; pdfcer will not guess which you meant"
                );
                // `RUNTIME_ERROR` rather than a new usage code: clap owns
                // the usage vocabulary and this is not a malformed command
                // line — both flags are spelled correctly and mean what
                // they say. What cannot be done is honouring both.
                return Err(exit::RUNTIME_ERROR);
            }
        }
    }

    Ok(render_options)
}

/// Implement `pdfcer render-page <input> [--page N] [--scale S] -o <out>`.
///
/// # The pipeline
///
/// Four stages, each with its own failure mode and exit code:
///
/// 1. **Load** — [`Document::load`] reads the file, walks the
///    cross-reference chain, and eagerly parses every in-use object.
///    Mapped by [`exit_code_for_doc`].
/// 2. **Resolve the page tree** — [`pdfcer_core::page_tree::pages`]
///    flattens the tree with inheritance applied, yielding a `Vec<Page>`
///    in document order. Its index is what `--page` selects.
/// 3. **Rasterize** — [`pdfcer_render::render_page_with`] at `scale`
///    device pixels per user-space unit. The default face set is the
///    **bundled** Base-14 substitutes ([`pdfcer_render::RenderOptions::default`]);
///    `--font-dir` layers OPERATOR-supplied faces on top (decision 012).
///    The CLI never *auto-discovers* system fonts — rule R19 (decision
///    004) makes the default render deterministic, and a batch job whose
///    output silently depends on which fonts the runner happens to have
///    installed is not one anyone can trust. `--font-dir` is the explicit,
///    disclosed opt-in: the shell walks the folder (R61), and glyphs it
///    draws from a supplied face are reported via the `supplied` counter,
///    distinct from the bundled `substituted` counter (R62). Fonts the
///    document does not embed and that no supplied face matches are still
///    reported via `substituted`.
/// 4. **Encode + write** — `Pixmap::encode_png` (tiny-skia's
///    `png-format` feature) then a plain file write.
///
/// # Page selection
///
/// `--page` is **1-based**, matching how every PDF reader and every human
/// numbers pages. `0` and any value past the last page take the same
/// out-of-range path: a stderr message naming the actual page count, and
/// [`exit::RUNTIME_ERROR`]. Deliberately *not* a clap range constraint —
/// clap would exit `2` (usage error) for `--page 0` but this function
/// would exit `1` for `--page 999`, and a script branching on the exit
/// code should not have to care which flavour of "that page isn't there"
/// it hit.
///
/// # Output
///
/// One machine-readable line on stdout in the format documented in the
/// module header, and — only when the render was less than fully faithful
/// — a human-readable expansion on stderr. A clean render writes nothing
/// to stderr at all, so `2>/dev/null` is never needed and a non-empty
/// stderr is a real signal.
///
/// # Exit codes
///
/// `0` success; `3` the input could not be read or the output could not
/// be written; `4` the input is not a PDF; `1` everything else (structural
/// failure, page out of range, raster-size guard, PNG encoding).
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_render_page(
    input: &Path,
    page_number: u32,
    scale: f32,
    standard: Option<&str>,
    overprint_zero_tint_scope: Option<&str>,
    spot_colorant_device_model: Option<&str>,
    region: Option<&str>,
    output: &Path,
    annotations: bool,
    fast_subpixel: bool,
    max_cmyk_buffer_bytes: Option<&str>,
    probe_ink: Option<&str>,
    font_dirs: &[PathBuf],
    show_layers: &[String],
    hide_layers: &[String],
    print_state: bool,
) -> u8 {
    // Build the font environment from any `--font-dir` BEFORE loading the
    // document: the walk is pure shell-side I/O (R61), and a bad font dir
    // is a note, never a fatal error. With no `--font-dir` this is exactly
    // the bundled default and the deterministic path is untouched (R63).
    let (font_env, supplied_registered, font_notes) = build_font_environment(font_dirs);
    for note in &font_notes {
        eprintln!("pdfcer: font-dir: {note}");
    }

    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };

    let pages = match pdfcer_core::page_tree::pages(&doc) {
        Ok(pages) => pages,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };

    // 1-based → 0-based. `checked_sub` handles `--page 0` without a
    // panic or a wrap; `get` handles past-the-end. Both land on the same
    // message so the failure reads the same way whichever end it came
    // from.
    let Some(page) = page_number
        .checked_sub(1)
        .and_then(|i| pages.get(i as usize))
    else {
        eprintln!(
            "pdfcer: {}: page {page_number} is out of range (document has {} page(s), \
numbered 1..={})",
            input.display(),
            pages.len(),
            pages.len()
        );
        return exit::RUNTIME_ERROR;
    };

    let render_options = match resolve_render_options(
        &doc,
        RenderFlags {
            verb: "render-page",
            input,
            standard,
            overprint_zero_tint_scope,
            spot_colorant_device_model,
            max_cmyk_buffer_bytes,
            annotations,
            font_env,
            fast_subpixel,
            probe_ink,
            print_state,
            scale,
            show_layers,
            hide_layers,
        },
    ) {
        Ok(options) => options,
        Err(code) => return code,
    };

    // THE REGION BRANCH, and it is the same engine call with a smaller
    // pixmap -- `render_page_region` and `render_page` share one
    // implementation in `pdfcer-render`, so nothing about annotation
    // z-order, cancellation, layer state or diagnostics can differ between
    // them. Only the raster size and a translation on the base CTM.
    let parsed_region = match region.map(parse_region) {
        None => None,
        Some(Ok(r)) => Some(r),
        Some(Err(msg)) => {
            // A malformed region is an operator mistake, not a document
            // one, so it reports as a runtime error rather than as
            // anything that implicates the PDF.
            eprintln!("pdfcer: --region: {msg}");
            return exit::RUNTIME_ERROR;
        }
    };
    let rendered = match parsed_region {
        Some(r) => pdfcer_render::render_page_region(&doc.view(), page, scale, r, &render_options),
        None => pdfcer_render::render_page_with(&doc, page, scale, &render_options),
    };
    let rendered = match rendered {
        Ok(rendered) => rendered,
        Err(err) => {
            eprintln!("pdfcer: {}: page {page_number}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };

    let png = match rendered.pixmap.encode_png() {
        Ok(png) => png,
        Err(err) => {
            eprintln!("pdfcer: {}: PNG encoding failed: {err}", output.display());
            return exit::RUNTIME_ERROR;
        }
    };
    if let Err(err) = std::fs::write(output, &png) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }

    // The stable stdout line (module header, "stdout result-line format").
    // Counters last, key=value, fixed order — appended to, never reordered.
    let d = &rendered.diagnostics;
    println!(
        "rendered {} page {page_number} -> {} {}x{}; {}",
        input.display(),
        output.display(),
        rendered.pixmap.width(),
        rendered.pixmap.height(),
        render_counters_line(d, &doc, supplied_registered)
    );
    // A SECOND LINE, NOT MORE KEYS ON THE FIRST ONE.
    //
    // The stable line is `key=<integer>` pairs in a fixed order and a
    // published contract (`tools/check-metrics-line-contract.py` holds all
    // three copies of it in step). This payload is four floats and a
    // classification, it is absent unless asked for, and folding it in
    // would mean either changing that line's shape for every render or
    // emitting placeholder zeros -- which read exactly like "no ink here",
    // the one misreading `InkProbeSource` exists to prevent. So it gets its
    // own prefixed line, which a parser can select or ignore whole.
    if let Some(probe) = &d.ink_probe {
        println!("{}", format_ink_probe(probe));
    }
    report_diagnostics(
        d,
        // The RESOLVED ceiling (flag over setting over default), read back
        // off the options the render actually ran with, so the note that
        // quotes it cannot disagree with the buffer that used it.
        render_options.max_cmyk_buffer_bytes,
        u64::from(rendered.pixmap.width()) * u64::from(rendered.pixmap.height()),
    );

    exit::SUCCESS
}

/// The `key=<integer>` half of the stable result line, shared by
/// `render-page` and `export-image` (`Pass 248.0`).
///
/// ONE format string, not two. The counters are a PUBLISHED CONTRACT
/// (`R212`, `tools/check-metrics-line-contract.py`), and the moment a
/// second verb printed its own copy there would be a fourth place for the
/// list to drift — and the copy nobody tests is the one that goes stale.
/// `export-image` prints exactly what `render-page` prints, after its own
/// prefix, so a parity harness that reads one can read the other.
///
/// The comments inside are kept with the arguments they explain; they
/// are the record of WHY each key exists, and a helper that dropped them
/// would be a list of field accesses nobody could audit.
pub(crate) fn render_counters_line(
    d: &pdfcer_render::Diagnostics,
    doc: &Document,
    // The `--font-dir` registration count, which lives in the shell (the
    // walk is shell-side I/O, R61) and not in `Diagnostics`.
    supplied_registered: usize,
) -> String {
    // The Pass 6.0 annotation counters are APPENDED to the metrics half,
    // after every pre-existing key — the stable-line contract's
    // append-never-reorder rule (module docs). `annot_no_ap` is a SUM of
    // the per-subtype `annotations_without_ap` map (the machine line's
    // contract is `key=<integer>`); the per-subtype breakdown goes to
    // stderr where it cannot break a parser. `need_appearances` is the
    // document-scoped `/NeedAppearances` disclosure (R51).
    let annot_no_ap: usize = d.annotations_without_ap.values().sum();
    let need_appearances = usize::from(pdfcer_core::annot::need_appearances(doc));
    format!(
        "substituted={} notdef={} unsupported={} unknown={} deferred={} \
images={} images_culled={} images_unsupported={} forms={} forms_culled={} \
subpixel_culled={} \
images_codec_unsupported={} codec_features={} codec_geometry_mismatch={} \
dct_cmyk={} lzw_anomalies={} dct_cmyk_unverifiable={} jpx_preblended={} \
annots={} annots_painted={} annots_no_ap={} annots_hidden={} \
annots_state_missing={} annots_widget={} annots_degenerate={} \
annots_out_of_scope={} page_content_suppressed={} need_appearances={} \
unsupported_type3={} unsupported_noncmap={} unsupported_vertical={} \
unsupported_composite_not_embedded={} unsupported_unknown_subtype={} \
unsupported_unusable_program={} supplied={} supplied_registered={} \
contents_unresolved={} images_masked={} images_mask_unsupported={} \
masks_resampled={} mattes_undone={} mattes_not_undone={} oc_hidden={} \
cs_unresolved={} colors_not_set={} icc_alternate={} icc_device_fallback={} \
tint_applied={} tint_not_applied={} sep_all_approximated={} \
sep_none_suppressed={} pattern_spaces={} patterns_unpainted={} \
indexed_clamped={} indexed_short={} shadings={} shadings_via_sh={} \
shadings_paintable={} shadings_painted={} shadings_refused={} shadings_mesh={} \
mesh_records={} mesh_truncated={} mesh_unusable={} \
type3_glyphs={} type3_glyphs_missing={} type3_colors_ignored={} \
img_colorant_none={} img_uncalibrated={} \
blend_modes_applied={} blend_modes_ignored={} soft_masks_ignored={} \
soft_masks_applied={} soft_mask_tr_ignored={} soft_masks_reset_stale={} \
groups_flattened={} groups_special={} \
groups_composited={} groups_knockout_approx={} \
overprint_requested={} overprint_opm1={} overprint_effective={} \
overprint_composited={} overprint_refused={} overprint_pixels={} nonseparable_composited={} nonseparable_pixels={} \
groups_backdrop_reruns={} soft_masks_on_group_result={} \
overprint_images_unsupported={} overprint_shadings_unsupported={} \
blend_space_subtractive={} blend_space_from_output_intent={} blends_in_wrong_space={} \
cmyk_buffer={} cmyk_buffer_refused={} cmyk_bridged_pixels={} \
cmyk_groups_approximated={} cmyk_unbridged_images={} cmyk_native_image_pixels={} rendering_intents_set={} \
icc_managed_paints={} icc_unmanaged_paints={} \
overprint_process_images_unsupported={} annots_icon_painted={} page_resources_defaulted={}",
        d.glyphs_substituted,
        d.glyphs_notdef,
        d.fonts_unsupported,
        d.unknown_ops,
        d.deferred_ops,
        d.images_rendered,
        d.images_culled,
        d.images_unsupported,
        d.forms_rendered,
        d.forms_culled,
        d.subpixel_culled,
        d.images_codec_unsupported,
        d.codec_feature_unsupported.values().sum::<usize>(),
        d.codec_geometry_mismatch,
        d.dct_cmyk_images,
        d.lzw_framing_anomalies,
        d.dct_cmyk_polarity_unverifiable,
        d.jpx_smask_in_data_preblended,
        d.annotations_total,
        d.annotations_painted,
        annot_no_ap,
        d.annotations_hidden,
        d.annotations_appearance_state_missing,
        d.annotations_widget,
        d.annotations_placement_degenerate,
        d.annotations_out_of_scope,
        usize::from(d.page_content_suppressed),
        need_appearances,
        // Per-reason breakdown of `unsupported` (R20): always emitted in
        // a fixed order, even at zero, so the line stays diffable. Sum ==
        // `unsupported`. `unusable_program` non-zero = an embedded program
        // pdfcer could not parse (the class that hid the TrueType misroute).
        unsupported_reason(d, "Type3"),
        unsupported_reason(d, "NonIdentityCmap"),
        unsupported_reason(d, "VerticalWriting"),
        unsupported_reason(d, "CompositeNotEmbedded"),
        unsupported_reason(d, "UnknownSubtype"),
        unsupported_reason(d, "UnusableProgram"),
        // decision 012: the SUPPLIED trust level, appended after every
        // pre-existing key. `supplied` = glyphs drawn from an
        // operator-supplied face; `supplied_registered` = name→file
        // registrations the `--font-dir` walk added (0 without the flag).
        d.glyphs_supplied,
        supplied_registered,
        // Appended after every pre-existing key: `/Contents` entries this
        // page named that are not in the file, so their marks are simply
        // absent from the raster (§7.3.10 + Table 30).
        d.contents_streams_unresolved,
        // Image transparency (§8.9.6, §11.6.5.3), appended after every
        // pre-existing key. `images_masked` is a SUBSET of `images` —
        // those whose `/SMask`, `/Mask` or JPX opacity channel was
        // actually composited — and is census, not shortfall. The
        // per-mechanism breakdown goes to stderr, where a new key cannot
        // break a parser. `images_mask_unsupported` is the shortfall
        // twin: the picture is on the page but too solid.
        d.images_masked,
        d.images_mask_unsupported,
        d.masks_resampled,
        d.mattes_undone,
        d.mattes_not_undone,
        // §8.11.3.2 optional content, appended after every pre-existing
        // key. A page that renders emptier than expected because a
        // producer turned a layer OFF is indistinguishable from a render
        // that failed — unless this number is on the line. It is the
        // disclosure channel for the one feature whose correct behaviour
        // is "draw less" (R183).
        d.oc_sections_hidden,
        // ---- §8.6 colour, appended after every pre-existing key ----
        //
        // `pdfcer_render::ColorDiagnostics` has carried twelve counters
        // since the colour-space slice shipped, and **no shell read any of
        // them** — they were computed, merged across nested form XObjects,
        // unit-tested, and then dropped on the floor at the crate
        // boundary. The cost is not abstract: a page whose gradients are
        // `/Pattern` fills paints NOTHING for them, and
        // `patterns_unpainted` was the only thing that could have said so.
        // pdfcer reported such a page as a clean render. That is precisely
        // the silence project rule 4 forbids, and it survived because the
        // obligation is discharged in a *different crate* from the one
        // that computes it — an engine-side counter with no shell caller
        // discloses to nobody.
        //
        // All twelve go on the line, including the census ones
        // (`tint_applied`, `sep_none_suppressed`, `pattern_spaces`,
        // `indexed_clamped`). They are merged as a unit and documented
        // individually as answering distinct operator questions, so a
        // partial exposure would only create a second judgement call later
        // about which half was worth reporting. The per-reason strings go
        // to stderr with the rest.
        d.color.spaces_unresolved,
        d.color.colors_not_set,
        d.color.icc_alternate_used,
        d.color.icc_device_fallback_used,
        d.color.tint_transforms_applied,
        d.color.tint_transform_not_applied,
        d.color.separation_all_approximated,
        d.color.separation_none_suppressed,
        d.color.pattern_spaces_selected,
        d.color.patterns_unpainted,
        d.color.indexed_index_clamped,
        d.color.indexed_lookup_short,
        // ---- §8.7.4 shadings, appended after every pre-existing key ----
        //
        // Wired at the same time as the counters themselves, deliberately.
        // The colour block directly above spent months computed-and-unread
        // because adding a counter and adding its shell surface were
        // treated as two changes; they are one, and this is the first
        // counter block written after that lesson.
        //
        // `shadings` beside `shadings_painted` is the load-bearing pair
        // while the geometry slice is outstanding: a non-zero left number
        // and a zero right one is the honest statement that pdfcer found
        // the gradients, understood them, and drew none of them. When the
        // geometry lands the right number moves and nothing else on this
        // line changes — which is what makes it possible to SHOW the
        // feature arriving rather than assert it.
        //
        // `shadings_paintable` sits between them and answers the question
        // an operator actually has: *will an update fix MY file?* A page
        // whose shadings are all type 7 meshes has `paintable=0` and needs
        // a different answer from one where paintable equals shadings.
        d.shading.encountered,
        d.shading.via_sh,
        d.shading.paintable,
        d.shading.painted,
        d.shading.refused,
        d.shading.mesh(),
        d.shading.mesh_records,
        d.shading.mesh_truncated,
        d.shading.mesh_unusable,
        d.type3_glyph_procs_run,
        d.type3_glyphs_missing,
        d.type3_colors_ignored,
        // Appended after every pre-existing key. Both exist because the
        // pixel-parity harness reads THIS LINE and nothing else — a
        // stderr sentence, however well written, is invisible to it, and
        // a `Lab` image with a perfectly good explanation on stderr still
        // landed in that harness's *unexplained* bucket.
        //
        // These two very nearly shipped as counters with no shell caller,
        // which is the exact defect `Pass 84.0` existed to fix: the patch
        // that was supposed to add them here aborted before writing, the
        // engine-side half landed, and only the stable-line ORDER TEST
        // caught the gap. That test is the reason this file cannot grow a
        // counter the CLI does not print.
        d.images_colorant_none,
        d.images_uncalibrated_colorimetry,
        // §11.3.5 blend modes and §11.6.5 soft masks. Appended after every
        // pre-existing key.
        //
        // SEVENTH copy of the stale-shortfall claim, corrected
        // 2026-08-18. This read "Neither is implemented; before these
        // existed neither was COUNTED either" — the second half is still
        // true and worth keeping, the first half stopped being true when
        // blend modes landed and again when soft masks landed (`cb20770`).
        //
        // Where they actually stand, and neither is "implemented" without
        // a qualifier:
        //  * BLEND MODES — all 15 of Table 136 and Table 137 are applied,
        //    in device sRGB on an additive page (which is what §8.6.6.4
        //    specifies for an additive device) and in the group's own
        //    colour space on a subtractive one, since `Pass 97.1e`'s
        //    colorant buffer. The 4 NONSEPARABLE modes are computed by pdfcer
        //    itself (`pdfcer_render::blend_nonsep`) and counted separately on
        //    `nonseparable_composited`, because they take a different code
        //    path and the two can fail independently.
        //
        //    This bullet said they were "recognised and DECLINED, so they
        //    land in `blend_modes_ignored`" until 2026-08-19. That was the
        //    EIGHTH copy of the claim, and it survived the commit that
        //    announced it was fixing the eighth — the sweep report and the
        //    sweep discharge drifted apart.
        //  * SOFT MASKS — built correctly, and since `Pass 97.0` applied
        //    to a transparency group's RESULT (§11.4.5) rather than folded
        //    into its contents' clip; see `soft_masks_on_group_result`
        //    below. Folding into the clip is still what happens to an
        //    ELEMENTARY object and is correct there (§11.6.4.1 makes the
        //    mask value that object's `q_m`). `/TR` is still counted and
        //    never evaluated.
        //
        // `soft_masks_ignored` is still the one to watch: an ignored mask
        // paints MORE than the document asked for.
        d.blend_modes_applied,
        d.blend_modes_ignored,
        d.soft_masks_ignored,
        // §11.6.5 soft masks that were BUILT and applied, plus the two
        // shortfalls inside that: a `/TR` transfer function not
        // evaluated (which can leave visible exactly the content a
        // document meant to hide, since `/TR` is where a mask is
        // inverted), and a `/SMask /None` reset that could not restore
        // the pre-mask clip because a `W n` intervened.
        d.soft_masks_applied,
        d.soft_mask_transfer_ignored,
        d.soft_masks_reset_stale,
        // §11.4.7 transparency groups. Appended after every pre-existing
        // key. A group is a COMPOSITING SCOPE, so flattening it applies
        // blend/alpha/mask to each object inside instead of to the group's
        // result — a difference no blend-mode counter can express.
        d.transparency_groups_flattened,
        d.transparency_groups_special,
        // §11.4.5 group compositing. `composited` counts groups rendered
        // to their own buffer and applied as a unit.
        //
        // NOT a clean census, and this was measured on 2026-08-18 rather
        // than reasoned: a `tiny_skia::Pixmap` starts TRANSPARENT, and a
        // transparent initial backdrop IS isolated semantics (§11.4.7).
        // pdfcer allocates a buffer whenever the outer graphics state is
        // non-neutral — so a NON-isolated group under a `/BM` silently
        // becomes an isolated one, and every blend inside it composites
        // against nothing. On the suite transparency patches that is 14,
        // 15 and 7 wrong cells out of 16, and each one is counted here as
        // a success. `Pass 97.0` SHIPPED that fix on 2026-08-21, and
        // this sentence still read "until it lands this number over-reports"
        // a day afterwards. A non-isolated group now renders over its own
        // backdrop, so the over-report is gone on the additive path.
        //
        // AND IT IS GONE ON THE SUBTRACTIVE PATH TOO, as of
        // `Pass 97.1g`. This sentence read "it survives on a SUBTRACTIVE
        // page, where a non-isolated group is still composited as if
        // isolated" until 2026-08-24. A subtractive page now takes the
        // same two-walk route the additive one has taken since `97.0`,
        // through `CmykBuffer::composite_non_isolated`.
        //
        // What survives is narrower and worth naming rather than
        // rounding to zero: a non-isolated group whose SECOND buffer
        // could not be allocated still falls back to the isolated
        // approximation, and is still counted in
        // `cmyk_groups_approximated`.
        //
        // `knockout_approx` CHANGED MEANING in `Pass 97.0`. It used to
        // read "/K groups get their outer boundary right and their
        // internal occlusion order wrong", because knockout was
        // unimplemented. §11.4.6 is implemented now, so this counts only
        // the ELEMENTS inside a knockout group that read the destination
        // back and therefore could not be given knockout semantics — a
        // group pdfcer renders exactly reports zero. What it has never
        // counted is the implicit knockout population (§9.3.8 `/TK` text,
        // §11.7.4.4 `B`/`b`, §11.6.7 shading patterns), none of which is
        // treated as knockout at all.
        d.transparency_groups_composited,
        d.transparency_groups_knockout_approximated,
        // §8.6.7 overprint. SIMULATED since `bf75351`, per-pixel, through
        // CompatibleOverprint (§11.7.4.3, Table 149) — see
        // `overprint_composited` below for what actually ran.
        //
        // This comment used to read "Tracked and reported, not
        // simulated: pdfcer composites in additive RGB and there is no
        // per-colorant state for overprint to preserve." That was true
        // when written and false from `bf75351` onward, and it survived
        // the fix to the stdout note it sits directly above (`e11b4f8`,
        // which corrected the prose and missed the comment describing the
        // same numbers four lines up). Third occurrence of the shape;
        // recorded because a stale comment is the half nobody re-reads.
        //
        // What IS still approximate, and it is NARROWER since `Pass 228.0`,
        // `238.0` and `239.0`: the four PROCESS colorants are preserved, and
        // a spot colorant painted by a PATH FILL, a STENCIL MASK, a SAMPLED
        // IMAGE, an axial/radial/function SHADING or a shading PATTERN keeps
        // a plane of its own, through transparency groups and knockout
        // groups too. A spot painted by a MESH shading (types 4-7) still
        // goes through its tint transform and cannot be left standing the
        // way a press leaves it.
        //
        // FOURTH occurrence of the stale-comment shape this block already
        // names three of: said "a SPOT colorant has no plane of its own"
        // unconditionally until 2026-09-02, and survived that day's sweep of
        // six sibling sites because the grep matched a phrasing this one
        // does not use.
        d.overprint_requested,
        d.overprint_mode1_requested,
        // The subset that is a REAL difference: `overprint_requested` is
        // enabled far more often than it matters, and this is the set that
        // is composited through Table 149 rather than blended Normal.
        d.overprint_effective,
        // What actually ran, what could not, and how much it moved.
        // `overprint_refused` is the one to watch -- it is a shortfall the
        // operator cannot detect by looking at the page.
        d.overprint_composited,
        d.overprint_refused,
        d.overprint_pixels,
        // The four NON-SEPARABLE blend modes take their own code path --
        // pdfcer computes Table 137 per pixel because the rasteriser's
        // versions are wrong (decision 066). They are counted separately
        // from `blend_modes_applied` because the two paths fail
        // independently, and a page can exercise one without the other.
        d.nonseparable_composited,
        d.nonseparable_pixels,
        // §11.4.4's second walk. NOT a shortfall — the only place in the
        // renderer where one page's content stream is interpreted twice,
        // disclosed because nothing else makes that visible. Zero is the
        // normal reading and does not mean non-isolated groups were
        // mishandled: §11.4.4 NOTE 5 makes the single walk exact whenever
        // the group's interior composites Normal throughout.
        d.transparency_groups_backdrop_reruns,
        // 11.4.5 soft masks that reached the GROUP RESULT rather than
        // being folded into the contents' clip.
        //
        // Read it against `soft_masks_applied`, and the difference is NOT
        // a shortfall on its own: a mask on an elementary object belongs
        // in the clip, because 11.6.4.1 makes the mask value that object's
        // q_m and a q_m multiplies coverage exactly as a clip does. What
        // this counter is for is the other case -- a document with
        // transparency groups where this stays at zero while
        // `soft_masks_reset_stale` climbs is one whose group masks are
        // being applied once per object inside instead of once to the
        // composite.
        d.soft_masks_on_group_result,
        // Images painted under /OP true that CompatibleOverprint was
        // never offered — a whole object class with no path, distinct from
        // `overprint_refused`'s "offered and could not run". Non-zero on
        // four suite patches, and it is the number that has to fall before
        // the /Indexed colorant fix can be observed at all.
        d.overprint_images_unsupported,
        d.overprint_shadings_unsupported,
        // 11.3.4's blending colour space, and how often it mattered.
        //
        // `blend_space_subtractive` counts the page and every transparency
        // group whose blending space is DeviceCMYK / Separation / DeviceN,
        // or a four-component ICCBased resolving to one. It is a CENSUS,
        // not a shortfall on its own -- a page can be entirely DeviceCMYK
        // and entirely correct, because Normal is c_s on either side of
        // the complement.
        //
        // `blends_in_wrong_space` is the shortfall: non-Normal blend modes
        // pdfcer computed ADDITIVELY inside one of those groups, where
        // 11.3.4 requires the components complemented before the blend
        // function and complemented back after it. Non-zero means the
        // picture is plausible and wrong -- on suite PCS1_162's Difference
        // cell the two answers are green and magenta.
        //
        // SINCE Pass 97.1e THIS ONLY FIRES WHERE THE COLORANT BUFFER DID
        // NOT RUN. A subtractive page composites in ink and reports zero
        // here; read it together with `cmyk_buffer` below, never alone.
        d.blend_space_subtractive,
        d.blend_space_from_output_intent,
        d.blends_in_wrong_space,
        // THE KEY THAT CHANGES WHAT THE PREVIOUS ONE MEANS. When
        // `cmyk_buffer=1` the blends counted by `blends_in_wrong_space`
        // were PERFORMED subtractively -- that counter is fixed at
        // `/BM`-selection time and measures exposure to 11.3.4, not
        // failure. Read the pair, never the second alone.
        u8::from(d.cmyk_buffer_engaged),
        // The page asked for ink and the buffer would not fit (the byte
        // ceiling). Non-zero means this render is the pre-97.1e
        // approximation, disclosed rather than failed.
        d.cmyk_buffer_refused,
        // Pixels that entered the buffer as CONVERTED sRGB rather than as
        // authored ink -- images, shadings, group results. A disclosure:
        // an image's samples were flattened to sRGB in the decode loop
        // long before any canvas saw them.
        d.cmyk_bridged_pixels,
        // A real shortfall, and the one Pass 97.1f removes: the group's
        // RESULT composited in ink, its INTERIOR did not.
        d.cmyk_groups_approximated,
        // Should always be zero; see the field's docs for why it is
        // counted rather than asserted.
        d.cmyk_unbridged_images,
        // `Pass 130.1`. Appended per the stable-line append-never-reorder
        // rule: the complement of `cmyk_bridged_pixels`.
        d.cmyk_native_image_pixels,
        d.rendering_intents_set,
        // `Pass 199.2`. Appended per the stable-line append-never-reorder
        // rule. The PAIR is the measurement: `managed` alone cannot
        // distinguish "the engine ran and agreed" from "the branch was never
        // reached", which is precisely the ambiguity that made an earlier
        // ablation in this area uninterpretable.
        d.icc_managed_paints,
        d.icc_unmanaged_paints,
        // `Pass 204.0`. Appended per the stable-line append-never-insert rule.
        d.overprint_process_images_unsupported,
        // APPENDED, not inserted beside its `annots_*` relatives. The
        // module docs promise "keys are appended, never reordered", and a
        // contract test asserts the whole list — inserting this next to
        // `annots_no_ap`, where it reads better, broke that test and would
        // have broken every positional parser downstream. Readability of the
        // line is not worth a published contract.
        d.annotations_icon_painted,
        // Appended under the same contract, `Pass 290.0`: this page's
        // `/Resources` was on neither the page nor any ancestor, so pdfcer
        // supplied the empty dictionary every name on the page was looked up
        // in. 1 explains an otherwise unexplained pile of `unsupported=`,
        // `cs_unresolved=` and unpainted forms; on a page with no content it
        // is simply a true fact about a file Acrobat writes.
        usize::from(d.page_resources_defaulted),
    )
}

/// Everything `copy-page` was asked for (`Pass 248.2`).
pub(crate) struct CopyPageArgs<'a> {
    pub(crate) input: &'a Path,
    /// 1-based.
    pub(crate) page: u32,
    pub(crate) dpi: f32,
    pub(crate) svg: bool,
    pub(crate) emf: bool,
    pub(crate) raster: bool,
    pub(crate) pdf: bool,
    pub(crate) background: Option<&'a str>,
    pub(crate) annotations: bool,
    pub(crate) font_dirs: &'a [PathBuf],
    pub(crate) show_layers: &'a [String],
    pub(crate) hide_layers: &'a [String],
}

/// `copy-page`: produce the page in every clipboard format the target
/// applications read, and place them in one transaction (`Pass 248.2`).
///
/// # Contract
///
/// - Exit `SUCCESS` with the stable line `copied <input> page <N> ->
///   clipboard formats=<a,b,c> <W>x<H> dpi=<D> background=<#rrggbb|none>;
///   <counters>` (the same counter half as `export-image`), then the `svg:`
///   disclosure line when the SVG was placed, then the per-page notes.
/// - `RUNTIME_ERROR` for a page out of range, a bad `--background`, every
///   payload switched off, a render failure, or a clipboard the OS would
///   not hand over. On a non-Windows build: refused by name, pointing at
///   `export-image`, which writes the same bytes to files.
///
/// # Why the payloads are produced BEFORE the clipboard is opened
///
/// A render can take seconds on a CAD sheet; Windows serialises clipboard
/// access across every process, and holding it open while rendering would
/// stall the operator's other applications for that long. Everything is
/// built first; the transaction itself is milliseconds.
pub(crate) fn cmd_copy_page(args: CopyPageArgs<'_>) -> u8 {
    use pdfcer_render::export::{Rgb, encode_png, flatten_over};

    let CopyPageArgs {
        input,
        page: page_number,
        dpi,
        svg: want_svg,
        emf: want_emf,
        raster: want_raster,
        pdf: want_pdf,
        background,
        annotations,
        font_dirs,
        show_layers,
        hide_layers,
    } = args;

    if !(want_svg || want_emf || want_raster || want_pdf) {
        eprintln!(
            "pdfcer: {}: every payload is switched off (--no-svg --no-emf --no-raster --no-pdf); nothing to copy",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    if !dpi.is_finite() || dpi <= 0.0 {
        eprintln!(
            "pdfcer: {}: --dpi must be a positive number, got {dpi}",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    let background = match background.map(Rgb::parse_hex) {
        None => None,
        Some(Ok(rgb)) => Some(rgb),
        Some(Err(msg)) => {
            eprintln!("pdfcer: {}: --background: {msg}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };

    let (font_env, supplied_registered, font_notes) = build_font_environment(font_dirs);
    for note in &font_notes {
        eprintln!("pdfcer: font-dir: {note}");
    }
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let pages = match pdfcer_core::page_tree::pages(&doc) {
        Ok(pages) => pages,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let Some(page) = page_number
        .checked_sub(1)
        .and_then(|i| pages.get(i as usize))
    else {
        eprintln!(
            "pdfcer: {}: page {page_number} is out of range (document has {} page(s), numbered 1..={})",
            input.display(),
            pages.len(),
            pages.len()
        );
        return exit::RUNTIME_ERROR;
    };

    let scale = dpi / 72.0;
    let mut render_options = match resolve_render_options(
        &doc,
        RenderFlags {
            verb: "copy-page",
            input,
            standard: None,
            overprint_zero_tint_scope: None,
            spot_colorant_device_model: None,
            max_cmyk_buffer_bytes: None,
            annotations,
            font_env,
            fast_subpixel: false,
            probe_ink: None,
            print_state: false,
            scale,
            show_layers,
            hide_layers,
        },
    ) {
        Ok(options) => options,
        Err(code) => return code,
    };
    // Rendered TRANSPARENT and flattened per payload if a background was
    // asked for -- one render serves every format.
    render_options.backdrop = pdfcer_render::PageBackdrop::Transparent;

    let mut payload = clipboard::ClipboardPayload::default();
    let mut svg_outcome = None;
    let mut counters_diag = None;
    let mut size = (0u32, 0u32);

    if want_svg {
        let svg_options = pdfcer_render::svg::SvgOptions::default()
            .with_raster_dpi(dpi)
            .with_background(background);
        match pdfcer_render::svg::export_svg(&doc, page, &render_options, &svg_options) {
            Ok(export) => {
                size = (
                    num_px(export.outcome.width_pt * export.outcome.scale),
                    num_px(export.outcome.height_pt * export.outcome.scale),
                );
                payload.svg = Some(export.svg);
                svg_outcome = Some(export.outcome);
            }
            Err(err) => {
                eprintln!(
                    "pdfcer: {}: page {page_number}: svg: {err}",
                    input.display()
                );
                return exit::RUNTIME_ERROR;
            }
        }
    }
    let mut emf_outcome = None;
    if want_emf {
        let emf_options = pdfcer_render::emf::EmfOptions::default()
            .with_raster_dpi(dpi)
            .with_background(background);
        match pdfcer_render::emf::export_emf(&doc, page, &render_options, &emf_options) {
            Ok(export) => {
                payload.emf = Some(export.emf);
                emf_outcome = Some(export.outcome);
            }
            Err(err) => {
                eprintln!(
                    "pdfcer: {}: page {page_number}: emf: {err}",
                    input.display()
                );
                return exit::RUNTIME_ERROR;
            }
        }
    }
    if want_raster {
        let rendered = match pdfcer_render::render_page_with(&doc, page, scale, &render_options) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("pdfcer: {}: page {page_number}: {err}", input.display());
                return exit::RUNTIME_ERROR;
            }
        };
        let flat = match background {
            Some(bg) => flatten_over(&rendered.pixmap, bg).into_owned(),
            None => rendered.pixmap.clone(),
        };
        match encode_png(&flat, Some(dpi)) {
            Ok(png) => payload.png = Some(png),
            Err(err) => {
                eprintln!("pdfcer: {}: page {page_number}: {err}", input.display());
                return exit::RUNTIME_ERROR;
            }
        }
        size = (flat.width(), flat.height());
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            payload.pixels_per_metre = (f64::from(dpi) / 0.0254).round().max(0.0) as u32;
        }
        payload.pixmap = Some(flat);
        counters_diag = Some(rendered.diagnostics);
    }
    if want_pdf {
        let view = DocumentView::new(&doc, doc.bytes(), doc.version());
        match pdfcer_core::pageops::extract(&view, &[page_number as usize - 1]) {
            Ok((bytes, _report)) => payload.pdf = Some(bytes),
            Err(err) => return report_page_op_error(&err),
        }
    }

    let placed = match clipboard::place(&payload) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("pdfcer: {}: page {page_number}: {err}", input.display());
            if matches!(err, clipboard::ClipboardError::Unsupported) {
                eprintln!(
                    "pdfcer: note: `export-image --format svg|png` writes the same bytes to files on every platform"
                );
            }
            return exit::RUNTIME_ERROR;
        }
    };

    // The counter half comes from the raster render when there was one,
    // else from the SVG's own walk -- same interpreter, same numbers.
    let diag = counters_diag
        .as_ref()
        .or(svg_outcome.as_ref().map(|o| &o.diagnostics));
    let background_token = background.map_or_else(|| "none".to_owned(), Rgb::to_hex);
    println!(
        "copied {} page {page_number} -> clipboard formats={} {}x{} dpi={dpi} background={}; {}",
        input.display(),
        placed.formats.join(","),
        size.0,
        size.1,
        background_token,
        diag.map_or_else(String::new, |d| render_counters_line(
            d,
            &doc,
            supplied_registered
        ))
    );
    if let Some(o) = &svg_outcome {
        let t = &o.tally;
        println!(
            "svg: ops={} images={} dashed_pre_applied={} blend_modes={} \
shadings_rasterised={} soft_masks_kept={} overprint_approximated={} \
nonseparable_approximated={} non_isolated_isolated={} colorant_buffer_on_screen={} exact={} \
shadings_as_gradients={}",
            o.ops,
            o.images_embedded,
            o.dashed_strokes_pre_applied,
            o.blend_modes_used,
            t.shadings_rasterised,
            t.soft_masks_kept,
            t.overprint_approximated,
            t.nonseparable_approximated,
            t.non_isolated_groups_isolated,
            t.colorant_buffer_on_screen,
            u8::from(t.is_exact()),
            t.shadings_as_gradients
        );
        eprintln!(
            "pdfcer: note: the vector payload carries text as glyph OUTLINES; a paste into Word or Inkscape is editable as shapes, not as words"
        );
        if !t.is_exact() {
            eprintln!(
                "pdfcer: note: the vector payload embeds {} shading(s) as raster and approximates {} overprint, {} non-separable blend, {} non-isolated group(s) -- see `export-image --format svg` for the per-kind notes",
                t.shadings_rasterised,
                t.overprint_approximated,
                t.nonseparable_approximated,
                t.non_isolated_groups_isolated
            );
        }
        if o.blend_modes_used > 0 {
            eprintln!(
                "pdfcer: note: {} element(s) use mix-blend-mode; Word's SVG importer shows them as Normal, Inkscape honours them",
                o.blend_modes_used
            );
        }
    }
    if let Some(o) = &emf_outcome {
        print_emf_disclosure(o, page_number, false);
    }
    eprintln!(
        "pdfcer: note: a paste takes the FIRST format the application knows: Word/PowerPoint/Excel and Inkscape take the SVG (vectors); LibreOffice 24.x and Paste Special take the EMF; Paint, GIMP, browsers take the PNG (pixels, with transparency)"
    );
    if let Some(d) = diag {
        report_diagnostics(
            d,
            render_options.max_cmyk_buffer_bytes,
            u64::from(size.0) * u64::from(size.1),
        );
    }
    exit::SUCCESS
}

/// The `emf:` stable line and the per-page notes for a metafile export or
/// copy (`Pass 248.4`) — one function, so `export-image` and `copy-page`
/// disclose the same facts in the same words.
pub(crate) fn print_emf_disclosure(
    o: &pdfcer_render::emf::EmfOutcome,
    page_number: impl std::fmt::Display,
    keep_text: bool,
) {
    println!(
        "emf: ops={} rasters={} alpha_rasterised={} blend_modes_dropped={} gradients_rasterised={} \
images={} layers_rasterised={} dashed_pre_applied={} nonzero_multi_subpath={}",
        o.ops,
        o.rasters_embedded,
        o.ops_rasterised_for_alpha,
        o.blend_modes_dropped,
        o.gradients_rasterised,
        o.images_embedded,
        o.layers_rasterised,
        o.dashed_strokes_pre_applied,
        o.nonzero_fills_multi_subpath
    );
    if keep_text {
        let k = &o.text;
        println!(
            "emf-text: kept={} outlines={} paint={} unmapped={} geometry={} symbol_face={}",
            k.runs_as_text,
            k.runs_as_outlines(),
            k.fallback_paint,
            k.fallback_unmapped,
            k.fallback_geometry,
            k.fallback_symbol_face
        );
        eprintln!(
            "pdfcer: note: page {page_number}: {} text run(s) kept as text, each character at its PDF position; an EMF cannot carry a font, so each is drawn in the INSTALLED font of the recorded name, and a machine without it substitutes one. The characters come from the PDF's /ToUnicode and encoding. {} run(s) stay glyph OUTLINES",
            k.runs_as_text,
            k.runs_as_outlines()
        );
    } else {
        eprintln!(
            "pdfcer: note: page {page_number}: the metafile carries text as glyph OUTLINES; it is the format for LibreOffice 24.x and legacy Win32 consumers -- Word, PowerPoint and Inkscape take the SVG instead"
        );
    }
    if o.rasters_embedded > 0 {
        eprintln!(
            "pdfcer: note: page {page_number}: {} element(s) are embedded as alpha bitmaps because EMF cannot express them as vectors ({} for transparency, {} for blend modes, {} gradients, {} images, {} transparency groups); Inkscape's EMF import draws none of those",
            o.rasters_embedded,
            o.ops_rasterised_for_alpha,
            o.blend_modes_dropped,
            o.gradients_rasterised,
            o.images_embedded,
            o.layers_rasterised
        );
    }
    if o.nonzero_fills_multi_subpath > 0 {
        eprintln!(
            "pdfcer: note: page {page_number}: {} nonzero-rule fill(s) have several subpaths; LibreOffice 24.x ignores the fill rule and may show holes where they overlap",
            o.nonzero_fills_multi_subpath
        );
    }
}

/// A device dimension recovered from points × scale, rounded up the way
/// `page_device_geometry` rounds — for the stable line's `WxH` on the SVG
/// route, where no pixmap exists to ask.
pub(crate) fn num_px(v: f32) -> u32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = v.round().max(0.0) as u32;
    n
}

/// Everything `export-image` was asked for (`Pass 248.0`).
///
/// A struct for the reason `ExportDxfArgs` is one: the flag set is past
/// the point where positional parameters read as documentation.
pub(crate) struct ExportImageArgs<'a> {
    pub(crate) input: &'a Path,
    /// A `parse_pages` spec.
    pub(crate) pages: &'a str,
    pub(crate) format: ImageFormatArg,
    /// Pixel density; the render scale is `dpi / 72`.
    pub(crate) dpi: f32,
    /// Keep the page group's alpha (PNG only).
    pub(crate) transparent: bool,
    /// JPEG quality, 1–100.
    pub(crate) quality: u8,
    /// `#rrggbb` the page is flattened onto; `None` means white.
    pub(crate) background: Option<&'a str>,
    /// Single-page destination.
    pub(crate) output: Option<&'a Path>,
    /// Multi-page destination directory.
    pub(crate) output_dir: Option<&'a Path>,
    pub(crate) standard: Option<&'a str>,
    pub(crate) overprint_zero_tint_scope: Option<&'a str>,
    pub(crate) spot_colorant_device_model: Option<&'a str>,
    pub(crate) annotations: bool,
    pub(crate) fast_subpixel: bool,
    pub(crate) max_cmyk_buffer_bytes: Option<&'a str>,
    pub(crate) font_dirs: &'a [PathBuf],
    pub(crate) show_layers: &'a [String],
    pub(crate) hide_layers: &'a [String],
    pub(crate) print_state: bool,
    /// Outlines or kept text (SVG only).
    pub(crate) svg_text: SvgTextArg,
    /// Outlines or kept text (EMF only).
    pub(crate) emf_text: SvgTextArg,
}

/// `export-image`: render each selected page and write it as a PNG or JPEG
/// (`Pass 248.0`).
///
/// # Contract
///
/// - Exit `SUCCESS` with one stable stdout line per page:
///   `exported <input> page <N> -> <path> <W>x<H> format=<png|jpeg>
///   dpi=<D> transparent=<0|1> background=<#rrggbb|none>; <counters>` where
///   `<counters>` is byte-for-byte `render-page`'s counter set
///   ([`render_counters_line`]). Then `report_diagnostics`'s stderr notes,
///   per page.
/// - `RUNTIME_ERROR` before touching the disk for: `--transparent` with
///   JPEG (no alpha channel — refused by name, never flattened silently);
///   a `--quality` outside 1–100; a non-positive or non-finite `--dpi`; a
///   `--background` that is not `#rrggbb`; a page spec selecting more than
///   one page without `--output-dir`; neither destination flag; and every
///   refusal `resolve_render_options` makes.
/// - `IO_ERROR` when a file cannot be written. Pages before it are on disk
///   and their lines were printed; the run stops at the first failure
///   rather than continuing to fill a directory it cannot write to.
///
/// # Why the whole render option set is shared with `render-page`
///
/// See [`RenderFlags`]. The short version: a flag `export-image` parsed and
/// did not honour would look, to the operator, exactly like the flag
/// working.
pub(crate) fn cmd_export_image(args: ExportImageArgs<'_>) -> u8 {
    use pdfcer_render::export::{JpegOptions, Rgb, encode_jpeg, encode_png, flatten_over};

    let ExportImageArgs {
        input,
        pages: pages_spec,
        format,
        dpi,
        transparent,
        quality,
        background,
        output,
        output_dir,
        standard,
        overprint_zero_tint_scope,
        spot_colorant_device_model,
        annotations,
        fast_subpixel,
        max_cmyk_buffer_bytes,
        font_dirs,
        show_layers,
        hide_layers,
        print_state,
        svg_text,
        emf_text,
    } = args;

    // ---- operator-mistake refusals, all before any I/O ----
    if svg_text == SvgTextArg::Keep && format != ImageFormatArg::Svg {
        // Refused by name: a raster or EMF has no text to keep, and
        // accepting the flag would look like it did something.
        eprintln!(
            "pdfcer: {}: --svg-text keep applies to --format svg only",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    if emf_text == SvgTextArg::Keep && format != ImageFormatArg::Emf {
        eprintln!(
            "pdfcer: {}: --emf-text keep applies to --format emf only",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    if transparent && format == ImageFormatArg::Jpeg {
        // Refused by name. JPEG has no alpha channel; flattening silently
        // would hand back a file that looks exactly like the flag working.
        eprintln!(
            "pdfcer: {}: --transparent cannot be honoured for JPEG, which has no alpha channel; drop the flag (the page is flattened onto white, or onto --background) or use --format png",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    if !(1..=100).contains(&quality) {
        eprintln!(
            "pdfcer: {}: --quality {quality} is outside 1..=100",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    if !dpi.is_finite() || dpi <= 0.0 {
        eprintln!(
            "pdfcer: {}: --dpi must be a positive number, got {dpi}",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    let background = match background.map(Rgb::parse_hex) {
        None => None,
        Some(Ok(rgb)) => Some(rgb),
        Some(Err(msg)) => {
            eprintln!("pdfcer: {}: --background: {msg}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    if output.is_none() && output_dir.is_none() {
        eprintln!(
            "pdfcer: {}: nowhere to write — pass --output <file> for one page, or --output-dir <dir> for several",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }

    // Font environment before the document, as `render-page` does: the
    // walk is shell-side I/O and a bad directory is a note, never fatal.
    let (font_env, supplied_registered, font_notes) = build_font_environment(font_dirs);
    for note in &font_notes {
        eprintln!("pdfcer: font-dir: {note}");
    }

    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let pages = match pdfcer_core::page_tree::pages(&doc) {
        Ok(pages) => pages,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let selected = match parse_pages(pages_spec, pages.len()) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("pdfcer: {}: --pages: {msg}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    if selected.len() > 1 && output_dir.is_none() {
        eprintln!(
            "pdfcer: {}: --pages selected {} pages but --output names one file; pass --output-dir <dir> instead",
            input.display(),
            selected.len()
        );
        return exit::RUNTIME_ERROR;
    }

    // scale = dpi / 72 is the engine's own unit (`render-page --scale`).
    let scale = dpi / 72.0;
    let mut render_options = match resolve_render_options(
        &doc,
        RenderFlags {
            verb: "export-image",
            input,
            standard,
            overprint_zero_tint_scope,
            spot_colorant_device_model,
            max_cmyk_buffer_bytes,
            annotations,
            font_env,
            fast_subpixel,
            probe_ink: None,
            print_state,
            scale,
            show_layers,
            hide_layers,
        },
    ) {
        Ok(options) => options,
        Err(code) => return code,
    };
    // The one option `render-page` never sets. A JPEG is rendered
    // TRANSPARENT too, and flattened by the encoder over `--background`:
    // one render serves any backdrop, and the renderer never learns a
    // colour it would then have to disclose.
    render_options.backdrop =
        if transparent || background.is_some() || format == ImageFormatArg::Jpeg {
            pdfcer_render::PageBackdrop::Transparent
        } else {
            pdfcer_render::PageBackdrop::White
        };

    // Zero-padded to the widest 1-based page number in the run, so a
    // directory listing sorts in page order (`export-dxf`'s convention).
    let width = selected
        .iter()
        .map(|i| (i + 1).to_string().len())
        .max()
        .unwrap_or(1);
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "page".to_owned());

    for &index in &selected {
        let page = &pages[index];
        let page_number = index + 1;
        let path = match (output, output_dir) {
            (Some(file), _) => file.to_path_buf(),
            (None, Some(dir)) => dir.join(format!(
                "{stem}_p{page_number:0width$}.{}",
                format.extension()
            )),
            (None, None) => unreachable!("checked above"),
        };

        if format == ImageFormatArg::Emf {
            // The Windows-metafile route (`Pass 248.4`): the same export
            // recording as SVG, written as [MS-EMF] records. `--dpi` is the
            // recording scale and the resolution of every bitmap inside it;
            // `--transparent` is EMF's natural state (nothing is drawn where
            // nothing was painted) and `--background` an opaque first fill.
            let emf_options = pdfcer_render::emf::EmfOptions::default()
                .with_raster_dpi(dpi)
                .with_background(if transparent { None } else { background })
                .with_text(emf_text.into());
            let export =
                match pdfcer_render::emf::export_emf(&doc, page, &render_options, &emf_options) {
                    Ok(e) => e,
                    Err(err) => {
                        eprintln!("pdfcer: {}: page {page_number}: {err}", input.display());
                        return exit::RUNTIME_ERROR;
                    }
                };
            if let Err(err) = std::fs::write(&path, &export.emf) {
                eprintln!("pdfcer: {}: {err}", path.display());
                return exit::IO_ERROR;
            }
            let o = &export.outcome;
            let background_token = match (transparent, background) {
                (true, _) | (false, None) => "none".to_owned(),
                (false, Some(bg)) => bg.to_hex(),
            };
            println!(
                "exported {} page {page_number} -> {} {}x{} format=emf dpi={dpi} transparent={} background={}; {}",
                input.display(),
                path.display(),
                num_px(o.width_pt * dpi / 72.0),
                num_px(o.height_pt * dpi / 72.0),
                u8::from(background_token == "none"),
                background_token,
                render_counters_line(&o.diagnostics, &doc, supplied_registered)
            );
            print_emf_disclosure(o, page_number, emf_text == SvgTextArg::Keep);
            report_diagnostics(
                &o.diagnostics,
                render_options.max_cmyk_buffer_bytes,
                u64::from(num_px(o.width_pt * dpi / 72.0))
                    * u64::from(num_px(o.height_pt * dpi / 72.0)),
            );
            continue;
        }
        if format == ImageFormatArg::Svg {
            // The vector route (`Pass 248.1`): one recording of the page
            // through the export recorder, written as SVG. `--dpi` is the
            // recording scale, which only matters for what the writer
            // had to embed as raster; `--transparent` is the SVG's natural
            // state, so it is accepted and means "no --background".
            let svg_options = pdfcer_render::svg::SvgOptions::default()
                .with_raster_dpi(dpi)
                .with_background(if transparent { None } else { background })
                .with_text(svg_text.into());
            let export =
                match pdfcer_render::svg::export_svg(&doc, page, &render_options, &svg_options) {
                    Ok(e) => e,
                    Err(err) => {
                        eprintln!("pdfcer: {}: page {page_number}: {err}", input.display());
                        return exit::RUNTIME_ERROR;
                    }
                };
            if let Err(err) = std::fs::write(&path, export.svg.as_bytes()) {
                eprintln!("pdfcer: {}: {err}", path.display());
                return exit::IO_ERROR;
            }
            let o = &export.outcome;
            let background_token = match (transparent, background) {
                (true, _) | (false, None) => "none".to_owned(),
                (false, Some(bg)) => bg.to_hex(),
            };
            // The same prefix and the same counter half as a raster
            // export -- WxH is the recording grid at `--dpi` -- so one
            // parser reads every format.
            println!(
                "exported {} page {page_number} -> {} {}x{} format=svg dpi={dpi} transparent={} background={}; {}",
                input.display(),
                path.display(),
                num_px(o.width_pt * o.scale),
                num_px(o.height_pt * o.scale),
                u8::from(background_token == "none"),
                background_token,
                render_counters_line(&o.diagnostics, &doc, supplied_registered)
            );
            // A SECOND LINE, NOT MORE KEYS ON THE FIRST -- the SVG-only
            // disclosure, prefixed so a parser can take or leave it whole
            // (the same shape as `render-page`'s ink-probe line). `exact=1`
            // means the whole page went out as geometry.
            let t = &o.tally;
            println!(
                "svg: ops={} images={} dashed_pre_applied={} blend_modes={} \
shadings_rasterised={} soft_masks_kept={} overprint_approximated={} \
nonseparable_approximated={} non_isolated_isolated={} colorant_buffer_on_screen={} exact={} \
shadings_as_gradients={}",
                o.ops,
                o.images_embedded,
                o.dashed_strokes_pre_applied,
                o.blend_modes_used,
                t.shadings_rasterised,
                t.soft_masks_kept,
                t.overprint_approximated,
                t.nonseparable_approximated,
                t.non_isolated_groups_isolated,
                t.colorant_buffer_on_screen,
                u8::from(t.is_exact()),
                t.shadings_as_gradients
            );
            // Rule 4 in prose, once per page: what is inferred or
            // approximated in the file, by name.
            if svg_text == SvgTextArg::Keep {
                let k = &o.text;
                println!(
                    "svg-text: kept={} outlines={} fonts={} not_sfnt={} paint={} unmapped={} conflict={} geometry={} font_build={} restricted={}",
                    k.runs_as_text,
                    k.runs_as_outlines(),
                    k.fonts_embedded,
                    k.fallback_not_sfnt,
                    k.fallback_paint,
                    k.fallback_unmapped,
                    k.fallback_conflict,
                    k.fallback_geometry,
                    k.fallback_font_build,
                    k.fallback_restricted
                );
                eprintln!(
                    "pdfcer: note: page {page_number}: {} text run(s) kept as text in {} embedded font(s), each character at its PDF position; the characters come from the PDF's /ToUnicode and encoding, so text a PDF maps wrongly is written wrongly. {} run(s) stay glyph OUTLINES. Every raster inside the SVG is sampled at {dpi} dpi",
                    k.runs_as_text,
                    k.fonts_embedded,
                    k.runs_as_outlines()
                );
            } else {
                eprintln!(
                    "pdfcer: note: page {page_number}: text is exported as glyph OUTLINES (not editable as text; --svg-text keep writes real text); every raster inside the SVG is sampled at {dpi} dpi"
                );
            }
            if t.shadings_rasterised > 0 {
                eprintln!(
                    "pdfcer: note: page {page_number}: {} shading(s) are embedded as RASTER images (function-based, mesh, two-circle radial, or carrying /Background or /BBox); {} went out as native gradients",
                    t.shadings_rasterised, t.shadings_as_gradients
                );
            }
            if t.soft_masks_kept > 0 {
                eprintln!(
                    "pdfcer: note: page {page_number}: {} soft mask(s) are carried as luminance mask images",
                    t.soft_masks_kept
                );
            }
            if t.overprint_approximated > 0 || t.nonseparable_approximated > 0 {
                eprintln!(
                    "pdfcer: note: page {page_number}: {} overprinted paint(s) and {} non-separable blend(s) are drawn Normal -- SVG cannot express either per paint",
                    t.overprint_approximated, t.nonseparable_approximated
                );
            }
            if t.non_isolated_groups_isolated > 0 {
                eprintln!(
                    "pdfcer: note: page {page_number}: {} non-isolated group(s) are composited as isolated",
                    t.non_isolated_groups_isolated
                );
            }
            if t.colorant_buffer_on_screen > 0 {
                eprintln!(
                    "pdfcer: note: page {page_number}: the page declares a subtractive blending space; the SVG composites on screen (sRGB) instead"
                );
            }
            if o.blend_modes_used > 0 {
                eprintln!(
                    "pdfcer: note: page {page_number}: {} element(s) use mix-blend-mode, which Inkscape and browsers honour and Word's SVG importer shows as Normal",
                    o.blend_modes_used
                );
            }
            report_diagnostics(
                &o.diagnostics,
                render_options.max_cmyk_buffer_bytes,
                u64::from(num_px(o.width_pt * o.scale)) * u64::from(num_px(o.height_pt * o.scale)),
            );
            continue;
        }
        let rendered = match pdfcer_render::render_page_with(&doc, page, scale, &render_options) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("pdfcer: {}: page {page_number}: {err}", input.display());
                return exit::RUNTIME_ERROR;
            }
        };

        let bytes = match format {
            ImageFormatArg::Png => {
                // A non-transparent PNG with a chosen background is
                // flattened here, over that colour; the white default was
                // already composited by the renderer.
                let flat = match background {
                    Some(bg) if !transparent => flatten_over(&rendered.pixmap, bg),
                    _ => std::borrow::Cow::Borrowed(&rendered.pixmap),
                };
                encode_png(&flat, Some(dpi))
            }
            ImageFormatArg::Jpeg => {
                let mut opts = JpegOptions::default();
                opts.quality = quality;
                opts.background = background.unwrap_or(Rgb::WHITE);
                opts.dpi = Some(dpi);
                encode_jpeg(&rendered.pixmap, &opts)
            }
            // Written and `continue`d above, before the raster render.
            ImageFormatArg::Svg | ImageFormatArg::Emf => continue,
        };
        let bytes = match bytes {
            Ok(b) => b,
            Err(err) => {
                eprintln!("pdfcer: {}: page {page_number}: {err}", path.display());
                return exit::RUNTIME_ERROR;
            }
        };
        if let Err(err) = std::fs::write(&path, &bytes) {
            eprintln!("pdfcer: {}: {err}", path.display());
            return exit::IO_ERROR;
        }

        let d = &rendered.diagnostics;
        let background_token = match (transparent, background) {
            (true, _) => "none".to_owned(),
            (false, Some(bg)) => bg.to_hex(),
            (false, None) => Rgb::WHITE.to_hex(),
        };
        println!(
            "exported {} page {page_number} -> {} {}x{} format={} dpi={dpi} transparent={} background={}; {}",
            input.display(),
            path.display(),
            rendered.pixmap.width(),
            rendered.pixmap.height(),
            format.as_str(),
            u8::from(transparent),
            background_token,
            render_counters_line(d, &doc, supplied_registered)
        );
        if transparent {
            // Rule 4, the positive direction: say what the file IS, since a
            // viewer that draws transparency over a checkerboard and one
            // that draws it over white will show two different pictures.
            eprintln!(
                "pdfcer: note: page {page_number} was exported with its transparency kept — pixels nothing painted are fully transparent, not white"
            );
        }
        report_diagnostics(
            d,
            render_options.max_cmyk_buffer_bytes,
            u64::from(rendered.pixmap.width()) * u64::from(rendered.pixmap.height()),
        );
    }

    exit::SUCCESS
}

/// Resolve `--show-layer` / `--hide-layer` names into a complete
/// [`pdfcer_render::LayerVisibility`].
///
/// # The set REPLACES the document's configuration, so it is built from it
///
/// `LayerVisibility` is not a patch (see that type's module docs): the
/// renderer uses it *instead of* `/OCProperties /D`. So the answer starts
/// from [`pdfcer_core::annot::optional_content_default_off`] — what the
/// document asks for — and applies the operator's names on top. Passing
/// only the named groups would show every layer the document had turned
/// off, which is a wrong raster that looks plausible.
///
/// # Errors
///
/// Returns the offending name when it appears in BOTH lists. That is
/// refused rather than resolved by flag order: the operator asked for two
/// contradictory things and there is no reading of the command line that
/// says which one they meant. Order-dependence would make the same two
/// flags mean different things depending on how a script assembled them.
///
/// Names matching no layer are returned as the second tuple element for
/// the caller to report — a note, not a failure, so a batch over a
/// hundred drawings does not abort because one lacks a "Grid" layer.
pub(crate) fn resolve_layer_override(
    doc: &pdfcer_core::document::Document,
    show: &[String],
    hide: &[String],
) -> Result<(pdfcer_render::LayerVisibility, Vec<String>), String> {
    if let Some(clash) = show.iter().find(|n| hide.contains(n)) {
        return Err(clash.clone());
    }
    let graph = doc.view();
    let read = pdfcer_core::layers::read_layers(&graph);
    let mut hidden = pdfcer_core::annot::optional_content_default_off(&graph);
    let mut unmatched = Vec::new();
    for (names, make_visible) in [(show, true), (hide, false)] {
        for name in names {
            let mut matched = false;
            for l in read.layers.iter().filter(|l| &l.name == name) {
                matched = true;
                if make_visible {
                    hidden.remove(&l.id);
                } else {
                    hidden.insert(l.id);
                }
            }
            if !matched {
                unmatched.push(name.clone());
            }
        }
    }
    Ok((pdfcer_render::LayerVisibility::hiding(hidden), unmatched))
}

/// Count of `unsupported` fonts attributed to one reason key (0 when the
/// reason never occurred) — the accessor behind the fixed-order tokens on
/// `render-page`'s stdout line.
pub(crate) fn unsupported_reason(d: &pdfcer_render::Diagnostics, key: &str) -> usize {
    d.fonts_unsupported_by_reason.get(key).copied().unwrap_or(0)
}

/// A `" (reason=count, …)"` suffix naming only the reasons that actually
/// occurred, for the stderr note. Empty when the breakdown is empty (it
/// never is when `fonts_unsupported > 0`, but the guard keeps the caller
/// honest). Reasons are emitted in `UnsupportedFont::all_reason_keys`
/// order so the note is stable.
pub(crate) fn fonts_unsupported_breakdown(d: &pdfcer_render::Diagnostics) -> String {
    let parts: Vec<String> = pdfcer_render::text::UnsupportedFont::all_reason_keys()
        .iter()
        .filter_map(|key| {
            let n = unsupported_reason(d, key);
            (n > 0).then(|| format!("{key}={n}"))
        })
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

/// Write the human-readable half of the R20 honesty report to stderr —
/// the detail that is too unbounded (font names, operator names) to put
/// on the machine-readable stdout line.
///
/// Silent when the render was fully faithful. That silence is the point:
/// it makes "stderr had output" a usable signal in a batch script rather
/// than noise the operator learns to ignore.
pub(crate) fn report_diagnostics(
    d: &pdfcer_render::Diagnostics,
    max_cmyk_buffer_bytes: Option<usize>,
    raster_pixels: u64,
) {
    if !d.substituted_fonts.is_empty() {
        eprintln!(
            "pdfcer: note: {} glyph(s) drawn with bundled substitute faces, not the \
document's own: {}",
            d.glyphs_substituted,
            d.substituted_fonts.join(", ")
        );
    }
    // decision 012 / R62: supplied faces are disclosed SEPARATELY from
    // bundled — the operator's own shapes, not pdfcer's guess. Positions
    // still come from the PDF's `/Widths`, so this improves shapes, not
    // layout (R63: such a render is machine-dependent by definition).
    if !d.supplied_fonts.is_empty() {
        eprintln!(
            "pdfcer: note: {} glyph(s) drawn with operator-supplied faces (shapes only — \
positions still come from the document's own widths, and this render is machine-dependent): {}",
            d.glyphs_supplied,
            d.supplied_fonts.join(", ")
        );
    }
    if d.fonts_unsupported > 0 {
        eprintln!(
            "pdfcer: note: {} font(s) use machinery this build does not implement; \
their text was SKIPPED, not approximated{}",
            d.fonts_unsupported,
            fonts_unsupported_breakdown(d)
        );
    }
    if d.glyphs_notdef > 0 {
        eprintln!(
            "pdfcer: note: {} glyph(s) had no mapping and were drawn as .notdef or omitted",
            d.glyphs_notdef
        );
    }
    // Clause 11 transparency. Split into two notes rather than one,
    // because the two failure DIRECTIONS are opposite and only one of them
    // can expose content: an ignored blend mode composites the same marks
    // by the wrong rule, while an ignored soft mask paints marks the
    // document asked to be faded or hidden.
    if d.blend_modes_applied > 0 {
        eprintln!(
            "pdfcer: note: {} graphics-state(s) selected a non-Normal BLEND MODE (/BM, ISO \
32000-1 §11.3.5) and pdfcer APPLIED it. ★ APPLIED IS NOT APPLIED CORRECTLY, and this note said \
\"Census, not a problem\" until 2026-08-18: §11.3.4 requires blending in the GROUP'S colour \
space with subtractive components complemented before and after, and pdfcer blends in device \
sRGB — so on CMYK content these composited by the wrong rule. Measured: the Difference cell of \
suite PCS3_164 fails, and Difference is |cb - cs|, the mode most sensitive to whether its \
operands were complemented first. Reported because a page whose appearance depends on blending \
is one whose appearance depends on pdfcer's compositing being right, and that is worth knowing \
when comparing against another renderer",
            d.blend_modes_applied
        );
    }
    if d.blend_modes_ignored > 0 {
        eprintln!(
            "pdfcer: note: {} graphics-state(s) named a blend mode outside ISO 32000-1 Tables 136/137, so those marks were composited as Normal. The page is not blank where they are, it is WRONG there, which is harder to notice: a blend composited as Normal looks like an ordinary opaque overlay",
            d.blend_modes_ignored
        );
    }
    if d.overprint_requested > 0 {
        eprintln!(
            "pdfcer: note: {} PAINT(s) were affected by OVERPRINT; {} graphics-state operator(s) enabled it (/OP or /op, ISO 32000-1 §8.6.7){}. pdfcer COMPOSITED {} of those paints through CompatibleOverprint (§11.7.4.3, Table 149) and REFUSED {}. ★ THE FIRST TWO NUMBERS COUNT DIFFERENT THINGS and neither is a subset of the other: the first counts PAINTED OBJECTS, the second counts `gs` OPERATORS, and one `gs` governs every paint until the next one — so a single enable can produce many affected paints, or none. The first is the one to read, because a DeviceCMYK fill over a DeviceCMYK backdrop at overprint mode 0 specifies all four components and is IDENTICAL to Normal, and producers enable overprint document-wide. {} A REFUSED count above zero is stronger still: those paints knocked the backdrop out where a press would have shown ink. On a PDF/X file this matters: Acrobat turns Overprint Preview ON automatically for PDF/X, so the document's EXPECTED appearance includes overprint",
            d.overprint_effective,
            d.overprint_requested,
            if d.overprint_mode1_requested > 0 {
                format!(
                    ", {} of them with overprint MODE 1 (/OPM 1)",
                    d.overprint_mode1_requested
                )
            } else {
                String::new()
            },
            d.overprint_composited,
            d.overprint_refused,
            // THE APPROXIMATION CLAUSE IS NOW CONDITIONAL, because as
            // of `Pass 97.1e` it is FALSE on a subtractive page. The
            // sentence it replaces -- "pdfcer composites in additive RGB
            // with CMYK reconstructed per pixel" -- was true of every
            // render pdfcer had ever performed until a colorant buffer
            // existed, which is exactly the kind of claim that goes stale
            // silently: nothing tests an operator-facing paragraph.
            if d.cmyk_buffer_engaged {
                "★ WHAT IS AND IS NOT APPROXIMATE HERE: this page's blending colour space is SUBTRACTIVE, so pdfcer composited it in a four-colorant buffer plus one plane per spot colorant, and Table 149 read the backdrop's colorants DIRECTLY rather than reconstructing them from an RGB composite. That is the exact case CompatibleOverprint was written for. What remains approximate is ONE thing, and it is NARROWER again than this sentence used to say: a spot colorant painted by a PATH FILL (`Pass 228.0`/`230.0`), a STENCIL MASK or a SAMPLED IMAGE (`Pass 238.0`), an axial, radial or function SHADING or a shading PATTERN (`Pass 239.0`) has a plane of its own and is left standing the way a press leaves it, through transparency and knockout groups as well. What still has no plane is a spot painted by a MESH shading (types 4-7) — that one flattens through its tint transform. ★★ This sentence read 'a SPOT colorant still has no plane of its own' until 2026-09-02, four Passes after that stopped being true; then 'an IMAGE or a SHADING' until `Pass 238.0`; then 'a SHADING' until `Pass 239.0`: an accurate disclosure falsified three times by improvements to the very thing it describes, in operator-facing output. ★ AN OVERPRINTING IMAGE IN A PROCESS SPACE (`DeviceGray`, `DeviceRGB`, `DeviceCMYK`) now leaves every spot plane to the backdrop — Table 149's row 2 is TWO rows, not one: a process source takes `c_s` for the group's PROCESS components and `c_b` for its SPOT ones, in both overprint modes, and `Pass 238.0` gave the image path the second half. Measured on this project's own synthetic fixtures: a grey PATH and the same grey as an IMAGE, both overprinting a spot, now leave it standing identically. Read `overprint_images_unsupported` for what genuinely could not run"
            } else {
                "★ WHAT IS STILL APPROXIMATE, because a composited count is not a correct one: this page's blending colour space is ADDITIVE, so pdfcer composited in RGB with CMYK reconstructed per pixel. The four PROCESS colorants survive that reconstruction; a SPOT colorant does not — it is flattened through its tint transform and cannot be left standing the way a press leaves it"
            }
        );
    }
    if d.cmyk_buffer_engaged {
        eprintln!(
            "pdfcer: note: this page composited in a FOUR-COLORANT buffer rather than on screen. ★ WHERE THAT DECISION CAME FROM: {}. A page group that declares /CS /DeviceCMYK is the file saying so; `output_intent` means the page group declared NOTHING and pdfcer took the space from the document's output intent instead -- which ISO 32000-2's Annex P permits informatively and WITHOUT ranking it against the device's own space, so it is a choice the `page_blend_space_source` setting controls, not a fact about the file. Set that to `device_native` for ISO 32000-1's literal answer — ISO 32000-1 §11.7.2 and §11.6.6 both make that a `shall`, and §11.4.7 requires the result be converted to the display's space BEFORE the white paper is composited in, which is the order pdfcer uses. Blend modes ran through §11.3.4's subtractive complement and §11.3.5.3's K selection. ★ WHAT DID NOT: {} pixel(s) entered the buffer as CONVERTED sRGB rather than as authored ink. ★★ THAT POPULATION HAS SHRUNK THREE TIMES AND THIS SENTENCE USED TO NAME THE WRONG ONE: it said \"images and shadings resolve their colour to sRGB before any canvas sees them, so bridging them is the only information that reaches the compositor\", which was true of every image until `Pass 130.1`, of every shading until `Pass 137.0`, of every mesh until `Pass 137.1`, and of a `Separation`/`DeviceN` image outside overprint until `Pass 140.0`. What still bridges is content with NO AUTHORED INK TO KEEP — an image or mesh in an additive space, a `Separation`/`DeviceN` over a non-`DeviceCMYK` alternate, a parametric shading whose ramp yields no colorants. A FALL in this number is the intended outcome: it measures ink identity LOST on the way to the compositor, so less lost means less reported. And {} transparency group(s) could not be composited natively in ink — a KNOCKOUT group keeps its §11.4.6 semantics but runs its interior on screen, and a NON-ISOLATED group whose second content walk could not be given a buffer falls back to isolated semantics, dropping its backdrop. ★ THAT FALLBACK IS NOW THE ONLY WAY A NON-ISOLATED GROUP REACHES THIS COUNT: since `Pass 97.1g` an ordinary non-isolated group is rendered twice — once over nothing for its own alpha, once over its real backdrop for its colour — and §11.4.4's removal is applied, so it is composited natively and is not counted here. An ordinary isolated group is not among them either: it gets its own colorant buffer and crosses no conversion at all. ★ AND A CONSEQUENCE FOR HOW YOU CHECK THIS PAGE: a renderer that composites in RGB is not a reference for one that composites in ink. The two are REQUIRED to differ, by up to ~100 of 255 levels on saturated overlaps, and where they disagree the RGB answer is the wrong one",
            d.blend_space_from, d.cmyk_bridged_pixels, d.cmyk_groups_approximated
        );
    }
    if d.cmyk_buffer_refused > 0 {
        eprintln!(
            "pdfcer: note: this page's PAGE GROUP declares a SUBTRACTIVE blending colour space, and pdfcer composited it on screen ANYWAY — the four-colorant buffer would have exceeded its allocation ceiling at this raster size. The page rendered; it rendered the way every pdfcer release before this one rendered it, with §11.3.4's complement not applied. ★ TWO WAYS OUT, and the second is new: RE-RENDER AT A LOWER RESOLUTION, or RAISE THE CEILING — it is {} here, which permits {} pixel(s) and this raster wanted {}. Set `max_cmyk_buffer_bytes` in settings.txt or pass --max-cmyk-buffer-bytes; there is no upper limit, it costs 20 bytes per pixel and roughly 50% more time",
            pdfcer_core::settings::format_byte_size(max_cmyk_buffer_bytes),
            pdfcer_render::max_cmyk_composite_pixels(max_cmyk_buffer_bytes),
            raster_pixels
        );
    }
    if d.cmyk_unbridged_images > 0 {
        eprintln!(
            "pdfcer: note: {} IMAGE BRUSH(es) reached a subtractive paint with no conversion path and were NOT PAINTED. This is not supposed to be reachable — the page is missing marks, and the render should be reported rather than trusted",
            d.cmyk_unbridged_images
        );
    }
    if d.transparency_groups_flattened > 0 {
        eprintln!(
            "pdfcer: note: {} TRANSPARENCY GROUP(s) (/Group /S /Transparency, ISO 32000-1 \
§11.4.7) were FLATTENED — their contents were painted straight onto the page instead of being \
composited into a buffer and applied as a unit. Blend mode, constant alpha and any soft mask \
therefore applied to each object INSIDE the group rather than to the group's result. That is \
exact for a group holding one opaque object and approximate for everything else, so a page whose \
blending looks wrong while every blend-mode counter looks right is explained by this number",
            d.transparency_groups_flattened
        );
    }
    if d.transparency_groups_special > 0 {
        eprintln!(
            "pdfcer: note: {} transparency group(s) on this page are ISOLATED (/I true) or \
KNOCKOUT (/K true) — Table 147, NOT Table 96, which is the table of entries COMMON to all group \
dictionaries. Both are rendered per ISO 32000-1 11.4.5 and 11.4.6: an isolated group blends \
against a TRANSPARENT initial backdrop rather than the page, and a knockout group composites each \
element against the group's INITIAL backdrop rather than the accumulated result. This is a census, \
not a shortfall — it is here because these two are where FLATTENING (the number above, if it is \
non-zero) stops being a usable approximation, and because knockout is honoured only for an \
explicit /K true: implicit knockout from 9.3.8's /TK default, from 11.7.4.4's B/b, and from \
11.6.7's shading patterns is not",
            d.transparency_groups_special
        );
    }
    if d.soft_masks_ignored > 0 {
        eprintln!(
            "pdfcer: note: {} graphics-state(s) selected a SOFT MASK (/SMask, §11.6.5) that \
pdfcer does not implement; those marks were painted UNMASKED. This paints MORE than the document \
asked for — content the author faded out or masked away appears at full strength, so on a page \
whose design relies on a mask this is the difference between an artefact and showing what was \
meant to be hidden",
            d.soft_masks_ignored
        );
    }
    if (d.unknown_ops > 0 || d.deferred_ops > 0) && !d.sample_ops.is_empty() {
        eprintln!(
            "pdfcer: note: {} unknown and {} deferred content operator(s); \
first distinct names: {}",
            d.unknown_ops,
            d.deferred_ops,
            d.sample_ops.join(", ")
        );
    }
    if d.contents_streams_unresolved > 0 {
        eprintln!(
            "pdfcer: note: {} /Contents entr(y/ies) on this page name an object that is NOT \
in the file; that content is MISSING from the raster (ISO 32000-1 \u{a7}7.3.10: a reference to an \
absent object is the null object; Table 30: absent /Contents = an empty page)",
            d.contents_streams_unresolved
        );
    }
    if d.images_unsupported > 0 {
        eprintln!(
            "pdfcer: note: {} image(s) could not be decoded and are MISSING from the raster \
(nothing was substituted for them)",
            d.images_unsupported
        );
    }
    if d.images_codec_unsupported > 0 {
        eprintln!(
            "pdfcer: note: {} of those need an image codec this build does not implement",
            d.images_codec_unsupported
        );
    }
    // The per-name breakdown is the actionable half of R27: "pdfcer has
    // no JPEG decoder" and "pdfcer has a JPEG decoder but not the
    // arithmetic-coded variant" are different problems with different
    // answers, and only the name distinguishes them.
    if !d.codec_feature_unsupported.is_empty() {
        let named: Vec<String> = d
            .codec_feature_unsupported
            .iter()
            .map(|(feature, count)| format!("{feature} x{count}"))
            .collect();
        eprintln!(
            "pdfcer: note: unsupported codec feature(s): {}",
            named.join(", ")
        );
    }
    if d.codec_geometry_mismatch > 0 {
        eprintln!(
            "pdfcer: note: {} image(s) whose codestream geometry disagrees with the image \
dictionary; the dictionary was used for placement and the codestream for sample layout",
            d.codec_geometry_mismatch
        );
    }
    // `dct_cmyk_images` (the benign YCCK census) deliberately prints
    // NOTHING here: decision 006 verified those images decode without
    // polarity ambiguity, and the pre-006 "check the colours" note
    // cried wolf on known-good files. The count stays on the stdout
    // metrics line; stderr stays a shortfall-only channel. Only the
    // R30 shape below warrants a warning.
    if d.dct_cmyk_polarity_unverifiable > 0 {
        eprintln!(
            "pdfcer: note: {} four-component CMYK JPEG(s) with ColorTransform 0 and no \
/Decode: the polarity of this shape is UNVERIFIABLE — if the producer used the undocumented \
inverted-CMYK convention the image renders as its own negative. pdfcer draws the raw samples, \
as every reference engine does, and reports rather than guesses (docs/decisions/006, R30). \
Please keep the file and report it",
            d.dct_cmyk_polarity_unverifiable
        );
    }
    // /SMaskInData 2 alters the COLOUR SAMPLES themselves (a backdrop
    // is mixed into them), which is why it warrants a note where the
    // benign YCCK census does not: the picture on the page is not the
    // picture the document describes, merely the closest one pdfcer can
    // draw without clause 11 Matte machinery.
    if d.jpx_smask_in_data_preblended > 0 {
        eprintln!(
            "pdfcer: note: {} JPEG2000 image(s) with /SMaskInData 2: their colour channels \
are preblended with a backdrop and the opacity channel needs a /Matte entry to undo. Drawn from \
the preblended channels as stored - correct wherever the image is opaque, showing the backdrop \
where it is not. Un-premultiplication arrives with the transparency model",
            d.jpx_smask_in_data_preblended
        );
    }
    if d.lzw_framing_anomalies > 0 {
        eprintln!(
            "pdfcer: note: {} LZW stream(s) missing a ClearCode or EndOfInformation; \
recovered, but the producer is non-conformant",
            d.lzw_framing_anomalies
        );
    }
    // `images_masked` deliberately prints NOTHING here — it is
    // verified-correct volume, and decision 006 §4.4 records what a note
    // on known-good files does to an operator's trust in this channel.
    // The per-mechanism breakdown is offered only as context beside a
    // shortfall, never on its own.
    if d.images_mask_unsupported > 0 {
        let named: Vec<String> = d
            .mask_refused
            .iter()
            .map(|(reason, count)| format!("{reason} x{count}"))
            .collect();
        eprintln!(
            "pdfcer: note: {} image(s) carry an /SMask or /Mask that could not be applied \
({}); they are drawn FULLY OPAQUE, so the page shows content the document intended to be \
hidden or see-through",
            d.images_mask_unsupported,
            named.join(", ")
        );
    }
    if d.mattes_not_undone > 0 {
        eprintln!(
            "pdfcer: note: {} soft mask(s) carry /Matte (preblended colour) whose inversion \
was not applied; the alpha IS applied, but colours in the partially-transparent regions stay \
shifted toward the matte colour. The reason is in the image divergences below",
            d.mattes_not_undone
        );
    }
    if !d.image_notes.is_empty() {
        eprintln!(
            "pdfcer: note: image divergences: {}",
            d.image_notes.join("; ")
        );
    }
    if d.xobject_depth_overflows > 0 {
        eprintln!(
            "pdfcer: note: {} form XObject invocation(s) refused as too deeply nested or \
cyclic; their content is missing from the raster",
            d.xobject_depth_overflows
        );
    }
    if d.tolerated > 0 {
        eprintln!(
            "pdfcer: note: {} structural oddity(ies) tolerated while interpreting the page",
            d.tolerated
        );
    }
    // --- Pass 6.0 annotation honesty (R43/R50/R27) -------------------
    if !d.annotations_without_ap.is_empty() {
        // `Pass 289.0` REWROTE THIS NOTE, and the old one is worth naming:
        // it said these were "NOT painted (pdfcer never synthesises a look)",
        // which became FALSE the moment the icon class started drawing. A
        // note that contradicts the line printed under it is worse than no
        // note — the operator has to decide which of pdfcer's own statements
        // to believe.
        //
        // The two numbers now answer two different questions and neither
        // lies: how many annotations the FILE left without an appearance, and
        // how many of those the operator nevertheless SAW.
        let named: Vec<String> = d
            .annotations_without_ap
            .iter()
            .map(|(subtype, count)| format!("{subtype} x{count}"))
            .collect();
        let total: usize = d.annotations_without_ap.values().sum();
        let drawn = d.annotations_icon_painted;
        eprintln!(
            "pdfcer: note: {total} annotation(s) carry no appearance stream: {}. {}",
            named.join(", "),
            if drawn == 0 {
                "None was painted -- pdfcer draws a named standard icon (12.5.6.4/12.5.6.12) and \
                 synthesises nothing else, so a /Square or /Line with no /AP is disclosed here \
                 and left blank"
                    .to_owned()
            } else if drawn == total {
                format!(
                    "All {drawn} named a standard icon and were drawn from pdfcer's OWN artwork \
                     -- the file supplied the NAME, not the picture (12.5.6.4/12.5.6.12 put that \
                     duty on the reader)"
                )
            } else {
                format!(
                    "{drawn} named a standard icon and were drawn from pdfcer's OWN artwork; the \
                     remaining {} would need geometry pdfcer will not invent and are left blank",
                    total - drawn
                )
            }
        );
    }
    if d.annotations_appearance_state_missing > 0 {
        eprintln!(
            "pdfcer: note: {} annotation(s) have an appearance-state (/AS) that could not be \
resolved (missing, or naming an absent state); displayed as nothing, never guessed",
            d.annotations_appearance_state_missing
        );
    }
    if d.annotations_hidden > 0 {
        // R50: a hidden annotation is content the operator cannot see —
        // disclosed, because it is a document-forensics-relevant fact.
        eprintln!(
            "pdfcer: note: {} annotation(s) are suppressed on screen by the Hidden or NoView \
flag; honoured (not painted) AND disclosed",
            d.annotations_hidden
        );
    }
    if d.annotations_placement_degenerate > 0 {
        eprintln!(
            "pdfcer: note: {} annotation(s) carry an appearance that could not be placed \
(missing /Rect or /BBox, or a degenerate transformed box); refused by name, never mis-placed",
            d.annotations_placement_degenerate
        );
    }
    if !d.annotation_notes.is_empty() {
        eprintln!(
            "pdfcer: note: annotation placement notes: {}",
            d.annotation_notes.join("; ")
        );
    }
    report_color_diagnostics(&d.color);
    report_shading_diagnostics(&d.shading);
}

/// The §8.7.4 shading half of the honesty report.
///
/// # The sentence this function exists to make possible
///
/// *"This page has three gradients on it and pdfcer drew none of them."*
///
/// Before the shading model slice, a page of gradients produced
/// `deferred=52, first names BDC, sh, BMC` and nothing else — a count of
/// anonymous operators, from which neither an operator nor a future
/// session could tell how much of the page was missing, or why, or
/// whether an update would fix it.
///
/// The per-type breakdown goes here rather than on the stdout line for the
/// same reason every other breakdown in this file does: a new key on the
/// machine line can break a parser, and a new stderr sentence cannot.
///
/// Silent when a page has no shadings at all, so "stderr had output" stays
/// a usable batch signal.
pub(crate) fn report_shading_diagnostics(s: &pdfcer_render::ShadingDiagnostics) {
    if s.encountered == 0 {
        return;
    }
    // The census first: what is on the page, by type. `by_type` is indexed
    // 1..=7 at positions 0..=6, and only non-zero entries are named, so
    // the sentence stays short on a page with one gradient.
    let named: Vec<String> = s
        .by_type
        .iter()
        .enumerate()
        .filter(|(_, n)| **n > 0)
        .map(|(i, n)| format!("type{}={n}", i + 1))
        .collect();
    // The headline has to stay TRUE as painting lands type by type, which
    // is why it states the two numbers and lets the reader subtract rather
    // than asserting a capability. Its first version read "pdfcer resolves
    // gradients but does not yet draw them" — accurate for exactly one
    // commit, and false the moment the axial and radial painters shipped.
    // A sentence that encodes the current state of the roadmap goes stale
    // silently; a sentence that reports two counters cannot.
    let unpainted = s.encountered.saturating_sub(s.painted);
    eprintln!(
        "pdfcer: note: {} shading(s) found ({}), {} of them via the `sh` operator; \
{} painted, {} NOT — an unpainted shading leaves whatever was underneath it showing through",
        s.encountered,
        if named.is_empty() {
            "none classified".to_owned()
        } else {
            named.join(", ")
        },
        s.via_sh,
        s.painted,
        unpainted
    );
    // The two kinds pdfcer does not paint are named SEPARATELY, because
    // they are different amounts of remaining work: type 1 is one function
    // and an inverse matrix, the meshes are a bit-packed stream format.
    // An operator waiting for one should not be quoted the other's
    // timeline.
    if s.by_type[0] > 0 {
        eprintln!(
            "pdfcer: note: {} of those are FUNCTION-BASED shadings (type 1, ISO 32000-1 \
8.7.4.5.2), which this build resolves but does not paint. Unlike the axial and radial types it \
has no /Extend at all — outside its transformed domain rectangle the standard paints the \
background colour, or nothing",
            s.by_type[0]
        );
    }
    if s.mesh() > 0 {
        eprintln!(
            // 8.7.4.5.5-.8, NOT 9.x -- the first version of this string said
            // 9 and printed a clause that does not exist to the operator.
            // Same class as the stale DeviceN sentence fixed earlier today:
            // a wrong fact in a user-visible string, invisible to every
            // gate, caught only by reading the actual output.
            //
            // ** AND THE REST OF IT WENT STALE THE SAME WAY. ** Until
            // `Pass 125.0` this sentence ended "they are a materially larger
            // piece of work than the axial and radial types. Counted
            // separately so that waiting for one is not mistaken for waiting
            // for the other" -- i.e. it told the operator to wait for
            // something that had arrived. `R212`: the counter and the
            // sentence beside it are two claims, and only one of them is
            // under test.
            "pdfcer: note: {} of those are MESH shadings (types 4-7, ISO 32000-1 \
8.7.4.5.5-.8) -- their geometry is a bit-packed stream of triangles or Bezier patches rather \
than a formula, and pdfcer decodes and paints them. Two things about them are pdfcer's choice \
rather than the standard's, because the standard declines to say: a patch surface is \
approximated by flat cells whose density is chosen from its size ON SCREEN, so it is finer \
when you zoom in; and interpolation across a triangle is linear, which is what Gouraud \
names. A mesh also still resolves its colour to screen RGB before compositing, so on an ink \
page it is bridged like any other shading and its overprint is not represented",
            s.mesh()
        );
    }
    if s.refused > 0 {
        eprintln!(
            "pdfcer: note: {} shading(s) were REFUSED outright — the dictionary itself could \
not be used (no /ShadingType, unusable /Coords, a /ColorSpace that would not resolve, or a \
Pattern colour space, which 8.7.4.4 forbids). These will NOT be fixed by the geometry work; \
the file is malformed",
            s.refused
        );
    }
    if s.missing_function > 0 {
        eprintln!(
            "pdfcer: note: {} shading(s) of an analytic type carry no usable /Function, so \
they have no colour at any coordinate. Nothing can be drawn for them even once the geometry \
lands",
            s.missing_function
        );
    }
    if s.function_arity_mismatch > 0 {
        eprintln!(
            "pdfcer: note: {} shading /Function(s) produce a different number of components \
than their colour space takes (8.7.4.4); the colours such a shading would paint are not the \
ones the document specifies",
            s.function_arity_mismatch
        );
    }
    if s.mesh_truncated > 0 {
        eprintln!(
            "pdfcer: note: {} mesh shading stream(s) ended part-way through a record. pdfcer \
painted the records that were complete and discarded the remainder. ISO 32000-1 states an \
error condition for exactly ONE of the four mesh types (type 4, 8.7.4.5.5) and says nothing \
about the other three, so keeping the complete part is pdfcer's decision and this is it being \
disclosed rather than a verdict on the file. A type 5 stream that does not hold a whole \
number of rows is counted here too",
            s.mesh_truncated
        );
    }
    if s.mesh_unusable > 0 {
        eprintln!(
            "pdfcer: note: {} mesh shading(s) could not be decoded at all and painted \
nothing -- whatever was underneath them shows through. The reason for each is named in the \
divergence list below. This is NOT the same as a refused shading: the dictionary was fine \
and the stream was not",
            s.mesh_unusable
        );
    }
    if s.ramps_incomplete > 0 {
        eprintln!(
            "pdfcer: note: {} colour ramp(s) have gaps — the /Function failed at one or more \
sample points, and those bands of the gradient have no colour rather than an invented one",
            s.ramps_incomplete
        );
    }
    if s.ramps_managed > 0 {
        eprintln!(
            "pdfcer: note: {} shading(s) were colour-managed — their colour space carried an \
embedded ICC profile, or was a Lab/CalRGB/CalGray space on a document with an output intent, and \
every ramp sample or mesh corner went through it rather than through the device-space fallback. \
This is the same route a fill in that space takes, so a gradient and a flat fill of one colour \
land on one value; it is disclosed because the conversion leaves nothing on the page to point at",
            s.ramps_managed
        );
    }
    if !s.notes.is_empty() {
        eprintln!("pdfcer: note: shading divergences: {}", s.notes.join("; "));
    }
}

/// The §8.6 colour half of the honesty report.
///
/// # Why this is a separate function
///
/// [`pdfcer_render::ColorDiagnostics`] is a self-contained struct that
/// `pdfcer-render` merges across nested form XObjects as a unit, and its
/// counters answer a different family of operator question from the rest
/// of [`pdfcer_render::Diagnostics`] — "why is this the wrong colour?"
/// rather than "why is this missing?". Keeping the reporting in one place
/// mirrors that, and makes it visible at a glance whether a counter has a
/// shell caller. It did not, for every one of them, until this function
/// existed: the counters were computed and unit-tested inside the engine
/// and never crossed the crate boundary, which is disclosure to nobody.
///
/// # What prints, and what deliberately does not
///
/// Only the counters that describe a **shortfall or a choice** get a
/// sentence. The pure-census ones (`tint_transforms_applied`,
/// `pattern_spaces_selected`, `indexed_index_clamped`) are on the stdout
/// line where a script can read them, and print nothing here — decision
/// 006 §4.4 records what a note on a known-good file does to an
/// operator's trust in this channel, and "silence means faithful" is only
/// a usable signal if it is kept true.
///
/// `separation_none_suppressed` is the exception among the census
/// counters and DOES print, for R183's reason: content missing because
/// the standard says to omit it is otherwise indistinguishable from
/// content missing because pdfcer failed.
pub(crate) fn report_color_diagnostics(c: &pdfcer_render::ColorDiagnostics) {
    if c.spaces_unresolved > 0 {
        eprintln!(
            "pdfcer: note: {} colour space(s) named by `cs`/`CS` could not be resolved \
(absent from the page's /ColorSpace resources, malformed, or an unknown family); the space is \
left UNSET rather than defaulted to DeviceGray, so subsequent colour operators changed nothing",
            c.spaces_unresolved
        );
    }
    if c.colors_not_set > 0 {
        eprintln!(
            "pdfcer: note: {} colour-setting operator(s) could not be honoured (unresolved \
space, or an operand count that did not match the space); the PREVIOUS colour stayed in force, \
so those marks are painted in a stale colour",
            c.colors_not_set
        );
    }
    if c.patterns_unpainted > 0 {
        eprintln!(
            "pdfcer: note: {} fill/stroke(s) named a PATTERN and NOTHING was painted for \
them (ISO 32000-1 §8.7). SHADING patterns (PatternType 2) ARE painted now — see \
shadings_painted — so this number is the remainder: TILING patterns, a name with no matching \
/Pattern resource, a degenerate pattern matrix, or a shading pdfcer models but cannot draw (a \
mesh). An invented solid colour would be worse than a gap, so a page that looks blank where a \
hatch belongs is explained by this number",
            c.patterns_unpainted
        );
    }
    if c.tint_transform_not_applied > 0 {
        eprintln!(
            "pdfcer: note: {} spot-colour conversion(s) (/Separation or /DeviceN) had no \
usable /tintTransform in the DOCUMENT, so the tint was rendered as the alternate space's \
neutral: the lightness is right and the HUE IS NOT the document's",
            c.tint_transform_not_applied
        );
    }
    if c.separation_all_approximated > 0 {
        eprintln!(
            "pdfcer: note: {} /Separation /All conversion(s) rendered as a neutral of \
luminance 1-tint. §8.6.6.4 describes an INK behaviour (\"all available colorants at once\"), not \
a screen appearance, so this is pdfcer's choice and is disclosed as one",
            c.separation_all_approximated
        );
    }
    if c.separation_none_suppressed > 0 {
        eprintln!(
            "pdfcer: note: {} paint operation(s) suppressed because the colour space was \
/Separation /None or an all-/None /DeviceN. This is pdfcer OBEYING §8.6.6.4/.5, not failing — \
disclosed because a page missing content for a conformant reason otherwise looks identical to \
one that broke",
            c.separation_none_suppressed
        );
    }
    if c.icc_alternate_used > 0 || c.icc_device_fallback_used > 0 {
        eprintln!(
            "pdfcer: note: {} ICCBased space(s) resolved to their /Alternate and {} to the \
device space implied by /N — the spec's own fallback structure (§8.6.5.5 Table 66), not an \
approximation pdfcer invented. That is the RESOLUTION, not the paint: whether each paint was then \
converted through its embedded profile is icc_managed_paints / icc_unmanaged_paints on the \
metrics line. A three-component space is managed to the screen whenever its profile models, and \
any space is managed to ink when the document names an /OutputIntent; a one- or four-component \
space on a page without one is painted through the fallback, so an operator matching a brand \
colour should read the two counters rather than this line",
            c.icc_alternate_used, c.icc_device_fallback_used
        );
    }
    if c.indexed_lookup_short > 0 {
        eprintln!(
            "pdfcer: note: {} /Indexed lookup(s) fell past the end of a SHORT lookup table \
and were painted BLACK. Producers routinely trim trailing unused entries, so this is tolerated \
rather than fatal — but a wrongly-black palette entry now has a named cause",
            c.indexed_lookup_short
        );
    }
    if !c.notes.is_empty() {
        eprintln!("pdfcer: note: colour divergences: {}", c.notes.join("; "));
    }
}

/// Everything `export-dxf` was asked for.
///
/// A struct rather than nine positional parameters: the flag set grew past
/// the point where a call site reads as documentation, and
/// `clippy::too_many_arguments` is the lint that says so. Field names at
/// the call site also make the two mutually-exclusive destination flags
/// legible — `output: None, output_dir: Some(..)` states the mode, where a
/// pair of bare `Option`s in argument position would not.
pub(crate) struct ExportDxfArgs<'a> {
    pub(crate) input: &'a Path,
    /// 1-based, single-page mode. Ignored when `pages` is set (clap makes
    /// them mutually exclusive).
    pub(crate) page: u32,
    /// A `parse_pages` spec — multi-page mode.
    pub(crate) pages: Option<&'a str>,
    /// Single-page destination file.
    pub(crate) output: Option<&'a Path>,
    /// Multi-page destination directory.
    pub(crate) output_dir: Option<&'a Path>,
    pub(crate) units: DxfUnitArg,
    /// Explicit override; `None` means "derive it from the ce dimensions".
    pub(crate) scale: Option<f64>,
    pub(crate) fit_arcs: bool,
    /// Whether page text becomes `TEXT` entities.
    pub(crate) text: bool,
}

/// `export-dxf` — a page's vector geometry as CAD-importable DXF.
///
/// ## Contract
///
/// - One `export-dxf …` line carrying `entities=`, the per-kind counts, and
///   what was skipped, then `0` on success.
/// - **What did NOT make it into the file goes to stderr in prose.** A
///   drawing that is half annotation exports as geometry alone, and an
///   operator who is not told opens it in SOLIDWORKS and concludes the
///   export lost things at random. That sentence is needed BEFORE the file
///   is opened, not after.
/// - **Read-only on the input.** An `EditSession` IS constructed — it is
///   the only route to the `/PieceInfo` dimension sidecar, which is where
///   the drawing's calibration lives — but nothing is mutated and no save
///   path is reachable from here. The session is a reader in this function
///   and the absence of any `save` call is what makes that true.
///
/// ## Scale: derived when not given, and REFUSED when ambiguous
///
/// `--scale` is optional. Omitted, the page's ce dimensions are consulted
/// (`suggest_scale`) and the three outcomes are handled differently on
/// purpose:
///
/// - **Calibrated** — the derived figure is printed BEFORE the file is
///   written, naming the group it came from. That is rule 4: an inference
///   is disclosed, not applied silently.
/// - **Uncalibrated** — falls back to paper scale with the loud warning
///   this command already carried. pdfcer genuinely does not know, and
///   saying so is the honest answer.
/// - **Conflicting** — the export is REFUSED and every candidate listed.
///   A sheet with a 1:1 plan and a 1:5 detail is an ordinary drawing and
///   DXF carries one scale; choosing either would export half the sheet
///   wrong by a factor of five, and it would look entirely plausible.
///   `--scale` resolves it, which is what the refusal says.
///
/// The inference is scoped to **the pages being exported**, not to the
/// document. It shipped document-wide, which was wrong in both directions
/// on a multi-page sheet set: an unambiguous page-1 export could be
/// refused because page 3 held a 1:5 detail, and — the half that actually
/// damages metal — a page 1 with no calibration of its own would be
/// exported at page 3's scale with nothing on screen or in the output
/// looking odd. `dimension_groups_on_page` resolves each ce dimension's
/// owning page through its annotation's `/P`, and only those groups get a
/// vote.
///
/// ## Two modes, and why multi-page shares ONE scale
///
/// - `--page N -o file.dxf` — one page, one file.
/// - `--pages <spec> --output-dir <dir>` — one DXF per page, named
///   `<stem>_p<n>.dxf` zero-padded to the widest page number in the run.
///   Identical naming to the GUI's multi-page export, so a batch script
///   and an operator produce interchangeable output (project rule 11).
///
/// The scale is inferred from the union of every selected page's ce
/// dimension groups and applied to all of them, because `--scale` is one
/// value and a run that silently used a different scale per file would be
/// the plausible-wrong-answer failure this whole feature exists to close.
/// The consequence is deliberate: **pages at different scales are a
/// refusal**, reported exactly like two disagreeing groups on one page,
/// with the same two remedies (separate runs, or an explicit `--scale`).
pub(crate) fn cmd_export_dxf(args: ExportDxfArgs<'_>) -> u8 {
    use pdfcer_core::export::dxf::{
        DxfOptions, DxfOutcome, DxfScaleSuggestion, DxfText, DxfUnits, suggest_scale_for_groups,
        write_dxf,
    };

    let ExportDxfArgs {
        input,
        page,
        pages: pages_spec,
        output,
        output_dir,
        units,
        scale,
        fit_arcs,
        text,
    } = args;

    if let Some(s) = scale
        && (!s.is_finite() || s <= 0.0)
    {
        eprintln!(
            "pdfcer: {}: --scale must be a positive number; {s} would collapse or mirror the drawing",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    // Clap enforces that `--output` and `--output-dir` are mutually
    // exclusive and that `--output-dir` requires `--pages`. What it cannot
    // express is that ONE of them must be present, because which one is
    // legal depends on the other flag — so that is checked here, with a
    // message naming the flag the operator actually wants rather than a
    // generic "required argument missing".
    if output.is_none() && output_dir.is_none() {
        eprintln!(
            "pdfcer: {}: nowhere to write — pass --output <file.dxf> for a single page, or --output-dir <dir> with --pages",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    if pages_spec.is_some() && output_dir.is_none() {
        eprintln!(
            "pdfcer: {}: --pages writes one DXF per page and needs --output-dir <dir>; --output names a single file",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }

    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    // Read-only: see the contract above. This exists solely to reach the
    // `/PieceInfo` sidecar the drawing's calibration lives in.
    let session = pdfcer_core::edit::EditSession::new(doc);
    let doc = &session;
    let page_list = match doc.pages() {
        Ok(p) => p,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };

    // ---- which pages ----
    //
    // `parse_pages` is the established spec parser and REFUSES an
    // out-of-range page rather than dropping it (see its own docs on why
    // silently handing back 30 pages of a requested 50 is how a mistake
    // ships to a thousand documents). The single-page path keeps its own
    // bounds message, which names the page the operator typed.
    let indices: Vec<usize> = match pages_spec {
        Some(spec) => match parse_pages(spec, page_list.len()) {
            Ok(v) => v,
            Err(message) => {
                eprintln!("pdfcer: {}: {message}", input.display());
                return exit::RUNTIME_ERROR;
            }
        },
        None => {
            let index = (page.max(1) - 1) as usize;
            if page_list.get(index).is_none() {
                eprintln!(
                    "pdfcer: {}: no page {page} — the document has {} page(s)",
                    input.display(),
                    page_list.len()
                );
                return exit::RUNTIME_ERROR;
            }
            vec![index]
        }
    };

    // ---- decompose every page BEFORE writing anything ----
    //
    // All-or-nothing on purpose. A run that wrote four files and then died
    // on page five would leave a directory an operator has to reconcile by
    // hand against a page list, and the usual cause of a decompose failure
    // (a malformed content stream) is a property of the document rather
    // than of the moment, so retrying gains nothing.
    let mut models = Vec::with_capacity(indices.len());
    for index in &indices {
        let Some(target) = page_list.get(*index) else {
            eprintln!(
                "pdfcer: {}: no page {} — the document has {} page(s)",
                input.display(),
                index + 1,
                page_list.len()
            );
            return exit::RUNTIME_ERROR;
        };
        match pdfcer_core::vector::decompose_page(
            &doc.view(),
            target,
            pdfcer_core::vector::Matrix::IDENTITY,
        ) {
            Ok(m) => models.push(m),
            Err(err) => {
                eprintln!(
                    "pdfcer: {}: page {}: {err} — nothing was written",
                    input.display(),
                    index + 1
                );
                return exit::RUNTIME_ERROR;
            }
        }
    }

    // ---- resolve the drawing scale (see this function's contract) ----
    //
    // Done AFTER decomposition so a page that cannot be read fails on that
    // rather than on a scale question the operator would then have answered
    // for nothing.
    //
    // Scoped to the SELECTED pages' groups, deduplicated: a group present
    // on two pages must not vote twice, or it would inflate `agreeing` and
    // read as corroboration of itself.
    let mut groups: Vec<pdfcer_core::dimension::GroupId> = Vec::new();
    for index in &indices {
        for id in doc.dimension_groups_on_page(*index) {
            if !groups.contains(&id) {
                groups.push(id);
            }
        }
    }
    let multi = indices.len() > 1;
    let scope = if multi { "these pages'" } else { "this page's" };
    let suggestion = suggest_scale_for_groups(&doc.dimension_model(), &groups);
    // Whether pdfcer chose this number or the operator did — read before
    // `scale` is shadowed. See the paper-scale disclosure at the end.
    let scale_was_derived = scale.is_none();
    let scale = match scale {
        Some(explicit) => explicit,
        None => match &suggestion {
            DxfScaleSuggestion::Calibrated {
                scale,
                group,
                agreeing,
                ..
            } => {
                // Rule 4: the inference is stated BEFORE the file is
                // written, naming its source, so the operator can see what
                // pdfcer concluded and re-run with --scale if it is wrong.
                eprintln!(
                    "pdfcer: {}: using scale {scale} derived from the ce dimension group {group:?}{} — the drawing is calibrated, so this export is at REAL size, not paper size. Pass --scale to override.",
                    input.display(),
                    if *agreeing > 1 {
                        format!(" (and {} other calibrated group(s) agree)", agreeing - 1)
                    } else {
                        String::new()
                    }
                );
                *scale
            }
            DxfScaleSuggestion::Uncalibrated => 1.0,
            DxfScaleSuggestion::Conflicting { candidates } => {
                eprintln!(
                    "pdfcer: {}: REFUSED — {scope} ce dimension groups disagree about the scale, and a DXF carries only one. Nothing was written.",
                    input.display()
                );
                for c in candidates {
                    eprintln!("    {:?} says scale {}", c.group, c.scale);
                }
                eprintln!(
                    "  A 1:1 plan and a 1:5 detail on one sheet is an ordinary drawing, so pdfcer will not pick for you: choosing wrong exports part of the sheet at the wrong size and the result looks entirely plausible. Pass --scale <n> to say which, or export the views separately."
                );
                return exit::RUNTIME_ERROR;
            }
        },
    };

    let opts = DxfOptions {
        units: match units {
            DxfUnitArg::In => DxfUnits::Inches,
            DxfUnitArg::Mm => DxfUnits::Millimetres,
        },
        scale,
        fit_arcs,
        text: if text {
            DxfText::Entities
        } else {
            DxfText::Omit
        },
        ..DxfOptions::default()
    };

    // Zero-padded to the LARGEST page number in the run, not to the
    // document's page count: exporting pages 8-10 of a 400-page file
    // should not produce `_p008`.
    let width = indices
        .iter()
        .map(|i| (i + 1).to_string().len())
        .max()
        .unwrap_or(1);
    let stem = input
        .file_stem()
        .map_or_else(|| "export".to_owned(), |s| s.to_string_lossy().into_owned());

    let mut total = DxfOutcome::default();
    for (index, model) in indices.iter().zip(&models) {
        let (dxf, out) = write_dxf(model, &opts);
        let path = match output_dir {
            Some(dir) => dir.join(format!("{stem}_p{:0width$}.dxf", index + 1)),
            // Unreachable: the guard above rejects both-absent, and clap
            // rejects both-present. Handled rather than unwrapped so a
            // future flag change cannot turn this into a panic.
            None => match output {
                Some(path) => path.to_path_buf(),
                None => return exit::RUNTIME_ERROR,
            },
        };
        if let Err(err) = std::fs::write(&path, dxf.as_bytes()) {
            eprintln!("pdfcer: {}: {err}", path.display());
            return exit::IO_ERROR;
        }
        let entities = out.polylines + out.circles + out.arcs + out.splines + out.text_entities;
        println!(
            "export-dxf {} page {} -> {}; entities={entities} polylines={} circles={} arcs={} splines={} text={} unreadable_text={} skipped_text={} skipped_images={} units={} scale={} fit_arcs={}",
            input.display(),
            index + 1,
            path.display(),
            out.polylines,
            out.circles,
            out.arcs,
            out.splines,
            out.text_entities,
            out.unreadable_text,
            out.skipped_text,
            out.skipped_images,
            match units {
                DxfUnitArg::In => "in",
                DxfUnitArg::Mm => "mm",
            },
            scale,
            u32::from(fit_arcs),
        );
        total.polylines += out.polylines;
        total.circles += out.circles;
        total.arcs += out.arcs;
        total.splines += out.splines;
        total.skipped_text += out.skipped_text;
        total.skipped_images += out.skipped_images;
        total.text_entities += out.text_entities;
        total.unreadable_text += out.unreadable_text;
    }

    // ---- the disclosures, in prose, on stderr ----
    //
    // SUMMED across the run and emitted ONCE. Per-file would be the
    // obvious choice and is the wrong one: a forty-page batch would print
    // forty near-identical paragraphs, and a disclosure repeated forty
    // times is one an operator scrolls past — which is the same
    // learned-past failure the paper-scale gating below was written to
    // avoid, arriving through volume instead of through wording. The
    // per-page machine-readable line above already carries each page's
    // own counts for anything that needs them.
    if total.skipped_text > 0 {
        eprintln!(
            "pdfcer: {}: {} text object(s) were NOT exported — this DXF carries geometry only, so any dimensions, labels and notes on the drawing are absent from it. Their outlines are not there either; the text was never converted to curves.",
            input.display(),
            total.skipped_text
        );
    }
    if total.unreadable_text > 0 {
        eprintln!(
            "pdfcer: {}: {} text run(s) could NOT be read and are absent from the DXF — pdfcer could not map their character codes to characters (a font with no /ToUnicode, typically). This is different from --no-text: these are labels you can see on the page that pdfcer cannot transcribe, so the DXF is missing text you will expect to find in it.",
            input.display(),
            total.unreadable_text
        );
    }
    if total.skipped_images > 0 {
        eprintln!(
            "pdfcer: {}: {} image(s) were NOT exported — DXF has no raster entity in the subset pdfcer writes, so a scanned or rendered region of the page is simply missing rather than blank.",
            input.display(),
            total.skipped_images
        );
    }
    // Gated on THREE things, and each rules out a different way of telling
    // the operator something they already know:
    //
    //   * the scale is 1 — there is nothing to warn about otherwise;
    //   * the pages are UNCALIBRATED — a group calibrated to an explicit
    //     1:1 is a real answer the operator gave, and warning them that
    //     pdfcer might not know the scale when it demonstrably does is the
    //     shape of disclosure that gets learned past and then ignored when
    //     it matters;
    //   * the 1 was DERIVED, not typed. This third clause was missing
    //     and it produced a genuinely absurd message: `--scale 1` on an
    //     uncalibrated drawing printed "pdfcer does not know what scale the
    //     drawing is at … pass --scale 2 for 1:2, and so on" — instructing
    //     the operator to do the thing they had just done. It is the same
    //     objection as the second clause arriving from the other side: an
    //     explicit `--scale 1` is the operator answering, exactly as an
    //     explicit 1:1 calibration is. Found by running the command rather
    //     than by reading it.
    if scale_was_derived
        && (scale - 1.0).abs() < f64::EPSILON
        && matches!(suggestion, DxfScaleSuggestion::Uncalibrated)
    {
        eprintln!(
            "pdfcer: {}: exported at PAPER scale. Nothing on {} is calibrated, so pdfcer does not know what scale the drawing is at — if it is a scaled view, a 1:2 detail say, the geometry is that fraction of real size and will look entirely plausible. Either measure a known feature in the GUI (the scale then comes across automatically) or pass --scale 2 for 1:2, and so on.",
            input.display(),
            if multi { "these pages" } else { "this page" }
        );
    }

    exit::SUCCESS
}
