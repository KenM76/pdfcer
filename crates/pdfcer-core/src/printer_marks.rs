//! Printer's marks: crop (trim) marks, bleed marks, registration targets,
//! colour bars and a page-information line, drawn in a band outside the
//! trim box.
//!
//! ISO 32000-1 §14.11.2: the trim box positions the page and is the source
//! of cut marks; the bleed box is the region page content is clipped to in
//! production, and printing marks fall outside it. Marks are page content
//! marked `/Artifact << /Type /Page >>` (§14.8.2.2 Table 330: production
//! aids such as cut marks and colour bars) and stroked in the
//! `/Separation /All` colour space (§8.6.6.4), so they image on every plate.
//!
//! Pure types and geometry; [`crate::edit::EditSession::add_printer_marks`]
//! writes them.

use crate::bates::{helvetica_width, winansi_bytes, winansi_code};
use crate::page_tree::Rect;

/// Which marks to draw and their geometry, in points.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PrinterMarks {
    /// Crop marks at the trim box corners.
    pub trim: bool,
    /// Bleed marks at the bleed box corners.
    pub bleed: bool,
    /// Registration targets at the middle of each side.
    pub registration: bool,
    /// Process-colour and grey-tint patches along the top.
    pub color_bars: bool,
    /// A line along the bottom: [`Self::label`], the page number and count.
    pub page_info: bool,
    /// Gap between the trim edge and where marks start; widened to clear
    /// the bleed box. At least 0.
    pub offset: f64,
    /// Length of a crop mark, and the depth of the marks band. Above 0.
    pub length: f64,
    /// Stroke width of the marks. Above 0.
    pub line_width: f64,
    /// The words that start the page-information line (a file name, a job).
    pub label: String,
    /// 0-based pages to mark; `None` marks every page.
    pub pages: Option<Vec<usize>>,
}

impl PrinterMarks {
    /// Every mark: 6 pt offset, 18 pt marks, 0.25 pt lines, every page.
    #[must_use]
    pub fn all(label: impl Into<String>) -> Self {
        Self {
            trim: true,
            bleed: true,
            registration: true,
            color_bars: true,
            page_info: true,
            offset: 6.0,
            length: 18.0,
            line_width: 0.25,
            label: label.into(),
            pages: None,
        }
    }

    /// [`Self::all`] with every mark turned off; turn on the ones wanted.
    #[must_use]
    pub fn none(label: impl Into<String>) -> Self {
        Self {
            trim: false,
            bleed: false,
            registration: false,
            color_bars: false,
            page_info: false,
            ..Self::all(label)
        }
    }

    /// Whether any mark is on.
    #[must_use]
    pub const fn any(&self) -> bool {
        self.trim || self.bleed || self.registration || self.color_bars || self.page_info
    }

    /// Refuse marks that cannot be drawn.
    ///
    /// # Errors
    ///
    /// [`PrinterMarksError::NoMarks`], [`PrinterMarksError::Geometry`] for a
    /// non-finite or out-of-range number, [`PrinterMarksError::Unencodable`]
    /// for a label character outside `WinAnsiEncoding`.
    pub fn check(&self) -> Result<(), PrinterMarksError> {
        if !self.any() {
            return Err(PrinterMarksError::NoMarks);
        }
        for (what, value, min_exclusive) in [
            ("offset", self.offset, false),
            ("length", self.length, true),
            ("line width", self.line_width, true),
        ] {
            let low = if min_exclusive {
                value <= 0.0
            } else {
                value < 0.0
            };
            if !value.is_finite() || low || value > 144.0 {
                return Err(PrinterMarksError::Geometry { what, value });
            }
        }
        if let Some(c) = self.label.chars().find(|&c| winansi_code(c).is_none()) {
            return Err(PrinterMarksError::Unencodable(c));
        }
        Ok(())
    }

    /// The page-information line for page `number` (1-based) of `count`.
    #[must_use]
    pub fn info_line(&self, number: usize, count: usize) -> String {
        let label = self.label.trim();
        if label.is_empty() {
            format!("page {number} of {count}")
        } else {
            format!("{label}   page {number} of {count}") // string-gap-exempt: the page-info line separates file and page with a wide gap
        }
    }
}

/// Why marks could not be added.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum PrinterMarksError {
    /// Every mark was turned off.
    #[error("no printer's marks were asked for")]
    NoMarks,
    /// A size that is negative, zero where it must not be, above 144 pt or
    /// not finite.
    #[error("printer's marks {what} {value} is out of range")]
    Geometry {
        /// Which size.
        what: &'static str,
        /// Its value.
        value: f64,
    },
    /// The label has a character Helvetica's `WinAnsiEncoding` lacks.
    #[error("printer's marks label has {0:?}, which WinAnsiEncoding cannot write")]
    Unencodable(char),
    /// The page selection was empty.
    #[error("no pages were selected for printer's marks")]
    NoPages,
}

/// What [`crate::edit::EditSession::add_printer_marks`] did.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PrinterMarksOutcome {
    /// 0-based pages marked, ascending.
    pub pages: Vec<usize>,
    /// Marked pages with no `/TrimBox`: their crop box was taken as the trim.
    pub trim_assumed: Vec<usize>,
    /// Points the largest marked page grew by on each side.
    pub grown_by: f64,
}

/// Where one page's marks go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MarksLayout {
    pub trim: Rect,
    pub bleed: Rect,
    /// Gap from the trim edge to the marks band (offset, widened to clear
    /// the bleed).
    pub gap: f64,
    /// The new media and crop box.
    pub sheet: Rect,
}

impl MarksLayout {
    /// The layout for a page with `trim` and `bleed` (the bleed taken as at
    /// least the trim).
    pub(crate) fn new(marks: &PrinterMarks, trim: Rect, bleed: Rect) -> Self {
        let bleed = union(trim, bleed);
        let reach = (trim.llx - bleed.llx)
            .max(trim.lly - bleed.lly)
            .max(bleed.urx - trim.urx)
            .max(bleed.ury - trim.ury);
        let gap = marks.offset.max(reach);
        let band = gap + marks.length;
        let sheet = Rect {
            llx: trim.llx - band,
            lly: trim.lly - band,
            urx: trim.urx + band,
            ury: trim.ury + band,
        };
        Self {
            trim,
            bleed,
            gap,
            sheet,
        }
    }

    /// Points added on each side of the trim.
    pub(crate) fn band(&self) -> f64 {
        self.trim.llx - self.sheet.llx
    }
}

fn union(a: Rect, b: Rect) -> Rect {
    Rect {
        llx: a.llx.min(b.llx),
        lly: a.lly.min(b.lly),
        urx: a.urx.max(b.urx),
        ury: a.ury.max(b.ury),
    }
}

/// The clip that opens the page: everything after it is cut to the bleed.
pub(crate) fn clip_content(bleed: Rect) -> Vec<u8> {
    format!(
        "q {} {} {} {} re W n\n",
        num(bleed.llx),
        num(bleed.lly),
        num(bleed.urx - bleed.llx),
        num(bleed.ury - bleed.lly)
    )
    .into_bytes()
}

/// The marks stream: closes the clip [`clip_content`] opened, then draws.
/// `all` is the `/Separation /All` colour space resource name, `font` the
/// Helvetica font resource name.
pub(crate) fn marks_content(
    marks: &PrinterMarks,
    layout: &MarksLayout,
    all: &[u8],
    font: &[u8],
    info: &str,
) -> Vec<u8> {
    let mut s = String::from("\nQ\nq /Artifact <</Type /Page>> BDC\n");
    let all = String::from_utf8_lossy(all);
    s += &format!(
        "/{all} CS 1 SCN /{all} cs 1 scn {} w 0 J 0 j [] 0 d\n",
        num(marks.line_width)
    );
    if marks.trim {
        corner_marks(&mut s, layout.trim, layout.trim, layout.gap, marks.length);
    }
    if marks.bleed && layout.bleed != layout.trim {
        corner_marks(
            &mut s,
            layout.bleed,
            layout.trim,
            layout.gap,
            marks.length / 2.0,
        );
    }
    if marks.registration {
        registration_targets(&mut s, layout, marks.length);
    }
    if marks.color_bars {
        color_bars(&mut s, layout, marks.length);
    }
    if marks.page_info {
        page_info(
            &mut s,
            layout,
            marks.length,
            &String::from_utf8_lossy(font),
            info,
        );
    }
    s += "EMC Q\n";
    s.into_bytes()
}

/// Two strokes per corner of `at`, running outward from `gap` beyond
/// `trim` for `len`: horizontal ones at `at`'s y, vertical ones at its x.
fn corner_marks(s: &mut String, at: Rect, trim: Rect, gap: f64, len: f64) {
    let (l0, l1) = (trim.llx - gap, trim.llx - gap - len);
    let (r0, r1) = (trim.urx + gap, trim.urx + gap + len);
    let (b0, b1) = (trim.lly - gap, trim.lly - gap - len);
    let (t0, t1) = (trim.ury + gap, trim.ury + gap + len);
    for y in [at.lly, at.ury] {
        line(s, l0, y, l1, y);
        line(s, r0, y, r1, y);
    }
    for x in [at.llx, at.urx] {
        line(s, x, b0, x, b1);
        line(s, x, t0, x, t1);
    }
}

fn line(s: &mut String, x0: f64, y0: f64, x1: f64, y1: f64) {
    *s += &format!("{} {} m {} {} l S\n", num(x0), num(y0), num(x1), num(y1));
}

/// A circle with a cross through it at the middle of each side's band.
fn registration_targets(s: &mut String, layout: &MarksLayout, len: f64) {
    let t = layout.trim;
    let mid = layout.gap + len / 2.0;
    let r = len / 3.0;
    let (cx, cy) = (f64::midpoint(t.llx, t.urx), f64::midpoint(t.lly, t.ury));
    for (x, y) in [
        (cx, t.ury + mid),
        (cx, t.lly - mid),
        (t.llx - mid, cy),
        (t.urx + mid, cy),
    ] {
        circle(s, x, y, r);
        line(s, x - r * 1.4, y, x + r * 1.4, y);
        line(s, x, y - r * 1.4, x, y + r * 1.4);
    }
}

/// A circle of four Béziers (κ = 0.5523), stroked.
fn circle(s: &mut String, x: f64, y: f64, r: f64) {
    let k = r * 0.552_284_75;
    *s += &format!("{} {} m\n", num(x + r), num(y));
    for (p1, p2, p3) in [
        ((x + r, y + k), (x + k, y + r), (x, y + r)),
        ((x - k, y + r), (x - r, y + k), (x - r, y)),
        ((x - r, y - k), (x - k, y - r), (x, y - r)),
        ((x + k, y - r), (x + r, y - k), (x + r, y)),
    ] {
        *s += &format!(
            "{} {} {} {} {} {} c\n",
            num(p1.0),
            num(p1.1),
            num(p2.0),
            num(p2.1),
            num(p3.0),
            num(p3.1)
        );
    }
    *s += "S\n";
}

/// The patches, in `DeviceCMYK`: the four process inks, their overprints,
/// then black at 75, 50 and 25 per cent.
pub(crate) const BAR_PATCHES: [[f64; 4]; 10] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
    [0.0, 1.0, 1.0, 0.0],
    [1.0, 0.0, 1.0, 0.0],
    [1.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 0.75],
    [0.0, 0.0, 0.0, 0.5],
    [0.0, 0.0, 0.0, 0.25],
];

/// Square patches along the top band, starting a mark-length in from the
/// left trim edge so they clear the crop marks.
fn color_bars(s: &mut String, layout: &MarksLayout, len: f64) {
    let t = layout.trim;
    let size = len * 0.6;
    let y = t.ury + layout.gap + (len - size) / 2.0;
    let room = (t.urx - t.llx) / 2.0 - len;
    let fit = (room / size).floor().max(0.0);
    let count = BAR_PATCHES.len().min(fit as usize);
    *s += "q /DeviceCMYK cs\n";
    for (i, c) in BAR_PATCHES.iter().take(count).enumerate() {
        let x = t.llx + len + size * i as f64;
        *s += &format!(
            "{} {} {} {} k {} {} {} {} re f\n",
            num(c[0]),
            num(c[1]),
            num(c[2]),
            num(c[3]),
            num(x),
            num(y),
            num(size),
            num(size)
        );
    }
    *s += "Q\n";
}

/// The information line in the bottom band, left-aligned past the crop
/// marks; Helvetica, sized to the band.
fn page_info(s: &mut String, layout: &MarksLayout, len: f64, font: &str, info: &str) {
    let t = layout.trim;
    let size = (len * 0.4).min(7.0);
    let bytes = winansi_bytes(info);
    let room = (t.urx - t.llx) / 2.0 - len;
    let size = if helvetica_width(&bytes) * size > room && room > 0.0 {
        room / helvetica_width(&bytes).max(f64::EPSILON)
    } else {
        size
    };
    let y = t.lly - layout.gap - len / 2.0 - size * 0.35;
    *s += &format!(
        "BT /{font} {} Tf {} {} Td (",
        num(size),
        num(t.llx + len),
        num(y)
    );
    for b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                s.push('\\');
                s.push(char::from(b));
            }
            0x20..=0x7E => s.push(char::from(b)),
            _ => *s += &format!("\\{b:03o}"),
        }
    }
    *s += ") Tj ET\n";
}

fn num(v: f64) -> String {
    crate::bates::number(v)
}

#[cfg(test)]
mod tests;
