//! Separation preview: the page as a press prints it with chosen plates left
//! off (ISO 32000-2 §10.8.3, "Separation simulation"). A display aid; the
//! document is untouched, and a render without it is byte-identical.

/// The four process colorant names, in the colorant buffer's plane order —
/// the spellings ISO 32000-1 §8.6.6.4 and §8.6.6.5 give them.
pub const PROCESS_INKS: [&str; 4] = ["Cyan", "Magenta", "Yellow", "Black"];

/// A separation-preview request: composite the page in ink and leave the
/// [`Self::hidden`] plates off.
///
/// Set through [`crate::RenderOptions::with_separation_preview`].
///
/// - The page is composited in the colorant buffer with every overprint and
///   blend resolved, and the hidden plates are zeroed **just before** the
///   conversion to sRGB, so overprinted ink under a hidden plate shows
///   through as it would on press.
/// - A page that would otherwise composite on screen (an additive page
///   group, or none under `device_native`) is composited in ink as if its
///   blending space were `DeviceCMYK`; [`crate::Diagnostics::inks_forced_subtractive`]
///   says so.
/// - Names match exactly, as the file spells them after `#xx` decoding
///   (§7.3.5). The process plates are [`PROCESS_INKS`]; a `/Separation
///   /Cyan` paints the Cyan plate, so hiding `Cyan` hides it too.
/// - [`crate::Diagnostics::inks`] lists the page's plates and
///   [`crate::Diagnostics::inks_unmatched`] the names that matched none. A
///   spot past the plane cap is flattened into process ink
///   (`cmyk_spots_flattened`) and cannot be hidden on its own.
/// - [`crate::RenderOptions::ink_probe`] is read before any plate is hidden,
///   so it reports every plate's coverage.
///
/// # Example
///
/// ```
/// use pdfcer_render::{RenderOptions, SeparationPreview};
///
/// let options = RenderOptions::default()
///     .with_separation_preview(Some(SeparationPreview::new().with_hidden(["Black"])));
/// assert!(options.separation_preview.is_some_and(|p| p.hides("Black")));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct SeparationPreview {
    /// Colorant names whose plates are left off.
    pub hidden: Vec<String>,
}

impl SeparationPreview {
    /// Every plate shown.
    #[must_use]
    pub const fn new() -> Self {
        Self { hidden: Vec::new() }
    }

    /// Set the plates to leave off.
    #[must_use]
    pub fn with_hidden<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.hidden = names.into_iter().map(Into::into).collect();
        self
    }

    /// Whether the plate named `name` is left off.
    #[must_use]
    pub fn hides(&self, name: &str) -> bool {
        self.hidden.iter().any(|h| h == name)
    }
}
