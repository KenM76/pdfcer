//! `pdfcer-core`'s [`ReplacementFaces`] over the faces a shell supplied
//! (decision 178): each face described from its own `name`, `OS/2`, `post`
//! and `cmap` tables, the picked one subset for embedding.

use pdfcer_core::font_embed::FontEmbedPlan;
use pdfcer_core::fontinfo::FsTypeBits;
use pdfcer_core::text_edit::{FaceCandidate, FaceClass, ReplacementFaces};
use skrifa::raw::TableProvider;
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider};

use crate::font::subset::{plan_subset, subset_tag_for};
use crate::font::{FontData, FontEnvironment};

/// Installed faces offered to the replacement-face ladder.
///
/// Every member of a collection is a face of its own. A file that does not
/// parse as a font is ignored.
///
/// # Examples
///
/// ```
/// use pdfcer_core::text_edit::EditOptions;
/// use pdfcer_render::font::InstalledFaces;
///
/// let faces = InstalledFaces::new();
/// let opts = EditOptions::default().with_replacement_faces(Box::leak(Box::new(faces)));
/// assert!(opts.replacement_faces.is_some());
/// ```
#[derive(Debug, Clone, Default)]
pub struct InstalledFaces {
    /// `(label, bytes, collection index)` per face.
    faces: Vec<(String, FontData, u32)>,
}

impl InstalledFaces {
    /// No faces: the ladder falls to its standard-14 floor.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every distinct font `env` holds under a name, each labelled with
    /// the first name it is registered under.
    #[must_use]
    pub fn from_environment(env: &FontEnvironment) -> Self {
        let mut out = Self::new();
        let mut seen: Vec<*const u8> = Vec::new();
        for name in env.named_faces() {
            let Some(data) = env.named(name) else {
                continue;
            };
            let ptr = data.bytes().as_ptr();
            if !seen.contains(&ptr) {
                seen.push(ptr);
                out.insert(name, data.clone());
            }
        }
        out
    }

    /// Offer every face in `data` under `label` (its file path, for the
    /// disclosure).
    pub fn insert(&mut self, label: &str, data: FontData) {
        let count = if data.bytes().starts_with(b"ttcf") {
            crate::font::sfnt::read_u32(data.bytes(), 8).unwrap_or(0)
        } else {
            1
        };
        for i in 0..count.min(256) {
            if FontRef::from_index(data.bytes(), i).is_ok() {
                self.faces.push((label.to_owned(), data.clone(), i));
            }
        }
    }

    /// How many faces are offered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.faces.len()
    }

    /// Whether no face is offered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }
}

/// The first `id` string the face's `name` table holds, control characters
/// dropped.
fn name_string(font: &FontRef<'_>, id: StringId) -> Option<String> {
    let s = font.localized_strings(id).english_or_first()?;
    let s: String = s.chars().filter(|c| !c.is_control()).collect();
    (!s.trim().is_empty()).then(|| s.trim().to_owned())
}

/// Serif style from `OS/2`: PANOSE Latin-text serif style (2–10 serif,
/// 11–15 sans), else the IBM family class (1–5 and 7 serif, 8 sans).
fn serif_of(panose: &[u8], family_class: i16) -> Option<bool> {
    if panose.first() == Some(&2) {
        match panose.get(1) {
            Some(2..=10) => return Some(true),
            Some(11..=15) => return Some(false),
            _ => {}
        }
    }
    match family_class >> 8 {
        1..=5 | 7 => Some(true),
        8 => Some(false),
        _ => None,
    }
}

/// `font`'s class from `OS/2` and `post`; the default where a table is
/// absent.
fn class_of(font: &FontRef<'_>) -> FaceClass {
    let fixed = font.post().is_ok_and(|p| p.is_fixed_pitch() != 0);
    let mut class = FaceClass::new().with_fixed_pitch(fixed);
    if let Ok(os2) = font.os2() {
        let italic = os2.fs_selection().bits() & 0x1 != 0;
        let width = u8::try_from(os2.us_width_class()).unwrap_or(5);
        class = class
            .with_serif(serif_of(os2.panose_10(), os2.s_family_class()))
            .with_italic(italic)
            .with_weight(os2.us_weight_class())
            .with_width(width);
    }
    class
}

/// Name ID 6, else the family's ASCII alphanumerics, else `Face<id>`: a
/// face must have some name to follow its subset tag in `/BaseFont`. The
/// flag is `true` when the name was derived.
fn postscript_name(id: usize, font: &FontRef<'_>, family: &str) -> (String, bool) {
    if let Some(name) = name_string(font, StringId::POSTSCRIPT_NAME) {
        return (name, false);
    }
    let derived: String = family.chars().filter(char::is_ascii_alphanumeric).collect();
    if derived.is_empty() {
        (format!("Face{id}"), true)
    } else {
        (derived, true)
    }
}

fn candidate(id: usize, label: &str, font: &FontRef<'_>, chars: &[char]) -> FaceCandidate {
    let family = name_string(font, StringId::TYPOGRAPHIC_FAMILY_NAME)
        .or_else(|| name_string(font, StringId::FAMILY_NAME))
        .unwrap_or_default();
    let (postscript, derived) = postscript_name(id, font, &family);
    let charmap = font.charmap();
    let missing = chars
        .iter()
        .copied()
        .filter(|&c| charmap.map(c).is_none())
        .collect();
    let mut c = FaceCandidate::new(id, label, &postscript, &family)
        .with_class(class_of(font))
        .with_missing(missing);
    if derived {
        c = c.with_derived_name();
    }
    match font.os2() {
        Ok(os2) => c.with_fs_type(FsTypeBits::decode(os2.fs_type(), os2.version())),
        Err(_) => c,
    }
}

impl ReplacementFaces for InstalledFaces {
    fn candidates(&self, chars: &[char]) -> Vec<FaceCandidate> {
        self.faces
            .iter()
            .enumerate()
            .filter_map(|(id, (label, data, index))| {
                let font = FontRef::from_index(data.bytes(), *index).ok()?;
                Some(candidate(id, label, &font, chars))
            })
            .collect()
    }

    fn plan(&self, candidate: &FaceCandidate, chars: &[char]) -> Result<FontEmbedPlan, String> {
        let (_, data, index) = self
            .faces
            .get(candidate.id)
            .ok_or_else(|| "the face is no longer offered".to_owned())?;
        let name = &candidate.postscript_name;
        plan_subset(data.bytes(), *index, chars, name, &subset_tag_for(name))
            .map_err(|e| e.to_string())
    }
}
