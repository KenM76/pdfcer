//! Whether overprint is simulated on screen ([`OverprintPreview`]).

/// Whether marks painted with `/OP` or `/op true` are composited as
/// overprints (ISO 32000-1 §8.6.7, §11.7.4) or as ordinary knockouts.
///
/// A viewing choice, not a document property: Acrobat offers it as
/// *Overprint Preview*. Under [`Self::Off`] every mark paints with the
/// `Normal` blend over whatever is beneath it, as a viewer without
/// separations shows it. Nothing written to a file changes either way.
///
/// [`Diagnostics::overprint_requested`](crate::Diagnostics::overprint_requested)
/// counts the request under both values, so a shell can offer Acrobat's
/// *Automatic* arm (simulate only when the page asks for it);
/// `overprint_effective` is `0` under [`Self::Off`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum OverprintPreview {
    /// Composite overprinting marks per §11.7.4. The default.
    #[default]
    Simulate,
    /// Paint every overprinting mark as a knockout.
    Off,
}

impl OverprintPreview {
    /// Whether overprint is simulated.
    #[must_use]
    pub const fn simulates(self) -> bool {
        matches!(self, Self::Simulate)
    }
}
