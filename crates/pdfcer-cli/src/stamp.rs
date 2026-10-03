use super::*;

/// Implement `pdfcer place-stamp` (`Pass 293.0`; `--as-content`, `Pass 450.0`).
///
/// Two documents, one placement: the collection supplies the artwork, the
/// input supplies the page. Everything the engine decided on the way past is
/// printed to stderr, because the CLI has no session in which to show it
/// (rule 11) — the invocation IS the commit.
// One argument per flag, as every other command function here is shaped.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_place_stamp(
    input: &Path,
    from: &Path,
    stamp_page: Option<usize>,
    stamp: Option<&str>,
    page: usize,
    rect: Option<&str>,
    at: Option<&str>,
    as_content: bool,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    if stamp_page.is_none() && stamp.is_none() {
        eprintln!("pdfcer: name the artwork with --stamp-page N or --stamp INTERNAL-NAME");
        return exit::EDIT_REFUSED;
    }
    if rect.is_none() && at.is_none() {
        eprintln!("pdfcer: say where: --at X,Y places at the stamp's own size, --rect fills a box");
        return exit::EDIT_REFUSED;
    }
    let source_doc = match open_for_read(from) {
        Ok(doc) => doc,
        Err(code) => return code,
    };
    let (source_index, dynamic) = match resolve_stamp(from, &source_doc, stamp_page, stamp) {
        Ok(found) => found,
        Err(code) => return code,
    };
    let rect = match placement_rect(from, &source_doc, source_index, rect, at) {
        Ok(rect) => rect,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let (src, target) = (source_index, page.saturating_sub(1));
    let placed = if as_content {
        session
            .place_page_content(&source_doc.view(), src, target, rect)
            .map(Placed::from)
    } else {
        session
            .place_page_artwork(&source_doc.view(), src, target, rect)
            .map(Placed::from)
    };
    let placed = match placed {
        Ok(placed) => placed,
        Err(err) => return report_edit_error(input, &err),
    };
    disclose_placement(input, &placed, rect, dynamic);
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
        "place-stamp {} from {} stamp_page={} page={} -> {}; {}",
        input.display(),
        from.display(),
        source_index + 1,
        page,
        output.display(),
        placed.fields(as_content, dynamic),
    );
    finish_edit(input, &outcome)
}

/// What either placement verb reported, for one disclosure and one output
/// line. `obj` is the annotation, or with `--as-content` the content stream.
struct Placed {
    obj: pdfcer_core::object::ObjId,
    form: pdfcer_core::object::ObjId,
    rect: pdfcer_core::page_tree::Rect,
    scale_x: f64,
    scale_y: f64,
    distorted: bool,
    objects_imported: usize,
    resources_renamed: usize,
    annots_ignored: usize,
    widgets_ignored: usize,
    group_carried: bool,
}

impl Placed {
    /// The machine-readable `key=value` tail of the output line.
    fn fields(&self, as_content: bool, dynamic: bool) -> String {
        format!(
            "{}={} form={} rect={:.2},{:.2},{:.2},{:.2} scale={:.3},{:.3} distorted={} \
             objects_imported={} resources_renamed={} annots_ignored={} widgets_ignored={} \
             group_carried={} dynamic={}",
            if as_content { "content" } else { "obj" },
            self.obj.num,
            self.form.num,
            self.rect.llx,
            self.rect.lly,
            self.rect.urx,
            self.rect.ury,
            self.scale_x,
            self.scale_y,
            u32::from(self.distorted),
            self.objects_imported,
            self.resources_renamed,
            self.annots_ignored,
            self.widgets_ignored,
            u32::from(self.group_carried),
            u32::from(dynamic),
        )
    }
}

impl From<pdfcer_core::edit::PlacedArtwork> for Placed {
    fn from(p: pdfcer_core::edit::PlacedArtwork) -> Self {
        Self {
            obj: p.annot_id,
            form: p.form_id,
            rect: p.rect,
            scale_x: p.scale_x,
            scale_y: p.scale_y,
            distorted: p.distorted,
            objects_imported: p.objects_imported,
            resources_renamed: p.resources_renamed,
            annots_ignored: p.source_annotations_ignored,
            widgets_ignored: p.source_widgets_ignored,
            group_carried: p.transparency_group_carried,
        }
    }
}

impl From<pdfcer_core::edit::PlacedPageContent> for Placed {
    fn from(p: pdfcer_core::edit::PlacedPageContent) -> Self {
        Self {
            obj: p.content_id,
            form: p.form_id,
            rect: p.rect,
            scale_x: p.scale_x,
            scale_y: p.scale_y,
            distorted: p.distorted,
            objects_imported: p.objects_imported,
            resources_renamed: p.resources_renamed,
            annots_ignored: p.source_annotations_ignored,
            widgets_ignored: p.source_widgets_ignored,
            group_carried: p.transparency_group_carried,
        }
    }
}

/// The source page index for `--stamp-page`, or `--stamp NAME` resolved
/// through the collection's name tree (`#` optional), and whether that stamp
/// is dynamic.
fn resolve_stamp(
    from: &Path,
    source_doc: &pdfcer_core::document::Document,
    stamp_page: Option<usize>,
    stamp: Option<&str>,
) -> Result<(usize, bool), u8> {
    let (None, Some(name)) = (stamp_page, stamp) else {
        return Ok((stamp_page.unwrap_or(1).saturating_sub(1), false));
    };
    let collection = pdfcer_core::stamp_file::read(source_doc);
    let wanted = name.trim_start_matches('#');
    let Some(entry) = collection
        .stamps
        .iter()
        .find(|e| e.internal.trim_start_matches('#') == wanted)
    else {
        let names: Vec<_> = collection
            .stamps
            .iter()
            .map(|e| e.internal.clone())
            .collect();
        eprintln!(
            "pdfcer: {}: no stamp named {name:?} in this collection ({} stamp(s): {})",
            from.display(),
            names.len(),
            names.join(", ")
        );
        return Err(exit::EDIT_REFUSED);
    };
    let Some(index) = entry.page_index else {
        if let Some(why) = &collection.page_tree_error {
            eprintln!(
                "pdfcer: {}: that stamp's page cannot be resolved because the page tree \
                 would not walk ({why})",
                from.display()
            );
        } else {
            eprintln!(
                "pdfcer: {}: that stamp names a page the collection does not have",
                from.display()
            );
        }
        return Err(exit::EDIT_REFUSED);
    };
    Ok((index, entry.dynamic))
}

/// `--rect` as given, or `--at X,Y` at the source page's crop-box size —
/// Acrobat's click-to-place, the same box the engine maps, so `--at` reports
/// `distorted=0` by construction.
fn placement_rect(
    from: &Path,
    source_doc: &pdfcer_core::document::Document,
    source_index: usize,
    rect: Option<&str>,
    at: Option<&str>,
) -> Result<pdfcer_core::page_tree::Rect, u8> {
    if let Some(spec) = rect {
        return rect_from(spec).map_err(|msg| {
            eprintln!("pdfcer: --rect: {msg}");
            exit::EDIT_REFUSED
        });
    }
    let spec = at.unwrap_or_default();
    let Some((x, y)) = spec.split_once(',') else {
        eprintln!("pdfcer: --at: expected X,Y in points");
        return Err(exit::EDIT_REFUSED);
    };
    let (Ok(x), Ok(y)) = (x.trim().parse::<f64>(), y.trim().parse::<f64>()) else {
        eprintln!("pdfcer: --at: expected two numbers, got {spec:?}");
        return Err(exit::EDIT_REFUSED);
    };
    let pages = pdfcer_core::page_tree::pages(source_doc).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", from.display());
        exit::RUNTIME_ERROR
    })?;
    let Some(page) = pages.get(source_index) else {
        eprintln!(
            "pdfcer: {}: the collection has {} page(s), so page {} does not exist",
            from.display(),
            pages.len(),
            source_index + 1
        );
        return Err(exit::EDIT_REFUSED);
    };
    let (w, h) = (page.crop_box.width(), page.crop_box.height());
    Ok(pdfcer_core::page_tree::Rect::from_corners(
        x,
        y,
        x + w,
        y + h,
    ))
}

/// The rule-4 disclosures, on stderr before the machine-readable line.
fn disclose_placement(
    input: &Path,
    placed: &Placed,
    rect: pdfcer_core::page_tree::Rect,
    dynamic: bool,
) {
    if placed.distorted {
        // The height keeping the artwork's proportions:
        // bbox.h / bbox.w == (rect.h / sy) / (rect.w / sx), so rect.h * sx / sy.
        eprintln!(
            "pdfcer: {}: the artwork was SQUASHED to fill your rectangle - {:.3}x horizontally \
             and {:.3}x vertically. The stamp does not have the proportions it was drawn with; \
             a rectangle {:.1}x{:.1}pt would keep them.",
            input.display(),
            placed.scale_x,
            placed.scale_y,
            rect.width(),
            rect.height() * placed.scale_x / placed.scale_y,
        );
    }
    if dynamic {
        eprintln!(
            "pdfcer: {}: that is a DYNAMIC stamp. Its text is recomputed by Acrobat from form \
             scripts at placement; pdfcer placed the artwork as drawn, so what is on the page is \
             its DESIGN-TIME text.",
            input.display()
        );
    }
    if placed.widgets_ignored > 0 {
        eprintln!(
            "pdfcer: {}: {} form-field widget(s) on the stamp's page were NOT carried - only its \
             artwork was imported. (This is how a dynamic stamp's live text is left behind.)",
            input.display(),
            placed.widgets_ignored
        );
    }
    if placed.annots_ignored > 0 {
        eprintln!(
            "pdfcer: {}: {} annotation(s) on the stamp's page were NOT carried - an annotation is \
             not page content, so anything drawn as a comment on the stamp is missing from it.",
            input.display(),
            placed.annots_ignored
        );
    }
}

/// `stamp-list` — print the stamps in a collection file (`Pass 288.0`).
///
/// Reads Acrobat's own shipped collections and pdfcer's alike; the format is
/// the same file, and pdfcer has no private variant of it.
pub(crate) fn cmd_stamp_list(input: &Path) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: stamp-list: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let collection = pdfcer_core::stamp_file::read(&doc);

    if !collection.is_stamp_file() {
        // Not an error. "This PDF is not a stamp collection" is a fact about
        // the file, and a caller scripting over a folder should be able to ask
        // without handling a failure for every ordinary document.
        println!(
            "{}: not a stamp collection (no /Names /Pages name tree)",
            input.display()
        );
        return exit::SUCCESS;
    }

    println!(
        "{}: category={} stamps={}",
        input.display(),
        collection.category.as_deref().unwrap_or("(untitled)"),
        collection.stamps.len()
    );
    // `page=MISSING` is a claim about the STAMP, so it must not be printed
    // when the reason every index is absent is that the PAGE TREE would not
    // walk (`Pass 290.1`). Said wrongly, it tells an operator his signature
    // stamps are corrupt when the only damaged thing in the file is a page
    // he never sees.
    let page_tree_unreadable = collection.page_tree_error.is_some();
    if let Some(why) = &collection.page_tree_error {
        eprintln!(
            "pdfcer: {}: the page tree would not walk ({why}), so NO stamp below can be \
             resolved to a page. `page=UNKNOWN` means pdfcer could not look — it is NOT a \
             claim that the stamp names a page the file does not have.",
            input.display()
        );
    }
    for s in &collection.stamps {
        let page = s.page_index.map_or_else(
            || {
                if page_tree_unreadable {
                    "page=UNKNOWN".to_owned()
                } else {
                    // A name pointing at a page the file does not have. Named
                    // rather than hidden: it is exactly the defect
                    // `stamp-pack` refuses to create.
                    "page=MISSING".to_owned()
                }
            },
            |i| format!("page={}", i + 1),
        );
        println!(
            "  {:<28} display={:<26} {page}{}",
            s.internal,
            s.display,
            if s.dynamic { " DYNAMIC" } else { "" }
        );
    }
    if collection.stamps.iter().any(|s| s.dynamic) {
        println!(
            "note: a DYNAMIC stamp's text is recomputed by Acrobat when it is placed, from \
             AcroForm calculation scripts. pdfcer reads and reports these; it does not author \
             them, and placing one draws the design-time text."
        );
    }
    exit::SUCCESS
}

/// Print what the text-annotation generator DECIDED, when it decided
/// anything (`Pass 291.0`, project rules 4 and 11).
///
/// # Why the CLI prints instead of showing
///
/// A GUI can put an inference in a status line the operator glances at. The
/// CLI has no session: the invocation IS the commit, so the only moment this
/// can be said is on the way past. Everything below goes to **stderr**, so a
/// script parsing the stable stdout line is unaffected.
///
/// # What is deliberately NOT printed
///
/// [`pdfcer_core::annot_author::StampLabelFit::AsRequested`] -- the label fit
/// at the size that was asked for, so nobody decided anything and there is
/// nothing to report. Printing it would be telling the operator their own
/// instruction back, which is the nagging rule 4 exists to prevent.
pub(crate) fn report_text_annot_inferences(input: &Path, o: &pdfcer_core::edit::TextAnnotOutcome) {
    if let Some(fit) = o.stamp_label_fit
        && fit.is_inference()
    {
        report_stamp_label_fit(input, fit);
    }
    if let Some(size) = o.applied_autosize {
        eprintln!(
            "pdfcer: {}: the text was AUTO-SIZED to {size:.1}pt (the /DA asked for `0 Tf`, \
             so a size was chosen for you).",
            input.display()
        );
    }
    if o.unencodable_chars > 0 {
        eprintln!(
            "pdfcer: {}: {} character(s) had no WinAnsi code and were written as `?` \
             (a standard-14 face is Latin-only, ISO 32000-1 \u{a7}9.6.6.2).",
            input.display(),
            o.unencodable_chars
        );
    }
}

/// The one sentence for one `StampLabelFit`, shared by the authoring verb
/// and the restyle verb (`Pass 292.0`).
///
/// Shared deliberately rather than written twice: both verbs run the SAME fit
/// computation, so two sentences describing it would be two chances to
/// describe it differently -- and the operator would learn that a stamp
/// behaves differently when it is placed than when it is resized, which is
/// not true.
///
/// The caller checks `is_inference()`; this function says nothing for
/// `AsRequested` even if called.
pub(crate) fn report_stamp_label_fit(input: &Path, fit: pdfcer_core::annot_author::StampLabelFit) {
    use pdfcer_core::annot_author::StampLabelFit;
    match fit {
        StampLabelFit::BoxGrown { width, .. } => eprintln!(
            "pdfcer: {}: the stamp box was WIDENED to {width:.1}pt to hold the label \
             (fit=grow). The rectangle written is not the one you gave.",
            input.display()
        ),
        StampLabelFit::LabelShrunk { size, requested } => eprintln!(
            "pdfcer: {}: the stamp label was SHRUNK to {size:.1}pt (you asked for \
             {requested:.1}pt) so it would fit the box (fit=shrink). The size on the page \
             is pdfcer's, not yours.",
            input.display()
        ),
        StampLabelFit::LabelClipped {
            hidden_chars,
            overflow,
            ..
        } => eprintln!(
            "pdfcer: {}: the stamp label does NOT fit its box and was CLIPPED \
             (fit=clip): {hidden_chars} character(s) are not fully on the page, and the \
             label is {overflow:.1}pt wider than the space available.",
            input.display()
        ),
        // `AsRequested` decided nothing, and the wildcard is forced:
        // `StampLabelFit` is `#[non_exhaustive]`, so a match in a downstream
        // crate cannot be exhaustive however carefully it is written.
        //
        // A future variant therefore lands here SILENTLY rather than as a
        // compile error, while `is_inference()` (which is
        // `!matches!(self, AsRequested)`) will have already decided it is
        // worth reporting. If you add a variant, add its sentence here in the
        // same commit.
        _ => {}
    }
}

/// `stamp-pack` — name a PDF's pages as stamps (`Pass 288.0`).
pub(crate) fn cmd_stamp_pack(
    input: &Path,
    category: &str,
    stamps: &[String],
    stamps_from: Option<&Path>,
    output: &Path,
) -> u8 {
    // The name list, from `--stamps-from` when given. Blank lines and `#`
    // comments are skipped so a downloaded artwork sheet's name list can carry
    // section headings the way a human would write one.
    let from_file: Vec<String> = match stamps_from {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(str::to_owned)
                .collect(),
            Err(err) => {
                eprintln!("pdfcer: stamp-pack: {}: {err}", path.display());
                return exit::RUNTIME_ERROR;
            }
        },
        None => Vec::new(),
    };
    let stamps: &[String] = if stamps_from.is_some() {
        &from_file
    } else {
        stamps
    };
    if stamps.is_empty() {
        eprintln!("pdfcer: stamp-pack: no stamp names given -- the name list is empty.");
        return exit::RUNTIME_ERROR;
    }
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: stamp-pack: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let mut session = pdfcer_core::edit::EditSession::new(doc);

    // `Internal=Display`, or just `Display` — in which case the internal name
    // is the display name with spaces removed, which is the shape Adobe's own
    // files use (`SBForPublicRelease` / `For Public Release`).
    let parsed: Vec<(String, String)> = stamps
        .iter()
        .map(|s| match s.split_once('=') {
            Some((i, d)) => (i.to_owned(), d.to_owned()),
            None => (s.replace(' ', ""), s.clone()),
        })
        .collect();

    let written = match pdfcer_core::stamp_file::name_stamp_pages(&mut session, &parsed) {
        Ok(w) => w,
        Err(err) => {
            eprintln!("pdfcer: stamp-pack: naming the stamp pages: {err}");
            return exit::RUNTIME_ERROR;
        }
    };

    if let Err(err) = session.set_info_field(pdfcer_core::edit::InfoField::Title, Some(category)) {
        eprintln!("pdfcer: stamp-pack: setting the category name: {err}");
        return exit::RUNTIME_ERROR;
    }

    let bytes = match session.to_full_bytes(&pdfcer_core::writer::SaveOptions::default()) {
        Ok((bytes, _)) => bytes,
        Err(err) => {
            eprintln!("pdfcer: stamp-pack: saving the collection: {err}");
            return exit::RUNTIME_ERROR;
        }
    };
    if let Err(err) = write_output(output, bytes) {
        eprintln!("pdfcer: stamp-pack: {}: {err}", output.display());
        return exit::RUNTIME_ERROR;
    }

    println!(
        "{}: category={category} stamps_named={}",
        output.display(),
        written.stamps_named
    );
    for skipped in &written.skipped {
        // Disclosed, never silent: this stamp named a page the document does
        // not have, so it was NOT written.
        println!("  SKIPPED {skipped} -- the document has no such page");
    }
    exit::SUCCESS
}
