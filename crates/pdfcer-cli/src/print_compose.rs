//! Compose a print job's sheets into a new PDF for `print --pdf-direct`, so
//! a printer that reads PDF receives sheets that are already final: paper,
//! scale, n-up, booklet and poster laid out with the same imposition
//! geometry (`pdfcer_print::imposition`) the driver route uses.
//!
//! Each source page is drawn as a form XObject
//! ([`EditSession::place_page_content`]), which maps its crop box onto a
//! rectangle without turning it. A page that must turn is therefore placed
//! upright on a sheet whose media box is turned, and the sheet's `/Rotate`
//! (ISO 32000-1 §7.7.3.3 Table 30) turns the whole sheet back onto the
//! paper. Annotations do not travel with a form, so the printable ones are
//! burned into their pages first.

use pdfcer_core::edit::{CropBoxEdit, EditSession, PageBox, PageBoxEdit};
use pdfcer_core::page_tree::Rect as PdfRect;
use pdfcer_print::imposition::{self, BookletSpec, NUpSpec, PosterSpec, Rect};
use pdfcer_print::{Orientation, ScaleMode};

/// The paper the sheets are composed for, portrait, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Paper {
    pub physical_pt: (f64, f64),
    pub printable_pt: (f64, f64),
    /// The printable area's top-left corner, from the paper's.
    pub offset_pt: (f64, f64),
}

impl Paper {
    /// The paper as given, turned to portrait if it was reported landscape.
    // Only the Windows driver-caps path and the tests construct one.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn portrait(
        physical: (f64, f64),
        printable: (f64, f64),
        offset: (f64, f64),
    ) -> Self {
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
    pub(crate) fn frame(&self, turn: u16) -> ((f64, f64), Rect) {
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
pub(crate) enum Shape {
    /// One page per sheet, placed by the scale mode.
    Pages,
    NUp(NUpSpec),
    Booklet(BookletSpec),
    Poster(PosterSpec),
}

/// What to compose.
pub(crate) struct Request<'a> {
    pub paper: Paper,
    pub shape: Shape,
    pub mode: ScaleMode,
    pub orientation: Orientation,
    /// The file name, for poster tile labels.
    pub label: &'a str,
}

/// The composed PDF and what composing it decided.
pub(crate) struct Composed {
    pub pdf: Vec<u8>,
    /// Pages in the composed PDF: one per side of paper.
    pub pages: usize,
    pub notes: Vec<String>,
}

/// One source page placed on a sheet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Item {
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
pub(crate) struct Sheet {
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

/// Compose `sequence` (0-based pages of `source`, in print order) into a
/// new PDF. `source` is the job's own never-saved session; its printable
/// annotations are burned in and its crop boxes are changed for poster
/// tiles.
///
/// # Errors
///
/// A plain sentence when the job cannot be expressed as a PDF: n-up or a
/// booklet over pages with different `/Rotate`, an imposition error, or an
/// edit the engine refused.
pub(crate) fn compose(
    source: &mut EditSession,
    sequence: &[usize],
    req: &Request<'_>,
) -> Result<Composed, String> {
    let mut notes = burn_annotations(source, sequence);
    let pages = source.pages().map_err(|e| e.to_string())?;
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
        let placed = pdfcer_print::place_page((w, h), (area.width, area.height), req.mode);
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
fn common_rotate(srcs: &[Src], sequence: &[usize], what: &str) -> Result<u16, String> {
    let mut turns = sequence
        .iter()
        .filter_map(|&i| srcs.get(i))
        .map(|s| s.rotate);
    let first = turns.next().unwrap_or(0);
    if turns.any(|t| t != first) {
        return Err(format!(
            "{what} over pages with different rotations cannot be composed into a PDF"
        ));
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
) -> Result<Vec<Sheet>, String> {
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
            .map_err(|e| e.to_string())
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
) -> Result<Vec<Sheet>, String> {
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
            .map_err(|e| e.to_string())
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
) -> Result<Vec<Sheet>, String> {
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
        let layout =
            imposition::plan_poster(printable, src.size(), &spec).map_err(|e| e.to_string())?;
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
fn build(source: &mut EditSession, sheets: &[Sheet], paper: &Paper) -> Result<Vec<u8>, String> {
    use pdfcer_core::text_edit::blank_document;
    let err = |e: &dyn std::fmt::Display| e.to_string();
    let media = |turn: u16| {
        let (w, h) = paper.frame(turn).0;
        PdfRect::from_corners(0.0, 0.0, w, h)
    };
    let first = sheets.first().ok_or("no pages left to print")?;
    let mut target = EditSession::new(blank_document(media(first.rotate), 1).map_err(|e| err(&e))?);
    let upright = blank_document(media(0), 1).map_err(|e| err(&e))?;
    let turned = blank_document(media(90), 1).map_err(|e| err(&e))?;
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
            .map_err(|e| err(&e))?;
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
        .map_err(|e| err(&e))
}

/// Draw one sheet's pages, marks, borders and label on `target`'s page
/// `index`, whose media box is `media` points.
fn place_sheet(
    source: &mut EditSession,
    target: &mut EditSession,
    index: usize,
    sheet: &Sheet,
    media: (f64, f64),
) -> Result<(), String> {
    let err = |e: &dyn std::fmt::Display| e.to_string();
    let pdf = |r: Rect| PdfRect::from_corners(r.x, media.1 - r.bottom(), r.right(), media.1 - r.y);
    if sheet.rotate != 0 {
        target
            .set_page_rotation(index, i32::from(sheet.rotate))
            .map_err(|e| err(&e))?;
    }
    for item in &sheet.items {
        if let Some(crop) = item.crop {
            let edit = PageBoxEdit::new(item.source, PageBox::Crop, CropBoxEdit::Set(crop));
            source.set_page_boxes(&[edit]).map_err(|e| err(&e))?;
        }
        target
            .place_page_content(&source.view(), item.source, index, pdf(item.rect))
            .map_err(|e| err(&e))?;
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
        target.add_text(&req).map_err(|e| err(&e))?;
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
) -> Result<(), String> {
    let imported = pdfcer_core::svg_import::import(svg.as_bytes()).map_err(|e| e.to_string())?;
    let whole = PdfRect::from_corners(0.0, 0.0, media.0, media.1);
    target
        .add_svg(index, whole, &imported)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Without SVG import the marks cannot be placed, so the layout is refused
/// rather than printed without the cut marks or borders it asked for.
#[cfg(not(feature = "svg-import"))]
fn place_strokes(
    _target: &mut EditSession,
    _index: usize,
    _svg: &str,
    _media: (f64, f64),
) -> Result<(), String> {
    Err("cut marks and borders need a build with the svg-import feature".into())
}

/// The sheet's cut marks and borders as one SVG over the whole media box,
/// or `None` when it has neither.
pub(crate) fn strokes_svg(sheet: &Sheet, media: (f64, f64)) -> Option<String> {
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
pub(crate) mod tests;
