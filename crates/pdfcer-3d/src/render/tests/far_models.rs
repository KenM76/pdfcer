use super::*;

#[test]
fn a_small_model_far_from_the_origin_keeps_its_view_direction() {
    let dir = [-1.0, 1.0, -1.0];
    let far = 1e17;
    // A unit triangle at x = 1e17, and one 64 units across (1e17 rounds
    // to multiples of 16) out on every axis.
    let models = [
        vec![[far, 0.0, 0.0], [far, 1.0, 0.0], [far, 0.0, 1.0]],
        vec![
            [far, far, far],
            [far + 64.0, far, far],
            [far, far + 64.0, far],
        ],
    ];
    for positions in models {
        let meshes = [triangles(positions, vec![[0, 1, 2]])];
        for perspective in [false, true] {
            let camera =
                Camera::fit_meshes(&meshes, dir, [0.0, 0.0, 1.0], perspective, 1.0).unwrap();
            let seen = normalize(sub(camera.target, camera.eye)).unwrap();
            let want = normalize(dir).unwrap();
            assert!((0..3).all(|i| (seen[i] - want[i]).abs() < 1e-6), "{seen:?}");
            let image = render(&meshes, &camera, &small()).unwrap();
            let drawn = image.rgba.chunks(4).filter(|p| *p != [255; 4]).count();
            // In perspective the eye cannot come nearer than f64 can place
            // it (EYE_SHARE), so the model may be framed below a pixel.
            assert!(perspective || drawn > 0, "{drawn}");
        }
    }
}

#[test]
fn an_on_screen_triangle_is_drawn_when_projecting_it_would_overflow() {
    let wide = RenderOptions {
        width: 30,
        height: 10,
        ..RenderOptions::default()
    };
    // A tiny field of view: x times the focal length (about 1.1e300)
    // overflows, though x over z does not.
    let camera = Camera {
        eye: [0.0; 3],
        target: [0.0, 0.0, 1e308],
        up: [0.0, 1.0, 0.0],
        projection: Projection::Perspective { fov_y: 1e-298 },
    };
    let far = triangles(
        vec![
            [1.5e8, -3e7, 1e308],
            [2.5e8, -3e7, 1e308],
            [2e8, 3e7, 1e308],
        ],
        vec![[0, 1, 2]],
    );
    assert!(drawn(&render(&[far], &camera, &wide).unwrap()) > 0);
    // Half the orthographic height times the aspect ratio overflows.
    let big = triangles(
        vec![
            [-5e307, -5e307, 0.0],
            [5e307, -5e307, 0.0],
            [0.0, 5e307, 0.0],
        ],
        vec![[0, 1, 2]],
    );
    let image = render(&[big], &ortho(1.5e308), &wide).unwrap();
    assert_ne!(pixel(&image, 15, 5), [255; 4]);
}

#[test]
fn a_triangle_whose_edges_overflow_is_drawn_after_framing() {
    let h = f64::MAX / 1.5;
    let meshes = [triangles(
        vec![[-h, 0.0, 0.0], [h, 0.0, 0.0], [0.0, h, 0.0]],
        vec![[0, 1, 2]],
    )];
    for perspective in [false, true] {
        let camera =
            Camera::fit_meshes(&meshes, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], perspective, 1.0);
        let image = render(&meshes, &camera.unwrap(), &small()).unwrap();
        assert!(drawn(&image) > 0, "perspective {perspective}");
    }
}
