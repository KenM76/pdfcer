use super::levels::{CANADA_OHS, LevelLimit, LuxSchedule};
use super::*;
use crate::{Camera, IlluminanceMap, IlluminanceUnit, MapPatch, Projection, RenderOptions};

fn band(from: f64, colour: [u8; 3]) -> LuxBand {
    LuxBand { from, colour }
}

fn two(blend: BandBlend) -> LuxBands {
    LuxBands::new(
        vec![band(0.0, [0, 0, 0]), band(100.0, [200, 100, 50])],
        blend,
    )
    .unwrap()
}

#[test]
fn a_band_table_must_rise() {
    assert_eq!(
        LuxBands::new(Vec::new(), BandBlend::Stepped),
        Err(LuxBandsError::Empty)
    );
    let flat = vec![band(5.0, [0; 3]), band(5.0, [0; 3])];
    assert_eq!(
        LuxBands::new(flat, BandBlend::Stepped),
        Err(LuxBandsError::NotAscending(1))
    );
    let nan = vec![band(f64::NAN, [0; 3])];
    assert_eq!(
        LuxBands::new(nan, BandBlend::Stepped),
        Err(LuxBandsError::NotAscending(0))
    );
    let many: Vec<LuxBand> = (0..17).map(|i| band(f64::from(i), [0; 3])).collect();
    assert_eq!(
        LuxBands::new(many, BandBlend::Stepped),
        Err(LuxBandsError::TooMany {
            count: 17,
            limit: 16
        })
    );
}

#[test]
fn a_figure_takes_its_band_or_a_mix_of_two() {
    let stepped = two(BandBlend::Stepped);
    assert_eq!(stepped.colour(-5.0), [0, 0, 0]);
    assert_eq!(stepped.colour(f64::NAN), [0, 0, 0]);
    assert_eq!(stepped.colour(99.9), [0, 0, 0]);
    assert_eq!(stepped.colour(100.0), [200, 100, 50]);
    assert_eq!(stepped.colour(1e9), [200, 100, 50]);
    let graded = two(BandBlend::Graded);
    assert_eq!(graded.colour(50.0), [100, 50, 25]);
    assert_eq!(graded.colour(500.0), [200, 100, 50]);
    let u = graded.uniforms();
    assert_eq!((u.count, u.graded, u.from[1]), (2, true, 100.0));
    assert_eq!(u.colour[1], [200.0 / 255.0, 100.0 / 255.0, 50.0 / 255.0]);
    let decl = format!("u_band_from[{MAX_LUX_BANDS}]");
    assert!(GLSL_ES_300_LIGHT_MAP_FRAGMENT.contains(&decl));
}

#[test]
fn the_canada_steps_are_the_schedules_levels() {
    assert_eq!(CANADA_OHS.len(), 61);
    for l in CANADA_OHS {
        let screen = l.schedule == LuxSchedule::ScreenWork;
        assert_eq!(l.limit == LevelLimit::Maximum, screen, "{l:?}");
    }
    let mut levels: Vec<f64> = CANADA_OHS.iter().map(|l| l.lux).collect();
    levels.push(0.0);
    levels.sort_by(f64::total_cmp);
    levels.dedup();
    let steps = LuxBands::canada_ohs_steps(BandBlend::Stepped);
    let bounds: Vec<f64> = steps.bands().iter().map(|b| b.from).collect();
    assert_eq!(bounds, levels);
    assert!(LuxBands::new(steps.bands().to_vec(), BandBlend::Graded).is_ok());
}

/// A 2 by 2 floor seen from straight above, its two triangles' corners at
/// `figures`.
fn floor_map(figures: [[f64; 3]; 2]) -> (IlluminanceMap, Camera) {
    let [a, b, c, d] = [
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
    ];
    let patch = |corners, f: [f64; 3]| {
        Some(MapPatch {
            corners,
            divisions: 1,
            figures: f.to_vec(),
        })
    };
    let map = IlluminanceMap {
        unit: IlluminanceUnit::Lux,
        reasons: Vec::new(),
        notes: Vec::new(),
        meshes: vec![vec![
            patch([a, b, c], figures[0]),
            patch([a, c, d], figures[1]),
        ]],
    };
    let camera = Camera {
        eye: [0.0, 0.0, 5.0],
        target: [0.0; 3],
        up: [0.0, 1.0, 0.0],
        projection: Projection::Perspective { fov_y: 40.0 },
    };
    (map, camera)
}

fn options() -> RenderOptions {
    RenderOptions {
        width: 64,
        height: 64,
        background: [9, 9, 9, 255],
        ..RenderOptions::default()
    }
}

fn pixel(img: &crate::Image, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * img.width + x) * 4) as usize;
    [
        img.rgba[i],
        img.rgba[i + 1],
        img.rgba[i + 2],
        img.rgba[i + 3],
    ]
}

#[test]
fn a_light_map_draws_each_surface_in_its_band() {
    let bands = two(BandBlend::Stepped);
    let (map, camera) = floor_map([[120.0; 3]; 2]);
    let img = render_light_map(&map, &bands, &camera, &options()).unwrap();
    assert_eq!(pixel(&img, 32, 32), [200, 100, 50, 255]);
    assert_eq!(pixel(&img, 0, 0), [9, 9, 9, 255]);
    let (map, camera) = floor_map([[20.0; 3]; 2]);
    let img = render_light_map(&map, &bands, &camera, &options()).unwrap();
    assert_eq!(pixel(&img, 32, 32), [0, 0, 0, 255]);
}

#[test]
fn a_graded_light_map_runs_between_its_corners() {
    // Figure 0 along the left edge, 100 along the right: the middle reads 50.
    let left_right = [[0.0, 100.0, 100.0], [0.0, 100.0, 0.0]];
    let (map, camera) = floor_map(left_right);
    let graded = two(BandBlend::Graded);
    let img = render_light_map(&map, &graded, &camera, &options()).unwrap();
    let [r, g, b, _] = pixel(&img, 32, 20);
    assert!(
        r.abs_diff(100) <= 6 && g.abs_diff(50) <= 3 && b.abs_diff(25) <= 2,
        "{r} {g} {b}"
    );
    let [right, ..] = pixel(&img, 40, 20);
    assert!(right > r, "{right} {r}");
}

#[test]
fn a_table_spreads_its_colours_evenly_over_a_range() {
    let steps = LuxBands::canada_ohs_steps(BandBlend::Graded);
    let spread = steps.spread(0.4, 1.8);
    assert_eq!(spread.blend(), BandBlend::Graded);
    assert_eq!(spread.bands().len(), steps.bands().len());
    for (i, (s, b)) in spread.bands().iter().zip(steps.bands()).enumerate() {
        assert_eq!(s.colour, b.colour);
        assert!(
            (s.from - (0.4 + 1.4 * i as f64 / 14.0)).abs() < 1e-12,
            "{i}"
        );
    }
    let last = spread.bands().last().unwrap();
    assert_eq!(spread.colour(1.8), last.colour);
    for (lo, hi) in [
        (2.0, 2.0),
        (3.0, 1.0),
        (f64::NAN, f64::INFINITY),
        (1e300, 1e300),
    ] {
        let s = steps.spread(lo, hi);
        assert!(
            LuxBands::new(s.bands().to_vec(), s.blend()).is_ok(),
            "{lo} {hi}"
        );
    }
}

#[test]
fn a_map_takes_the_steps_in_lux_and_its_own_range_when_relative() {
    let (mut map, _) = floor_map([[0.4, 1.0, 1.934], [0.4, 1.934, 0.7]]);
    let steps = LuxBands::canada_ohs_steps(BandBlend::Stepped);
    assert_eq!(LuxBands::for_map(&map, BandBlend::Stepped), steps);
    map.unit = IlluminanceUnit::Relative;
    let relative = LuxBands::for_map(&map, BandBlend::Stepped);
    assert_eq!(relative, steps.spread(0.4, 1.934));
    let colours: std::collections::HashSet<_> = [0.4, 0.7, 1.0, 1.934]
        .iter()
        .map(|&f| relative.colour(f))
        .collect();
    assert_eq!(colours.len(), 4, "a relative map is not one colour");
    map.meshes.clear();
    assert_eq!(
        LuxBands::for_map(&map, BandBlend::Stepped),
        steps.spread(0.0, 1.0)
    );
}
