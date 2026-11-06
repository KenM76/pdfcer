//! Frame-level tests of [`crate::Projection::Fisheye`]: an eye inside a
//! room of six coloured walls, so every direction meets a wall.

#![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::panic)] // Tests fail loudly by design.

use crate::camera::lens::{Layout, Lens};
use crate::render::raster::Paint;
use crate::render::render_finished;
use crate::vec3::{dot, length, sub};
use crate::{
    AssembledModel, Bounds, Camera, FisheyeMapping, FrameLens, LightingScheme, ModelLine,
    Projection, RenderError, RenderMode, RenderModeKind, RenderOptions, TriangleMesh, pick,
    render_model,
};

const EQUI: FisheyeMapping = FisheyeMapping::Equidistant;
const STEREO: FisheyeMapping = FisheyeMapping::Stereographic;
pub(super) const BACKGROUND: [u8; 4] = [1, 2, 3, 255];
pub(super) const WALLS: [[u8; 4]; 6] = [
    [200, 0, 0, 255],
    [0, 200, 0, 255],
    [0, 0, 200, 255],
    [200, 200, 0, 255],
    [0, 200, 200, 255],
    [200, 0, 200, 255],
];

/// The wall of the room |p| ≤ 5 at coordinate `axis` = `sign` · 5.
pub(super) fn wall(axis: usize, sign: f64) -> TriangleMesh {
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let corner = |a: f64, b: f64| {
        let mut p = [0.0; 3];
        p[axis] = 5.0 * sign;
        p[u] = a;
        p[v] = b;
        p
    };
    TriangleMesh {
        positions: vec![
            corner(-5.0, -5.0),
            corner(5.0, -5.0),
            corner(5.0, 5.0),
            corner(-5.0, 5.0),
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        ..TriangleMesh::default()
    }
}

/// The six walls of a box around the origin, each its own colour.
pub(super) fn room() -> Vec<TriangleMesh> {
    (0..6)
        .map(|i| wall(i / 2, if i % 2 == 0 { 1.0 } else { -1.0 }))
        .collect()
}

/// A fisheye off the room's centre, looking past its walls' axes so face
/// seams cross the walls obliquely.
pub(super) fn camera(fov: f64, mapping: FisheyeMapping) -> Camera {
    Camera {
        eye: [0.3, -0.2, 0.1],
        target: [3.3, 0.6, 1.0],
        up: [0.1, 0.2, 1.0],
        projection: Projection::Fisheye { fov, mapping },
    }
}

/// Small-picture render options under `lighting`.
pub(super) fn options(lighting: LightingScheme) -> RenderOptions {
    RenderOptions {
        width: 120,
        height: 90,
        background: BACKGROUND,
        lighting,
        ambient_occlusion: false,
        shadows: false,
        lamp_shadows: false,
        ..RenderOptions::default()
    }
}

/// The room drawn through `camera`: the picture and its depth keys.
pub(super) fn draw(camera: &Camera, options: &RenderOptions) -> (Vec<u8>, Vec<f64>) {
    let meshes = room();
    let paints: Vec<Paint> = WALLS
        .iter()
        .map(|&colour| Paint {
            colour,
            texture: None,
        })
        .collect();
    let model = (&meshes[..], &[][..], &[][..]);
    let (image, depth) = render_finished(model, &paints, &[true; 6], camera, options).unwrap();
    (image.rgba, depth)
}

/// The wall `ray_through` meets at pixel (`i`, `j`), `None` outside the
/// lens; `Some(None)` for a ray meeting nothing.
fn wall_at(camera: &Camera, i: i64, j: i64) -> Option<Option<(usize, f64)>> {
    let ray = camera.ray_through(i as f64 + 0.5, j as f64 + 0.5, 120, 90);
    match ray {
        Err(RenderError::OutsideLens) => None,
        Err(err) => panic!("{err}"),
        Ok(ray) => Some(pick(&room(), &ray).map(|h| (h.mesh, h.distance))),
    }
}

/// Pixel (`i`, `j`) and every pixel within two of it see the same wall.
fn interior(walls: &[Option<Option<(usize, f64)>>], i: i64, j: i64) -> bool {
    let mesh = |w: Option<Option<(usize, f64)>>| w.flatten().map(|(m, _)| m);
    let here = mesh(walls[(j * 120 + i) as usize]);
    (-2..=2).all(|dj: i64| {
        (-2..=2).all(|di: i64| {
            let (x, y) = (i + di, j + dj);
            !(0..120).contains(&x)
                || !(0..90).contains(&y)
                || mesh(walls[(y * 120 + x) as usize]) == here
        })
    })
}

/// The wall and depth hit at every pixel of a 120 x 90 picture, row by row.
pub(super) fn picks(camera: &Camera) -> Vec<Option<Option<(usize, f64)>>> {
    (0..90)
        .flat_map(|j| (0..120).map(move |i| (i, j)))
        .map(|(i, j)| wall_at(camera, i, j))
        .collect()
}

/// Every pixel away from a wall edge shows the wall its ray meets, at that
/// wall's depth; every pixel outside the lens is background.
pub(super) fn check_composition(fov: f64, mapping: FisheyeMapping, cube: bool) {
    let camera = camera(fov, mapping);
    let lens = Lens::for_camera(&camera, 120, 90).unwrap().unwrap();
    assert_eq!(
        matches!(lens.layout(), Layout::Cube { .. }),
        cube,
        "{mapping:?} {fov}"
    );
    let (rgba, depth) = draw(&camera, &options(LightingScheme::None));
    let walls = picks(&camera);
    let inside = walls.iter().filter(|w| w.is_some()).count();
    let mut checked = 0;
    for j in 0..90i64 {
        for i in 0..120i64 {
            let p = (j * 120 + i) as usize;
            let px: [u8; 4] = rgba[p * 4..p * 4 + 4].try_into().unwrap();
            match walls[p] {
                None => {
                    assert_eq!(px, BACKGROUND, "outside the lens at {i},{j}");
                    assert_eq!(depth[p], f64::NEG_INFINITY);
                }
                Some(None) => panic!("a ray from inside the room missed at {i},{j}"),
                Some(Some((mesh, distance))) if interior(&walls, i, j) => {
                    assert_eq!(px, WALLS[mesh], "{mapping:?} {fov} at {i},{j}");
                    let key = 1.0 / distance;
                    assert!(
                        (depth[p] / key - 1.0).abs() < 0.01,
                        "{mapping:?} {fov} at {i},{j}: {} vs {key}",
                        depth[p]
                    );
                    checked += 1;
                }
                Some(Some(_)) => {}
            }
        }
    }
    assert!(
        checked > inside / 2,
        "{checked} of {inside} interior pixels"
    );
}

#[test]
fn a_narrow_fisheye_composes_from_one_face() {
    check_composition(60.0, EQUI, false);
    check_composition(60.0, STEREO, false);
}

#[test]
fn a_wide_fisheye_composes_from_cube_faces() {
    check_composition(200.0, EQUI, true);
    check_composition(200.0, STEREO, true);
    check_composition(300.0, STEREO, true);
}

#[test]
fn a_full_sphere_equidistant_fisheye_draws_background_outside_its_circle() {
    check_composition(360.0, EQUI, true);
    let camera = camera(360.0, EQUI);
    let walls = picks(&camera);
    assert!(walls.iter().any(Option::is_none), "the corners lie outside");
    assert!(walls[45 * 120 + 60].is_some());
}

#[test]
fn a_stereographic_fisheye_fills_the_frame() {
    for fov in [90.0, 200.0, 350.0] {
        let walls = picks(&camera(fov, STEREO));
        assert!(walls.iter().all(Option::is_some), "{fov}");
    }
}

/// A point projected through the lens is on the ray through its pixel.
#[test]
fn ray_through_inverts_the_lens() {
    let points = [
        [4.0, 1.0, 1.0],
        [4.5, 0.2, 1.6],
        [4.0, 1.0, 2.0],
        [-3.0, 2.0, 0.5],
        [0.5, -4.0, -1.0],
        [-2.0, -1.0, 3.0],
        [1.0, 3.0, -4.0],
    ];
    for (mapping, fov) in [
        (EQUI, 90.0),
        (EQUI, 220.0),
        (EQUI, 360.0),
        (STEREO, 120.0),
        (STEREO, 330.0),
        (FisheyeMapping::Equirectangular, 360.0),
    ] {
        let camera = camera(fov, mapping);
        let lens = Lens::for_camera(&camera, 120, 90).unwrap().unwrap();
        let basis = camera.basis().unwrap();
        let mut seen = 0;
        for p in points {
            let c = basis.map(|axis| dot(sub(p, camera.eye), axis));
            if lens.max_angle() < c[0].hypot(c[1]).atan2(c[2]) {
                continue;
            }
            let [x, y] = lens.pixel(c).unwrap();
            let ray = camera.ray_through(x, y, 120, 90).unwrap();
            let to = sub(p, ray.origin);
            let along = dot(to, ray.direction);
            let off = length(sub(to, ray.direction.map(|d| d * along)));
            assert!(
                along > ray.min_distance && off < 1e-9 * length(to),
                "{mapping:?} {fov} {p:?}"
            );
            seen += 1;
        }
        assert!(seen >= 2, "{mapping:?} {fov}: {seen}");
    }
}

#[test]
fn a_field_outside_the_mappings_range_is_refused_by_name() {
    let cases = [
        (EQUI, 0.0),
        (EQUI, -10.0),
        (EQUI, f64::NAN),
        (EQUI, f64::INFINITY),
        (EQUI, 360.5),
        (STEREO, 360.0),
        (STEREO, 1e-320),
    ];
    for (mapping, fov) in cases {
        let camera = camera(fov, mapping);
        let refused = RenderError::FisheyeField { mapping };
        let render = crate::render(&room(), &camera, &options(LightingScheme::None));
        assert_eq!(render.err(), Some(refused.clone()), "{mapping:?} {fov}");
        assert_eq!(
            camera.ray_through(60.0, 45.0, 120, 90).err(),
            Some(refused.clone())
        );
        assert_eq!(FrameLens::new(&camera, 120, 90).err(), Some(refused));
    }
    assert!(Lens::for_camera(&camera(360.0, EQUI), 120, 90).is_ok());
}

#[test]
fn a_pixel_outside_the_circle_has_no_ray() {
    let camera = camera(360.0, EQUI);
    assert_eq!(
        camera.ray_through(0.5, 0.5, 120, 90).err(),
        Some(RenderError::OutsideLens)
    );
    assert!(camera.ray_through(60.0, 45.0, 120, 90).is_ok());
}

/// Camera-fixed lights stay with the fisheye camera, not each face: a flat
/// wall lit by directional lights is one colour across every face seam.
#[test]
fn camera_lights_do_not_turn_with_each_face() {
    let camera = camera(300.0, STEREO);
    let (rgba, _) = draw(&camera, &options(LightingScheme::White));
    let walls = picks(&camera);
    for mesh in 0..6 {
        let shades: Vec<[u8; 4]> = (0..90i64)
            .flat_map(|j| (0..120i64).map(move |i| (i, j)))
            .filter(|&(i, j)| {
                let p = (j * 120 + i) as usize;
                walls[p].flatten().map(|w| w.0) == Some(mesh) && interior(&walls, i, j)
            })
            .map(|(i, j)| {
                let p = (j * 120 + i) as usize * 4;
                rgba[p..p + 4].try_into().unwrap()
            })
            .collect();
        for s in &shades {
            let spread = s.iter().zip(shades[0]).map(|(a, b)| a.abs_diff(b)).max();
            assert!(spread <= Some(1), "wall {mesh}: {s:?} vs {:?}", shades[0]);
        }
    }
}

/// The faces a GPU host is told to draw include every face the CPU path
/// finds owning a pixel.
#[test]
fn the_host_face_set_covers_every_owned_face() {
    for mapping in [EQUI, STEREO] {
        for fov in [
            30.0, 60.0, 90.0, 120.0, 150.0, 179.0, 200.0, 250.0, 300.0, 340.0, 359.0,
        ] {
            for (w, h) in [(120, 90), (90, 120), (300, 40), (40, 300), (64, 64)] {
                let camera = camera(fov, mapping);
                let Ok(frame) = FrameLens::new(&camera, w, h) else {
                    assert!(mapping == STEREO && fov >= 360.0);
                    continue;
                };
                let lens = Lens::for_camera(&camera, w, h).unwrap().unwrap();
                let layout = lens.layout();
                let drawn: Vec<u32> = frame.faces.iter().map(|f| f.face).collect();
                for j in 0..h {
                    for i in 0..w {
                        let Some(d) = lens.direction(f64::from(i) + 0.5, f64::from(j) + 0.5) else {
                            continue;
                        };
                        let owner = u32::from(layout.owner(d));
                        assert!(
                            drawn.contains(&owner),
                            "{mapping:?} {fov} {w}x{h}: face {owner}"
                        );
                    }
                }
            }
        }
    }
}

/// The lens line and point uniforms a host reads from the frame are the
/// fisheye camera's eye and the basis its view matrix encodes.
#[test]
fn the_lens_frame_carries_the_fisheye_eye_and_basis() {
    let mut camera = camera(200.0, STEREO);
    camera.eye = [-3.0, 2.2, 2.0];
    camera.target = [1.0, -1.3, 0.3];
    camera.up = [0.1, 0.2, 1.0];
    let frame = FrameLens::new(&camera, 120, 90).unwrap();
    assert_eq!(frame.eye, [-3.0, 2.2, 2.0]);
    let m = camera.view_matrix().unwrap();
    let rows = [
        [m[0], m[4], m[8]],
        [m[1], m[5], m[9]],
        [-m[2], -m[6], -m[10]],
    ];
    for (axis, row) in frame.basis.iter().zip(rows) {
        for (a, b) in axis.iter().zip(row) {
            assert!((a - b).abs() < 1e-6, "{:?} vs {rows:?}", frame.basis);
        }
    }
}

#[test]
fn a_planar_camera_has_no_lens_frame() {
    let mut camera = camera(180.0, EQUI);
    camera.projection = Projection::Perspective { fov_y: 40.0 };
    assert_eq!(
        FrameLens::new(&camera, 120, 90).err(),
        Some(RenderError::Camera("the camera is not a fisheye"))
    );
    camera.projection = Projection::Fisheye {
        fov: 180.0,
        mapping: EQUI,
    };
    assert!(
        camera
            .projection_matrix(1.0, &Bounds::of(&room()).unwrap())
            .is_err()
    );
}

/// A fisheye's depth range encloses every distance from the eye to the
/// model, the eye inside it or not.
#[test]
fn a_fisheye_depth_range_encloses_the_model_around_the_eye() {
    let bounds = Bounds::of(&room()).unwrap();
    for eye in [[0.3, -0.2, 0.1], [20.0, 3.0, -4.0]] {
        let camera = Camera {
            eye,
            ..camera(250.0, EQUI)
        };
        let [near, far] = camera.depth_range(1.0, &bounds).unwrap();
        for k in 0..8 {
            let corner = [0, 1, 2].map(|a| if k >> a & 1 == 1 { 5.0 } else { -5.0 });
            let d = length(sub(corner, eye)) as f32;
            assert!(near <= d && d <= far, "{eye:?}: {d} outside {near}..{far}");
        }
        assert!(near > 0.0);
    }
}

/// A straight wire above the axis bends towards the picture centre at its
/// ends under a fisheye, its middle where the lens puts it.
#[test]
fn a_wire_bends_under_a_fisheye() {
    const RED: [u8; 4] = [255, 0, 0, 255];
    let model = AssembledModel {
        meshes: vec![wall(2, -1.0)],
        lines: vec![ModelLine {
            points: vec![[-20.0, 3.0, 5.0], [20.0, 3.0, 5.0]],
            colour: Some(RED),
            ..ModelLine::default()
        }],
        ..AssembledModel::default()
    };
    let camera = Camera {
        eye: [0.0; 3],
        target: [0.0, 0.0, 10.0],
        up: [0.0, 1.0, 0.0],
        projection: Projection::Fisheye {
            fov: 180.0,
            mapping: EQUI,
        },
    };
    let options = RenderOptions {
        width: 101,
        height: 101,
        ..options(LightingScheme::None)
    };
    let rgba = render_model(&model, &camera, &options).unwrap().rgba;
    let row = |i: usize| (0..101).find(|j| rgba[(j * 101 + i) * 4..][..4] == RED);
    let lens = Lens::for_camera(&camera, 101, 101).unwrap().unwrap();
    let [_, y] = lens.pixel([0.0, 3.0, 5.0]).unwrap();
    let middle = row(50).unwrap();
    assert!((middle as f64 - y).abs() <= 1.0, "{middle} vs {y}");
    let end = row(10).unwrap();
    assert!(
        end > middle + 5,
        "the end row {end} is not nearer the centre than {middle}"
    );
}

/// A vertex mode's point lands where the lens puts its vertex.
#[test]
fn a_vertex_point_lands_where_the_lens_puts_it() {
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    // No symmetry of the triangle maps it to itself, so a mirrored or
    // transposed lens misses a vertex.
    let corners = [[5.0, 5.0, 5.0], [-5.0, 4.0, 5.0], [2.0, -6.0, 5.0]];
    let model = AssembledModel {
        meshes: vec![TriangleMesh {
            positions: corners.to_vec(),
            triangles: vec![[0, 1, 2]],
            ..TriangleMesh::default()
        }],
        ..AssembledModel::default()
    };
    let camera = Camera {
        eye: [0.0; 3],
        target: [0.0, 0.0, 10.0],
        up: [0.0, 1.0, 0.0],
        projection: Projection::Fisheye {
            fov: 180.0,
            mapping: STEREO,
        },
    };
    let options = RenderOptions {
        width: 101,
        height: 101,
        render_mode: RenderMode {
            aux_colour: [0, 0, 255],
            ..RenderMode::new(RenderModeKind::Vertices)
        },
        ..options(LightingScheme::None)
    };
    let rgba = render_model(&model, &camera, &options).unwrap().rgba;
    let lens = Lens::for_camera(&camera, 101, 101).unwrap().unwrap();
    let basis = camera.basis().unwrap();
    for vertex in corners {
        let along = |axis: [f64; 3]| (0..3).map(|i| axis[i] * vertex[i]).sum::<f64>();
        let [x, y] = lens.pixel(basis.map(along)).unwrap();
        let p = (y.floor() as usize * 101 + x.floor() as usize) * 4;
        assert_eq!(rgba[p..p + 4], BLUE, "{vertex:?} at {x},{y}");
    }
}
