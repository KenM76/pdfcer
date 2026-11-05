//! Which picture a glTF material draws with: its base-colour texture as
//! stored, or one baked with its occlusion map or alpha-mask cut-out
//! (see [`super::bake`]); and the maps PRC carries no counterpart for.

use pdfcer_json::Json;

use super::bake::{self, Alpha, Recipe};
use super::textures::{Images, Key, Source, TextureUse, Textures};
use super::{GltfImport, GltfOcclusion};
use crate::SceneTexture;

/// The texture a material draws with.
#[derive(Debug, Clone, Copy)]
pub(super) struct Drawn {
    /// Index into the scene's textures.
    pub(super) texture: usize,
    pub(super) using: TextureUse,
    /// Whether the texture's alpha cuts out an alpha mask.
    pub(super) cut: bool,
}

/// What [`texture`] reads a material from.
pub(super) struct Inputs<'a> {
    pub(super) doc: &'a Json,
    pub(super) images: &'a Images,
    pub(super) occlusion: GltfOcclusion,
}

/// Material `m`'s texture, baked when it has an occlusion map or cuts out
/// an alpha mask; `None` when it draws none. Every loss is a note and
/// every bake an inference.
pub(super) fn texture(
    cx: &Inputs<'_>,
    m: &Json,
    (label, alpha): (&str, Alpha),
    textures: &mut Textures,
    out: &mut GltfImport,
) -> Option<Drawn> {
    let base = m
        .get("pbrMetallicRoughness")
        .and_then(|p| p.get("baseColorTexture"))
        .and_then(|info| {
            let what = (label, "base colour", "drawn in its base colour");
            Textures::source(cx.doc, cx.images, info, what, out)
        });
    let occlusion = occlusion(cx, m, label, base.as_ref(), out);
    let cut = matches!(alpha, Alpha::Mask { .. }) && base.is_some();
    if occlusion.is_some() || cut {
        let recipe = Recipe {
            base: base.map(|b| b.image),
            occlusion: occlusion.map(|(o, s)| (o.image, s)),
            alpha,
        };
        match bake::bake(&recipe) {
            Ok(png) => return baked(png, &recipe, (base, occlusion), label, textures, out),
            Err(why) => {
                let what = match (occlusion.is_some(), cut) {
                    (true, true) => "occlusion texture and alpha mask cut-out",
                    (true, false) => "occlusion texture",
                    _ => "alpha mask cut-out",
                };
                out.notes.push(format!("{label}: {what} not drawn: {why}"));
            }
        }
    }
    let b = base?;
    let key = (Some(b.texture), None, alpha_key(alpha, false));
    let keep = alpha == Alpha::Blend;
    let make = || SceneTexture {
        image: b.image.to_vec(),
        wrap: b.wrap,
        alpha: keep,
    };
    Some(Drawn {
        texture: textures.add((key, b.image.len()), label, make, out)?,
        using: b.using,
        cut: false,
    })
}

/// The material's occlusion texture and strength when it can be baked:
/// asked for, readable, and over the base texture's coordinates.
fn occlusion<'a>(
    cx: &Inputs<'a>,
    m: &Json,
    label: &str,
    base: Option<&Source<'_>>,
    out: &mut GltfImport,
) -> Option<(Source<'a>, f64)> {
    let info = m.get("occlusionTexture")?;
    if cx.occlusion == GltfOcclusion::Drop {
        out.notes
            .push(format!("{label}: occlusion texture not drawn, as asked"));
        return None;
    }
    let what = (label, "occlusion", "not drawn");
    let o = Textures::source(cx.doc, cx.images, info, what, out)?;
    if let Some(b) = base
        && (b.using != o.using || b.wrap != o.wrap)
    {
        out.notes.push(format!(
            "{label}: occlusion texture not drawn: its texture coordinates, transform or \
             wrapping differ from the base colour texture's"
        ));
        return None;
    }
    let strength = info.get("strength").and_then(Json::as_f64).unwrap_or(1.0);
    Some((o, strength.clamp(0.0, 1.0)))
}

/// A baked picture added to the scene, with the inferences it rests on;
/// `None` when the texture-byte limit drops it.
fn baked(
    png: Vec<u8>,
    recipe: &Recipe<'_>,
    (base, occlusion): (Option<Source<'_>>, Option<(Source<'_>, f64)>),
    label: &str,
    textures: &mut Textures,
    out: &mut GltfImport,
) -> Option<Drawn> {
    let cut = matches!(recipe.alpha, Alpha::Mask { .. }) && base.is_some();
    let lead = base.or(occlusion.map(|(o, _)| o));
    let key: Key = (
        base.map(|b| b.texture),
        occlusion.map(|(o, s)| (o.texture, s.to_bits())),
        alpha_key(recipe.alpha, cut),
    );
    let wrap = lead.map_or([crate::TextureWrap::Repeat; 2], |l| l.wrap);
    let alpha = recipe.keeps_alpha();
    let bytes = png.len();
    let make = || SceneTexture {
        image: png,
        wrap,
        alpha,
    };
    let texture = textures.add((key, bytes), label, make, out)?;
    if let Some((_, s)) = occlusion {
        let into = if base.is_some() {
            "the base colour texture"
        } else {
            "a texture of the base colour"
        };
        out.inferred.push(format!(
            "{label}: occlusion texture baked into {into} at strength {s}; it darkens \
             direct light as well as ambient"
        ));
    }
    if let Alpha::Mask { cutoff, .. } = recipe.alpha
        && cut
    {
        out.inferred.push(format!(
            "{label}: alpha mask cut out by the texture's alpha against cutoff {cutoff}; \
             cut edges are filtered, not hard"
        ));
    }
    Some(Drawn {
        texture,
        using: lead.map_or(
            TextureUse {
                set: 0,
                matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            },
            |l| l.using,
        ),
        cut,
    })
}

/// The alpha handling as part of a texture [`Key`].
fn alpha_key(alpha: Alpha, cut: bool) -> (u8, u64, u64) {
    match alpha {
        Alpha::Opaque => (0, 0, 0),
        Alpha::Blend => (1, 0, 0),
        Alpha::Mask { cutoff, factor } if cut => (2, cutoff.to_bits(), factor.to_bits()),
        Alpha::Mask { .. } => (3, 0, 0),
    }
}

/// Materials with each map PRC has no counterpart for.
#[derive(Debug, Default)]
pub(super) struct Unread {
    normal: usize,
    metal: usize,
    emissive: usize,
}

impl Unread {
    /// Counts material `m`'s normal, metallic-roughness and emissive maps
    /// (an `emissiveFactor` is carried).
    pub(super) fn count(&mut self, m: &Json) {
        let has = |v: Option<&Json>| usize::from(v.is_some());
        self.normal += has(m.get("normalTexture"));
        self.metal += has(m
            .get("pbrMetallicRoughness")
            .and_then(|p| p.get("metallicRoughnessTexture")));
        self.emissive += has(m.get("emissiveTexture"));
    }

    /// One note naming what was not carried, if anything.
    pub(super) fn note(&self, out: &mut GltfImport) {
        let parts: Vec<String> = [
            (self.normal, "a normal map"),
            (self.metal, "a metallic-roughness map"),
            (self.emissive, "an emissive map"),
        ]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} with {what}"))
        .collect();
        if !parts.is_empty() {
            out.notes.push(format!(
                "materials: {}; not drawn, since the PRC is written with a diffuse colour, \
                 texture and emissive colour only",
                parts.join(", ")
            ));
        }
    }
}
