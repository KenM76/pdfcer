//! The byte-budget ladder behind
//! [`EditSession::optimize_to_budget`](crate::edit::EditSession::optimize_to_budget).
//!
//! Step 0 is the operator's settings as given. Steps 1 to 11 add one rule
//! for colour and grey images (inserted first, so the operator's own rules
//! and per-object overrides still win): JPEG at quality 80 down to 30 at
//! native resolution, then quality 30 resampled to 200 down to 72 ppi. Each
//! step is at least as lossy as the one before in both quality and
//! resolution, so the written size is close to monotone and the search can
//! bisect. Bilevel and indexed images are never touched by the ladder.

use super::image::{ImageEncode, ImagePatch};
use super::{
    ImageClass, OptimizeError, OptimizePlan, OptimizeSettings, OptimizedFile, Rule, Selector,
    StreamAction,
};
use crate::writer::{RebuildReport, WriteError};

/// `(quality, ppi)` for ladder steps 1 to 11; `None` keeps the resolution.
pub(crate) const LADDER: [(u8, Option<u16>); 11] = [
    (80, None),
    (70, None),
    (60, None),
    (50, None),
    (40, None),
    (30, None),
    (30, Some(200)),
    (30, Some(150)),
    (30, Some(120)),
    (30, Some(96)),
    (30, Some(72)),
];

/// The most lossy ladder step.
pub const LAST_STEP: usize = LADDER.len();

/// One trial save of the budget search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct BudgetAttempt {
    /// Ladder step: 0 is the operator's settings, 1 to [`LAST_STEP`] the
    /// ladder.
    pub step: usize,
    /// The JPEG quality ceiling the step imposed; `None` at step 0.
    pub quality: Option<u8>,
    /// The resolution ceiling the step imposed; `None` keeps the operator's.
    pub ppi: Option<u16>,
    /// The bytes the step would write: the rewrite, or the original file
    /// when the rewrite was not smaller.
    pub bytes: u64,
}

/// The result of
/// [`EditSession::optimize_to_budget`](crate::edit::EditSession::optimize_to_budget).
///
/// The session holds `plan` applied, as one undoable command.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BudgetFit {
    /// The chosen step's plan.
    pub plan: OptimizePlan,
    /// The chosen step's file, as [`EditSession::save_optimized`] returns it.
    ///
    /// [`EditSession::save_optimized`]: crate::edit::EditSession::save_optimized
    pub file: OptimizedFile,
    /// The structure rebuild's report, when one ran.
    pub structure: Option<RebuildReport>,
    /// The chosen ladder step.
    pub step: usize,
    /// Every trial, in the order run.
    pub attempts: Vec<BudgetAttempt>,
    /// Whether `file` fits the budget. `false` means even the last step
    /// missed and `file` is the closest the ladder reaches.
    pub met: bool,
}

/// Why a byte-budget search failed. The session is left as it was.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BudgetError {
    /// Planning or applying a step failed.
    #[error(transparent)]
    Optimize(#[from] OptimizeError),
    /// Writing a step failed.
    #[error(transparent)]
    Write(#[from] WriteError),
}

/// The JPEG quality and resolution ceilings step `step` imposes on
/// `settings`: the ladder's, lowered to any quality or resolution the
/// operator set on `images` or the default stream action. `(None, None)` at
/// step 0; the resolution is `None` on a native-resolution step.
pub(crate) fn step_ceilings(settings: &OptimizeSettings, step: usize) -> (Option<u8>, Option<u16>) {
    let Some(&(quality, ppi)) = step.checked_sub(1).and_then(|i| LADDER.get(i)) else {
        return (None, None);
    };
    // `images` folds over the default stream action, so its value wins.
    let base = match settings.streams {
        StreamAction::Image(p) => p,
        _ => ImagePatch::new(),
    };
    let quality = settings
        .images
        .quality
        .or(base.quality)
        .map_or(quality, |q| q.min(quality));
    let base_ppi = settings.images.ppi.or(base.ppi);
    (Some(quality), ppi.map(|p| base_ppi.map_or(p, |b| b.min(p))))
}

/// `settings` with ladder step `step` (1 to [`LAST_STEP`]) applied, at the
/// ceilings [`step_ceilings`] gives; step 0 returns `settings` unchanged.
pub(crate) fn step_settings(settings: &OptimizeSettings, step: usize) -> OptimizeSettings {
    let mut out = settings.clone();
    let (Some(quality), ppi) = step_ceilings(settings, step) else {
        return out;
    };
    let mut patch = ImagePatch::new();
    patch.encode = Some(ImageEncode::Jpeg);
    patch.quality = Some(quality);
    patch.ppi = ppi;
    patch.above = ppi;
    let mut select = Selector::new();
    select.image_classes = vec![ImageClass::Color, ImageClass::Gray];
    out.rules.insert(
        0,
        Rule {
            select,
            action: StreamAction::Image(patch),
        },
    );
    out
}

/// The bytes `file` stands for on disk.
pub(crate) const fn written_len(file: &OptimizedFile) -> u64 {
    match file {
        OptimizedFile::Smaller(bytes) | OptimizedFile::NotSmaller { bytes, .. } => {
            bytes.len() as u64
        }
        OptimizedFile::KeptOriginal { original, .. } => *original,
    }
}
