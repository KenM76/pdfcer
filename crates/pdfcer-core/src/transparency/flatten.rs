//! What [`crate::edit::EditSession::flatten_transparency`] takes, what it
//! hands the flattener, and what it reports.
//!
//! The compositing itself needs a renderer, which `pdfcer-core` cannot
//! depend on, so it is injected as a [`TransparencyFlattener`]: the
//! shipped one is `pdfcer_emf::RecordingFlattener`. The flattener returns
//! content-stream bytes and raster patches; the session writes them.

use crate::object::ObjId;
use crate::page_tree::Rect;
use crate::view::DocumentView;

use super::TransparencyUse;

/// The choices a flatten run makes.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FlattenOptions {
    /// Text outside the flattened area stays live text. `false` redraws
    /// the whole page, every text run as outlines.
    pub keep_text: bool,
    /// Strokes the flattener redraws stay stroked paths. `false` draws
    /// each as a filled outline of its stroke.
    pub keep_strokes: bool,
    /// Pixels per inch for what cannot be computed as vector regions.
    pub raster_ppi: f64,
}

impl Default for FlattenOptions {
    fn default() -> Self {
        Self {
            keep_text: true,
            keep_strokes: true,
            raster_ppi: Self::DEFAULT_PPI,
        }
    }
}

impl FlattenOptions {
    /// The default raster resolution.
    pub const DEFAULT_PPI: f64 = 300.0;
    /// The lowest raster resolution accepted.
    pub const MIN_PPI: f64 = 36.0;
    /// The highest raster resolution accepted.
    pub const MAX_PPI: f64 = 2400.0;

    /// The defaults: keep text, keep strokes, 300 ppi.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set [`Self::keep_text`].
    #[must_use]
    pub const fn with_keep_text(mut self, keep: bool) -> Self {
        self.keep_text = keep;
        self
    }

    /// Set [`Self::keep_strokes`].
    #[must_use]
    pub const fn with_keep_strokes(mut self, keep: bool) -> Self {
        self.keep_strokes = keep;
        self
    }

    /// Set [`Self::raster_ppi`].
    #[must_use]
    pub const fn with_raster_ppi(mut self, ppi: f64) -> Self {
        self.raster_ppi = ppi;
        self
    }
}

/// Computes one page's flattened appearance from the unflattened view.
///
/// The returned [`FlattenedPage::content`] is content-stream bytes in the
/// page's default user space that paint opaquely, naming each raster
/// patch by its [`RasterPatch::name`] as an XObject. With
/// [`FlattenMode::Overlay`] it is painted over the page's own content
/// (whose transparency the session switches off) and must cover every
/// place that content painted transparently; with [`FlattenMode::Rebuilt`]
/// it replaces the page's content.
pub trait TransparencyFlattener {
    /// Flatten page `page` (0-based) of `view`.
    ///
    /// # Errors
    ///
    /// [`FlattenFailure`] when the page cannot be recorded or rendered.
    fn flatten_page(
        &self,
        view: &DocumentView<'_>,
        page: usize,
        options: &FlattenOptions,
    ) -> Result<FlattenedPage, FlattenFailure>;
}

/// Why a [`TransparencyFlattener`] gave up on a page.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct FlattenFailure {
    message: String,
}

impl FlattenFailure {
    /// A failure saying `message`.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// How a page was flattened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FlattenMode {
    /// The page's content was kept with its transparency switched off and
    /// an opaque overlay painted over where it was transparent.
    Overlay,
    /// The page's content was replaced by the flattened drawing.
    Rebuilt,
}

/// Why an area was rasterised rather than drawn as vector regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RasterReason {
    /// An image lies in the flattened area.
    Image,
    /// A smooth shading or gradient lies in the flattened area.
    Shading,
    /// A transparency group that could not be resolved.
    Group,
    /// Compositing the flattener does not model (a soft mask over varying
    /// content, a blend over an image, an overprint it declines), or more
    /// regions than its ceiling.
    NotModelled,
}

impl RasterReason {
    /// The lower-case name reports use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Shading => "shading",
            Self::Group => "group",
            Self::NotModelled => "not-modelled",
        }
    }
}

/// One rasterised area.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct RasterisedRegion {
    /// The area, in default user space.
    pub rect: Rect,
    /// Why it was rasterised.
    pub reason: RasterReason,
}

impl RasterisedRegion {
    /// A region at `rect` rasterised for `reason`.
    #[must_use]
    pub const fn new(rect: Rect, reason: RasterReason) -> Self {
        Self { rect, reason }
    }
}

/// An RGB image a [`FlattenedPage`] paints.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RasterPatch {
    /// The XObject name the content uses, without the slash.
    pub name: String,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 3` bytes, rows top to bottom.
    pub rgb: Vec<u8>,
}

impl RasterPatch {
    /// A patch; `None` when `rgb` is not `width * height * 3` bytes or
    /// either side is zero.
    #[must_use]
    pub fn new(name: String, width: u32, height: u32, rgb: Vec<u8>) -> Option<Self> {
        let want = usize::try_from(u64::from(width) * u64::from(height) * 3).ok()?;
        (width > 0 && height > 0 && rgb.len() == want).then_some(Self {
            name,
            width,
            height,
            rgb,
        })
    }
}

/// One page's flattened drawing, from a [`TransparencyFlattener`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FlattenedPage {
    /// Content-stream bytes in default user space.
    pub content: Vec<u8>,
    /// The images [`Self::content`] names.
    pub images: Vec<RasterPatch>,
    /// Overlay or replacement.
    pub mode: FlattenMode,
    /// Opaque vector paints written.
    pub vector_regions: usize,
    /// What was rasterised, and why.
    pub rasterised: Vec<RasterisedRegion>,
    /// Text runs drawn as outlines.
    pub text_runs_outlined: usize,
    /// Strokes drawn as filled outlines.
    pub strokes_outlined: usize,
    /// Spot inks whose paint was redrawn in process colour, by name.
    pub spot_inks: Vec<String>,
    /// Anything else the flattener wants said.
    pub notes: Vec<String>,
}

impl FlattenedPage {
    /// An empty drawing in `mode`.
    #[must_use]
    pub const fn new(mode: FlattenMode) -> Self {
        Self {
            content: Vec::new(),
            images: Vec::new(),
            mode,
            vector_regions: 0,
            rasterised: Vec::new(),
            text_runs_outlined: 0,
            strokes_outlined: 0,
            spot_inks: Vec::new(),
            notes: Vec::new(),
        }
    }
}

/// One page of a [`FlattenTransparencyReport`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PageFlattenReport {
    /// 0-based page index.
    pub page: usize,
    /// What the scan found before flattening.
    pub found: TransparencyUse,
    /// `None` when the page's content used no transparency and was left
    /// untouched.
    pub mode: Option<FlattenMode>,
    /// Opaque vector paints written.
    pub vector_regions: usize,
    /// What was rasterised, and why.
    pub rasterised: Vec<RasterisedRegion>,
    /// Text runs drawn as outlines.
    pub text_runs_outlined: usize,
    /// Strokes drawn as filled outlines.
    pub strokes_outlined: usize,
    /// Spot inks redrawn in process colour.
    pub spot_inks: Vec<String>,
    /// Annotations that use transparency, left as they are.
    pub annotations_left: Vec<ObjId>,
    /// The flattener's notes.
    pub notes: Vec<String>,
}

impl PageFlattenReport {
    /// A report for a page left untouched.
    pub(crate) fn untouched(page: usize, found: TransparencyUse) -> Self {
        let annotations_left = found.annotations();
        Self {
            page,
            found,
            mode: None,
            vector_regions: 0,
            rasterised: Vec::new(),
            text_runs_outlined: 0,
            strokes_outlined: 0,
            spot_inks: Vec::new(),
            annotations_left,
            notes: Vec::new(),
        }
    }
}

/// What [`crate::edit::EditSession::flatten_transparency`] did.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct FlattenTransparencyReport {
    /// True when anything was written (one undo entry).
    pub changed: bool,
    /// One entry per page asked for, in the order asked.
    pub pages: Vec<PageFlattenReport>,
    /// Sentences for the operator (rule 4: inferred content is disclosed).
    pub disclosures: Vec<String>,
}
