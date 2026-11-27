//! Cancel-latency probe: how long a render takes to return after its
//! [`RenderCancel`] is set (request G337).
//!
//! ```text
//! cargo run --release -p pdfcer-render --example cancel_latency -- <file.pdf> [delay_ms] [scale]
//! ```
//!
//! Renders page 1 once uncancelled (the baseline), then again on a worker
//! thread whose token is set `delay_ms` (default 100) after the render
//! starts, for both the direct raster path and the display-list recorder.
//! `LATENCY` is the time from `cancel()` to the worker returning, which is
//! what a shell joining its render worker waits for.

use std::time::{Duration, Instant};

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::cancel::RenderCancel;
use pdfcer_render::{RenderOptions, record_page, render_page_with_view};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: cancel_latency <pdf> [delay_ms] [scale]");
    let delay: u64 = args.next().map_or(100, |s| s.parse().expect("delay_ms"));
    let scale: f32 = args.next().map_or(1.0, |s| s.parse().expect("scale"));
    let doc = Document::load(std::path::Path::new(&path)).expect("load");
    let pages = page_tree::pages(&doc).expect("pages");
    let page = &pages[0];

    let t = Instant::now();
    let full = render_page_with_view(&doc.view(), page, scale, &RenderOptions::default());
    println!("BASELINE  render  {:?}  ok={}", t.elapsed(), full.is_ok());

    for route in ["render", "record"] {
        let token = RenderCancel::new();
        let options = RenderOptions::default().with_cancel(token.clone());
        std::thread::scope(|s| {
            let start = Instant::now();
            let worker = s.spawn(|| match route {
                "render" => render_page_with_view(&doc.view(), page, scale, &options)
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                _ => record_page(&doc.view(), page, scale, 0, &options)
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            });
            std::thread::sleep(Duration::from_millis(delay));
            let set = Instant::now();
            token.cancel();
            let outcome = worker.join().expect("worker panicked");
            println!(
                "{route:<8}  cancel at {:?}  LATENCY {:?}  total {:?}  outcome {:?}",
                set - start,
                set.elapsed(),
                start.elapsed(),
                outcome
            );
        });
    }
}
