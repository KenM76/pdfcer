//! `gpu-raster-measure <pdf> <page> <scale> <out-dir> [bands]`: records the
//! page, times the CPU replay whole and in bands, packs the GPU scene and
//! writes `data.js` and `scene.bin` for `webgl/gpu_raster.html`, which draws it, compares
//! each row's hash with the replay's and times the draw. `EVALUATE=1` also
//! runs the scene's CPU evaluation against the replay; `STATS=1` prints
//! the per-pixel work the shader does; `TILE=n` caps the framebuffer the
//! page is drawn through (default 4096).

use std::time::Instant;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{
    BAND_ROW_LATTICE, GLSL_ES_300_FRAGMENT, GLSL_ES_300_VERTEX, RenderOptions, record_page,
};

fn fnv(bytes: &[u8]) -> u32 {
    let mut h = 0x811c_9dc5u32;
    for b in bytes {
        h = (h ^ u32::from(*b)).wrapping_mul(0x0100_0193);
    }
    h
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pdf = &a[1];
    let page_no: usize = a[2].parse().expect("page index");
    let scale: f32 = a[3].parse().expect("scale");
    let out = &a[4];
    let bands: u32 = a.get(5).map_or(8, |b| b.parse().expect("bands"));
    let tile: u32 = std::env::var("TILE")
        .ok()
        .map_or(4096, |t| t.parse().expect("TILE"));
    let doc = Document::from_bytes(std::fs::read(pdf).expect("pdf")).expect("loads");
    let page = page_tree::pages(&doc).expect("pages").remove(page_no);
    let t = Instant::now();
    let list =
        record_page(&doc.view(), &page, scale, 1, &RenderOptions::default()).expect("records");
    println!("record {:.1} ms", ms(t));
    let t = Instant::now();
    let cpu = list
        .replay_region(list.key(), page.crop_box)
        .expect("replays");
    let whole = ms(t);
    let (w, h) = (cpu.pixmap.width(), cpu.pixmap.height());
    println!("{w}x{h}: CPU replay {whole:.1} ms");
    if list.band_exact().is_ok() {
        let per = (h / bands).div_ceil(BAND_ROW_LATTICE) * BAND_ROW_LATTICE;
        let t = Instant::now();
        std::thread::scope(|s| {
            for i in 0..bands {
                let r0 = (i * per).min(h);
                let r1 = if i + 1 == bands {
                    h
                } else {
                    ((i + 1) * per).min(h)
                };
                if r1 > r0 {
                    let list = &list;
                    s.spawn(move || list.replay_rows(list.key(), r0..r1).ok());
                }
            }
        });
        println!("CPU {bands} bands {:.1} ms", ms(t));
    }
    let t = Instant::now();
    let scene = match list.gpu_scene(list.key(), page.crop_box) {
        Ok(s) => s,
        Err(e) => {
            println!("refused: {e}");
            std::process::exit(2);
        }
    };
    println!(
        "pack {:.1} ms, texture {}x{}",
        ms(t),
        scene.data.width,
        scene.data.height
    );
    if std::env::var_os("STATS").is_some() {
        stats(&scene.data.texels);
    }
    if std::env::var_os("EVALUATE").is_some() {
        let t = Instant::now();
        let same = scene.evaluate() == cpu.pixmap.data();
        println!("CPU evaluate {:.1} ms, equal {same}", ms(t));
    }
    let rows: Vec<String> = cpu
        .pixmap
        .data()
        .chunks(w as usize * 4)
        .map(|r| fnv(r).to_string())
        .collect();
    let texels: Vec<u8> = scene
        .data
        .texels
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let js = format!(
        "const D = {{w:{w}, h:{h}, tw:{}, th:{}, tl:{}, tile:{tile}, cpu_ms:{whole:.1}}};\nD.vs = {:?};\nD.fs = {:?};\nD.rows = [{}];\n",
        scene.data.width,
        scene.data.height,
        scene.data.layers,
        GLSL_ES_300_VERTEX,
        GLSL_ES_300_FRAGMENT,
        rows.join(","),
    );
    std::fs::write(format!("{out}/data.js"), js).expect("writes data.js");
    std::fs::write(format!("{out}/scene.bin"), texels).expect("writes scene.bin");
}

/// Per-pixel work the shader does, read back out of the scene texture: per
/// raster tile, the paints listed, the fill edges walked, and the pairs a
/// walk of every edge against every other would take; plus the clip
/// chains' edges.
fn stats(t: &[i32]) {
    let at = |i: usize| -> [i32; 4] {
        let k = i * 4;
        [t[k], t[k + 1], t[k + 2], t[k + 3]]
    };
    let lane = |e: usize| t[e];
    let [w, h, cols, rows] = at(0);
    let [tile_base, op_base, clip_base, _] = at(1);
    let dcols = (w + 8190) / 8191;
    let stride = (dcols * ((h + 8190) / 8191)) as usize;
    // Edges a fill walks in raster tile (c, r).
    let fill_edges = |hd: i32, c: i32, r: i32| -> i64 {
        if hd < 0 {
            return 0;
        }
        let hd = hd as usize;
        let o = at(hd + 3);
        let tiles = at(hd + 2);
        let rws = at(hd + 1);
        // Raster tile (c, r) in this fill's DrawTiler tile pixels.
        let cc = ((c * 16 - o[0]) >> 4) - tiles[0];
        let rr = ((r * 4 - o[1]) >> 2) - tiles[1];
        if cc < 0 || rr < 0 || cc >= tiles[2] || rr >= tiles[3] {
            return 0;
        }
        let e = at(hd + rws[3] as usize + (rr * tiles[2] + cc) as usize);
        i64::from(e[1])
    };
    // Per pixel of tile (c, r): edges live on its row, and of those, edges
    // at or left of its last sample column.
    let fill_live = |hd: i32, c: i32, r: i32| -> (i64, i64) {
        if hd < 0 {
            return (0, 0);
        }
        let hd = hd as usize;
        let o = at(hd + 3);
        let tiles = at(hd + 2);
        let rws = at(hd + 1);
        let cc = ((c * 16 - o[0]) >> 4) - tiles[0];
        let rr = ((r * 4 - o[1]) >> 2) - tiles[1];
        if cc < 0 || rr < 0 || cc >= tiles[2] || rr >= tiles[3] {
            return (0, 0);
        }
        let e = at(hd + rws[3] as usize + (rr * tiles[2] + cc) as usize);
        let sh = o[2];
        let (mut live, mut left) = (0i64, 0i64);
        for pr in 0..4 {
            let py = r * 4 + pr - o[1];
            let (y0, y1) = if sh == 0 {
                (py, py)
            } else {
                (py << 2, (py << 2) + 3)
            };
            for pc in 0..16 {
                let px = c * 16 + pc - o[0];
                let c3 = if sh == 0 { px } else { (px << 2) + 3 };
                for i in e[0]..e[0] + e[1] {
                    let ed = at(hd + i as usize);
                    let (fy, ly) = (ed[2], ed[3] >> 1);
                    if fy <= y1 && ly >= y0 {
                        live += 1;
                        let x = (ed[0] + ed[1] * (y0.max(fy) - fy) + 0x8000) >> 16;
                        if x <= c3 {
                            left += 1;
                        }
                    }
                }
            }
        }
        (live, left)
    };
    let (mut paints, mut edges, mut pairs, mut clip_edges, mut max_edges) =
        (0i64, 0i64, 0i64, 0i64, 0i64);
    let mut worst = (0i64, 0, 0);
    let ntiles = (cols * rows) as usize;
    let (mut live_sum, mut left_sum, mut sampled) = (0i64, 0i64, 0i64);
    for ti in 0..ntiles {
        let (c, r) = ((ti % cols as usize) as i32, (ti / cols as usize) as i32);
        let tile = at(tile_base as usize + ti);
        let sample = ti % 7 == 0;
        if sample {
            sampled += 1;
        }
        let mut here = 0i64;
        for e in tile[0]..tile[0] + tile[1] {
            let op = at(op_base as usize + lane(e as usize) as usize);
            if op[2] == 0x00FF_FFFE || op[2] == 0x00FF_FFFD {
                continue;
            }
            paints += 1;
            let n = fill_edges(op[0], c, r);
            if sample {
                let (l, f) = fill_live(op[0], c, r);
                live_sum += l;
                left_sum += f;
            }
            edges += n;
            pairs += n * n;
            here += n;
            max_edges = max_edges.max(n);
            let mut at_ = op[1];
            let mut guard = 0;
            while at_ >= 0 && guard < 64 {
                let k = at(clip_base as usize + at_ as usize * stride);
                let m = fill_edges(k[0], c, r);
                clip_edges += m;
                here += m;
                at_ = k[1];
                guard += 1;
            }
        }
        if here > worst.0 {
            worst = (here, c, r);
        }
    }
    let px = 64.0 * ntiles as f64;
    println!(
        "sampled {sampled} tiles: edges live on the pixel's row/px {:.1}, live and left of it/px {:.1}",
        live_sum as f64 / (64.0 * sampled as f64),
        left_sum as f64 / (64.0 * sampled as f64)
    );
    println!(
        "tiles {ntiles}: paints/tile {:.1}, fill edges/px {:.1}, clip edges/px {:.1}, split pairs/tile {:.0}, max edges in one fill tile {max_edges}, worst tile {} edges at ({}, {})",
        paints as f64 / ntiles as f64,
        edges as f64 * 64.0 / px,
        clip_edges as f64 * 64.0 / px,
        pairs as f64 / ntiles as f64,
        worst.0,
        worst.1,
        worst.2
    );
}
