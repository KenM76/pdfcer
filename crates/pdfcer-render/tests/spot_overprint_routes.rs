//! Spot inks overprinting process colour, overprinting white, and DeviceN
//! with five or six colorants, on every route (`Pass 700.20`, request X075).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::emf::{EmfOptions, export_emf_view};
use pdfcer_render::plates::{Plates, render_plates};
use pdfcer_render::{EdgeAntialiasing, PageBackdrop, RenderOptions};

const PAGE: f32 = 200.0;

/// A buffer ceiling that bands a 200 px page into 50-row bands, the shape a
/// page takes at print resolution.
const BANDED_CAP: usize = 200 * 20 * 50;

/// `(x, y)` page points: the overlap, the top object alone, the bottom alone.
const OVERLAP: (f32, f32) = (100.0, 100.0);
const TOP: (f32, f32) = (100.0, 150.0);
const BOTTOM: (f32, f32) = (50.0, 50.0);

fn obj(num: u32, dict: &str, stream: Option<&[u8]>) -> Vec<u8> {
    let mut out = format!("{num} 0 obj\n").into_bytes();
    match stream {
        None => out.extend_from_slice(dict.as_bytes()),
        Some(data) => {
            out.extend_from_slice(
                format!("<< {dict} /Length {} >>\nstream\n", data.len()).as_bytes(),
            );
            out.extend_from_slice(data);
            out.extend_from_slice(b"\nendstream");
        }
    }
    out.extend_from_slice(b"\nendobj\n");
    out
}

fn assemble(objects: Vec<Vec<u8>>) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for o in &objects {
        offsets.push(buf.len());
        buf.extend_from_slice(o);
    }
    let xref = buf.len();
    let n = objects.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    buf
}

/// Separation SpotA: alternate M = 1.
pub(crate) const SPOT_A: &str = "[/Separation /SpotA /DeviceCMYK << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 1 0 0] /N 1 >>]";
/// Separation SpotB: alternate Y = 1.
const SPOT_B: &str = "[/Separation /SpotB /DeviceCMYK << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0 1 0] /N 1 >>]";
/// Separation SpotK: alternate K = 1, a "spot black".
const SPOT_K: &str = "[/Separation /SpotK /DeviceCMYK << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0 0 1] /N 1 >>]";

/// The objects every fixture shares, numbered from 5: the profile, the
/// shared colour-space and function objects. Returns their bodies.
fn shared_objects(icc: &[u8]) -> Vec<Vec<u8>> {
    // 5 profile, 6 DN5 tint transform, 7 DN6 tint transform,
    // 8 misleading NChannel tint transform (SpotA -> green).
    let dn5 = b"{ 3 index add dup 1 gt { pop 1 } if 4 1 roll 3 -1 roll pop }";
    // c m y k a b -> c, m+a, y+b, k
    let dn6 = b"{ exch 4 index add dup 1 gt { pop 1 } if exch 3 index add dup 1 gt { pop 1 } if 4 2 roll exch pop 4 -1 roll pop }";
    // c a -> c+a, 0, a, 0  (deliberately NOT SpotA's own appearance)
    let green = b"{ dup 3 1 roll add dup 1 gt { pop 1 } if exch 0 exch 0 }";
    vec![
        obj(5, "/N 4", Some(icc)),
        obj(
            6,
            "/FunctionType 4 /Domain [0 1 0 1 0 1 0 1 0 1] /Range [0 1 0 1 0 1 0 1]",
            Some(dn5),
        ),
        obj(
            7,
            "/FunctionType 4 /Domain [0 1 0 1 0 1 0 1 0 1 0 1] /Range [0 1 0 1 0 1 0 1]",
            Some(dn6),
        ),
        obj(
            8,
            "/FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1 0 1]",
            Some(green),
        ),
    ]
}

fn color_spaces() -> String {
    format!(
        "/SpotA {SPOT_A} /SpotB {SPOT_B} /SpotK {SPOT_K} \
         /S4 [/Separation /S4 /DeviceCMYK << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [1 0 0 0] /N 1 >>] \
         /S5 [/Separation /S5 /DeviceCMYK << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 1 1 0] /N 1 >>] \
         /DN5 [/DeviceN [/Cyan /Magenta /Yellow /Black /SpotA] /DeviceCMYK 6 0 R] \
         /DN6 [/DeviceN [/Cyan /Magenta /Yellow /Black /SpotA /SpotB] /DeviceCMYK 7 0 R] \
         /NCh [/DeviceN [/Cyan /SpotA] /DeviceCMYK 8 0 R << /Subtype /NChannel \
              /Colorants << /SpotA {SPOT_A} >> \
              /Process << /ColorSpace /DeviceCMYK /Components [/Cyan /Magenta /Yellow /Black] >> >>] \
         /NChP [/DeviceN [/P1 /SpotA] /DeviceCMYK 8 0 R << /Subtype /NChannel \
              /Colorants << /SpotA {SPOT_A} >> \
              /Process << /ColorSpace /DeviceCMYK /Components [/P1 /P2 /P3 /P4] >> >>]"
    )
}

/// A 200 x 200 pt page with a CMYK output intent, the shared resources and
/// `content`; `extra` are further objects numbered from 9.
pub(crate) fn fixture(content: &str, extra_xobjects: &str, extra: Vec<Vec<u8>>) -> Vec<u8> {
    let icc = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic/icc-rgb/dest-cmyk.icc"),
    )
    .expect("profile");
    let resources = format!(
        "<< /ColorSpace << {} >> /ExtGState << \
           /OP1 << /OP true /op true /OPM 1 >> /OP0 << /OP true /op true /OPM 0 >> \
           /NoOP << /OP false /op false /OPM 1 >> >> \
           /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >> >> \
           /Shading << /Sh 11 0 R >> /XObject << {extra_xobjects} >> >>",
        color_spaces()
    );
    let mut objects = vec![
        obj(
            1,
            "<< /Type /Catalog /Pages 2 0 R /OutputIntents [<< /Type /OutputIntent /S /GTS_PDFX \
             /OutputConditionIdentifier (Synthetic) /DestOutputProfile 5 0 R >>] >>",
            None,
        ),
        obj(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>", None),
        obj(
            3,
            &format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE} {PAGE}] /Contents 4 0 R /Resources {resources} >>"
            ),
            None,
        ),
        obj(4, "", Some(content.as_bytes())),
    ];
    objects.extend(shared_objects(&icc));
    objects.extend(extra);
    assemble(objects)
}

/// Bottom square 20..120, top square 80..180.
pub(crate) const BOTTOM_RECT: &str = "20 20 100 100 re f";
pub(crate) const TOP_RECT: &str = "80 80 100 100 re f";

/// One probe point's process ink and spot tints.
pub(crate) type ProbedInk = ([f32; 4], Vec<(String, f32)>);

/// One measurement: the pixel at each probe point, plus the buffer's ink.
#[derive(Debug, Clone)]
pub(crate) struct Sample {
    pub rgb: [[u8; 3]; 3],
    pub ink: Vec<ProbedInk>,
}

fn device(p: (f32, f32), scale: f32) -> (u32, u32) {
    ((p.0 * scale) as u32, ((PAGE - p.1) * scale) as u32)
}

fn pixel(pm: &tiny_skia::Pixmap, p: (f32, f32), scale: f32) -> [u8; 3] {
    let (x, y) = device(p, scale);
    let c = pm.pixel(x, y).unwrap().demultiply();
    [c.red(), c.green(), c.blue()]
}

pub(crate) fn load(bytes: &[u8]) -> (Document, page_tree::Page) {
    let doc = Document::from_bytes(bytes.to_vec()).expect("fixture parses");
    let page = page_tree::pages(&doc).expect("page tree").remove(0);
    (doc, page)
}

/// The CLI's raster export options (`resolve_render_options` with every
/// setting at its default) and its print options.
pub(crate) fn route_options(print: bool) -> RenderOptions {
    let o = RenderOptions::default().with_backdrop(PageBackdrop::White);
    if print {
        o.with_print_intent(true)
            .with_edge_antialiasing(EdgeAntialiasing::Off)
    } else {
        o
    }
}

pub(crate) fn composite(bytes: &[u8], options: &RenderOptions) -> Sample {
    let (doc, page) = load(bytes);
    let mut rgb = [[0; 3]; 3];
    let mut ink = Vec::new();
    for (i, p) in [OVERLAP, TOP, BOTTOM].into_iter().enumerate() {
        let (x, y) = device(p, 1.0);
        let r = pdfcer_render::render_page_with_view(
            &doc.view(),
            &page,
            1.0,
            &options.clone().with_ink_probe(x, y),
        )
        .expect("renders");
        rgb[i] = pixel(&r.pixmap, p, 1.0);
        let probe = r.diagnostics.ink_probe.expect("probe");
        ink.push((probe.cmyk.unwrap_or([-1.0; 4]), probe.spots));
    }
    Sample { rgb, ink }
}

/// Each plate's grey at the three points, by ink name.
pub(crate) fn plates(bytes: &[u8]) -> Vec<(String, [u8; 3])> {
    let (doc, page) = load(bytes);
    match render_plates(&doc.view(), &page, 1.0, &route_options(false)).expect("plates") {
        Plates::Separated(ps) => ps
            .into_iter()
            .map(|p| {
                let g = [OVERLAP, TOP, BOTTOM].map(|q| pixel(&p.pixmap, q, 1.0)[0]);
                (p.ink, g)
            })
            .collect(),
        other => panic!("not separated: {other:?}"),
    }
}

/// The EMF played back through GDI: the colour at the three points.
#[cfg(windows)]
pub(crate) fn emf(bytes: &[u8], patch: bool, tag: &str) -> [[u8; 3]; 3] {
    let (doc, page) = load(bytes);
    let opts = EmfOptions::default()
        .with_raster_dpi(72.0)
        .with_patch_approximations(patch);
    let out = export_emf_view(&doc.view(), &page, &route_options(false), &opts).expect("emf");
    let dir = crate::export_emf::scratch_dir(&format!("x075-{tag}"));
    let raw = crate::export_emf::play_with_gdi(&dir, tag, &out.emf, PAGE as u32, PAGE as u32);
    let _ = std::fs::remove_dir_all(&dir);
    [OVERLAP, TOP, BOTTOM].map(|p| {
        let (x, y) = device(p, 1.0);
        let i = ((y * PAGE as u32 + x) * 4) as usize;
        [raw[i + 2], raw[i + 1], raw[i]]
    })
}

fn cases() -> Vec<(&'static str, Vec<u8>)> {
    let img = |n: u32, cs: &str, data: &[u8], smask: &str| {
        obj(
            n,
            &format!(
                "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace {cs} /BitsPerComponent 8 {smask}"
            ),
            Some(data),
        )
    };
    let full_mask = obj(
        12,
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8",
        Some(&[255; 4]),
    );
    let spot_img = vec![
        img(9, SPOT_A, &[255; 4], ""),
        img(10, SPOT_A, &[255; 4], "/SMask 12 0 R"),
        full_mask,
    ];
    let mut dn5_px = Vec::new();
    for _ in 0..4 {
        dn5_px.extend_from_slice(&[0, 0, 0, 0, 255]);
    }
    let shading = obj(
        11,
        &format!(
            "<< /ShadingType 2 /ColorSpace {SPOT_A} /Coords [80 0 180 0] \
             /Function << /FunctionType 2 /Domain [0 1] /C0 [1] /C1 [1] /N 1 >> >>"
        ),
        None,
    );
    let cyan = format!("1 0 0 0 k {BOTTOM_RECT}");
    let k1 = format!("0 0 0 1 k {BOTTOM_RECT}");
    let spot_under = format!("/SpotA cs 1 scn {BOTTOM_RECT}");
    let imgx = "/Im 9 0 R /ImS 10 0 R /DnI 13 0 R";
    let dn_img = obj(
        13,
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace [/DeviceN [/Cyan /Magenta /Yellow /Black /SpotA] /DeviceCMYK 6 0 R] /BitsPerComponent 8",
        Some(&dn5_px),
    );
    // `assemble` numbers by position: keep 9..=13 in order.
    let mut ex = spot_img;
    ex.insert(2, shading);
    ex.push(dn_img);
    let f = |c: String| fixture(&c, imgx, ex.clone());
    vec![
        (
            "a fill op1",
            f(format!("{cyan} /OP1 gs /SpotA cs 1 scn {TOP_RECT}")),
        ),
        (
            "a fill op0",
            f(format!("{cyan} /OP0 gs /SpotA cs 1 scn {TOP_RECT}")),
        ),
        (
            "a fill noop",
            f(format!("{cyan} /NoOP gs /SpotA cs 1 scn {TOP_RECT}")),
        ),
        (
            "a cmyk-over-spot op1",
            f(format!("{spot_under} /OP1 gs 1 0 0 0 k {TOP_RECT}")),
        ),
        (
            "a text op1",
            f(format!(
                "{cyan} /OP1 gs /SpotA cs 1 scn BT /F1 200 Tf 75 60 Td (I) Tj ET"
            )),
        ),
        (
            "a image op1",
            f(format!("{cyan} /OP1 gs q 100 0 0 100 80 80 cm /Im Do Q")),
        ),
        (
            "a smask-image op1",
            f(format!("{cyan} /OP1 gs q 100 0 0 100 80 80 cm /ImS Do Q")),
        ),
        (
            "a shading op1",
            f(format!("{cyan} /OP1 gs q 80 80 100 100 re W n /Sh sh Q")),
        ),
        (
            "b spot0 over K op1",
            f(format!("{k1} /OP1 gs /SpotA cs 0 scn {TOP_RECT}")),
        ),
        (
            "b white cmyk over spot op1",
            f(format!("{spot_under} /OP1 gs 0 0 0 0 k {TOP_RECT}")),
        ),
        (
            "b white cmyk over spot op0",
            f(format!("{spot_under} /OP0 gs 0 0 0 0 k {TOP_RECT}")),
        ),
        (
            "c DN5 SpotA only",
            f(format!("/DN5 cs 0 0 0 0 1 scn {TOP_RECT}")),
        ),
        (
            "c DN5 op1 over cyan",
            f(format!("{cyan} /OP1 gs /DN5 cs 0 0 0 0 1 scn {TOP_RECT}")),
        ),
        (
            "c DN6 SpotB only",
            f(format!("/DN6 cs 0 0 0 0 0 1 scn {TOP_RECT}")),
        ),
        (
            "c NCh Colorants SpotA",
            f(format!("/NCh cs 0 1 scn {TOP_RECT}")),
        ),
        (
            "c NChP Process P1",
            f(format!("/NChP cs 1 0 scn {TOP_RECT}")),
        ),
        ("c DN5 image", f("q 100 0 0 100 80 80 cm /DnI Do Q".into())),
        (
            "d 50K over spot op1",
            f(format!("{spot_under} /OP1 gs 0 0 0 0.5 k {TOP_RECT}")),
        ),
        (
            "d 50K over spot op0",
            f(format!("{spot_under} /OP0 gs 0 0 0 0.5 k {TOP_RECT}")),
        ),
        (
            "d spotK over spot op1",
            f(format!("{spot_under} /OP1 gs /SpotK cs 0.5 scn {TOP_RECT}")),
        ),
        (
            "d spotK over spot op0",
            f(format!("{spot_under} /OP0 gs /SpotK cs 0.5 scn {TOP_RECT}")),
        ),
    ]
}

/// Prints every case on every route; run with `--ignored --nocapture`.
#[test]
#[ignore = "measurement harness"]
fn measure_every_route() {
    for (name, bytes) in cases() {
        let c = composite(&bytes, &route_options(false));
        let p = composite(&bytes, &route_options(true));
        println!("== {name}");
        println!("  composite rgb {:?}", c.rgb);
        for (i, (k, s)) in c.ink.iter().enumerate() {
            println!("    ink[{i}] {k:?} {s:?}");
        }
        println!("  print rgb {:?}", p.rgb);
        let banded = composite(
            &bytes,
            &route_options(true).with_max_cmyk_buffer_bytes(Some(BANDED_CAP)),
        );
        println!("  banded rgb {:?}", banded.rgb);
        for (i, (k, s)) in banded.ink.iter().enumerate() {
            println!("    ink[{i}] {k:?} {s:?}");
        }
        let fb = composite(
            &bytes,
            &route_options(true).with_max_cmyk_buffer_bytes(Some(1)),
        );
        println!("  refused rgb {:?}", fb.rgb);
        println!("  plates {:?}", plates(&bytes));
        #[cfg(windows)]
        {
            println!("  emf       {:?}", emf(&bytes, false, "m0"));
            println!("  emf patch {:?}", emf(&bytes, true, "m1"));
        }
    }
}

/// Process and spot appearances on this profile, alone on white paper.
const W: [u8; 3] = [255, 255, 255];
const CY: [u8; 3] = [0, 174, 239];
const MA: [u8; 3] = [237, 2, 140];
const YE: [u8; 3] = [255, 241, 1];
const K100: [u8; 3] = [35, 31, 32];
const K50: [u8; 3] = [147, 149, 152];

/// Two inks laid on one another: §10.8.3's multiplicative simulation.
fn mul(a: [u8; 3], b: [u8; 3]) -> [u8; 3] {
    std::array::from_fn(|i| ((u16::from(a[i]) * u16::from(b[i]) + 127) / 255) as u8)
}

/// What each case must show at the overlap, the top object alone and the
/// bottom object alone, and its non-blank plates as `#` inked / `+` part
/// / `.` blank at the same three points.
fn expected(name: &str) -> ([[u8; 3]; 3], &'static [(&'static str, &'static str)]) {
    const OVER: &[(&str, &str)] = &[("Cyan", "#.#"), ("SpotA", "##.")];
    const UNDER: &[(&str, &str)] = &[("Cyan", "##."), ("SpotA", "#.#")];
    const SPOT_TOP: &[(&str, &str)] = &[("SpotA", "##.")];
    const SPOT_BOTTOM: &[(&str, &str)] = &[("SpotA", "#.#")];
    let over = [mul(CY, MA), MA, CY];
    let k50 = [mul(K50, MA), K50, MA];
    match name {
        "a fill op1" | "a fill op0" | "a text op1" | "a image op1" | "a smask-image op1"
        | "a shading op1" => (over, OVER),
        "a fill noop" => ([MA, MA, CY], &[("Cyan", "..#"), ("SpotA", "##.")]),
        "a cmyk-over-spot op1" => ([mul(CY, MA), CY, MA], UNDER),
        "b spot0 over K op1" => ([K100, W, K100], &[("Black", "#.#")]),
        "b white cmyk over spot op1" | "b white cmyk over spot op0" => ([MA, W, MA], SPOT_BOTTOM),
        "c DN5 SpotA only" | "c NCh Colorants SpotA" | "c DN5 image" => ([MA, MA, W], SPOT_TOP),
        "c DN5 op1 over cyan" => ([MA, MA, CY], &[("Cyan", "..#"), ("SpotA", "##.")]),
        "c DN6 SpotB only" => ([YE, YE, W], &[("SpotB", "##.")]),
        "c NChP Process P1" => ([CY, CY, W], &[("Cyan", "##.")]),
        "d 50K over spot op1" | "d 50K over spot op0" => {
            (k50, &[("Black", "++."), ("SpotA", "#.#")])
        }
        "d spotK over spot op1" | "d spotK over spot op0" => {
            (k50, &[("SpotK", "++."), ("SpotA", "#.#")])
        }
        other => panic!("no expectation for {other}"),
    }
}

/// `got` within 3 levels per channel of `want` at every point.
fn close(got: [[u8; 3]; 3], want: [[u8; 3]; 3]) -> bool {
    got.iter()
        .flatten()
        .zip(want.iter().flatten())
        .all(|(g, w)| g.abs_diff(*w) <= 3)
}

/// Every case on one raster route.
fn check_raster_route(route: &str, options: &RenderOptions) {
    let mut wrong = Vec::new();
    for (name, bytes) in cases() {
        let got = composite(&bytes, options).rgb;
        let (want, _) = expected(name);
        if !close(got, want) {
            wrong.push(format!("{name}: got {got:?}, want {want:?}"));
        }
    }
    assert!(wrong.is_empty(), "{route} route:\n{}", wrong.join("\n"));
}

#[test]
fn the_composite_simulates_every_spot() {
    check_raster_route("composite", &route_options(false));
}

#[test]
fn the_print_route_simulates_every_spot() {
    check_raster_route("print", &route_options(true));
}

/// X075's own shape: a page too large for one colorant buffer.
#[test]
fn a_banded_render_keeps_every_spot_plane() {
    let options = route_options(true).with_max_cmyk_buffer_bytes(Some(BANDED_CAP));
    check_raster_route("banded", &options);
    for (name, bytes) in cases() {
        let (doc, page) = load(&bytes);
        let render = |o: &RenderOptions| {
            pdfcer_render::render_page_with_view(&doc.view(), &page, 1.0, o)
                .expect("renders")
                .diagnostics
        };
        let (d, whole) = (render(&options), render(&route_options(true)));
        assert!(d.cmyk_bands > 1, "{name}: not banded");
        assert_eq!(d.cmyk_spots_flattened, 0, "{name}: {d:?}");
        // A plane present in several bands is still one plane.
        assert_eq!(d.cmyk_spot_planes, whole.cmyk_spot_planes, "{name}");
    }
}

#[test]
fn every_plate_carries_its_own_ink() {
    let mut wrong = Vec::new();
    for (name, bytes) in cases() {
        let (_, want) = expected(name);
        for (ink, grey) in plates(&bytes) {
            let got: String = grey
                .iter()
                .map(|g| match g {
                    0..128 => '#',
                    241.. => '.',
                    _ => '+',
                })
                .collect();
            let want = want
                .iter()
                .find(|(i, _)| *i == ink)
                .map_or("...", |(_, p)| *p);
            if got != want {
                wrong.push(format!("{name} {ink}: got {got}, want {want}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// With patches the EMF must show the raster's answer; without, any
/// difference must be disclosed in its tally.
#[cfg(windows)]
#[test]
fn the_emf_patches_every_spot_and_discloses_without_them() {
    let mut wrong = Vec::new();
    for (i, (name, bytes)) in cases().into_iter().enumerate() {
        let (want, _) = expected(name);
        let got = emf(&bytes, true, &format!("p{i}"));
        if !close(got, want) {
            wrong.push(format!("{name} patched: got {got:?}, want {want:?}"));
        }
        let (doc, page) = load(&bytes);
        let opts = EmfOptions::default().with_raster_dpi(72.0);
        let out = export_emf_view(&doc.view(), &page, &route_options(false), &opts).expect("emf");
        if !close(emf(&bytes, false, &format!("u{i}")), want) && out.outcome.tally.is_exact() {
            wrong.push(format!("{name} unpatched: differs and tally is exact"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Five spot Separations on one page, one more than `MAX_SPOTS`; and a
/// ceiling that holds one row of process colour and no spot plane.
#[test]
fn every_uncarried_spot_is_counted_with_its_reason() {
    let five: String = ["SpotA", "SpotB", "SpotK", "S4", "S5"]
        .map(|s| format!("/{s} cs 1 scn {BOTTOM_RECT} "))
        .concat();
    let (doc, page) = load(&fixture(&five, "", Vec::new()));
    let d = pdfcer_render::render_page_with_view(&doc.view(), &page, 1.0, &route_options(false))
        .expect("renders")
        .diagnostics;
    assert_eq!(d.cmyk_spot_planes, 4, "{d:?}");
    assert_eq!(d.cmyk_spots_flattened_roster, 1, "{d:?}");
    assert_eq!(d.cmyk_spots_flattened, 1, "{d:?}");

    let (_, one) = cases()
        .into_iter()
        .find(|(n, _)| *n == "a fill op1")
        .unwrap();
    let (doc, page) = load(&one);
    let options = route_options(false).with_max_cmyk_buffer_bytes(Some(PAGE as usize * 20));
    let d = pdfcer_render::render_page_with_view(&doc.view(), &page, 1.0, &options)
        .expect("renders")
        .diagnostics;
    assert_eq!(d.cmyk_spots_flattened_ceiling, 1, "{d:?}");
    assert_eq!(d.cmyk_spots_flattened, 1, "{d:?}");
}
