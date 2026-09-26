use super::*;

/// The style flags `set-markup-style` accepts, bundled so
/// [`cmd_set_markup_style`] stays inside clippy's seven-argument limit.
///
/// Strings rather than parsed values because each one is tri-state —
/// absent, a value, or the literal `none` — and parsing them at the same
/// place keeps the three "what does `none` mean here" answers in one
/// readable block instead of three clap attributes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MarkupStyleArg<'a> {
    /// `--color RRGGBB|none`.
    pub(crate) color: Option<&'a str>,
    /// `--interior RRGGBB|none`.
    pub(crate) interior: Option<&'a str>,
    /// `--width PT`.
    pub(crate) width: Option<f64>,
    /// `--opacity 0.0-1.0|none`.
    pub(crate) opacity: Option<&'a str>,
    /// `--dash ON,OFF,...|solid`.
    pub(crate) dash: Option<&'a str>,
}

/// Parse a `--dash ON,OFF,...|solid` flag into a
/// [`StyleEdit`](pdfcer_core::edit::StyleEdit) over a
/// [`BorderDash`](pdfcer_core::annot_author::BorderDash).
///
/// Three outcomes, and the three-way split is the point of the flag:
///
/// * absent → `None` → **leave the border style alone**, preserving a dash
///   the file already carries. This is the case that used to lose the
///   operator's dash silently (`Pass 258.0`).
/// * `solid` → `Some(StyleEdit::Clear)` → remove the dash.
/// * `4,2` → `Some(StyleEdit::Set(..))` → dash with that pattern.
///
/// # Errors
///
/// A message naming the flag, for the caller to print. §8.4.3.6's
/// constraints are enforced here rather than deeper down so the operator is
/// told which value was wrong while the number they typed is still in view.
pub(crate) fn parse_dash_edit(
    value: Option<&str>,
) -> Result<Option<pdfcer_core::edit::StyleEdit<pdfcer_core::annot_author::BorderDash>>, String> {
    use pdfcer_core::annot_author::BorderDash;
    use pdfcer_core::edit::StyleEdit;

    let Some(raw) = value else {
        return Ok(None);
    };
    if raw.eq_ignore_ascii_case("solid") {
        return Ok(Some(StyleEdit::Clear));
    }
    let mut pattern = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        match part.parse::<f64>() {
            Ok(v) => pattern.push(v),
            Err(_) => {
                return Err(format!(
                    "--dash: `{part}` is not a number; expected comma-separated point \
                     lengths like `4,2`, or `solid`"
                ));
            }
        }
    }
    BorderDash::new(pattern)
        .map(|d| Some(StyleEdit::Set(d)))
        .ok_or_else(|| {
            format!(
                "--dash: `{raw}` is not a usable dash pattern -- every length must be \
             non-negative and at least one must be greater than zero (ISO 32000-1 \
             8.4.3.6). Use `solid` to remove a dash."
            )
        })
}

/// Parse a `RRGGBB`-or-`none` colour flag into a
/// [`StyleEdit`](pdfcer_core::edit::StyleEdit).
///
/// `None` back means the flag was absent, i.e. leave the property alone.
/// The literal `none` becomes [`StyleEdit::Clear`], which is a real markup
/// style and not a way of spelling black — §12.5.6 has no transparent
/// value in a colour array, so "no border" IS an absent `/C`.
///
/// # Errors
///
/// A message naming the flag, for the caller to print.
pub(crate) fn parse_color_edit(
    flag: &str,
    value: Option<&str>,
) -> Result<Option<pdfcer_core::edit::StyleEdit<pdfcer_core::annot_author::Color>>, String> {
    use pdfcer_core::edit::StyleEdit;
    match value {
        None => Ok(None),
        Some(v) if v.eq_ignore_ascii_case("none") => Ok(Some(StyleEdit::Clear)),
        Some(v) => parse_color(v)
            .map(|c| Some(StyleEdit::Set(c)))
            .map_err(|e| format!("--{flag}: {e}")),
    }
}

/// Implement `pdfcer set-markup-note` (`Pass 154.0`).
///
/// # Why the previous note is PRINTED and not merely discarded
///
/// The invocation IS the commit here — no session, no undo — so rule 4's
/// disclosure has to happen on the way past. And a note is content the
/// operator cannot recover by looking: a restyled shape still shows its
/// geometry, overwritten words leave nothing on the page. Printing them is
/// the only chance to keep them.
pub(crate) fn cmd_set_markup_note(
    input: &Path,
    at: (usize, usize),
    // The note being written: text, author, date. Bundled because they are
    // one thing — the note — and splitting them across three positional
    // parameters is what makes a call site transposable.
    note: (Option<&str>, Option<&str>, Option<&str>),
    clear: bool,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    use pdfcer_core::edit::MarkupNote;

    let (page, index) = at;
    let (note, author, date) = note;
    if page == 0 {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    if !clear && note.is_none() {
        eprintln!(
            "pdfcer: {}: set-markup-note needs --note TEXT, or --clear to remove the note",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }

    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let annot_id = {
        let slots = match session.page_slots() {
            Ok(slots) => slots,
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", input.display());
                return exit::RUNTIME_ERROR;
            }
        };
        let Some(slot) = slots.get(page - 1) else {
            eprintln!(
                "pdfcer: {}: no page {page} — the document has {} page(s)",
                input.display(),
                slots.len()
            );
            return exit::RUNTIME_ERROR;
        };
        let annots = pdfcer_core::annot::page_annotations(&session.graph(), slot.id);
        let Some(annot) = annots.get(index) else {
            eprintln!(
                "pdfcer: {}: page {page} has no annotation at index {index} — it has {}",
                input.display(),
                annots.len()
            );
            return exit::RUNTIME_ERROR;
        };
        let Some(id) = annot.id else {
            eprintln!(
                "pdfcer: {}: page {page} index {index} is a direct dictionary inside /Annots, not an indirect object — it has no identity to write a note onto",
                input.display()
            );
            return exit::EDIT_REFUSED;
        };
        id
    };

    let outcome = if clear {
        session.clear_markup_note(annot_id)
    } else {
        let mut n = MarkupNote::new(note.unwrap_or_default());
        if let Some(a) = author {
            n = n.by(a);
        }
        if let Some(d) = date {
            n = n.at(d);
        }
        session.set_markup_note(annot_id, &n)
    };
    let change = match outcome {
        Ok(c) => c,
        Err(err) => return report_edit_error(input, &err),
    };

    // The disclosure, before the outcome, because destroyed words are the
    // thing the operator cannot get back by looking at the page.
    if let Some(previous) = &change.replaced {
        eprintln!(
            "pdfcer: {}: this REPLACED an existing note, whose text was: {previous:?}. Those words are not on the page and nothing in the saved file shows they were ever there — keep them if you may want them.",
            input.display()
        );
        if let Some(who) = &change.replaced_author {
            eprintln!("pdfcer: {}: its author was {who:?}.", input.display());
        }
    }
    if !clear && author.is_none() && change.replaced_author.is_some() {
        eprintln!(
            "pdfcer: {}: no --note-author was given, so the existing author was LEFT AS IT WAS rather than cleared. Correcting a comment does not un-sign it.",
            input.display()
        );
    }

    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(o) => o,
        Err(code) => return code,
    };

    println!(
        "set-markup-note {} page {page} index {index} -> {}",
        input.display(),
        output.display()
    );
    println!(
        "  subtype={} keys_written={} replaced={}",
        change.subtype,
        if change.keys_written.is_empty() {
            "none".to_owned()
        } else {
            change.keys_written.join(",")
        },
        match &change.replaced {
            Some(t) => format!("{} character(s)", t.chars().count()),
            None => "nothing".to_owned(),
        }
    );
    // Rule 11: the CLI PRINTS what the GUI would disclose off-canvas.
    // Whether the picture moved with the words is the whole point of
    // `Pass 258.1` and is invisible in the exit code.
    println!(
        "  appearance={}",
        if change.appearance_rebaked {
            "re-baked from the new text"
        } else if change.subtype == "FreeText" {
            "left alone -- it is not one pdfcer authored, so the box still paints its previous words"
        } else {
            "unchanged (this subtype does not paint its /Contents)"
        }
    );
    // Rule 11 again, and this one is a DROP the operator did not ask for.
    // A stale `/RC` would have left the pop-up -- or, on a `/FreeText`, the
    // page itself -- showing the OLD comment while `/Contents` held the new
    // one. pdfcer cannot author rich text, so it removes the copy it can no
    // longer keep true, and says which keys went.
    if !change.rich_text_dropped.is_empty() {
        println!(
            "  rich_text_dropped={} (this annotation carried a rich-text copy of the same comment; \
             pdfcer cannot author rich text, so it removed the copy rather than leave it saying \
             the old words)",
            change.rich_text_dropped.join(",")
        );
    }
    finish_edit(input, &saved)
}

/// `--icon` for `set-text-annot-style`: the seven §12.5.6.4 names pdfcer
/// draws an appearance for.
///
/// The clause's set is OPEN — "Additional names may be supported as well" —
/// so this enum is what pdfcer can AUTHOR, not what it can read. A file's
/// own icon name reaches a caller intact through
/// `Annotation::icon` and prints on `list-annotations`.
/// `--state` for `set-review-state`: Table 171's two vocabularies.
///
/// One flag rather than two, because `/StateModel` is *"Required if `State`
/// is present"* and is derivable from the value — offering it separately
/// would let an operator pair `accepted` with the `Marked` model, which is
/// the single non-conforming combination.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ReviewStateArg {
    /// Review model.
    Accepted,
    /// Review model.
    Rejected,
    /// Review model. British spelling, as the standard writes it.
    Cancelled,
    /// Review model.
    Completed,
    /// Review model — an explicit "no status", which is a written value and
    /// not the same as leaving the key off.
    None,
    /// Marked model.
    Marked,
    /// Marked model.
    Unmarked,
}

impl ReviewStateArg {
    /// The core review state this word names.
    pub(crate) fn to_core(self) -> pdfcer_core::edit::ReviewState {
        use pdfcer_core::edit::ReviewState as R;
        match self {
            Self::Accepted => R::Accepted,
            Self::Rejected => R::Rejected,
            Self::Cancelled => R::Cancelled,
            Self::Completed => R::Completed,
            Self::None => R::None,
            Self::Marked => R::Marked,
            Self::Unmarked => R::Unmarked,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum StickyIconArg {
    Comment,
    Key,
    Note,
    Help,
    NewParagraph,
    Paragraph,
    Insert,
}

impl StickyIconArg {
    /// The core sticky-note icon this word names.
    pub(crate) fn to_core(self) -> pdfcer_core::annot_author::StickyIcon {
        use pdfcer_core::annot_author::StickyIcon as I;
        match self {
            Self::Comment => I::Comment,
            Self::Key => I::Key,
            Self::Note => I::Note,
            Self::Help => I::Help,
            Self::NewParagraph => I::NewParagraph,
            Self::Paragraph => I::Paragraph,
            Self::Insert => I::Insert,
        }
    }
}

/// Resolve `--page`/`--index` to an annotation's object id.
///
/// One helper rather than a copy in each verb: the addressing convention is
/// `list-annotations`' output and a second spelling of it would drift.
pub(crate) fn resolve_annotation(
    session: &pdfcer_core::edit::EditSession,
    input: &Path,
    page: usize,
    index: usize,
) -> Result<pdfcer_core::object::ObjId, u8> {
    if page == 0 {
        eprintln!("pdfcer: --page is 1-based; 0 is not a page");
        return Err(exit::EDIT_REFUSED);
    }
    let slots = match session.page_slots() {
        Ok(slots) => slots,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return Err(exit::RUNTIME_ERROR);
        }
    };
    let Some(slot) = slots.get(page - 1) else {
        eprintln!(
            "pdfcer: {}: --page {page} is out of range (the document has {} page(s))",
            input.display(),
            slots.len()
        );
        return Err(exit::EDIT_REFUSED);
    };
    let annots = pdfcer_core::annot::page_annotations(&session.graph(), slot.id);
    let Some(annot) = annots.get(index) else {
        eprintln!(
            "pdfcer: {}: --index {index} is out of range (page {page} has {} annotation(s))",
            input.display(),
            annots.len()
        );
        return Err(exit::EDIT_REFUSED);
    };
    annot.id.ok_or_else(|| {
        eprintln!(
            "pdfcer: {}: that annotation is a direct object and has no identity to address",
            input.display()
        );
        exit::EDIT_REFUSED
    })
}

/// Implement `pdfcer set-text-annot-style`.
// One argument per flag the subcommand accepts, which is how every other
// command function in this file is shaped; bundling them into a struct would
// hide the mapping the `Command` match relies on.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_set_text_annot_style(
    input: &Path,
    page: usize,
    index: usize,
    icon: Option<StickyIconArg>,
    color: Option<&str>,
    font_size: Option<f64>,
    stamp_fit: Option<StampFitArg>,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    if icon.is_none() && color.is_none() && font_size.is_none() {
        eprintln!("pdfcer: nothing to change: pass --icon, --color, --font-size, or several");
        return exit::EDIT_REFUSED;
    }
    // `--stamp-fit` alone changes nothing: it says what to do about a box when
    // a NEW size no longer fits it, and without a new size there is no re-fit
    // to police. Said rather than ignored (`Pass 292.0`).
    if stamp_fit.is_some() && font_size.is_none() {
        eprintln!(
            "pdfcer: --stamp-fit only applies with --font-size: it decides what happens to the \
             box when the RESIZED label no longer fits it"
        );
        return exit::EDIT_REFUSED;
    }
    // `is_sign_positive` + `is_normal` rather than `!(s > 0.0)`: a NaN is
    // neither greater nor not-greater than zero, and clippy is right that the
    // negated comparison hides that.
    if font_size.is_some_and(|s| !s.is_finite() || s <= 0.0) {
        eprintln!("pdfcer: --font-size must be a positive number of points");
        return exit::EDIT_REFUSED;
    }
    let parsed_color = match color.map(parse_color) {
        None => None,
        Some(Ok(c)) => Some(c),
        Some(Err(msg)) => {
            eprintln!("pdfcer: --color: {msg}");
            return exit::EDIT_REFUSED;
        }
    };

    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let annot_id = match resolve_annotation(&session, input, page, index) {
        Ok(id) => id,
        Err(code) => return code,
    };

    let style = pdfcer_core::edit::TextAnnotStyle {
        icon: icon.map(StickyIconArg::to_core),
        color: parsed_color,
        font_size,
        stamp_fit: stamp_fit.map(StampFitArg::to_fit),
    };
    let change = match session.set_text_annot_style(annot_id, &style) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };

    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };

    println!(
        "set-text-annot-style {} page {page} index {index} -> {}",
        input.display(),
        output.display()
    );
    // What the re-bake DECIDED, before the machine-readable line (rule 11).
    if let Some(fit) = change.stamp_label_fit
        && fit.is_inference()
    {
        report_stamp_label_fit(input, fit);
    }
    if change.appearance_was_foreign {
        eprintln!(
            "pdfcer: {}: the previous appearance was NOT one pdfcer would have drawn, and \
             re-baking has replaced it. A stamp whose label pdfcer cannot read back is artwork \
             (Acrobat's custom stamps are), and the restyle could not leave it in place: the \
             change is invisible unless /AP moves.",
            input.display()
        );
    }
    println!(
        "  obj={} subtype={} icon_written={} color_written={} font_size_written={} \
rect={:.2},{:.2},{:.2},{:.2} was_foreign={} appearance={}",
        change.annot_id.num,
        change.subtype,
        u32::from(change.icon_written),
        u32::from(change.color_written),
        u32::from(change.font_size_written),
        change.rect_after.llx,
        change.rect_after.lly,
        change.rect_after.urx,
        change.rect_after.ury,
        u32::from(change.appearance_was_foreign),
        match change.appearance {
            pdfcer_core::edit::AppearanceWrite::InPlace(_) => "in-place",
            pdfcer_core::edit::AppearanceWrite::Created(_) => "created",
            pdfcer_core::edit::AppearanceWrite::CopiedOnWrite { .. } => "copied",
            // `AppearanceWrite` is #[non_exhaustive]; a future variant must
            // print SOMETHING rather than fail to compile a shell.
            _ => "other",
        },
    );
    finish_edit(input, &saved)
}

/// Implement `pdfcer set-review-state`.
pub(crate) fn cmd_set_review_state(
    input: &Path,
    page: usize,
    index: usize,
    // (state, author, /M date) -- bundled for the same reason
    // `cmd_add_reply` bundles its note triple: they are one operator
    // intention arriving as three flags, and splitting them out is what
    // pushes this past clippy's argument limit for no reader's benefit.
    status: (ReviewStateArg, &str, Option<&str>),
    output: &Path,
    mode: SaveMode,
) -> u8 {
    let (state, author, note_date) = status;
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let target_id = match resolve_annotation(&session, input, page, index) {
        Ok(id) => id,
        Err(code) => return code,
    };

    let added = match session.add_review_state(target_id, state.to_core(), author, note_date) {
        Ok(a) => a,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };

    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };

    println!(
        "set-review-state {} page {page} index {index} -> {}",
        input.display(),
        output.display()
    );
    // Rule 11. `attached_to` is the disclosure that matters: a star of
    // statuses all pointing at the comment renders identically to the
    // per-user chain 12.5.6.3 requires, so this is the only place the
    // difference is visible.
    println!(
        "  state_obj={} target_obj={} state={} model={} attached_to={} chain_depth={}",
        added.state_id.num,
        added.target_id.num,
        added.state.as_str(),
        added.state.model(),
        added.attached_to.num,
        added.chain_depth,
    );
    if added.attached_to != added.target_id {
        println!(
            "  note: this is not {author}'s first status on that comment, so it replies to their previous one rather than to the comment -- 12.5.6.3 requires the per-user chain."
        );
    }
    finish_edit(input, &saved)
}

/// Implement `pdfcer add-reply`.
pub(crate) fn cmd_add_reply(
    input: &Path,
    page: usize,
    index: usize,
    note: (&str, Option<&str>, Option<&str>),
    output: &Path,
    mode: SaveMode,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let parent_id = match resolve_annotation(&session, input, page, index) {
        Ok(id) => id,
        Err(code) => return code,
    };

    let mut built = pdfcer_core::edit::MarkupNote::new(note.0);
    if let Some(a) = note.1 {
        built = built.by(a);
    }
    if let Some(d) = note.2 {
        built = built.at(d);
    }

    let added = match session.add_reply(parent_id, &built) {
        Ok(a) => a,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };

    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };

    println!(
        "add-reply {} page {page} index {index} -> {}",
        input.display(),
        output.display()
    );
    // Rule 11. The two pop-up booleans are the disclosure pdfcer-gui asked
    // for by name: 12.5.6.14 makes a pop-up structural and a shell that
    // DRAWS them would otherwise discover a second window on a screenshot.
    println!(
        "  reply_obj={} parent_obj={} page={} parent_had_popup={} reply_has_popup={}",
        added.reply_id.num,
        added.parent_id.num,
        added.page_index + 1,
        u32::from(added.parent_had_popup),
        u32::from(added.reply_has_popup),
    );
    if added.reply_has_popup && !added.parent_had_popup {
        println!(
            "  note: the reply carries a /Popup and its parent did not, so this document now \
has a comment window it did not have before."
        );
    }
    finish_edit(input, &saved)
}

/// Implement `pdfcer set-annotation-open`.
///
/// Addressed by `--page` + `--index`, the exact pair `list-annotations`
/// prints, so the two commands compose — the same convention every other
/// annotation verb uses rather than a second one invented here.
pub(crate) fn cmd_set_annotation_open(
    input: &Path,
    page: usize,
    index: usize,
    open: bool,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    if page == 0 {
        eprintln!("pdfcer: --page is 1-based; 0 is not a page");
        return exit::EDIT_REFUSED;
    }
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let annot_id = {
        let slots = match session.page_slots() {
            Ok(slots) => slots,
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", input.display());
                return exit::RUNTIME_ERROR;
            }
        };
        let Some(slot) = slots.get(page - 1) else {
            eprintln!(
                "pdfcer: {}: --page {page} is out of range (the document has {} page(s))",
                input.display(),
                slots.len()
            );
            return exit::EDIT_REFUSED;
        };
        let annots = pdfcer_core::annot::page_annotations(&session.graph(), slot.id);
        let Some(annot) = annots.get(index) else {
            eprintln!(
                "pdfcer: {}: --index {index} is out of range (page {page} has {} annotation(s))",
                input.display(),
                annots.len()
            );
            return exit::EDIT_REFUSED;
        };
        match annot.id {
            Some(id) => id,
            None => {
                eprintln!(
                    "pdfcer: {}: that annotation is a direct object and has no identity to address",
                    input.display()
                );
                return exit::EDIT_REFUSED;
            }
        }
    };

    let change = match session.set_annotation_open(annot_id, open) {
        Ok(change) => change,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };

    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };

    println!(
        "set-annotation-open {} page {page} index {index} -> {}",
        input.display(),
        output.display()
    );
    // Rule 11: the CLI PRINTS what the GUI discloses off-canvas. WHICH of
    // the two objects moved is the whole answer here, and `was=` is the
    // three-state prior value -- `none` for a key the file never carried.
    println!(
        "  obj={} subtype={} open={} was={} annotation_written={} popup_written={}",
        change.annot_id.num,
        change.subtype,
        u32::from(change.open),
        match change.was {
            Some(true) => "1".to_owned(),
            Some(false) => "0".to_owned(),
            None => "none".to_owned(),
        },
        u32::from(change.annotation_written),
        u32::from(change.popup_written),
    );
    if !change.annotation_written && !change.popup_written {
        println!(
            "  nothing was written: a /{} carries no /Open of its own and this one has no \
/Popup companion, so it has no window to open. Not a refusal -- there was nowhere to put the \
state, and no undo entry was pushed.",
            change.subtype
        );
    }
    finish_edit(input, &saved)
}

/// `set-annotation-flags` — write an annotation's `/F` display flags
/// (ISO 32000-1 §12.5.3 Table 165).
///
/// # Why the whole word rather than per-bit toggles
///
/// Table 165's bits interact — `NoView` with `Print` means *prints but is not
/// on screen* — so the CLI takes the complete set the operator wants the
/// annotation to END with. A toggle-one-bit interface lets a sequence of
/// individually-sensible invocations build a state nobody chose, and on a
/// batch tool that sequence is a shell script nobody reads back.
///
/// # Rule 4 — the invocation IS the commit here
///
/// The before/after words are printed unasked, because `/F` is invisible: an
/// operator who hides an annotation sees nothing change and has no other way
/// to confirm it happened. The refusal for a widget names the verb that does
/// own a widget's visibility.
pub(crate) fn cmd_set_annotation_flags(
    input: &Path,
    at: (usize, usize),
    flags: pdfcer_core::annot::AnnotFlags,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    let (page, index) = at;
    if page == 0 {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let annot_id = {
        let slots = match session.page_slots() {
            Ok(s) => s,
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", input.display());
                return exit::RUNTIME_ERROR;
            }
        };
        let Some(slot) = slots.get(page - 1) else {
            eprintln!(
                "pdfcer: {}: no page {page} — the document has {} page(s)",
                input.display(),
                slots.len()
            );
            return exit::RUNTIME_ERROR;
        };
        let annots = pdfcer_core::annot::page_annotations(&session.graph(), slot.id);
        let Some(annot) = annots.get(index) else {
            eprintln!(
                "pdfcer: {}: page {page} has {} annotation(s); no index {index}",
                input.display(),
                annots.len()
            );
            return exit::RUNTIME_ERROR;
        };
        let Some(id) = annot.id else {
            eprintln!(
                "pdfcer: {}: page {page} index {index} is a direct dictionary inside /Annots, not an indirect object — it has no identity to edit",
                input.display()
            );
            return exit::EDIT_REFUSED;
        };
        id
    };

    let out = match session.set_annotation_flags(annot_id, flags) {
        Ok(o) => o,
        Err(err) => return report_edit_error(input, &err),
    };

    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(o) => o,
        Err(code) => return code,
    };

    println!(
        "set-annotation-flags {} page={page} index={index} -> {}",
        input.display(),
        output.display()
    );
    println!(
        "  subtype={} flags=0x{:X}->0x{:X}",
        out.subtype, out.before.0, out.after.0
    );
    // Rule 4: `/F` is INVISIBLE. An operator who hides an annotation sees
    // nothing change, so what pdfcer did is stated rather than left to be
    // inferred from a file that now looks emptier.
    let named = |f: pdfcer_core::annot::AnnotFlags| {
        let mut v: Vec<&str> = Vec::new();
        if f.invisible() {
            v.push("invisible");
        }
        if f.hidden() {
            v.push("hidden");
        }
        if f.print() {
            v.push("print");
        }
        if f.no_zoom() {
            v.push("no-zoom");
        }
        if f.no_rotate() {
            v.push("no-rotate");
        }
        if f.no_view() {
            v.push("no-view");
        }
        if f.locked() {
            v.push("locked");
        }
        if f.locked_contents() {
            v.push("locked-contents");
        }
        if v.is_empty() {
            "none".to_owned()
        } else {
            v.join(",")
        }
    };
    println!("  set={}", named(out.after));
    if out.after.locked() && !out.before.locked() {
        eprintln!(
            "pdfcer: {}: this annotation is now LOCKED — pdfcer's move, resize, rotate and restyle verbs will refuse it until the flag is cleared. Re-run without --locked to unlock.",
            input.display()
        );
    }
    if out.after.hidden() || out.after.invisible() {
        eprintln!(
            "pdfcer: {}: this annotation is now HIDDEN — it will not appear on screen or in print, and nothing on the page will show that it is there.",
            input.display()
        );
    }
    finish_edit(input, &saved)
}

/// Implement `pdfcer set-markup-style`.
///
/// ## Addressing, and why it matches `delete-annotation` exactly
///
/// `--page` + `--index`, resolved against the SAME `page_annotations`
/// walk `list-annotations` prints from. The core verb takes an `ObjId`,
/// because object identity is the only handle that stays correct while a
/// session mutates — but the operator's source of truth is the list
/// command's output, which deliberately does not print object numbers.
/// Resolving here is what makes "list it, then restyle that index"
/// reliable rather than approximately right.
///
/// ## What it prints
///
/// One machine-readable line carrying `subtype=`, how the appearance was
/// written (`ap=in-place|created|copied`), whether `/Rect` moved, and how
/// many properties the regeneration dropped — **plus a prose sentence on
/// stderr naming each dropped property**. The same twice-over the deletion
/// verb uses, for the same reason: the counter is for a script, the
/// sentence is for the person who would otherwise wonder later why their
/// cloudy border went straight.
///
/// Under rule 4 the CLI invocation IS the commit — there is no session and
/// no undo — so what pdfcer could not carry over is printed on the way
/// past rather than offered for confirmation.
pub(crate) fn cmd_set_markup_style(
    input: &Path,
    page: usize,
    index: usize,
    style: &MarkupStyleArg<'_>,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    use pdfcer_core::edit::{AppearanceWrite, DroppedProperty, MarkupStyle, StyleEdit};

    if page == 0 {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }

    // Parse the flags BEFORE opening the file: a mistyped colour should
    // not cost a parse of a large document.
    let (stroke, interior) = match (
        parse_color_edit("color", style.color),
        parse_color_edit("interior", style.interior),
    ) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(msg), _) | (_, Err(msg)) => {
            eprintln!("pdfcer: {msg}");
            return exit::EDIT_REFUSED;
        }
    };
    let opacity = match style.opacity {
        None => None,
        Some(v) if v.eq_ignore_ascii_case("none") => Some(StyleEdit::Clear),
        Some(v) => match v.parse::<f64>() {
            Ok(a) if (0.0..=1.0).contains(&a) => Some(StyleEdit::Set(a)),
            _ => {
                eprintln!("pdfcer: --opacity: `{v}` is not a number in 0.0..=1.0, or `none`");
                return exit::EDIT_REFUSED;
            }
        },
    };
    let dash = match parse_dash_edit(style.dash) {
        Ok(d) => d,
        Err(msg) => {
            eprintln!("pdfcer: {msg}");
            return exit::EDIT_REFUSED;
        }
    };
    let wanted = MarkupStyle {
        stroke,
        interior,
        width: style.width,
        opacity,
        endings: None,
        dash,
    };

    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // Resolved inside a block so the session borrow ends before the
    // mutable call below.
    let (annot_id, subtype) = {
        let slots = match session.page_slots() {
            Ok(slots) => slots,
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", input.display());
                return exit::RUNTIME_ERROR;
            }
        };
        let Some(slot) = slots.get(page - 1) else {
            eprintln!(
                "pdfcer: {}: no page {page} — the document has {} page(s)",
                input.display(),
                slots.len()
            );
            return exit::RUNTIME_ERROR;
        };
        let annots = pdfcer_core::annot::page_annotations(&session.graph(), slot.id);
        let Some(annot) = annots.get(index) else {
            eprintln!(
                "pdfcer: {}: page {page} has no annotation at index {index} — it has {} \
                 (indices 0..{})",
                input.display(),
                annots.len(),
                annots.len().saturating_sub(1)
            );
            return exit::RUNTIME_ERROR;
        };
        let Some(id) = annot.id else {
            eprintln!(
                "pdfcer: {}: page {page} index {index} is a direct dictionary inside /Annots, \
                 not an indirect object — it has no identity to restyle",
                input.display()
            );
            return exit::EDIT_REFUSED;
        };
        (id, String::from_utf8_lossy(&annot.subtype).into_owned())
    };

    let change = match session.set_markup_style(annot_id, &wanted) {
        Ok(change) => change,
        Err(err) => return report_edit_error(input, &err),
    };

    // The prose half of the disclosure. One sentence per dropped
    // property, each naming what the appearance no longer draws AND that
    // the dictionary key survived — because "it is still in the file" and
    // "it is still on the page" are different, and only the second is
    // what the operator sees.
    for dropped in &change.dropped {
        // NOTE the trailing \-continuations: without them a wrapped Rust
        // string literal carries every leading space of the next SOURCE
        // line into the message, which is how the first live run of this
        // command printed a sentence with ragged gaps in the middle of it.
        let note = match dropped {
            DroppedProperty::BorderEffect => {
                "the /BE cloudy border effect: the regenerated appearance draws a \
                 straight outline. The /BE key is still in the dictionary, but pdfcer \
                 paints from /AP."
            }
            DroppedProperty::BorderStyle => {
                "the /BS /S border style: pdfcer authors solid and DASHED strokes, so \
                 this one is beveled, inset or underline, which it does not."
            }
            DroppedProperty::DashPattern => {
                "the /BS /D dash array: it could not be used, so the regenerated stroke \
                 is continuous. A dash pdfcer can read is preserved, not dropped."
            }
            DroppedProperty::RectDifferences => {
                "the /RD rectangle differences: pdfcer draws from /Rect (or the \
                 explicit geometry keys) directly."
            }
            DroppedProperty::LineEnding => {
                "a /LE line ending outside None/OpenArrow/ClosedArrow: it regenerates \
                 as no ending."
            }
            _ => {
                "the previous appearance stream was NOT one pdfcer would have drawn \
                 from this annotation's own properties (compared byte for byte), so \
                 anything it drew beyond the shape — a shadow, a gradient, a raster, \
                 text — is not in the new one."
            }
        };
        eprintln!("pdfcer: {}: dropped — {note}", input.display());
    }
    let rect_moved = change.rect_before != Some(change.rect_after);
    if rect_moved {
        eprintln!(
            "pdfcer: {}: /Rect moved. For every subtype except Square and Circle the \
             rectangle is derived from the geometry plus a margin that contains the stroke and \
             any arrowheads, so changing the width resizes the box. This is correct, not drift.",
            input.display()
        );
    }

    let ap = match change.appearance {
        AppearanceWrite::InPlace(_) => "in-place",
        AppearanceWrite::Created(_) => "created",
        AppearanceWrite::CopiedOnWrite { .. } => "copied",
        // `AppearanceWrite` is #[non_exhaustive]; a future variant must
        // print SOMETHING rather than fail to compile a shell.
        _ => "other",
    };
    if matches!(change.appearance, AppearanceWrite::CopiedOnWrite { .. }) {
        eprintln!(
            "pdfcer: {}: the appearance stream was SHARED with another annotation \
             (§12.5.2 permits it), so it was copied rather than rewritten — the other \
             annotation keeps the look it had.",
            input.display()
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
        "set-markup-style {} page {page} index {index} mode={} -> {}; \
subtype={subtype} ap={ap} rect_moved={} dropped={} changed={} objects={} verbatim={} \
reserialized={} promoted={} appended={} out_bytes={} undo_verified={} undo_identical={} \
delinearized={}",
        input.display(),
        mode.name(),
        output.display(),
        u32::from(rect_moved),
        change.dropped.len(),
        outcome.changed,
        r.objects_written,
        r.objects_verbatim,
        r.objects_reserialized,
        r.promoted.len(),
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
        u32::from(r.delinearized),
    );
    finish_edit(input, &outcome)
}

/// `delete-annotation` — the general annotation-deletion verb (Pass 38.5).
///
/// ## Resolving `--page`/`--index` to an object, and why NOT an object number
///
/// The core verb takes an [`ObjId`](pdfcer_core::object::ObjId), because
/// object identity is the only handle that stays correct while a session
/// mutates. A CLI cannot use that: the operator's source of truth is
/// `list-annotations`' own output, which prints `page=` and `index=` and
/// deliberately does **not** print object numbers. So the pair is resolved
/// here, against the SAME `page_annotations` walk `list-annotations` uses,
/// which is what makes "list it, then delete that index" reliable rather
/// than approximately right.
///
/// Both out-of-range cases are refused with the count that was actually
/// there — an index past the end says how many annotations the page has,
/// because "index 4 is out of range" without the bound sends the operator
/// back to re-run the list command for a number this process already knew.
///
/// ## Contract
///
/// - One `delete-annotation …` line carrying `subtype=`, `route=`,
///   `popup_removed=`, `parent_popup_cleared=`, `replies_orphaned=`,
///   `group_promoted=` and `ap_removed=`, then the exit code from
///   [`finish_edit`].
/// - **Every non-obvious consequence is ALSO printed in prose to stderr**,
///   for the same reason `delete-field` prints its `selection_cleared`
///   disclosure twice: the machine-readable field is for a script, the
///   sentence is for the person who will otherwise wonder an hour later why
///   three other comments changed.
/// - Refusals — no such page or index, a `/Widget` target, an encrypted
///   document, a certification at `/P` below 3 — go through
///   [`report_edit_error`] or their own message BEFORE any mutation.
pub(crate) fn cmd_delete_annotation(
    input: &Path,
    page: usize,
    index: usize,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    if page == 0 {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // Resolved inside a block so the session borrow ends before the mutable
    // call below.
    let annot_id = {
        let slots = match session.page_slots() {
            Ok(slots) => slots,
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", input.display());
                return exit::RUNTIME_ERROR;
            }
        };
        let Some(slot) = slots.get(page - 1) else {
            eprintln!(
                "pdfcer: {}: no page {page} — the document has {} page(s)",
                input.display(),
                slots.len()
            );
            return exit::RUNTIME_ERROR;
        };
        let annots = pdfcer_core::annot::page_annotations(&session.graph(), slot.id);
        let Some(annot) = annots.get(index) else {
            eprintln!(
                "pdfcer: {}: page {page} has no annotation at index {index} — it has {} (indices 0..{})",
                input.display(),
                annots.len(),
                annots.len().saturating_sub(1)
            );
            return exit::RUNTIME_ERROR;
        };
        // An annotation reached as a DIRECT dictionary inside `/Annots` has no
        // object identity to delete. Malformed (Table 164 dictionaries are
        // indirect objects) and refused by name rather than silently skipped.
        let Some(id) = annot.id else {
            eprintln!(
                "pdfcer: {}: page {page} index {index} is a direct dictionary inside /Annots, not an indirect object — it has no identity to delete, and rewriting the array around it would be a repair this command does not perform",
                input.display()
            );
            return exit::EDIT_REFUSED;
        };
        id
    };

    let gone = match session.delete_annotation(annot_id) {
        Ok(gone) => gone,
        Err(err) => return report_edit_error(input, &err),
    };

    // The prose half of the disclosures. Ordered worst-first: the group
    // promotion changes what a reader SEES on other annotations, which is
    // the one an operator is least likely to predict.
    if gone.group_members_promoted > 0 {
        eprintln!(
            "pdfcer: {}: {} other annotation(s) were subordinates of the one you deleted (/RT /Group). While it existed, a conforming reader was required to IGNORE their own author and note text and display its instead — so those now become visible. Their /IRT link was removed; nothing else about them changed.",
            input.display(),
            gone.group_members_promoted
        );
    }
    if gone.replies_orphaned > 0 {
        eprintln!(
            "pdfcer: {}: {} repl(ies) pointed at the annotation you deleted. They were KEPT — they are separate annotations with their own text — and their now-dangling /IRT was removed, so each is a standalone comment. Deleting a whole thread means deleting each member.",
            input.display(),
            gone.replies_orphaned
        );
    }
    if gone.popup_removed {
        eprintln!(
            "pdfcer: {}: its /Popup window was deleted with it — ISO 32000-1 12.5.6.14 says a pop-up \"shall not appear alone\", so leaving it would be non-conforming, not merely untidy.",
            input.display()
        );
    }
    if gone.parent_popup_cleared {
        eprintln!(
            "pdfcer: {}: you deleted a /Popup window; its parent annotation was kept (deleting a window does not delete the comment it belongs to) and its now-dangling /Popup key was removed.",
            input.display()
        );
    }
    // DELETE IS NOT REDACT, and this is the one warning that must not be
    // conditional on anything. ISO 32000-1 Annex H.7.3: "although the two
    // objects have been deleted, they are still present in the file" — an
    // incremental save APPENDS a free-list entry, it does not go back and
    // overwrite the bytes. So the note text of a deleted comment is still
    // recoverable from the saved file with a hex editor.
    //
    // An operator deleting a comment because it was confidential is exactly
    // the person who will not think to ask, and the redaction feature two
    // subcommands away is the one that actually removes bytes. Printed on
    // every incremental delete, with the remedy, rather than left to a
    // manual page.
    if matches!(mode, SaveMode::Incremental) {
        eprintln!(
            "pdfcer: {}: note — an incremental save APPENDS the deletion; the annotation's bytes, including its note text, are still present in the output file and recoverable (ISO 32000-1 Annex H.7.3). Deleting is not redacting. Pass --mode full if the content must not survive in the file — but note that a full rewrite destroys every existing signature.",
            input.display()
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
    let route = match gone.route {
        pdfcer_core::edit::AnnotationDeletionRoute::General => "general",
        pdfcer_core::edit::AnnotationDeletionRoute::RedactionMark => "redaction-mark",
        pdfcer_core::edit::AnnotationDeletionRoute::Dimension => "dimension",
        // `AnnotationDeletionRoute` is #[non_exhaustive]: a future route must
        // print an honest unknown rather than be mapped to the wrong verb.
        _ => "other",
    };
    println!(
        "delete-annotation {} page={page} index={index} subtype={} route={route} popup_removed={} parent_popup_cleared={} replies_orphaned={} group_promoted={} ap_removed={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        sanitize_token(&gone.subtype),
        u32::from(gone.popup_removed),
        u32::from(gone.parent_popup_cleared),
        gone.replies_orphaned,
        gone.group_members_promoted,
        gone.appearance_streams_removed,
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

/// Implement `pdfcer reorder-annotations`.
///
/// The index space is `list-annotations`' — [`pdfcer_core::annot::page_annotations`]
/// order, which skips null and non-dictionary `/Annots` entries — because
/// that is the only numbering the operator has ever been shown. Indices are
/// mapped to object ids here and the core verb is given ids, which it checks
/// against the array rather than trusts; a direct-dictionary annotation has
/// no id and is reported as pinned rather than silently dropped from the
/// order.
pub(crate) fn cmd_reorder_annotations(
    input: &Path,
    page: usize,
    order: &str,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    if page == 0 {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // Resolve indices to ids inside a block so the session borrow ends
    // before the mutable call below.
    let (ids, pinned_indices, count) = {
        let slots = match session.page_slots() {
            Ok(slots) => slots,
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", input.display());
                return exit::RUNTIME_ERROR;
            }
        };
        let Some(slot) = slots.get(page - 1) else {
            eprintln!(
                "pdfcer: {}: no page {page} — the document has {} page(s)",
                input.display(),
                slots.len()
            );
            return exit::RUNTIME_ERROR;
        };
        let annots = pdfcer_core::annot::page_annotations(&session.graph(), slot.id);
        let count = annots.len();

        let mut indices: Vec<usize> = Vec::new();
        for token in order.split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            match token.parse::<usize>() {
                Ok(i) => indices.push(i),
                Err(_) => {
                    eprintln!(
                        "pdfcer: {}: --order: `{token}` is not an annotation index (a whole number from 0, as list-annotations prints them)",
                        input.display()
                    );
                    return exit::EDIT_REFUSED;
                }
            }
        }
        let mut seen = vec![false; count];
        for &i in &indices {
            if i >= count {
                if count == 0 {
                    eprintln!(
                        "pdfcer: {}: --order names index {i}, but page {page} has no annotations",
                        input.display()
                    );
                } else {
                    eprintln!(
                        "pdfcer: {}: --order names index {i}, but page {page} has {count} annotation(s) (indices 0..{})",
                        input.display(),
                        count - 1
                    );
                }
                return exit::EDIT_REFUSED;
            }
            if seen[i] {
                eprintln!(
                    "pdfcer: {}: --order lists index {i} more than once; a reorder names every annotation exactly once",
                    input.display()
                );
                return exit::EDIT_REFUSED;
            }
            seen[i] = true;
        }
        if indices.len() != count {
            let missing: Vec<String> = seen
                .iter()
                .enumerate()
                .filter(|(_, s)| !**s)
                .map(|(i, _)| i.to_string())
                .collect();
            eprintln!(
                "pdfcer: {}: --order lists {} of page {page}'s {count} annotation(s); missing {} — a reorder that drops one would be a delete under another name",
                input.display(),
                indices.len(),
                missing.join(",")
            );
            return exit::EDIT_REFUSED;
        }

        let mut ids: Vec<pdfcer_core::object::ObjId> = Vec::with_capacity(count);
        let mut pinned_indices: Vec<usize> = Vec::new();
        for &i in &indices {
            match annots[i].id {
                Some(id) => ids.push(id),
                None => pinned_indices.push(i),
            }
        }
        (ids, pinned_indices, count)
    };

    let outcome = match session.reorder_annotations(page - 1, &ids) {
        Ok(o) => o,
        Err(err) => return report_edit_error(input, &err),
    };

    // Disclosures, worst-first: the case where the order the operator
    // arranged is NOT the tab order comes before the ones that merely
    // qualify it.
    let tabs_label = match &outcome.tabs {
        pdfcer_core::edit::PageTabs::Absent => "absent".to_owned(),
        pdfcer_core::edit::PageTabs::Row => "R".to_owned(),
        pdfcer_core::edit::PageTabs::Column => "C".to_owned(),
        pdfcer_core::edit::PageTabs::Structure => "S".to_owned(),
        pdfcer_core::edit::PageTabs::ArrayOrder => "A".to_owned(),
        pdfcer_core::edit::PageTabs::WidgetOrder => "W".to_owned(),
        pdfcer_core::edit::PageTabs::Other(name) => name.clone(),
        _ => "?".to_owned(),
    };
    match &outcome.tabs {
        pdfcer_core::edit::PageTabs::Row
        | pdfcer_core::edit::PageTabs::Column
        | pdfcer_core::edit::PageTabs::Structure => {
            eprintln!(
                "pdfcer: {}: page {page} declares /Tabs /{tabs_label}, so a reader tabs by {} — NOT by the array order you just arranged. The array is reordered (paint order changed as asked); the tab order is not, and /Tabs was left as it was.",
                input.display(),
                match &outcome.tabs {
                    pdfcer_core::edit::PageTabs::Row => "row (geometry)",
                    pdfcer_core::edit::PageTabs::Column => "column (geometry)",
                    _ => "the structure tree",
                }
            );
        }
        pdfcer_core::edit::PageTabs::Absent => {
            eprintln!(
                "pdfcer: {}: page {page} has no /Tabs entry, so the file does not STATE a tab order; readers generally use /Annots order, which is now the order you gave. /Tabs was not written.",
                input.display()
            );
        }
        pdfcer_core::edit::PageTabs::WidgetOrder => {
            eprintln!(
                "pdfcer: {}: page {page} declares /Tabs /W: widgets are tabbed in the array order you just arranged. What follows them is contested inside ISO 32000-2 itself (Table 31 says array order, 12.5.1 says row order); pdfcer reads Table 31.",
                input.display()
            );
        }
        pdfcer_core::edit::PageTabs::Other(name) => {
            eprintln!(
                "pdfcer: {}: page {page} declares /Tabs /{name}, which is not a value ISO 32000 permits (R, C, S; A and W in PDF 2.0) — a producer defect, reported verbatim and left as it was.",
                input.display()
            );
        }
        _ => {}
    }
    if !pinned_indices.is_empty() {
        let list: Vec<String> = pinned_indices.iter().map(ToString::to_string).collect();
        eprintln!(
            "pdfcer: {}: {} annotation(s) at index {} are direct dictionaries inside /Annots, not indirect objects — they have no identity to reorder by and stayed at their original position; the others were arranged around them.",
            input.display(),
            pinned_indices.len(),
            list.join(",")
        );
    }
    if outcome.non_widgets_moved > 0 {
        eprintln!(
            "pdfcer: {}: {} of the {} annotation(s) that moved are not form widgets. /Annots order is also PAINT order, so where a link or markup overlaps another annotation, which one draws on top has changed.",
            input.display(),
            outcome.non_widgets_moved,
            outcome.moved
        );
    }
    if outcome.array_copied {
        eprintln!(
            "pdfcer: {}: this page shared its /Annots array with another page; the array was copied before reordering so the other page keeps its order.",
            input.display()
        );
    }
    if outcome.trap_net_pinned {
        eprintln!(
            "pdfcer: {}: page {page} carries a trap network annotation (/TrapNet), which ISO 32000-1 12.5.6.21 requires to be the LAST entry; it was held in place and the other annotations were arranged around it.{}",
            input.display(),
            if outcome.annot_states_permuted {
                " Its /AnnotStates array was permuted alongside so the two stay parallel (Table 366)."
            } else {
                ""
            }
        );
    }
    if outcome.goto_e_targets_reindexed > 0 {
        eprintln!(
            "pdfcer: {}: {} embedded-file link target(s) (/GoToE, ISO 32000-1 Table 202) elsewhere in this document named an annotation on page {page} by its /Annots index; each was re-indexed to the annotation's new position so it still points at the same annotation.",
            input.display(),
            outcome.goto_e_targets_reindexed
        );
    }
    if outcome.moved == 0 {
        eprintln!(
            "pdfcer: {}: the order given is the order page {page} already had; nothing changed and nothing was recorded.",
            input.display()
        );
    }

    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };

    println!(
        "reorder-annotations {} page={page} mode={} -> {}; annotations={count} moved={} non_widgets_moved={} pinned={} tabs={tabs_label} array_copied={} trap_net_pinned={} annot_states_permuted={} goto_e_reindexed={} {}",
        input.display(),
        mode.name(),
        output.display(),
        outcome.moved,
        outcome.non_widgets_moved,
        outcome.pinned,
        u32::from(outcome.array_copied),
        u32::from(outcome.trap_net_pinned),
        u32::from(outcome.annot_states_permuted),
        outcome.goto_e_targets_reindexed,
        edit_metrics(&saved)
    );
    finish_edit(input, &saved)
}

/// `move-annotation` — translate one annotation and every geometry key it
/// carries (`Pass 149.0`).
///
/// The locate-then-move shape mirrors `cmd_delete_annotation` exactly,
/// including the direct-dictionary refusal, because an operator who can name
/// an annotation for one verb must be able to name it the same way for the
/// other. Diverging on the addressing scheme is how a shell ends up with two
/// annotation identities.
pub(crate) fn cmd_move_annotation(
    input: &Path,
    page: usize,
    index: usize,
    dx: f64,
    dy: f64,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    if page == 0 {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // Resolved inside a block so the session borrow ends before the mutable
    // call below.
    let annot_id = {
        let slots = match session.page_slots() {
            Ok(slots) => slots,
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", input.display());
                return exit::RUNTIME_ERROR;
            }
        };
        let Some(slot) = slots.get(page - 1) else {
            eprintln!(
                "pdfcer: {}: no page {page} — the document has {} page(s)",
                input.display(),
                slots.len()
            );
            return exit::RUNTIME_ERROR;
        };
        let annots = pdfcer_core::annot::page_annotations(&session.graph(), slot.id);
        let Some(annot) = annots.get(index) else {
            eprintln!(
                "pdfcer: {}: page {page} has no annotation at index {index} — it has {} (indices 0..{})",
                input.display(),
                annots.len(),
                annots.len().saturating_sub(1)
            );
            return exit::RUNTIME_ERROR;
        };
        let Some(id) = annot.id else {
            eprintln!(
                "pdfcer: {}: page {page} index {index} is a direct dictionary inside /Annots, not an indirect object — it has no identity to move, and rewriting the array around it would be a repair this command does not perform",
                input.display()
            );
            return exit::EDIT_REFUSED;
        };
        id
    };

    let moved = match session.move_annotation(annot_id, dx, dy) {
        Ok(m) => m,
        Err(err) => return report_edit_error(input, &err),
    };

    // The disclosures, worst-first: the two things NOT moved come before the
    // two that were, because an operator predicts the move and does not
    // predict the exceptions.
    if moved.rect_differences_untouched {
        eprintln!(
            "pdfcer: {}: this annotation carries /RD (rect differences), which were NOT translated and must not be — they are four inset DISTANCES from /Rect, not coordinates, so shifting them would have deformed the annotation while claiming to move it.",
            input.display()
        );
    }
    if let Some(popup) = moved.popup_left_behind {
        eprintln!(
            "pdfcer: {}: its /Popup window (object {}) was LEFT WHERE IT WAS. A pop-up is a separate annotation with its own placement, which ISO 32000-1 12.5.6.14 leaves to the reader; move it separately if you want it to follow.",
            input.display(),
            popup.num
        );
    }
    if moved.geometry_keys_moved.is_empty() {
        eprintln!(
            "pdfcer: {}: this annotation carries no geometry key — its /Rect IS its geometry, which is normal for a Text note, a Stamp or a Link. Nothing was missed.",
            input.display()
        );
    }
    if moved.appearance_carried {
        eprintln!(
            "pdfcer: {}: its appearance stream was carried UNCHANGED. ISO 32000-1 12.5.5 recomputes the placement matrix from the appearance BBox and the new /Rect, so a pure translation moves the artwork 1:1 with no re-authoring — which also means an appearance pdfcer did not draw survives this move intact.",
            input.display()
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

    println!(
        "move-annotation {} page={page} index={index} -> {}",
        input.display(),
        output.display()
    );
    println!(
        "  subtype={} dx={dx:.3} dy={dy:.3} rect=[{:.2} {:.2} {:.2} {:.2}]->[{:.2} {:.2} {:.2} {:.2}]",
        moved.subtype,
        moved.from.llx,
        moved.from.lly,
        moved.from.urx,
        moved.from.ury,
        moved.to.llx,
        moved.to.lly,
        moved.to.urx,
        moved.to.ury,
    );
    println!(
        "  geometry_keys_moved={} appearance_carried={} rect_differences_untouched={} popup_left_behind={}",
        if moved.geometry_keys_moved.is_empty() {
            "-".to_owned()
        } else {
            moved.geometry_keys_moved.join(",")
        },
        u32::from(moved.appearance_carried),
        u32::from(moved.rect_differences_untouched),
        moved
            .popup_left_behind
            .map_or_else(|| "-".to_owned(), |p| p.num.to_string()),
    );
    finish_edit(input, &outcome)
}

/// `rotate-annotation` — turn one annotation about a point (`Pass 155.0`).
///
/// Prints the `/Rect` growth explicitly, because an operator who rotates a
/// note 30° and sees its rectangle get larger will otherwise report it as a
/// defect. It is §12.5.2 requiring an upright rectangle, not pdfcer scaling
/// anything.
pub(crate) fn cmd_rotate_annotation(
    input: &Path,
    at: (usize, usize),
    degrees: f64,
    absolute: bool,
    anchor: (f64, f64),
    output: &Path,
    mode: SaveMode,
) -> u8 {
    let (page, index) = at;
    if page == 0 {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let annot_id = {
        let slots = match session.page_slots() {
            Ok(s) => s,
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", input.display());
                return exit::RUNTIME_ERROR;
            }
        };
        let Some(slot) = slots.get(page - 1) else {
            eprintln!(
                "pdfcer: {}: no page {page} — the document has {} page(s)",
                input.display(),
                slots.len()
            );
            return exit::RUNTIME_ERROR;
        };
        let annots = pdfcer_core::annot::page_annotations(&session.graph(), slot.id);
        let Some(annot) = annots.get(index) else {
            eprintln!(
                "pdfcer: {}: page {page} has no annotation at index {index} — it has {}",
                input.display(),
                annots.len()
            );
            return exit::RUNTIME_ERROR;
        };
        let Some(id) = annot.id else {
            eprintln!(
                "pdfcer: {}: page {page} index {index} is a direct dictionary inside /Annots, not an indirect object — it has no identity to rotate",
                input.display()
            );
            return exit::EDIT_REFUSED;
        };
        id
    };

    // `--absolute` routes to the setter that reads the current angle out of
    // the file rather than trusting a caller's idea of it (`Pass 155.2`).
    let out = match if absolute {
        session.set_annotation_rotation(annot_id, anchor, degrees)
    } else {
        session.rotate_annotation(annot_id, anchor, degrees)
    } {
        Ok(o) => o,
        Err(err) => return report_edit_error(input, &err),
    };
    if absolute {
        // Rule 4: the outcome echoes the DELTA applied, not the absolute
        // target the caller asked for, so the two numbers differ on screen
        // and the reason is said rather than left to be inferred.
        eprintln!(
            "pdfcer: {}: --absolute: the annotation was at {:.4} degrees, so {:.4} was applied to reach {degrees:.4}. The lines below report the turn that happened, not the angle you asked for.",
            input.display(),
            degrees - out.degrees,
            out.degrees,
        );
    }

    let grew = (out.to.urx - out.to.llx) * (out.to.ury - out.to.lly)
        > (out.from.urx - out.from.llx) * (out.from.ury - out.from.lly) + 1e-6;
    if grew {
        eprintln!(
            "pdfcer: {}: its /Rect is now LARGER, and that is correct — ISO 32000-1 12.5.2 requires an upright rectangle, and the upright box bounding a rotated shape is bigger unless the angle is a multiple of 90 degrees. The artwork did not grow; the rectangle around it did.",
            input.display()
        );
    }
    // Rule 4: pdfcer chose between three rules for the new rectangle on
    // evidence the operator cannot see, and only two of the three compose.
    // The one that does not is disclosed BEFORE the outcome lines, because
    // it is the thing they would not predict -- rotating twice really does
    // enlarge such an annotation, and saying nothing would leave them to
    // discover it the way the operator discovered the defect this replaced.
    if out.rect_derived_from == pdfcer_core::edit::RectDerivation::PreviousRect {
        eprintln!(
            "pdfcer: {}: this annotation carries no appearance stream and no rotatable geometry keys, so its artwork IS its rectangle and there is nowhere to record an orientation. Its new /Rect had to be bounded from the old one, which means REPEATED rotation of this annotation keeps enlarging it. Give it an appearance stream first if you need to turn it more than once.",
            input.display()
        );
    }
    if out.rect_differences_untouched {
        eprintln!(
            "pdfcer: {}: this annotation carries /RD (rect differences), which were LEFT ALONE. They are four insets measured along the /Rect's own axes (Table 175), and at an angle that is not a quarter turn no axis-aligned inset expresses the rotated result — so pdfcer did not invent one.",
            input.display()
        );
    }
    if !out.appearance_matrix_updated {
        eprintln!(
            "pdfcer: {}: this annotation has no appearance stream, so only its geometry keys turned. A reader that regenerates the appearance will draw the rotated shape; one that does not will draw nothing, exactly as before.",
            input.display()
        );
    }

    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(o) => o,
        Err(code) => return code,
    };

    println!(
        "rotate-annotation {} page={page} index={index} -> {}",
        input.display(),
        output.display()
    );
    println!(
        "  subtype={} degrees={:.4} anchor=({:.2} {:.2}) rect=[{:.2} {:.2} {:.2} {:.2}]->[{:.2} {:.2} {:.2} {:.2}]",
        out.subtype,
        out.degrees,
        anchor.0,
        anchor.1,
        out.from.llx,
        out.from.lly,
        out.from.urx,
        out.from.ury,
        out.to.llx,
        out.to.lly,
        out.to.urx,
        out.to.ury,
    );
    println!(
        "  geometry_keys_rotated={} appearance_matrix={} rect_derived={}",
        if out.geometry_keys_rotated.is_empty() {
            "none".to_owned()
        } else {
            out.geometry_keys_rotated.join(",")
        },
        if out.appearance_matrix_updated {
            "composed"
        } else {
            "absent"
        },
        match out.rect_derived_from {
            pdfcer_core::edit::RectDerivation::Artwork => "artwork",
            pdfcer_core::edit::RectDerivation::Geometry => "geometry",
            pdfcer_core::edit::RectDerivation::PreviousRect => "previous-rect",
            _ => "unknown",
        }
    );
    finish_edit(input, &saved)
}

/// `resize-annotation` — scale one annotation and every geometry key it owns
/// about a caller-supplied anchor (`Pass 151.0`).
///
/// # Why the anchor and the factors are both the caller's to supply
///
/// The same split `transform-objects` uses. Which corner is fixed is a
/// decision a *shell* makes from which grip was grabbed; a flag that took a
/// grip name would encode one shell's affordance in a batch tool that has no
/// grips at all. Two flags and two factors compose into every gesture a GUI
/// can produce, including the mirror (a negative factor) that no grip-name
/// vocabulary would have had a word for.
///
/// # What it prints, and why the exceptions come first
///
/// Same discipline as `move-annotation`: the things that did NOT travel are
/// reported before the things that did, because an operator predicts the
/// resize and does not predict the exceptions. `--scale-stroke-width` being
/// off is the one most likely to surprise — somebody who scaled a callout 3×
/// and expected a heavier border needs telling it stayed at 1.0 pt (rule 4:
/// the invocation IS the commit here, so the CLI prints on the way past).
pub(crate) fn cmd_resize_annotation(
    input: &Path,
    at: (usize, usize),
    factors: (f64, f64),
    anchor: (f64, f64),
    opts: &pdfcer_core::edit::ResizeOptions,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    let (page, index) = at;
    let (sx, sy) = factors;
    if page == 0 {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let annot_id = {
        let slots = match session.page_slots() {
            Ok(slots) => slots,
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", input.display());
                return exit::RUNTIME_ERROR;
            }
        };
        let Some(slot) = slots.get(page - 1) else {
            eprintln!(
                "pdfcer: {}: no page {page} — the document has {} page(s)",
                input.display(),
                slots.len()
            );
            return exit::RUNTIME_ERROR;
        };
        let annots = pdfcer_core::annot::page_annotations(&session.graph(), slot.id);
        let Some(annot) = annots.get(index) else {
            eprintln!(
                "pdfcer: {}: page {page} has no annotation at index {index} — it has {} (indices 0..{})",
                input.display(),
                annots.len(),
                annots.len().saturating_sub(1)
            );
            return exit::RUNTIME_ERROR;
        };
        let Some(id) = annot.id else {
            eprintln!(
                "pdfcer: {}: page {page} index {index} is a direct dictionary inside /Annots, not an indirect object — it has no identity to resize, and rewriting the array around it would be a repair this command does not perform",
                input.display()
            );
            return exit::EDIT_REFUSED;
        };
        id
    };

    let out = match session.resize_annotation(annot_id, anchor, sx, sy, opts) {
        Ok(r) => r,
        Err(err) => return report_edit_error(input, &err),
    };

    // The exceptions, before the outcome.
    if out.stroke_width.is_none() {
        eprintln!(
            "pdfcer: {}: its border width was NOT scaled — a line weight is a drafting convention rather than a length in the scaled space, which is why the default leaves it alone. Pass --scale-stroke-width if you wanted it to follow.",
            input.display()
        );
    }
    if out.rect_differences_scaled == Some(false) {
        eprintln!(
            "pdfcer: {}: its /RD (rect differences) were left UNSCALED at your request. They are inset distances measured in the space that just changed size, so the annotation's proportions have changed.",
            input.display()
        );
    }
    match out.appearance {
        pdfcer_core::edit::ResizedAppearance::None => {
            eprintln!(
                "pdfcer: {}: this annotation has no appearance stream, so nothing was redrawn. It paints from its geometry keys in a reader that regenerates and paints NOTHING in one that does not — unchanged by this resize, but worth knowing.",
                input.display()
            );
        }
        pdfcer_core::edit::ResizedAppearance::CarriedDistorted => {
            eprintln!(
                "pdfcer: {}: its appearance was CARRIED under a non-uniform scale, as you allowed. ISO 32000-1 12.5.5's placement matrix now stretches the artwork unequally, so its stroke is anisotropic — the border is thicker on one axis than the other, and no /BS /W value describes that.",
                input.display()
            );
        }
        pdfcer_core::edit::ResizedAppearance::CarriedUniform => {
            eprintln!(
                "pdfcer: {}: its appearance was CARRIED rather than redrawn — pdfcer did not draw it, so pdfcer did not replace it. ISO 32000-1 12.5.5's matrix scales it uniformly, which is exactly right.",
                input.display()
            );
        }
        pdfcer_core::edit::ResizedAppearance::Rebuilt => {}
        // `ResizedAppearance` is `#[non_exhaustive]`, so this arm is compulsory
        // rather than defensive. It is written to SPEAK because the compulsory
        // form of it — an empty `_ => {}` — is precisely how a future variant
        // would ship as silence.
        other => {
            eprintln!(
                "pdfcer: {}: this build of pdfcer-core reported an appearance outcome this CLI does not recognise ({other:?}). The resize was performed; the description above may be incomplete.",
                input.display()
            );
        }
    }
    if out.geometry_keys_scaled.is_empty() {
        eprintln!(
            "pdfcer: {}: this annotation carries no geometry key — its /Rect IS its geometry, which is normal for a Text note, a Stamp or a Link. Nothing was missed.",
            input.display()
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

    println!(
        "resize-annotation {} page={page} index={index} -> {}",
        input.display(),
        output.display()
    );
    println!(
        "  subtype={} sx={sx:.4} sy={sy:.4} anchor=({:.2} {:.2}) rect=[{:.2} {:.2} {:.2} {:.2}]->[{:.2} {:.2} {:.2} {:.2}]",
        out.subtype,
        anchor.0,
        anchor.1,
        out.from.llx,
        out.from.lly,
        out.from.urx,
        out.from.ury,
        out.to.llx,
        out.to.lly,
        out.to.urx,
        out.to.ury,
    );
    println!(
        "  geometry_keys_scaled={} appearance={} stroke_width={} rect_differences={}",
        if out.geometry_keys_scaled.is_empty() {
            "none".to_owned()
        } else {
            out.geometry_keys_scaled.join(",")
        },
        match out.appearance {
            pdfcer_core::edit::ResizedAppearance::None => "none",
            pdfcer_core::edit::ResizedAppearance::Rebuilt => "rebuilt",
            pdfcer_core::edit::ResizedAppearance::CarriedUniform => "carried-uniform",
            pdfcer_core::edit::ResizedAppearance::CarriedDistorted => "carried-distorted",
            _ => "unrecognised",
        },
        match out.stroke_width {
            Some((before, after)) => format!("{before:.3}->{after:.3}"),
            None => "unchanged".to_owned(),
        },
        match out.rect_differences_scaled {
            Some(true) => "scaled",
            Some(false) => "kept",
            None => "absent",
        }
    );
    finish_edit(input, &outcome)
}
