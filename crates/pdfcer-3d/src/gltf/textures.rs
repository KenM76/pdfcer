//! glTF textures as [`SceneTexture`]s, and the coordinates
//! that draw them [glTF 2.0 §3.8, §3.9.2; `KHR_texture_transform`].

use std::borrow::Cow;
use std::collections::HashMap;

use pdfcer_json::Json;

use super::accessor::{self, Data, Refusal};
use super::container::{data_uri, percent_decode};
use super::{GltfImport, member};
use crate::{SceneTexture, TextureWrap};

/// Images past this size are not loaded.
const MAX_IMAGE: usize = 1 << 30;

/// Each image's file bytes, or why it could not be loaded.
pub(super) type Images = Vec<Result<Vec<u8>, &'static str>>;

/// Every image, from a `data:` URI, the resolver or a buffer view
/// [§3.8.2], keeping at most `budget` bytes of them in all: each entry
/// pays for its own copy, even of a buffer view another entry names. A
/// failure is held for the materials that use the image.
pub(super) fn images<R>(doc: &Json, buffers: &[Vec<u8>], resolve: &mut R, budget: usize) -> Images
where
    R: FnMut(&str) -> Option<Vec<u8>>,
{
    let views = member(doc, "bufferViews");
    let image = |im: &Json, resolve: &mut R| match im.get("uri").and_then(Json::as_str) {
        Some(uri) if uri.starts_with("data:") => {
            data_uri(uri).map(Cow::Owned).ok_or("a bad data URI")
        }
        Some(uri) => resolve(&percent_decode(uri))
            .map(Cow::Owned)
            .ok_or("its file could not be read"),
        None => im
            .get("bufferView")
            .and_then(Json::as_u32)
            .and_then(|v| views.get(v as usize))
            .and_then(|v| accessor::view_bytes(v, buffers))
            .map(Cow::Borrowed)
            .ok_or("its buffer view is out of range"),
    };
    let mut left = budget;
    member(doc, "images")
        .iter()
        .map(|im| image(im, resolve).and_then(|b| checked(b, &mut left)))
        .collect()
}

/// `b` as a kept image, its length taken from `left` before it is copied.
fn checked(b: Cow<'_, [u8]>, left: &mut usize) -> Result<Vec<u8>, &'static str> {
    if b.len() > MAX_IMAGE {
        return Err("it is too large");
    }
    if crate::write::picture_info(&b).is_none() {
        return Err("it is not a PNG or JPEG file");
    }
    *left = left
        .checked_sub(b.len())
        .ok_or("the import's limit on image bytes is reached")?;
    Ok(b.into_owned())
}

/// How one material draws a texture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TextureUse {
    /// The `TEXCOORD_n` set read.
    pub(super) set: usize,
    /// glTF (u, v) to picture (u, v): `u' = m[0]·(u, v, 1)`, `v'` from
    /// `m[1]`.
    pub(super) matrix: [[f64; 3]; 2],
}

/// A texture a material names, ready to draw or bake.
#[derive(Debug, Clone, Copy)]
pub(super) struct Source<'a> {
    /// The glTF texture index.
    pub(super) texture: usize,
    /// The image file's bytes.
    pub(super) image: &'a [u8],
    pub(super) wrap: [TextureWrap; 2],
    pub(super) using: TextureUse,
}

/// What makes two scene textures the same: the base and occlusion texture
/// indices, the occlusion strength's bits, and the alpha handling.
pub(super) type Key = (Option<usize>, Option<(usize, u64)>, (u8, u64, u64));

/// The glTF texture-to-scene map: one [`SceneTexture`] per distinct
/// [`Key`], holding at most `budget` picture bytes in all.
#[derive(Debug)]
pub(super) struct Textures {
    seen: HashMap<Key, usize>,
    budget: usize,
    left: usize,
}

impl Textures {
    /// An empty cache that holds at most `budget` picture bytes.
    pub(super) fn new(budget: usize) -> Self {
        Self {
            seen: HashMap::new(),
            budget,
            left: budget,
        }
    }

    /// Material `label`'s `kind` texture `info` ("base colour",
    /// "occlusion"). `None`, with a note ending in `fallback`, when it
    /// cannot be drawn.
    pub(super) fn source<'a>(
        doc: &Json,
        images: &'a Images,
        info: &Json,
        (label, kind, fallback): (&str, &str, &str),
        out: &mut GltfImport,
    ) -> Option<Source<'a>> {
        let mut drawn = |why: &str| {
            out.notes
                .push(format!("{label}: {kind} texture {why}; {fallback}"));
        };
        let Some(t) = info.get("index").and_then(Json::as_u32).map(|t| t as usize) else {
            drawn("names no texture");
            return None;
        };
        let Some(tex) = member(doc, "textures").get(t) else {
            drawn("does not exist");
            return None;
        };
        let Some(source) = tex.get("source").and_then(Json::as_u32) else {
            drawn("uses an image format pdfcer does not read (WebP, KTX2)");
            return None;
        };
        match images.get(source as usize) {
            Some(Ok(image)) => Some(Source {
                texture: t,
                image,
                wrap: wraps(doc, tex),
                using: uses(info),
            }),
            Some(Err(why)) => {
                drawn(&format!("dropped: {why}"));
                None
            }
            None => {
                drawn("names no image");
                None
            }
        }
    }

    /// The scene texture for `key`, made by `make` the first time from a
    /// picture of `bytes` bytes. `None`, with a note naming material
    /// `label`, when that would pass the import's texture-byte limit.
    pub(super) fn add(
        &mut self,
        (key, bytes): (Key, usize),
        label: &str,
        make: impl FnOnce() -> SceneTexture,
        out: &mut GltfImport,
    ) -> Option<usize> {
        if let Some(&k) = self.seen.get(&key) {
            return Some(k);
        }
        let Some(left) = self.left.checked_sub(bytes) else {
            out.notes.push(format!(
                "{label}: texture dropped: the import's limit of {} bytes of scene textures \
                 is reached; drawn in its base colour",
                self.budget
            ));
            return None;
        };
        self.left = left;
        let k = out.scene.textures.len();
        out.scene.textures.push(make());
        self.seen.insert(key, k);
        Some(k)
    }
}

/// A sampler's wrap modes along u and v; repeat when unset [§3.8.4].
fn wraps(doc: &Json, tex: &Json) -> [TextureWrap; 2] {
    let sampler = tex
        .get("sampler")
        .and_then(Json::as_u32)
        .and_then(|s| member(doc, "samplers").get(s as usize));
    let wrap = |key| match sampler.and_then(|s| s.get(key)).and_then(Json::as_u32) {
        Some(33071) => TextureWrap::Clamp,
        Some(33648) => TextureWrap::MirroredRepeat,
        _ => TextureWrap::Repeat,
    };
    [wrap("wrapS"), wrap("wrapT")]
}

/// The coordinate set and `KHR_texture_transform` of a texture info: the
/// matrix `T · R · S`, rotation counter-clockwise in glTF's top-left
/// (u, v) space.
fn uses(info: &Json) -> TextureUse {
    let set = |v: &Json| v.get("texCoord").and_then(Json::as_u32).map(|s| s as usize);
    let tt = info
        .get("extensions")
        .and_then(|e| e.get("KHR_texture_transform"));
    let pair = |key, default: [f64; 2]| {
        let a = tt.and_then(|t| t.get(key)).and_then(Json::as_array);
        match a.map(|a| a.iter().map(Json::as_f64).collect::<Option<Vec<_>>>()) {
            Some(Some(v)) => match v[..] {
                [a, b] => [a, b],
                _ => default,
            },
            _ => default,
        }
    };
    let [ox, oy] = pair("offset", [0.0, 0.0]);
    let [sx, sy] = pair("scale", [1.0, 1.0]);
    let r = tt
        .and_then(|t| t.get("rotation"))
        .and_then(Json::as_f64)
        .unwrap_or(0.0);
    let (sin, cos) = r.sin_cos();
    TextureUse {
        set: tt.and_then(set).or_else(|| set(info)).unwrap_or(0),
        matrix: [[cos * sx, sin * sy, ox], [-sin * sx, cos * sy, oy]],
    }
}

/// A primitive's `TEXCOORD_n` through `using`, flipped from glTF's
/// top-left origin to the scene's bottom-left; `count` is its vertex
/// count.
pub(super) fn coordinates(
    doc: &Json,
    data: &Data<'_>,
    attrs: &Json,
    using: TextureUse,
    count: usize,
) -> Result<Vec<[f64; 2]>, Refusal> {
    let key = format!("TEXCOORD_{}", using.set);
    let Some(a) = attrs.get(&key).and_then(Json::as_u32) else {
        return Err(format!("it has no {key}"));
    };
    let flat = accessor::read(doc, data, a as usize, 2)?;
    if flat.len() != 2 * count {
        return Err(format!("its {key} and POSITION counts differ"));
    }
    let [m0, m1] = using.matrix;
    Ok(flat
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[u, v]| {
            let tu = m0[0] * u + m0[1] * v + m0[2];
            let tv = m1[0] * u + m1[1] * v + m1[2];
            [tu, 1.0 - tv]
        })
        .collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_texture_transform_is_offset_rotation_scale() {
        let info = pdfcer_json::parse(
            br#"{"index":0,"texCoord":0,"extensions":{"KHR_texture_transform":
                {"offset":[0.5,0.25],"rotation":1.5707963267948966,"scale":[2,3],"texCoord":1}}}"#,
        )
        .unwrap();
        let u = uses(&info);
        assert_eq!(u.set, 1, "the extension's texCoord wins");
        // (1, 0): scaled to (2, 0), turned to (0, -2), offset to (0.5, -1.75).
        let [m0, m1] = u.matrix;
        assert!((m0[0] + m0[2] - 0.5).abs() < 1e-12);
        assert!((m1[0] + m1[2] + 1.75).abs() < 1e-12);
        let plain = uses(&pdfcer_json::parse(br#"{"index":0}"#).unwrap());
        assert_eq!(plain.set, 0);
        assert_eq!(plain.matrix, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    }

    /// The KHR_texture_transform README's example (offset (0, 1), a quarter turn, half scale),
    /// which its caption says draws only the image's lower-left quadrant; v runs down.
    #[test]
    fn the_extensions_own_example_draws_the_lower_left_quadrant() {
        let info = pdfcer_json::parse(
            br#"{"index":0,"extensions":{"KHR_texture_transform":
                {"offset":[0,1],"rotation":1.57079632679,"scale":[0.5,0.5]}}}"#,
        )
        .unwrap();
        let [m0, m1] = uses(&info).matrix;
        for [u, v] in [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]] {
            let (tu, tv) = (m0[0] * u + m0[1] * v + m0[2], m1[0] * u + m1[1] * v + m1[2]);
            assert!(
                (-1e-9..=0.5 + 1e-9).contains(&tu) && (0.5 - 1e-9..=1.0 + 1e-9).contains(&tv),
                "({u}, {v}) to ({tu}, {tv})"
            );
        }
    }
}
