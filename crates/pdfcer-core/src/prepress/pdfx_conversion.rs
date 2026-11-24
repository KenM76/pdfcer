//! What [`crate::edit::EditSession::convert_to_pdfx`] is asked for and what
//! it reports. The steps it runs and the claim it writes are described on
//! the verb.

use crate::preflight::{PdfxLevel, PdfxReport, PdfxVerdict};
use crate::transparency::{FlattenOptions, FlattenTransparencyReport};

use super::{ConvertColoursReport, InkEditReport, InkSelection, IntentChoice};

/// The printing condition a PDF/X file is prepared for: the `/GTS_PDFX`
/// output intent (ISO 32000-2 §14.11.5, Table 365).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PdfxOutputIntent {
    /// The destination ICC profile, embedded as `/DestOutputProfile` and
    /// the target every converted colour is moved to.
    pub profile: Vec<u8>,
    /// `/OutputConditionIdentifier`, such as a registered condition name
    /// (`FOGRA39`). `None` uses the profile's description, or `Custom`
    /// when it has none.
    pub condition: Option<String>,
    /// `/RegistryName`, conventionally `http://www.color.org`.
    pub registry_name: Option<String>,
    /// `/Info`, the human-readable condition. `None` keeps the profile's
    /// description.
    pub info: Option<String>,
}

impl PdfxOutputIntent {
    /// An intent for `profile`, named by the profile's description.
    #[must_use]
    pub const fn new(profile: Vec<u8>) -> Self {
        Self {
            profile,
            condition: None,
            registry_name: None,
            info: None,
        }
    }

    /// Set [`Self::condition`].
    #[must_use]
    pub fn with_condition(mut self, condition: impl Into<String>) -> Self {
        self.condition = Some(condition.into());
        self
    }

    /// Set [`Self::registry_name`].
    #[must_use]
    pub fn with_registry_name(mut self, registry: impl Into<String>) -> Self {
        self.registry_name = Some(registry.into());
        self
    }

    /// Set [`Self::info`].
    #[must_use]
    pub fn with_info(mut self, info: impl Into<String>) -> Self {
        self.info = Some(info.into());
        self
    }
}

/// How [`crate::edit::EditSession::convert_to_pdfx`] converts.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PdfxConversion {
    /// The printing condition.
    pub intent: PdfxOutputIntent,
    /// The `/Title` written when the document has none (pre-X-4 levels
    /// require one). `None` writes none.
    pub title: Option<String>,
    /// Seconds since 1970-01-01 UTC, written as `/ModDate` and, when
    /// missing, `/CreationDate` (ISO 32000-1 §7.9.4). `None` writes no
    /// date; pdfcer-core never reads the clock.
    pub at: Option<i64>,
    /// `/Trapped`. `None` keeps `/True` or `/False` and otherwise writes
    /// `/False`, disclosed.
    pub trapped: Option<bool>,
    /// Spot inks to convert to process colour first. `None` keeps every
    /// spot ink, which every PDF/X level allows.
    pub spot_to_process: Option<InkSelection>,
    /// How transparency is flattened for X-1a and X-3.
    pub flatten: FlattenOptions,
    /// The rendering intent of the colour conversion.
    pub rendering_intent: IntentChoice,
    /// Write a `/TrimBox` equal to the crop box on pages with neither a
    /// trim nor an art box. `false` leaves them, and the claim is refused.
    pub infer_trim_boxes: bool,
    /// Write the claim only on [`PdfxVerdict::Conforms`]. Off by default:
    /// every level has rows pdfcer cannot check yet, so `Conforms` is
    /// unreachable today and the default claims on
    /// [`PdfxVerdict::Unconfirmed`], disclosed.
    pub require_conforms: bool,
    /// When the claim is refused, keep the conversion steps (one undo
    /// entry) without the claim. Off by default: a refused conversion
    /// changes nothing.
    pub keep_partial: bool,
}

impl PdfxConversion {
    /// The defaults for `intent`: relative colorimetric, trim boxes
    /// inferred, spot inks kept, no title or date.
    #[must_use]
    pub fn new(intent: PdfxOutputIntent) -> Self {
        Self {
            intent,
            title: None,
            at: None,
            trapped: None,
            spot_to_process: None,
            flatten: FlattenOptions::default(),
            rendering_intent: IntentChoice::Fixed(
                crate::color::RenderingIntent::RelativeColorimetric,
            ),
            infer_trim_boxes: true,
            require_conforms: false,
            keep_partial: false,
        }
    }

    /// Set [`Self::title`].
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set [`Self::at`].
    #[must_use]
    pub const fn with_time(mut self, unix_seconds: i64) -> Self {
        self.at = Some(unix_seconds);
        self
    }

    /// Set [`Self::trapped`].
    #[must_use]
    pub const fn with_trapped(mut self, trapped: bool) -> Self {
        self.trapped = Some(trapped);
        self
    }

    /// Set [`Self::spot_to_process`].
    #[must_use]
    pub fn with_spot_to_process(mut self, inks: InkSelection) -> Self {
        self.spot_to_process = Some(inks);
        self
    }

    /// Set [`Self::flatten`].
    #[must_use]
    pub const fn with_flatten(mut self, options: FlattenOptions) -> Self {
        self.flatten = options;
        self
    }

    /// Set [`Self::rendering_intent`].
    #[must_use]
    pub const fn with_rendering_intent(mut self, intent: IntentChoice) -> Self {
        self.rendering_intent = intent;
        self
    }

    /// Set [`Self::infer_trim_boxes`].
    #[must_use]
    pub const fn with_infer_trim_boxes(mut self, infer: bool) -> Self {
        self.infer_trim_boxes = infer;
        self
    }

    /// Set [`Self::require_conforms`].
    #[must_use]
    pub const fn with_require_conforms(mut self, require: bool) -> Self {
        self.require_conforms = require;
        self
    }

    /// Set [`Self::keep_partial`].
    #[must_use]
    pub const fn with_keep_partial(mut self, keep: bool) -> Self {
        self.keep_partial = keep;
        self
    }
}

/// What [`crate::edit::EditSession::convert_to_pdfx`] did.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PdfxConversionReport {
    /// The level converted to.
    pub level: PdfxLevel,
    /// The verdict on the document as it was.
    pub before: PdfxVerdict,
    /// The verdict on the converted document with the claim written.
    pub after: PdfxVerdict,
    /// The claim is in the document (one undo entry).
    pub claimed: bool,
    /// The claim was refused and the steps were kept without it
    /// ([`PdfxConversion::keep_partial`]).
    pub kept_partial: bool,
    /// The transparency flatten, for X-1a and X-3.
    pub flatten: Option<FlattenTransparencyReport>,
    /// The spot-to-process conversion, when asked for.
    pub spot_to_process: Option<InkEditReport>,
    /// The colour conversion.
    pub colours: Option<ConvertColoursReport>,
    /// 0-based pages given an inferred `/TrimBox`.
    pub trim_boxes: Vec<usize>,
    /// One sentence per step taken, in order.
    pub steps: Vec<String>,
    /// Everything inferred, left or refused, one sentence each (rule 4).
    pub disclosures: Vec<String>,
    /// The validation of the converted document with the claim written:
    /// on a refusal, what remains.
    pub validation: PdfxReport,
}

impl PdfxConversionReport {
    /// An empty report for `level`, with `validation` standing for both
    /// verdicts until the conversion runs.
    pub(crate) fn new(level: PdfxLevel, validation: PdfxReport) -> Self {
        let before = validation.verdict();
        Self {
            level,
            before,
            after: before,
            claimed: false,
            kept_partial: false,
            flatten: None,
            spot_to_process: None,
            colours: None,
            trim_boxes: Vec::new(),
            steps: Vec::new(),
            disclosures: Vec::new(),
            validation,
        }
    }
}
