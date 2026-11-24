//! [`InkCanvas`]: a page of colorant planes that paints and transparency
//! groups composite into, read back as ink on the sheet.

use crate::blend::{Blend, union_};
use crate::coverage::{CoverageMask, CoverageWarning};
use crate::overprint::ComponentRule;
use crate::pixel::{
    MAX_SPOTS, PixelCmyk, composite_element_cmyk, composite_element_knockout_cmyk, overprint_pixel,
    remove_backdrop_cmyk, remove_backdrop_spots,
};

/// The four process colorants, in plane order.
pub const PROCESS: [&str; 4] = ["Cyan", "Magenta", "Yellow", "Black"];

/// Planes per layer pixel: four process, [`MAX_SPOTS`] spots and alpha.
const PLANES: usize = 4 + MAX_SPOTS + 1;

/// Bytes one layer costs per pixel: its planes plus the group's alpha and
/// shape and, for a non-isolated or knockout group, its initial backdrop.
const LAYER_BYTES_PER_PIXEL: u64 = ((PLANES * 2 + 2) * 4) as u64;

/// Why an [`InkCanvas`] refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InkError {
    /// The page, or one more group layer, would pass the memory ceiling.
    #[error("{needed} bytes of colorant planes is over the {limit}-byte ceiling")]
    TooLarge {
        /// Bytes the allocation would bring the canvas to.
        needed: u64,
        /// The caller's ceiling.
        limit: u64,
    },
    /// More colorants than the caller's cap, or than four process plus
    /// [`MAX_SPOTS`] spots.
    #[error("{count} colorants is over the cap of {limit}")]
    TooManyColorants {
        /// Colorants asked for, process included.
        count: usize,
        /// The effective cap.
        limit: usize,
    },
    /// A spot colorant named twice, or named as a process colorant.
    #[error("colorant {0:?} is listed twice")]
    DuplicateColorant(String),
    /// A width or height of zero.
    #[error("a canvas needs a non-zero width and height")]
    Empty,
    /// A coverage mask whose length is not `width × height`.
    #[error("coverage mask holds {actual} bytes; the canvas has {expected} pixels")]
    MaskSize {
        /// `width × height`.
        expected: usize,
        /// The mask's length.
        actual: usize,
    },
    /// A tint list longer than the canvas's colorant list.
    #[error("{actual} tints for {expected} colorants")]
    TintCount {
        /// The canvas's colorant count.
        expected: usize,
        /// The paint's tint count.
        actual: usize,
    },
    /// [`InkCanvas::end_group`] with no group open.
    #[error("no transparency group is open")]
    NoOpenGroup,
}

/// The caller's ceilings, checked before anything is allocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Limits {
    /// Bytes the canvas may hold across the page and every open group.
    pub max_bytes: u64,
    /// Colorants allowed, the four process ones included.
    pub max_colorants: usize,
}

impl Limits {
    /// Ceilings of `max_bytes` and `max_colorants`.
    #[must_use]
    pub const fn new(max_bytes: u64, max_colorants: usize) -> Self {
        Self {
            max_bytes,
            max_colorants,
        }
    }
}

/// One horizontal run of coverage: pixels `x0..x1` on row `y`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    /// Row, from the top.
    pub y: u32,
    /// First pixel.
    pub x0: u32,
    /// One past the last pixel.
    pub x1: u32,
    /// Coverage in `[0, 1]`.
    pub coverage: f32,
}

/// Where a paint marks the page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Coverage<'a> {
    /// One byte per pixel, row-major, `width × height` long; 255 is full.
    Mask(&'a [u8]),
    /// Runs of coverage; pixels in no span are not marked.
    Spans(&'a [Span]),
}

/// Overprint control for one paint (ISO 32000-1 §8.6.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Overprint {
    /// `OP false`: colorants the paint does not set are erased.
    #[default]
    Off,
    /// `OP true`, `OPM 0`: colorants the paint does not set keep the
    /// backdrop.
    On,
    /// `OP true`, `OPM 1`: as [`Self::On`], and a zero process tint also
    /// keeps the backdrop. Use only where §8.6.7 puts OPM 1 in scope: a
    /// `DeviceCMYK` colour set directly, not an image or a shading.
    OnNonzero,
}

/// One paint: where it marks, what it puts in each colorant, and how it
/// composites.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paint<'a> {
    /// Where the paint marks. Coverage multiplies alpha, never a tint.
    pub coverage: Coverage<'a>,
    /// A tint per colorant in the canvas's order (process, then spots);
    /// `None`, or an index past the end, is a colorant the paint does not
    /// set.
    pub tints: &'a [Option<f32>],
    /// Constant alpha in `[0, 1]`.
    pub alpha: f32,
    /// Blend mode. Spot planes take only separable, white-preserving modes;
    /// others are `Normal` there (§11.7.4.2).
    pub blend: Blend,
    /// Overprint and overprint mode.
    pub overprint: Overprint,
}

/// A transparency group (ISO 32000-1 §11.4.5, §11.4.6).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Group {
    /// Composited over a transparent backdrop rather than over what is
    /// beneath it.
    pub isolated: bool,
    /// Each element composites against the group's initial backdrop
    /// rather than against earlier elements.
    pub knockout: bool,
    /// Constant alpha applied to the group's result.
    pub alpha: f32,
    /// Blend mode the result composites into its parent with.
    pub blend: Blend,
}

struct Layer {
    /// Plane-major pixels: [`PLANES`] planes of `len` values.
    planes: Vec<f32>,
    /// `α_g`, `f_g` and the initial backdrop (`None` for an isolated,
    /// non-knockout group and for the page).
    group_alpha: Vec<f32>,
    group_shape: Vec<f32>,
    initial: Option<Vec<f32>>,
    group: Option<Group>,
}

/// A page of colorant planes: four process inks then up to [`MAX_SPOTS`]
/// spot inks, `0.0` meaning no ink, starting transparent (ISO 32000-2
/// §11.4.7). Paints and groups composite into it in the subtractive space
/// (§11.3.4); [`Self::landed`] and [`Self::coverage_masks`] read ink as it
/// lands on a medium of no ink.
pub struct InkCanvas {
    width: u32,
    height: u32,
    len: usize,
    spots: Vec<String>,
    limits: Limits,
    bytes: u64,
    layers: Vec<Layer>,
}

impl InkCanvas {
    /// A transparent `width × height` canvas over the process colorants
    /// and `spots`, in that order.
    ///
    /// # Errors
    ///
    /// [`InkError::TooManyColorants`], [`InkError::DuplicateColorant`],
    /// [`InkError::Empty`] or [`InkError::TooLarge`], each before any plane
    /// is allocated.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_ink::{InkCanvas, Limits};
    ///
    /// let canvas = InkCanvas::new(100, 100, &["PANTONE 185 C"], Limits::new(1 << 20, 8))?;
    /// assert_eq!(canvas.colorants().count(), 5);
    /// # Ok::<(), pdfcer_ink::InkError>(())
    /// ```
    pub fn new(width: u32, height: u32, spots: &[&str], limits: Limits) -> Result<Self, InkError> {
        let count = spots.len().saturating_add(4);
        let limit = limits.max_colorants.min(4 + MAX_SPOTS);
        if count > limit {
            return Err(InkError::TooManyColorants { count, limit });
        }
        for (i, s) in spots.iter().enumerate() {
            let earlier = spots.iter().take(i).any(|e| e == s);
            if earlier || PROCESS.contains(s) {
                return Err(InkError::DuplicateColorant((*s).to_owned()));
            }
        }
        if width == 0 || height == 0 {
            return Err(InkError::Empty);
        }
        let pixels = u64::from(width).saturating_mul(u64::from(height));
        // Saturating to u64::MAX is still refused: no ceiling admits it.
        let needed = pixels.saturating_mul(LAYER_BYTES_PER_PIXEL);
        let len = usize::try_from(pixels).map_err(|_| too_large(needed, limits))?;
        let planes_fit = len.checked_mul(PLANES).is_some();
        if needed > limits.max_bytes || needed == u64::MAX || !planes_fit {
            return Err(too_large(needed, limits));
        }
        let mut canvas = Self {
            width,
            height,
            len,
            spots: spots.iter().map(|s| (*s).to_owned()).collect(),
            limits,
            bytes: needed,
            layers: Vec::new(),
        };
        canvas.layers.push(canvas.layer(None, None));
        Ok(canvas)
    }

    /// Width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The colorant names in plane order: [`PROCESS`], then the spots.
    pub fn colorants(&self) -> impl Iterator<Item = &str> {
        PROCESS
            .into_iter()
            .chain(self.spots.iter().map(String::as_str))
    }

    fn layer(&self, initial: Option<Vec<f32>>, group: Option<Group>) -> Layer {
        Layer {
            planes: initial
                .clone()
                .unwrap_or_else(|| vec![0.0; self.len * PLANES]),
            group_alpha: vec![0.0; self.len],
            group_shape: vec![0.0; self.len],
            initial,
            group,
        }
    }

    /// Composite one paint into the innermost open group, or the page.
    ///
    /// # Errors
    ///
    /// [`InkError::MaskSize`] or [`InkError::TintCount`]; nothing is
    /// painted.
    pub fn paint(&mut self, paint: &Paint<'_>) -> Result<(), InkError> {
        let colorants = 4 + self.spots.len();
        if paint.tints.len() > colorants {
            return Err(InkError::TintCount {
                expected: colorants,
                actual: paint.tints.len(),
            });
        }
        if let Coverage::Mask(m) = paint.coverage
            && m.len() != self.len
        {
            return Err(InkError::MaskSize {
                expected: self.len,
                actual: m.len(),
            });
        }
        let source = Source::of(paint);
        let alpha = paint.alpha.clamp(0.0, 1.0);
        match paint.coverage {
            Coverage::Mask(m) => {
                for (idx, &c) in m.iter().enumerate() {
                    self.paint_at(idx, f32::from(c) / 255.0, alpha, &source, paint.blend);
                }
            }
            Coverage::Spans(spans) => {
                for span in spans {
                    for idx in self.span_pixels(span) {
                        self.paint_at(
                            idx,
                            span.coverage.clamp(0.0, 1.0),
                            alpha,
                            &source,
                            paint.blend,
                        );
                    }
                }
            }
        }
        Ok(())
    }

    fn span_pixels(&self, span: &Span) -> std::ops::Range<usize> {
        if span.y >= self.height {
            return 0..0;
        }
        let row = span.y as usize * self.width as usize;
        let x1 = span.x1.min(self.width) as usize;
        let x0 = (span.x0 as usize).min(x1);
        row + x0..row + x1
    }

    fn paint_at(&mut self, idx: usize, shape: f32, alpha: f32, source: &Source, blend: Blend) {
        if shape <= 0.0 {
            return;
        }
        let len = self.len;
        let Some(layer) = self.layers.last_mut() else {
            return;
        };
        let t = shape * alpha;
        let before = pixel(&layer.planes, len, idx);
        let knockout = layer.group.is_some_and(|g| g.knockout);
        let ag = at(&layer.group_alpha, idx);
        let (after, ag) = if source.overprinting {
            let rules = source.process_rules(before);
            let px = overprint_pixel(before, rules, source.process, source.spots, t);
            (px, union_(ag, t))
        } else if knockout {
            let initial = layer
                .initial
                .as_ref()
                .map_or(PixelCmyk::TRANSPARENT, |p| pixel(p, len, idx));
            let src = PixelCmyk {
                a: t,
                ..source.solid()
            };
            composite_element_knockout_cmyk(initial, before, src, shape, ag, blend)
        } else {
            let src = PixelCmyk {
                a: t,
                ..source.solid()
            };
            (composite_element_cmyk(before, src, blend), union_(ag, t))
        };
        put(&mut layer.group_alpha, idx, ag);
        let f = at(&layer.group_shape, idx);
        put(&mut layer.group_shape, idx, union_(f, shape));
        set_pixel(&mut layer.planes, len, idx, after);
    }

    /// Open a transparency group; paints go into it until [`Self::end_group`].
    ///
    /// # Errors
    ///
    /// [`InkError::TooLarge`] when its layer would pass the memory ceiling;
    /// nothing is allocated.
    pub fn begin_group(&mut self, group: Group) -> Result<(), InkError> {
        let layer_bytes = (self.len as u64).saturating_mul(LAYER_BYTES_PER_PIXEL);
        let needed = self.bytes.saturating_add(layer_bytes);
        if needed > self.limits.max_bytes {
            return Err(too_large(needed, self.limits));
        }
        let backdrop = if group.isolated {
            None
        } else {
            self.layers.last().map(|l| l.planes.clone())
        };
        let layer = self.layer(backdrop, Some(group));
        self.bytes = needed;
        self.layers.push(layer);
        Ok(())
    }

    /// Close the innermost group and composite its result into its parent:
    /// §11.4.4's backdrop removal against the initial backdrop, then the
    /// group as one element at `α_g × alpha` through its blend mode.
    ///
    /// # Errors
    ///
    /// [`InkError::NoOpenGroup`] when only the page is open.
    pub fn end_group(&mut self) -> Result<(), InkError> {
        if self.layers.len() < 2 {
            return Err(InkError::NoOpenGroup);
        }
        let (Some(layer), Some(parent)) = (self.layers.pop(), self.layers.last_mut()) else {
            return Err(InkError::NoOpenGroup);
        };
        let Some(group) = layer.group else {
            return Err(InkError::NoOpenGroup);
        };
        let len = self.len;
        let opacity = group.alpha.clamp(0.0, 1.0);
        for idx in 0..len {
            let ag = at(&layer.group_alpha, idx);
            if ag <= 0.0 {
                continue;
            }
            let accum = pixel(&layer.planes, len, idx);
            let initial = layer
                .initial
                .as_ref()
                .map_or(PixelCmyk::TRANSPARENT, |p| pixel(p, len, idx));
            let source = PixelCmyk {
                c: remove_backdrop_cmyk(accum, initial, ag),
                s: remove_backdrop_spots(accum, initial, ag),
                a: ag * opacity,
            };
            let before = pixel(&parent.planes, len, idx);
            let after = composite_element_cmyk(before, source, group.blend);
            set_pixel(&mut parent.planes, len, idx, after);
            let pa = at(&parent.group_alpha, idx);
            put(&mut parent.group_alpha, idx, union_(pa, source.a));
        }
        self.bytes = self
            .bytes
            .saturating_sub((len as u64).saturating_mul(LAYER_BYTES_PER_PIXEL));
        Ok(())
    }

    /// The page's ink in `colorant`'s plane as it lands on the sheet: tint
    /// times alpha, row-major, `None` past the colorant list. Reads the page
    /// group; paints inside a still-open group are not included.
    #[must_use]
    pub fn landed(&self, colorant: usize) -> Option<Vec<f32>> {
        if colorant >= 4 + self.spots.len() {
            return None;
        }
        let page = self.layers.first()?;
        Some(
            (0..self.len)
                .map(|idx| {
                    let (process, spots) = pixel(&page.planes, self.len, idx).landed();
                    process
                        .into_iter()
                        .chain(spots)
                        .nth(colorant)
                        .unwrap_or(0.0)
                })
                .collect(),
        )
    }

    /// One mask per rule over the page's landed ink, each with its flagged
    /// count. Every spot plane counts toward total area coverage; none
    /// makes a black rich.
    #[must_use]
    pub fn coverage_masks(&self, rules: &[CoverageWarning]) -> Vec<CoverageMask> {
        let spots = self.spots.len();
        let mut masks: Vec<Vec<u8>> = rules.iter().map(|_| Vec::with_capacity(self.len)).collect();
        if let Some(page) = self.layers.first() {
            for idx in 0..self.len {
                let (process, inks) = pixel(&page.planes, self.len, idx).landed();
                let inks = inks.get(..spots).unwrap_or_default();
                for (mask, rule) in masks.iter_mut().zip(rules) {
                    mask.push(u8::from(rule.flags(process, inks)));
                }
            }
        }
        masks
            .into_iter()
            .map(|m| CoverageMask::from_mask(self.width, self.height, m))
            .collect()
    }
}

/// A paint's tints resolved against the canvas's colorant list.
struct Source {
    process: [f32; 4],
    sets_process: [bool; 4],
    spots: [Option<f32>; MAX_SPOTS],
    overprint: Overprint,
    overprinting: bool,
}

impl Source {
    fn of(paint: &Paint<'_>) -> Self {
        let tint = |i: usize| {
            paint
                .tints
                .get(i)
                .copied()
                .flatten()
                .map(|t| t.clamp(0.0, 1.0))
        };
        let mut process = [0.0; 4];
        let mut sets_process = [false; 4];
        for (i, (p, s)) in process.iter_mut().zip(&mut sets_process).enumerate() {
            if let Some(t) = tint(i) {
                *p = t;
                *s = true;
            }
        }
        let mut spots = [None; MAX_SPOTS];
        for (i, s) in spots.iter_mut().enumerate() {
            *s = tint(4 + i);
        }
        let on = paint.overprint != Overprint::Off;
        let mut source = Self {
            process,
            sets_process,
            spots,
            overprint: paint.overprint,
            overprinting: false,
        };
        let keeps_spot = on && source.spots.iter().any(Option::is_none);
        let rules = source.process_rules(PixelCmyk::TRANSPARENT);
        source.overprinting = keeps_spot || rules.iter().any(|r| r.preserves_backdrop());
        if !on {
            // `OP false`: an unset spot plane is painted at zero tint.
            for s in &mut source.spots {
                s.get_or_insert(0.0);
            }
        }
        source
    }

    /// Table 149 for the four process components.
    fn process_rules(&self, _backdrop: PixelCmyk) -> [ComponentRule; 4] {
        let on = self.overprint != Overprint::Off;
        let nonzero = self.overprint == Overprint::OnNonzero;
        let mut rules = [ComponentRule::Source; 4];
        for ((r, &sets), &t) in rules.iter_mut().zip(&self.sets_process).zip(&self.process) {
            *r = if sets {
                ComponentRule::set(on, nonzero, t)
            } else {
                ComponentRule::unset(on)
            };
        }
        rules
    }

    /// The paint as an ordinary source pixel: every rule resolved against
    /// no backdrop, which is exact when none keeps the backdrop.
    fn solid(&self) -> PixelCmyk {
        let rules = self.process_rules(PixelCmyk::TRANSPARENT);
        let mut c = [0.0; 4];
        for ((o, r), &t) in c.iter_mut().zip(&rules).zip(&self.process) {
            *o = r.apply(0.0, t);
        }
        PixelCmyk {
            c,
            s: self.spots.map(|s| s.unwrap_or(0.0)),
            a: 0.0,
        }
    }
}

fn too_large(needed: u64, limits: Limits) -> InkError {
    InkError::TooLarge {
        needed,
        limit: limits.max_bytes,
    }
}

fn at(plane: &[f32], idx: usize) -> f32 {
    plane.get(idx).copied().unwrap_or(0.0)
}

fn put(plane: &mut [f32], idx: usize, v: f32) {
    if let Some(slot) = plane.get_mut(idx) {
        *slot = v;
    }
}

fn pixel(planes: &[f32], len: usize, idx: usize) -> PixelCmyk {
    let p = |plane: usize| at(planes, plane * len + idx);
    let mut px = PixelCmyk::TRANSPARENT;
    for (i, c) in px.c.iter_mut().enumerate() {
        *c = p(i);
    }
    for (i, s) in px.s.iter_mut().enumerate() {
        *s = p(4 + i);
    }
    px.a = p(PLANES - 1);
    px
}

fn set_pixel(planes: &mut [f32], len: usize, idx: usize, px: PixelCmyk) {
    let values = px.c.into_iter().chain(px.s).chain([px.a]);
    for (plane, v) in values.enumerate() {
        put(planes, plane * len + idx, v.clamp(0.0, 1.0));
    }
}
