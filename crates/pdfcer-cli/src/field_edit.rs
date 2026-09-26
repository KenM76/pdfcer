use super::*;

/// Borrowed argument bundle for [`cmd_add_text_field`] (clippy arg-count).
pub(crate) struct AddTextFieldArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) name: &'a str,
    pub(crate) page: usize,
    pub(crate) rect: &'a str,
    pub(crate) value: Option<&'a str>,
    pub(crate) max_len: Option<i64>,
    pub(crate) tooltip: Option<&'a str>,
    pub(crate) no_tooltip: bool,
    pub(crate) multiline: bool,
    pub(crate) read_only: bool,
    pub(crate) required: bool,
    pub(crate) password: bool,
    pub(crate) comb: bool,
    pub(crate) border: BorderArg,
    pub(crate) border_width: f64,
    pub(crate) background: Option<&'a str>,
    pub(crate) border_color: Option<&'a str>,
    pub(crate) visibility: VisibilityArg,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) defaults_from: Option<&'a str>,
    pub(crate) verify_undo: bool,
}

/// Borrowed argument bundle for [`cmd_add_check_box`] (clippy arg-count).
pub(crate) struct AddCheckBoxArgs<'a> {
    /// Which glyph the ON state draws — `CheckStyle::parse` names.
    pub(crate) check_style: Option<&'a str>,
    pub(crate) input: &'a Path,
    pub(crate) name: &'a str,
    pub(crate) page: usize,
    pub(crate) rect: &'a str,
    pub(crate) on_state: &'a str,
    pub(crate) checked: bool,
    pub(crate) tooltip: Option<&'a str>,
    pub(crate) no_tooltip: bool,
    pub(crate) read_only: bool,
    pub(crate) required: bool,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) defaults_from: Option<&'a str>,
    pub(crate) verify_undo: bool,
    pub(crate) border: BorderArg,
    pub(crate) border_width: f64,
    pub(crate) background: Option<&'a str>,
    pub(crate) border_color: Option<&'a str>,
    pub(crate) visibility: VisibilityArg,
}

/// Borrowed argument bundle for [`cmd_add_choice_field`] (clippy arg-count).
pub(crate) struct AddChoiceFieldArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) name: &'a str,
    pub(crate) page: usize,
    pub(crate) rect: &'a str,
    pub(crate) options: &'a [String],
    pub(crate) combo: bool,
    pub(crate) editable: bool,
    pub(crate) multi_select: bool,
    pub(crate) sort: bool,
    pub(crate) tooltip: Option<&'a str>,
    pub(crate) no_tooltip: bool,
    pub(crate) read_only: bool,
    pub(crate) required: bool,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) defaults_from: Option<&'a str>,
    pub(crate) verify_undo: bool,
    pub(crate) border: BorderArg,
    pub(crate) border_width: f64,
    pub(crate) background: Option<&'a str>,
    pub(crate) border_color: Option<&'a str>,
    pub(crate) visibility: VisibilityArg,
}

/// Borrowed argument bundle for [`cmd_add_push_button`] (clippy arg-count).
pub(crate) struct AddPushButtonArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) name: &'a str,
    pub(crate) page: usize,
    pub(crate) rect: &'a str,
    pub(crate) caption: &'a str,
    pub(crate) tooltip: Option<&'a str>,
    pub(crate) no_tooltip: bool,
    pub(crate) read_only: bool,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) defaults_from: Option<&'a str>,
    pub(crate) verify_undo: bool,
    pub(crate) border: BorderArg,
    pub(crate) border_width: f64,
    pub(crate) background: Option<&'a str>,
    pub(crate) border_color: Option<&'a str>,
    pub(crate) visibility: VisibilityArg,
}

/// Borrowed argument bundle for [`cmd_add_radio_button`] (clippy arg-count).
pub(crate) struct AddRadioButtonArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) name: &'a str,
    pub(crate) page: usize,
    pub(crate) rect: &'a str,
    pub(crate) export_value: &'a str,
    pub(crate) selected: bool,
    pub(crate) tooltip: Option<&'a str>,
    pub(crate) no_tooltip: bool,
    pub(crate) no_toggle_to_off: bool,
    pub(crate) radios_in_unison: bool,
    pub(crate) read_only: bool,
    pub(crate) required: bool,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) defaults_from: Option<&'a str>,
    pub(crate) verify_undo: bool,
    pub(crate) border: BorderArg,
    pub(crate) border_width: f64,
    pub(crate) background: Option<&'a str>,
    pub(crate) border_color: Option<&'a str>,
    pub(crate) visibility: VisibilityArg,
}

/// Parse `--page` (1-based) and `--rect` (`llx,lly,urx,ury`), the two
/// arguments every field-authoring subcommand takes in the same form.
///
/// Shared so the three subcommands cannot disagree about whether `--page` is
/// 1-based — which is the kind of divergence that produces a field on the
/// wrong page rather than an error.
pub(crate) fn parse_page_and_rect(
    input: &Path,
    page: usize,
    rect: &str,
) -> Result<(usize, pdfcer_core::page_tree::Rect), u8> {
    let Some(page_index) = page.checked_sub(1) else {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            input.display()
        );
        return Err(exit::EDIT_REFUSED);
    };
    let parts: Vec<f64> = rect
        .split(',')
        .filter_map(|t| t.trim().parse::<f64>().ok())
        .collect();
    let [llx, lly, urx, ury] = parts[..] else {
        eprintln!(
            "pdfcer: {}: --rect needs four numbers as LLX,LLY,URX,URY",
            input.display()
        );
        return Err(exit::EDIT_REFUSED);
    };
    Ok((
        page_index,
        pdfcer_core::page_tree::Rect { llx, lly, urx, ury },
    ))
}

/// `add-check-box` — author a new check box.
///
/// ## Contract
///
/// - Emits one `add-check-box …` line with the usual save-report fields,
///   then defers the exit code to [`finish_edit`].
/// - Every refusal — an `Off` on-state, XFA present, a name already used by
///   a different field type, a degenerate rectangle, an empty name, a page
///   out of range — goes through [`report_edit_error`] BEFORE any mutation.
/// - `--page` is 1-BASED here and 0-based in the core call.
pub(crate) fn cmd_add_check_box(args: &AddCheckBoxArgs<'_>) -> u8 {
    let (page_index, rect) = match parse_page_and_rect(args.input, args.page, args.rect) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let mut spec = pdfcer_core::edit::NewCheckBox::new(page_index, args.name, rect)
        .with_on_state(args.on_state)
        .checked(args.checked)
        .with_flags(args.read_only, args.required)
        .with_border(args.border.into(), args.border_width)
        .with_visibility(args.visibility.into());

    // `Pass 308.1`: the colours land on the SPEC, so the `/MK` dictionary and
    // the `/AP` artwork are written from one value. Parsed before anything is
    // staged, so a mistyped colour costs nothing.
    let chrome = match parse_creation_chrome(args.background, args.border_color) {
        Ok(c) => c,
        Err(code) => return code,
    };
    if let Some(c) = chrome.background {
        spec = spec.with_background(c);
    }
    if let Some(c) = chrome.border_color {
        spec = spec.with_border_color(c);
    }

    // The tick style. Refused by name on an unknown word rather than silently
    // defaulting to a check: an operator who typed `--check-style tik` wants
    // to be told, not to get a tick and believe it worked.
    if let Some(name) = args.check_style {
        let Some(style) = pdfcer_core::annot_author::CheckStyle::parse(name) else {
            eprintln!(
                "pdfcer: --check-style {name:?} -- known: check, cross, star, circle, square, diamond (Acrobat's six)"
            );
            return exit::RUNTIME_ERROR;
        };
        spec.style = style;
    }
    // R105: exactly one of the two must have been chosen. `clap`'s
    // `conflicts_with` rules out BOTH; only "neither" can reach here, and it
    // is refused rather than defaulted.
    spec = match (args.tooltip, args.no_tooltip) {
        (Some(t), _) => spec.with_tooltip(t),
        (None, true) => spec.declining_tooltip(),
        (None, false) => {
            eprintln!(
                "pdfcer: {}: decide about the accessibility name — pass --tooltip <text>, or --no-tooltip to decline it. It is what a screen reader announces for this field, so it is never defaulted silently.",
                args.input.display()
            );
            return exit::EDIT_REFUSED;
        }
    };

    // Applied to the SPEC before authoring, so everything downstream
    // — the merge check, the appearance build, the undo entry — sees
    // one fully-formed request rather than a partially-defaulted one.
    let defaults = match read_defaults(&session, args.input, args.defaults_from) {
        Ok(d) => d,
        Err(code) => return code,
    };
    let applied = defaults
        .map(|d| spec.apply_defaults(&d))
        .unwrap_or_default();
    let authored = match session.add_check_box(&spec) {
        Ok(o) => o,
        Err(err) => return report_edit_error(args.input, &err),
    };
    let field_id = authored.field_id;
    // Folded into the SAME disclosure struct the core produced, not
    // reported alongside it: one channel, so `any()` still answers for
    // everything and a caller gating on it cannot miss half the facts.
    let mut disclosures = authored.disclosures;
    disclosures.defaults_type_mismatch = applied.type_mismatch;
    disclosures.defaults_on_state_ambiguous = applied.on_state_ambiguous;
    report_field_disclosures(args.name, disclosures);
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
        "add-check-box {} name={:?} page={} rect={},{},{},{} on_state={:?} checked={} field={} {} merged={} tagged={} struct_tabs={} tooltip_declined={} background={} border_color={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.name,
        args.page,
        rect.llx,
        rect.lly,
        rect.urx,
        rect.ury,
        args.on_state,
        u32::from(args.checked),
        field_id.num,
        field_id.generation,
        u32::from(authored.merged),
        u32::from(authored.disclosures.tagged_document),
        u32::from(authored.disclosures.structure_tab_order),
        u32::from(authored.disclosures.tooltip_declined),
        mk_colour_token(spec.chrome.background),
        mk_colour_token(spec.chrome.border_color),
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
/// Everything `edit-field` takes. A struct because clippy's
/// `too_many_arguments` is right here: eighteen positional parameters, most
/// of them `Option<bool>`, is a call site where two can be swapped without
/// the compiler noticing.
/// Parse a standard-14 face name for `--font`.
///
/// The names are pdfcer's own spelling, hyphenated and lower-case, rather
/// than the PostScript `BaseFont` names (`Helvetica-BoldOblique`) -- a
/// command line is typed by a person, and the PostScript names are
/// case-sensitive in a way that produces a refusal for a shift key.
pub(crate) fn parse_std14(name: &str) -> Option<pdfcer_core::fontdata::Std14> {
    use pdfcer_core::fontdata::Std14 as F;
    Some(match name.to_ascii_lowercase().as_str() {
        "helvetica" | "helv" => F::Helvetica,
        "helvetica-bold" => F::HelveticaBold,
        "helvetica-oblique" | "helvetica-italic" => F::HelveticaOblique,
        "helvetica-bold-oblique" | "helvetica-bold-italic" => F::HelveticaBoldOblique,
        "times" | "times-roman" => F::TimesRoman,
        "times-bold" => F::TimesBold,
        "times-italic" | "times-oblique" => F::TimesItalic,
        "times-bold-italic" | "times-bold-oblique" => F::TimesBoldItalic,
        "courier" => F::Courier,
        "courier-bold" => F::CourierBold,
        "courier-oblique" | "courier-italic" => F::CourierOblique,
        "courier-bold-oblique" | "courier-bold-italic" => F::CourierBoldOblique,
        "symbol" => F::Symbol,
        "zapf-dingbats" | "zapfdingbats" | "dingbats" => F::ZapfDingbats,
        _ => return None,
    })
}

/// Parse a `--font-color` argument: 1, 3 or 4 comma-separated components.
///
/// Unlike `/MK` colours there is no `none` here -- a `/DA` always states a
/// colour, and Table 224 gives no spelling for "no colour" in a text-drawing
/// operator. Black is the default when the flag is omitted.
pub(crate) fn parse_text_colour(raw: &str) -> Option<pdfcer_core::vartext::TextColor> {
    use pdfcer_core::vartext::TextColor as C;
    let parts: Vec<f64> = raw
        .split(',')
        .map(|p| p.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    if !parts.iter().all(|v| v.is_finite()) {
        return None;
    }
    match parts.as_slice() {
        [g] => Some(C::Gray(*g)),
        [r, g, b] => Some(C::Rgb(*r, *g, *b)),
        [c, m, y, k] => Some(C::Cmyk(*c, *m, *y, *k)),
        _ => None,
    }
}

pub(crate) struct EditFieldArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) name: &'a str,
    pub(crate) required: Option<bool>,
    pub(crate) read_only: Option<bool>,
    pub(crate) tooltip: Option<&'a str>,
    pub(crate) multiline: Option<bool>,
    pub(crate) password: Option<bool>,
    pub(crate) comb: Option<bool>,
    pub(crate) max_len: Option<i64>,
    pub(crate) no_toggle_to_off: Option<bool>,
    pub(crate) radios_in_unison: Option<bool>,
    pub(crate) combo: Option<bool>,
    pub(crate) editable: Option<bool>,
    pub(crate) multi_select: Option<bool>,
    pub(crate) sort: Option<bool>,
    /// `/Q` justification, 0-2; validated in the engine.
    pub(crate) quadding: Option<i64>,
    /// Remove `/Q` entirely.
    pub(crate) clear_quadding: bool,
    /// `/DV`, the value a reset restores.
    pub(crate) default_value: Option<&'a str>,
    /// Remove `/DV`, so a reset clears the field.
    pub(crate) clear_default_value: bool,
    /// `Ff` bit 3, NoExport.
    pub(crate) no_export: Option<bool>,
    /// `Ff` bit 21, FileSelect.
    pub(crate) file_select: Option<bool>,
    /// `Ff` bit 23, DoNotSpellCheck.
    pub(crate) no_spell_check: Option<bool>,
    /// `Ff` bit 24, DoNotScroll.
    pub(crate) no_scroll: Option<bool>,
    /// `Ff` bit 27, CommitOnSelChange.
    pub(crate) commit_on_sel_change: Option<bool>,
    /// `/TM`, the export mapping name.
    pub(crate) mapping_name: Option<&'a str>,
    /// Remove `/TM`.
    pub(crate) clear_mapping_name: bool,
    /// A standard-14 face name for `/DA`.
    pub(crate) font: Option<&'a str>,
    /// A `/DR` `/Font` resource key for `/DA`.
    pub(crate) font_resource: Option<&'a str>,
    /// `/DA` size in points; 0 = auto.
    pub(crate) font_size: Option<f64>,
    /// `/DA` text colour, 1/3/4 components.
    pub(crate) font_color: Option<&'a str>,
    pub(crate) options: &'a [String],
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
}

/// `edit-field` — change an existing field's field-scope properties.
///
/// # What this command exists to carry, beyond doing the edit
///
/// Two disclosures, and the second is the reason the whole surface was
/// asked for:
///
/// 1. **How many widgets a field-scope change reached.** "I changed one
///    field" and "three things on screen look different" are the same event.
/// 2. **Whether the stored value still fits.** Shortening `/MaxLen` below
///    the current value, or removing a choice option that is selected, leaves
///    the file inconsistent — and Acrobat does both SILENTLY. pdfcer does not
///    truncate the operator's data and does not re-point their selection;
///    both would be inventing document state. It says so instead.
pub(crate) fn cmd_edit_field(args: &EditFieldArgs<'_>) -> u8 {
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let mut edit = pdfcer_core::edit::FieldEdit::new();
    if let Some(v) = args.required {
        edit = edit.with_required(v);
    }
    if let Some(v) = args.read_only {
        edit = edit.with_read_only(v);
    }
    if let Some(t) = args.tooltip {
        // An empty string is "remove it", which R105 models as an explicit
        // DECLINE rather than as an empty `/TU` — a screen reader announcing
        // an empty accessibility name is worse than one falling back to the
        // field's name.
        edit = edit.with_tooltip(if t.is_empty() {
            pdfcer_core::edit::TooltipChoice::Declined
        } else {
            pdfcer_core::edit::TooltipChoice::Text(t.to_owned())
        });
    }
    if let Some(v) = args.multiline {
        edit = edit.with_multiline(v);
    }
    if let Some(v) = args.password {
        edit = edit.with_password(v);
    }
    if let Some(v) = args.comb {
        edit = edit.with_comb(v);
    }
    if let Some(n) = args.max_len {
        // Zero REMOVES the limit rather than setting one of zero. A
        // /MaxLen of 0 is a field that accepts nothing, which is not a thing
        // anyone means to author, and the CLI has no other spelling for
        // "absent" on a numeric flag.
        edit = edit.with_max_len(if n <= 0 { None } else { Some(n) });
    }
    if let Some(v) = args.no_toggle_to_off {
        edit = edit.with_no_toggle_to_off(v);
    }
    if let Some(v) = args.radios_in_unison {
        edit = edit.with_radios_in_unison(v);
    }
    if let Some(v) = args.combo {
        edit = edit.with_combo(v);
    }
    if let Some(v) = args.editable {
        edit = edit.with_editable(v);
    }
    if let Some(v) = args.multi_select {
        edit = edit.with_multi_select(v);
    }
    if let Some(v) = args.sort {
        edit = edit.with_sort(v);
    }
    if let Some(q) = args.quadding {
        edit = edit.with_quadding(q);
    }
    if args.clear_quadding {
        edit = edit.clearing_quadding();
    }
    if let Some(v) = args.default_value {
        edit = edit.with_default_value(v);
    }
    if args.clear_default_value {
        edit = edit.clearing_default_value();
    }
    if let Some(v) = args.no_export {
        edit = edit.with_no_export(v);
    }
    if let Some(v) = args.file_select {
        edit = edit.with_file_select(v);
    }
    if let Some(v) = args.no_spell_check {
        edit = edit.with_no_spell_check(v);
    }
    if let Some(v) = args.no_scroll {
        edit = edit.with_no_scroll(v);
    }
    if let Some(v) = args.commit_on_sel_change {
        edit = edit.with_commit_on_sel_change(v);
    }
    if let Some(v) = args.mapping_name {
        edit = edit.with_mapping_name(v);
    }
    if args.clear_mapping_name {
        edit = edit.clearing_mapping_name();
    }

    // `/DA`. All three parts travel together because the string carries all
    // three -- writing a size without a face would mean inventing the face,
    // which is the substitution this project refuses elsewhere.
    if args.font.is_some() || args.font_resource.is_some() {
        let Some(size) = args.font_size else {
            eprintln!(
                "pdfcer: --font/--font-resource needs --font-size (0 means auto-size, which is what a new text field uses)"
            );
            return exit::RUNTIME_ERROR;
        };
        let colour = match args.font_color {
            None => pdfcer_core::vartext::TextColor::Gray(0.0),
            Some(raw) => match parse_text_colour(raw) {
                Some(c) => c,
                None => {
                    eprintln!(
                        "pdfcer: --font-color {raw:?} -- expected 1 (gray), 3 (RGB) or 4 (CMYK) comma-separated components in 0-1"
                    );
                    return exit::RUNTIME_ERROR;
                }
            },
        };
        let appearance = if let Some(name) = args.font {
            let Some(face) = parse_std14(name) else {
                eprintln!(
                    "pdfcer: --font {name:?} -- known: helvetica, helvetica-bold, helvetica-oblique, helvetica-bold-oblique, times, times-bold, times-italic, times-bold-italic, courier, courier-bold, courier-oblique, courier-bold-oblique, symbol, zapf-dingbats"
                );
                return exit::RUNTIME_ERROR;
            };
            pdfcer_core::edit::FieldAppearance::standard(face, size, colour)
        } else {
            // Unwrap-free: the branch is guarded by the `is_some()` above.
            let key = args.font_resource.unwrap_or_default();
            pdfcer_core::edit::FieldAppearance::resource(key.as_bytes().to_vec(), size, colour)
        };
        edit = edit.with_appearance(appearance);
    }
    if !args.options.is_empty() {
        let parsed: Vec<pdfcer_core::edit::ChoiceOption> = args
            .options
            .iter()
            .map(|raw| match raw.split_once('=') {
                Some((export, display)) => pdfcer_core::edit::ChoiceOption::new(export, display),
                None => pdfcer_core::edit::ChoiceOption::plain(raw.clone()),
            })
            .collect();
        edit = edit.with_options(parsed);
    }

    let outcome = match session.edit_field(args.name, &edit) {
        Ok(o) => o,
        Err(err) => return report_edit_error(args.input, &err),
    };

    if let Some(complaint) = &outcome.value_no_longer_fits {
        eprintln!("pdfcer: field {:?}: ★ {complaint}", args.name);
    }
    if outcome.sort_claim_unmet {
        eprintln!(
            "pdfcer: field {:?}: the Sort flag is now set and the option list is NOT in alphabetical order. ISO 32000-1 Table 230 makes that flag a record of what the WRITER did, and says conforming readers \"shall display the options in the order in which they occur\" — so pdfcer did not reorder anything, and the file now claims something that is not true. Pass the options in the order you want them.",
            args.name
        );
    }
    if outcome.widgets_affected > 1 {
        eprintln!(
            "pdfcer: field {:?}: this is ONE field with {} widgets, so the change applies to all {} of them — that is what a field-scope property is. Use `edit-widget` for the per-placement ones (position, border, visibility, caption).",
            args.name, outcome.widgets_affected, outcome.widgets_affected
        );
    }

    let saved = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };
    println!(
        "edit-field {} name={:?} -> {} flags=0x{:X}->0x{:X} widgets={} regenerated={} value_fits={} changed_objects={}",
        args.input.display(),
        args.name,
        args.output.display(),
        outcome.flags_before,
        outcome.flags_after,
        outcome.widgets_affected,
        u32::from(outcome.appearance_regenerated),
        u32::from(outcome.value_no_longer_fits.is_none()),
        saved.changed,
    );
    finish_edit(args.input, &saved)
}

/// Parse an `edit-widget` `/MK` colour argument, which has one more state
/// than a creation verb's (`Pass 308.3`).
///
/// `unset` is [`pdfcer_core::edit::MkColorEdit::Remove`]; everything else goes
/// through [`parse_mk_colour`], so the two surfaces cannot drift on what a
/// colour looks like.
///
/// `none` and `unset` are one letter apart in meaning and worlds apart in
/// effect, which is why both are words rather than, say, an empty string for
/// one of them. `none` states *no colour* and leaves the key present; `unset`
/// takes the key away and hands the builder back its own default. A creation
/// verb has no `unset` because there is nothing yet to remove.
pub(crate) fn parse_mk_colour_edit(raw: &str) -> Option<pdfcer_core::edit::MkColorEdit> {
    use pdfcer_core::edit::MkColorEdit;
    if raw.eq_ignore_ascii_case("unset") {
        return Some(MkColorEdit::Remove);
    }
    parse_mk_colour(raw).map(MkColorEdit::Set)
}

/// One `/MK` colour as the CLI spells it on an output line.
///
/// `-` for a key the widget does not carry, `none` for Table 189's empty
/// array, else the components. The exact inverse of [`parse_mk_colour`]'s
/// accepted spellings, so a value printed by `list-fields` can be passed
/// straight back to `--background`.
///
/// A free function rather than a closure in `list-fields` because the five
/// field-creation verbs print the same two tokens: two spellings of one
/// colour is how `background=0.85` and `background=0.85,0.85,0.85` end up in
/// the same output stream.
pub(crate) fn mk_colour_token(c: Option<pdfcer_core::forms::MkColor>) -> String {
    use pdfcer_core::forms::MkColor;
    match c {
        None => "-".to_owned(),
        Some(MkColor::None) => "none".to_owned(),
        Some(MkColor::Gray(g)) => format!("{g}"),
        Some(MkColor::Rgb(r, g, b)) => format!("{r},{g},{b}"),
        Some(MkColor::Cmyk(c, m, y, k)) => format!("{c},{m},{y},{k}"),
    }
}

/// Parse `--background` and `--border-color` for a field-creation verb
/// (`Pass 308.1`).
///
/// One function for all five verbs, so a mistyped colour is refused in the
/// same words whichever field was being created. `None` for an argument the
/// operator did not pass, which leaves that half of the spec's creation floor
/// standing -- the floor is a real colour that gets both written and painted,
/// not an absence.
///
/// # Errors
///
/// The process exit code to return, after printing the refusal. Never
/// partially applied: both are parsed before either is used.
pub(crate) fn parse_creation_chrome(
    background: Option<&str>,
    border_color: Option<&str>,
) -> Result<CreationChrome, u8> {
    fn one(flag: &str, raw: Option<&str>) -> Result<Option<pdfcer_core::forms::MkColor>, u8> {
        match raw {
            None => Ok(None),
            Some(raw) => match parse_mk_colour(raw) {
                Some(c) => Ok(Some(c)),
                None => {
                    eprintln!(
                        "pdfcer: {flag} {raw:?} -- expected `none` (Table 189's empty array), or 1 (gray), 3 (RGB) or 4 (CMYK) comma-separated components in 0-1"
                    );
                    Err(exit::RUNTIME_ERROR)
                }
            },
        }
    }
    Ok(CreationChrome {
        background: one("--background", background)?,
        border_color: one("--border-color", border_color)?,
    })
}

/// The two `/MK` colours a field-creation verb was asked for, each `None`
/// when the operator did not name it.
pub(crate) struct CreationChrome {
    /// `/MK` `/BG`.
    pub(crate) background: Option<pdfcer_core::forms::MkColor>,
    /// `/MK` `/BC`.
    pub(crate) border_color: Option<pdfcer_core::forms::MkColor>,
}

/// Parse an `/MK` colour argument into a [`pdfcer_core::forms::MkColor`].
///
/// Accepts `none` -> `MkColor::None`, which writes Table 189's **empty
/// array**. That is the standard's own spelling of *"no colour"* and is
/// deliberately NOT the same as the key being absent, so the CLI has to be
/// able to say it -- otherwise an operator could set a colour and never clear
/// it back to the state the file distinguishes.
///
/// Otherwise 1, 3 or 4 comma-separated numbers, matching Table 189's own
/// component counts: DeviceGray, DeviceRGB, DeviceCMYK. CMYK is passed
/// through as CMYK and never converted, because the count IS the colour
/// space (the same argument `Annotation::color` makes for raw components).
///
/// `None` on any other shape -- an unparseable component, or a count of 2 or
/// 5+, which Table 189 does not define. Refused rather than rounded to the
/// nearest legal count.
pub(crate) fn parse_mk_colour(raw: &str) -> Option<pdfcer_core::forms::MkColor> {
    use pdfcer_core::forms::MkColor;
    if raw.eq_ignore_ascii_case("none") {
        return Some(MkColor::None);
    }
    let parts: Vec<f32> = raw
        .split(',')
        .map(|p| p.trim().parse::<f32>())
        .collect::<Result<_, _>>()
        .ok()?;
    if !parts.iter().all(|v| v.is_finite()) {
        return None;
    }
    match parts.as_slice() {
        [g] => Some(MkColor::Gray(*g)),
        [r, g, b] => Some(MkColor::Rgb(*r, *g, *b)),
        [c, m, y, k] => Some(MkColor::Cmyk(*c, *m, *y, *k)),
        _ => None,
    }
}

/// Borrowed argument bundle for [`cmd_set_field_script`] (clippy arg-count).
pub(crate) struct SetFieldScriptArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) name: &'a str,
    pub(crate) format_number: Option<&'a str>,
    pub(crate) format_percent: Option<&'a str>,
    pub(crate) format_date: Option<i64>,
    pub(crate) format_date_string: Option<&'a str>,
    pub(crate) format_time: Option<i64>,
    pub(crate) format_special: Option<i64>,
    pub(crate) validate_range: Option<&'a str>,
    pub(crate) calculate: Option<&'a str>,
    pub(crate) clear: bool,
    pub(crate) trigger: Option<&'a str>,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `set-field-script` -- author a field's `/AA` format, validate or calculate
/// entry (`Pass 308.6`, request `G024`).
///
/// ## Contract
///
/// - Emits one `set-field-script ...` line carrying `trigger=`, `applied=`,
///   `replaced=`, `keystroke=` and, for a calculation, `co_position=` /
///   `co_entries=`, then defers the exit code to [`finish_edit`].
/// - **`replaced=` is the line that matters.** It names what was displaced, so
///   a script pdfcer could not describe is not overwritten silently. `-` means
///   the trigger was empty.
/// - Exactly one helper argument per run, or `--clear --trigger`. More than
///   one is refused rather than prioritised: there is no sensible precedence
///   between two formats, and picking one would write a script the operator
///   did not choose.
pub(crate) fn cmd_set_field_script(args: &SetFieldScriptArgs<'_>) -> u8 {
    use pdfcer_core::form_script::{AdvisoryHelper, FormatHelper};

    // Count the helper arguments before anything opens the file, so a
    // contradictory command line costs nothing.
    let chosen = usize::from(args.format_number.is_some())
        + usize::from(args.format_percent.is_some())
        + usize::from(args.format_date.is_some())
        + usize::from(args.format_date_string.is_some())
        + usize::from(args.format_time.is_some())
        + usize::from(args.format_special.is_some())
        + usize::from(args.validate_range.is_some())
        + usize::from(args.calculate.is_some())
        + usize::from(args.clear);
    if chosen != 1 {
        eprintln!(
            "pdfcer: set-field-script takes exactly one of --format-number, --format-percent, --format-date, --format-date-string, --format-time, --format-special, --validate-range, --calculate, or --clear --trigger <t>; {chosen} were given"
        );
        return exit::RUNTIME_ERROR;
    }

    let format = if let Some(raw) = args.format_number {
        match parse_number_format(raw) {
            Some(f) => Some(f),
            None => {
                eprintln!(
                    "pdfcer: --format-number {raw:?} -- expected six comma-separated values: decimals,separator,negative,currency-style,symbol,prepend (e.g. 2,0,0,0,$,true)"
                );
                return exit::RUNTIME_ERROR;
            }
        }
    } else if let Some(raw) = args.format_percent {
        let parts: Vec<&str> = raw.split(',').collect();
        match parts.as_slice() {
            [d, sep] => match (d.trim().parse(), sep.trim().parse()) {
                (Ok(decimals), Ok(separator_style)) => Some(FormatHelper::Percent {
                    decimals,
                    separator_style,
                }),
                _ => None,
            },
            _ => None,
        }
        .or_else(|| {
            eprintln!("pdfcer: --format-percent {raw:?} -- expected decimals,separator");
            None
        })
    } else {
        args.format_date
            .map(|index| FormatHelper::Date { index })
            .or_else(|| {
                args.format_date_string.map(|f| FormatHelper::DateEx {
                    format: f.as_bytes().to_vec(),
                })
            })
            .or_else(|| args.format_time.map(|index| FormatHelper::Time { index }))
            .or_else(|| {
                args.format_special
                    .map(|selector| FormatHelper::Special { selector })
            })
    };
    if format.is_none() && (args.format_percent.is_some() || args.format_number.is_some()) {
        return exit::RUNTIME_ERROR;
    }

    let validate = match args.validate_range {
        None => None,
        Some(raw) => match parse_range(raw) {
            Some((lower, upper)) => Some(AdvisoryHelper::RangeValidate { lower, upper }),
            None => {
                eprintln!(
                    "pdfcer: --validate-range {raw:?} -- expected MIN..MAX, MIN.. or ..MAX, with numbers on the bounds that are present"
                );
                return exit::RUNTIME_ERROR;
            }
        },
    };

    let calculate = match args.calculate {
        None => None,
        Some(raw) => match parse_calculation(raw) {
            Some(c) => Some(c),
            None => {
                eprintln!(
                    "pdfcer: --calculate {raw:?} -- expected OP:field,field where OP is SUM, AVG, PRD, MIN or MAX (case sensitive, as Acrobat writes them)"
                );
                return exit::RUNTIME_ERROR;
            }
        },
    };

    let cleared = match (args.clear, args.trigger) {
        (true, Some(t)) => match t {
            "format" | "validate" | "calculate" => Some(t),
            _ => {
                eprintln!("pdfcer: --trigger {t:?} -- known: format, validate, calculate");
                return exit::RUNTIME_ERROR;
            }
        },
        _ => None,
    };

    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let outcome = match (format, validate, calculate, cleared) {
        (Some(f), ..) => session.set_field_format(args.name, Some(f)),
        (_, Some(v), ..) => session.set_field_validation(args.name, Some(v)),
        (_, _, Some(c), _) => session.set_field_calculation(args.name, Some(c)),
        (_, _, _, Some("format")) => session.set_field_format(args.name, None),
        (_, _, _, Some("validate")) => session.set_field_validation(args.name, None),
        (_, _, _, Some(_)) => session.set_field_calculation(args.name, None),
        // Unreachable: the count above admits exactly one, and every one of
        // them lands in a branch. Stated as a refusal rather than a panic --
        // this crate does not panic on a code path an operator can reach.
        (None, None, None, None) => {
            eprintln!("pdfcer: set-field-script: nothing to do");
            return exit::RUNTIME_ERROR;
        }
    };
    let change = match outcome {
        Ok(c) => c,
        Err(err) => return report_edit_error(args.input, &err),
    };

    let saved = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(o) => o,
        Err(code) => return code,
    };
    let r = &saved.report;
    let token = |c: Option<&pdfcer_core::form_script::ScriptClass>| {
        c.map_or_else(|| "-".to_owned(), |c| c.token().to_owned())
    };
    let (co_position, co_entries) = change.calculation_order.map_or_else(
        || ("-".to_owned(), "-".to_owned()),
        |o| {
            (
                o.position.map_or_else(|| "-".to_owned(), |p| p.to_string()),
                o.entries.to_string(),
            )
        },
    );
    println!(
        "set-field-script {} name={:?} trigger={} applied={} replaced={} keystroke={} co_position={co_position} co_entries={co_entries} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.name,
        change.trigger,
        token(change.applied.as_ref()),
        token(change.replaced.as_ref()),
        u32::from(change.keystroke_paired),
        args.mode.name(),
        args.output.display(),
        saved.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(saved.undo_verified),
        u32::from(saved.undo_identical),
    );
    finish_edit(args.input, &saved)
}

/// `decimals,separator,negative,currency-style,symbol,prepend`.
///
/// Six values in `AFNumber_Format`'s own argument order, so an operator who
/// has the Acrobat call in front of them can transcribe it left to right.
pub(crate) fn parse_number_format(raw: &str) -> Option<pdfcer_core::form_script::FormatHelper> {
    let parts: Vec<&str> = raw.split(',').collect();
    let [d, sep, neg, curr, symbol, prepend] = parts.as_slice() else {
        return None;
    };
    Some(pdfcer_core::form_script::FormatHelper::Number {
        decimals: d.trim().parse().ok()?,
        separator_style: sep.trim().parse().ok()?,
        negative_style: neg.trim().parse().ok()?,
        currency_style: curr.trim().parse().ok()?,
        // NOT trimmed: a currency symbol may legitimately be or contain a
        // space, and trimming one would write a symbol the operator did not
        // ask for.
        currency: (*symbol).as_bytes().to_vec(),
        prepend_currency: match prepend.trim() {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => return None,
        },
    })
}

/// `MIN..MAX`, `MIN..` or `..MAX`.
///
/// An absent bound is `None`, which writes `false` into `AFRange_Validate`'s
/// enable flag -- the standard spelling for "this bound is not in force", and
/// not the same as a bound of zero.
pub(crate) fn parse_range(raw: &str) -> Option<(Option<f64>, Option<f64>)> {
    let (lo, hi) = raw.split_once("..")?;
    let parse = |s: &str| -> Option<Option<f64>> {
        let s = s.trim();
        if s.is_empty() {
            Some(None)
        } else {
            s.parse().ok().map(Some)
        }
    };
    let lower = parse(lo)?;
    let upper = parse(hi)?;
    Some((lower, upper))
}

/// `OP:field,field,...`.
///
/// The operation code is matched EXACTLY -- `SimpleOp::from_code` is case
/// sensitive because Acrobat writes `"SUM"`, and accepting `"sum"` here would
/// let the CLI author a call pdfcer's own classifier reads as `Custom`.
pub(crate) fn parse_calculation(raw: &str) -> Option<pdfcer_core::form_script::CalcHelper> {
    let (op, fields) = raw.split_once(':')?;
    let op = pdfcer_core::form_script::SimpleOp::from_code(op.trim().as_bytes())?;
    let operands = fields
        .split(',')
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(|f| f.as_bytes().to_vec())
        .collect();
    Some(pdfcer_core::form_script::CalcHelper::Simple { op, operands })
}

/// Everything `edit-widget` takes. Same reasoning as [`EditFieldArgs`].
pub(crate) struct EditWidgetArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) name: &'a str,
    pub(crate) index: usize,
    pub(crate) rect: Option<&'a str>,
    pub(crate) border_style: Option<&'a str>,
    pub(crate) border_width: Option<f64>,
    pub(crate) visibility: Option<&'a str>,
    pub(crate) caption: Option<&'a str>,
    /// `/MK` `/BG`, as `none` or 1/3/4 comma-separated components.
    pub(crate) background: Option<&'a str>,
    /// `/MK` `/BC`, same spelling as `background`.
    pub(crate) border_color: Option<&'a str>,
    /// How the resize treats stroke width, `/RD` and an appearance pdfcer
    /// cannot rebuild (`Pass 187.0`) — the same three answers
    /// `resize-annotation` takes.
    pub(crate) resize: pdfcer_core::edit::ResizeOptions,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
}

/// `edit-widget` — change one widget's geometry, border, visibility or
/// caption.
pub(crate) fn cmd_edit_widget(args: &EditWidgetArgs<'_>) -> u8 {
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let mut edit = pdfcer_core::edit::WidgetEdit::new();
    if let Some(spec) = args.rect {
        let parts: Vec<f64> = spec
            .split(',')
            .map(|p| p.trim().parse::<f64>())
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_default();
        let [llx, lly, urx, ury] = parts.as_slice() else {
            eprintln!(
                "pdfcer: --rect {spec:?} is not four comma-separated numbers (llx,lly,urx,ury in points, origin BOTTOM-left)"
            );
            return exit::RUNTIME_ERROR;
        };
        edit = edit.with_rect(pdfcer_core::page_tree::Rect {
            llx: *llx,
            lly: *lly,
            urx: *urx,
            ury: *ury,
        });
    }
    // Border style and width are ONE dictionary in the file (`/BS`), so
    // supplying either means writing both — the other half is taken from the
    // Table 166 default rather than from the file, which is stated here
    // because it is the one place this command is lossy.
    if args.border_style.is_some() || args.border_width.is_some() {
        let style = match args.border_style.unwrap_or("solid") {
            "solid" => pdfcer_core::edit::BorderStyle::Solid,
            "dashed" => pdfcer_core::edit::BorderStyle::Dashed,
            "beveled" => pdfcer_core::edit::BorderStyle::Beveled,
            "inset" => pdfcer_core::edit::BorderStyle::Inset,
            "underline" => pdfcer_core::edit::BorderStyle::Underline,
            other => {
                eprintln!(
                    "pdfcer: --border-style {other:?} — known: solid, dashed, beveled, inset, underline"
                );
                return exit::RUNTIME_ERROR;
            }
        };
        edit = edit.with_border(pdfcer_core::edit::BorderSpec {
            style,
            width: args.border_width.unwrap_or(1.0),
        });
    }
    if let Some(v) = args.visibility {
        let visibility = match v {
            "screen-and-print" => pdfcer_core::edit::Visibility::VisibleAndPrints,
            "screen-only" => pdfcer_core::edit::Visibility::ScreenOnly,
            "print-only" => pdfcer_core::edit::Visibility::PrintOnly,
            "hidden" => pdfcer_core::edit::Visibility::Hidden,
            other => {
                eprintln!(
                    "pdfcer: --visibility {other:?} — known: screen-and-print, screen-only, print-only, hidden"
                );
                return exit::RUNTIME_ERROR;
            }
        };
        edit = edit.with_visibility(visibility);
    }
    if let Some(c) = args.caption {
        edit = edit.with_caption(c);
    }
    for (raw, which) in [
        (args.background, "--background"),
        (args.border_color, "--border-color"),
    ] {
        let Some(raw) = raw else { continue };
        let Some(colour) = parse_mk_colour_edit(raw) else {
            eprintln!(
                "pdfcer: {which} {raw:?} -- expected `none` (Table 189's empty array, which STATES no colour), `unset` (REMOVE the key, so the builder's own default stands), or 1 (gray), 3 (RGB) or 4 (CMYK) comma-separated components in 0-1"
            );
            return exit::RUNTIME_ERROR;
        };
        // `Pass 308.3`: the enum carries the removal, so the CLI does not need
        // a second flag and the two spellings stay one argument.
        edit = match (which, colour) {
            ("--background", pdfcer_core::edit::MkColorEdit::Set(c)) => edit.with_background(c),
            ("--background", _) => edit.without_background(),
            (_, pdfcer_core::edit::MkColorEdit::Set(c)) => edit.with_border_color(c),
            (_, _) => edit.without_border_color(),
        };
    }
    edit = edit.with_resize(args.resize);

    let outcome = match session.edit_widget(args.name, args.index, &edit) {
        Ok(o) => o,
        Err(err) => return report_edit_error(args.input, &err),
    };

    if let Some(stale) = &outcome.appearance_stale {
        eprintln!(
            "pdfcer: field {:?} widget {}: ★ {stale}",
            args.name, args.index
        );
    }
    // Rule 4 in the shell that has no session: the invocation IS the commit,
    // so anything pdfcer decided on the way past is printed on the way past.
    match outcome.stroke_width {
        Some((before, after)) => eprintln!(
            "pdfcer: field {:?} widget {}: border width scaled with the box, {before} -> {after} pt.",
            args.name, args.index
        ),
        None if outcome.resized => eprintln!(
            "pdfcer: field {:?} widget {}: its border width was NOT scaled — a line weight is a drafting convention rather than a length in the scaled space, which is why the default leaves it alone. Pass --scale-stroke-width if you wanted it to follow.",
            args.name, args.index
        ),
        None => {}
    }
    if outcome.rect_differences_scaled == Some(false) {
        eprintln!(
            "pdfcer: field {:?} widget {}: this widget has /RD (rect differences) and they were left UNSCALED, so its inner margins are now a different proportion of the box.",
            args.name, args.index
        );
    }
    if outcome.siblings_untouched > 0 {
        eprintln!(
            "pdfcer: field {:?}: {} OTHER widget(s) of this field are unchanged and stand where they were. Position, border, visibility and caption are per-placement, so this is correct — but a field that looks like one thing to an operator now has two appearances.",
            args.name, outcome.siblings_untouched
        );
    }

    let saved = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };
    let rect_token = |r: Option<pdfcer_core::page_tree::Rect>| {
        r.map_or_else(
            || "-".to_owned(),
            |r| format!("{},{},{},{}", r.llx, r.lly, r.urx, r.ury),
        )
    };
    println!(
        "edit-widget {} name={:?} index={} -> {} rect={}->{} resized={} regenerated={} siblings_untouched={} changed_objects={}",
        args.input.display(),
        args.name,
        args.index,
        args.output.display(),
        rect_token(outcome.rect_before),
        rect_token(outcome.rect_after),
        u32::from(outcome.resized),
        u32::from(outcome.appearance_regenerated),
        outcome.siblings_untouched,
        saved.changed,
    );
    finish_edit(args.input, &saved)
}

/// `rename-field` — change a field's partial name `/T` (decision 020's F6).
///
/// The disclosure this exists to carry is `descendants_renamed`. Renaming a
/// grouping node re-derives every descendant's fully-qualified name without
/// writing to one of them (§12.7.3.2), so the operator's one-field request
/// can rename a subtree. Saying so is rule 4; leaving it to be discovered
/// when an FDF stops matching is not.
pub(crate) fn cmd_rename_field(
    input: &Path,
    name: &str,
    to: &str,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let rename = match session.rename_field(name, to) {
        Ok(r) => r,
        Err(err) => return report_edit_error(input, &err),
    };

    // In prose, for the person about to wonder why six fields moved. The
    // machine-readable count is on the result line below.
    if rename.descendants_renamed > 0 {
        eprintln!(
            "pdfcer: field {name:?}: {} field(s) beneath it now have different fully-qualified names, because §12.7.3.2 builds those names from this one — no object of theirs was written. Button actions naming them ARE repaired -- see the line below -- but an FDF or a JavaScript naming them is not, and no longer matches",
            rename.descendants_renamed
        );
    }

    // `Pass 184.0`. Reset, submit and show/hide buttons name their targets by
    // NAME, so a rename breaks them -- and this repairs them in the same
    // undoable command. Reported because pdfcer edited objects the operator did
    // not name, which is exactly what rule 4 is about, and because a form whose
    // logic lives in a SCRIPT is NOT fixed by this and must not be implied to
    // be.
    if rename.action_targets_retargeted > 0 {
        eprintln!(
            "pdfcer: field {name:?}: {} button-action target(s) named the old name and were repointed at {:?} in the same undoable step -- reset, submit and show/hide buttons name their fields by NAME, so a rename would otherwise have left them pointing at nothing. JavaScript is NOT rewritten: a script that names the old field is still broken.",
            rename.action_targets_retargeted, rename.to
        );
    }

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
        "rename-field {} from={:?} to={:?} descendants_renamed={} action_targets_retargeted={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        rename.from,
        rename.to,
        rename.descendants_renamed,
        rename.action_targets_retargeted,
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

/// The `rotate-widget` argument bundle -- seven parameters, past the point
/// where same-typed positionals stay readable.
pub(crate) struct RotateWidgetArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) name: &'a str,
    pub(crate) index: usize,
    pub(crate) degrees: i64,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `rotate-widget` -- set a form-field widget's `/MK /R` and redraw its
/// appearance in the rotated frame (Pass 177.0).
///
/// ## Contract
///
/// - Emits one `rotate-widget ...` line carrying `was=` / `now=`,
///   `normalised=`, `regenerated=` and `siblings_untouched=`, then defers the
///   exit code to [`finish_edit`].
/// - **`was=-` means the file was SILENT**, which is a different fact from
///   `was=0`. Table 189 defaults `/R` to 0, so a silent file renders upright
///   -- but writing `0` into a widget whose `/MK` never had the key changes
///   the saved bytes for no visible change. The CLI prints the distinction
///   because a script that treated them as equal would write that invention
///   back.
/// - **`regenerated=0` gets a second line, always.** It means `/MK /R` was
///   written and the pixels did not move, which is the outcome most likely to
///   be reported as a defect: a conforming PDF 2.0 reader ignores `/MK` when
///   an appearance stream is present (PDF Association erratum #56), so the
///   field still looks upright there. The engine's own sentence is printed
///   verbatim rather than paraphrased.
/// - A non-multiple of 90 is refused through [`report_edit_error`] with the
///   engine's message.
pub(crate) fn cmd_rotate_widget(args: &RotateWidgetArgs) -> u8 {
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let rotation = match session.rotate_widget(args.name, args.index, args.degrees) {
        Ok(r) => r,
        Err(err) => return report_edit_error(args.input, &err),
    };
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
        "rotate-widget {} name={} index={} was={} now={} normalised={} regenerated={} siblings_untouched={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.name,
        args.index,
        // `-` for "the file said nothing", never `0`.
        rotation
            .was
            .map_or_else(|| "-".to_owned(), |d| d.to_string()),
        rotation
            .now
            .map_or_else(|| "-".to_owned(), |d| d.to_string()),
        u32::from(rotation.normalised),
        u32::from(rotation.appearance_regenerated),
        rotation.siblings_untouched,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    if let Some(note) = &rotation.appearance_stale {
        println!("  note: {note}");
    }
    finish_edit(args.input, &outcome)
}

/// The `set-button-action` argument bundle.
pub(crate) struct SetButtonActionArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) name: &'a str,
    pub(crate) reset: bool,
    pub(crate) reset_only: &'a [String],
    pub(crate) reset_except: &'a [String],
    pub(crate) submit: Option<&'a str>,
    pub(crate) submit_format: SubmitFormatArg,
    pub(crate) submit_only: &'a [String],
    pub(crate) submit_except: &'a [String],
    pub(crate) submit_get: bool,
    pub(crate) submit_coordinates: bool,
    pub(crate) include_no_value_fields: bool,
    pub(crate) canonical_dates: bool,
    pub(crate) include_annotations: bool,
    pub(crate) only_current_user_annotations: bool,
    pub(crate) include_incremental_updates: bool,
    pub(crate) exclude_document_path: bool,
    pub(crate) embed_form: bool,
    pub(crate) goto_page: Option<usize>,
    pub(crate) goto_view: GotoViewArg,
    pub(crate) hide: &'a [String],
    pub(crate) show: &'a [String],
    pub(crate) named: Option<NamedActionArg>,
    pub(crate) uri: Option<&'a str>,
    pub(crate) clear: bool,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

impl SetButtonActionArgs<'_> {
    /// Build the `/SubmitForm` spec, refusing every option that belongs to a
    /// different format **by name**.
    ///
    /// # Why refuse rather than ignore
    ///
    /// `pdfcer-core`'s `SubmitFormat` makes the standard's nine flag gates
    /// unrepresentable — `--submit-get` has nowhere to live in an FDF submit.
    /// A shell that simply dropped it would parse a flag, act on nothing, and
    /// print a success line: the operator asked for GET, was told the submit
    /// was written, and got POST. That is the "a shell flag can be parsed and
    /// never used" failure, and unit tests against core cannot see it because
    /// core was never asked.
    pub(crate) fn submit_spec(&self, url: &str) -> Result<pdfcer_core::edit::SubmitSpec, String> {
        use pdfcer_core::edit::{FdfOptions, SubmitFormat, SubmitScope, SubmitSpec};

        let html_only = [
            (self.submit_get, "--submit-get"),
            (self.submit_coordinates, "--submit-coordinates"),
        ];
        let fdf_only = [
            (self.include_annotations, "--include-annotations"),
            (
                self.only_current_user_annotations,
                "--only-current-user-annotations",
            ),
            (
                self.include_incremental_updates,
                "--include-incremental-updates",
            ),
            (self.exclude_document_path, "--exclude-document-path"),
            (self.embed_form, "--embed-form"),
        ];
        let refuse = |set: &[(bool, &str)], why: &str| -> Result<(), String> {
            for (given, flag) in set {
                if *given {
                    return Err(format!("{flag} {why}"));
                }
            }
            Ok(())
        };

        let format = match self.submit_format {
            SubmitFormatArg::Fdf => {
                refuse(
                    &html_only,
                    "applies only to --submit-format html: ISO 32000-1 Table 237 says bits 4 \
and 5 `shall` be clear unless ExportFormat is set",
                )?;
                let mut opts = FdfOptions::default();
                opts.include_annotations = self.include_annotations;
                opts.only_current_user_annotations = self.only_current_user_annotations;
                opts.include_incremental_updates = self.include_incremental_updates;
                opts.exclude_document_path = self.exclude_document_path;
                opts.embed_form = self.embed_form;
                SubmitFormat::Fdf(opts)
            }
            SubmitFormatArg::Html => {
                refuse(
                    &fdf_only,
                    "applies only to --submit-format fdf: Table 237 says each of bits 7, 8, 11, \
12 and 14 `shall be used only when the form is being submitted in Forms Data Format`",
                )?;
                SubmitFormat::Html {
                    get: self.submit_get,
                    coordinates: self.submit_coordinates,
                }
            }
            SubmitFormatArg::Xfdf => {
                refuse(&html_only, "applies only to --submit-format html")?;
                refuse(&fdf_only, "applies only to --submit-format fdf")?;
                SubmitFormat::Xfdf
            }
            SubmitFormatArg::Pdf => {
                refuse(&html_only, "applies only to --submit-format html")?;
                refuse(&fdf_only, "applies only to --submit-format fdf")?;
                // Bit 9: "all other flags shall be ignored except GetMethod".
                // Refused rather than accepted-and-ignored so the operator
                // learns their selection did nothing NOW, not from a server
                // that received the whole file.
                refuse(
                    &[
                        (!self.submit_only.is_empty(), "--submit-only"),
                        (!self.submit_except.is_empty(), "--submit-except"),
                        (self.include_no_value_fields, "--include-no-value-fields"),
                        (self.canonical_dates, "--canonical-dates"),
                    ],
                    "has no meaning with --submit-format pdf: bit 9 says all other flags \
`shall be ignored`, so the ENTIRE document goes and field selection does not apply",
                )?;
                SubmitFormat::WholeDocument
            }
        };

        let scope = if !self.submit_only.is_empty() {
            SubmitScope::Only(self.submit_only.to_vec())
        } else if !self.submit_except.is_empty() {
            SubmitScope::Except(self.submit_except.to_vec())
        } else {
            SubmitScope::All
        };

        let mut spec = SubmitSpec::new(url);
        spec.format = format;
        spec.scope = scope;
        spec.include_no_value_fields = self.include_no_value_fields;
        spec.canonical_dates = self.canonical_dates;
        Ok(spec)
    }

    /// Every submit-shaped option that was given without `--submit`.
    ///
    /// Same reasoning as [`Self::submit_spec`]'s refusals, one level up: a
    /// `--submit-format html` on a `--reset` invocation is an operator who
    /// believes something about the command that is not true.
    pub(crate) fn stray_submit_options(&self) -> Vec<&'static str> {
        [
            (!self.submit_only.is_empty(), "--submit-only"),
            (!self.submit_except.is_empty(), "--submit-except"),
            (self.submit_get, "--submit-get"),
            (self.submit_coordinates, "--submit-coordinates"),
            (self.include_no_value_fields, "--include-no-value-fields"),
            (self.canonical_dates, "--canonical-dates"),
            (self.include_annotations, "--include-annotations"),
            (
                self.only_current_user_annotations,
                "--only-current-user-annotations",
            ),
            (
                self.include_incremental_updates,
                "--include-incremental-updates",
            ),
            (self.exclude_document_path, "--exclude-document-path"),
            (self.embed_form, "--embed-form"),
        ]
        .into_iter()
        .filter_map(|(given, flag)| given.then_some(flag))
        .collect()
    }
}

/// `set-button-action` -- attach a Reset action to a push button, or remove
/// one (Pass 182.0).
///
/// ## Contract
///
/// - Emits one `set-button-action ...` line carrying `action=` and
///   `replaced=`, then defers the exit code to [`finish_edit`].
/// - **`replaced=` names what was destroyed**, including an action pdfcer
///   would never author -- a script, a submit. A form editor overwriting
///   somebody else's button should know what it took out, and reporting a
///   removed script as "nothing" is the failure that reads as safe.
/// - Exactly one of the four mode flags is required; clap enforces the
///   exclusivity and this function refuses the empty case by name rather
///   than defaulting to one, because every default here is a different
///   document.
pub(crate) fn cmd_set_button_action(args: &SetButtonActionArgs) -> u8 {
    use pdfcer_core::edit::{ButtonAction, ResetScope};

    if args.submit.is_none() {
        let stray = args.stray_submit_options();
        if !stray.is_empty() {
            eprintln!(
                "pdfcer: {} {} only to --submit, and this is not a submit. Nothing was \
written -- a flag that is parsed and then ignored is how an operator ends up believing \
something about a file that is not true.",
                stray.join(", "),
                if stray.len() == 1 { "applies" } else { "apply" }
            );
            return exit::EDIT_REFUSED;
        }
    }

    let action = if args.clear {
        None
    } else if args.reset {
        Some(ButtonAction::ResetForm {
            scope: ResetScope::All,
        })
    } else if !args.reset_only.is_empty() {
        Some(ButtonAction::ResetForm {
            scope: ResetScope::Only(args.reset_only.to_vec()),
        })
    } else if !args.reset_except.is_empty() {
        Some(ButtonAction::ResetForm {
            scope: ResetScope::Except(args.reset_except.to_vec()),
        })
    } else if let Some(url) = args.submit {
        match args.submit_spec(url) {
            Ok(spec) => Some(ButtonAction::SubmitForm(spec)),
            Err(why) => {
                eprintln!("pdfcer: {why}");
                return exit::EDIT_REFUSED;
            }
        }
    } else if let Some(page_index) = args.goto_page {
        Some(ButtonAction::GoToPage {
            page_index,
            view: args.goto_view.into(),
        })
    } else if !args.hide.is_empty() {
        Some(ButtonAction::SetHidden {
            targets: args.hide.to_vec(),
            hidden: true,
        })
    } else if !args.show.is_empty() {
        Some(ButtonAction::SetHidden {
            targets: args.show.to_vec(),
            hidden: false,
        })
    } else if let Some(named) = args.named {
        Some(ButtonAction::Named(named.into()))
    } else if let Some(uri) = args.uri {
        Some(ButtonAction::Uri {
            uri: uri.to_owned(),
        })
    } else {
        eprintln!(
            "pdfcer: say what the button should do: --reset, --reset-only A,B, \
--reset-except A,B, --submit URL, --hide A,B, --show A,B, --goto-page N, --named next-page, --uri URL, or --clear to \
remove its action"
        );
        return exit::EDIT_REFUSED;
    };

    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let change = match session.set_button_action(args.name, action) {
        Ok(c) => c,
        Err(err) => return report_edit_error(args.input, &err),
    };
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
    let applied = match &change.applied {
        None => "none",
        Some(ButtonAction::ResetForm { .. }) => "ResetForm",
        Some(ButtonAction::SubmitForm(_)) => "SubmitForm",
        Some(ButtonAction::GoToPage { .. }) => "GoTo",
        Some(ButtonAction::SetHidden { hidden: true, .. }) => "Hide",
        Some(ButtonAction::SetHidden { hidden: false, .. }) => "Show",
        Some(ButtonAction::Named(_)) => "Named",
        Some(ButtonAction::Uri { .. }) => "URI",
        // `ButtonAction` is `#[non_exhaustive]`: a variant added in core and
        // not taught to this shell would otherwise stop compiling here, which
        // is the right outcome, but the arm has to exist for the crate to
        // build at all across a version skew.
        Some(_) => "other",
    };
    println!(
        "set-button-action {} name={} action={} replaced={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.name,
        applied,
        change.replaced.as_deref().unwrap_or("-"),
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    if let Some(d) = &change.submit {
        report_submit_disclosure(args.input, d);
    }
    if let Some(d) = &change.hide {
        report_hide_disclosure(args.input, d);
    }
    finish_edit(args.input, &outcome)
}

/// **State what a `/Hide` button just authored would move** (`Pass 183.1`).
///
/// ## Contract
///
/// - Goes to **stderr**, like every other disclosure here.
/// - Names the direction explicitly. `/H`'s default is *hide*, so "show" is
///   the case a file can lose by omission, and an operator reading a caption
///   has no way to check which was written.
/// - Reads **every** field of `HideDisclosure`, per
///   `tools/check-outcome-disclosed.py`.
pub(crate) fn report_hide_disclosure(input: &Path, d: &pdfcer_core::edit::HideDisclosure) {
    eprintln!(
        "pdfcer: {}: this button {} {} field(s) -- {} -- affecting {} widget(s).",
        input.display(),
        if d.shows { "SHOWS" } else { "HIDES" },
        d.targets.len(),
        d.targets.join(", "),
        d.widgets_affected,
    );
    if !d.targets_without_widgets.is_empty() {
        eprintln!(
            "  {} of them own no widget, so for those the button does nothing: {}",
            d.targets_without_widgets.len(),
            d.targets_without_widgets.join(", ")
        );
    }
    eprintln!(
        "  a hide action ASSIGNS, it does not toggle -- pressing it twice does not put things \
back. A field's widgets on every page move together."
    );
}

/// **State what the button just authored would send** (`Pass 183.0`).
///
/// ## Contract
///
/// - Prints the always-on summary first, then every itemised list that is not
///   empty. In the GUI the itemisation is one gesture away; in `pdfcer` the
///   invocation IS the commit (rule 11), so there is no later screen and it is
///   printed on the way past.
/// - Goes to **stderr**, like every other disclosure here, so the stdout
///   metrics line stays machine-parseable.
/// - Reads **every** field of `SubmitDisclosure`. A field no shell reads is a
///   disclosure that does not happen — see `tools/check-outcome-disclosed.py`,
///   which enforces exactly that and lists this struct.
pub(crate) fn report_submit_disclosure(input: &Path, d: &pdfcer_core::edit::SubmitDisclosure) {
    eprintln!("pdfcer: {}: this button {}", input.display(), d.summary());
    eprintln!(
        "  destination: {} (scheme {}, {}), format {}, method {}",
        d.url,
        d.scheme,
        if d.encrypted {
            "encrypted"
        } else {
            "NOT encrypted -- the data travels in the clear"
        },
        d.format,
        d.method,
    );
    if d.whole_document {
        eprintln!(
            "  the ENTIRE document file is sent. Field selection does not apply -- there is no \
partial-PDF submission -- so attachments, metadata, every prior revision and any private \
application data go with it."
        );
    } else {
        eprintln!(
            "  {} field value(s){}",
            d.fields.len(),
            if d.fields.is_empty() {
                String::new()
            } else {
                format!(": {}", d.fields.join(", "))
            }
        );
    }
    let lists: [(&[String], &str); 6] = [
        (
            &d.hidden_fields,
            "HIDDEN field(s) -- their values are sent and you were never shown them",
        ),
        (&d.password_fields, "password field(s)"),
        (
            &d.file_select_fields,
            "file-select field(s) -- each sends the CONTENTS of a local file named by its own text",
        ),
        (
            &d.valueless_fields,
            "empty field(s), sent by name only (form structure, not data)",
        ),
        (
            &d.excluded_by_no_export,
            "field(s) NOT sent: their NoExport flag overrides your selection",
        ),
        (
            &d.required_without_value,
            "required field(s) with no value yet -- the standard obliges them to have one at \
submit time and names no consequence",
        ),
    ];
    for (names, why) in lists {
        if !names.is_empty() {
            eprintln!("  {} {}: {}", names.len(), why, names.join(", "));
        }
    }
    if d.includes_document_path {
        eprintln!(
            "  the payload also carries THIS DOCUMENT'S OWN FILE PATH and its identity \
fingerprint. That is the baseline FDF behaviour with no flag set; --exclude-document-path \
suppresses it."
        );
    }
    if d.includes_incremental_updates {
        eprintln!(
            "  the payload carries every incremental update since the document was opened, \
signatures included -- and a SAVE is performed immediately before sending."
        );
    }
    if d.includes_annotations {
        eprintln!("  the payload carries the document's markup annotations, whoever wrote them.");
    }
    if d.embeds_source_document {
        eprintln!("  the payload embeds a copy of this whole PDF inside itself.");
    }
    if d.includes_click_coordinates {
        eprintln!("  the payload carries where the mouse was clicked on the button.");
    }
    eprintln!(
        "  pdfcer sent nothing: it has no network code and fires no trigger. This describes what \
another program would send if it honoured the button."
    );
}

/// `move-widget` — translate one widget annotation's `/Rect`.
///
/// The disclosure this verb owes the operator is `siblings_left_behind`: a
/// field with widgets on three pages looks like ONE thing to someone who
/// asked to move "the signature box", and moving one while silently leaving
/// two behind is the kind of partial result that reads as a bug an hour
/// later. It is printed in prose when it is non-zero, and always on the
/// machine-readable line.
pub(crate) fn cmd_move_widget(
    input: &Path,
    name: &str,
    index: usize,
    dx: f64,
    dy: f64,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let moved = match session.move_widget(name, index, dx, dy) {
        Ok(m) => m,
        Err(err) => return report_edit_error(input, &err),
    };

    if moved.siblings_left_behind > 0 {
        eprintln!(
            "pdfcer: field {name:?}: moved widget {index} only — {} other widget(s) of this field stayed where they were. A field's widgets are separate appearances and can sit on different pages; move each one you want moved",
            moved.siblings_left_behind
        );
    }

    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "move-widget {} name={name:?} index={index} dx={dx} dy={dy} from=[{} {} {} {}] to=[{} {} {} {}] siblings_left_behind={} mode={} -> {}; changed={} objects={} appended={} out_bytes={}",
        input.display(),
        moved.from.llx,
        moved.from.lly,
        moved.from.urx,
        moved.from.ury,
        moved.to.llx,
        moved.to.lly,
        moved.to.urx,
        moved.to.ury,
        moved.siblings_left_behind,
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
    );
    finish_edit(input, &outcome)
}

/// `delete-field` and `delete-widget` — the two deletion verbs, which differ
/// only in whether an index was given.
///
/// ## Why ONE function behind two subcommands
///
/// They are the same operation with a different scope, and §3.6.3 makes the
/// last-member case of `delete-widget` *become* `delete-field` — so a second
/// implementation would be two code paths that have to agree about what
/// "gone" means, which is the kind of agreement that quietly lapses. The
/// subcommands stay separate at the surface because `--index` is meaningless
/// for one of them and mandatory for the other, and an optional index whose
/// absence silently means "delete everything" is a footgun.
///
/// ## Contract
///
/// - Emits one `delete-field …` / `delete-widget …` line carrying
///   `widgets_removed=`, `field_removed=`, `selection_cleared=` and
///   `emptied_parents=`, then defers the exit code to [`finish_edit`].
/// - `selection_cleared=1` is §3.6.3's required disclosure and is ALSO
///   printed in prose to stderr — a value the operator set, silently
///   discarded, is exactly what rule 4 forbids.
/// - Refusals — no such field, an index past the end, an encrypted or
///   certified document — go through [`report_edit_error`] before any
///   mutation.
pub(crate) fn cmd_delete_form_field(
    input: &Path,
    name: &str,
    index: Option<usize>,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let outcome_of_delete = match index {
        Some(i) => session.delete_widget(name, i),
        None => session.delete_field(name),
    };
    let deletion = match outcome_of_delete {
        Ok(d) => d,
        Err(err) => return report_edit_error(input, &err),
    };

    // §3.6.3's disclosure, in prose. The machine-readable field below is for
    // scripts; this is for the person who is about to wonder why the form
    // came back blank.
    if deletion.selection_cleared {
        eprintln!(
            "pdfcer: field {name:?}: the widget you deleted held this field's selected value, which no remaining widget can display — the selection has been cleared to Off"
        );
    }
    // `Pass 184.0`. Counted, never repaired: a deletion supplies no
    // replacement name, and quietly dropping the entry would change what the
    // button does to the fields that remain. Same traversal as the rename's
    // repair, opposite conclusion, and the difference is whether pdfcer has to
    // invent anything.
    if deletion.action_targets_orphaned > 0 {
        eprintln!(
            "pdfcer: field {name:?}: {} button-action target(s) still name it and now point at nothing. pdfcer does NOT repair these -- there is no correct field to repoint a Reset button at -- so each is a button that will do less than it says. They are invisible to the dangling-reference census, because a name is not a reference.",
            deletion.action_targets_orphaned
        );
    }
    if deletion.emptied_parents > 0 {
        eprintln!(
            "pdfcer: field {name:?}: {} grouping node(s) were left with no fields beneath them and were removed as well — a named node owning nothing still occupies its slot in the field-name space",
            deletion.emptied_parents
        );
    }

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
    let verb = if index.is_some() {
        "delete-widget"
    } else {
        "delete-field"
    };
    println!(
        "{verb} {} name={:?} index={} widgets_removed={} field_removed={} selection_cleared={} emptied_parents={} action_targets_orphaned={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        name,
        index.map_or_else(|| "-".to_owned(), |i| i.to_string()),
        deletion.widgets_removed,
        u32::from(deletion.field_removed),
        u32::from(deletion.selection_cleared),
        deletion.emptied_parents,
        deletion.action_targets_orphaned,
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

/// `delete-field-group` — remove a grouping node and its whole subtree.
///
/// # Why this command refuses to run without `--yes`
///
/// Every other delete in this CLI removes the thing you named. This one
/// removes the thing you named **and every field beneath it**, and the
/// operator typing the command cannot see that set — a subtree is exactly
/// the shape whose contents are invisible from its name. `Personal` might
/// be one field or forty.
///
/// So the default is the listing: resolve the node, print the terminals by
/// name, write nothing, exit `0`. `--yes` is the second, deliberate act.
/// This is rule 4's disclosure obligation in the shape a CLI can honour —
/// there is no canvas to show the affected fields on, so the names are the
/// disclosure, and a flag is the confirmation.
///
/// Exiting `0` from the dry run is deliberate: nothing failed. A non-zero
/// exit would make a scripted preview indistinguishable from a refusal, and
/// the whole point is that previewing is a normal, expected thing to do.
///
/// # Why the terminals are listed on stdout, not stderr
///
/// The opposite of `fill-field`'s conversion note. That note is an aside
/// about an operation you asked for; this listing **is** the output of the
/// dry run — the answer to the question the invocation asked. A script
/// capturing stdout wants it.
pub(crate) fn cmd_delete_field_group(
    input: &Path,
    name: &str,
    output: &Path,
    yes: bool,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // The preview runs the same gates as the deletion, so a dry run that
    // succeeds is a promise the real run can keep.
    let preview = match session.field_group_deletion_preview(name) {
        Ok(p) => p,
        Err(err) => return report_edit_error(input, &err),
    };

    if !yes {
        // The listing IS the output. One line per terminal so the set is
        // greppable and diffable, then a summary that says what else goes.
        // Terminals AND the grouping nodes, both by name. The nodes matter
        // to the operator for a reason the count cannot convey: each one is
        // a NAME being freed, and a name that comes back is a name a later
        // `add-*-field` can take. "nodes=2" does not tell them `Personal`
        // is about to become available again.
        for t in &preview.terminals {
            println!("would-delete field={t:?}");
        }
        for n in &preview.nodes {
            println!("would-delete group={n:?}");
        }
        println!(
            "delete-field-group {} name={:?} DRY-RUN terminals={} widgets={} nodes={} — nothing written; pass --yes to delete",
            input.display(),
            name,
            preview.terminals.len(),
            preview.widgets_removed,
            preview.nodes_removed,
        );
        return exit::SUCCESS;
    }

    let deletion = match session.delete_field_group(name) {
        Ok(d) => d,
        Err(err) => return report_edit_error(input, &err),
    };

    // Named even on the real run. The operator may have passed `--yes`
    // straight away, and a destructive act should say what it destroyed
    // whether or not it was previewed first.
    for t in &deletion.terminals {
        println!("deleted field={t:?}");
    }
    // `Pass 184.0`. Counted, never repaired -- see the field-delete path for
    // why a deletion cannot supply a replacement name. Matched by PREFIX here,
    // because a grouping node takes its whole subtree with it.
    if deletion.action_targets_orphaned > 0 {
        eprintln!(
            "pdfcer: {} button-action target(s) still name a field in this group and now point at nothing. pdfcer does NOT repair these -- there is no correct field to repoint a Reset button at. They are invisible to the dangling-reference census, because a name is not a reference.",
            deletion.action_targets_orphaned
        );
    }

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
        "delete-field-group {} name={:?} terminals={} widgets_removed={} nodes_removed={} action_targets_orphaned={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        name,
        deletion.terminals.len(),
        deletion.widgets_removed,
        deletion.nodes_removed,
        deletion.action_targets_orphaned,
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

/// `add-radio-button` — author ONE member of a radio group.
///
/// ## Contract
///
/// - Emits one `add-radio-button …` line with the usual save-report fields,
///   then defers the exit code to [`finish_edit`].
/// - **One invocation adds one MEMBER.** Repeating the verb with the same
///   `--name` and a different `--export-value` is how a group is built; the
///   `merged=` field in the output line says which happened, so a script can
///   tell "created the group" from "joined it" without re-reading the file.
/// - Refusals — an `Off` export value, a duplicate export value in a group
///   that is not `--radios-in-unison`, a positional-`/Opt` group pdfcer cannot
///   extend, a name already used by a different field type or KIND, plus
///   every structural refusal the sibling authoring verbs share — go through
///   [`report_edit_error`] BEFORE any mutation.
/// - `--page` is 1-BASED here and 0-based in the core call.
pub(crate) fn cmd_add_radio_button(args: &AddRadioButtonArgs<'_>) -> u8 {
    let (page_index, rect) = match parse_page_and_rect(args.input, args.page, args.rect) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let mut spec =
        pdfcer_core::edit::NewRadioButton::new(page_index, args.name, rect, args.export_value)
            .selected(args.selected)
            .with_group_flags(args.no_toggle_to_off, args.radios_in_unison)
            .with_flags(args.read_only, args.required)
            .with_border(args.border.into(), args.border_width)
            .with_visibility(args.visibility.into());

    // `Pass 308.1`: the colours land on the SPEC, so the `/MK` dictionary and
    // the `/AP` artwork are written from one value. Parsed before anything is
    // staged, so a mistyped colour costs nothing.
    let chrome = match parse_creation_chrome(args.background, args.border_color) {
        Ok(c) => c,
        Err(code) => return code,
    };
    if let Some(c) = chrome.background {
        spec = spec.with_background(c);
    }
    if let Some(c) = chrome.border_color {
        spec = spec.with_border_color(c);
    }
    // R105, exactly as the sibling verbs: `clap`'s `conflicts_with` rules out
    // BOTH being passed, so only "neither" can reach here, and it is refused
    // rather than defaulted.
    spec = match (args.tooltip, args.no_tooltip) {
        (Some(t), _) => spec.with_tooltip(t),
        (None, true) => spec.declining_tooltip(),
        (None, false) => {
            eprintln!(
                "pdfcer: {}: decide about the accessibility name — pass --tooltip <text>, or --no-tooltip to decline it. It is what a screen reader announces for this field, so it is never defaulted silently.",
                args.input.display()
            );
            return exit::EDIT_REFUSED;
        }
    };

    // Applied to the SPEC before authoring, so everything downstream
    // — the merge check, the appearance build, the undo entry — sees
    // one fully-formed request rather than a partially-defaulted one.
    let defaults = match read_defaults(&session, args.input, args.defaults_from) {
        Ok(d) => d,
        Err(code) => return code,
    };
    let applied = defaults
        .map(|d| spec.apply_defaults(&d))
        .unwrap_or_default();
    let authored = match session.add_radio_button(&spec) {
        Ok(o) => o,
        Err(err) => return report_edit_error(args.input, &err),
    };
    let field_id = authored.field_id;
    // Folded into the SAME disclosure struct the core produced, not
    // reported alongside it: one channel, so `any()` still answers for
    // everything and a caller gating on it cannot miss half the facts.
    let mut disclosures = authored.disclosures;
    disclosures.defaults_type_mismatch = applied.type_mismatch;
    disclosures.defaults_on_state_ambiguous = applied.on_state_ambiguous;
    report_field_disclosures(args.name, disclosures);
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
        "add-radio-button {} name={:?} page={} rect={},{},{},{} export_value={:?} selected={} field={} {} merged={} tagged={} struct_tabs={} tooltip_declined={} background={} border_color={} flags_ignored={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.name,
        args.page,
        rect.llx,
        rect.lly,
        rect.urx,
        rect.ury,
        args.export_value,
        u32::from(args.selected),
        field_id.num,
        field_id.generation,
        u32::from(authored.merged),
        u32::from(authored.disclosures.tagged_document),
        u32::from(authored.disclosures.structure_tab_order),
        u32::from(authored.disclosures.tooltip_declined),
        mk_colour_token(spec.chrome.background),
        mk_colour_token(spec.chrome.border_color),
        u32::from(authored.disclosures.group_flags_ignored),
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

/// `add-choice-field` — author a new list box or drop-down.
///
/// ## Contract
///
/// - Emits one `add-choice-field …` line with the usual save-report fields,
///   then defers the exit code to [`finish_edit`].
/// - `--option EXPORT=LABEL` splits the submitted value from the displayed
///   one; `--option LABEL` makes them the same. **The first `=` splits**, so
///   a label may contain `=` and an export value may not — the export value
///   is form data and the label is prose, and prose is where an `=` actually
///   turns up.
/// - Refusals — no options, `--editable` without `--combo`, a duplicated
///   export value, plus every structural refusal the other authoring
///   subcommands share — go through [`report_edit_error`] before any
///   mutation.
pub(crate) fn cmd_add_choice_field(args: &AddChoiceFieldArgs<'_>) -> u8 {
    let (page_index, rect) = match parse_page_and_rect(args.input, args.page, args.rect) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let options: Vec<pdfcer_core::edit::ChoiceOption> = args
        .options
        .iter()
        .map(|raw| match raw.split_once('=') {
            Some((export, display)) => pdfcer_core::edit::ChoiceOption::new(export, display),
            None => pdfcer_core::edit::ChoiceOption::plain(raw),
        })
        .collect();

    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let mut spec = pdfcer_core::edit::NewChoiceField::new(page_index, args.name, rect, options)
        .multi_select(args.multi_select)
        .sorted(args.sort)
        .with_flags(args.read_only, args.required)
        .with_border(args.border.into(), args.border_width)
        .with_visibility(args.visibility.into());

    // `Pass 308.1`: the colours land on the SPEC, so the `/MK` dictionary and
    // the `/AP` artwork are written from one value. Parsed before anything is
    // staged, so a mistyped colour costs nothing.
    let chrome = match parse_creation_chrome(args.background, args.border_color) {
        Ok(c) => c,
        Err(code) => return code,
    };
    if let Some(c) = chrome.background {
        spec = spec.with_background(c);
    }
    if let Some(c) = chrome.border_color {
        spec = spec.with_border_color(c);
    }
    if args.combo {
        spec = spec.as_combo(args.editable);
    } else {
        // Carried through UNCHANGED rather than silently cleared, so the core
        // refuses `--editable` without `--combo` instead of the CLI quietly
        // dropping a flag the operator asked for.
        spec.editable = args.editable;
    }
    // R105: exactly one of the two must have been chosen. `clap`'s
    // `conflicts_with` rules out BOTH; only "neither" can reach here, and it
    // is refused rather than defaulted.
    spec = match (args.tooltip, args.no_tooltip) {
        (Some(t), _) => spec.with_tooltip(t),
        (None, true) => spec.declining_tooltip(),
        (None, false) => {
            eprintln!(
                "pdfcer: {}: decide about the accessibility name — pass --tooltip <text>, or --no-tooltip to decline it. It is what a screen reader announces for this field, so it is never defaulted silently.",
                args.input.display()
            );
            return exit::EDIT_REFUSED;
        }
    };

    // Applied to the SPEC before authoring, so everything downstream
    // — the merge check, the appearance build, the undo entry — sees
    // one fully-formed request rather than a partially-defaulted one.
    let defaults = match read_defaults(&session, args.input, args.defaults_from) {
        Ok(d) => d,
        Err(code) => return code,
    };
    let applied = defaults
        .map(|d| spec.apply_defaults(&d))
        .unwrap_or_default();
    let authored = match session.add_choice_field(&spec) {
        Ok(outcome) => outcome,
        Err(err) => return report_edit_error(args.input, &err),
    };
    let field_id = authored.field_id;
    // R4 + decision 020 §3.4.3/§3.5.3: everything pdfcer knows and the
    // operator cannot see is said at the moment it happens, not left to be
    // discovered later.
    // Folded into the SAME disclosure struct the core produced, not
    // reported alongside it: one channel, so `any()` still answers for
    // everything and a caller gating on it cannot miss half the facts.
    let mut disclosures = authored.disclosures;
    disclosures.defaults_type_mismatch = applied.type_mismatch;
    disclosures.defaults_on_state_ambiguous = applied.on_state_ambiguous;
    report_field_disclosures(args.name, disclosures);
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
        "add-choice-field {} name={:?} page={} rect={},{},{},{} options={} no_options={} combo={} editable={} multi_select={} sort={} field={} {} merged={} tagged={} struct_tabs={} tooltip_declined={} background={} border_color={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.name,
        args.page,
        rect.llx,
        rect.lly,
        rect.urx,
        rect.ury,
        // The SPEC's count, not the argument count. `--defaults-from` can
        // fill this list, and a summary reading `options=0` beside a file
        // carrying three of them is the shape where a wrong number sits next
        // to a right one and nobody notices.
        spec.options.len(),
        u32::from(authored.disclosures.has_no_options),
        u32::from(args.combo),
        u32::from(args.editable),
        u32::from(args.multi_select),
        u32::from(args.sort),
        field_id.num,
        field_id.generation,
        u32::from(authored.merged),
        u32::from(authored.disclosures.tagged_document),
        u32::from(authored.disclosures.structure_tab_order),
        u32::from(authored.disclosures.tooltip_declined),
        mk_colour_token(spec.chrome.background),
        mk_colour_token(spec.chrome.border_color),
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

/// `add-push-button` — author a new push button (§12.7.4.2.2).
///
/// ## Contract
///
/// - Emits one `add-push-button …` line with the usual save-report fields,
///   then defers the exit code to [`finish_edit`].
/// - Every refusal — XFA present, a name already used by a different field
///   type or by a grouping node, a degenerate rectangle, an empty name, a
///   page out of range, an undecided accessibility name — goes through
///   [`report_edit_error`] (or the R105 branch below) BEFORE any mutation.
/// - `--page` is 1-BASED here and 0-based in the core call.
/// - **`inert=1` on every successful run.** The machine-readable line
///   carries the fact as a field and not only as a stderr sentence, so a
///   script that captures stdout and discards stderr still learns that the
///   button it just made does nothing. This is the one creation verb whose
///   success has a caveat that is true 100% of the time, and a caveat only
///   ever delivered on the human channel is one that automation cannot see.
pub(crate) fn cmd_add_push_button(args: &AddPushButtonArgs<'_>) -> u8 {
    let (page_index, rect) = match parse_page_and_rect(args.input, args.page, args.rect) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let mut spec = pdfcer_core::edit::NewPushButton::new(page_index, args.name, rect, args.caption)
        .with_flags(args.read_only)
        .with_border(args.border.into(), args.border_width)
        .with_visibility(args.visibility.into());

    // `Pass 308.1`: the colours land on the SPEC, so the `/MK` dictionary and
    // the `/AP` artwork are written from one value. Parsed before anything is
    // staged, so a mistyped colour costs nothing.
    let chrome = match parse_creation_chrome(args.background, args.border_color) {
        Ok(c) => c,
        Err(code) => return code,
    };
    if let Some(c) = chrome.background {
        spec = spec.with_background(c);
    }
    if let Some(c) = chrome.border_color {
        spec = spec.with_border_color(c);
    }
    // R105: exactly one of the two must have been chosen. `clap`'s
    // `conflicts_with` rules out BOTH; only "neither" can reach here, and it
    // is refused rather than defaulted.
    spec = match (args.tooltip, args.no_tooltip) {
        (Some(t), _) => spec.with_tooltip(t),
        (None, true) => spec.declining_tooltip(),
        (None, false) => {
            eprintln!(
                "pdfcer: {}: decide about the accessibility name — pass --tooltip <text>, or --no-tooltip to decline it. It is what a screen reader announces for this field, so it is never defaulted silently.",
                args.input.display()
            );
            return exit::EDIT_REFUSED;
        }
    };

    // Applied to the SPEC before authoring, so everything downstream sees one
    // fully-formed request rather than a partially-defaulted one — and so the
    // empty-caption disclosure is computed against the caption that actually
    // lands, not against the argument.
    let defaults = match read_defaults(&session, args.input, args.defaults_from) {
        Ok(d) => d,
        Err(code) => return code,
    };
    let applied = defaults
        .map(|d| spec.apply_defaults(&d))
        .unwrap_or_default();
    let authored = match session.add_push_button(&spec) {
        Ok(outcome) => outcome,
        Err(err) => return report_edit_error(args.input, &err),
    };
    let field_id = authored.field_id;
    // Folded into the SAME disclosure struct the core produced, not reported
    // alongside it: one channel, so `any()` still answers for everything.
    let mut disclosures = authored.disclosures;
    disclosures.defaults_type_mismatch = applied.type_mismatch;
    disclosures.defaults_on_state_ambiguous = applied.on_state_ambiguous;
    report_field_disclosures(args.name, disclosures);
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
        "add-push-button {} name={:?} page={} rect={},{},{},{} caption={:?} no_caption={} inert={} read_only={} field={} {} merged={} tagged={} struct_tabs={} tooltip_declined={} background={} border_color={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.name,
        args.page,
        rect.llx,
        rect.lly,
        rect.urx,
        rect.ury,
        // The SPEC's caption, not the argument. `--defaults-from` can fill
        // it, and a summary printing the empty argument beside a file
        // carrying a copied caption is the wrong-number-next-to-a-right-one
        // shape the choice verb's `options=` count already had once.
        spec.caption,
        u32::from(authored.disclosures.push_button_no_caption),
        u32::from(authored.disclosures.push_button_inert),
        u32::from(args.read_only),
        field_id.num,
        field_id.generation,
        u32::from(authored.merged),
        u32::from(authored.disclosures.tagged_document),
        u32::from(authored.disclosures.structure_tab_order),
        u32::from(authored.disclosures.tooltip_declined),
        mk_colour_token(spec.chrome.background),
        mk_colour_token(spec.chrome.border_color),
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

/// Read a `--defaults-from` template, or `None` when the flag was absent.
///
/// Separated from the four creation commands so the lookup, the
/// field-not-found refusal and the "flag absent" case are decided once. A
/// template that does not exist is an ERROR, not an empty default: the
/// operator named a field, and silently proceeding with nothing copied would
/// be indistinguishable from a successful copy of a field that has no
/// copyable properties.
pub(crate) fn read_defaults(
    session: &pdfcer_core::edit::EditSession,
    input: &Path,
    from: Option<&str>,
) -> Result<Option<pdfcer_core::edit::FieldDefaults>, u8> {
    match from {
        None => Ok(None),
        Some(name) => match session.field_defaults(name) {
            Ok(defaults) => Ok(Some(defaults)),
            Err(err) => Err(report_edit_error(input, &err)),
        },
    }
}

/// Print every disclosure a field-creation call owes the operator.
///
/// # Why this is one function rather than three copies
///
/// Three of these are things pdfcer KNOWS and the operator cannot see by
/// looking at the result: a tagged document whose tag tree the new field is
/// absent from, a page whose structure tab order gives the new field no tab
/// position at all, and a declined accessibility name. None is an error —
/// each is a true statement about a document created exactly as asked — and
/// none is discoverable after the fact.
///
/// Copied per verb, the third copy is where one of them goes missing, and it
/// would go missing SILENTLY: a disclosure that is never printed looks
/// exactly like a disclosure that did not apply.
pub(crate) fn report_field_disclosures(name: &str, d: pdfcer_core::edit::FieldAuthorDisclosures) {
    if d.tooltip_declined {
        eprintln!(
            "pdfcer: field {name:?}: no accessibility name (tooltip) was set, as requested — screen readers will announce the field's name instead"
        );
    }
    if d.tagged_document {
        eprintln!(
            "pdfcer: field {name:?}: this document is tagged (/StructTreeRoot), and the new field is NOT in its structure tree — pdfcer does not write structure elements"
        );
    }
    if d.structure_tab_order {
        eprintln!(
            "pdfcer: field {name:?}: this page uses structure tab order (/Tabs /S) and the new field is untagged, so its tab position is UNDEFINED — not last. Set an explicit tab order, or use row/column order for this page."
        );
    }
    if d.has_no_options {
        eprintln!(
            "pdfcer: field {name:?}: this choice field has no options and cannot be filled until options are added"
        );
    }
    if d.group_flags_ignored {
        eprintln!(
            "pdfcer: field {name:?}: this member joined an EXISTING radio group, so the group's own --no-toggle-to-off / --radios-in-unison settings apply and the ones passed here were ignored. Those flags live on the field, so honouring them now would have changed how the members already in the group behave."
        );
    }
    if d.defaults_type_mismatch {
        eprintln!(
            "pdfcer: field {name:?}: --defaults-from copied NOTHING. Every property the field types share is a yes/no flag, and those are never copied (a --flag cannot express 'off'), so the only copyable properties are type-specific: --max-len for text, the option list for choice, the on-state for a check box, the caption for a push button. A radio template has nothing to copy at all."
        );
    }
    if d.defaults_on_state_ambiguous {
        eprintln!(
            "pdfcer: field {name:?}: the --defaults-from check box has widgets with DIFFERENT on-state names, and the first one was used. A check box normally uses one on-state everywhere it appears, so a template that does not is worth a look."
        );
    }
    if d.push_button_inert {
        eprintln!(
            "pdfcer: field {name:?}: this push button has NO ACTION and does nothing when clicked. Creation never attaches one, so what was created is a valid, inert button, not a working submit or reset. Give it behaviour with `set-button-action --name {name} --reset` (or --submit, --goto-page, --named, --uri)."
        );
    }
    if d.push_button_no_caption {
        eprintln!(
            "pdfcer: field {name:?}: this push button has an EMPTY caption and will render as a blank plate. Pass --caption <text> if that was not intended."
        );
    }
}

/// `add-text-field` — author a new text form field.
///
/// ## Contract
///
/// - Emits one `add-text-field …` line with the usual save-report fields,
///   then defers the exit code to [`finish_edit`].
/// - Every refusal — XFA present, a name already used by a different field
///   type, a degenerate rectangle, an empty name, a page out of range —
///   goes through [`report_edit_error`] BEFORE any mutation, with the same
///   message and exit code the GUI will surface.
/// - `--page` is 1-BASED here and 0-based in the core call, matching every
///   other page-taking subcommand in this CLI.
/// - Every disclosure the core reports is printed by
///   [`report_field_disclosures`], so a fact stated by one authoring verb
///   cannot be silently dropped by another.
pub(crate) fn cmd_add_text_field(args: &AddTextFieldArgs<'_>) -> u8 {
    let (page_index, rect) = match parse_page_and_rect(args.input, args.page, args.rect) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let (llx, lly, urx, ury) = (rect.llx, rect.lly, rect.urx, rect.ury);

    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let mut spec = pdfcer_core::edit::NewTextField::new(page_index, args.name, rect)
        .with_password(args.password)
        .with_comb(args.comb)
        .with_border(args.border.into(), args.border_width)
        .with_visibility(args.visibility.into())
        .with_flags(args.multiline, args.read_only, args.required);

    // `Pass 308.1`: the colours land on the SPEC, so the `/MK` dictionary and
    // the `/AP` artwork are written from one value. Parsed before anything is
    // staged, so a mistyped colour costs nothing.
    let chrome = match parse_creation_chrome(args.background, args.border_color) {
        Ok(c) => c,
        Err(code) => return code,
    };
    if let Some(c) = chrome.background {
        spec = spec.with_background(c);
    }
    if let Some(c) = chrome.border_color {
        spec = spec.with_border_color(c);
    }
    if let Some(v) = args.value {
        spec = spec.with_value(v);
    }
    if let Some(m) = args.max_len {
        spec = spec.with_max_len(m);
    }
    // R105: exactly one of the two must have been chosen. `clap`'s
    // `conflicts_with` rules out BOTH; only "neither" can reach here, and it
    // is refused rather than defaulted.
    spec = match (args.tooltip, args.no_tooltip) {
        (Some(t), _) => spec.with_tooltip(t),
        (None, true) => spec.declining_tooltip(),
        (None, false) => {
            eprintln!(
                "pdfcer: {}: decide about the accessibility name — pass --tooltip <text>, or --no-tooltip to decline it. It is what a screen reader announces for this field, so it is never defaulted silently.",
                args.input.display()
            );
            return exit::EDIT_REFUSED;
        }
    };

    // Applied to the SPEC before authoring, so everything downstream
    // — the merge check, the appearance build, the undo entry — sees
    // one fully-formed request rather than a partially-defaulted one.
    let defaults = match read_defaults(&session, args.input, args.defaults_from) {
        Ok(d) => d,
        Err(code) => return code,
    };
    let applied = defaults
        .map(|d| spec.apply_defaults(&d))
        .unwrap_or_default();
    let authored = match session.add_text_field(&spec) {
        Ok(o) => o,
        Err(err) => return report_edit_error(args.input, &err),
    };
    let field_id = authored.field_id;
    // Folded into the SAME disclosure struct the core produced, not
    // reported alongside it: one channel, so `any()` still answers for
    // everything and a caller gating on it cannot miss half the facts.
    let mut disclosures = authored.disclosures;
    disclosures.defaults_type_mismatch = applied.type_mismatch;
    disclosures.defaults_on_state_ambiguous = applied.on_state_ambiguous;
    report_field_disclosures(args.name, disclosures);
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
        "add-text-field {} name={:?} page={} rect={},{},{},{} field={} {} merged={} tagged={} struct_tabs={} tooltip_declined={} background={} border_color={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.name,
        args.page,
        llx,
        lly,
        urx,
        ury,
        field_id.num,
        field_id.generation,
        u32::from(authored.merged),
        u32::from(authored.disclosures.tagged_document),
        u32::from(authored.disclosures.structure_tab_order),
        u32::from(authored.disclosures.tooltip_declined),
        mk_colour_token(spec.chrome.background),
        mk_colour_token(spec.chrome.border_color),
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

/// Borrowed argument bundle for [`cmd_paste_field`].
///
/// A bundle rather than eleven parameters, matching every other authoring
/// handler in this file: clippy's `too_many_arguments` bites at eight, and a
/// struct keeps the dispatch arm readable.
pub(crate) struct PasteFieldArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) clip: &'a Path,
    pub(crate) page: usize,
    pub(crate) rect: &'a str,
    pub(crate) as_new: Option<&'a str>,
    pub(crate) as_widget_of: Option<&'a str>,
    pub(crate) copy_value: bool,
    pub(crate) copy_actions: bool,
    pub(crate) tooltip: Option<&'a str>,
    pub(crate) carry_tooltip: bool,
    pub(crate) no_tooltip: bool,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// Borrowed argument bundle for [`cmd_copy_field`].
pub(crate) struct CopyFieldArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) name: &'a str,
    pub(crate) output: &'a Path,
    pub(crate) cut: Option<&'a Path>,
    pub(crate) mode: SaveMode,
}

/// `copy-field` — write a field onto a portable clip file, and with `--cut`
/// remove it from the document as well.
pub(crate) fn cmd_copy_field(args: &CopyFieldArgs<'_>) -> u8 {
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // The CUT path goes through `cut_field`, which is copy-then-delete as ONE
    // undo entry -- not two calls here, for the reason `cut_field`'s own doc
    // gives: two commands is two undos for one gesture.
    let (clip, deletion) = if args.cut.is_some() {
        match session.cut_field(args.name) {
            Ok(cut) => (cut.clip, Some(cut.deletion)),
            Err(err) => return report_edit_error(args.input, &err),
        }
    } else {
        match session.copy_field(args.name) {
            Ok(clip) => (clip, None),
            Err(err) => return report_edit_error(args.input, &err),
        }
    };

    let bytes = clip.to_bytes();
    if let Err(err) = std::fs::write(args.output, &bytes) {
        eprintln!("pdfcer: {}: {err}", args.output.display());
        return exit::IO_ERROR;
    }

    let verb = if args.cut.is_some() {
        "cut-field"
    } else {
        "copy-field"
    };
    print_field_clip_line(verb, Some(args.input), &clip, args.output, bytes.len());

    let Some(cut_output) = args.cut else {
        return exit::SUCCESS;
    };
    // What leaving cost, on stderr with the other disclosures. `/V` pointing
    // at a state no remaining widget could show, and grouping nodes pruned
    // because they became childless, are both invisible in the saved file.
    if let Some(deletion) = deletion {
        if deletion.selection_cleared {
            eprintln!(
                "pdfcer: {}: the cut field held the value, so /V was cleared to /Off on what remains.",
                args.input.display()
            );
        }
        if deletion.emptied_parents > 0 {
            eprintln!(
                "pdfcer: {}: {} grouping node(s) became childless and were pruned with the field -- a named node with nothing under it still occupies its slot in the field-name space.",
                args.input.display(),
                deletion.emptied_parents
            );
        }
        // `Pass 184.0`. A cut removes the field from THIS document, so any
        // button here that named it is now pointing at nothing -- and the
        // pasted copy in the other document has no button naming it either.
        if deletion.action_targets_orphaned > 0 {
            eprintln!(
                "pdfcer: {}: {} button-action target(s) still name the field you cut and now point at nothing. pdfcer does not repair these; the buttons stayed behind and the field did not.",
                args.input.display(),
                deletion.action_targets_orphaned
            );
        }
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        cut_output,
        args.mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "  cut=1 cut_out={} mode={} changed={} objects={} appended={} out_bytes={}",
        cut_output.display(),
        args.mode.name(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
    );
    finish_edit(args.input, &outcome)
}

/// `inspect-field-clip` — say what a clip carries, without pasting it.
pub(crate) fn cmd_inspect_field_clip(path: &Path) -> u8 {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", path.display());
            return exit::IO_ERROR;
        }
    };
    let clip = match pdfcer_core::formclip::FieldClip::from_bytes(&bytes) {
        Ok(clip) => clip,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", path.display());
            return exit::EDIT_REFUSED;
        }
    };
    print_field_clip_line("inspect-field-clip", None, &clip, path, bytes.len());
    exit::SUCCESS
}

/// The one-line summary both clip subcommands print.
///
/// Same shape for both so a script can parse one format: the verb, the facts,
/// then `-> <file>`. Every counted fact is one a caller might branch on
/// before stamping the clip across a directory — `actions=1` in particular,
/// because a carried calculation is the thing that is invisible afterwards.
pub(crate) fn print_field_clip_line(
    verb: &str,
    input: Option<&Path>,
    clip: &pdfcer_core::formclip::FieldClip,
    file: &Path,
    bytes: usize,
) {
    let source = input.map_or_else(String::new, |p| format!("{} ", p.display()));
    let bbox = clip.bbox().map_or_else(
        || "-".to_owned(),
        |r| format!("{},{},{},{}", r.llx, r.lly, r.urx, r.ury),
    );
    println!(
        "{verb} {source}field={:?} type={} button={} widgets={} value={} actions={} calc={} tooltip={} font={} bbox={bbox} objects={} -> {}; bytes={bytes}",
        clip.source_name(),
        clip.field_type().map_or("-", field_type_token),
        clip.button_kind().map_or("-", button_kind_token),
        clip.widget_count(),
        u32::from(clip.carries_value()),
        u32::from(clip.carries_actions()),
        u32::from(clip.carries_calculation()),
        clip.tooltip().map_or_else(
            || "-".to_owned(),
            |t| format!("{:?}", String::from_utf8_lossy(t))
        ),
        clip.carried_font().map_or_else(
            || "-".to_owned(),
            |f| String::from_utf8_lossy(f).into_owned()
        ),
        clip.object_count(),
        file.display(),
    );
}

/// The `inspect-field-clip` token for a field type (`/FT` spelling).
pub(crate) const fn field_type_token(ft: pdfcer_core::forms::FieldType) -> &'static str {
    match ft {
        pdfcer_core::forms::FieldType::Button => "Btn",
        pdfcer_core::forms::FieldType::Text => "Tx",
        pdfcer_core::forms::FieldType::Choice => "Ch",
        pdfcer_core::forms::FieldType::Signature => "Sig",
    }
}

/// The `inspect-field-clip` token for a button kind.
pub(crate) const fn button_kind_token(kind: pdfcer_core::forms::ButtonKind) -> &'static str {
    match kind {
        pdfcer_core::forms::ButtonKind::Push => "push",
        pdfcer_core::forms::ButtonKind::Check => "check",
        pdfcer_core::forms::ButtonKind::Radio => "radio",
    }
}

/// `paste-field` — plant a copied field, under one of the two policies.
pub(crate) fn cmd_paste_field(args: &PasteFieldArgs<'_>) -> u8 {
    use pdfcer_core::formclip::{FieldClip, FieldPastePolicy, PasteTooltip};

    let (page_index, rect) = match parse_page_and_rect(args.input, args.page, args.rect) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let bytes = match std::fs::read(args.clip) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.clip.display());
            return exit::IO_ERROR;
        }
    };
    let clip = match FieldClip::from_bytes(&bytes) {
        Ok(clip) => clip,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.clip.display());
            return exit::EDIT_REFUSED;
        }
    };

    // THE POLICY. `clap`'s `conflicts_with` rules out BOTH; only "neither"
    // can reach here, and neither is not a default — the two pastes produce
    // documents that differ in a way nothing on the page shows.
    let policy = match (args.as_new, args.as_widget_of) {
        (Some(name), _) => {
            // R105: exactly one of the three tooltip answers must have been
            // chosen. `clap` rules out combinations; only "none" gets here.
            let tooltip = match (args.tooltip, args.carry_tooltip, args.no_tooltip) {
                (Some(t), _, _) => PasteTooltip::Text(t.to_owned()),
                (None, true, _) => PasteTooltip::Carry,
                (None, false, true) => PasteTooltip::Declined,
                (None, false, false) => {
                    eprintln!(
                        "pdfcer: {}: decide about the accessibility name — pass --tooltip <text>, --carry-tooltip to reuse the copied field's, or --no-tooltip to decline it. It is what a screen reader announces for a form field, so it is never defaulted silently (R105).",
                        args.input.display()
                    );
                    return exit::EDIT_REFUSED;
                }
            };
            FieldPastePolicy::NewField {
                name: name.to_owned(),
                tooltip,
                copy_value: args.copy_value,
                copy_actions: args.copy_actions,
            }
        }
        (None, Some(existing)) => FieldPastePolicy::AdditionalWidget {
            existing: existing.to_owned(),
        },
        (None, None) => {
            eprintln!(
                "pdfcer: {}: choose which paste this is — --as-new <NAME> for an independent field, or --as-widget-of <NAME> for another view of a field already here. The two produce documents that differ only in whether the two places share a value, so there is no safe default.",
                args.input.display()
            );
            return exit::EDIT_REFUSED;
        }
    };

    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let outcome = match session.paste_field(&clip, page_index, rect, &policy) {
        Ok(outcome) => outcome,
        Err(err) => return report_edit_error(args.input, &err),
    };
    // Off-canvas by construction: stderr, never stdout, so a script's parsed
    // line is unaffected and a human still sees every one of them.
    for disclosure in &outcome.disclosures {
        eprintln!("pdfcer: {}: {disclosure}", args.input.display());
    }

    let saved = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };
    let r = &saved.report;
    println!(
        "paste-field {} clip={} source_field={:?} policy={} page={} rect={},{},{},{} field={} {} widgets={} created={} merged={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.clip.display(),
        clip.source_name(),
        if outcome.created {
            "new-field"
        } else {
            "additional-widget"
        },
        args.page,
        rect.llx,
        rect.lly,
        rect.urx,
        rect.ury,
        outcome.field_id.num,
        outcome.field_id.generation,
        outcome.widget_ids.len(),
        u32::from(outcome.created),
        u32::from(outcome.merged),
        args.mode.name(),
        args.output.display(),
        saved.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(saved.undo_verified),
        u32::from(saved.undo_identical),
    );
    finish_edit(args.input, &saved)
}
