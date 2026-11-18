//! `X086`: write a free-form mesh in any colour space, read it back, and
//! re-express a mesh through a non-linear colour conversion.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::filters::decode_stream;
use pdfcer_core::mesh_shading::{
    MeshError, MeshGeometry, MeshStreamFormat, MeshTriangle, MeshVertex, MeshWriteOptions,
    Reexpress, decode_mesh, reexpress_mesh, reexpress_mesh_with_breaks, write_free_form_mesh,
};
use pdfcer_core::object::{Dict, Name, Object};
use pdfcer_core::settings::MeshPatchPadding;

fn v(x: f64, y: f64, color: &[f64]) -> MeshVertex {
    MeshVertex {
        xy: [x, y],
        color: color.to_vec(),
    }
}

/// A spot ink plus black: `[/DeviceN [/PANTONE#20185 /Black] /DeviceCMYK fn]`.
fn devicen() -> Object {
    Object::Array(vec![
        Object::Name(Name::from(b"DeviceN")),
        Object::Array(vec![
            Object::Name(Name::from(b"PANTONE 185")),
            Object::Name(Name::from(b"Black")),
        ]),
        Object::Name(Name::from(b"DeviceCMYK")),
        Object::Integer(9),
    ])
}

fn read_back(dict: &Dict, data: &[u8], components: usize) -> Vec<MeshTriangle> {
    let plain = decode_stream(dict, data).expect("FlateDecode");
    let format = MeshStreamFormat::from_dict(dict, components, MeshPatchPadding::PerRecord)
        .expect("our own dictionary is valid");
    let mesh = decode_mesh(&plain, &format).expect("our own stream decodes");
    assert!(!mesh.truncated);
    match mesh.geometry {
        MeshGeometry::Triangles(t) => t,
        MeshGeometry::Patches(_) => panic!("type 4 is triangles"),
    }
}

/// A fan of triangles, half of them sharing an edge with the one before.
fn strip(components: usize) -> Vec<MeshTriangle> {
    let c = |k: f64| -> Vec<f64> {
        (0..components)
            .map(|i| ((k * 0.137 + i as f64 * 0.31) % 1.0).abs())
            .collect()
    };
    let pts: Vec<MeshVertex> = (0..12)
        .map(|i| {
            let x = f64::from(i / 2) * 20.0;
            let y = if i % 2 == 0 { 0.0 } else { 30.0 };
            v(x + 5.0, y + 5.0, &c(f64::from(i)))
        })
        .collect();
    pts.windows(3)
        .map(|w| [w[0].clone(), w[1].clone(), w[2].clone()])
        .collect()
}

#[test]
fn a_devicen_mesh_keeps_its_space_and_round_trips_at_16_bits() {
    let tris = strip(2);
    let opts = MeshWriteOptions::new(devicen(), 2).with_background(vec![0.0, 1.0]);
    let w = write_free_form_mesh(&tris, &opts).unwrap();
    assert_eq!(
        w.dict.get(b"ColorSpace"),
        Some(&devicen()),
        "space kept exactly"
    );
    assert_eq!(w.dict.get(b"ShadingType"), Some(&Object::Integer(4)));
    assert_eq!(w.report.bits_per_component, 16);
    assert_eq!(w.report.triangles, tris.len());
    assert!(w.report.shared_edges > 0, "a strip shares edges");

    let back = read_back(&w.dict, &w.data, 2);
    assert_eq!(back.len(), tris.len());
    for (a, b) in tris.iter().flatten().zip(back.iter().flatten()) {
        for k in 0..2 {
            assert!((a.xy[k] - b.xy[k]).abs() < 1e-6, "{a:?} vs {b:?}");
        }
        for (x, y) in a.color.iter().zip(&b.color) {
            assert!((x - y).abs() <= 0.5 / 65535.0 + 1e-9, "{x} vs {y}");
        }
    }
}

#[test]
fn eight_bit_components_round_trip_within_half_a_level() {
    let tris = strip(4);
    let opts = MeshWriteOptions::new(Object::Name(Name::from(b"DeviceCMYK")), 4)
        .with_bits_per_component(8);
    let w = write_free_form_mesh(&tris, &opts).unwrap();
    let back = read_back(&w.dict, &w.data, 4);
    for (a, b) in tris.iter().flatten().zip(back.iter().flatten()) {
        for (x, y) in a.color.iter().zip(&b.color) {
            assert!((x - y).abs() <= 0.5 / 255.0 + 1e-9, "{x} vs {y}");
        }
    }
}

#[test]
fn hostile_inputs_are_errors_not_panics() {
    let gray = || Object::Name(Name::from(b"DeviceGray"));
    let one = vec![[
        v(0.0, 0.0, &[0.0]),
        v(1.0, 0.0, &[0.5]),
        v(0.0, 1.0, &[1.0]),
    ]];
    let write = |t: &[MeshTriangle], o: &MeshWriteOptions| write_free_form_mesh(t, o).unwrap_err();

    assert_eq!(
        write(&[], &MeshWriteOptions::new(gray(), 1)),
        MeshError::Empty
    );
    let mut nan = one.clone();
    nan[0][1].xy[0] = f64::NAN;
    assert!(matches!(
        write(&nan, &MeshWriteOptions::new(gray(), 1)),
        MeshError::NonFinite { triangle: 0 }
    ));
    assert!(matches!(
        write(
            &one,
            &MeshWriteOptions::new(gray(), 1).with_bits_per_component(32)
        ),
        MeshError::BadOutputBits(32)
    ));
    assert!(matches!(
        write(
            &one,
            &MeshWriteOptions::new(Object::Name(Name::from(b"DeviceRGB")), 1)
        ),
        MeshError::ColorSpaceMismatch { .. }
    ));
    assert!(matches!(
        write(&one, &MeshWriteOptions::new(gray(), 1).with_triangle_cap(0)),
        MeshError::TooManyTriangles { .. }
    ));

    let mut dict = Dict::new();
    dict.insert(Name::from(b"ShadingType"), Object::Integer(4));
    dict.insert(Name::from(b"BitsPerCoordinate"), Object::Integer(16));
    dict.insert(Name::from(b"BitsPerComponent"), Object::Integer(8));
    dict.insert(Name::from(b"BitsPerFlag"), Object::Integer(8));
    dict.insert(
        Name::from(b"Decode"),
        Object::Array(vec![Object::Integer(0), Object::Integer(1)]),
    );
    assert!(matches!(
        MeshStreamFormat::from_dict(&dict, 1, MeshPatchPadding::PerRecord),
        Err(MeshError::BadDecode { .. })
    ));
    dict.insert(Name::from(b"BitsPerComponent"), Object::Integer(32));
    assert!(MeshStreamFormat::from_dict(&dict, 1, MeshPatchPadding::PerRecord).is_err());
}

#[test]
fn truncated_streams_keep_complete_records() {
    let tris = strip(1);
    let opts = MeshWriteOptions::new(Object::Name(Name::from(b"DeviceGray")), 1);
    let w = write_free_form_mesh(&tris, &opts).unwrap();
    let plain = decode_stream(&w.dict, &w.data).unwrap();
    let format = MeshStreamFormat::from_dict(&w.dict, 1, MeshPatchPadding::PerRecord).unwrap();
    for cut in 0..plain.len() {
        // Every prefix decodes or declines; none panics.
        if let Ok(m) = decode_mesh(&plain[..cut], &format) {
            assert!(m.truncated || cut % 11 == 0, "cut {cut}");
        }
    }
}

/// Decode a gray type 4 mesh written from `tris`.
fn gray_mesh(tris: &[MeshTriangle]) -> pdfcer_core::mesh_shading::DecodedMesh {
    let opts = MeshWriteOptions::new(Object::Name(Name::from(b"DeviceGray")), 1);
    let w = write_free_form_mesh(tris, &opts).unwrap();
    let plain = decode_stream(&w.dict, &w.data).unwrap();
    let format = MeshStreamFormat::from_dict(&w.dict, 1, MeshPatchPadding::PerRecord).unwrap();
    decode_mesh(&plain, &format).unwrap()
}

/// Gray `g` to "CMYK" through a deliberately non-linear curve on K.
fn curved(c: &[f64]) -> Option<Vec<f64>> {
    let g = c[0];
    Some(vec![0.0, 0.0, 0.0, (1.0 - g).powf(2.2)])
}

#[test]
fn a_uniform_mesh_reexpresses_without_subdividing() {
    let flat = vec![[
        v(0.0, 0.0, &[0.4]),
        v(100.0, 0.0, &[0.4]),
        v(0.0, 100.0, &[0.4]),
    ]];
    let (tris, report) =
        reexpress_mesh(&gray_mesh(&flat), 4, curved, &Reexpress::default()).unwrap();
    assert_eq!(report.subdivisions, 0);
    assert_eq!(tris.len(), 1);
    assert_eq!(report.over_tolerance, 0);
}

#[test]
fn a_nonlinear_conversion_subdivides_until_within_tolerance() {
    let ramp = vec![[
        v(0.0, 0.0, &[0.0]),
        v(100.0, 0.0, &[1.0]),
        v(0.0, 100.0, &[0.0]),
    ]];
    let tol = 1.0 / 255.0;
    let (tris, report) =
        reexpress_mesh(&gray_mesh(&ramp), 4, curved, &Reexpress::default()).unwrap();
    assert!(report.subdivisions > 0, "a curve is not one triangle");
    assert_eq!(report.over_tolerance, 0);
    assert_eq!(report.triangles, tris.len());
    // Sample inside every output triangle: linear K vs the true curve.
    for t in &tris {
        for (wa, wb, wc) in [
            (1.0, 1.0, 1.0),
            (4.0, 1.0, 1.0),
            (1.0, 4.0, 1.0),
            (1.0, 1.0, 4.0),
        ] {
            let s = wa + wb + wc;
            let k = (wa * t[0].color[3] + wb * t[1].color[3] + wc * t[2].color[3]) / s;
            let x = (wa * t[0].xy[0] + wb * t[1].xy[0] + wc * t[2].xy[0]) / s;
            let truth = curved(&[x / 100.0]).unwrap()[3];
            assert!((k - truth).abs() <= 2.0 * tol, "at x {x}: {k} vs {truth}");
        }
    }
}

/// Linear except for a knee at `g = 0.8`.
fn knee(c: &[f64]) -> Option<Vec<f64>> {
    Some(vec![0.0, 0.0, 0.0, (c[0] - 0.8).max(0.0) * 5.0])
}

#[test]
fn subdivision_concentrates_where_the_conversion_bends() {
    let splits = |g0: f64, g1: f64| {
        let m = gray_mesh(&[[
            v(0.0, 0.0, &[g0]),
            v(100.0, 0.0, &[g1]),
            v(0.0, 100.0, &[g0]),
        ]]);
        reexpress_mesh(&m, 4, knee, &Reexpress::default())
            .unwrap()
            .1
            .subdivisions
    };
    assert_eq!(splits(0.0, 0.5), 0, "straight span");
    assert!(splits(0.5, 1.0) > 0, "the span holding the knee splits");
}

#[test]
fn a_declining_or_misshapen_conversion_is_an_error() {
    let m = gray_mesh(&strip(1));
    assert_eq!(
        reexpress_mesh(&m, 4, |_| None, &Reexpress::default()).unwrap_err(),
        MeshError::ConversionDeclined
    );
    assert!(matches!(
        reexpress_mesh(&m, 4, |_| Some(vec![0.0]), &Reexpress::default()).unwrap_err(),
        MeshError::ConversionShape {
            expected: 4,
            found: 1
        }
    ));
    assert!(matches!(
        reexpress_mesh(&m, 4, curved, &Reexpress::default().with_triangle_cap(2)).unwrap_err(),
        MeshError::TooManyTriangles { .. }
    ));
}

/// A type 3 stitch can be zero at every probe and still carry a band:
/// the function grid sees it and splits there.
#[test]
fn a_narrow_function_band_between_the_probes_splits_the_triangle() {
    let band = |s: &[f64]| {
        let t = s.first().copied()?;
        Some(vec![
            if (0.125..0.375).contains(&t) {
                1.0
            } else {
                0.0
            },
            0.0,
            0.0,
            0.0,
        ])
    };
    let tri = [
        v(0.0, 0.0, &[0.0]),
        v(100.0, 0.0, &[1.0]),
        v(0.0, 100.0, &[1.0]),
    ];
    let mut m = gray_mesh(&[tri]);
    m.parametric = true;
    let (tris, report) = reexpress_mesh(&m, 4, band, &Reexpress::default()).unwrap();
    assert!(tris.len() > 1, "split where the band is");
    assert!(tris.iter().flatten().any(|v| v.color[0] == 1.0));
    assert!(report.subdivisions > 0);
    let ramp = |s: &[f64]| Some(vec![s[0], 0.0, 0.0, 0.0]);
    let (one, report) = reexpress_mesh(&m, 4, ramp, &Reexpress::default()).unwrap();
    assert_eq!((one.len(), report.subdivisions), (1, 0));
    m.parametric = false;
    let (blind, _) = reexpress_mesh(&m, 4, band, &Reexpress::default()).unwrap();
    assert_eq!(blind.len(), 1, "without the grid the probes miss the band");
}

/// The parametric triangle `t` = 0, 1, 1.
fn parametric_triangle() -> pdfcer_core::mesh_shading::DecodedMesh {
    let tri = [
        v(0.0, 0.0, &[0.0]),
        v(100.0, 0.0, &[1.0]),
        v(0.0, 100.0, &[1.0]),
    ];
    let mut m = gray_mesh(&[tri]);
    m.parametric = true;
    m
}

#[test]
fn the_function_breaks_find_a_band_narrower_than_a_grid_step() {
    let m = parametric_triangle();
    let band = |s: &[f64]| {
        let t = s.first().copied()?;
        Some(vec![
            f64::from(u8::from((0.126..0.127).contains(&t))),
            0.0,
            0.0,
            0.0,
        ])
    };
    let (blank, _) = reexpress_mesh(&m, 4, band, &Reexpress::default()).unwrap();
    assert_eq!(blank.len(), 1, "the grid steps over the band");
    let breaks = [0.0, 0.126, 0.127, 1.0];
    let (tris, report) =
        reexpress_mesh_with_breaks(&m, 4, band, &breaks, &Reexpress::default()).unwrap();
    assert!(tris.len() > 1, "split where the band is");
    assert!(report.over_tolerance > 0, "the band's edges are jumps");
}

#[test]
fn a_step_of_one_and_a_half_tolerances_is_never_accepted() {
    let m = parametric_triangle();
    let params = Reexpress::default();
    let tol = 1.0 / 255.0;
    let step = move |s: &[f64]| {
        let t = s.first().copied()?;
        Some(vec![if t < 0.5 { 0.0 } else { 1.5 * tol }, 0.0, 0.0, 0.0])
    };
    let (_, report) = reexpress_mesh_with_breaks(&m, 4, step, &[0.0, 0.5, 1.0], &params).unwrap();
    assert!(report.over_tolerance > 0, "{report:?}");
    let ramp = |s: &[f64]| Some(vec![s[0], 0.0, 0.0, 0.0]);
    let (one, report) = reexpress_mesh_with_breaks(&m, 4, ramp, &[0.0, 1.0], &params).unwrap();
    assert_eq!((one.len(), report.subdivisions), (1, 0));
}

/// Most an output triangle's blended first component is from `convert` of
/// the [`parametric_triangle`]'s `t` = (x + y) / 100, over a grid of points
/// in each triangle.
fn worst_inside(out: &[MeshTriangle], convert: impl Fn(f64) -> f64) -> f64 {
    let mut worst = 0.0_f64;
    for tri in out {
        for i in 0..=16 {
            for j in 0..=16 - i {
                let w = [f64::from(i) / 16.0, f64::from(j) / 16.0];
                let w = [w[0], w[1], 1.0 - w[0] - w[1]];
                let mix = |f: &dyn Fn(&MeshVertex) -> f64| {
                    w.iter().zip(tri).map(|(w, v)| w * f(v)).sum::<f64>()
                };
                let t = mix(&|v| v.xy[0] + v.xy[1]) / 100.0;
                worst = worst.max((mix(&|v| v.color[0]) - convert(t)).abs());
            }
        }
    }
    worst
}

#[test]
fn a_function_clamped_by_its_colour_space_splits_where_it_clamps() {
    let m = parametric_triangle();
    // Clamps at t = 1/6 and 5/6, off every split's dyadic midpoints.
    let clamped = |t: f64| (-0.25 + 1.5 * t).clamp(0.0, 1.0);
    let convert = move |s: &[f64]| Some(vec![clamped(*s.first()?), 0.0, 0.0, 0.0]);
    let params = Reexpress::default();
    let (tris, report) =
        reexpress_mesh_with_breaks(&m, 4, convert, &[0.0, 1.0 / 6.0, 5.0 / 6.0, 1.0], &params)
            .unwrap();
    assert!(tris.len() > 1);
    assert_eq!(report.over_tolerance, 0, "{report:?}");
    let worst = worst_inside(&tris, clamped);
    assert!(worst <= params_tolerance(), "off by {worst}");
    let (blind, _) = reexpress_mesh_with_breaks(&m, 4, convert, &[0.0, 1.0], &params).unwrap();
    let missed = worst_inside(&blind, clamped);
    assert!(
        missed > params_tolerance(),
        "the probes alone: off by {missed}"
    );
    let ramp = |s: &[f64]| Some(vec![s.first()?.clamp(0.0, 1.0), 0.0, 0.0, 0.0]);
    let (one, report) = reexpress_mesh_with_breaks(&m, 4, ramp, &[0.0, 1.0], &params).unwrap();
    assert_eq!((one.len(), report.subdivisions), (1, 0));
}

/// [`Reexpress::default`]'s tolerance.
fn params_tolerance() -> f64 {
    1.0 / 255.0
}
