//! The light meter's GPU halves, finished with the CPU's own work.

// Tests fail loudly by design and index fixed fixtures.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use super::tests::{at, camera, glow_room, meter};
use super::*;
use crate::IndirectOptions;
use crate::indirect::tests::quad;

#[test]
fn a_gpu_read_finished_with_the_cpu_gather_reads_as_the_cpu_meter() {
    let (mut model, dark) = glow_room([1.0; 3]);
    model.meshes.push(quad([
        [-0.5, -0.5, 0.5],
        [0.5, -0.5, 0.5],
        [0.5, 0.5, 0.5],
        [-0.5, 0.5, 0.5],
    ]));
    model.colours.push(Some([255, 255, 255, 255]));
    model.finishes.push(None);
    let options = RenderOptions {
        indirect: IndirectOptions::default(),
        ..dark
    };
    let m = meter(&model, &options, &MeterSettings::default());
    let scene = m.gpu_scene().unwrap();
    assert_eq!((scene.point_count, scene.glow_count), (0, 3));
    let mut q = at([0.2, -0.3, 0.0]);
    q.radius = 0.3;
    q.samples = 9;
    let want = m.read(&q).unwrap();
    let read = m.gpu_read(&q).unwrap();
    assert!(read.needs_pass);
    assert_eq!(read.point_count as usize, want.on_surface);
    let short = vec![0.0; 4];
    assert!(matches!(
        read.clone().finish(&short),
        Err(IlluminanceError::BadQuery("read"))
    ));
    // Each sample's texel as the CPU gathers it: green bounced, blue glow.
    let size = 4 * (read.target_width * read.target_height) as usize;
    let mut texels = vec![0.0f32; size];
    for i in 0..read.point_count as usize {
        let p = &read.points.texels[4 * i..4 * i + 3];
        let n = &read.normals.texels[4 * i..4 * i + 3];
        let origin = scene.origin;
        let p = std::array::from_fn(|k| f64::from(p[k]) + origin[k]);
        let n = std::array::from_fn(|k| f64::from(n[k]));
        let s = m.measure(p, n);
        texels[4 * i + 1] = luminance(s.bounced) as f32;
        texels[4 * i + 2] = luminance(s.emitted) as f32;
    }
    let got = read.finish(&texels).unwrap();
    assert!(want.bounced > 0.0 && want.emitted > 0.0, "{want:?}");
    assert_eq!((got.direct, got.ambient), (want.direct, want.ambient));
    assert_eq!(got.lights, want.lights);
    assert!(
        (got.total - want.total).abs() <= 1e-5 * want.total,
        "{got:?} {want:?}"
    );
}

#[test]
fn a_gpu_meter_built_from_the_cpu_atlas_reads_as_the_cpu_meter() {
    let (mut model, dark) = glow_room([1.0; 3]);
    model.meshes.push(quad([
        [-0.5, -0.5, 0.5],
        [0.5, -0.5, 0.5],
        [0.5, 0.5, 0.5],
        [-0.5, 0.5, 0.5],
    ]));
    model.colours.push(Some([255, 255, 255, 255]));
    model.finishes.push(None);
    let options = RenderOptions {
        indirect: IndirectOptions::default(),
        ..dark
    };
    let settings = MeterSettings::default();
    let cpu = meter(&model, &options, &settings);
    let prepared = PreparedModel::new(&model);
    let make = || LightMeter::gpu_new(&model, &prepared, &camera(), &options, &settings).unwrap();
    let gpu = make();
    let build = gpu.build.as_ref().unwrap();
    // The probes trace the glow: the ceiling's row is white.
    assert_eq!(build.glow_count, 3);
    assert_eq!(&build.glow.texels[4..7], &[1.0; 3]);
    let atlas = cpu.volume.as_ref().unwrap().atlas().texels;
    assert!(matches!(
        make().finish(&atlas[1..]),
        Err(IlluminanceError::BadQuery("atlas"))
    ));
    let gpu = gpu.finish(&atlas).unwrap();
    for q in [
        IlluminanceQuery::new(2, [0.0, 0.0, 0.5], [0.0, 0.0, -1.0]),
        at([1.5, 1.5, 0.0]),
        at([0.2, -0.3, 0.0]),
    ] {
        let (a, b) = (cpu.read(&q).unwrap(), gpu.read(&q).unwrap());
        assert!(a.bounced > 0.0, "{a:?}");
        assert!((a.total - b.total).abs() <= 1e-6 * a.total, "{a:?} {b:?}");
    }
}
