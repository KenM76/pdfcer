//! glTF textures as [`SceneTexture`]s, and the coordinates
//! that draw them [glTF 2.0 §3.8, §3.9.2; `KHR_texture_transform`].

use pdfcer_json::Json;

use super::accessor::{self, Refusal};
use super::container::{data_uri, percent_decode};
use super::{GltfImport, member};
use crate::{SceneTexture, TextureWrap};

/// Images past this size are not loaded.
const MAX_IMAGE: usize = 1 << 30;

/// Each image's file bytes, or why it could not be loaded.
pub(super) type Images = Vec<Result<Vec<u8>, &'static str>>;

/// Every image, from a `data:` URI, the resolver or a buffer view
/// [§3.8.2]. A failure is held for the materials that use the image.
pub(super) fn images<R>(doc: &Json, buffers: &[Vec<u8>], resolve: &mut R) -> Images
where
    R: FnMut(&str) -> Option<Vec<u8>>,
{
    let views = member(doc, "bufferViews");
    let image = |im: &Json, resolve: &mut R| match im.get("uri").and_then(Json::as_str) {
        Some(uri) if uri.starts_with("data:") => data_uri(uri).ok_or("a bad data URI"),
        Some(uri) => resolve(&percent_decode(uri)).ok_or("its file could not be read"),
        None => im
            .get("bufferView")
            .and_then(Json::as_u32)
            .and_then(|v| views.get(v as usize))
            .and_then(|v| accessor::view_bytes(v, buffers))
            .map(<[u8]>::to_vec)
            .ok_or("its buffer view is out of range"),
    };
    member(doc, "images")
        .iter()
        .map(|im| image(im, resolve).and_then(|b| checked(b)))
        .collect()
}

fn checked(b: Vec<u8>) -> Result<Vec<u8>, &'static str> {
    if b.len() > MAX_IMAGE {
        return Err("it is too large");
    }
    match crate::write::picture_info(&b) {
        Some(_) => Ok(b),
        None => Err("it is not a PNG or JPEG file"),
    }
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
/// [`Key`].
#[derive(Debug, Default)]
pub(super) struct Textures {
    seen: Vec<(Key, usize)>,
}

impl Textures {
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

    /// The scene texture for `key`, made by `make` the first time.
    pub(super) fn add(
        &mut self,
        key: Key,
        make: impl FnOnce() -> SceneTexture,
        out: &mut GltfImport,
    ) -> usize {
        if let Some(&(_, k)) = self.seen.iter().find(|(seen, _)| *seen == key) {
            return k;
        }
        let k = out.scene.textures.len();
        out.scene.textures.push(make());
        self.seen.push((key, k));
        k
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
    buffers: &[Vec<u8>],
    attrs: &Json,
    using: TextureUse,
    count: usize,
) -> Result<Vec<[f64; 2]>, Refusal> {
    let key = format!("TEXCOORD_{}", using.set);
    let Some(a) = attrs.get(&key).and_then(Json::as_u32) else {
        return Err(format!("it has no {key}"));
    };
    let flat = accessor::read(doc, buffers, a as usize, 2)?;
    if flat.len() != 2 * count {
        return Err(format!("its {key} and POSITION counts differ"));
    }
    let [m0, m1] = using.matrix;
    Ok(flat
        .chunks_exact(2)
        .filter_map(|c| match *c {
            [u, v] => Some((u, v)),
            _ => None,
        })
        .map(|(u, v)| {
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
}
