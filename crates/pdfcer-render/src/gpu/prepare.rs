//! Every clip's and paint's edges, tiles and colours, computed on every
//! core: the per-item work of [`super::scene`]'s packing is independent, so
//! it is split across threads and gathered back in display-list order.

use std::sync::atomic::{AtomicUsize, Ordering};

use tiny_skia::FillRule;

use super::bins;
use super::block::{self, FillBlock};
use super::fill_edges::FillEdges;
use super::image::{self, ImagePaint};
use super::scene::{Frame, GpuRefusal, colors};
use crate::canvas::Brush;
use crate::device_clip;
use crate::display_list::{ClipDef, Op};

/// Ops a worker takes at a time.
const BLOCK: usize = 512;

/// One paint ready to lay out.
pub(crate) struct Prepared {
    pub block: FillBlock,
    pub clip: Option<usize>,
    pub color: i32,
    pub memset: i32,
    pub image: Option<ImagePaint>,
}

/// One clip ready to lay out: its fill in each `DrawTiler` tile, where it
/// covers anything, and its parent.
pub(crate) type PreparedClip = (Vec<Option<FillBlock>>, Option<usize>);

/// `ops` with every text run's ops in its place.
fn flatten<'a>(ops: &'a [Op], out: &mut Vec<&'a Op>) {
    for op in ops {
        match op {
            Op::Text { ops, .. } => flatten(ops, out),
            _ => out.push(op),
        }
    }
}

/// Each tile's edges laid out, at that tile's origin.
fn fill_blocks(frame: &Frame<'_>, edges: Vec<Option<FillEdges>>) -> Vec<Option<FillBlock>> {
    let tiles = frame.draw_tiles();
    edges
        .into_iter()
        .zip(tiles)
        .map(|(e, (x, y, _, _))| e.map(|e| block::build(&e, bins::build(&e), (x as i32, y as i32))))
        .collect()
}

/// Each tile's image sampler for `brush`, `None` throughout for a solid.
///
/// # Errors
///
/// [`GpuRefusal::Image`] where a sampler cannot be reproduced exactly over
/// its tile's fill.
fn samplers(
    frame: &Frame<'_>,
    brush: &Brush,
    ts: tiny_skia::Transform,
    edges: &[Option<FillEdges>],
) -> Result<Option<Vec<Option<ImagePaint>>>, GpuRefusal> {
    let Brush::Image {
        texels,
        quality,
        transform,
    } = brush
    else {
        return Ok(None);
    };
    let v = image::per_tile(frame, texels, *quality, *transform, ts)?;
    for (e, i) in edges.iter().zip(&v) {
        if let (Some(e), Some(i)) = (e, i)
            && !i.in_range(e.area)
        {
            return Err(GpuRefusal::Image);
        }
    }
    Ok(Some(v))
}

/// `op` packed into `out`, one paint per `DrawTiler` tile it draws in.
fn prepare(frame: &Frame<'_>, op: &Op, out: &mut Vec<Prepared>) -> Result<(), GpuRefusal> {
    let (brush, clip, bounds, ts, edges) = match op {
        Op::Fill {
            path,
            brush,
            rule,
            ctm,
            clip,
            bounds,
        } => {
            let ts = frame.to_region(*ctm);
            let e = frame.edges(path, *rule, ts, brush.anti_alias);
            (brush, clip, bounds, ts, e)
        }
        Op::Stroke {
            path,
            brush,
            stroke,
            ctm,
            clip,
            bounds,
        } => {
            let ts = frame.to_region(*ctm);
            let e = match device_clip::stroke_outline(path, stroke, ts) {
                Some(o) => frame.edges(&o, FillRule::Winding, ts, brush.anti_alias),
                None => Ok(Vec::new()),
            };
            (brush, clip, bounds, ts, e)
        }
        Op::Text { .. } => return Ok(()),
        Op::Layer { .. } => return Err(GpuRefusal::Transparency),
    };
    if bounds.is_some_and(|b| !b.intersects(frame.bounds)) {
        return Ok(());
    }
    let (color, memset) = colors(brush)?;
    let edges = edges?;
    let images = samplers(frame, &brush.brush, ts, &edges)?;
    for (k, block) in fill_blocks(frame, edges).into_iter().enumerate() {
        let image = match images.as_ref().map(|v| v.get(k).cloned().flatten()) {
            // An uninvertible pattern draws nothing in its tile.
            Some(None) => continue,
            Some(i) => i,
            None => None,
        };
        let Some(block) = block else { continue };
        out.push(Prepared {
            block,
            clip: clip.map(|c| c.index()),
            color,
            memset,
            image,
        });
    }
    Ok(())
}

fn prepare_clip(frame: &Frame<'_>, def: &ClipDef) -> Result<PreparedClip, GpuRefusal> {
    let Some(path) = def.path.as_deref() else {
        return Ok(((0..frame.draw_tiles().len()).map(|_| None).collect(), None));
    };
    let edges = frame.edges(path, def.rule, frame.to_region(def.ctm), true)?;
    Ok((fill_blocks(frame, edges), def.parent.map(|p| p.index())))
}

/// A unit of work, in the order results are gathered.
enum Job<'a> {
    Clip(&'a ClipDef),
    Ops(&'a [&'a Op]),
}

enum Done {
    Clip(PreparedClip),
    Paints(Vec<Prepared>),
}

/// `job` under `frame`'s replay state, stopping at the first refusal.
fn run(frame: &Frame<'_>, job: &Job<'_>) -> Result<Done, GpuRefusal> {
    device_clip::with_lattice_anchor(frame.anchor, || {
        device_clip::with_row_origin(0, || match job {
            Job::Clip(def) => prepare_clip(frame, def).map(Done::Clip),
            Job::Ops(ops) => {
                let mut v = Vec::with_capacity(ops.len());
                for op in *ops {
                    prepare(frame, op, &mut v)?;
                }
                Ok(Done::Paints(v))
            }
        })
    })
}

/// `f` over `0..n` on every core; results in index order.
pub(crate) fn ordered<R: Send>(n: usize, f: impl Fn(usize) -> R + Sync) -> Vec<R> {
    let threads = workers().min(n);
    if threads <= 1 {
        return (0..n).map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let mut done: Vec<(usize, R)> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= n {
                            break;
                        }
                        mine.push((i, f(i)));
                    }
                    mine
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    });
    done.sort_unstable_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, r)| r).collect()
}

/// Every clip of `frame`, then every paint of its ops, in order; or the
/// first refusal in that order.
pub(crate) fn all(frame: &Frame<'_>) -> Result<(Vec<PreparedClip>, Vec<Prepared>), GpuRefusal> {
    let mut flat = Vec::new();
    flatten(frame.ops, &mut flat);
    let jobs: Vec<Job<'_>> = frame
        .clips
        .iter()
        .map(Job::Clip)
        .chain(flat.chunks(BLOCK).map(Job::Ops))
        .collect();
    let (mut clips, mut paints) = (Vec::with_capacity(frame.clips.len()), Vec::new());
    for done in ordered(jobs.len(), |i| jobs.get(i).map(|j| run(frame, j))) {
        match done {
            Some(Ok(Done::Clip(c))) => clips.push(c),
            Some(Ok(Done::Paints(p))) => paints.extend(p),
            Some(Err(e)) => return Err(e),
            None => {}
        }
    }
    Ok((clips, paints))
}

/// `fills` copied end to end into `dst`, which holds exactly their texels,
/// on every core. Freeing them is a large share of packing, so off wasm32
/// it happens on a detached thread, after the scene is returned.
pub(crate) fn copy_fills(dst: &mut [[i32; 4]], fills: Vec<FillBlock>) {
    let per = fills.len().div_ceil(workers()).max(1);
    let mut it = fills.into_iter();
    let spent = std::sync::Mutex::new(Vec::new());
    let sink = &spent;
    std::thread::scope(|s| {
        let mut rest = dst;
        loop {
            let group: Vec<FillBlock> = it.by_ref().take(per).collect();
            if group.is_empty() {
                break;
            }
            let n = group
                .iter()
                .map(|f| f.texels.len())
                .sum::<usize>()
                .min(rest.len());
            let (mut head, tail) = std::mem::take(&mut rest).split_at_mut(n);
            rest = tail;
            let copy = move || {
                for f in &group {
                    let k = f.texels.len().min(head.len());
                    let (to, after) = std::mem::take(&mut head).split_at_mut(k);
                    to.copy_from_slice(f.texels.get(..k).unwrap_or_default());
                    head = after;
                }
                if let Ok(mut v) = sink.lock() {
                    v.push(group);
                }
            };
            if workers() > 1 {
                s.spawn(copy);
            } else {
                copy();
            }
        }
    });
    let spent = spent.into_inner().unwrap_or_default();
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(move || drop(spent));
    #[cfg(target_arch = "wasm32")]
    drop(spent);
}

#[cfg(not(target_arch = "wasm32"))]
fn workers() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

/// `wasm32-unknown-unknown` has no threads.
#[cfg(target_arch = "wasm32")]
fn workers() -> usize {
    1
}
