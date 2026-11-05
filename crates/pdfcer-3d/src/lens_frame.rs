//! A fisheye frame on the CPU: the lens's faces drawn as ordinary
//! perspective frames, each picture pixel taking the nearest texel of the
//! face that owns its direction ([`crate::FrameLens`] states the contract
//! the GPU path shares).

use crate::lens::{Face, Layout, Lens, faces};
use crate::raster::Paint;
use crate::render::{Camera, Image, RenderError, RenderOptions, render_view};
use crate::vec3::dot;

/// [`crate::render::render_finished`] through `lens`: the picture and each
/// pixel's depth key, 1 / the distance from the eye (`NEG_INFINITY` where
/// nothing opaque was drawn or outside the picture circle).
pub(crate) fn render(
    model: crate::gloss::Model<'_>,
    paints: &[Paint<'_>],
    opaque: &[bool],
    (camera, lens): (&Camera, &Lens),
    options: &RenderOptions,
) -> Result<(Image, Vec<f64>), RenderError> {
    let (width, height) = (options.width, options.height);
    let basis = camera.basis()?;
    let layout = lens.layout();
    let owners = owners(lens, layout, width, height);
    let mut present = [false; 6];
    for &o in &owners {
        if let Some(p) = present.get_mut(usize::from(o)) {
            *p = true;
        }
    }
    let wanted = |code: u8| present.get(usize::from(code)).copied().unwrap_or(false);
    let pixels = owners.len();
    let mut rgba = options.background.repeat(pixels);
    let mut depth = vec![f64::NEG_INFINITY; pixels];
    for face in faces(camera, lens, layout, wanted)? {
        let face_options = RenderOptions {
            width: face.width,
            height: face.height,
            ..*options
        };
        let (image, keys) = render_view(
            model,
            paints,
            opaque,
            &face.camera,
            &face_options,
            Some(&basis),
        )?;
        resample(
            (&face, &image, &keys),
            (lens, &owners, width),
            (&mut rgba, &mut depth),
        );
    }
    Ok((
        Image {
            width,
            height,
            rgba,
        },
        depth,
    ))
}

/// Each picture pixel's owning face ([`Layout::owner`]), 255 outside the
/// picture circle.
fn owners(lens: &Lens, layout: Layout, width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(width as usize * height as usize);
    for j in 0..height {
        for i in 0..width {
            let d = lens.direction(f64::from(i) + 0.5, f64::from(j) + 0.5);
            out.push(d.map_or(255, |d| layout.owner(d)));
        }
    }
    out
}

/// Copies into the picture, at every pixel `face` owns, the colour and
/// depth of the face texel its direction lands in.
fn resample(
    (face, image, keys): (&Face, &Image, &[f64]),
    (lens, owners, width): (&Lens, &[u8], u32),
    (rgba, depth): (&mut [u8], &mut [f64]),
) {
    let (fw, fh) = (f64::from(face.width), f64::from(face.height));
    let [sx, sy] = face.scale;
    for (p, _) in owners.iter().enumerate().filter(|(_, o)| **o == face.code) {
        let (i, j) = (p % width as usize, p / width as usize);
        let Some(d) = lens.direction(i as f64 + 0.5, j as f64 + 0.5) else {
            continue;
        };
        let [x, y, z] = face.axes.map(|a| dot(d, a));
        if z.is_nan() || z <= 0.0 {
            continue;
        }
        let u = ((x / z * sx + 1.0) / 2.0 * fw).floor().clamp(0.0, fw - 1.0) as usize;
        let v = ((1.0 - y / z * sy) / 2.0 * fh).floor().clamp(0.0, fh - 1.0) as usize;
        let t = v * face.width as usize + u;
        let (Some(key), Some(src)) = (keys.get(t), image.rgba.get(t * 4..t * 4 + 4)) else {
            continue;
        };
        if let (Some(d), Some(dst)) = (depth.get_mut(p), rgba.get_mut(p * 4..p * 4 + 4)) {
            // A face key is 1 / the distance along its axis; along the ray
            // that is z times 1 / the distance from the eye.
            *d = if key.is_finite() { key * z } else { *key };
            dst.copy_from_slice(src);
        }
    }
}
