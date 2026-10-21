//! The built-in settings bundles.

use std::fmt;

use super::{
    CleanupOptions, DiscardObject, DiscardOptions, ImageClass, ImageEncode, ImagePatch,
    MetadataKind, OptimizeSettings, RebuildOptions, Rule, Selector, StreamAction,
};

/// pdfcer's built-in settings bundles.
///
/// The values are pdfcer's own, published in `--show-preset` and
/// `docs/core-api`. Every preset recompresses eligible streams at level 9
/// and turns every [`RebuildOptions`] option on. Image settings:
///
/// | preset | colour, grey and palette images | 1-bit images |
/// |---|---|---|
/// | `lossless`, `archive` | kept | kept |
/// | `standard` | above 225 ppi to 150, quality 60 | above 450 ppi to 300 |
/// | `screen` | above 144 ppi to 96, written as JPEG at quality 80 (palette images stay Flate) | above 450 ppi to 300 |
/// | `print` | above 450 ppi to 300, quality 80 | above 900 ppi to 600 |
///
/// The encoding is otherwise kept: a resampled JPEG stays JPEG (never above
/// its source's quality), anything else is Flate. Every image change is
/// lossy and itemised in the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Preset {
    /// Lossless changes only.
    Lossless,
    /// The everyday default.
    Standard,
    /// Smallest file for screen and mobile reading.
    Screen,
    /// Keeps print-resolution content.
    Print,
    /// Keeps PDF/A conformance.
    Archive,
}

impl Preset {
    /// Every preset, in display order.
    pub const ALL: [Self; 5] = [
        Self::Lossless,
        Self::Standard,
        Self::Screen,
        Self::Print,
        Self::Archive,
    ];

    /// The stable name used by `--preset` and `extends`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lossless => "lossless",
            Self::Standard => "standard",
            Self::Screen => "screen",
            Self::Print => "print",
            Self::Archive => "archive",
        }
    }

    /// Look a preset up by [`Self::as_str`]; `mobile` is an alias of
    /// `screen`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        if name == "mobile" {
            return Some(Self::Screen);
        }
        Self::ALL.into_iter().find(|p| p.as_str() == name)
    }

    /// The preset's settings.
    #[must_use]
    pub fn settings(self) -> OptimizeSettings {
        let mut s = OptimizeSettings::new();
        s.streams = match self {
            Self::Lossless | Self::Standard | Self::Screen | Self::Print | Self::Archive => {
                StreamAction::Recompress { level: 9 }
            }
        };
        s.structure = RebuildOptions::all();
        (s.images, s.rules) = images(self);
        // Removing an unreferenced named destination can break a link from
        // another file, so only the smallest-file preset does it.
        s.cleanup = CleanupOptions {
            unreferenced_destinations: matches!(self, Self::Screen),
            ..CleanupOptions::all()
        };
        // Page thumbnails are redrawn by every viewer that shows them; the
        // smallest-file preset also drops producers' private data,
        // per-object XMP and print-only alternate images. Archive keeps
        // everything PDF/A may need.
        s.discard = match self {
            Self::Lossless | Self::Archive => DiscardOptions::none(),
            Self::Standard | Self::Print => DiscardOptions::none().with(MetadataKind::Thumbnail),
            Self::Screen => DiscardOptions::none()
                .with(MetadataKind::Thumbnail)
                .with(MetadataKind::PieceInfo)
                .with(MetadataKind::ObjectXmp)
                .with_object(DiscardObject::AlternateImages),
        };
        s
    }
}

impl fmt::Display for Preset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A resample of images drawn above `above` ppi to `ppi`.
const fn resample(ppi: u16, above: u16) -> ImagePatch {
    let mut p = ImagePatch::new();
    p.ppi = Some(ppi);
    p.above = Some(above);
    p
}

/// A rule giving images of `classes` the patch `patch`.
fn rule(classes: Vec<ImageClass>, patch: ImagePatch) -> Rule {
    let mut select = Selector::new();
    select.image_classes = classes;
    Rule {
        select,
        action: StreamAction::Image(patch),
    }
}

/// The preset's image settings and its per-class rules.
fn images(preset: Preset) -> (ImagePatch, Vec<Rule>) {
    let (mut all, mono) = match preset {
        Preset::Lossless | Preset::Archive => return (ImagePatch::new(), Vec::new()),
        Preset::Standard => (resample(150, 225), resample(300, 450)),
        Preset::Screen => (resample(96, 144), resample(300, 450)),
        Preset::Print => (resample(300, 450), resample(600, 900)),
    };
    all.quality = Some(if preset == Preset::Standard { 60 } else { 80 });
    let mut rules = Vec::new();
    if preset == Preset::Screen {
        all.encode = Some(ImageEncode::Jpeg);
        let mut keep = ImagePatch::new();
        keep.encode = Some(ImageEncode::Keep);
        rules.push(rule(vec![ImageClass::Indexed], keep));
    }
    let mut mono = mono;
    mono.encode = Some(ImageEncode::Keep);
    rules.push(rule(vec![ImageClass::Mono], mono));
    (all, rules)
}
