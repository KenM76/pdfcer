//! Compose a print job's sheets into a new PDF, so a printer that reads PDF
//! receives sheets that are already final: paper, scale, n-up, booklet and
//! poster laid out with the same imposition geometry ([`crate::imposition`])
//! the driver route uses. Both shells call [`compose`]; the bytes it returns
//! are what is sent, at actual size.
//!
//! Each source page is drawn as a form XObject
//! (`EditSession::place_page_content`), which maps its crop box onto a
//! rectangle without turning it. A page that must turn is therefore placed
//! upright on a sheet whose media box is turned, and the sheet's `/Rotate`
//! (ISO 32000-1 §7.7.3.3 Table 30) turns the whole sheet back onto the
//! paper. Annotations do not travel with a form, so the printable ones are
//! burned into their pages first.

use pdfcer_core::edit::{CropBoxEdit, EditSession, PageBox, PageBoxEdit};
use pdfcer_core::page_tree::Rect as PdfRect;

use crate::imposition::{
    self, Binding, BookletSpec, BookletSubset, ImpositionError, NUpGrid, NUpSpec, PageOrder,
    PosterSpec, Rect,
};
use crate::{Orientation, ScaleMode};

/// Why a job could not be composed into a PDF. Each prints as a plain
/// sentence fit for a report line.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ComposeError {
    /// N-up or a booklet over pages whose `/Rotate` differ: one sheet has
    /// one `/Rotate`, so they cannot share it. Holds the layout's name.
    #[error("{0} over pages with different rotations cannot be composed into a PDF")]
    MixedRotations(&'static str),
    /// The imposition planner refused the layout.
    #[error(transparent)]
    Imposition(#[from] ImpositionError),
    /// The sequence named no page of the document.
    #[error("no pages left to print")]
    NoPages,
    /// Cut marks or cell borders were asked for in a build without the
    /// `svg-import` feature, which draws them.
    #[error("cut marks and borders need a build with the svg-import feature")]
    StrokesUnavailable,
    /// The engine refused an edit while drawing the sheets.
    #[error("{0}")]
    Engine(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
}

fn engine<E: std::error::Error + Send + Sync + 'static>(e: E) -> ComposeError {
    ComposeError::Engine(Box::new(e))
}

/// The paper the sheets are composed for, portrait, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paper {
    /// The whole sheet, width then height.
    pub physical_pt: (f64, f64),
    /// The area the printer can mark, width then height.
    pub printable_pt: (f64, f64),
    /// The printable area's top-left corner, from the paper's.
    pub offset_pt: (f64, f64),
}

impl Paper {
    /// The paper as given, turned to portrait if it was reported landscape.
    /// The arguments are a driver's `physical_pt`, `printable_pt` and
    /// `offset_pt` ([`crate::PrinterCaps`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_print::compose::Paper;
    ///
    /// // Landscape Letter, printable area inset 10 pt left and 20 pt top.
    /// let p = Paper::portrait((792.0, 612.0), (752.0, 552.0), (10.0, 20.0));
    /// assert_eq!(p.physical_pt, (612.0, 792.0));
    /// assert_eq!(p.printable_pt, (552.0, 752.0));
    /// ```
    #[must_use]
    pub fn portrait(physical: (f64, f64), printable: (f64, f64), offset: (f64, f64)) -> Self {
        if physical.0 <= physical.1 {
            return Self {
                physical_pt: physical,
                printable_pt: printable,
                offset_pt: offset,
            };
        }
        Self {
            physical_pt: (physical.1, physical.0),
            printable_pt: (printable.1, printable.0),
            offset_pt: (offset.1, physical.0 - offset.0 - printable.0),
        }
    }

    /// The media box size of a sheet whose `/Rotate` is `turn`, and the
    /// printable area within it (top-left origin, y down), such that the
    /// sheet turned clockwise by `turn` lies on this portrait paper.
    #[must_use]
    pub fn frame(&self, turn: u16) -> ((f64, f64), Rect) {
        let (w, h) = self.physical_pt;
        let (ox, oy) = self.offset_pt;
        let (pw, ph) = self.printable_pt;
        match turn % 360 {
            90 => ((h, w), Rect::new(oy, w - ox - pw, ph, pw)),
            180 => ((w, h), Rect::new(w - ox - pw, h - oy - ph, pw, ph)),
            270 => ((h, w), Rect::new(h - oy - ph, ox, ph, pw)),
            _ => ((w, h), Rect::new(ox, oy, pw, ph)),
        }
    }
}

/// How the job's pages become sheets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// One page per sheet, placed by the scale mode.
    Pages,
    /// Several pages to a sheet.
    NUp(NUpSpec),
    /// Pages paired and ordered to fold into a booklet.
    Booklet(BookletSpec),
    /// Each page tiled across several sheets.
    Poster(PosterSpec),
}

impl Shape {
    /// The shape of a job from its options, with the planner settings the
    /// driver route uses: `n_up` is pages per sheet and whether each cell
    /// is bordered; the first option given wins, and [`Shape::Pages`] when
    /// none is.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_print::compose::Shape;
    ///
    /// assert_eq!(Shape::for_job(None, None, None), Shape::Pages);
    /// assert!(matches!(Shape::for_job(Some((4, true)), None, None), Shape::NUp(_)));
    /// ```
    #[must_use]
    pub fn for_job(
        n_up: Option<(u32, bool)>,
        booklet: Option<(Binding, BookletSubset)>,
        poster: Option<PosterSpec>,
    ) -> Self {
        if let Some((count, border)) = n_up {
            return Self::NUp(NUpSpec {
                grid: NUpGrid::Count(count),
                order: PageOrder::Horizontal,
                border,
                auto_rotate: true,
            });
        }
        if let Some((binding, subset)) = booklet {
            return Self::Booklet(BookletSpec {
                binding,
                subset,
                sheets: None,
                auto_rotate: true,
            });
        }
        poster.map_or(Self::Pages, Self::Poster)
    }

    /// Whether a job of this shape must be composed before a printer that
    /// reads PDF can print it as asked: anything but whole pages, a paper
    /// other than the printer's own (`paper_chosen`), or a percentage
    /// scale. A job needing none of these is sent as it is.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_print::compose::Shape;
    /// use pdfcer_print::ScaleMode;
    ///
    /// assert!(!Shape::Pages.needs_composing(false, ScaleMode::Fit));
    /// assert!(Shape::Pages.needs_composing(false, ScaleMode::Custom(0.5)));
    /// ```
    #[must_use]
    pub fn needs_composing(self, paper_chosen: bool, mode: ScaleMode) -> bool {
        self != Self::Pages || paper_chosen || matches!(mode, ScaleMode::Custom(_))
    }
}

/// What to compose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Request<'a> {
    /// The paper to compose for.
    pub paper: Paper,
    /// How pages become sheets.
    pub shape: Shape,
    /// How a page is scaled into its sheet; [`Shape::Pages`] only.
    pub mode: ScaleMode,
    /// Whether pages print portrait, landscape, or as each is shaped.
    pub orientation: Orientation,
    /// The file name, for poster tile labels.
    pub label: &'a str,
}

/// The composed PDF and what composing it decided.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Composed {
    /// The PDF to send, to be printed at actual size.
    pub pdf: Vec<u8>,
    /// Pages in the composed PDF: one per side of paper.
    pub pages: usize,
    /// What was left out or clipped, one plain sentence each, for the
    /// shell to print or show.
    pub notes: Vec<String>,
}

/// One source page placed on a sheet.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Item {
    /// 0-based page of the source document.
    pub source: usize,
    /// Where its crop box lands, in media points (top-left origin, y down).
    pub rect: Rect,
    /// A poster tile: the part of the page to show instead of its crop
    /// box, in default user space.
    pub crop: Option<PdfRect>,
}

/// One page of the composed PDF.
#[derive(Clone, Debug, Default, PartialEq)]
struct Sheet {
    /// `/Rotate`: 0, 90, 180 or 270.
    pub rotate: u16,
    pub items: Vec<Item>,
    /// Cut marks, in media points.
    pub lines: Vec<((f64, f64), (f64, f64))>,
    /// N-up cell borders, in media points.
    pub boxes: Vec<Rect>,
    /// A poster tile's label box and text.
    pub label: Option<(Rect, String)>,
}

/// A source page's crop box and its `/Rotate`, normalised to 0..360.
#[derive(Clone, Copy, Debug)]
struct Src {
    crop: PdfRect,
    rotate: u16,
}

impl Src {
    fn size(self) -> (f64, f64) {
        (self.crop.width(), self.crop.height())
    }
}

/// Compose `sequence` (0-based pages of `source`, in print order; an index
/// past the last page is skipped) into a new PDF.
///
/// `source` must be a session opened for this job and never saved: its
/// printable annotations are burned into their pages and its crop boxes
/// are changed for poster tiles.
///
/// # Errors
///
/// [`ComposeError`] when the job cannot be expressed as a PDF: n-up or a
/// booklet over pages with different `/Rotate`, a layout the imposition
/// planner refuses, no page to print, or an edit the engine refused.
///
/// # Examples
///
/// ```
/// use pdfcer_core::edit::EditSession;
/// use pdfcer_core::page_tree::Rect;
/// use pdfcer_core::text_edit::blank_document;
/// use pdfcer_print::compose::{compose, Paper, Request, Shape};
/// use pdfcer_print::{Orientation, ScaleMode};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let letter = Rect::from_corners(0.0, 0.0, 612.0, 792.0);
/// let mut job = EditSession::new(blank_document(letter, 3)?);
/// let request = Request {
///     paper: Paper::portrait((612.0, 792.0), (576.0, 756.0), (18.0, 18.0)),
///     shape: Shape::for_job(Some((2, false)), None, None),
///     mode: ScaleMode::Fit,
///     orientation: Orientation::Auto,
///     label: "report.pdf",
/// };
/// let composed = compose(&mut job, &[0, 1, 2], &request)?;
/// assert_eq!(composed.pages, 2);
/// assert!(composed.pdf.starts_with(b"%PDF-"));
/// # Ok(())
/// # }
/// ```
pub fn compose(
    source: &mut EditSession,
    sequence: &[usize],
    req: &Request<'_>,
) -> Result<Composed, ComposeError> {
    let mut notes = burn_annotations(source, sequence);
    let pages = source.pages().map_err(engine)?;
    let srcs: Vec<Src> = pages
        .iter()
        .map(|p| Src {
            crop: p.crop_box,
            rotate: p.rotate % 360,
        })
        .collect();
    let sheets = match req.shape {
        Shape::Pages => plan_pages(&srcs, sequence, req, &mut notes),
        Shape::NUp(spec) => plan_n_up(&srcs, sequence, req, spec)?,
        Shape::Booklet(spec) => plan_booklet(&srcs, sequence, req, spec)?,
        Shape::Poster(spec) => plan_poster(&srcs, sequence, req, spec, &mut notes)?,
    };
    let pdf = build(source, &sheets, &req.paper)?;
    Ok(Composed {
        pdf,
        pages: sheets.len(),
        notes,
    })
}

/// Burn the printable annotations of `sequence` into their pages, and the
/// form fields when every widget prints. Returns what was left out.
fn burn_annotations(source: &mut EditSession, sequence: &[usize]) -> Vec<String> {
    use pdfcer_core::annot::AnnotFlags;
    let Ok(pages) = source.pages() else {
        return Vec::new();
    };
    let prints = |f: AnnotFlags| f.0 & AnnotFlags::PRINT != 0 && f.0 & AnnotFlags::HIDDEN == 0;
    let (mut kept, mut widgets_print, mut has_widgets) = (0usize, true, false);
    for &i in sequence {
        let Some(page) = pages.get(i) else { continue };
        let annots = pdfcer_core::annot::page_annotations(&source.graph(), page.id);
        let mut burn = Vec::new();
        for a in &annots {
            if a.is_widget() {
                has_widgets = true;
                widgets_print &= prints(a.flags);
            } else if prints(a.flags) && a.subtype != b"Popup" {
                burn.extend(a.id);
            }
        }
        for id in burn {
            if source.flatten_annotations(i, Some(&[id])).is_err() {
                kept += 1;
            }
        }
    }
    let mut notes = Vec::new();
    if has_widgets && widgets_print {
        if source.flatten_fields(None).is_err() {
            notes.push("form fields could not be drawn into the sheets and do not print.".into());
        }
    } else if has_widgets {
        notes.push(
            "form fields do not print on the composed sheets: some are set not to print, and \
             pdfcer draws all of a form's fields or none."
                .into(),
        );
    }
    if kept > 0 {
        notes.push(format!(
            "{kept} annotation(s) could not be drawn into the sheets and do not print."
        ));
    }
    notes
}

/// The quarter turn that prints a page landscape: counter-clockwise, as a
/// driver's landscape does.
const LANDSCAPE: u16 = 270;

/// One page per sheet, turned for the orientation and placed by the mode.
fn plan_pages(
    srcs: &[Src],
    sequence: &[usize],
    req: &Request<'_>,
    notes: &mut Vec<String>,
) -> Vec<Sheet> {
    let mut clipped = 0usize;
    let mut sheets = Vec::new();
    for &i in sequence {
        let Some(&src) = srcs.get(i) else { continue };
        let (w, h) = src.size();
        let shown_landscape = if src.rotate % 180 == 90 { h > w } else { w > h };
        let turn = match req.orientation {
            Orientation::Portrait => 0,
            Orientation::Landscape => LANDSCAPE,
            Orientation::Auto if shown_landscape => LANDSCAPE,
            Orientation::Auto => 0,
        };
        let rotate = (src.rotate + turn) % 360;
        let (_, area) = req.paper.frame(rotate);
        let placed = crate::place_page((w, h), (area.width, area.height), req.mode);
        clipped += usize::from(placed.clipped);
        let rect = Rect::new(
            area.x + placed.offset_x_pt,
            area.y + placed.offset_y_pt,
            w * placed.scale,
            h * placed.scale,
        );
        sheets.push(Sheet {
            rotate,
            items: vec![Item {
                source: i,
                rect,
                crop: None,
            }],
            ..Sheet::default()
        });
    }
    if clipped > 0 {
        notes.push(format!(
            "{clipped} page(s) are larger than the printable area at this scale and lose their edges."
        ));
    }
    sheets
}

/// The one `/Rotate` every page of `sequence` shares.
fn common_rotate(
    srcs: &[Src],
    sequence: &[usize],
    what: &'static str,
) -> Result<u16, ComposeError> {
    let mut turns = sequence
        .iter()
        .filter_map(|&i| srcs.get(i))
        .map(|s| s.rotate);
    let first = turns.next().unwrap_or(0);
    if turns.any(|t| t != first) {
        return Err(ComposeError::MixedRotations(what));
    }
    Ok(first)
}

/// The turn a sheet's pages share: the source's, plus a landscape turn when
/// asked, plus a clockwise quarter turn when the planner would turn every
/// page in its cell (`any_rotated` on the first plan).
fn sheet_turn(base: u16, req: &Request<'_>, any_rotated: bool) -> u16 {
    let landscape = if req.orientation == Orientation::Landscape {
        LANDSCAPE
    } else {
        0
    };
    (base + landscape + if any_rotated { 90 } else { 0 }) % 360
}

fn moved(r: Rect, area: Rect) -> Rect {
    Rect::new(area.x + r.x, area.y + r.y, r.width, r.height)
}

/// N-up: the grid planned on the turned sheet, so each page sits upright in
/// media space and the sheet's `/Rotate` turns it.
fn plan_n_up(
    srcs: &[Src],
    sequence: &[usize],
    req: &Request<'_>,
    spec: NUpSpec,
) -> Result<Vec<Sheet>, ComposeError> {
    let base = common_rotate(srcs, sequence, "n-up")?;
    let sizes: Vec<(f64, f64)> = sequence
        .iter()
        .filter_map(|&i| srcs.get(i))
        .map(|s| s.size())
        .collect();
    let plan = |turn: u16, spec: &NUpSpec| {
        let (_, area) = req.paper.frame(turn);
        imposition::plan_n_up((area.width, area.height), &sizes, spec)
            .map(|l| (l, area))
            .map_err(ComposeError::from)
    };
    let first = sheet_turn(base, req, false);
    let (mut layout, mut area) = plan(first, &spec)?;
    let mut rotate = first;
    if layout.slots.iter().any(|s| s.fit.rotated) {
        rotate = sheet_turn(base, req, true);
        (layout, area) = plan(
            rotate,
            &NUpSpec {
                auto_rotate: false,
                ..spec
            },
        )?;
    }
    let mut sheets = vec![
        Sheet {
            rotate,
            ..Sheet::default()
        };
        layout.sheets
    ];
    for slot in &layout.slots {
        let (Some(sheet), Some(&source)) = (sheets.get_mut(slot.sheet), sequence.get(slot.source))
        else {
            continue;
        };
        sheet.items.push(Item {
            source,
            rect: moved(slot.fit.rect, area),
            crop: None,
        });
        sheet.boxes.extend(slot.border.map(|b| moved(b, area)));
    }
    Ok(sheets)
}

/// Booklet: each side of each sheet is one composed page, in the order the
/// planner first names it.
fn plan_booklet(
    srcs: &[Src],
    sequence: &[usize],
    req: &Request<'_>,
    spec: BookletSpec,
) -> Result<Vec<Sheet>, ComposeError> {
    let base = common_rotate(srcs, sequence, "a booklet")?;
    let sizes: Vec<(f64, f64)> = sequence
        .iter()
        .filter_map(|&i| srcs.get(i))
        .map(|s| s.size())
        .collect();
    let plan = |turn: u16, spec: &BookletSpec| {
        let (_, area) = req.paper.frame(turn);
        imposition::plan_booklet((area.width, area.height), &sizes, spec)
            .map(|l| (l, area))
            .map_err(ComposeError::from)
    };
    let first = sheet_turn(base, req, false);
    let (mut layout, mut area) = plan(first, &spec)?;
    let mut rotate = first;
    if layout
        .slots
        .iter()
        .any(|s| s.fit.is_some_and(|f| f.rotated))
    {
        rotate = sheet_turn(base, req, true);
        (layout, area) = plan(
            rotate,
            &BookletSpec {
                auto_rotate: false,
                ..spec
            },
        )?;
    }
    let mut order: Vec<(usize, imposition::BookletSide)> = Vec::new();
    let mut sheets: Vec<Sheet> = Vec::new();
    for slot in &layout.slots {
        let key = (slot.sheet, slot.side);
        let at = order.iter().position(|k| *k == key).unwrap_or_else(|| {
            order.push(key);
            sheets.push(Sheet {
                rotate,
                ..Sheet::default()
            });
            order.len() - 1
        });
        let (Some(fit), Some(&source), Some(sheet)) = (
            slot.fit,
            slot.source.and_then(|s| sequence.get(s)),
            sheets.get_mut(at),
        ) else {
            continue;
        };
        sheet.items.push(Item {
            source,
            rect: moved(fit.rect, area),
            crop: None,
        });
    }
    Ok(sheets)
}

/// Poster: each page tiled across sheets at the page's own turn, each tile
/// shown through a crop box cut to its part of the page.
fn plan_poster(
    srcs: &[Src],
    sequence: &[usize],
    req: &Request<'_>,
    spec: PosterSpec,
    notes: &mut Vec<String>,
) -> Result<Vec<Sheet>, ComposeError> {
    let mut sheets = Vec::new();
    for &i in sequence {
        let Some(&src) = srcs.get(i) else { continue };
        let (_, area) = req.paper.frame(src.rotate);
        let printable = (area.width, area.height);
        if !spec.tiles_page(src.size(), printable) {
            let one = Request {
                orientation: Orientation::Portrait,
                ..*req
            };
            sheets.extend(plan_pages(srcs, &[i], &one, notes));
            continue;
        }
        let layout = imposition::plan_poster(printable, src.size(), &spec)?;
        for tile in &layout.tiles {
            sheets.push(tile_sheet(&layout, tile, i, src, area, req.label));
        }
    }
    Ok(sheets)
}

fn tile_sheet(
    layout: &imposition::PosterLayout,
    tile: &imposition::PosterTile,
    source: usize,
    src: Src,
    area: Rect,
    label: &str,
) -> Sheet {
    let s = tile.source_pt;
    // `source_pt` is top-left, y down, in the crop box's own points.
    let crop = PdfRect::from_corners(
        src.crop.llx + s.x,
        src.crop.ury - s.y - s.height,
        src.crop.llx + s.x + s.width,
        src.crop.ury - s.y,
    );
    let at = |p: (f64, f64)| (area.x + p.0, area.y + p.1);
    Sheet {
        rotate: src.rotate,
        items: vec![Item {
            source,
            rect: moved(tile.sheet_pt, area),
            crop: Some(crop),
        }],
        lines: layout
            .cut_mark_segments(tile)
            .iter()
            .map(|m| (at(m.from), at(m.to)))
            .collect(),
        boxes: Vec::new(),
        label: layout
            .label_rect(tile)
            .map(|r| (moved(r, area), layout.tile_label(tile, label))),
    }
}

/// Write `sheets` as a new PDF drawn from `source`.
fn build(
    source: &mut EditSession,
    sheets: &[Sheet],
    paper: &Paper,
) -> Result<Vec<u8>, ComposeError> {
    use pdfcer_core::text_edit::blank_document;
    let media = |turn: u16| {
        let (w, h) = paper.frame(turn).0;
        PdfRect::from_corners(0.0, 0.0, w, h)
    };
    let first = sheets.first().ok_or(ComposeError::NoPages)?;
    let mut target = EditSession::new(blank_document(media(first.rotate), 1).map_err(engine)?);
    let upright = blank_document(media(0), 1).map_err(engine)?;
    let turned = blank_document(media(90), 1).map_err(engine)?;
    for sheet in sheets.iter().skip(1) {
        let blank = if sheet.rotate % 180 == 0 {
            &upright
        } else {
            &turned
        };
        target
            .insert_pages(
                &blank.view(),
                &[0],
                pdfcer_core::pageops::InsertPosition::End,
            )
            .map_err(engine)?;
    }
    for (index, sheet) in sheets.iter().enumerate() {
        place_sheet(
            source,
            &mut target,
            index,
            sheet,
            paper.frame(sheet.rotate).0,
        )?;
    }
    target
        .to_full_bytes(&pdfcer_core::writer::SaveOptions::default())
        .map(|(bytes, _)| bytes)
        .map_err(engine)
}

/// Draw one sheet's pages, marks, borders and label on `target`'s page
/// `index`, whose media box is `media` points.
fn place_sheet(
    source: &mut EditSession,
    target: &mut EditSession,
    index: usize,
    sheet: &Sheet,
    media: (f64, f64),
) -> Result<(), ComposeError> {
    let pdf = |r: Rect| PdfRect::from_corners(r.x, media.1 - r.bottom(), r.right(), media.1 - r.y);
    if sheet.rotate != 0 {
        target
            .set_page_rotation(index, i32::from(sheet.rotate))
            .map_err(engine)?;
    }
    for item in &sheet.items {
        if let Some(crop) = item.crop {
            let edit = PageBoxEdit::new(item.source, PageBox::Crop, CropBoxEdit::Set(crop));
            source.set_page_boxes(&[edit]).map_err(engine)?;
        }
        target
            .place_page_content(&source.view(), item.source, index, pdf(item.rect))
            .map_err(engine)?;
    }
    if let Some(svg) = strokes_svg(sheet, media) {
        place_strokes(target, index, &svg, media)?;
    }
    if let Some((r, text)) = &sheet.label {
        let mut req = pdfcer_core::text_edit::AddTextRequest::new(
            index,
            (r.x, media.1 - r.bottom() + r.height * 0.2),
            text.clone(),
        );
        req.size = (r.height * 0.8).max(4.0);
        target.add_text(&req).map_err(engine)?;
    }
    Ok(())
}

/// Draw the marks-and-borders `svg` over the whole of page `index`.
#[cfg(feature = "svg-import")]
fn place_strokes(
    target: &mut EditSession,
    index: usize,
    svg: &str,
    media: (f64, f64),
) -> Result<(), ComposeError> {
    let imported = pdfcer_core::svg_import::import(svg.as_bytes()).map_err(engine)?;
    let whole = PdfRect::from_corners(0.0, 0.0, media.0, media.1);
    target
        .add_svg(index, whole, &imported)
        .map(|_| ())
        .map_err(engine)
}

/// Without SVG import the marks cannot be placed, so the layout is refused
/// rather than printed without the cut marks or borders it asked for.
#[cfg(not(feature = "svg-import"))]
fn place_strokes(
    _target: &mut EditSession,
    _index: usize,
    _svg: &str,
    _media: (f64, f64),
) -> Result<(), ComposeError> {
    Err(ComposeError::StrokesUnavailable)
}

/// The sheet's cut marks and borders as one SVG over the whole media box,
/// or `None` when it has neither.
fn strokes_svg(sheet: &Sheet, media: (f64, f64)) -> Option<String> {
    if sheet.lines.is_empty() && sheet.boxes.is_empty() {
        return None;
    }
    let mut d = String::new();
    for (a, b) in &sheet.lines {
        d.push_str(&format!("M{:.3} {:.3}L{:.3} {:.3}", a.0, a.1, b.0, b.1));
    }
    for r in &sheet.boxes {
        d.push_str(&format!(
            "M{:.3} {:.3}H{:.3}V{:.3}H{:.3}Z",
            r.x,
            r.y,
            r.right(),
            r.bottom(),
            r.x
        ));
    }
    let (w, h) = media;
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" \
         viewBox=\"0 0 {w} {h}\"><path d=\"{d}\" fill=\"none\" stroke=\"#000\" \
         stroke-width=\"0.5\"/></svg>"
    ))
}

#[cfg(test)]
mod tests;
