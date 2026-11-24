//! A display list packed for [`super::glsl::GLSL_ES_300_FRAGMENT`]: every fill
//! and clip as tiled integer edges, every paint as the `tiny-skia` 0.11
//! low-precision pipeline's colour, and per tile the paints reaching it,
//! in one integer texture.

use pdfcer_core::page_tree::Rect as PageRect;
use tiny_skia::{Color, FillRule, Path, PathSegment, Transform};

use super::bins::{TILE_H, TILE_W};
use super::block::FillBlock;
use super::fill_edges::{self, FillEdges};
use super::image::{self, IMAGE_PAINT, ImagePaint};
use super::layer::LAYER_BEGIN;
use crate::RenderError;
use crate::cancel::RenderCancel;
use crate::canvas::{Brush, BrushSpec};
use crate::device_clip::{self, Fit};
use crate::display_list::{ClipDef, DeviceBounds, DisplayList, DisplayListKey, Op};

/// Texels per row of [`GpuScene::data`].
pub const SCENE_TEXTURE_WIDTH: u32 = 2048;

/// Rows per layer of [`GpuScene::data`], the smallest `GL_MAX_TEXTURE_SIZE`
/// WebGL 2 guarantees.
pub const SCENE_TEXTURE_ROWS: u32 = 2048;

/// Layers [`GpuScene::data`] may need before the scene is refused: 1 GiB.
const MAX_LAYERS: usize = 16;

/// A page ready for the graphics card: draw a `width` × `height` `RGBA8`
/// framebuffer with [`super::glsl::GLSL_ES_300_VERTEX`] (three vertices,
/// no attributes) and [`super::glsl::GLSL_ES_300_FRAGMENT`], `data` bound
/// to `u_scene`; `readPixels` of the framebuffer is then
/// [`DisplayList::replay_region`]'s RGBA8, byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct GpuScene {
    /// Raster width, pixels.
    pub width: u32,
    /// Raster height, pixels.
    pub height: u32,
    /// `u_scene`, read by the shader with `texelFetch`.
    pub data: IntTexture,
}

/// An `RGBA32I` 2D array texture: `texels` holds `width` × `height` ×
/// `layers` × 4 values, layer 0 row 0 first; texel `i` sits at
/// `(i % width, i / width % height, i / (width * height))`. `height` is
/// [`SCENE_TEXTURE_ROWS`] whenever `layers` exceeds 1.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct IntTexture {
    /// The values, 4 per texel.
    pub texels: Vec<i32>,
    /// Texels per row.
    pub width: u32,
    /// Rows per layer.
    pub height: u32,
    /// Layers.
    pub layers: u32,
}

/// Why a page is drawn by the CPU instead ([`DisplayList::gpu_scene`]).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GpuRefusal {
    /// The list cannot be replayed for this region at all.
    #[error(transparent)]
    Render(#[from] RenderError),
    /// The page paints an image the graphics card cannot sample exactly:
    /// bicubic, a non-finite inverse, or bilinear samples beyond 2^24
    /// image pixels.
    #[error("the page paints an image the graphics card cannot sample exactly")]
    Image,
    /// The list paints a shading as a gradient brush, which only an export
    /// recording carries; a snapshot's shadings are drawn.
    #[error("the page paints a shading")]
    Shading,
    /// The page uses a soft mask, a blend mode the graphics card does not
    /// draw (non-separable, colour dodge or burn, soft light, or any but
    /// Normal on an image), or groups nested more than 8 deep.
    #[error("the page uses transparency the graphics card does not draw")]
    Transparency,
    /// A path still carries curves after fitting, which the scene does not
    /// flatten.
    #[error("a path keeps curves after fitting")]
    Curves,
    /// The packed page would not fit 16 layers of 2,048 × 2,048 texels.
    #[error("the page needs {texels} texels, more than the scene texture holds")]
    TooMuchData {
        /// Texels needed.
        texels: usize,
    },
    /// The [`RenderCancel`] passed to [`DisplayList::gpu_scene_cancellable`]
    /// was cancelled: the page was not judged, only abandoned.
    #[error("packing was cancelled")]
    Cancelled,
}

impl DisplayList {
    /// This list packed to draw `region` on the graphics card, byte for
    /// byte the raster [`Self::replay_region`] returns (decision 223).
    ///
    /// Solid, image and shading fills and strokes, clips and glyph outlines
    /// are drawn there; a page needing anything else is refused with the reason, for
    /// the host to replay on the CPU.
    ///
    /// # Errors
    ///
    /// [`GpuRefusal::Render`] where [`Self::replay_region`] fails; another
    /// [`GpuRefusal`] variant names what the graphics card cannot draw.
    pub fn gpu_scene(
        &self,
        expected: DisplayListKey,
        region: PageRect,
    ) -> Result<GpuScene, GpuRefusal> {
        self.pack_scene(expected, region, None)
    }

    /// [`Self::gpu_scene`], abandoned within one block of ops once `cancel`
    /// is cancelled, checked before and throughout packing.
    ///
    /// # Errors
    ///
    /// [`GpuRefusal::Cancelled`] once `cancel` is cancelled, distinct from
    /// every refusal that says the page cannot be drawn; otherwise as
    /// [`Self::gpu_scene`].
    pub fn gpu_scene_cancellable(
        &self,
        expected: DisplayListKey,
        region: PageRect,
        cancel: &RenderCancel,
    ) -> Result<GpuScene, GpuRefusal> {
        self.pack_scene(expected, region, Some(cancel))
    }

    fn pack_scene(
        &self,
        expected: DisplayListKey,
        region: PageRect,
        cancel: Option<&RenderCancel>,
    ) -> Result<GpuScene, GpuRefusal> {
        let mut frame = self.region_frame(expected, region)?;
        frame.cancel = cancel;
        frame.check_cancel()?;
        let (width, height) = (frame.width, frame.height);
        if width == 0
            || height == 0
            || width > crate::MAX_PIXMAP_EDGE
            || height > crate::MAX_PIXMAP_EDGE
        {
            return Err(RenderError::BadRasterSize { width, height }.into());
        }
        let packed = device_clip::with_lattice_anchor(frame.anchor, || {
            device_clip::with_row_origin(0, || pack(&frame))
        })?;
        Ok(GpuScene {
            width,
            height,
            data: packed,
        })
    }
}

/// What [`DisplayList::replay_region`] replays, for [`pack`].
pub(crate) struct Frame<'a> {
    pub ops: &'a [Op],
    pub clips: &'a [ClipDef],
    pub width: u32,
    pub height: u32,
    pub x0: f32,
    pub y0: f32,
    pub anchor: (f64, f64),
    pub bounds: DeviceBounds,
    pub cancel: Option<&'a RenderCancel>,
}

impl Frame<'_> {
    /// [`GpuRefusal::Cancelled`] once the frame's token is cancelled.
    pub(crate) fn check_cancel(&self) -> Result<(), GpuRefusal> {
        if self.cancel.is_some_and(RenderCancel::is_cancelled) {
            return Err(GpuRefusal::Cancelled);
        }
        Ok(())
    }

    /// `ctm` in the region's pixels, as the replay maps it there.
    pub(crate) fn to_region(&self, ctm: Transform) -> Transform {
        ctm.post_translate(-self.x0, -self.y0)
    }

    /// The raster's `DrawTiler` tiles, `(x, y, width, height)`, in its
    /// order: one tile unless a side exceeds [`fill_edges::DRAW_TILE`].
    pub(crate) fn draw_tiles(&self) -> Vec<(u32, u32, u32, u32)> {
        let t = fill_edges::DRAW_TILE;
        let mut v = Vec::new();
        for y in (0..self.height).step_by(t as usize) {
            for x in (0..self.width).step_by(t as usize) {
                v.push((x, y, (self.width - x).min(t), (self.height - y).min(t)));
            }
        }
        v
    }

    /// The edges a fill of `path` under `ts` scans in each of
    /// [`Self::draw_tiles`], in that tile's pixels; `None` where it draws
    /// nothing. As `fill_path` does, the path is translated to each tile
    /// and back in turn, so a later tile sees the rounding of every earlier
    /// round trip.
    pub(crate) fn edges(
        &self,
        path: &Path,
        rule: FillRule,
        ts: Transform,
        anti_alias: bool,
    ) -> Result<Vec<Option<FillEdges>>, GpuRefusal> {
        let tiles = self.draw_tiles();
        let none = || (0..tiles.len()).map(|_| None).collect();
        let Fit::Device(p) = device_clip::fit(path, ts, self.width, self.height) else {
            return Ok(none());
        };
        if p.segments()
            .any(|s| matches!(s, PathSegment::QuadTo(..) | PathSegment::CubicTo(..)))
        {
            return Err(GpuRefusal::Curves);
        }
        if let [_] = tiles.as_slice() {
            let size = (self.width, self.height);
            return Ok(vec![fill_edges::build(&p, rule, size, anti_alias)]);
        }
        if fill_edges::skips(&p) {
            return Ok(none());
        }
        let mut out = Vec::with_capacity(tiles.len());
        let mut path = Some(p);
        for &(x, y, w, h) in &tiles {
            let (dx, dy) = (x as f32, y as f32);
            let Some(local) = path
                .take()
                .and_then(|p| p.transform(Transform::from_translate(-dx, -dy)))
            else {
                break;
            };
            out.push(fill_edges::scan(&local, rule, (w, h), anti_alias));
            path = local.transform(Transform::from_translate(dx, dy));
        }
        out.resize_with(tiles.len(), || None);
        Ok(out)
    }
}

/// One paint as the shader draws it.
struct Paint {
    /// Its fill; `usize::MAX` for a group marker.
    fill: usize,
    /// A [`super::layer`] marker's op texel.
    marker: Option<[i32; 4]>,
    clip: Option<usize>,
    /// The pipeline's colour, premultiplied, `r | g << 8 | b << 16 | a << 24`.
    color: i32,
    /// The colour an opaque unclipped paint writes at full coverage.
    memset: i32,
    /// Its [`super::blend::code`].
    blend: i32,
    image: Option<ImagePaint>,
    shade: Option<std::sync::Arc<super::shade::ShadePaint>>,
}

/// The texture's sections, gathered before they are laid out.
struct Builder {
    fills: Vec<FillBlock>,
    /// Per clip: its fill in each `DrawTiler` tile, its parent.
    clips: Vec<ClipFills>,
    paints: Vec<Paint>,
}

fn pack_rgba(c: [u8; 4]) -> i32 {
    i32::from_le_bytes(c)
}

/// The pipeline colours of a solid paint ([`IMAGE_PAINT`] and 0 for an
/// image) and its [`super::blend::code`], or why the scene cannot draw it.
pub(crate) fn colors(brush: &BrushSpec) -> Result<(i32, i32, i32), GpuRefusal> {
    let blend = super::blend::code(brush.blend)?;
    let rgba = match &brush.brush {
        Brush::Solid { rgba } => *rgba,
        Brush::Gradient(_) => return Err(GpuRefusal::Shading),
        Brush::Image { .. } if blend == 0 => return Ok((IMAGE_PAINT, 0, 0)),
        Brush::Image { .. } => return Err(GpuRefusal::Transparency),
    };
    let p = Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]).premultiply();
    // `push_uniform_color`'s rounding.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // in 0..=255.5
    let q = |v: f32| (v * 255.0 + 0.5) as u8;
    let color = [q(p.red()), q(p.green()), q(p.blue()), q(p.alpha())];
    let m = p.to_color_u8();
    Ok((
        pack_rgba(color),
        pack_rgba([m.red(), m.green(), m.blue(), m.alpha()]),
        blend,
    ))
}

/// Every clip's and paint's fill, in the order the scene lays them out.
fn builder(frame: &Frame<'_>) -> Result<Builder, GpuRefusal> {
    let (clips, paints) = super::prepare::all(frame)?;
    let mut b = Builder {
        fills: Vec::with_capacity(clips.len() + paints.len()),
        clips: Vec::with_capacity(clips.len()),
        paints: Vec::with_capacity(paints.len()),
    };
    for (tiles, parent) in clips {
        let fills = tiles
            .into_iter()
            .map(|f| {
                f.map(|f| {
                    b.fills.push(f);
                    b.fills.len() - 1
                })
            })
            .collect();
        b.clips.push((fills, parent));
    }
    for p in paints {
        let fill = match p.block {
            Some(f) => {
                b.fills.push(f);
                b.fills.len() - 1
            }
            None => usize::MAX,
        };
        b.paints.push(Paint {
            fill,
            marker: p.marker,
            clip: p.clip,
            color: p.color,
            memset: p.memset,
            blend: p.blend,
            image: p.image,
            shade: p.shade,
        });
    }
    Ok(b)
}

/// `frame`'s scene texture.
fn pack(frame: &Frame<'_>) -> Result<IntTexture, GpuRefusal> {
    let b = builder(frame)?;
    frame.check_cancel()?;
    let stride = frame.draw_tiles().len();
    let cols = (frame.width as i32 + TILE_W - 1) / TILE_W;
    let rows = (frame.height as i32 + TILE_H - 1) / TILE_H;
    let lists = tile_lists(&b, (cols, rows));
    frame.check_cancel()?;
    let fill_texels: usize = b.fills.iter().map(|f| f.texels.len()).sum();
    let clip_base = 2 + fill_texels;
    let op_base = clip_base + b.clips.len() * stride;
    let tile_base = op_base + b.paints.len();
    let entry_base = tile_base + lists.len();
    let image_base = entry_base + lists.iter().map(Vec::len).sum::<usize>().div_ceil(4);
    let samplers: Vec<_> = b.paints.iter().map(|p| p.image.as_ref()).collect();
    let (mut rec_of, images, div_base) = image::sections(&samplers, image_base);
    let shade_base = image_base + images.len();
    let shadings: Vec<_> = b.paints.iter().map(|p| p.shade.as_ref()).collect();
    let (shade_of, shades) = super::shade::sections(&shadings, shade_base);
    for (r, s) in rec_of.iter_mut().zip(&shade_of) {
        if *s >= 0 {
            *r = *s;
        }
    }
    let total = shade_base + shades.len();
    let (width, per) = (SCENE_TEXTURE_WIDTH as usize, SCENE_TEXTURE_ROWS as usize);
    if total > width * per * MAX_LAYERS {
        return Err(GpuRefusal::TooMuchData { texels: total });
    }
    let rows_needed = total.div_ceil(width).max(1);
    let (height, layers) = (rows_needed.min(per), rows_needed.div_ceil(per));
    let mut out: Vec<[i32; 4]> = vec![[0; 4]; width * height * layers];
    out.truncate(clip_base);
    if let Some([a, b]) = out.get_mut(..2) {
        *a = [frame.width as i32, frame.height as i32, cols, rows];
        *b = [tile_base as i32, op_base as i32, clip_base as i32, div_base];
    }
    let mut headers = Vec::with_capacity(b.fills.len());
    let mut at = 2;
    for f in &b.fills {
        headers.push(at as i32);
        at += f.texels.len();
    }
    super::prepare::copy_fills(out.get_mut(2..).unwrap_or_default(), b.fills);
    for p in &b.paints {
        let origin = headers.get(p.fill).map(|h| *h as usize + 3);
        if let Some([_, _, _, code]) = origin.and_then(|o| out.get_mut(o)) {
            *code = p.blend;
        }
    }
    records(
        &mut out,
        (&b.clips, &b.paints, &rec_of),
        &headers,
        (&lists, entry_base),
    );
    out.extend(images);
    out.extend(shades);
    out.resize(width * height * layers, [0; 4]);
    Ok(IntTexture {
        texels: out.into_flattened(),
        width: width as u32,
        height: height as u32,
        layers: layers as u32,
    })
}

/// The clip, op and tile-list records and the packed tile entries, after
/// the fills whose headers are `headers`; an image paint's op carries its
/// record texel `rec_of` in place of a memset colour.
fn records(
    out: &mut Vec<[i32; 4]>,
    (clips, paints, rec_of): (&[ClipFills], &[Paint], &[i32]),
    headers: &[i32],
    (lists, entry_base): (&[Vec<i32>], usize),
) {
    for (fills, parent) in clips {
        for fill in fills {
            let h = fill.and_then(|f| headers.get(f).copied()).unwrap_or(-1);
            out.push([h, parent.map_or(-1, |p| p as i32), 0, 0]);
        }
    }
    for (p, rec) in paints.iter().zip(rec_of) {
        if let Some(m) = p.marker {
            out.push(m);
            continue;
        }
        let h = headers.get(p.fill).copied().unwrap_or(-1);
        let memset = if *rec >= 0 { *rec } else { p.memset };
        out.push([h, p.clip.map_or(-1, |c| c as i32), p.color, memset]);
    }
    let mut entry = (entry_base * 4) as i32;
    for l in lists {
        out.push([entry, l.len() as i32, 0, 0]);
        entry += l.len() as i32;
    }
    let run: Vec<i32> = lists.iter().flatten().copied().collect();
    out.extend(
        run.chunks(4)
            .map(|c| std::array::from_fn(|k| c.get(k).copied().unwrap_or(0))),
    );
}

/// A clip's fill in each `DrawTiler` tile, and its parent.
type ClipFills = (Vec<Option<usize>>, Option<usize>);

/// The raster tiles `p`'s fill covers, unless its clip's fill misses them.
fn paint_tiles(b: &Builder, p: &Paint, (cols, rows): (i32, i32)) -> Vec<u32> {
    let Some(fill) = b.fills.get(p.fill) else {
        return Vec::new();
    };
    let clip: Option<Vec<_>> = p.clip.and_then(|c| b.clips.get(c)).map(|(fills, _)| {
        fills
            .iter()
            .flatten()
            .filter_map(|f| b.fills.get(*f))
            .map(|f| &f.footprint)
            .collect()
    });
    let mut out = Vec::new();
    for (col, row) in fill.footprint.tiles() {
        if !(0..cols).contains(&col) || !(0..rows).contains(&row) {
            continue;
        }
        // A clip whose own fills miss the tile admits nothing there.
        if let Some(c) = &clip
            && !c.iter().any(|f| f.covers(col, row))
        {
            continue;
        }
        out.push((row * cols + col) as u32);
    }
    out
}

/// Each tile's paints, in paint order. A group's markers reach every tile
/// one of its members reaches.
fn tile_lists(b: &Builder, (cols, rows): (i32, i32)) -> Vec<Vec<i32>> {
    let mut reach: Vec<Vec<u32>> = Vec::with_capacity(b.paints.len());
    // Per open group: its begin marker's index, the tiles its members reach.
    let mut open: Vec<(usize, Vec<u32>)> = Vec::new();
    for p in &b.paints {
        let tiles = match p.marker {
            Some([_, _, LAYER_BEGIN, _]) => {
                open.push((reach.len(), Vec::new()));
                Vec::new()
            }
            Some(_) => {
                let (begin, mut seen) = open.pop().unwrap_or_default();
                seen.sort_unstable();
                seen.dedup();
                if let Some(r) = reach.get_mut(begin) {
                    r.clone_from(&seen);
                }
                seen
            }
            None => paint_tiles(b, p, (cols, rows)),
        };
        if let Some((_, seen)) = open.last_mut() {
            seen.extend_from_slice(&tiles);
        }
        reach.push(tiles);
    }
    let mut lists: Vec<Vec<i32>> = vec![Vec::new(); (cols * rows) as usize];
    for (i, tiles) in reach.iter().enumerate() {
        for t in tiles {
            if let Some(l) = lists.get_mut(*t as usize) {
                l.push(i as i32);
            }
        }
    }
    lists
}
