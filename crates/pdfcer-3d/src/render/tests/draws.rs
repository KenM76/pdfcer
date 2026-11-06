use super::*;

#[test]
fn a_facing_square_fills_the_middle_and_leaves_the_corners() {
    let image = render(&[quad(0.0, 1.0, 0.0)], &ortho(4.0), &small()).unwrap();
    assert_eq!(pixel(&image, 20, 20), [190, 192, 200, 255]);
    assert_eq!(pixel(&image, 1, 1), [255, 255, 255, 255]);
    // Half the view height is covered: pixels 10..30.
    assert_eq!(pixel(&image, 10, 20)[0], 190);
    assert_eq!(pixel(&image, 9, 20)[0], 255);
}

#[test]
fn stored_normals_shade_smoothly_across_a_flat_square() {
    let mut mesh = quad(0.0, 1.0, 0.0);
    let s = 3f64.sqrt() / 2.0;
    // The left corners lean 60° away from the viewer; the right face it.
    mesh.normals = vec![[s, 0.0, 0.5], [0.0, 0.0, 2.0]];
    mesh.triangle_normals = vec![[0, 1, 1], [0, 1, 0]];
    let image = render(&[mesh.clone()], &ortho(4.0), &small()).unwrap();
    let red = |x| pixel(&image, x, 20)[0];
    assert!(
        red(11) < red(20) && red(20) < red(29),
        "{} {} {}",
        red(11),
        red(20),
        red(29)
    );
    // Shade 0.3 + 0.7 cos θ at the edges: 0.65 left, 1.0 right.
    assert!((i32::from(red(10)) - 128).abs() <= 4, "{}", red(10));
    assert!(red(29) >= 186);
    mesh.normals.clear();
    mesh.triangle_normals.clear();
    let flat = render(&[mesh], &ortho(4.0), &small()).unwrap();
    assert_eq!(pixel(&flat, 11, 20), pixel(&flat, 29, 20));
}

#[test]
fn the_nearer_surface_hides_the_farther_whatever_the_order() {
    let far = quad(-1.0, 2.0, 1.0);
    let near = quad(1.0, 0.5, 0.0);
    for meshes in [[far.clone(), near.clone()], [near, far]] {
        let image = render(&meshes, &ortho(8.0), &small()).unwrap();
        assert_eq!(pixel(&image, 20, 20), [190, 192, 200, 255]);
        assert!(
            pixel(&image, 20, 12)[0] < 190,
            "the tilted far square is darker"
        );
    }
}

/// Glass (40% blue) in front of red blends whichever is listed first,
/// glass behind red is hidden, a clear mesh draws nothing, and a mesh
/// with no colour takes the options' colour.
#[test]
fn meshes_draw_in_their_own_colours_and_glass_blends() {
    let (near, far) = (quad(1.0, 1.0, 0.0), quad(-1.0, 1.0, 0.0));
    let at_middle = |meshes: &[TriangleMesh], colours: &[Option<[u8; 4]>]| {
        let image = render_coloured(meshes, colours, &ortho(4.0), &small()).unwrap();
        pixel(&image, 20, 20)
    };
    let two = [near.clone(), far.clone()];
    assert_eq!(at_middle(&two, &[GLASS, RED]), [153, 0, 102, 255]);
    let flipped = [far.clone(), near.clone()];
    assert_eq!(at_middle(&flipped, &[RED, GLASS]), [153, 0, 102, 255]);
    assert_eq!(at_middle(&flipped, &[GLASS, RED]), [255, 0, 0, 255]);
    assert_eq!(
        at_middle(&two, &[Some([0, 0, 255, 0]), RED]),
        [255, 0, 0, 255]
    );
    assert_eq!(at_middle(&two, &[None]), [190, 192, 200, 255]);
    let panes = [quad(2.0, 1.0, 0.0), near.clone(), far.clone()];
    assert_eq!(
        at_middle(&panes, &[GLASS, GLASS, RED]),
        [92, 0, 163, 255],
        "glass does not hide glass"
    );
    let clear = RenderOptions {
        background: [0, 0, 0, 0],
        ..small()
    };
    let image = render_coloured(&[near], &[GLASS], &ortho(4.0), &clear).unwrap();
    assert_eq!(
        pixel(&image, 20, 20),
        [0, 0, 255, 102],
        "glass over nothing"
    );
}

#[test]
fn each_edge_bounds_a_triangle() {
    let tri = TriangleMesh {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]],
        triangles: vec![[0, 1, 2]],
        faces: vec![0..1],
        normals_recalculated: true,
        normals: Vec::new(),
        triangle_normals: Vec::new(),
        triangle_graphics: Vec::new(),
        triangle_faces: Vec::new(),
        uvs: Vec::new(),
        triangle_uvs: Vec::new(),
    };
    let image = render(&[tri], &ortho(4.0), &small()).unwrap();
    assert_ne!(pixel(&image, 20, 25)[0], 255);
    // Just past each edge: below the base, left and right of the apex.
    for (x, y) in [(20, 31), (13, 15), (27, 15)] {
        assert_eq!(pixel(&image, x, y)[0], 255, "({x},{y})");
    }
}

#[test]
fn a_face_turned_away_is_still_drawn() {
    let mut back = quad(0.0, 1.0, 0.0);
    for t in &mut back.triangles {
        t.swap(1, 2);
    }
    let image = render(&[back], &ortho(4.0), &small()).unwrap();
    assert_eq!(pixel(&image, 20, 20), [190, 192, 200, 255]);
}

#[test]
fn perspective_clips_what_is_behind_the_camera() {
    let camera = Camera {
        projection: Projection::Perspective { fov_y: 60.0 },
        ..ortho(1.0)
    };
    let behind = quad(20.0, 1.0, 0.0);
    let image = render(&[behind], &camera, &small()).unwrap();
    assert!(image.rgba.chunks(4).all(|p| p == [255, 255, 255, 255]));
    // A floor running from behind the camera to beyond the target.
    let floor = TriangleMesh {
        positions: vec![[-5.0, -1.0, 20.0], [5.0, -1.0, 20.0], [0.0, -1.0, -20.0]],
        triangles: vec![[0, 1, 2]],
        faces: vec![0..1],
        normals_recalculated: true,
        normals: Vec::new(),
        triangle_normals: Vec::new(),
        triangle_graphics: Vec::new(),
        triangle_faces: Vec::new(),
        uvs: Vec::new(),
        triangle_uvs: Vec::new(),
    };
    let image = render(&[floor], &camera, &small()).unwrap();
    assert_ne!(pixel(&image, 20, 38), [255, 255, 255, 255]);
    assert_eq!(pixel(&image, 20, 2), [255, 255, 255, 255]);
}

#[test]
fn a_fitted_camera_frames_the_model() {
    // Depth matters in perspective: the nearer square looks larger.
    let meshes = [quad(3.0, 3.0, 0.0), quad(-3.0, 3.0, 0.0)];
    let bounds = Bounds::of(&meshes).unwrap();
    for perspective in [true, false] {
        let camera =
            Camera::fit(&bounds, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], perspective, 2.0).unwrap();
        let options = RenderOptions {
            width: 80,
            height: 40,
            ..RenderOptions::default()
        };
        let image = render(&meshes, &camera, &options).unwrap();
        assert_ne!(pixel(&image, 40, 20)[0], 255);
        // Nothing drawn touches the image border.
        for x in 0..80 {
            assert_eq!(pixel(&image, x, 0)[0], 255);
            assert_eq!(pixel(&image, x, 39)[0], 255);
        }
    }
}

#[test]
fn a_mesh_fit_ignores_empty_box_corners() {
    // A thin strip along the box's diagonal: seen across the diagonal,
    // the box is as tall as it is wide, the strip is not.
    let strip = TriangleMesh {
        positions: vec![[0.0, 0.0, 0.0], [10.0, 10.0, 0.0], [10.0, 10.01, 0.0]],
        triangles: vec![[0, 1, 2]],
        ..TriangleMesh::default()
    };
    let meshes = [strip];
    let (dir, up) = ([0.0, 0.0, -1.0], [1.0, -1.0, 0.0]);
    let height = |c: Camera| match c.projection {
        Projection::Orthographic { height } => height,
        _ => unreachable!(),
    };
    let boxed = height(Camera::fit(&Bounds::of(&meshes).unwrap(), dir, up, false, 2.0).unwrap());
    let tight = height(Camera::fit_meshes(&meshes, dir, up, false, 2.0).unwrap());
    assert!((boxed / tight - 2.0).abs() < 0.01, "{boxed} vs {tight}");
    let near = Camera::fit_meshes(&meshes, dir, up, true, 2.0).unwrap();
    let far = Camera::fit(&Bounds::of(&meshes).unwrap(), dir, up, true, 2.0).unwrap();
    assert!(length(sub(near.eye, near.target)) < 0.6 * length(sub(far.eye, far.target)));

    // Lopsided in its box: the limiting vertex sits exactly on the
    // margin, measured from the re-centred target.
    let tri = TriangleMesh {
        positions: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [1.0, 2.0, 1.0]],
        triangles: vec![[0, 1, 2]],
        ..TriangleMesh::default()
    };
    // Narrow (width limits) and wide (height limits).
    for aspect in [0.5, 3.0] {
        let c = Camera::fit_meshes(
            std::slice::from_ref(&tri),
            [0.0, 0.3, -1.0],
            [1.0, 1.0, 0.0],
            true,
            aspect,
        )
        .unwrap();
        let [right, upward, forward] = c.basis().unwrap();
        let ty = 15f64.to_radians().tan() / FIT_MARGIN;
        let fill = tri
            .positions
            .iter()
            .map(|&p| {
                let r = sub(p, c.eye);
                let z = dot(r, forward);
                (dot(r, right).abs() / (ty * aspect)).max(dot(r, upward).abs() / ty) / z
            })
            .fold(0.0, f64::max);
        assert!((fill - 1.0).abs() < 1e-6, "aspect {aspect}: {fill}");
    }
}

#[test]
fn a_fitted_camera_fills_the_tighter_axis() {
    let drawn = |image: &Image, horizontal: bool, line: u32| {
        let n = if horizontal {
            image.width
        } else {
            image.height
        };
        (0..n)
            .filter(|&i| {
                let (x, y) = if horizontal { (i, line) } else { (line, i) };
                pixel(image, x, y)[0] != 255
            })
            .count()
    };
    // Height-limited: a 6 x 6 square in an 80 x 40 image.
    let square = [quad(0.0, 3.0, 0.0)];
    // Width-limited and off-origin: a 10 x 1 strip in a 40 x 40 image.
    let strip = [TriangleMesh {
        positions: vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        faces: vec![0..2],
        normals_recalculated: true,
        normals: Vec::new(),
        triangle_normals: Vec::new(),
        triangle_graphics: Vec::new(),
        triangle_faces: Vec::new(),
        uvs: Vec::new(),
        triangle_uvs: Vec::new(),
    }];
    for perspective in [true, false] {
        let fit = |meshes: &[TriangleMesh], aspect| {
            let bounds = Bounds::of(meshes).unwrap();
            Camera::fit(
                &bounds,
                [0.0, 0.0, -1.0],
                [0.0, 1.0, 0.0],
                perspective,
                aspect,
            )
            .unwrap()
        };
        let wide = RenderOptions {
            width: 80,
            height: 40,
            ..RenderOptions::default()
        };
        let image = render(&square, &fit(&square, 2.0), &wide).unwrap();
        let rows = drawn(&image, false, 40);
        assert!(
            (34..=37).contains(&rows),
            "{perspective}: {rows} of 40 rows"
        );

        let image = render(&strip, &fit(&strip, 1.0), &small()).unwrap();
        let cols = drawn(&image, true, 20);
        assert!(
            (34..=37).contains(&cols),
            "{perspective}: {cols} of 40 columns"
        );
        // Centred: the margins differ by at most a pixel.
        let left = (0..40)
            .take_while(|&x| pixel(&image, x, 20)[0] == 255)
            .count();
        assert!(
            left.abs_diff(40 - cols - left) <= 1,
            "{perspective}: left {left}"
        );
    }
}

#[test]
fn bad_sizes_and_cameras_are_refused() {
    let m = [quad(0.0, 1.0, 0.0)];
    let size = |width, height| RenderOptions {
        width,
        height,
        ..RenderOptions::default()
    };
    assert!(matches!(
        render(&m, &ortho(1.0), &size(0, 10)),
        Err(RenderError::Size { .. })
    ));
    assert!(matches!(
        render(&m, &ortho(1.0), &size(10_000, 10_000)),
        Err(RenderError::Size { .. })
    ));
    let bad = [
        Camera {
            eye: [0.0; 3],
            target: [0.0; 3],
            ..ortho(1.0)
        },
        Camera {
            up: [0.0, 0.0, 1.0],
            ..ortho(1.0)
        },
        Camera {
            projection: Projection::Perspective { fov_y: 180.0 },
            ..ortho(1.0)
        },
        ortho(0.0),
    ];
    for camera in bad {
        assert!(matches!(
            render(&m, &camera, &small()),
            Err(RenderError::Camera(_))
        ));
    }
    let b = Bounds::of(&m).unwrap();
    assert!(Camera::fit(&b, [0.0; 3], [0.0, 1.0, 0.0], true, 1.0).is_err());
    assert!(Camera::fit(&b, [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], true, 0.0).is_err());
}

#[test]
fn a_fully_transparent_mesh_does_not_widen_the_box_or_the_fit() {
    let unit = TriangleMesh {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        triangles: vec![[0, 1, 2]],
        ..TriangleMesh::default()
    };
    let far = TriangleMesh {
        positions: vec![[1e6, 0.0, 0.0], [1e6, 1.0, 0.0], [0.0, 1.0, 0.0]],
        ..unit.clone()
    };
    let both = [unit.clone(), far];
    let colours = [None, Some([10, 20, 30, 0])];
    let b = Bounds::of_coloured(&both, &colours).unwrap();
    assert_eq!((b.min, b.max), ([0.0; 3], [1.0, 1.0, 0.0]));
    let (dir, up) = ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    for perspective in [true, false] {
        let alone = Camera::fit_meshes(std::slice::from_ref(&unit), dir, up, perspective, 1.0);
        let clear = Camera::fit_coloured(&both, &colours, dir, up, perspective, 1.0);
        assert_eq!(clear.unwrap(), alone.unwrap());
    }
    // A translucent mesh is drawn, so it is framed.
    let faint = [None, Some([10, 20, 30, 1])];
    assert_eq!(Bounds::of_coloured(&both, &faint).unwrap().max[0], 1e6);
    // Nothing drawn: nothing to frame.
    let none = Camera::fit_coloured(&both[1..], &colours[1..], dir, up, true, 1.0);
    assert!(matches!(none, Err(RenderError::Camera(_))));
}

#[test]
fn non_finite_and_missing_vertices_are_skipped() {
    let mesh = TriangleMesh {
        positions: vec![[0.0, 0.0, 0.0], [f64::NAN, 0.0, 0.0], [0.0, 1.0, 0.0]],
        triangles: vec![[0, 1, 2], [0, 2, 9]],
        faces: vec![0..2],
        normals_recalculated: true,
        normals: Vec::new(),
        triangle_normals: Vec::new(),
        triangle_graphics: Vec::new(),
        triangle_faces: Vec::new(),
        uvs: Vec::new(),
        triangle_uvs: Vec::new(),
    };
    let image = render(std::slice::from_ref(&mesh), &ortho(4.0), &small()).unwrap();
    assert!(image.rgba.chunks(4).all(|p| p == [255, 255, 255, 255]));
    // Neither triangle draws, so neither widens the box.
    assert_eq!(Bounds::of(&[mesh]), None);
}

#[test]
fn a_scheme_lights_each_channel() {
    let lit = |lighting| {
        let options = RenderOptions {
            lighting,
            ..small()
        };
        let image = render(&[quad(0.0, 1.0, 0.0)], &ortho(4.0), &options).unwrap();
        pixel(&image, 20, 20)
    };
    assert_eq!(lit(LightingScheme::None), [190, 192, 200, 255]);
    // A face square to the camera: ambient .1 plus the camera light.
    assert_eq!(lit(LightingScheme::Headlamp), [171, 173, 200, 255]);
    assert_eq!(lit(LightingScheme::Artwork), [190, 192, 200, 255]);
    assert_eq!(lit(LightingScheme::Hard)[0], 255, "over-bright saturates");
}

#[test]
fn every_thread_count_draws_the_same_pixels() {
    // Overlapping tilted squares, glass, and a floor crossing the near
    // plane, in a tall image so the bands are many.
    let mut meshes: Vec<TriangleMesh> = (0..12)
        .map(|k| {
            let mut q = quad(f64::from(k) * 0.3 - 2.0, 1.0 + f64::from(k) * 0.1, 0.4);
            for p in &mut q.positions {
                p[1] += f64::from(k % 5) * 0.4 - 0.8;
            }
            q
        })
        .collect();
    meshes.push(TriangleMesh {
        positions: vec![[-5.0, -1.0, 20.0], [5.0, -1.0, 20.0], [0.0, -1.0, -20.0]],
        triangles: vec![[0, 1, 2]],
        ..quad(0.0, 1.0, 0.0)
    });
    let colours: Vec<_> = (0..meshes.len())
        .map(|k| {
            Some([
                40 * (k % 6) as u8,
                100,
                200,
                if k % 3 == 0 { 120 } else { 255 },
            ])
        })
        .collect();
    let camera = Camera {
        projection: Projection::Perspective { fov_y: 60.0 },
        ..ortho(1.0)
    };
    let draw = |threads| {
        let options = RenderOptions {
            width: 37,
            height: 301,
            lighting: LightingScheme::Night,
            threads,
            ..RenderOptions::default()
        };
        render_coloured(&meshes, &colours, &camera, &options).unwrap()
    };
    let one = draw(1);
    assert!(one.rgba.chunks(4).any(|p| p != [255, 255, 255, 255]));
    for threads in [2, 3, 7, 64, 0] {
        assert_eq!(draw(threads), one, "{threads} threads");
    }
}

#[test]
fn a_lamp_lights_a_pool_on_one_large_face() {
    // No stored normals and four corners all far from the lamp: shaded
    // per corner, every pixel would get one shade.
    let lamp = crate::ModelLight::new(
        crate::ModelLightKind::Point {
            position: [0.0, 0.0, 1.0],
            attenuation: [0.0, 0.0, 1.0],
        },
        [1.0; 3],
    );
    let paint = Paint {
        colour: [100, 100, 100, 255],
        texture: None,
    };
    let image = render_painted(
        (&[quad(0.0, 1.0, 0.0)], &[lamp]),
        &[paint],
        &[true],
        &ortho(2.0),
        &small(),
    )
    .unwrap();
    let red = |x, y| pixel(&image, x, y)[0];
    assert!(
        red(20, 20) > red(2, 2) + 50,
        "{} {}",
        red(20, 20),
        red(2, 2)
    );
}

#[test]
fn a_lamp_casts_a_card_s_shadow_on_the_floor() {
    // Lamp at (-1, 0, 2), card spanning x -0.5..0.5 at z 1: its shadow
    // on the floor spans x 0..2, the part past x 0.5 visible.
    let lamp = crate::ModelLight::new(
        crate::ModelLightKind::Point {
            position: [-1.0, 0.0, 2.0],
            attenuation: [1.0, 0.0, 0.0],
        },
        [1.0; 3],
    );
    let paint = Paint {
        colour: [200, 200, 200, 255],
        texture: None,
    };
    let draw = |lamp_shadows| {
        let options = RenderOptions {
            lamp_shadows,
            ambient_occlusion: false,
            ..small()
        };
        let meshes = [quad(0.0, 3.0, 0.0), quad(1.0, 0.5, 0.0)];
        render_painted(
            (&meshes, std::slice::from_ref(&lamp)),
            &[paint; 2],
            &[true; 2],
            &ortho(6.0),
            &options,
        )
        .unwrap()
    };
    let (on, off) = (draw(true), draw(false));
    let red = |image: &Image, x| pixel(image, x, 20)[0];
    // x = 1.3 is in the shadow, x = 2.6 past it, x = -2.5 behind the lamp.
    assert!(red(&on, 28) < 20 && red(&off, 28) > 100, "{}", red(&on, 28));
    assert_eq!(red(&on, 37), red(&off, 37));
    assert_eq!(red(&on, 3), red(&off, 3));
    // Off the shadow's edge the card's top is lit.
    assert_eq!(red(&on, 20), red(&off, 20));
}

#[test]
fn a_model_past_the_pixel_test_ceiling_is_refused_before_drawing() {
    // 2^14 triangles, each covering every pixel of a 4096 x 4096 image.
    let big = [[-10.0, -10.0, 0.0], [30.0, -10.0, 0.0], [-10.0, 30.0, 0.0]];
    let mesh = triangles(big.to_vec(), vec![[0, 1, 2]; 1 << 14]);
    let options = RenderOptions {
        width: 4096,
        height: 4096,
        ..RenderOptions::default()
    };
    let start = std::time::Instant::now();
    assert_eq!(
        render(&[mesh], &ortho(4.0), &options),
        Err(RenderError::TooMuchWork {
            limit: crate::MAX_RENDER_PIXEL_TESTS
        })
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(20));
    // One such triangle, shadows and all, is far inside it.
    let one = triangles(big.to_vec(), vec![[0, 1, 2]]);
    let image = render(&[one], &ortho(4.0), &small()).unwrap();
    assert!(image.rgba.chunks(4).all(|p| p != [255, 255, 255, 255]));
}

#[test]
fn a_box_at_the_float_ceiling_has_a_finite_centre_inside_it() {
    let b = Bounds {
        min: [f64::MAX / 2.0; 3],
        max: [f64::MAX; 3],
    };
    let c = b.centre();
    assert!(c.iter().all(|v| v.is_finite()));
    assert!((0..3).all(|i| b.min[i] <= c[i] && c[i] <= b.max[i]));
    assert_eq!(c, [f64::MAX * 0.75; 3]);
}

#[test]
fn an_undrawn_triangle_neither_widens_the_box_nor_the_fit() {
    let mesh = triangles(
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [f64::NAN, 0.0, 0.0],
            [1e6, 0.0, 0.0],
            [0.0, 1e6, 0.0],
        ],
        vec![[0, 1, 2], [3, 4, 5]],
    );
    let meshes = [mesh];
    let bounds = Bounds::of(&meshes).unwrap();
    assert_eq!((bounds.min, bounds.max), ([0.0; 3], [1.0, 1.0, 0.0]));
    let camera =
        Camera::fit_meshes(&meshes, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], false, 1.0).unwrap();
    let Projection::Orthographic { height } = camera.projection else {
        unreachable!()
    };
    assert!((height - 1.1).abs() < 1e-9, "{height}");
    assert!((0..3).all(|i| (camera.target[i] - [0.5, 0.5, 0.0][i]).abs() < 1e-9));
}
