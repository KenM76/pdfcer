//! Measurement harness for [`pdfcer_render::render_page_region`] — the numbers
//! behind `docs/render-region-measurements.md`.
//!
//! ```text
//! cargo run --release -p pdfcer-render --example region_bench -- <file.pdf>
//! ```
//!
//! # Why this exists as a committed example rather than a throwaway script
//!
//! It answers one question that governs whether a tiled viewer is a good idea,
//! and the answer is counter-intuitive enough that it must stay re-runnable:
//! **how much of a region render's cost is resolution- and area-independent?**
//!
//! The `FLOOR` case renders a **1x1 point** region. Whatever that costs is not
//! fill — it is content-stream interpretation and path construction, paid in
//! full no matter how few pixels come out. On a dense CAD sheet it is
//! essentially the entire cost, which is why `render_page_region` buys
//! *reachable zoom and bounded memory* and **not** speed, and why a 3x3 tile
//! ring is several times slower than one region covering the same area.
//!
//! Re-run it before changing anything about the interpreter's per-operator
//! cost, or before deciding a display-list cache is not worth building.
//!
//! # The `PAN` cases — `Pass 75.0`'s acceptance criterion, as a measurement
//!
//! Criterion 1 for the display list is not a property, it is a **number**:
//! second-and-subsequent region renders of an unchanged page must fall from
//! ~700 ms to roughly fill cost. So the criterion is checked here, by the
//! committed harness, rather than asserted in prose.
//!
//! `RECORD` is the one-off interpretation. `PAN` replays the recorded list
//! against a sequence of viewports — which is what a shell's pan gesture
//! actually is, one region per frame — and prints the per-frame cost beside
//! the `REGION` figure it replaces. `MEMORY` is criterion 4.
//!
//! A page the recorder refuses (a shading, an overprint composite, a soft
//! mask) prints `RECORD  refused: <reason>` and skips the `PAN` block. That
//! is not a harness failure: it is the measurement saying this document must
//! use the uncached path, and it should be read as data.
//!
//! `--release` matters: a debug build's ratios are not the shipped ratios.
use std::time::Instant;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_render::{RenderOptions, record_page, render_page_region, render_page_view};

fn main() {
    let path = std::env::args().nth(1).expect("usage: region_bench <pdf>");
    let t = Instant::now();
    let doc = Document::load(std::path::Path::new(&path)).expect("load");
    let pages = page_tree::pages(&doc).expect("pages");
    let page = &pages[0];
    println!("load {:?}  page {:?}", t.elapsed(), page.crop_box);
    let cb = page.crop_box;
    let opts = RenderOptions::default();

    for scale in [1.0f32, 2.0] {
        let t = Instant::now();
        match render_page_view(&doc.view(), page, scale) {
            Ok(r) => println!(
                "FULL  scale {scale:>5}  {}x{} = {:>10} px  {:?}",
                r.pixmap.width(),
                r.pixmap.height(),
                r.pixmap.width() * r.pixmap.height(),
                t.elapsed()
            ),
            Err(e) => println!("FULL  scale {scale:>5}  ERR {e}"),
        }
    }

    // THE FLOOR: a 1x1 pt region. Whatever this costs is resolution- and
    // area-independent -- i.e. it is content-stream interpretation plus path
    // construction, and it is what a display-list cache would remove.
    for _ in 0..2 {
        let tiny = Rect::from_corners(
            cb.llx + 500.0,
            cb.lly + 400.0,
            cb.llx + 501.0,
            cb.lly + 401.0,
        );
        let t = Instant::now();
        match render_page_region(&doc.view(), page, 1.0, tiny, &opts) {
            Ok(r) => println!(
                "FLOOR  1x1pt        {}x{} = {:>10} px  {:?}",
                r.pixmap.width(),
                r.pixmap.height(),
                r.pixmap.width() * r.pixmap.height(),
                t.elapsed()
            ),
            Err(e) => println!("FLOOR  ERR {e}"),
        }
    }

    // A 400x300 pt viewport near the middle, at increasing zoom.
    for scale in [1.0f32, 2.0, 8.0, 32.0] {
        let region = Rect::from_corners(
            cb.llx + (cb.urx - cb.llx) * 0.40,
            cb.lly + (cb.ury - cb.lly) * 0.40,
            cb.llx + (cb.urx - cb.llx) * 0.40 + 400.0 / f64::from(scale),
            cb.lly + (cb.ury - cb.lly) * 0.40 + 300.0 / f64::from(scale),
        );
        let t = Instant::now();
        match render_page_region(&doc.view(), page, scale, region, &opts) {
            Ok(r) => println!(
                "REGION scale {scale:>5}  {}x{} = {:>10} px  {:?}",
                r.pixmap.width(),
                r.pixmap.height(),
                r.pixmap.width() * r.pixmap.height(),
                t.elapsed()
            ),
            Err(e) => println!("REGION scale {scale:>5}  ERR {e}"),
        }
    }

    // ---------------------------------------------------------------- Pass 75.0
    //
    // The pan loop, which is the case the display list exists for: the same
    // page, a different viewport every frame.
    for scale in [1.0f32, 8.0] {
        let t = Instant::now();
        let list = match record_page(&doc.view(), page, scale, 0, &opts) {
            Ok(list) => list,
            Err(e) => {
                println!("RECORD scale {scale:>5}  refused: {e}");
                continue;
            }
        };
        let record_time = t.elapsed();
        println!(
            "RECORD scale {scale:>5}  {:>7} ops  {:>5} clips  {:?}",
            list.op_count(),
            list.clip_count(),
            record_time
        );
        println!(
            "MEMORY scale {scale:>5}  {:>7} KiB held",
            list.memory_bytes() / 1024
        );

        // The FLOOR case, replayed. This is the direct counterpart of the
        // `FLOOR` line above and the cleanest statement of what the display
        // list bought: the same 1x1 pt region that costs ~650 ms to render
        // from the content stream costs THIS to replay. Whatever is left is
        // per-replay overhead -- the op walk and the cull -- and is the only
        // part that still scales with the page rather than with the viewport.
        let tiny = Rect::from_corners(
            cb.llx + 500.0,
            cb.lly + 400.0,
            cb.llx + 501.0,
            cb.lly + 401.0,
        );
        let t = Instant::now();
        match list.replay_region(list.key(), tiny) {
            Ok(r) => println!(
                "PFLOOR scale {scale:>5}  {}x{} = {:>10} px  {:?}",
                r.pixmap.width(),
                r.pixmap.height(),
                r.pixmap.width() * r.pixmap.height(),
                t.elapsed()
            ),
            Err(e) => println!("PFLOOR scale {scale:>5}  ERR {e}"),
        }

        // Eight viewports along a diagonal pan, each 400x300 pt at scale 1
        // and correspondingly smaller as the zoom rises -- so the PIXEL count
        // per frame stays comparable across scales and the number being
        // compared is interpretation, not fill.
        let mut total = std::time::Duration::ZERO;
        let mut frames = 0u32;
        for i in 0..8 {
            let step = f64::from(i) * 20.0;
            let region = Rect::from_corners(
                cb.llx + (cb.urx - cb.llx) * 0.30 + step,
                cb.lly + (cb.ury - cb.lly) * 0.30 + step,
                cb.llx + (cb.urx - cb.llx) * 0.30 + step + 400.0 / f64::from(scale),
                cb.lly + (cb.ury - cb.lly) * 0.30 + step + 300.0 / f64::from(scale),
            );
            let t = Instant::now();
            match list.replay_region(list.key(), region) {
                Ok(_) => {
                    total += t.elapsed();
                    frames += 1;
                }
                // string-gap-exempt: aligned with the FLOOR/REGION/RECORD case labels
                Err(e) => println!("PAN    scale {scale:>5}  frame {i} ERR {e}"),
            }
        }
        if frames > 0 {
            println!(
                // string-gap-exempt: aligned with the FLOOR/REGION/RECORD case labels
                "PAN    scale {scale:>5}  {frames} frames  {:?} total  {:?} per frame",
                total,
                total / frames
            );
        }
    }
}
