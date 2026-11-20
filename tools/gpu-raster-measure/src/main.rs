//! `gpu-raster-measure <pdf> <page> <scale> <out-dir> [bands]`: records the
//! page, times the CPU replay whole and in bands, packs the GPU scene and
//! writes `data.js` and `scene.bin` for `webgl/gpu_raster.html`, which draws it, compares
//! each row's hash with the replay's and times the draw. `EVALUATE=1` also
//! runs the scene's CPU evaluation against the replay; `TILE=n` caps the
//! framebuffer the page is drawn through (default 4096).

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
