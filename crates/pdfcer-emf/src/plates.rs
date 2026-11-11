//! A page's separation plates as vector metafiles: one EMF per ink, in the
//! ink order of [`pdfcer_render::plates::render_plates`].
//!
//! The page is recorded once, composited in ink as a separation preview
//! renders it (ISO 32000-2 §10.8.3). Each plate replays that recording with
//! every fill and stroke turned to grey at the tint it lays on this ink, so
//! full tint is black. A paint that lays none of this ink knocks it out to
//! paper white, unless it overprints (§11.7.4.3, Table 149 through
//! `cmyk_group_rules_with_planes`, `/OPM` included), in which case the
//! plate keeps what is beneath and the paint is left off. A knockout white
//! with nothing painted beneath it on the plate is not written.
//!
//! Images, shadings, soft masks and transparency cannot be expressed as one
//! grey: they become `EMR_ALPHABLEND` patches cut from the plate's own
//! raster (the same render [`pdfcer_render::plates::render_plates`] makes), counted
//! per plate. A patch whose pixels are bare paper is written as a clip
//! exclusion only and not counted.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::Page;
use pdfcer_core::view::DocumentView;
use tiny_skia::{BlendMode, Pixmap};

use super::patch::{Plan, PxRect, contains, merge, px_rect};
use super::{EmfExport, EmfOptions, EmfOutcome, Writer, export_emf_view, metafile_units};
use pdfcer_render::RenderError;
use pdfcer_render::font::RenderOptions;
use pdfcer_render::writer_seam::op_box;
use pdfcer_render::writer_seam::{Brush, BrushSpec};
use pdfcer_render::writer_seam::{DeviceBounds, ExportRecording, Op, record_page_for_plates};

/// Why part of a plate is pixels rather than vector paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PlateRaster {
    /// An image.
    Image,
    /// A shading or gradient.
    Shading,
    /// A soft mask (§11.6.5).
    SoftMask,
    /// Constant alpha below 1, a blend mode other than `Normal`, or a
    /// transparency group with opacity or a blend of its own.
    Transparency,
}

impl PlateRaster {
    /// A short lower-case English name, as `pdfcer` reports it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Shading => "shading",
            Self::SoftMask => "soft mask",
            Self::Transparency => "transparency",
        }
    }
}

impl std::fmt::Display for PlateRaster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The pixel patches on one plate, per cause. A cause counts each object
/// whose patch holds ink on this plate; [`Self::rasters`] counts the
/// `EMR_ALPHABLEND` records, which overlapping objects share.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct PlatePatches {
    /// Images.
    pub images: usize,
    /// Shadings and gradients.
    pub shadings: usize,
    /// Soft-masked groups.
    pub soft_masks: usize,
    /// Semi-transparent or blended paint and groups.
    pub transparency: usize,
    /// `EMR_ALPHABLEND` records written.
    pub rasters: usize,
}

impl PlatePatches {
    /// Objects patched, every cause summed.
    #[must_use]
    pub fn cases(&self) -> usize {
        self.images + self.shadings + self.soft_masks + self.transparency
    }

    fn add(&mut self, cause: PlateRaster) {
        match cause {
            PlateRaster::Image => self.images += 1,
            PlateRaster::Shading => self.shadings += 1,
            PlateRaster::SoftMask => self.soft_masks += 1,
            PlateRaster::Transparency => self.transparency += 1,
        }
    }
}

/// What one plate's metafile holds.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct EmfPlateOutcome {
    /// Page width in points.
    pub width_pt: f32,
    /// Page height in points.
    pub height_pt: f32,
    /// Fills and strokes written as vector paths, knockouts included.
    pub ops: usize,
    /// Of [`Self::ops`], paper-white knockouts.
    pub knockouts: usize,
    /// Knockouts not written because nothing was painted beneath them.
    pub knockouts_elided: usize,
    /// Overprinting paints left off this plate, keeping what is beneath.
    pub overprints_kept: usize,
    /// The pixel patches.
    pub patches: PlatePatches,
}

/// One ink's plate as an EMF.
#[derive(Debug, Clone)]
pub struct EmfPlate {
    /// The ink, as [`pdfcer_render::plates::Plate::ink`] spells it.
    pub ink: String,
    /// The metafile: grey on transparent paper, full tint black. Unlabelled.
    pub emf: Vec<u8>,
    /// What it holds.
    pub outcome: EmfPlateOutcome,
}

/// What [`export_emf_plates`] produced for a page.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum EmfPlates {
    /// One metafile per ink: the four process inks, then each spot in
    /// plane order.
    Separated(Vec<EmfPlate>),
    /// The page could not be composited in ink, so this is its ordinary
    /// [`super::export_emf_view`] metafile, as
    /// [`pdfcer_render::plates::Plates::Composite`].
    Composite(Box<EmfExport>),
}

/// Why [`export_emf_plates_strict`] returned no plates.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PlateMetafileError {
    /// A plate would need a pixel patch.
    #[error("the {ink} plate needs pixels for {reason}")]
    Raster {
        /// The plate's ink.
        ink: String,
        /// The first object on it that needs pixels.
        reason: PlateRaster,
    },
    /// The page could not be exported at all.
    #[error("vector export failed: {0}")]
    Export(#[from] RenderError),
}

/// Export `page` as one EMF per separation plate. [`EmfOptions::raster_dpi`]
/// sets the recording scale and the patches' resolution; the other options
/// apply only to an [`EmfPlates::Composite`] result.
///
/// # Errors
///
/// [`RenderError`], as for a render.
///
/// # Examples
///
/// ```no_run
/// # fn f(doc: &pdfcer_core::document::Document, page: &pdfcer_core::page_tree::Page)
/// # -> Result<(), pdfcer_render::RenderError> {
/// use pdfcer_render::RenderOptions;
/// use pdfcer_emf::{EmfOptions, EmfPlates, export_emf_plates};
///
/// let plates = export_emf_plates(doc, page, &RenderOptions::default(), &EmfOptions::default())?;
/// if let EmfPlates::Separated(plates) = plates {
///     for plate in &plates {
///         println!("{}: {} patches", plate.ink, plate.outcome.patches.rasters);
///     }
/// }
/// # Ok(()) }
/// ```
pub fn export_emf_plates(
    doc: &Document,
    page: &Page,
    render: &RenderOptions,
    emf: &EmfOptions,
) -> Result<EmfPlates, RenderError> {
    export_emf_plates_view(&doc.view(), page, render, emf)
}

/// [`export_emf_plates`] over a [`DocumentView`].
///
/// # Errors
///
/// As [`export_emf_plates`].
pub fn export_emf_plates_view(
    view: &DocumentView<'_>,
    page: &Page,
    render: &RenderOptions,
    emf: &EmfOptions,
) -> Result<EmfPlates, RenderError> {
    plates_impl(view, page, render, emf, None)
}

/// [`export_emf_plates_view`], refusing the page when any plate would hold
/// a pixel patch: every plate returned is vector paths only.
///
/// # Errors
///
/// [`PlateMetafileError::Raster`] naming the first such plate and why;
/// [`PlateMetafileError::Export`] when the export fails.
pub fn export_emf_plates_strict(
    view: &DocumentView<'_>,
    page: &Page,
    render: &RenderOptions,
    emf: &EmfOptions,
) -> Result<EmfPlates, PlateMetafileError> {
    let mut refused = None;
    let plates = plates_impl(view, page, render, emf, Some(&mut refused))?;
    match refused {
        Some((ink, reason)) => Err(PlateMetafileError::Raster { ink, reason }),
        None => Ok(plates),
    }
}

/// The first plate a strict export refuses, and why.
type Refusal = Option<(String, PlateRaster)>;

/// What every plate of one page shares.
struct Shared<'a> {
    view: &'a DocumentView<'a>,
    page: &'a Page,
    render: &'a RenderOptions,
    scale: f32,
    rec: ExportRecording,
    inks: Vec<String>,
}

fn plates_impl(
    view: &DocumentView<'_>,
    page: &Page,
    render: &RenderOptions,
    emf: &EmfOptions,
    mut strict: Option<&mut Refusal>,
) -> Result<EmfPlates, RenderError> {
    let dpi = emf.dpi();
    let scale = dpi / 72.0;
    let (inks, _) = pdfcer_render::writer_seam::probe_inks(view, page, scale, render)?;
    if inks.is_empty() {
        return Ok(EmfPlates::Composite(Box::new(export_emf_view(
            view, page, render, emf,
        )?)));
    }
    let rec = record_page_for_plates(view, page, scale, render)?;
    let (unit, size_u) = metafile_units(rec.page_size, dpi)?;
    let shared = Shared {
        view,
        page,
        render,
        scale,
        rec,
        inks,
    };
    let mut plates = Vec::with_capacity(shared.inks.len());
    for ink in &shared.inks {
        let (plate, refusal) = plate(&shared, ink, unit, size_u)?;
        if let (Some(out), Some(why)) = (strict.as_deref_mut(), refusal) {
            *out = Some((plate.ink, why));
            break;
        }
        plates.push(plate);
    }
    Ok(EmfPlates::Separated(plates))
}

/// One plate's metafile.
fn plate(
    shared: &Shared<'_>,
    ink: &str,
    unit: f32,
    size_u: (i32, i32),
) -> Result<(EmfPlate, Option<PlateRaster>), RenderError> {
    let (w_px, h_px) = shared.rec.page_size;
    let mut build = Build::new(PlateInk::of(ink), &shared.inks);
    build.walk(&shared.rec.ops);
    let mut ops = build.ops;
    let mut outcome = build.outcome;
    let plan = patch_plan(shared, ink, &build.sites, &mut ops, &mut outcome.patches)?;
    let refusal = plan.as_ref().and_then(|p| p.refusal);
    let empty = EmfOutcome::empty(
        shared.rec.tally,
        pdfcer_render::interpret::Diagnostics::default(),
    );
    let mut w = Writer::new(&shared.rec.clips, unit, (w_px, h_px), empty);
    w.write_metafile(&ops, plan.as_ref().map(|p| &p.plan), None, size_u);
    outcome.ops = w.outcome.ops;
    outcome.patches.rasters = w.outcome.rasters_embedded;
    #[allow(clippy::cast_precision_loss)]
    {
        outcome.width_pt = w_px as f32 / shared.scale;
        outcome.height_pt = h_px as f32 / shared.scale;
    }
    let plate = EmfPlate {
        ink: ink.to_owned(),
        emf: w.out,
        outcome,
    };
    Ok((plate, refusal))
}

/// A plate's patches and the first cause that put ink in one.
struct PlatePlan {
    plan: Plan,
    refusal: Option<PlateRaster>,
}

/// Merge the sites into rectangles, render the plate's raster when there
/// are any, mark the bare-paper ones, count the rest per cause and drop the
/// vector ops lying wholly inside one.
fn patch_plan(
    shared: &Shared<'_>,
    ink: &str,
    sites: &[(PlateRaster, DeviceBounds)],
    ops: &mut Vec<Op>,
    patches: &mut PlatePatches,
) -> Result<Option<PlatePlan>, RenderError> {
    let (w, h) = shared.rec.page_size;
    let rects = merge(sites.iter().filter_map(|s| px_rect(s.1, w, h)).collect());
    if rects.is_empty() {
        return Ok(None);
    }
    let truth = pdfcer_render::writer_seam::plate_pixmap(
        shared.view,
        shared.page,
        shared.scale,
        shared.render,
        &shared.inks,
        ink,
    )?;
    if (truth.width(), truth.height()) != (w, h) {
        return Err(RenderError::BadRasterSize {
            width: truth.width(),
            height: truth.height(),
        });
    }
    let blank: Vec<bool> = rects.iter().map(|r| bare_paper(&truth, *r)).collect();
    let mut refusal = None;
    for (cause, bounds) in sites {
        let Some(r) = px_rect(*bounds, w, h) else {
            continue;
        };
        let inked = rects
            .iter()
            .zip(&blank)
            .any(|(c, b)| !*b && contains(*c, r));
        if inked {
            patches.add(*cause);
            refusal.get_or_insert(*cause);
        }
    }
    ops.retain(|op| {
        let Some(r) = op_box(op).and_then(|b| px_rect(b, w, h)) else {
            return true;
        };
        !rects.iter().any(|c| contains(*c, r))
    });
    Ok(Some(PlatePlan {
        plan: Plan::new(rects, truth, blank),
        refusal,
    }))
}

/// Every pixel of `r` in `plate` is opaque white.
fn bare_paper(plate: &Pixmap, (x0, y0, x1, y1): PxRect) -> bool {
    let stride = plate.width() as usize * 4;
    (y0..y1).all(|y| {
        let row = y as usize * stride;
        plate
            .data()
            .get(row + x0 as usize * 4..row + x1 as usize * 4)
            .is_none_or(|px| px.iter().all(|&v| v == 255))
    })
}

/// The ink a plate is for.
#[derive(Debug, Clone, Copy)]
enum PlateInk<'a> {
    /// An index into [`pdfcer_render::PROCESS_INKS`].
    Process(usize),
    /// A spot colorant, as `CmykBuffer::ink_names` spells it.
    Spot(&'a str),
}

impl<'a> PlateInk<'a> {
    fn of(ink: &'a str) -> Self {
        pdfcer_render::PROCESS_INKS
            .iter()
            .position(|p| *p == ink)
            .map_or(Self::Spot(ink), Self::Process)
    }
}

/// A spot colorant's name as the ink list spells it.
fn spot_name(colorant: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(colorant)
}

/// What `brush` lays on `plate`: `Some(tint)`, `0` being a knockout to
/// paper, or `None` when it overprints and leaves the plate alone.
fn tint_on(brush: &BrushSpec, plate: PlateInk<'_>, inks: &[String]) -> Option<f32> {
    if let Some(t) = brush.all_colorants {
        return Some(t);
    }
    match plate {
        PlateInk::Process(i) => match brush.overprint_process {
            Some(op) => op.get(i).copied().flatten(),
            None => process_tints(brush, inks).get(i).copied(),
        },
        PlateInk::Spot(name) => {
            if let Some(s) = brush.spots.iter().find(|s| spot_name(&s.colorant) == name) {
                Some(s.tint)
            } else if brush.overprint_process.is_some() {
                None
            } else {
                Some(0.0)
            }
        }
    }
}

/// The process tints `brush` lays: only the process channels it named when
/// every spot it names has a plate, else its colour flattened to process
/// ink, as the colorant buffer chooses (`BrushSpec::process_tints`).
fn process_tints(brush: &BrushSpec, inks: &[String]) -> [f32; 4] {
    let all_plated = !brush.spots.is_empty()
        && brush
            .spots
            .iter()
            .all(|s| inks.iter().any(|i| *i == spot_name(&s.colorant)));
    if all_plated {
        return brush.process_tints.unwrap_or([0.0; 4]);
    }
    if let Some(c) = brush.cmyk {
        return c;
    }
    let Brush::Solid { rgba } = brush.brush else {
        return [0.0; 4];
    };
    let f = |v: u8| f32::from(v) / 255.0;
    pdfcer_render::overprint::rgb_to_cmyk(f(rgba[0]), f(rgba[1]), f(rgba[2]))
}

/// Why `brush` cannot be one grey, if it cannot.
fn raster_cause(brush: &BrushSpec) -> Option<PlateRaster> {
    match brush.brush {
        Brush::Image { .. } => Some(PlateRaster::Image),
        Brush::Gradient(_) => Some(PlateRaster::Shading),
        Brush::Solid { rgba } => (rgba[3] < 255 || brush.blend != BlendMode::SourceOver)
            .then_some(PlateRaster::Transparency),
    }
}

/// Where vector ink is already on the plate, for knockout elision. Exact
/// boxes up to a cap, then their union.
#[derive(Default)]
struct Painted {
    boxes: Vec<DeviceBounds>,
    union: Option<DeviceBounds>,
}

const PAINTED_BOXES: usize = 512;

impl Painted {
    fn add(&mut self, b: DeviceBounds) {
        if self.boxes.len() < PAINTED_BOXES {
            self.boxes.push(b);
        }
        self.union = Some(self.union.map_or(b, |u| DeviceBounds {
            left: u.left.min(b.left),
            top: u.top.min(b.top),
            right: u.right.max(b.right),
            bottom: u.bottom.max(b.bottom),
        }));
    }

    fn hits(&self, b: DeviceBounds) -> bool {
        let meets = |o: &DeviceBounds| {
            !(o.right <= b.left || b.right <= o.left || o.bottom <= b.top || b.bottom <= o.top)
        };
        match self.union {
            None => false,
            Some(u) if self.boxes.len() < PAINTED_BOXES => {
                meets(&u) && self.boxes.iter().any(meets)
            }
            Some(u) => meets(&u),
        }
    }
}

/// One plate's op list being built from the recording.
struct Build<'a> {
    plate: PlateInk<'a>,
    inks: &'a [String],
    ops: Vec<Op>,
    sites: Vec<(PlateRaster, DeviceBounds)>,
    painted: Painted,
    outcome: EmfPlateOutcome,
}

impl<'a> Build<'a> {
    fn new(plate: PlateInk<'a>, inks: &'a [String]) -> Self {
        Self {
            plate,
            inks,
            ops: Vec::new(),
            sites: Vec::new(),
            painted: Painted::default(),
            outcome: EmfPlateOutcome::default(),
        }
    }

    fn walk(&mut self, ops: &[Op]) {
        for op in ops {
            match op {
                Op::Fill { brush, bounds, .. } | Op::Stroke { brush, bounds, .. } => {
                    self.paint(
                        op,
                        brush,
                        bounds.unwrap_or(pdfcer_render::writer_seam::UNKNOWN),
                    );
                }
                Op::Layer { paint, ops, mask } => {
                    let plain = mask.is_none()
                        && paint.opacity >= 1.0
                        && paint.blend == BlendMode::SourceOver
                        && paint.nonseparable.is_none();
                    if plain {
                        self.walk(ops);
                    } else if let Some(b) = op_box(op) {
                        let cause = if mask.is_some() {
                            PlateRaster::SoftMask
                        } else {
                            PlateRaster::Transparency
                        };
                        self.sites.push((cause, b));
                    }
                }
                Op::Text { ops, .. } => self.walk(ops),
            }
        }
    }

    fn paint(&mut self, op: &Op, brush: &BrushSpec, bounds: DeviceBounds) {
        if let Some(cause) = raster_cause(brush) {
            self.sites.push((cause, bounds));
            return;
        }
        let Some(tint) = tint_on(brush, self.plate, self.inks) else {
            self.outcome.overprints_kept += 1;
            return;
        };
        let tint = tint.clamp(0.0, 1.0);
        if tint <= 0.0 {
            if !self.painted.hits(bounds) {
                self.outcome.knockouts_elided += 1;
                return;
            }
            self.outcome.knockouts += 1;
        } else {
            self.painted.add(bounds);
        }
        self.ops.push(grey(op, tint));
    }
}

/// `op` painted opaque grey at `tint`: full tint black, none white.
fn grey(op: &Op, tint: f32) -> Op {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let g = (255.0 * (1.0 - tint)).round() as u8;
    let mut out = op.clone();
    if let Op::Fill { brush, .. } | Op::Stroke { brush, .. } = &mut out {
        *brush = BrushSpec {
            brush: Brush::Solid {
                rgba: [g, g, g, 255],
            },
            blend: BlendMode::SourceOver,
            anti_alias: brush.anti_alias,
            spots: Vec::new(),
            process_tints: None,
            all_colorants: None,
            overprint_process: None,
            cmyk: None,
        };
    }
    out
}
