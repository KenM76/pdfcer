use super::*;

/// The scaling modes, as command-line words.
///
/// A separate type from `pdfcer_print::ScaleMode` because that one carries a
/// free-form `Custom(f64)` which clap cannot express as a value-enum
/// variant, and because the CLI's vocabulary is allowed to differ from
/// the engine's — `shrink` reads better than `ShrinkOversized` in a
/// shell.
/// `--binding` on `print`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum BindingArg {
    /// Side-by-side halves, bound on the left.
    Left,
    /// Side-by-side halves, bound on the right.
    Right,
    /// Stacked halves, bound on the left (a horizontal fold).
    LeftTall,
    /// Stacked halves, bound on the right.
    RightTall,
}

impl BindingArg {
    /// The imposition type this maps to.
    // Consumed only by the `#[cfg(windows)]` `cmd_print`; the arg type
    // itself is parsed on every platform so `--help` is identical
    // everywhere, which is why the mapping is relaxed rather than gated.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) const fn to_binding(self) -> pdfcer_print::imposition::Binding {
        match self {
            Self::Left => pdfcer_print::imposition::Binding::Left,
            Self::Right => pdfcer_print::imposition::Binding::Right,
            Self::LeftTall => pdfcer_print::imposition::Binding::LeftTall,
            Self::RightTall => pdfcer_print::imposition::Binding::RightTall,
        }
    }
}

/// `--booklet-subset` on `print`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum BookletSubsetArg {
    /// Every face.
    BothSides,
    /// Front faces only — the first pass on a printer without duplex.
    FrontOnly,
    /// Back faces only — the second pass, after re-feeding.
    BackOnly,
}

impl BookletSubsetArg {
    /// The imposition type this maps to.
    // Consumed only by the `#[cfg(windows)]` `cmd_print`; the arg type
    // itself is parsed on every platform so `--help` is identical
    // everywhere, which is why the mapping is relaxed rather than gated.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) const fn to_subset(self) -> pdfcer_print::imposition::BookletSubset {
        match self {
            Self::BothSides => pdfcer_print::imposition::BookletSubset::BothSides,
            Self::FrontOnly => pdfcer_print::imposition::BookletSubset::FrontOnly,
            Self::BackOnly => pdfcer_print::imposition::BookletSubset::BackOnly,
        }
    }
}

/// `--comments` on `print` — which annotation classes reach the paper.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum CommentsArg {
    /// Page content and form fields only, no review markup. The
    /// DEFAULT, matching Reader rather than Acrobat Pro: a comment
    /// reaching paper unasked is the costlier mistake.
    Document,
    /// Page content plus all markup annotations.
    Markups,
    /// Page content plus stamps only — narrower than markup.
    Stamps,
    /// Form fields alone. The page itself is NOT printed, which is the
    /// point: this is for printing onto a pre-printed form.
    FieldsOnly,
}

impl CommentsArg {
    /// The render type this maps to.
    // Consumed only by the `#[cfg(windows)]` `cmd_print`; the arg type
    // itself is parsed on every platform so `--help` is identical
    // everywhere, which is why the mapping is relaxed rather than gated.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) const fn to_scope(self) -> pdfcer_render::AnnotationScope {
        match self {
            Self::Document => pdfcer_render::AnnotationScope::Document,
            Self::Markups => pdfcer_render::AnnotationScope::DocumentAndMarkups,
            Self::Stamps => pdfcer_render::AnnotationScope::DocumentAndStamps,
            Self::FieldsOnly => pdfcer_render::AnnotationScope::FormFieldsOnly,
        }
    }
}

/// `--orientation` on `print`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum OrientationArg {
    /// Decide per page from its own aspect ratio.
    Auto,
    /// Force portrait.
    Portrait,
    /// Force landscape.
    Landscape,
}

impl OrientationArg {
    /// The core type this maps to.
    // Consumed only by the `#[cfg(windows)]` `cmd_print`; the arg type
    // itself is parsed on every platform so `--help` is identical
    // everywhere, which is why the mapping is relaxed rather than gated.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) const fn to_orientation(self) -> pdfcer_print::Orientation {
        match self {
            Self::Auto => pdfcer_print::Orientation::Auto,
            Self::Portrait => pdfcer_print::Orientation::Portrait,
            Self::Landscape => pdfcer_print::Orientation::Landscape,
        }
    }
}

/// `--duplex` on `print`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum DuplexArg {
    /// One side only.
    Simplex,
    /// Flip on the long edge (book binding).
    LongEdge,
    /// Flip on the short edge (notepad binding).
    ShortEdge,
}

impl DuplexArg {
    /// The core type this maps to.
    // Consumed only by the `#[cfg(windows)]` `cmd_print`; the arg type
    // itself is parsed on every platform so `--help` is identical
    // everywhere, which is why the mapping is relaxed rather than gated.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) const fn to_duplex(self) -> pdfcer_print::Duplex {
        match self {
            Self::Simplex => pdfcer_print::Duplex::Simplex,
            Self::LongEdge => pdfcer_print::Duplex::LongEdge,
            Self::ShortEdge => pdfcer_print::Duplex::ShortEdge,
        }
    }
}

/// `--subset` on `print`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SubsetArg {
    /// Every selected page.
    All,
    /// Only odd DOCUMENT page numbers.
    Odd,
    /// Only even DOCUMENT page numbers.
    Even,
}

impl SubsetArg {
    /// The core type this maps to.
    // Consumed only by the `#[cfg(windows)]` `cmd_print`; the arg type
    // itself is parsed on every platform so `--help` is identical
    // everywhere, which is why the mapping is relaxed rather than gated.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) const fn to_subset(self) -> pdfcer_print::PageSubset {
        match self {
            Self::All => pdfcer_print::PageSubset::All,
            Self::Odd => pdfcer_print::PageSubset::Odd,
            Self::Even => pdfcer_print::PageSubset::Even,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum PrintScaleArg {
    /// Scale to fill the printable area, enlarging a small page.
    /// Reader's own default.
    Fit,
    /// 1 PDF point = 1/72 inch on paper, clipping if it must.
    Actual,
    /// Actual size, except reduce a page too big for the sheet. Never
    /// enlarges — which is the whole difference from fit.
    Shrink,
}

impl PrintScaleArg {
    /// The print-spooler scale mode this word names.
    // Consumed only by the `#[cfg(windows)]` `cmd_print`; the arg type
    // itself is parsed on every platform so `--help` is identical
    // everywhere, which is why the mapping is relaxed rather than gated.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn to_mode(self) -> pdfcer_print::ScaleMode {
        match self {
            Self::Fit => pdfcer_print::ScaleMode::Fit,
            Self::Actual => pdfcer_print::ScaleMode::ActualSize,
            Self::Shrink => pdfcer_print::ScaleMode::ShrinkOversized,
        }
    }

    /// `#[cfg(windows)]` because every caller is: the mode name is only
    /// ever printed on a result line that reports a real device, and the
    /// non-Windows build has no device to report. Without the gate this is
    /// a dead-code warning that `-D warnings` turns into a failed build —
    /// which was invisible for as long as the crate did not compile on
    /// non-Windows at all.
    #[cfg(windows)]
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Fit => "fit",
            Self::Actual => "actual",
            Self::Shrink => "shrink",
        }
    }
}

/// `print` — send pages to a printer.
///
/// # It does a DRY RUN unless told otherwise
///
/// `--send` is required to start a job. Without it every step runs —
/// device context, capability query, placement, rasterisation, the page
/// loop — and stops before `StartDoc`.
///
/// That default is the opposite of most tools and is deliberate. Printing
/// is irreversible, consumes a physical resource, and occupies a device
/// other people may be waiting for. A command whose safe mode is the one
/// you get by *not* thinking is a command that fails safely for the
/// person who mistyped a page range at 2 a.m.
///
/// It also makes the command testable by its own author on a machine
/// with one printer whose owner is sitting at it — which is how this was
/// written.
///
/// # Rasterised, and it says so
///
/// pdfcer renders each page to pixels and sends the bitmap. Reader sends
/// vector and text to the driver and lets it RIP, keeping "print as
/// image" as an explicitly-invoked fallback for driver bugs
/// (`printing__rendering_pipeline_and_resolution.md`).
///
/// So pdfcer's default IS Reader's fallback. On a CAD drawing that is
/// visibly coarser than the driver's own output, and an operator
/// printing a drawing needs telling before the paper comes out, not
/// after. The result line says `mode=raster` on every run for that
/// reason.
// Twelve arguments, five over clippy's bound. They are `clap`'s own
// parsed flags handed straight through, and bundling them into a struct
// would mean a second definition of the command's surface that has to be
// kept in step with the derive — the same reasoning `interpret::run`
// carries for its decomposed render inputs.
#[allow(clippy::too_many_arguments)]
#[cfg(windows)]
pub(crate) fn cmd_print(
    input: &Path,
    printer: Option<&str>,
    scale: PrintScaleArg,
    scale_percent: Option<u32>,
    pages_spec: &str,
    send: bool,
    dpi_cap: u32,
    to_file: Option<PathBuf>,
    copies: u16,
    uncollated: bool,
    subset: SubsetArg,
    reverse: bool,
    orientation: OrientationArg,
    duplex: DuplexArg,
    pick_tray: bool,
    paper: Option<&str>,
    paper_size: Option<&str>,
    printer_config: Option<&Path>,
    comments: CommentsArg,
    n_up: Option<u32>,
    n_up_border: bool,
    booklet: bool,
    poster: bool,
    poster_scale: f64,
    poster_overlap: f64,
    poster_large_only: bool,
    poster_max_tiles: u32,
    binding: BindingArg,
    booklet_subset: BookletSubsetArg,
    line_width_mm: Option<f64>,
    poster_cut_marks: bool,
    poster_labels: bool,
) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let name = match print_target(printer) {
        Ok(name) => name,
        Err(code) => return code,
    };
    let session = pdfcer_core::edit::EditSession::new(doc);
    let page_list = match session.pages() {
        Ok(p) => p,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let selected = match parse_pages(pages_spec, page_list.len()) {
        Ok(s) => s,
        Err(msg) => {
            eprintln!("pdfcer: {msg}");
            return exit::RUNTIME_ERROR;
        }
    };
    let config = match printer_config {
        Some(path) => match load_printer_config(path, &name) {
            Ok(config) => Some(config),
            Err(code) => return code,
        },
        None => None,
    };
    let (selected_paper, requested_sheet_pt) = match resolve_paper(&name, paper, paper_size) {
        Ok(paper) => paper,
        Err(code) => return code,
    };
    // The geometry must be read for the sheet THIS JOB will use, not
    // the device's default one. Planning against the default while
    // printing on another is the same defect `for_orientation` exists to
    // prevent, in a second dimension: the two halves would describe
    // different sheets, with no clip reported and nothing to explain it.
    let caps = match pdfcer_print::printer_caps_for(&name, config.as_ref(), selected_paper) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("pdfcer: {err}");
            return exit::EDIT_REFUSED;
        }
    };
    report_sheet_mismatch(&name, requested_sheet_pt, &caps);

    // R83 — a control the device may not honour is disclosed rather than
    // left to be discovered from the paper. Both of these produce a job
    // that succeeds and comes out wrong, which is the only kind the
    // operator cannot diagnose.
    if let Ok(features) = pdfcer_print::device_features(&name) {
        if duplex.to_duplex() != pdfcer_print::Duplex::Simplex && !features.supports_duplex {
            eprintln!(
                "pdfcer: {name:?} does not report duplex support, so this job will very \
                 likely print single-sided. pdfcer never simulates duplex by reordering pages."
            );
        }
        if pick_tray {
            match features.form_source_bin {
                pdfcer_print::FormSourceSupport::Listed => {}
                // NOT a refusal. Measured 2026-08-18: "Microsoft Print to
                // PDF" reports no bin list at all and its own default is
                // already DMBIN_FORMSOURCE, so treating silence as "no"
                // would deny a working capability.
                pdfcer_print::FormSourceSupport::NotListed
                | pdfcer_print::FormSourceSupport::Unknown => eprintln!(
                    "pdfcer: {name:?} does not advertise a size-matched input tray. The \
                     request is sent anyway — Windows' Form-to-Tray Assignment is a spooler \
                     feature and several drivers honour it without listing it — but if the \
                     paper comes from the usual tray, that is why."
                ),
            }
        }
    }

    let device_settings = pdfcer_print::DeviceSettings {
        orientation: orientation.to_orientation(),
        duplex: duplex.to_duplex(),
        pick_tray_by_page_size: pick_tray,
        paper: selected_paper,
    };
    let mode = match scale_percent {
        Some(pct) => pdfcer_print::ScaleMode::Custom(f64::from(pct) / 100.0),
        None => scale.to_mode(),
    };
    // The placement and resolution arithmetic comes from `pdfcer-print`
    // rather than being repeated here, so the GUI and the CLI cannot
    // come to disagree about where a page lands — the drift whose
    // symptom is a GUI print landing differently from a CLI print of the
    // same document, which nobody thinks to compare.
    // The DISPLAYED size, not the raw media box: /Rotate is a display
    // rotation the renderer honours, so a page that is portrait in the
    // file and landscape on screen must be planned as landscape or the
    // placement and the pixels describe different shapes. See
    // `displayed_page_size` for what went wrong before this.
    let page_sizes: Vec<(f64, f64)> = page_list
        .iter()
        .map(|p| {
            let mb = p.media_box;
            pdfcer_print::displayed_page_size(
                ((mb.urx - mb.llx).abs(), (mb.ury - mb.lly).abs()),
                i32::from(p.rotate),
            )
        })
        .collect();
    let spec = pdfcer_print::JobSpec {
        pages: selected.clone(),
        mode,
        max_dpi: dpi_cap,
        subset: subset.to_subset(),
        reverse,
        copies,
        collate: if uncollated {
            pdfcer_print::Collate::Uncollated
        } else {
            pdfcer_print::Collate::Collated
        },
    };
    // TURNED for this job before ANY layout is computed against it —
    // and it must be built after `spec`, because the page that decides
    // `--orientation auto` is the first page the SEQUENCE sends, not
    // `pages[0]`.
    //
    // `printer_caps` reports the device's default `DEVMODE`, so on a
    // portrait-default printer it hands back a portrait printable area
    // while a landscape job prints on a sheet the driver has turned.
    // Every consumer below reads `device.printable_pt` — plain placement,
    // n-up cells, poster tiles, booklet halves — so turning it here is
    // what keeps all four honest rather than four separate fixes that
    // would eventually disagree.
    let device = pdfcer_print::DeviceGeometry::from_caps(
        &caps,
        device_settings.orientation,
        spec.first_page_pt(&page_sizes),
    );
    // ---- The three job-shape modes are mutually exclusive ----
    //
    // N-up, booklet and poster each REMAP the job rather than scale it,
    // and no two of them compose. Before this guard existed the three
    // branches ran in sequence and the last one to fire silently
    // overwrote the others' work: `--poster --booklet` composed nine
    // poster tiles, threw them away, and printed a booklet. The operator
    // got a plausible job that was not the one they asked for, with no
    // indication anything had been discarded.
    //
    // Refusing is right rather than picking a precedence. There is no
    // reading of `--poster --booklet` that is obviously intended, so any
    // precedence pdfcer chose would be a guess presented as a result.
    {
        let modes = [
            (n_up.is_some(), "--n-up"),
            (booklet, "--booklet"),
            (poster, "--poster"),
        ];
        let named: Vec<&str> = modes
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, n)| *n)
            .collect();
        if named.len() > 1 {
            eprintln!(
                "pdfcer: {} cannot be combined — each one changes the shape of the job, and no two of them compose. Pick one.",
                named.join(" and ")
            );
            return exit::EDIT_REFUSED;
        }
    }

    let resolution = pdfcer_print::job_resolution(&device, &spec);
    let plans = pdfcer_print::plan_job(&device, &page_sizes, &spec);
    let dpi = resolution.dpi;
    let capped = resolution.capped;
    let clipped = plans.iter().filter(|p| p.placement.clipped).count();
    let mut bitmaps: Vec<pdfcer_print::PageBitmap> = Vec::new();

    // Every render in this job uses these options, so a line-width choice
    // reaches N-up cells, booklet halves and poster tiles alike.
    let print_options = print_render_options(comments, line_width_mm, dpi);
    if let (Some(mm), pdfcer_render::StrokeDisplay::Fixed { device_px }) =
        (line_width_mm, print_options.stroke_display)
    {
        eprintln!("pdfcer: every line prints {mm} mm wide ({device_px:.2} px at {dpi} dpi).");
    }

    // ---- N-up: several source pages composited onto one sheet ----
    //
    // Handled as its own path rather than as another `ScaleMode`,
    // because it changes the SHAPE of the job: N source pages become one
    // sheet, so the one-plan-per-page arithmetic above no longer
    // describes it. Trying to express that as a placement would mean a
    // plan whose `index` is a lie.
    if let Some(count) = n_up {
        let nup = pdfcer_print::imposition::NUpSpec {
            grid: pdfcer_print::imposition::NUpGrid::Count(count),
            order: pdfcer_print::imposition::PageOrder::Horizontal,
            border: n_up_border,
            auto_rotate: true,
        };
        let sequence = spec.sequence();
        let ordered_sizes: Vec<(f64, f64)> = sequence
            .iter()
            .filter_map(|&i| page_sizes.get(i).copied())
            .collect();
        let layout =
            match pdfcer_print::imposition::plan_n_up(device.printable_pt, &ordered_sizes, &nup) {
                Ok(l) => l,
                Err(err) => {
                    eprintln!("pdfcer: {err}");
                    return exit::RUNTIME_ERROR;
                }
            };
        let mut sheets: Vec<pdfcer_print::PageBitmap> = Vec::new();
        for sheet_index in 0..layout.sheets {
            // One pixmap per SHEET, at the device resolution, with each
            // source page drawn into its own cell. Compositing here
            // rather than sending one blit per cell keeps the spooler
            // loop unchanged — it still sees one bitmap per physical
            // sheet, which is what a sheet is.
            let px = |pt: f64| (pt * f64::from(resolution.dpi) / 72.0).round().max(1.0) as u32;
            let (sw, sh) = (px(device.printable_pt.0), px(device.printable_pt.1));
            let Some(mut sheet) = pdfcer_render::tiny_skia::Pixmap::new(sw, sh) else {
                eprintln!("pdfcer: a sheet of {sw}x{sh} pixels is too large to compose");
                return exit::RUNTIME_ERROR;
            };
            sheet.fill(pdfcer_render::tiny_skia::Color::WHITE);
            for slot in layout.slots.iter().filter(|s| s.sheet == sheet_index) {
                let Some(&source) = sequence.get(slot.source) else {
                    continue;
                };
                let (Some(page), Some(&size)) = (page_list.get(source), page_sizes.get(source))
                else {
                    continue;
                };
                let scale = (f64::from(resolution.dpi) / 72.0) * slot.fit.scale;
                let options = print_options.clone();
                let rendered = match pdfcer_render::render_page_with_view(
                    &session.view(),
                    page,
                    scale as f32,
                    &options,
                ) {
                    Ok(r) => r,
                    Err(err) => {
                        eprintln!("pdfcer: page {}: {err}", source + 1);
                        return exit::RUNTIME_ERROR;
                    }
                };
                let _ = size;
                sheet.draw_pixmap(
                    px(slot.fit.rect.x) as i32,
                    px(slot.fit.rect.y) as i32,
                    rendered.pixmap.as_ref(),
                    &pdfcer_render::tiny_skia::PixmapPaint::default(),
                    pdfcer_render::tiny_skia::Transform::identity(),
                    None,
                );
            }
            sheets.push(pdfcer_print::PageBitmap {
                width: sheet.width(),
                height: sheet.height(),
                rgba: sheet.data().to_vec(),
                // The sheet is already the printable area at device
                // resolution, so it is placed 1:1 with no further
                // scaling — the imposition did the fitting.
                placement: pdfcer_print::Placement {
                    scale: 1.0,
                    offset_x_pt: 0.0,
                    offset_y_pt: 0.0,
                    clipped: false,
                },
                page_pt: device.printable_pt,
            });
        }
        bitmaps = sheets;
    }

    // ---- Poster: ONE page tiled across MANY sheets ----
    //
    // The inverse of N-up, and its own path for the same reason: it
    // changes the SHAPE of the job. N-up puts many pages on one sheet by
    // scaling them into cells; poster puts one page on many sheets by
    // cropping it into tiles, and no `Placement` expresses a crop.
    //
    // Planned PER PAGE rather than once for the document, because
    // `plan_poster` takes one page size: a document whose pages differ in
    // size tiles each to its own grid, which is the only answer that does
    // not silently letterbox the odd one out.
    if poster {
        let spec_p = pdfcer_print::imposition::PosterSpec {
            tile_scale: poster_scale,
            overlap_pt: poster_overlap,
            cut_marks: poster_cut_marks,
            labels: poster_labels,
            tile_only_large_pages: poster_large_only,
            max_tiles: poster_max_tiles,
        };
        // The sheet raster size now lives in `poster_sheets_for_page`, which
        // is where the sheets are actually composed. It used to be computed
        // here and threaded in, which is how a whole-page raster ended up
        // being the thing that scaled with magnification.
        let label_name = input.file_name().map_or_else(
            || input.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        if poster_labels {
            let (_, replaced) = winansi_bytes(&label_name);
            if replaced > 0 {
                eprintln!(
                    "pdfcer: {replaced} character(s) of the file name have no glyph in the label font and print as '?'."
                );
            }
        }
        let mut sheets: Vec<pdfcer_print::PageBitmap> = Vec::new();
        let mut tiled_pages = 0usize;
        let mut untiled_pages = 0usize;

        for &index in &spec.sequence() {
            let (Some(page), Some(&size)) = (page_list.get(index), page_sizes.get(index)) else {
                continue;
            };
            // `tile_only_large_pages` asks the planner, not this loop: the
            // predicate is the planner's to own so the CLI and the GUI
            // cannot disagree about what counts as "large" (R171 — read the
            // value off the one place that owns it, never restate it).
            if !spec_p.tiles_page(device.printable_pt, size) {
                untiled_pages += 1;
                // Printed at its natural placement, in sequence, so a
                // mixed document comes off the printer in reading order
                // rather than with the small pages collected at the end.
                let render_scale = f64::from(resolution.dpi) / 72.0;
                let options = print_options.clone();
                let rendered = match pdfcer_render::render_page_with_view(
                    &session.view(),
                    page,
                    render_scale as f32,
                    &options,
                ) {
                    Ok(r) => r,
                    Err(err) => {
                        eprintln!("pdfcer: page {}: {err}", index + 1);
                        return exit::RUNTIME_ERROR;
                    }
                };
                sheets.push(pdfcer_print::PageBitmap {
                    width: rendered.pixmap.width(),
                    height: rendered.pixmap.height(),
                    rgba: rendered.pixmap.data().to_vec(),
                    placement: pdfcer_print::Placement {
                        scale: 1.0,
                        offset_x_pt: 0.0,
                        offset_y_pt: 0.0,
                        clipped: false,
                    },
                    page_pt: size,
                });
                continue;
            }
            let layout =
                match pdfcer_print::imposition::plan_poster(device.printable_pt, size, &spec_p) {
                    Ok(l) => l,
                    Err(err) => {
                        eprintln!("pdfcer: page {}: {err}", index + 1);
                        return exit::RUNTIME_ERROR;
                    }
                };
            tiled_pages += 1;
            let options = print_options.clone();
            match poster_sheets_for_page(
                &session.view(),
                page,
                &layout,
                spec_p.tile_scale,
                resolution.dpi,
                device.printable_pt,
                &options,
                &label_name,
            ) {
                Ok((tile_sheets, route)) => {
                    if matches!(route, PosterRoute::PerTile) {
                        // Rule 11 / rule 4: the CLI prints what it had to
                        // do differently. A page the recorder refuses is
                        // re-interpreted once per sheet, which on a 4x5
                        // poster is twenty walks of the content stream —
                        // slow enough that an operator wondering why should
                        // be told rather than left to guess.
                        eprintln!(
                            "pdfcer: page {}: this page cannot be cached for tiling \
                             (it uses a shading, an overprint composite or a soft mask), \
                             so each of its {} sheets re-reads the page. Output is \
                             unaffected; this is a speed note.",
                            index + 1,
                            tile_sheets.len()
                        );
                    }
                    sheets.extend(tile_sheets);
                }
                Err(err) => {
                    eprintln!("pdfcer: page {}: {err}", index + 1);
                    return exit::RUNTIME_ERROR;
                }
            }
            eprintln!(
                "pdfcer: page {}: poster of {} x {} tiles ({} sheet(s)), assembled size \
{:.0} x {:.0} pt, {:.0} pt overlap.",
                index + 1,
                layout.columns,
                layout.rows,
                layout.tiles.len(),
                layout.poster_pt.0,
                layout.poster_pt.1,
                layout.overlap_pt,
            );
        }
        if untiled_pages > 0 {
            eprintln!(
                "pdfcer: {untiled_pages} page(s) already fit the paper and were printed \
untiled; {tiled_pages} page(s) were tiled."
            );
        }
        bitmaps = sheets;
    }

    // ---- Booklet: folded imposition, two page-halves per sheet face ----
    //
    // Its own path for the same reason N-up is: it changes the SHAPE of
    // the job. A booklet is not a scaling of the page sequence, it is a
    // REMAPPING of it — sheet 1 carries the last page beside the first —
    // and no `Placement` can express that.
    //
    // The blank positions are real slots with no source. They are
    // rendered as empty sheet halves rather than skipped, because a
    // booklet's blanks are structural: dropping them shortens the fold
    // and every subsequent sheet carries the wrong pages.
    if booklet {
        let spec_b = pdfcer_print::imposition::BookletSpec {
            binding: binding.to_binding(),
            subset: booklet_subset.to_subset(),
            sheets: None,
            auto_rotate: true,
        };
        let sequence = spec.sequence();
        let ordered_sizes: Vec<(f64, f64)> = sequence
            .iter()
            .filter_map(|&i| page_sizes.get(i).copied())
            .collect();
        let layout = match pdfcer_print::imposition::plan_booklet(
            device.printable_pt,
            &ordered_sizes,
            &spec_b,
        ) {
            Ok(l) => l,
            Err(err) => {
                eprintln!("pdfcer: {err}");
                return exit::RUNTIME_ERROR;
            }
        };
        let px = |pt: f64| (pt * f64::from(resolution.dpi) / 72.0).round().max(1.0) as u32;
        let (sw, sh) = (px(device.printable_pt.0), px(device.printable_pt.1));
        let mut faces: Vec<pdfcer_print::PageBitmap> = Vec::new();
        // One bitmap per SHEET FACE, in the order they must be fed.
        let mut keys: Vec<(usize, bool)> = layout
            .slots
            .iter()
            .map(|s| {
                (
                    s.sheet,
                    matches!(s.side, pdfcer_print::imposition::BookletSide::Back),
                )
            })
            .collect();
        keys.sort_unstable();
        keys.dedup();
        for (sheet_no, is_back) in keys {
            let Some(mut face) = pdfcer_render::tiny_skia::Pixmap::new(sw, sh) else {
                eprintln!("pdfcer: a sheet of {sw}x{sh} pixels is too large to compose");
                return exit::RUNTIME_ERROR;
            };
            face.fill(pdfcer_render::tiny_skia::Color::WHITE);
            for slot in layout.slots.iter().filter(|s| {
                s.sheet == sheet_no
                    && matches!(s.side, pdfcer_print::imposition::BookletSide::Back) == is_back
            }) {
                let (Some(source_pos), Some(fit)) = (slot.source, slot.fit) else {
                    // A structural blank. The half stays white.
                    continue;
                };
                let Some(&source) = sequence.get(source_pos) else {
                    continue;
                };
                let Some(page) = page_list.get(source) else {
                    continue;
                };
                let scale = (f64::from(resolution.dpi) / 72.0) * fit.scale;
                let options = print_options.clone();
                let rendered = match pdfcer_render::render_page_with_view(
                    &session.view(),
                    page,
                    scale as f32,
                    &options,
                ) {
                    Ok(r) => r,
                    Err(err) => {
                        eprintln!("pdfcer: page {}: {err}", source + 1);
                        return exit::RUNTIME_ERROR;
                    }
                };
                face.draw_pixmap(
                    px(fit.rect.x) as i32,
                    px(fit.rect.y) as i32,
                    rendered.pixmap.as_ref(),
                    &pdfcer_render::tiny_skia::PixmapPaint::default(),
                    pdfcer_render::tiny_skia::Transform::identity(),
                    None,
                );
            }
            faces.push(pdfcer_print::PageBitmap {
                width: face.width(),
                height: face.height(),
                rgba: face.data().to_vec(),
                placement: pdfcer_print::Placement {
                    scale: 1.0,
                    offset_x_pt: 0.0,
                    offset_y_pt: 0.0,
                    clipped: false,
                },
                page_pt: device.printable_pt,
            });
        }
        eprintln!(
            "pdfcer: booklet of {} sheet(s), {} page(s) after padding, {} blank position(s). \
             Print two-sided on the long edge, or print one side and re-feed.",
            layout.total_sheets, layout.padded_pages, layout.blank_positions
        );
        bitmaps = faces;
    }

    // PER-PAGE geometry in the plain path.
    //
    // `plans` above was computed against ONE `device`, turned for the
    // first page. That is right for the imposition paths, where a sheet
    // has one shape by construction, and wrong here: a CAD set with a
    // portrait title sheet and landscape drawings behind it needs each
    // page placed on the sheet IT will print on. Re-placing per page and
    // sending the matching setup with each is what makes
    // `--orientation auto` do what `Orientation`'s documentation has
    // always claimed.
    //
    // The device geometry is derived, not re-read: `from_caps` turns the
    // reported sheet, so this costs no extra Win32 round trip and — more
    // importantly — goes through the ONE place rotation is written.
    let mut setups: Vec<pdfcer_print::SheetSetup> = Vec::new();
    if n_up.is_none() && !booklet && !poster {
        for plan in &plans {
            let (Some(page), Some(&size)) = (page_list.get(plan.index), page_sizes.get(plan.index))
            else {
                continue;
            };
            let sheet_device =
                pdfcer_print::DeviceGeometry::from_caps(&caps, device_settings.orientation, size);
            let placement = pdfcer_print::place_page(size, sheet_device.printable_pt, mode);
            // The rasterisation scale follows the same rule `plan_job`
            // uses, and reads its DPI from the per-page geometry — which
            // is the same DPI, because `for_orientation` deliberately
            // does NOT swap it (a 600x300 plotter must not be rendered
            // as 300x600).
            let render_scale = (f64::from(resolution.dpi) / 72.0) * placement.scale;
            let options = print_options.clone();
            let rendered = match pdfcer_render::render_page_with_view(
                &session.view(),
                page,
                render_scale as f32,
                &options,
            ) {
                Ok(r) => r,
                Err(err) => {
                    eprintln!("pdfcer: page {}: {err}", plan.index + 1);
                    return exit::RUNTIME_ERROR;
                }
            };
            bitmaps.push(pdfcer_print::PageBitmap {
                width: rendered.pixmap.width(),
                height: rendered.pixmap.height(),
                rgba: rendered.pixmap.data().to_vec(),
                placement,
                page_pt: size,
            });
            setups.push(pdfcer_print::SheetSetup {
                orientation: pdfcer_print::resolve_orientation(device_settings.orientation, size),
                paper: selected_paper,
            });
        }
    }

    let dry = if send {
        pdfcer_print::DryRun::No
    } else {
        pdfcer_print::DryRun::Yes
    };
    // The imposition paths hand the spooler one bitmap per SHEET whose
    // `page_pt` is the printable area rather than a source page, so
    // `Auto` must NOT be re-derived from it there — it is resolved once,
    // from the page the layout was planned against, and every sheet
    // carries that same answer. The plain path above filled `setups`
    // with a per-page answer instead.
    let job_orientation = pdfcer_print::resolve_orientation(
        device_settings.orientation,
        spec.first_page_pt(&page_sizes),
    );
    let sheets: Vec<pdfcer_print::Sheet<'_>> = bitmaps
        .iter()
        .enumerate()
        .map(|(i, bitmap)| pdfcer_print::Sheet {
            bitmap,
            setup: setups.get(i).copied().unwrap_or(pdfcer_print::SheetSetup {
                orientation: job_orientation,
                paper: selected_paper,
            }),
        })
        .collect();
    let report = match pdfcer_print::spool_sheets(
        &name,
        &sheets,
        dry,
        to_file.as_deref(),
        device_settings,
        config.as_ref(),
    ) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("pdfcer: {err}");
            return exit::RUNTIME_ERROR;
        }
    };

    if capped {
        eprintln!(
            "pdfcer: rendering at {dpi} DPI, below the printer's {}x{}. A full-resolution page \
             costs about {} MB of memory each; raise --max-dpi if you need the detail and have \
             the memory.",
            caps.dpi_x,
            caps.dpi_y,
            resolution.uncapped_page_mb()
        );
    }
    if clipped > 0 {
        eprintln!(
            "pdfcer: {clipped} page(s) do not fit the printable area and will lose content off \
             the edges. Acrobat clips silently here; pdfcer says so. Use --scale fit to avoid it."
        );
    }
    if !report.printed {
        eprintln!(
            "pdfcer: DRY RUN — nothing was printed and no job was queued. Everything up to \
             starting the job ran against the real device. Add --send to print."
        );
    }

    // Rule 4: where the driver settings came from is not visible on the
    // paper, and it decides what a driver-level request could possibly
    // honour. `synthesised` in particular means the driver would not
    // describe itself and everything it holds that pdfcer does not model
    // was NOT carried.
    // A mixed job reconfigures the device part-way, which is invisible
    // in a page count and is the operator's evidence that `auto` did the
    // per-page thing rather than reading page 1 and applying it to
    // everything — the defect this replaced.
    if report.sheet_setups > 1 {
        eprintln!(
            "pdfcer: this job uses {} different sheet setups — the pages do not all print \
             the same way up or on the same paper, so the device is reconfigured part-way \
             through the job.",
            report.sheet_setups
        );
    }
    if report.settings_source == pdfcer_print::SettingsSource::Synthesised {
        eprintln!(
            "pdfcer: {name:?} would not report its own settings, so this job carried only \
             the settings pdfcer sets itself. Anything configured in the driver — media type, \
             output bin, quality, stapling — was not included."
        );
    }
    println!(
        "print {} printer={name:?} pages={} printed={} dpi={}x{} clipped={} mode=raster \
         settings={} setups={} job={}",
        input.display(),
        report.pages,
        u8::from(report.printed),
        report.dpi.0,
        report.dpi.1,
        report.clipped_pages,
        match report.settings_source {
            pdfcer_print::SettingsSource::DeviceDefault => "device-default",
            pdfcer_print::SettingsSource::DriverSupplied => "driver",
            pdfcer_print::SettingsSource::CallerSupplied => "file",
            pdfcer_print::SettingsSource::Synthesised => "synthesised",
        },
        report.sheet_setups,
        report
            .job_id
            .map_or_else(|| "-".to_owned(), |j| j.to_string()),
    );
    exit::SUCCESS
}
/// Which route [`poster_sheets_for_page`] took to rasterise a poster's tiles.
///
/// Reported rather than inferred because the two differ by an order of
/// magnitude in speed on a big poster, and the operator is entitled to know
/// which one their document got.
#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PosterRoute {
    /// The page was interpreted **once** into a display list and each tile
    /// replayed from it.
    Recorded,
    /// The page could not be recorded (module docs of
    /// `pdfcer_render::display_list` §3), so each tile was rendered as an
    /// independent region — correct, but one content-stream walk per sheet.
    PerTile,
}

/// Rasterise one tiled page into its poster sheets.
///
/// # Why this does NOT render the whole page and crop it
///
/// It used to, with a comment explaining that rendering per tile "would
/// re-rasterise the whole page for every sheet — on a 4x5 poster, twenty
/// times the work for identical pixels." That reasoning was correct about
/// cost and it shipped a **bug**:
///
/// ```text
/// $ pdfcer print --poster --poster-scale 8 <A3 CAD drawing>
/// pdfcer: page 1: requested raster size 39685x28063 is empty or
///            exceeds MAX_PIXMAP_EDGE
/// ```
///
/// A poster's whole point is magnifying one page across many sheets, so its
/// full raster is *by construction* larger than any single sheet — and past
/// a modest magnification, larger than `MAX_PIXMAP_EDGE` allows. Poster
/// printing therefore failed outright on exactly the documents people make
/// posters of. Memory scaled with the assembled poster instead of with the
/// paper in the printer.
///
/// The fix is per-tile **regions**, which bounds memory by the sheet — and
/// what makes it affordable is `Pass 75.0`'s display list: the page is
/// still interpreted **once**, and each tile replays from that. The original
/// comment's cost argument is honoured; only its implementation changed.
///
/// A page the recorder refuses falls back to an independent region render
/// per tile. That is the old cost the comment warned about, so it is
/// **disclosed** by the caller rather than absorbed silently — but it is
/// still strictly better than today's behaviour, which was to fail.
///
/// # Geometry
///
/// `tile.source_pt` is the window of the source page this tile shows, in the
/// page's own points, top-left origin, `+y` down, already clipped to the
/// page. Multiplying by `tile_scale × dpi/72` puts it in the page's DEVICE
/// space — which is the same arithmetic the previous implementation used to
/// index into the whole-page pixmap, so the two select the same pixels.
///
/// That device rectangle is mapped back through the inverse page CTM to get
/// the user-space region to ask for. Going through the real transform rather
/// than deriving the flip by hand is what keeps `/Rotate` 90/270 correct: a
/// hand-written mapping is right for the unrotated case and transposed for
/// the odd quarter-turns.
///
/// # Errors
///
/// A human-readable message when the page CTM is not invertible, a sheet
/// cannot be allocated, or a tile fails to rasterise.
#[cfg(windows)]
#[allow(clippy::too_many_arguments)] // each is an independent job setting
pub(crate) fn poster_sheets_for_page(
    view: &pdfcer_core::view::DocumentView<'_>,
    page: &pdfcer_core::page_tree::Page,
    layout: &pdfcer_print::imposition::PosterLayout,
    tile_scale: f64,
    dpi: u32,
    printable_pt: (f64, f64),
    options: &pdfcer_render::RenderOptions,
    document: &str,
) -> Result<(Vec<pdfcer_print::PageBitmap>, PosterRoute), String> {
    let device_scale = f64::from(dpi) / 72.0;
    #[allow(clippy::cast_possible_truncation)]
    let render_scale = (device_scale * tile_scale) as f32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let px = |pt: f64| (pt * device_scale).round().max(1.0) as u32;
    let (sw, sh) = (px(printable_pt.0), px(printable_pt.1));

    let (_, _, page_ctm) = pdfcer_render::page_device_geometry(page, render_scale);
    let Some(inverse) = page_ctm.invert() else {
        return Err("the page transform is not invertible, so it cannot be tiled".to_owned());
    };

    // One interpretation for the whole poster when the page allows it.
    let list = pdfcer_render::record_page(view, page, render_scale, 0, options).ok();
    let route = if list.is_some() {
        PosterRoute::Recorded
    } else {
        PosterRoute::PerTile
    };

    let mut sheets: Vec<pdfcer_print::PageBitmap> = Vec::new();
    for tile in &layout.tiles {
        let Some(mut sheet) = pdfcer_render::tiny_skia::Pixmap::new(sw, sh) else {
            return Err(format!(
                "a sheet of {sw}x{sh} pixels is too large to compose"
            ));
        };
        sheet.fill(pdfcer_render::tiny_skia::Color::WHITE);

        // The tile's window in the page's DEVICE space. `+ 1` of slack on
        // each side before inverting, so a sub-ULP round trip through the
        // inverse cannot land the region a fraction inside the window and
        // clip a column. Over-rendering costs pixels nobody reads; under-
        // rendering loses content at a tile seam, which is the defect a
        // poster shows most visibly.
        #[allow(clippy::cast_possible_truncation)]
        let (dx0, dy0) = (
            (tile.source_pt.x * tile_scale * device_scale) as f32 - 1.0,
            (tile.source_pt.y * tile_scale * device_scale) as f32 - 1.0,
        );
        #[allow(clippy::cast_possible_truncation)]
        let (dx1, dy1) = (
            ((tile.source_pt.x + tile.source_pt.width) * tile_scale * device_scale) as f32 + 1.0,
            ((tile.source_pt.y + tile.source_pt.height) * tile_scale * device_scale) as f32 + 1.0,
        );
        let mut corners = [
            pdfcer_render::tiny_skia::Point::from_xy(dx0, dy0),
            pdfcer_render::tiny_skia::Point::from_xy(dx1, dy0),
            pdfcer_render::tiny_skia::Point::from_xy(dx1, dy1),
            pdfcer_render::tiny_skia::Point::from_xy(dx0, dy1),
        ];
        inverse.map_points(&mut corners);
        let (mut lo_x, mut lo_y) = (f64::INFINITY, f64::INFINITY);
        let (mut hi_x, mut hi_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for c in corners {
            lo_x = lo_x.min(f64::from(c.x));
            hi_x = hi_x.max(f64::from(c.x));
            lo_y = lo_y.min(f64::from(c.y));
            hi_y = hi_y.max(f64::from(c.y));
        }
        let region = pdfcer_core::page_tree::Rect::from_corners(lo_x, lo_y, hi_x, hi_y);

        // Where the region's own pixel (0,0) sits in page-device space —
        // read from the SAME function the renderer uses to place it, never
        // recomputed here, so the two cannot disagree about the origin.
        //
        // AND THAT SENTENCE IS THE WHOLE TEST. When the renderer moved to
        // `region_base_geometry` for its `f64` deep-zoom arithmetic, this
        // call was left on `region_device_geometry` — two functions that
        // agree at ordinary scales to within a pixel and therefore produce
        // a tile whose CONTENT is right and whose WINDOW is one pixel off.
        // `tiles_rendered_as_regions_match_the_whole_page_crop` caught it
        // immediately, with 5,841 of 7,487,472 bytes differing and its own
        // message correctly reading that count as an offset rather than a
        // drawing error.
        let Some(rg) = pdfcer_render::region_base_geometry(page, render_scale, region) else {
            return Err("a tile's region does not map onto the page".to_owned());
        };
        let (rx0, ry0) = (rg.x0, rg.y0);

        let rendered = match &list {
            Some(list) => list.replay_region(list.key(), region),
            None => pdfcer_render::render_page_region(view, page, render_scale, region, options),
        }
        .map_err(|e| format!("tile r{} c{}: {e}", tile.row, tile.column))?;

        // The page-device pixel the tile's top-left corner shows, in the
        // rounding the previous implementation used — kept identical so the
        // content lands on the same sheet pixel it always has.
        #[allow(clippy::cast_possible_wrap)]
        let sx = px(tile.source_pt.x * tile_scale) as i32;
        #[allow(clippy::cast_possible_wrap)]
        let sy = px(tile.source_pt.y * tile_scale) as i32;
        #[allow(clippy::cast_possible_truncation)]
        let (ox, oy) = (rx0 as i32, ry0 as i32);
        #[allow(clippy::cast_possible_wrap)]
        sheet.draw_pixmap(
            px(tile.sheet_pt.x) as i32 - (sx - ox),
            px(tile.sheet_pt.y) as i32 - (sy - oy),
            rendered.pixmap.as_ref(),
            &pdfcer_render::tiny_skia::PixmapPaint::default(),
            pdfcer_render::tiny_skia::Transform::identity(),
            None,
        );
        draw_poster_marks(&mut sheet, layout, tile, device_scale, document)?;
        sheets.push(pdfcer_print::PageBitmap {
            width: sheet.width(),
            height: sheet.height(),
            rgba: sheet.data().to_vec(),
            placement: pdfcer_print::Placement {
                scale: 1.0,
                offset_x_pt: 0.0,
                offset_y_pt: 0.0,
                clipped: false,
            },
            page_pt: printable_pt,
        });
    }
    Ok((sheets, route))
}

/// The render options every page of a print job uses: the comment scope, and
/// with `--line-width` every stroke fixed at that many millimetres at `dpi`.
#[cfg(windows)]
pub(crate) fn print_render_options(
    comments: CommentsArg,
    line_width_mm: Option<f64>,
    dpi: u32,
) -> pdfcer_render::RenderOptions {
    let mut options =
        pdfcer_render::RenderOptions::default().with_annotation_scope(comments.to_scope());
    if let Some(mm) = line_width_mm {
        #[allow(clippy::cast_possible_truncation)]
        let device_px = (mm / 25.4 * f64::from(dpi)) as f32;
        options.stroke_display = pdfcer_render::StrokeDisplay::Fixed { device_px };
    }
    options
}

/// Clap parser for `--line-width`: millimetres, above 0 and at most 25.4.
pub(crate) fn parse_line_width_mm(s: &str) -> Result<f64, String> {
    let mm: f64 = s
        .parse()
        .map_err(|_| format!("`{s}` is not a width in millimetres"))?;
    if mm.is_finite() && mm > 0.0 && mm <= 25.4 {
        Ok(mm)
    } else {
        Err(format!(
            "a line width must be above 0 and at most 25.4 mm, not {s}"
        ))
    }
}

/// Cut-mark stroke width, in points: thin enough to cut along, and never
/// below one device pixel.
#[cfg(windows)]
pub(crate) const POSTER_CUT_MARK_WIDTH_PT: f64 = 0.5;

/// Draw one tile's cut marks and label onto its sheet, at the geometry
/// [`pdfcer_print::imposition::PosterLayout`] gives. Draws nothing when the
/// layout's flags are off.
///
/// # Errors
///
/// A message when the label cannot be rasterised.
#[cfg(windows)]
pub(crate) fn draw_poster_marks(
    sheet: &mut pdfcer_render::tiny_skia::Pixmap,
    layout: &pdfcer_print::imposition::PosterLayout,
    tile: &pdfcer_print::imposition::PosterTile,
    device_scale: f64,
    document: &str,
) -> Result<(), String> {
    use pdfcer_render::tiny_skia::{Paint, PathBuilder, PixmapPaint, Stroke, Transform};

    #[allow(clippy::cast_possible_truncation)]
    let dev = |pt: f64| (pt * device_scale) as f32;
    let segments = layout.cut_mark_segments(tile);
    let mut pb = PathBuilder::new();
    for seg in &segments {
        pb.move_to(dev(seg.from.0), dev(seg.from.1));
        pb.line_to(dev(seg.to.0), dev(seg.to.1));
    }
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color_rgba8(0, 0, 0, 255);
        let stroke = Stroke {
            width: dev(POSTER_CUT_MARK_WIDTH_PT).max(1.0),
            ..Stroke::default()
        };
        sheet.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    if let Some(rect) = layout.label_rect(tile) {
        if rect.width < 1.0 || rect.height < 1.0 {
            return Ok(());
        }
        let text = pdfcer_print::imposition::poster_tile_label(
            tile.row,
            tile.column,
            layout.rows,
            layout.columns,
            document,
        );
        let label = render_poster_label(&text, rect.width, rect.height, device_scale)?;
        #[allow(clippy::cast_possible_truncation)]
        sheet.draw_pixmap(
            dev(rect.x).round() as i32,
            dev(rect.y).round() as i32,
            label.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }
    Ok(())
}

/// `text` as WinAnsi codes, and how many characters had no WinAnsi code and
/// became `?`.
#[cfg(windows)]
pub(crate) fn winansi_bytes(text: &str) -> (Vec<u8>, usize) {
    use pdfcer_core::fontdata::{BaseEncoding, encoding_glyph_name, glyph_name_to_unicode};
    let mut replaced = 0;
    let bytes = text
        .chars()
        .map(|ch| {
            (0x20..=0xFF_u8)
                .find(|&code| {
                    encoding_glyph_name(BaseEncoding::WinAnsi, code).and_then(glyph_name_to_unicode)
                        == Some(ch)
                })
                .unwrap_or_else(|| {
                    replaced += 1;
                    b'?'
                })
        })
        .collect();
    (bytes, replaced)
}

/// Rasterise a poster label: `text` in Helvetica at `height_pt`, on a
/// transparent `width_pt` × `height_pt` box, at `device_scale` pixels per
/// point.
///
/// The label goes through pdfcer's own renderer, as a one-line PDF, so the
/// CLI needs no second text rasteriser. Text past the box is cut off.
///
/// # Errors
///
/// A message when the synthetic page fails to parse or render.
#[cfg(windows)]
pub(crate) fn render_poster_label(
    text: &str,
    width_pt: f64,
    height_pt: f64,
    device_scale: f64,
) -> Result<pdfcer_render::tiny_skia::Pixmap, String> {
    let (codes, _) = winansi_bytes(text);
    let mut content =
        format!("BT /F1 {height_pt:.3} Tf 0 {:.3} Td (", height_pt * 0.22).into_bytes();
    for b in codes {
        match b {
            b'(' | b')' | b'\\' => content.extend([b'\\', b]),
            0x20..=0x7E => content.push(b),
            _ => content.extend(format!("\\{b:03o}").as_bytes()),
        }
    }
    content.extend_from_slice(b") Tj ET");

    let objects: [Vec<u8>; 5] = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 {width_pt:.3} {height_pt:.3}] >>"
        )
        .into_bytes(),
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_vec(),
        [
            format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(),
            &content,
            b"\nendstream",
        ]
        .concat(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = pdf.len();
    pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for off in offsets {
        pdf.extend(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );

    let doc = pdfcer_core::document::Document::from_bytes(pdf)
        .map_err(|e| format!("poster label: {e}"))?;
    let page = pdfcer_core::page_tree::pages(&doc)
        .map_err(|e| format!("poster label: {e}"))?
        .remove(0);
    let options = pdfcer_render::RenderOptions::default()
        .with_backdrop(pdfcer_render::PageBackdrop::Transparent);
    #[allow(clippy::cast_possible_truncation)]
    let rendered = pdfcer_render::render_page_with(&doc, &page, device_scale as f32, &options)
        .map_err(|e| format!("poster label: {e}"))?;
    Ok(rendered.pixmap)
}

/// The poster tiler's differential oracle.
///
/// # Why a second implementation lives here, when this project treats "two
/// paths for one thing" as a trap
///
/// Because this is the *test* side of a differential, not a second shipping
/// path — the same shape `crates/pdfcer-render/tests/region_matches_full_page.rs`
/// uses, where a region render is proved against a crop of a full-page one.
///
/// The change being tested replaced "render the whole page, crop each tile
/// out of it" with "render each tile as a region". The only oracle worth
/// having is the implementation it replaced, held byte-for-byte — so it is
/// kept, verbatim, behind `#[cfg(test)]`, and the new one has to match it on
/// a page small enough that the old one still works.
///
/// It cannot drift into production: it is unreachable outside `cargo test`,
/// and if it ever stops compiling that is a signal the geometry it encodes
/// changed and the assertion below needs re-deriving rather than deleting.
#[cfg(all(test, windows))]
mod poster_tiling_tests {
    use super::{PosterRoute, poster_sheets_for_page};

    /// The PREVIOUS implementation, kept verbatim as the oracle: render the
    /// whole page once at tile scale, then copy each tile's window out of it.
    fn whole_page_reference(
        view: &pdfcer_core::view::DocumentView<'_>,
        page: &pdfcer_core::page_tree::Page,
        layout: &pdfcer_print::imposition::PosterLayout,
        tile_scale: f64,
        dpi: u32,
        printable_pt: (f64, f64),
        options: &pdfcer_render::RenderOptions,
    ) -> Vec<Vec<u8>> {
        let px = |pt: f64| (pt * f64::from(dpi) / 72.0).round().max(1.0) as u32;
        let (sw, sh) = (px(printable_pt.0), px(printable_pt.1));
        let render_scale = (f64::from(dpi) / 72.0) * tile_scale;
        let rendered =
            pdfcer_render::render_page_with_view(view, page, render_scale as f32, options)
                .expect("the reference implementation must be able to render this page whole");
        let mut out = Vec::new();
        for tile in &layout.tiles {
            let mut sheet = pdfcer_render::tiny_skia::Pixmap::new(sw, sh).expect("sheet");
            sheet.fill(pdfcer_render::tiny_skia::Color::WHITE);
            let sx = px(tile.source_pt.x * tile_scale) as i32;
            let sy = px(tile.source_pt.y * tile_scale) as i32;
            sheet.draw_pixmap(
                px(tile.sheet_pt.x) as i32 - sx,
                px(tile.sheet_pt.y) as i32 - sy,
                rendered.pixmap.as_ref(),
                &pdfcer_render::tiny_skia::PixmapPaint::default(),
                pdfcer_render::tiny_skia::Transform::identity(),
                None,
            );
            out.push(sheet.data().to_vec());
        }
        out
    }

    fn fixture(rel: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic")
            .join(rel)
    }

    /// Per-tile regions produce byte-identical sheets to cropping a
    /// whole-page render.
    ///
    /// Run over several magnifications, because the tile grid changes shape
    /// with `tile_scale` and a geometry error that cancels on a 2x2 grid
    /// will not on a 3x3.
    #[test]
    fn tiles_rendered_as_regions_match_the_whole_page_crop() {
        let doc = pdfcer_core::document::Document::load(&fixture("addtext/plain.pdf"))
            .expect("fixture loads");
        let page = pdfcer_core::page_tree::pages(&doc)
            .expect("page tree")
            .remove(0);
        let size = (
            page.crop_box.urx - page.crop_box.llx,
            page.crop_box.ury - page.crop_box.lly,
        );
        // An A4 printable area, near enough — the exact paper does not
        // matter, only that the page needs more than one sheet of it.
        let printable_pt = (560.0, 770.0);
        let options = pdfcer_render::RenderOptions::default();

        for tile_scale in [1.5_f64, 2.0, 3.0] {
            let spec = pdfcer_print::imposition::PosterSpec {
                tile_scale,
                overlap_pt: 12.0,
                cut_marks: false,
                labels: false,
                tile_only_large_pages: false,
                max_tiles: 64,
            };
            let layout = pdfcer_print::imposition::plan_poster(printable_pt, size, &spec)
                .expect("poster plans");
            assert!(
                layout.tiles.len() > 1,
                "a tile_scale of {tile_scale} must actually tile, or this proves nothing"
            );

            let expected = whole_page_reference(
                &doc.view(),
                &page,
                &layout,
                tile_scale,
                150,
                printable_pt,
                &options,
            );
            let (got, route) = poster_sheets_for_page(
                &doc.view(),
                &page,
                &layout,
                tile_scale,
                150,
                printable_pt,
                &options,
                "",
            )
            .expect("tiles rasterise");

            assert_eq!(
                route,
                PosterRoute::Recorded,
                "a plain text page must take the recorded route, or this test is \
                 measuring the fallback and not the change"
            );
            assert_eq!(got.len(), expected.len(), "sheet count at {tile_scale}x");
            for (i, (sheet, want)) in got.iter().zip(expected.iter()).enumerate() {
                let differing = sheet
                    .rgba
                    .iter()
                    .zip(want.iter())
                    .filter(|(a, b)| a != b)
                    .count();
                assert_eq!(
                    differing,
                    0,
                    "{tile_scale}x sheet {i}: rendering a tile as a REGION must give \
                     the same bytes as cropping it out of a whole-page render; \
                     {differing} of {} bytes differ. A count in the thousands with \
                     the right sheet size means the window is offset, not that the \
                     drawing is wrong.",
                    want.len()
                );
            }
        }
    }

    /// The bug this change exists to fix: a magnification whose whole-page
    /// raster exceeds `MAX_PIXMAP_EDGE` now tiles instead of failing.
    ///
    /// The control matters as much as the case — the reference implementation
    /// is asserted to FAIL here, so the test cannot pass because the numbers
    /// happened to stay small.
    #[test]
    fn a_magnification_too_large_to_raster_whole_still_tiles() {
        let doc = pdfcer_core::document::Document::load(&fixture("addtext/plain.pdf"))
            .expect("fixture loads");
        let page = pdfcer_core::page_tree::pages(&doc)
            .expect("page tree")
            .remove(0);
        let size = (
            page.crop_box.urx - page.crop_box.llx,
            page.crop_box.ury - page.crop_box.lly,
        );
        let printable_pt = (560.0, 770.0);
        let options = pdfcer_render::RenderOptions::default();
        // 612 x 792 pt at 150 DPI is 1275 x 1650 px; x30 magnification is
        // 38,250 x 49,500 -- comfortably past MAX_PIXMAP_EDGE (16,384).
        let tile_scale = 30.0;
        let spec = pdfcer_print::imposition::PosterSpec {
            tile_scale,
            overlap_pt: 0.0,
            cut_marks: false,
            labels: false,
            tile_only_large_pages: false,
            max_tiles: 4096,
        };
        let layout =
            pdfcer_print::imposition::plan_poster(printable_pt, size, &spec).expect("poster plans");

        // THE CONTROL: the old route cannot even produce the raster.
        let render_scale = ((150.0 / 72.0) * tile_scale) as f32;
        assert!(
            matches!(
                pdfcer_render::render_page_with_view(&doc.view(), &page, render_scale, &options),
                Err(pdfcer_render::RenderError::BadRasterSize { .. })
            ),
            "this test is only meaningful while a whole-page raster at this \
             magnification is impossible; if that changed, re-derive the numbers"
        );

        // Rasterise a few tiles rather than all 1,000-odd: the assertion is
        // about REACHABILITY, and byte-identity is already covered above.
        //
        // Which tiles is not a free choice, and two guesses were wrong before
        // this comment existed. Tile 0 is the page's top-left margin; the
        // middle of the grid is the middle of a text page's leading. Both are
        // legitimately blank, and asserting ink on a blank tile tests the
        // fixture's layout rather than the code.
        //
        // So the inked tile is LOCATED rather than guessed: a cheap scale-1
        // render gives the page's ink bounding box, and the tiles chosen are
        // the ones whose source window contains its centre.
        let probe = pdfcer_render::render_page_with_view(&doc.view(), &page, 1.0, &options)
            .expect("a scale-1 render of a text page");
        let (mut ix0, mut iy0, mut ix1, mut iy1) = (u32::MAX, u32::MAX, 0_u32, 0_u32);
        for y in 0..probe.pixmap.height() {
            for x in 0..probe.pixmap.width() {
                let px = probe.pixmap.pixel(x, y).expect("in bounds");
                if px.red() != 255 || px.green() != 255 || px.blue() != 255 {
                    ix0 = ix0.min(x);
                    iy0 = iy0.min(y);
                    ix1 = ix1.max(x);
                    iy1 = iy1.max(y);
                }
            }
        }
        assert!(ix0 <= ix1, "the fixture must have ink on page 1");
        // Scale 1 is one device pixel per point, and the device frame is
        // already top-left-origin -- the same frame `source_pt` uses.
        let ink_cx = f64::from(ix0 + ix1) / 2.0;
        let ink_cy = f64::from(iy0 + iy1) / 2.0;
        let inked: Vec<_> = layout
            .tiles
            .iter()
            .filter(|t| {
                t.source_pt.x <= ink_cx
                    && ink_cx < t.source_pt.x + t.source_pt.width
                    && t.source_pt.y <= ink_cy
                    && ink_cy < t.source_pt.y + t.source_pt.height
            })
            .cloned()
            .collect();
        assert!(
            !inked.is_empty(),
            "the tile grid must cover the page ink at ({ink_cx}, {ink_cy})"
        );

        let mut trimmed = layout.clone();
        trimmed.tiles = inked;
        let wanted = trimmed.tiles.len();
        let (sheets, route) = poster_sheets_for_page(
            &doc.view(),
            &page,
            &trimmed,
            tile_scale,
            150,
            printable_pt,
            &options,
            "",
        )
        .expect("tiles that no whole-page raster could hold must still rasterise");
        assert_eq!(route, PosterRoute::Recorded);
        assert_eq!(sheets.len(), wanted);
        assert!(
            sheets.iter().any(|s| s.rgba.iter().any(|&b| b != 255)),
            "the tile covering the page ink must carry ink at 30x, or the region path is producing blank paper and the reachability claim is empty"
        );
    }
}

#[cfg(test)]
mod print_line_width_arg_tests {
    use super::parse_line_width_mm;

    #[test]
    fn accepts_a_positive_width_up_to_an_inch() {
        assert_eq!(parse_line_width_mm("0.35"), Ok(0.35));
        assert_eq!(parse_line_width_mm("25.4"), Ok(25.4));
    }

    #[test]
    fn refuses_zero_negative_huge_and_non_numbers() {
        for bad in ["0", "-1", "25.5", "NaN", "inf", "thin"] {
            assert!(parse_line_width_mm(bad).is_err(), "{bad} was accepted");
        }
    }
}

#[cfg(all(test, windows))]
mod poster_marks_tests {
    use super::{
        CommentsArg, draw_poster_marks, print_render_options, render_poster_label, winansi_bytes,
    };
    use pdfcer_print::imposition::{PosterLayout, PosterSpec, Rect, plan_poster};
    use pdfcer_render::tiny_skia::{Color, Pixmap};

    const SCALE: f64 = 150.0 / 72.0;

    fn layout(cut_marks: bool, labels: bool) -> PosterLayout {
        let spec = PosterSpec {
            tile_scale: 1.0,
            overlap_pt: 0.0,
            cut_marks,
            labels,
            tile_only_large_pages: false,
            max_tiles: 64,
        };
        plan_poster((612.0, 792.0), (1224.0, 1584.0), &spec).expect("poster plans")
    }

    /// Dark pixels inside `rect` (points), on a sheet drawn at [`SCALE`].
    fn dark_in(sheet: &Pixmap, rect: Rect) -> usize {
        let px = |pt: f64| (pt * SCALE).round() as u32;
        let (x0, y0) = (px(rect.x), px(rect.y));
        let (x1, y1) = (
            px(rect.x + rect.width).min(sheet.width()),
            px(rect.y + rect.height).min(sheet.height()),
        );
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| sheet.pixel(x, y).is_some_and(|p| p.red() < 128))
            .count()
    }

    fn drawn(layout: &PosterLayout) -> Pixmap {
        let px = |pt: f64| (pt * SCALE).round() as u32;
        let mut sheet = Pixmap::new(px(612.0), px(792.0)).expect("sheet");
        sheet.fill(Color::WHITE);
        draw_poster_marks(&mut sheet, layout, &layout.tiles[0], SCALE, "plan.pdf")
            .expect("marks draw");
        sheet
    }

    #[test]
    fn nothing_is_drawn_when_both_flags_are_off() {
        let l = layout(false, false);
        assert_eq!(dark_in(&drawn(&l), Rect::new(0.0, 0.0, 612.0, 792.0)), 0);
    }

    #[test]
    fn cut_marks_land_in_the_band_and_nowhere_on_the_tile() {
        let l = layout(true, false);
        let sheet = drawn(&l);
        let tile = l.tiles[0].sheet_pt;
        assert!(dark_in(&sheet, Rect::new(0.0, 0.0, 612.0, tile.y)) > 0);
        // One point of stroke antialiasing may touch the tile edge.
        let inner = Rect::new(
            tile.x + 1.0,
            tile.y + 1.0,
            tile.width - 2.0,
            tile.height - 2.0,
        );
        assert_eq!(dark_in(&sheet, inner), 0);
    }

    #[test]
    fn the_label_is_printed_inside_its_rectangle() {
        let l = layout(false, true);
        let rect = l
            .label_rect(&l.tiles[0])
            .expect("labels reserve a rectangle");
        let sheet = drawn(&l);
        assert!(dark_in(&sheet, rect) > 50, "no label text was drawn");
        let tile = l.tiles[0].sheet_pt;
        assert_eq!(
            dark_in(&sheet, Rect::new(tile.x, tile.y, tile.width, tile.height)),
            0
        );
    }

    #[test]
    fn an_empty_label_renders_fully_transparent() {
        let label = render_poster_label("", 200.0, 12.0, SCALE).expect("renders");
        assert!(label.pixels().iter().all(|p| p.alpha() == 0));
        let label = render_poster_label("row 1", 200.0, 12.0, SCALE).expect("renders");
        assert!(label.pixels().iter().any(|p| p.alpha() > 0));
    }

    #[test]
    fn characters_outside_winansi_become_question_marks_and_are_counted() {
        let (bytes, replaced) = winansi_bytes("a\u{2014}\u{e9}\u{20ac}\u{6f22}");
        assert_eq!(bytes, vec![b'a', 0x97, 0xE9, 0x80, b'?']);
        assert_eq!(replaced, 1);
    }

    #[test]
    fn line_width_becomes_a_fixed_device_width_at_the_job_dpi() {
        let options = print_render_options(CommentsArg::Document, Some(25.4), 300);
        assert_eq!(
            options.stroke_display,
            pdfcer_render::StrokeDisplay::Fixed { device_px: 300.0 }
        );
        let options = print_render_options(CommentsArg::Document, None, 300);
        assert_eq!(
            options.stroke_display,
            pdfcer_render::StrokeDisplay::default()
        );
    }
}

/// `print-preview` — what a print WOULD do, without doing it.
///
/// # Why this exists before `print` does
///
/// Printing is an outward-facing, irreversible side effect: paper is
/// consumed and a shared device is occupied. So the surface that answers
/// "what would happen" ships before the one that makes it happen, and
/// this command deliberately has no flag that starts a job.
///
/// It is not a placeholder either. Everything a real print needs —
/// resolving the printer, reading its resolution and printable area,
/// and placing each page onto the sheet — happens here and is reported.
/// When spooling lands it will consume this exact result, so a preview
/// that reads correctly is evidence about the print, not a separate
/// approximation of it.
///
/// # The clip report is the point
///
/// Acrobat clips an oversized page **silently**
/// (`Acrobat_Features/printing__scaling_modes.md`). pdfcer names the pages
/// that would lose content, and the exit code reflects it, so a scripted
/// caller can refuse to print rather than discover the loss on paper.
// Eight arguments, one over clippy's bound, and for the same reason
// `cmd_print` carries the allow at twenty-nine: they are `clap`'s own
// parsed flags handed straight through, and bundling them into a struct
// would mean a second definition of the command's surface that has to be
// kept in step with the derive.
#[allow(clippy::too_many_arguments)]
#[cfg(windows)]
pub(crate) fn cmd_print_preview(
    input: &Path,
    printer: Option<&str>,
    scale: PrintScaleArg,
    scale_percent: Option<u32>,
    pages: &str,
    orientation: OrientationArg,
    paper: Option<&str>,
    paper_size: Option<&str>,
) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };

    // Resolve the printer: the named one, else the system default. An
    // unnamed preview on a machine with no default is a real dead end,
    // so it says which of the two problems it is.
    let chosen = match print_target(printer) {
        Ok(name) => name,
        Err(code) => return code,
    };
    let (selected_paper, requested_sheet_pt) = match resolve_paper(&chosen, paper, paper_size) {
        Ok(paper) => paper,
        Err(code) => return code,
    };

    // Read for the sheet the JOB will use, for the same reason
    // `--orientation` is honoured here: paper changes the printable
    // rectangle, and a preview planned against a different sheet than
    // the print would agree with the wrong answer instead of catching it.
    let caps = match pdfcer_print::printer_caps_for(&chosen, None, selected_paper) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("pdfcer: {err}");
            return exit::EDIT_REFUSED;
        }
    };
    report_sheet_mismatch(&chosen, requested_sheet_pt, &caps);

    // Through a session, matching every other page-addressing command:
    // `pages()` lives on `EditSession`, and reading through the same
    // type the editing commands use keeps one page-index space.
    let session = pdfcer_core::edit::EditSession::new(doc);
    let page_list = match session.pages() {
        Ok(p) => p,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let indices = match parse_pages(pages, page_list.len()) {
        Ok(i) => i,
        Err(err) => {
            eprintln!("pdfcer: {err}");
            return exit::EDIT_REFUSED;
        }
    };

    // The sheet as the DRIVER will present it, not as it was reported.
    //
    // `printer_caps` reads the device's default `DEVMODE`, so a
    // portrait-default printer reports a portrait sheet even for a job
    // that will print landscape. Reporting and placing against that
    // un-turned sheet is what made a landscape page come out at about
    // 77% of correct size, and a preview repeating the same mistake would
    // agree with the wrong print instead of catching it.
    //
    // The orientation page is the FIRST selected page, matching what
    // `print` resolves `auto` from — one `DEVMODE` covers the whole job.
    // The DISPLAYED size, not the raw media box: /Rotate is a display
    // rotation the renderer honours, so a page that is portrait in the
    // file and landscape on screen must be planned as landscape or the
    // placement and the pixels describe different shapes. See
    // `displayed_page_size` for what went wrong before this.
    let page_sizes: Vec<(f64, f64)> = page_list
        .iter()
        .map(|p| {
            let mb = p.media_box;
            pdfcer_print::displayed_page_size(
                ((mb.urx - mb.llx).abs(), (mb.ury - mb.lly).abs()),
                i32::from(p.rotate),
            )
        })
        .collect();
    let first_page_pt = indices
        .first()
        .and_then(|&i| page_sizes.get(i).copied())
        .unwrap_or(pdfcer_print::US_LETTER_PORTRAIT_PT);
    let device =
        pdfcer_print::DeviceGeometry::from_caps(&caps, orientation.to_orientation(), first_page_pt);
    let turned = device.default_orientation();

    println!(
        "printer name={:?} dpi={}x{} sheet_pt={:.1}x{:.1} printable_pt={:.1}x{:.1} \
         margin_pt={:.1},{:.1} orientation={}",
        chosen,
        device.dpi.0,
        device.dpi.1,
        device.physical_pt.0,
        device.physical_pt.1,
        device.printable_pt.0,
        device.printable_pt.1,
        device.offset_pt.0,
        device.offset_pt.1,
        match turned {
            pdfcer_print::Orientation::Landscape => "landscape",
            pdfcer_print::Orientation::Auto | pdfcer_print::Orientation::Portrait => "portrait",
        },
    );

    // A percentage wins over the word. Clap has already bounded it to
    // 1..=1000, so the conversion cannot produce a non-positive or
    // non-finite multiplier.
    let mode = match scale_percent {
        Some(pct) => pdfcer_print::ScaleMode::Custom(f64::from(pct) / 100.0),
        None => scale.to_mode(),
    };
    let mode_name = match scale_percent {
        Some(pct) => format!("{pct}%"),
        None => scale.name().to_owned(),
    };
    let mut clipped = 0usize;
    for i in &indices {
        let Some(page) = page_list.get(*i) else {
            continue;
        };
        // The MEDIA box is the sheet the page declares. A crop box would
        // be the right input for a viewer, but printing a cropped view
        // and printing the page are different operations, and Reader
        // prints the page.
        // …and turned by `/Rotate`, for the reason `displayed_page_size`
        // sets out: the renderer honours it, so planning against the
        // un-turned box would describe a different shape than the pixels.
        let mb = page.media_box;
        let size = pdfcer_print::displayed_page_size(
            ((mb.urx - mb.llx).abs(), (mb.ury - mb.lly).abs()),
            i32::from(page.rotate),
        );
        let p = pdfcer_print::place_page(size, device.printable_pt, mode);
        if p.clipped {
            clipped += 1;
        }
        println!(
            "page {} size_pt={:.1}x{:.1} scale={:.4} offset_pt={:.1},{:.1} clipped={}",
            i + 1,
            size.0,
            size.1,
            p.scale,
            p.offset_x_pt,
            p.offset_y_pt,
            u32::from(p.clipped),
        );
    }

    if clipped > 0 {
        // stderr, and named as a count: this is the fact that should stop
        // a scripted print, and it must not be lost in a stdout capture.
        eprintln!(
            "pdfcer: WARNING — {clipped} page(s) would lose content off the edge of the \
             paper at this scale. Acrobat clips these silently; pdfcer does not. Try \
             --scale fit or --scale shrink."
        );
    }
    println!(
        "print-preview {} printer={:?} scale={} pages={} clipped={clipped}",
        input.display(),
        chosen,
        mode_name,
        indices.len(),
    );
    // Zero even when pages would clip: the PREVIEW succeeded, and its
    // whole job is to report that fact. A non-zero exit would make "this
    // layout loses content" indistinguishable from "the file would not
    // open", and a caller that wants to branch has `clipped=` on the
    // summary line.
    exit::SUCCESS
}

/// The non-Windows arm — reports rather than vanishing, for the reason
/// given on `cmd_list_printers`.
///
/// Added 2026-08-18. `cmd_print` had been portable until the driver-sourced
/// `DEVMODE` work gave it four `#[cfg(windows)]` callees; the Windows build
/// stayed green and the Linux one stopped compiling. Every sibling in this
/// file already had its non-Windows arm, which is why the omission was
/// invisible locally — the pattern was established and simply not extended
/// to the one function that changed.
#[allow(clippy::too_many_arguments)]
#[cfg(not(windows))]
pub(crate) fn cmd_print(
    _input: &Path,
    _printer: Option<&str>,
    _scale: PrintScaleArg,
    _scale_percent: Option<u32>,
    _pages_spec: &str,
    _send: bool,
    _dpi_cap: u32,
    _to_file: Option<PathBuf>,
    _copies: u16,
    _uncollated: bool,
    _subset: SubsetArg,
    _reverse: bool,
    _orientation: OrientationArg,
    _duplex: DuplexArg,
    _pick_tray: bool,
    _paper: Option<&str>,
    _paper_size: Option<&str>,
    _printer_config: Option<&Path>,
    _comments: CommentsArg,
    _n_up: Option<u32>,
    _n_up_border: bool,
    _booklet: bool,
    _poster: bool,
    _poster_scale: f64,
    _poster_overlap: f64,
    _poster_large_only: bool,
    _poster_max_tiles: u32,
    _binding: BindingArg,
    _booklet_subset: BookletSubsetArg,
    _line_width_mm: Option<f64>,
    _poster_cut_marks: bool,
    _poster_labels: bool,
) -> u8 {
    eprintln!(
        "pdfcer: printing is available on Windows only in this build \
         (docs/decisions/003-distribution-posture.md §4.1)"
    );
    exit::EDIT_REFUSED
}

/// The non-Windows arm — reports rather than vanishing, for the reason
/// given on `cmd_list_printers`.
#[allow(clippy::too_many_arguments)]
#[cfg(not(windows))]
pub(crate) fn cmd_print_preview(
    _input: &Path,
    _printer: Option<&str>,
    _scale: PrintScaleArg,
    _scale_percent: Option<u32>,
    _pages: &str,
    _orientation: OrientationArg,
    _paper: Option<&str>,
    _paper_size: Option<&str>,
) -> u8 {
    eprintln!(
        "pdfcer: printing is available on Windows only in this build \
         (docs/decisions/003-distribution-posture.md §4.1)"
    );
    exit::EDIT_REFUSED
}

/// `list-printers` — what the print spooler can see.
///
/// # Why a whole subcommand for a list
///
/// Every later printing feature needs a printer NAME, and an operator
/// cannot supply one they cannot see. Shipping the query before the
/// action also means the platform binding is exercised and correct
/// before anything can put marks on paper.
///
/// Not built on non-Windows: the subcommand exists in the parser on
/// every platform (so `--help` is honest about what pdfcer offers) and
/// reports that it is unavailable rather than being silently missing —
/// a command that vanishes by platform is indistinguishable from a typo.
#[cfg(windows)]
pub(crate) fn cmd_list_printers() -> u8 {
    let printers = match pdfcer_print::list_printers() {
        Ok(p) => p,
        Err(err) => {
            eprintln!("pdfcer: {err}");
            return exit::IO_ERROR;
        }
    };
    for p in &printers {
        println!(
            "printer name={:?} driver={:?} port={:?} default={}",
            p.name,
            p.driver,
            p.port,
            u32::from(p.is_default),
        );
    }
    // Zero printers is a successful query of a machine with none — not a
    // failure. A non-zero exit would make "no printers installed"
    // indistinguishable from "the spooler is down", which is the one
    // distinction a caller actually needs here.
    println!("list-printers count={}", printers.len());
    exit::SUCCESS
}

/// The non-Windows arm — see the Windows version's docs for why this
/// reports rather than disappears.
#[cfg(not(windows))]
pub(crate) fn cmd_list_printers() -> u8 {
    eprintln!(
        "pdfcer: printing is available on Windows only in this build \
         (docs/decisions/003-distribution-posture.md §4.1)"
    );
    exit::EDIT_REFUSED
}

/// The printer a print-path subcommand should target.
///
/// Factored out because three subcommands now need it and a fourth copy
/// of "find the default, or explain that there isn't one" would be a
/// fourth place for the message to drift.
///
/// # Errors
///
/// An exit code, ready to return: [`exit::IO_ERROR`] when the spooler
/// cannot be queried at all, [`exit::EDIT_REFUSED`] when the machine has
/// no default and none was named.
#[cfg(windows)]
pub(crate) fn print_target(printer: Option<&str>) -> Result<String, u8> {
    if let Some(name) = printer {
        return Ok(name.to_owned());
    }
    let all = match pdfcer_print::list_printers() {
        Ok(p) => p,
        Err(err) => {
            eprintln!("pdfcer: {err}");
            return Err(exit::IO_ERROR);
        }
    };
    all.iter()
        .find(|p| p.is_default)
        .map(|p| p.name.clone())
        .ok_or_else(|| {
            eprintln!(
                "pdfcer: no default printer is set — pass --printer with one of the names from `pdfcer list-printers`"
            );
            exit::EDIT_REFUSED
        })
}

/// Turn `--paper` / `--paper-size` into a [`pdfcer_print::PaperSelection`],
/// **printing what pdfcer inferred on the way past**.
///
/// # Why this discloses rather than resolving quietly
///
/// Project rule 4. Two of the three paths here are pdfcer choosing a
/// value the operator did not type:
///
/// - `--paper a4` picks a form ID out of a name, and the driver's names
///   are not uniform (`"A4"` on one device, `"A4 (8.2 x 11.7 in; 210 x
///   297 mm)"` on the next), so the match is a judgement;
/// - `--paper-size 595x842` is rounded into the driver's own tenths of a
///   millimetre, so the sheet that gets fed is not exactly the one that
///   was asked for.
///
/// Both are stated on stderr. The invocation IS the commit in the CLI —
/// there is no session and no undo — so printing it on the way past is
/// the whole of the obligation (rule 11).
///
/// # Errors
///
/// An exit code. A form name that matches nothing, or matches more than
/// one form, is [`exit::EDIT_REFUSED`] naming the candidates rather than
/// a guess: picking one would be pdfcer deciding which sheet to consume.
#[cfg(windows)]
pub(crate) fn resolve_paper(
    printer: &str,
    paper: Option<&str>,
    paper_size: Option<&str>,
) -> Result<(pdfcer_print::PaperSelection, Option<(f64, f64)>), u8> {
    if let Some(spec) = paper_size {
        // `WxH`, in PDF points. `x` or `X`; nothing more elaborate,
        // because a unit-suffix grammar invented here would be a second
        // parser for something the crate already measures in points.
        let (w, h) = spec
            .split_once(['x', 'X'])
            .ok_or_else(|| {
                eprintln!(
                    "pdfcer: --paper-size wants WIDTHxHEIGHT in PDF points, for example 595x842"
                );
                exit::RUNTIME_ERROR
            })
            .and_then(
                |(w, h)| match (w.trim().parse::<f64>(), h.trim().parse::<f64>()) {
                    (Ok(w), Ok(h)) => Ok((w, h)),
                    _ => {
                        eprintln!(
                            "pdfcer: --paper-size {spec:?} is not two numbers separated by x"
                        );
                        Err(exit::RUNTIME_ERROR)
                    }
                },
            )?;
        let selection =
            pdfcer_print::PaperSelection::custom_from_points((w, h)).ok_or_else(|| {
                eprintln!(
                    "pdfcer: a custom sheet of {w}x{h} pt cannot be requested — a DEVMODE stores \
                 the size in tenths of a millimetre as a signed 16-bit number, so the largest \
                 sheet it can name is about 3.28 m ({} pt) on each axis, and the smallest is \
                 one tenth of a millimetre. Refused rather than clamped, because a silently \
                 shortened sheet looks like a pdfcer scaling fault.",
                    f64::from(pdfcer_print::MAX_CUSTOM_SHEET_TENTHS_MM) * 72.0 / 254.0,
                );
                exit::EDIT_REFUSED
            })?;
        if let Some((aw, ah)) = selection.size_pt() {
            eprintln!(
                "pdfcer: custom sheet {w:.2}x{h:.2} pt requested; the driver's unit is tenths \
                 of a millimetre, so the sheet fed will be {aw:.2}x{ah:.2} pt."
            );
        }
        let asked = selection.size_pt();
        return Ok((selection, asked));
    }

    let Some(wanted) = paper else {
        return Ok((pdfcer_print::PaperSelection::DeviceDefault, None));
    };

    // A form whose size the driver declined to state is reported as
    // (0, 0) — see `printer_forms`. Passing that on as an EXPECTATION
    // would make every such form look like the driver had refused the
    // request, so it becomes "nothing was stated" instead.
    let stated = |form: &pdfcer_print::PaperForm| {
        (form.size_pt.0 > 0.0 && form.size_pt.1 > 0.0).then_some(form.size_pt)
    };

    let forms = match pdfcer_print::printer_forms(printer) {
        Ok(forms) => forms,
        Err(err) => {
            eprintln!("pdfcer: {err}");
            return Err(exit::EDIT_REFUSED);
        }
    };
    if forms.is_empty() {
        eprintln!(
            "pdfcer: {printer:?} reports no paper forms at all, so --paper cannot be \
             resolved against anything. Use --paper-size to name a sheet by size."
        );
        return Err(exit::EDIT_REFUSED);
    }

    // A bare number is an ID. Checked against the list rather than
    // trusted: an ID the driver does not offer would be sent, ignored,
    // and print on the default sheet — a request that appears accepted
    // and was not, which is the defect class this whole change exists
    // to close.
    if let Ok(id) = wanted.parse::<u16>() {
        return match forms.iter().find(|f| f.id == id) {
            Some(form) => {
                eprintln!(
                    "pdfcer: paper form {} {:?}, {:.1}x{:.1} pt.",
                    form.id, form.name, form.size_pt.0, form.size_pt.1
                );
                Ok((pdfcer_print::PaperSelection::Form(form.id), stated(form)))
            }
            None => {
                eprintln!(
                    "pdfcer: {printer:?} does not offer paper form {id} — run \
                     `pdfcer list-paper-sizes --printer {printer:?}` for the ones it does."
                );
                Err(exit::EDIT_REFUSED)
            }
        };
    }

    // Exact first, then unique prefix. Substring matching is
    // deliberately NOT tried: on a driver whose names carry dimensions,
    // "A4" is a substring of half the list.
    let lowered = wanted.to_lowercase();
    let exact: Vec<&pdfcer_print::PaperForm> = forms
        .iter()
        .filter(|f| f.name.eq_ignore_ascii_case(wanted))
        .collect();
    let candidates: Vec<&pdfcer_print::PaperForm> = if exact.is_empty() {
        forms
            .iter()
            .filter(|f| f.name.to_lowercase().starts_with(&lowered))
            .collect()
    } else {
        exact
    };
    match candidates.as_slice() {
        [form] => {
            eprintln!(
                "pdfcer: --paper {wanted:?} resolved to form {} {:?}, {:.1}x{:.1} pt.",
                form.id, form.name, form.size_pt.0, form.size_pt.1
            );
            Ok((pdfcer_print::PaperSelection::Form(form.id), stated(form)))
        }
        [] => {
            eprintln!(
                "pdfcer: {printer:?} offers no paper form named {wanted:?} — run \
                 `pdfcer list-paper-sizes` to see the {} it does offer.",
                forms.len()
            );
            Err(exit::EDIT_REFUSED)
        }
        many => {
            eprintln!(
                "pdfcer: --paper {wanted:?} matches {} forms on {printer:?}; name one exactly \
                 or use its ID:",
                many.len()
            );
            for form in many {
                eprintln!("  {} {:?}", form.id, form.name);
            }
            Err(exit::EDIT_REFUSED)
        }
    }
}

/// Say so when the driver did not give the sheet that was asked for.
///
/// # Why this exists: a paper request can be ignored in silence
///
/// Measured on this machine, 2026-08-18, with `--paper-size 1000x1400`:
///
/// - **Microsoft Print to PDF** ignored it completely and reported its
///   own 612x792. Its form list has no user-defined entry, and a driver
///   that only supports enumerated forms is under no obligation to
///   honour `DMPAPER_USER` — it simply does not.
/// - **EPSON ET-16600** honoured the LENGTH (1399.9 pt) and clamped the
///   WIDTH to 595.2 pt, its maximum media width. A partial honour, which
///   is the worse case: the sheet is neither what was asked for nor
///   obviously wrong.
///
/// Neither reported an error. `spool` would have returned `Ok`, the
/// summary line would have said `printed=1`, and the paper would have
/// been the wrong size — which is the exact shape of the `pick_tray`
/// defect this whole change set is about, in a new place.
///
/// pdfcer's own arithmetic stays correct either way, because
/// [`pdfcer_print::printer_caps_for`] reads the geometry the device
/// ACTUALLY reports after the request, so placement is right for the
/// sheet that will really be fed. What is wrong is only the operator's
/// expectation, and that is exactly what a disclosure is for.
///
/// # Why the comparison is orientation-blind
///
/// `DC_PAPERSIZE` states a form portrait-first while `printer_caps`
/// reports the sheet in the DEVICE's default orientation, which is
/// landscape on plotters and label printers. Comparing the sorted pair
/// avoids a false alarm on every job sent to one of those.
#[cfg(windows)]
pub(crate) fn report_sheet_mismatch(
    printer: &str,
    requested_pt: Option<(f64, f64)>,
    caps: &pdfcer_print::PrinterCaps,
) {
    let Some((rw, rh)) = requested_pt else {
        return;
    };
    let unordered = |(a, b): (f64, f64)| if a <= b { (a, b) } else { (b, a) };
    let (r_short, r_long) = unordered((rw, rh));
    let (g_short, g_long) = unordered(caps.physical_pt);
    // A point and a half — comfortably more than the tenth-of-a-
    // millimetre (0.28 pt) rounding a DEVMODE imposes, comfortably less
    // than any real difference between two named forms.
    const TOLERANCE_PT: f64 = 1.5;
    if (r_short - g_short).abs() <= TOLERANCE_PT && (r_long - g_long).abs() <= TOLERANCE_PT {
        return;
    }
    eprintln!(
        "pdfcer: {printer:?} did not give the sheet that was asked for. Requested \
         {rw:.1}x{rh:.1} pt; the device reports {:.1}x{:.1} pt and that is what will be fed. \
         The job is planned against what the device reports, so the placement below is correct \
         for the REAL sheet — but the paper will not be the one requested. A driver that only \
         supports its enumerated forms ignores a custom size outright, and one that has a \
         maximum media width clamps to it.",
        caps.physical_pt.0, caps.physical_pt.1,
    );
}

/// Load a saved driver configuration and check it belongs to this device.
///
/// # Errors
///
/// An exit code. The device check is [`exit::EDIT_REFUSED`] rather than a
/// warning: a `DEVMODE`'s private tail is one driver's private format,
/// so handing it to another is undefined at the driver level rather than
/// merely wrong.
#[cfg(windows)]
pub(crate) fn load_printer_config(
    path: &Path,
    printer: &str,
) -> Result<pdfcer_print::PrinterConfiguration, u8> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", path.display());
            return Err(exit::IO_ERROR);
        }
    };
    let config = pdfcer_print::PrinterConfiguration::from_bytes(&bytes).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", path.display());
        exit::EDIT_REFUSED
    })?;
    config.ensure_device(printer).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", path.display());
        exit::EDIT_REFUSED
    })?;
    Ok(config)
}

/// Print, on stderr, everything a driver configuration says that pdfcer
/// can read.
///
/// pdfcer carries the whole structure and interprets a handful of members
/// of it, so this is a partial view BY CONSTRUCTION — and it says how
/// partial, in bytes, rather than implying it saw everything.
#[cfg(windows)]
pub(crate) fn report_configuration(config: &pdfcer_print::PrinterConfiguration) {
    let s = config.summary();
    let word = |o: Option<pdfcer_print::Orientation>| match o {
        Some(pdfcer_print::Orientation::Landscape) => "landscape".to_owned(),
        Some(pdfcer_print::Orientation::Portrait | pdfcer_print::Orientation::Auto) => {
            "portrait".to_owned()
        }
        None => "-".to_owned(),
    };
    let duplex = match s.duplex {
        Some(pdfcer_print::Duplex::LongEdge) => "long-edge",
        Some(pdfcer_print::Duplex::ShortEdge) => "short-edge",
        Some(pdfcer_print::Duplex::Simplex) => "simplex",
        None => "-",
    };
    println!(
        "settings device={:?} orientation={} paper_form={} form_name={} custom_pt={} \
         duplex={duplex} tray={} pick_tray_by_size={} driver_private_bytes={}",
        s.device,
        word(s.orientation),
        s.paper_form_id
            .map_or_else(|| "-".to_owned(), |v| v.to_string()),
        s.form_name.clone().unwrap_or_else(|| "-".to_owned()),
        s.custom_paper_pt
            .map_or_else(|| "-".to_owned(), |(w, h)| format!("{w:.1}x{h:.1}")),
        s.input_tray
            .map_or_else(|| "-".to_owned(), |v| v.to_string()),
        u32::from(s.picks_tray_by_size),
        s.driver_extra,
    );
}

/// `list-paper-sizes` — the forms a device offers.
///
/// # Why the ID is on every line
///
/// It is what `print --paper` takes, and it is stable where the NAME is
/// not: the same A4 is `"A4"` on one driver and
/// `"A4 (8.2 x 11.7 in; 210 x 297 mm)"` on the next, measured on this
/// machine. A script that pins a form should pin the ID.
///
/// # Exit code
///
/// `0` even for a device that offers none — that is a successful query
/// of an unusual device, and a non-zero exit would make it
/// indistinguishable from "no such printer", which is the one
/// distinction a caller needs.
#[cfg(windows)]
pub(crate) fn cmd_list_paper_sizes(printer: Option<&str>) -> u8 {
    let name = match print_target(printer) {
        Ok(name) => name,
        Err(code) => return code,
    };
    let forms = match pdfcer_print::printer_forms(&name) {
        Ok(forms) => forms,
        Err(err) => {
            eprintln!("pdfcer: {err}");
            return exit::EDIT_REFUSED;
        }
    };
    for form in &forms {
        println!(
            "paper id={} name={:?} size_pt={:.1}x{:.1}",
            form.id, form.name, form.size_pt.0, form.size_pt.1
        );
    }
    if forms
        .iter()
        .any(|f| f.size_pt.0 <= 0.0 || f.size_pt.1 <= 0.0)
    {
        // A zero is the driver declining to state a size, not a sheet of
        // no area. Said plainly, because a shell that plotted it would
        // draw nothing and blame pdfcer.
        eprintln!(
            "pdfcer: some forms report a size of 0x0 — that is the driver declining to \
             state one, not a sheet with no area."
        );
    }
    println!("list-paper-sizes printer={name:?} count={}", forms.len());
    exit::SUCCESS
}

/// `printer-properties` — the driver's own settings dialog.
///
/// # Exit codes
///
/// `0` whether the operator accepted or cancelled. Cancelling is the
/// operator declining, and reporting an error for it would be scolding
/// them for using the dialog correctly — `changed=` on the summary line
/// is what a script branches on.
#[cfg(windows)]
pub(crate) fn cmd_printer_properties(
    printer: Option<&str>,
    save: Option<&Path>,
    from: Option<&Path>,
    no_dialog: bool,
) -> u8 {
    let name = match print_target(printer) {
        Ok(name) => name,
        Err(code) => return code,
    };
    let start_from = match from {
        Some(path) => match load_printer_config(path, &name) {
            Ok(config) => Some(config),
            Err(code) => return code,
        },
        None => None,
    };
    let edited = if no_dialog {
        pdfcer_print::printer_configuration(&name).map(Some)
    } else {
        pdfcer_print::edit_printer_configuration(&name, None, start_from.as_ref())
    };
    let edited = match edited {
        Ok(edited) => edited,
        Err(err) => {
            eprintln!("pdfcer: {err}");
            return exit::EDIT_REFUSED;
        }
    };
    let Some(config) = edited else {
        println!("printer-properties printer={name:?} changed=0 saved=-");
        return exit::SUCCESS;
    };
    report_configuration(&config);
    let saved = match save {
        Some(path) => {
            if let Err(err) = std::fs::write(path, config.as_bytes()) {
                eprintln!("pdfcer: {}: {err}", path.display());
                return exit::IO_ERROR;
            }
            path.display().to_string()
        }
        None => {
            eprintln!(
                "pdfcer: nothing was saved — pass --save PATH to keep these settings for \
                 `print --printer-config`."
            );
            "-".to_owned()
        }
    };
    println!("printer-properties printer={name:?} changed=1 saved={saved:?}");
    exit::SUCCESS
}

/// The non-Windows arm — see `cmd_list_printers` for why this reports
/// rather than vanishing.
#[cfg(not(windows))]
pub(crate) fn cmd_list_paper_sizes(_printer: Option<&str>) -> u8 {
    eprintln!(
        "pdfcer: printing is available on Windows only in this build \
         (docs/decisions/003-distribution-posture.md §4.1)"
    );
    exit::EDIT_REFUSED
}

/// The non-Windows arm — see `cmd_list_printers` for why this reports
/// rather than vanishing.
#[cfg(not(windows))]
pub(crate) fn cmd_printer_properties(
    _printer: Option<&str>,
    _save: Option<&Path>,
    _from: Option<&Path>,
    _no_dialog: bool,
) -> u8 {
    eprintln!(
        "pdfcer: printing is available on Windows only in this build \
         (docs/decisions/003-distribution-posture.md §4.1)"
    );
    exit::EDIT_REFUSED
}
