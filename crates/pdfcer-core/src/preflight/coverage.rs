//! Total area coverage ([`Check::TotalAreaCoverage`]): the sum of every
//! plate's ink per point of the sheet, held to a limit.
//!
//! Measuring it needs the page composited in ink, which `pdfcer-core`
//! cannot do, so the measuring is injected as a [`CoverageMeter`]; the
//! shipped one is `pdfcer_render::TacMeter`. A run given no meter reports
//! every coverage rule as [`FindingKind::Incomplete`], never as a pass.
//!
//! Ink is read as it lands on the sheet after overprint and transparency
//! are resolved (ISO 32000-2 §10.8.3 separations, §11.7.4 overprint).

use crate::page_tree::{Page, Rect};
use crate::view::DocumentView;

use super::checks::{Sink, finding, incomplete};
use super::walk::{self, Event};
use super::{
    Check, ColourFamily, Finding, FindingKind, FindingTarget, Measure, MeasureUnit, Profile,
    ProfileRule, Severity,
};

/// Measures one page's total area coverage at a stated resolution.
pub trait CoverageMeter {
    /// Composite `page` of `view` in ink at `request.ppi` and measure the
    /// summed coverage of every pixel against `request.limit_percent`.
    ///
    /// # Errors
    ///
    /// [`TacFailure`] when the page cannot be composited in ink at all.
    fn measure_tac(
        &self,
        view: &DocumentView<'_>,
        page: &Page,
        request: &TacRequest,
    ) -> Result<TacMeasurement, TacFailure>;
}

/// What a [`CoverageMeter`] is asked to measure.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct TacRequest {
    /// The limit, in percent of one plate (300 is three solid plates). A
    /// pixel is over when its sum is strictly above it.
    pub limit_percent: f64,
    /// The resolution the page is composited at, in pixels per inch.
    pub ppi: f64,
    /// The most regions reported; the rest are bounded together in one.
    pub max_regions: usize,
}

impl TacRequest {
    /// The default resolution.
    pub const DEFAULT_PPI: f64 = 150.0;
    /// The lowest resolution accepted.
    pub const MIN_PPI: f64 = 9.0;
    /// The highest resolution accepted.
    pub const MAX_PPI: f64 = 600.0;
    /// The default region cap.
    pub const DEFAULT_MAX_REGIONS: usize = 8;

    /// A request for `limit_percent` at `ppi`, with the default region cap.
    #[must_use]
    pub const fn new(limit_percent: f64, ppi: f64) -> Self {
        Self {
            limit_percent,
            ppi,
            max_regions: Self::DEFAULT_MAX_REGIONS,
        }
    }

    /// Set [`Self::max_regions`] (at least 1).
    #[must_use]
    pub const fn with_max_regions(mut self, max: usize) -> Self {
        self.max_regions = if max == 0 { 1 } else { max };
        self
    }
}

/// One connected area of over-limit pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct TacRegion {
    /// Its bounds in page space (default user space, y-up).
    pub rect: Rect,
    /// The highest coverage inside it, in percent.
    pub max_percent: f64,
    /// Its over-limit pixels.
    pub over_pixels: u64,
    /// How many separate areas this one bounds: 1, or more for the region
    /// that collects those past [`TacRequest::max_regions`].
    pub merged: usize,
}

impl TacRegion {
    /// A region of one area.
    #[must_use]
    pub const fn new(rect: Rect, max_percent: f64, over_pixels: u64) -> Self {
        Self {
            rect,
            max_percent,
            over_pixels,
            merged: 1,
        }
    }

    /// Set [`Self::merged`].
    #[must_use]
    pub const fn with_merged(mut self, merged: usize) -> Self {
        self.merged = merged;
        self
    }
}

/// One page's coverage, as a [`CoverageMeter`] measured it.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct TacMeasurement {
    /// The highest coverage of any pixel, in percent.
    pub max_percent: f64,
    /// Pixels measured.
    pub pixels: u64,
    /// Pixels over the limit.
    pub over_pixels: u64,
    /// The resolution measured at, in pixels per inch.
    pub ppi: f64,
    /// The over-limit areas, largest first.
    pub regions: Vec<TacRegion>,
    /// Content measured approximately, one sentence each.
    pub caveats: Vec<String>,
    /// Content whose ink could not be counted, one sentence each.
    pub not_measured: Vec<String>,
}

impl TacMeasurement {
    /// The over-limit area in square inches.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // a pixel count as an area
    pub fn over_area_sq_in(&self) -> f64 {
        if self.ppi > 0.0 {
            self.over_pixels as f64 / (self.ppi * self.ppi)
        } else {
            0.0
        }
    }
}

/// Why a [`CoverageMeter`] could not measure a page.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct TacFailure {
    message: String,
}

impl TacFailure {
    /// A failure saying `message`.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Families whose ink on a press depends on a conversion the renderer
/// only estimates.
const ESTIMATED: [ColourFamily; 5] = [
    ColourFamily::DeviceRgb,
    ColourFamily::CalRgb,
    ColourFamily::Lab,
    ColourFamily::IccRgb,
    ColourFamily::IccOther,
];

/// Every [`Check::TotalAreaCoverage`] rule of `profile` on page `number`.
pub(super) fn page(
    view: &DocumentView<'_>,
    profile: &Profile,
    page: &Page,
    number: usize,
    meter: Option<&dyn CoverageMeter>,
    sink: &mut Sink,
) {
    for rule in &profile.rules {
        let Check::TotalAreaCoverage { limit_percent, ppi } = rule.check else {
            continue;
        };
        let Some(meter) = meter else {
            sink.push(incomplete(
                rule,
                Some(number),
                "total area coverage needs the page rasterised, and this run had no rasteriser",
            ));
            continue;
        };
        let request = TacRequest::new(limit_percent, ppi);
        match meter.measure_tac(view, page, &request) {
            Ok(m) => report(rule, view, page, number, &request, &m, sink),
            Err(e) => sink.push(incomplete(
                rule,
                Some(number),
                &format!(
                    "the page could not be composited in ink to measure total area coverage ({e})"
                ),
            )),
        }
    }
}

fn report(
    rule: &ProfileRule,
    view: &DocumentView<'_>,
    page: &Page,
    number: usize,
    request: &TacRequest,
    m: &TacMeasurement,
    sink: &mut Sink,
) {
    for region in &m.regions {
        sink.push(region_finding(rule, number, request, m.ppi, region));
    }
    if m.over_pixels > 0 && m.regions.is_empty() {
        let message = format!(
            "Total area coverage reaches {:.1}%, over the {}% limit, on {} pixels at {} ppi.",
            m.max_percent, request.limit_percent, m.over_pixels, m.ppi
        );
        sink.push(Finding {
            measured: Some(measure(m.max_percent, request)),
            ..finding(rule, Some(number), FindingTarget::Page, message)
        });
    }
    if !m.not_measured.is_empty() {
        sink.push(incomplete(rule, Some(number), &m.not_measured.join("; ")));
    }
    let mut caveats = m.caveats.clone();
    if paints_estimated_colour(view, page) {
        caveats.push(
            "the page paints in RGB, Lab or another non-ink colour space, whose ink is the \
             renderer's estimate, not an ICC conversion; convert colours to the output \
             profile first (pdfcer convert-colours) for the figure a press will see"
                .to_owned(),
        );
    }
    if !caveats.is_empty() {
        sink.push(Finding {
            severity: Severity::Info,
            kind: FindingKind::NeedsPerson,
            ..finding(
                rule,
                Some(number),
                FindingTarget::Page,
                format!(
                    "Total area coverage was measured approximately: {}.",
                    caveats.join("; ")
                ),
            )
        });
    }
}

fn region_finding(
    rule: &ProfileRule,
    number: usize,
    request: &TacRequest,
    ppi: f64,
    region: &TacRegion,
) -> Finding {
    let which = if region.merged > 1 {
        format!("{} smaller areas, bounded together,", region.merged)
    } else {
        "this area".to_owned()
    };
    #[allow(clippy::cast_precision_loss)] // a pixel count as an area
    let sq_in = if ppi > 0.0 {
        region.over_pixels as f64 / (ppi * ppi)
    } else {
        0.0
    };
    let message = format!(
        "Total area coverage in {which} reaches {:.1}%, over the {}% limit, across {} pixels \
         ({sq_in:.3} sq in) at {ppi} ppi.",
        region.max_percent, request.limit_percent, region.over_pixels
    );
    Finding {
        rect: Some(region.rect),
        measured: Some(measure(region.max_percent, request)),
        ..finding(rule, Some(number), FindingTarget::Page, message)
    }
}

const fn measure(value: f64, request: &TacRequest) -> Measure {
    Measure {
        value,
        limit: request.limit_percent,
        unit: MeasureUnit::Percent,
    }
}

fn paints_estimated_colour(view: &DocumentView<'_>, page: &Page) -> bool {
    walk::page_events(view, page)
        .iter()
        .any(|(_, e)| matches!(e, Event::Colour(u) if ESTIMATED.contains(&u.family)))
}

#[cfg(test)]
#[path = "coverage_tests.rs"]
mod tests;
