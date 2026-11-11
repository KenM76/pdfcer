//! Unit tests for `render`: shared fixtures here, the cases in `tests/`.

use super::*;
use crate::camera::FIT_MARGIN;
use crate::vec3::length;

fn quad(z: f64, half: f64, tilt: f64) -> TriangleMesh {
    TriangleMesh {
        positions: vec![
            [-half, -half, z - tilt],
            [half, -half, z + tilt],
            [half, half, z + tilt],
            [-half, half, z - tilt],
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
    }
}

fn ortho(height: f64) -> Camera {
    Camera {
        eye: [0.0, 0.0, 10.0],
        target: [0.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        projection: Projection::Orthographic { height },
    }
}

fn small() -> RenderOptions {
    RenderOptions {
        width: 40,
        height: 40,
        ..RenderOptions::default()
    }
}

fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let i = (y * image.width + x) as usize * 4;
    image.rgba[i..i + 4].try_into().unwrap()
}

const RED: Option<[u8; 4]> = Some([255, 0, 0, 255]);
const GLASS: Option<[u8; 4]> = Some([0, 0, 255, 102]);

fn triangles(positions: Vec<[f64; 3]>, triangles: Vec<[u32; 3]>) -> TriangleMesh {
    TriangleMesh {
        positions,
        triangles,
        ..TriangleMesh::default()
    }
}

fn drawn(image: &Image) -> usize {
    image.rgba.chunks(4).filter(|p| *p != [255; 4]).count()
}

mod bounce;
mod draws;
mod far_models;
mod far_side;
