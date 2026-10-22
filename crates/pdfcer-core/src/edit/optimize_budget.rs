//! [`EditSession::optimize_to_budget`]: the least lossy optimize that fits
//! a byte budget.

use super::EditSession;
use crate::optimize::budget::{
    self, BudgetAttempt, BudgetError, BudgetFit, LAST_STEP, step_ceilings, step_settings,
};
use crate::optimize::{OptimizePlan, OptimizeSettings, OptimizedFile};
use crate::writer::{RebuildReport, SaveOptions};

/// One step's trial: its plan, file and report.
type Trial = (OptimizePlan, OptimizedFile, Option<RebuildReport>);

impl EditSession {
    /// Optimize with `settings`, made lossier step by step until the saved
    /// file fits in `max_bytes`, and leave the session holding the least
    /// lossy step that fits (or the last step, when none does) as one
    /// undoable command.
    ///
    /// Step 0 is `settings` as given; if it fits, nothing lossier is tried.
    /// Otherwise steps 1 to [`budget::LAST_STEP`] write colour and grey
    /// images as JPEG at quality 80 down to 30, then resample them to 200
    /// down to 72 ppi (see [`crate::optimize::budget`]); the search bisects
    /// those steps. A ceiling already lower in `settings.images` is kept,
    /// and the operator's rules and per-object overrides win over the
    /// ladder. Bilevel and indexed images are never changed by the ladder.
    ///
    /// Each trial plans, applies and saves, then reverts its apply, so a
    /// search costs one full optimize per distinct step tried (at most six).
    ///
    /// # Errors
    ///
    /// [`BudgetError::Optimize`] when a step cannot be planned or applied,
    /// [`BudgetError::Write`] when it cannot be saved. The session is left
    /// as it was in either case.
    pub fn optimize_to_budget(
        &mut self,
        settings: &OptimizeSettings,
        max_bytes: u64,
        options: &SaveOptions,
    ) -> Result<BudgetFit, BudgetError> {
        let mut attempts: Vec<BudgetAttempt> = Vec::new();
        let mut done: Vec<Trial> = Vec::new();
        let mut run = |s: &mut Self, step: usize| -> Result<(Trial, bool), BudgetError> {
            let (quality, ppi) = step_ceilings(settings, step);
            // A step the operator's lower ceilings made equal to one already
            // tried writes the same file; reuse it.
            let same = attempts
                .iter()
                .position(|a| a.step != 0 && (a.quality, a.ppi) == (quality, ppi));
            let trial = match same.and_then(|i| done.get(i)) {
                Some(t) => t.clone(),
                None => s.budget_trial(settings, step, options)?,
            };
            let bytes = budget::written_len(&trial.1);
            done.push(trial.clone());
            attempts.push(BudgetAttempt {
                step,
                quality,
                ppi,
                bytes,
            });
            Ok((trial, bytes <= max_bytes))
        };
        let (mut best, mut step, mut met) = (run(self, 0)?, 0, true);
        if !best.1 {
            best = run(self, LAST_STEP)?;
            (step, met) = (LAST_STEP, best.1);
            // `hi` always fits; find the lowest step that does.
            let (mut lo, mut hi) = (1, LAST_STEP);
            while met && lo < hi {
                let mid = lo + (hi - lo) / 2;
                let trial = run(self, mid)?;
                if trial.1 {
                    (best, step, hi) = (trial, mid, mid);
                } else {
                    lo = mid + 1;
                }
            }
        }
        let ((plan, file, structure), _) = best;
        self.apply_optimize(&plan)?;
        Ok(BudgetFit {
            plan,
            file,
            structure,
            step,
            attempts,
            met,
        })
    }

    /// Plan, apply and save ladder step `step`, then revert the apply.
    fn budget_trial(
        &mut self,
        settings: &OptimizeSettings,
        step: usize,
        options: &SaveOptions,
    ) -> Result<Trial, BudgetError> {
        let settings = step_settings(settings, step);
        let plan = self.plan_optimize(&settings)?;
        let depth = self.undo_depth();
        self.apply_optimize(&plan)?;
        let saved = self.save_optimized(&plan, options, &settings.structure);
        // An apply with nothing to commit pushes no command.
        if self.undo_depth() > depth {
            self.revert_last();
        }
        let (file, structure) = saved?;
        Ok((plan, file, structure))
    }
}
