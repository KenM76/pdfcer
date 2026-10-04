//! A [`ReplacementFaces`] that keeps a description of each face, not its
//! bytes: a long-lived shell's resident size stays independent of how many
//! fonts it offers. The shell supplies the loader, so this crate reads no
//! files (decision 178 §2).

use std::fmt;

use pdfcer_core::font_embed::FontEmbedPlan;
use pdfcer_core::text_edit::{FaceCandidate, ReplacementFaces};
use skrifa::{FontRef, MetadataProvider};

use crate::font::installed_faces::{collection_indices, describe};
use crate::font::subset::{plan_subset, subset_tag_for};

/// Reads the bytes behind a label passed to [`FaceCatalog::add`].
type Loader = dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync;

/// One catalogued face: its description (with nothing missing), where it
/// lives and the codepoints its cmap maps, as sorted inclusive ranges.
struct Entry {
    description: FaceCandidate,
    label: String,
    index: u32,
    coverage: Vec<(u32, u32)>,
}

/// Installed faces described once and re-read only for the face the ladder
/// picks.
///
/// [`add`](Self::add) parses a file's faces and drops its bytes;
/// [`ReplacementFaces::plan`] calls the loader for the picked face's label
/// and refuses if the file no longer holds the face that was catalogued.
/// Results match [`InstalledFaces`](crate::font::InstalledFaces) over the
/// same files.
///
/// # Examples
///
/// ```
/// use pdfcer_core::text_edit::EditOptions;
/// use pdfcer_render::font::FaceCatalog;
///
/// // A shell's loader reads the file the label names; this one has none.
/// let catalog = FaceCatalog::new(|label: &str| Err(format!("{label}: not found")));
/// assert!(catalog.is_empty());
/// let opts = EditOptions::default().with_replacement_faces(Box::leak(Box::new(catalog)));
/// assert!(opts.replacement_faces.is_some());
/// ```
pub struct FaceCatalog {
    entries: Vec<Entry>,
    loader: Box<Loader>,
}

impl FaceCatalog {
    /// An empty catalogue whose [`ReplacementFaces::plan`] reads a face's
    /// bytes back through `loader`, given the label it was added under.
    #[must_use]
    pub fn new(loader: impl Fn(&str) -> Result<Vec<u8>, String> + Send + Sync + 'static) -> Self {
        Self {
            entries: Vec::new(),
            loader: Box::new(loader),
        }
    }

    /// Catalogue every face in `bytes` under `label` (the loader's key and
    /// the disclosure's source); returns how many faces were added. Bytes
    /// that do not parse as a font add none.
    pub fn add(&mut self, label: &str, bytes: &[u8]) -> usize {
        let before = self.entries.len();
        for index in collection_indices(bytes) {
            let Ok(font) = FontRef::from_index(bytes, index) else {
                continue;
            };
            let id = self.entries.len();
            self.entries.push(Entry {
                description: describe(id, label, &font),
                label: label.to_owned(),
                index,
                coverage: coverage_of(&font),
            });
        }
        self.entries.len() - before
    }

    /// How many faces are catalogued.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no face is catalogued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl fmt::Debug for FaceCatalog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FaceCatalog")
            .field("faces", &self.entries.len())
            .finish_non_exhaustive()
    }
}

/// The codepoints `font`'s cmap maps, merged into ranges. U+0000..U+00FF
/// are probed one by one: a symbol cmap answers them from U+F000..U+F0FF,
/// which its mapping list does not show.
fn coverage_of(font: &FontRef<'_>) -> Vec<(u32, u32)> {
    let charmap = font.charmap();
    let mut points: Vec<u32> = charmap.mappings().map(|(c, _)| c).collect();
    points.extend((0..=0xFF_u32).filter(|&c| charmap.map(c).is_some()));
    points.sort_unstable();
    points.dedup();
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for c in points {
        match ranges.last_mut() {
            Some((_, end)) if *end + 1 == c => *end = c,
            _ => ranges.push((c, c)),
        }
    }
    ranges
}

fn covers(ranges: &[(u32, u32)], c: char) -> bool {
    let c = u32::from(c);
    let i = ranges.partition_point(|&(_, end)| end < c);
    ranges.get(i).is_some_and(|&(start, _)| start <= c)
}

impl ReplacementFaces for FaceCatalog {
    fn candidates(&self, chars: &[char]) -> Vec<FaceCandidate> {
        self.entries
            .iter()
            .map(|e| {
                let missing = chars
                    .iter()
                    .copied()
                    .filter(|&c| !covers(&e.coverage, c))
                    .collect();
                e.description.clone().with_missing(missing)
            })
            .collect()
    }

    fn plan(&self, candidate: &FaceCandidate, chars: &[char]) -> Result<FontEmbedPlan, String> {
        let e = self
            .entries
            .get(candidate.id)
            .ok_or_else(|| "the face is no longer offered".to_owned())?;
        let bytes = (self.loader)(&e.label)?;
        let font = FontRef::from_index(&bytes, e.index)
            .map_err(|_| format!("{}: the file changed since it was catalogued", e.label))?;
        if describe(candidate.id, &e.label, &font).postscript_name != e.description.postscript_name
        {
            return Err(format!(
                "{}: the file changed since it was catalogued",
                e.label
            ));
        }
        let name = &candidate.postscript_name;
        plan_subset(&bytes, e.index, chars, name, &subset_tag_for(name)).map_err(|e| e.to_string())
    }
}
