//! Overprint's per-component decision, ISO 32000-1 §8.6.7 and §11.7.4.3
//! (Table 149, `CompatibleOverprint`).

/// The value `B(c_b, c_s)` selects, before it is applied.
///
/// Returned rather than a bare `f32` so a caller can *report* what overprint
/// did without having to compare numbers and infer it — a paint that
/// happens to leave a component unchanged because `c_s == c_b` is not the
/// same event as one that deliberately preserved the backdrop, and rule 4's
/// disclosure needs to tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentRule {
    /// Paint the source tint: `B = c_s`. The ordinary, non-overprint result.
    Source,
    /// Preserve the backdrop tint: `B = c_b`. This is overprint *acting* —
    /// the component survives because the source did not claim it.
    Backdrop,
    /// Paint zero tint: `B = 0.0`. An explicit erase, not a no-op.
    ///
    /// This cell surprises people and is **not** a spec defect. `SP-N2` in
    /// the consolidator records it as a confirmed negative result: a
    /// `Separation` paint with overprint **off** *erases* the process
    /// colorants it does not name, and the standard states this twice
    /// (Table 148, and Table 149's `c_s (= 0.0)` notation). Recorded so it
    /// is not "fixed" by a later reader who assumes it must be a typo.
    Zero,
}

impl ComponentRule {
    /// Resolve the rule to an actual tint value given the two operands.
    ///
    /// Kept separate from `pdfcer-render`'s `compatible_overprint` so the *decision* can be
    /// tested against Table 149 without threading sample values through
    /// every case — the table specifies which operand wins, and that is the
    /// part worth pinning.
    #[must_use]
    pub fn apply(self, backdrop: f32, source: f32) -> f32 {
        match self {
            Self::Source => source,
            Self::Backdrop => backdrop,
            Self::Zero => 0.0,
        }
    }

    /// Whether this rule leaves the backdrop showing through.
    ///
    /// The predicate a caller uses to answer "did overprint change what the
    /// operator sees here?" without re-deriving the table.
    #[must_use]
    pub const fn preserves_backdrop(self) -> bool {
        matches!(self, Self::Backdrop)
    }
}

impl ComponentRule {
    /// The rule for a component the paint **sets** (Table 149): its own
    /// tint, except that under overprint with nonzero overprint mode a
    /// zero tint keeps the backdrop (§8.6.7). Pass `nonzero_mode` only
    /// where OPM 1 is in scope: a `DeviceCMYK` colour set directly, on a
    /// process component, in a `DeviceCMYK` group.
    #[must_use]
    pub fn set(overprint: bool, nonzero_mode: bool, tint: f32) -> Self {
        if overprint && nonzero_mode && tint == 0.0 {
            Self::Backdrop
        } else {
            Self::Source
        }
    }

    /// The rule for a component the paint does **not** set: kept under
    /// overprint, erased to zero tint without it (§8.6.7, Table 149).
    #[must_use]
    pub const fn unset(overprint: bool) -> Self {
        if overprint {
            Self::Backdrop
        } else {
            Self::Zero
        }
    }
}
