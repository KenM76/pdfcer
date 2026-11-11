//! `--bands` — the cost split behind decision 223 (Pass 704.0).
//!
//! Per scale: record once, replay the whole page, PNG-encode it, then
//! replay the same list as N integer-row bands on N host threads and
//! stitch them. Every stitched picture is checked byte-identical to the
//! whole replay; a mismatch is reported, never averaged away. Threads
//! live here, in the tool — the engine itself stays thread-free.
//!
//! Times are the best of `--repeat` runs, wall clock. A `--ablate` set
//! applies to every replay below, so the record/replay split can be
//! taken with clip, fill or blend work suppressed.

use std::time::Instant;

use pdfcer_core::page_tree::Page;
use pdfcer_core::view::DocumentView;
use pdfcer_render::{DisplayList, RenderOptions, record_page};

/// Run the band table for `scales` × `threads` and print it.
pub fn run(
    view: &DocumentView<'_>,
    page: &Page,
    scales: &[f32],
    threads: &[usize],
    repeat: usize,
) -> bool {
    let opts = RenderOptions::default();
    let mut all_exact = true;
    println!();
    println!("scale    MP  record ms  replay ms  png ms  list MB  | bands: wall ms (speed-up)"); // string-gap-exempt: column-aligned table header
    for &scale in scales {
        let Some((list, record)) =
            best_of(repeat, || record_page(view, page, scale, 1, &opts).ok())
        else {
            println!("{scale:>5}  recording refused");
            continue;
        };
        let Some((whole, replay)) = best_of(repeat, || {
            list.replay_region(list.key(), page.crop_box).ok()
        }) else {
            println!("{scale:>5}  replay refused");
            continue;
        };
        let png = best_of(repeat, || whole.pixmap.encode_png().ok()).map_or(f64::NAN, |(_, t)| t);
        let (w, h) = (whole.pixmap.width(), whole.pixmap.height());
        let mut line = format!(
            "{scale:>5} {:>5.1} {:>10.1} {:>10.1} {:>7.1} {:>8.1}  |",
            f64::from(w) * f64::from(h) / 1e6,
            record * 1e3,
            replay * 1e3,
            png * 1e3,
            list.memory_bytes() as f64 / 1e6,
        );
        for &n in threads {
            match best_of(repeat, || banded(&list, h, n)) {
                Some((stitched, t)) => {
                    let (bytes, max) = diff(&stitched, whole.pixmap.data());
                    // Only a band-exact list promises a byte-identical stitch.
                    all_exact &= bytes == 0 || list.band_exact().is_err();
                    line.push_str(&format!(" {n}:{:.1} ({:.2}x)", t * 1e3, replay / t));
                    if bytes > 0 {
                        line.push_str(&format!(" MISMATCH {bytes}B max {max}"));
                        if std::env::var_os("BANDS_WHERE").is_some() {
                            report_where(&stitched, whole.pixmap.data(), w, h, n);
                        }
                    }
                }
                None => line.push_str(&format!(" {n}:refused")),
            }
        }
        if let Err(why) = list.band_exact() {
            line.push_str(&format!("  [not band-exact: {why}]"));
        }
        println!("{line}");
    }
    println!();
    println!(
        "{}",
        if all_exact {
            "stitch    : every banded picture is byte-identical to the whole replay"
        } else {
            "stitch    : MISMATCH — a banded picture differs from the whole replay"
        }
    );
    all_exact
}

/// Replay `list` as `n` row bands, one host thread each, and stitch them.
fn banded(list: &DisplayList, rows: u32, n: usize) -> Option<Vec<u8>> {
    let n = u32::try_from(n).ok()?.clamp(1, rows.max(1));
    let parts: Vec<Option<Vec<u8>>> = std::thread::scope(|sc| {
        let handles: Vec<_> = (0..n)
            .map(|i| {
                let cut = |k: u32| {
                    let lat = pdfcer_render::BAND_ROW_LATTICE;
                    if k == n {
                        rows
                    } else {
                        (rows * k / n / lat * lat).min(rows)
                    }
                };
                let band = cut(i)..cut(i + 1);
                sc.spawn(move || {
                    if band.is_empty() {
                        return Some(Vec::new());
                    }
                    let page = list.replay_rows(list.key(), band).ok()?;
                    Some(page.pixmap.data().to_vec())
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().ok().flatten())
            .collect()
    });
    let mut out = Vec::new();
    for p in parts {
        out.extend_from_slice(&p?);
    }
    Some(out)
}

/// Differing byte count and the largest per-byte delta, in levels.
fn diff(a: &[u8], b: &[u8]) -> (usize, u8) {
    if a.len() != b.len() {
        return (a.len().max(b.len()), u8::MAX);
    }
    a.iter()
        .zip(b)
        .filter(|(x, y)| x != y)
        .fold((0, 0), |(n, m), (x, y)| (n + 1, m.max(x.abs_diff(*y))))
}

/// Diagnostic: the 12 largest differing pixels, with band row offsets.
fn report_where(a: &[u8], b: &[u8], w: u32, h: u32, n: usize) {
    let mut px: Vec<(u8, u32, u32)> = (0..(w * h) as usize)
        .filter_map(|i| {
            let d = (0..4).map(|c| a[i * 4 + c].abs_diff(b[i * 4 + c])).max()?;
            (d > 0).then_some((d, i as u32 % w, i as u32 / w))
        })
        .collect();
    px.sort_unstable_by(|x, y| y.cmp(x));
    let n = n as u32;
    let seams: Vec<u32> = (1..n).map(|i| h * i / n).collect();
    eprintln!("  n={n} seams at rows {seams:?}");
    if let Some(dir) = std::env::var_os("BANDS_DUMP") {
        let dir = std::path::PathBuf::from(dir);
        let _ = std::fs::write(dir.join(format!("banded-{n}.rgba")), a);
        let _ = std::fs::write(dir.join("whole.rgba"), b);
        eprintln!("  dumped {w}x{h} RGBA to {}", dir.display());
    }
    for &(d, x, y) in px.iter().take(12) {
        let i = ((y * w + x) * 4) as usize;
        let near = seams
            .iter()
            .map(|s| y as i64 - *s as i64)
            .min_by_key(|v| v.abs());
        eprintln!(
            "  ({x},{y}) d={d} band={:?} whole={:?} seam_off={near:?}",
            &a[i..i + 4],
            &b[i..i + 4]
        );
    }
}

/// The fastest of `repeat` runs of `f`, with its last result.
fn best_of<T>(repeat: usize, mut f: impl FnMut() -> Option<T>) -> Option<(T, f64)> {
    let mut best = f64::MAX;
    let mut last = None;
    for _ in 0..repeat.max(1) {
        let t = Instant::now();
        let v = f()?;
        best = best.min(t.elapsed().as_secs_f64());
        last = Some(v);
    }
    last.map(|v| (v, best))
}
