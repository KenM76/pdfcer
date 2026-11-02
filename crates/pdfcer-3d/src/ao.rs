//! Screen-space ambient occlusion: each pixel is darkened by how far nearby
//! surfaces rise above its own, read from the depth buffer alone, then the
//! result is averaged over 4 × 4 pixels. [`crate::gpu_ao`]'s shaders do the
//! same arithmetic per pixel, so the two paths agree to rounding.
//!
//! A sample `v` (from the pixel's surface point to a neighbour's, within
//! `radius`) contributes `max(0, cos(v, n) − BIAS) · (1 − (|v| / radius)²)`;
//! the pixel keeps `1 − STRENGTH · Σ / SAMPLES`, clamped to 0–1. A flat
//! surface occludes nothing, a crease or contact darkens on both sides.

use crate::parallel;
use crate::raster::{View, at};
use crate::vec3::{cross, dot, length, normalize, scale, sub};

/// Samples per pixel.
pub(crate) const SAMPLES: usize = 16;
/// The sampling radius as a share of the model's bounding radius.
pub(crate) const RADIUS_SHARE: f64 = 0.04;
/// The largest sampling radius on screen, as a share of the image height.
pub(crate) const MAX_SCREEN_SHARE: f64 = 0.25;
/// How strongly the summed occlusion darkens.
pub(crate) const STRENGTH: f64 = 2.5;
/// The cosine a sample must exceed before it occludes: keeps a finely
/// tessellated curved surface from darkening itself.
pub(crate) const BIAS: f64 = 0.1;
/// The angle between successive samples of the spiral, radians.
pub(crate) const GOLDEN_ANGLE: f64 = 2.399_963_229_728_653;
/// Each pixel's spiral rotation, sixteenths of a turn, by `(y % 4) * 4 +
/// x % 4` (row 0 at the top): a 4 × 4 Bayer order, which the 4 × 4 average
/// removes.
pub(crate) const PATTERN: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

/// How a pixel and its view depth map to a view-space point (x right, y up,
/// z forward from the eye), and model units to pixels.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Screen {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) orthographic: bool,
    /// View x, y per unit of normalised device x, y: per unit of depth in
    /// perspective, absolute in orthographic.
    pub(crate) unproject: [f64; 2],
    /// Pixels per model unit: at depth 1 in perspective (divide by the
    /// depth), everywhere in orthographic.
    pub(crate) pixels: f64,
}

impl Screen {
    pub(crate) fn new(view: &View, width: u32, height: u32) -> Screen {
        let h = f64::from(height);
        match *view {
            View::Perspective { focal, aspect, .. } => Screen {
                width,
                height,
                orthographic: false,
                unproject: [aspect / focal, 1.0 / focal],
                pixels: focal * h / 2.0,
            },
            View::Orthographic {
                half_height,
                aspect,
            } => Screen {
                width,
                height,
                orthographic: true,
                unproject: [half_height * aspect, half_height],
                pixels: h / (2.0 * half_height),
            },
        }
    }

    /// The view-space point of pixel (`x`, `y`) at depth `z`.
    fn point(&self, x: i64, y: i64, z: f64) -> [f64; 3] {
        let nx = (x as f64 + 0.5) / f64::from(self.width) * 2.0 - 1.0;
        let ny = 1.0 - (y as f64 + 0.5) / f64::from(self.height) * 2.0;
        let k = if self.orthographic { 1.0 } else { z };
        [nx * self.unproject[0] * k, ny * self.unproject[1] * k, z]
    }

    fn radius_pixels(&self, radius: f64, z: f64) -> f64 {
        let per = if self.orthographic {
            self.pixels
        } else {
            self.pixels / z
        };
        (radius * per).min(MAX_SCREEN_SHARE * f64::from(self.height))
    }
}

/// The view depths of a whole image, row 0 at the top; `None` where nothing
/// was drawn.
pub(crate) struct Depths<'d> {
    pub(crate) screen: Screen,
    /// The CPU depth keys ([`View::project`]).
    pub(crate) keys: &'d [f64],
}

impl Depths<'_> {
    fn z(&self, x: i64, y: i64) -> Option<f64> {
        let (w, h) = (i64::from(self.screen.width), i64::from(self.screen.height));
        if x < 0 || y < 0 || x >= w || y >= h {
            return None;
        }
        let key = *self.keys.get((y * w + x) as usize)?;
        if !key.is_finite() {
            return None;
        }
        Some(if self.screen.orthographic {
            -key
        } else {
            1.0 / key
        })
    }

    fn point(&self, x: i64, y: i64) -> Option<[f64; 3]> {
        Some(self.screen.point(x, y, self.z(x, y)?))
    }

    /// The surface normal at a pixel from its neighbours, each axis taking
    /// the side nearer in depth, turned towards the eye.
    fn normal(&self, x: i64, y: i64, p: [f64; 3]) -> Option<[f64; 3]> {
        let side = |a: Option<[f64; 3]>, b: Option<[f64; 3]>| match (a, b) {
            (Some(a), Some(b)) => Some(
                if (at(a, 2) - at(p, 2)).abs() <= (at(b, 2) - at(p, 2)).abs() {
                    sub(a, p)
                } else {
                    sub(p, b)
                },
            ),
            (Some(a), None) => Some(sub(a, p)),
            (None, Some(b)) => Some(sub(p, b)),
            (None, None) => None,
        };
        let dx = side(self.point(x + 1, y), self.point(x - 1, y))?;
        let dy = side(self.point(x, y + 1), self.point(x, y - 1))?;
        let n = normalize(cross(dx, dy))?;
        let eye = if self.screen.orthographic {
            [0.0, 0.0, 1.0]
        } else {
            p
        };
        Some(if dot(n, eye) > 0.0 { scale(n, -1.0) } else { n })
    }

    /// The kept share (0–1) of pixel (`x`, `y`)'s light; 1 where nothing
    /// was drawn or no normal can be found.
    pub(crate) fn occlusion(&self, x: i64, y: i64, radius: f64) -> f64 {
        let Some(p) = self.point(x, y) else {
            return 1.0;
        };
        let Some(n) = self.normal(x, y, p) else {
            return 1.0;
        };
        let reach = self.screen.radius_pixels(radius, at(p, 2));
        let index = ((y & 3) * 4 + (x & 3)) as usize;
        let turn = f64::from(PATTERN.get(index).copied().unwrap_or(0)) / 16.0;
        let mut sum = 0.0;
        for i in 0..SAMPLES {
            let t = (i as f64 + 0.5) / SAMPLES as f64;
            let angle = i as f64 * GOLDEN_ANGLE + turn * std::f64::consts::TAU;
            let d = reach * t.sqrt();
            let sx = x + (d * angle.cos() + 0.5).floor() as i64;
            let sy = y + (d * angle.sin() + 0.5).floor() as i64;
            if (sx, sy) == (x, y) {
                continue;
            }
            let Some(q) = self.point(sx, sy) else {
                continue;
            };
            let v = sub(q, p);
            let l = length(v);
            if l > 0.0 && l < radius {
                let fall = 1.0 - (l / radius) * (l / radius);
                sum += (dot(v, n) / l - BIAS).max(0.0) * fall;
            }
        }
        (1.0 - STRENGTH * sum / SAMPLES as f64).clamp(0.0, 1.0)
    }

    /// The mean of `raw` over the 4 × 4 pixels from (`x` − 2, `y` − 2) that
    /// were drawn and lie within `radius` of this pixel's depth; 1 where
    /// nothing was drawn.
    fn blurred(&self, raw: &[f64], x: i64, y: i64, radius: f64) -> f64 {
        let Some(z) = self.z(x, y) else {
            return 1.0;
        };
        let w = i64::from(self.screen.width);
        let (mut sum, mut count) = (0.0, 0.0);
        for dy in -2..2 {
            for dx in -2..2 {
                let (qx, qy) = (x + dx, y + dy);
                if self.z(qx, qy).is_some_and(|q| (q - z).abs() < radius)
                    && let Some(a) = raw.get((qy * w + qx) as usize)
                {
                    sum += a;
                    count += 1.0;
                }
            }
        }
        if count > 0.0 { sum / count } else { 1.0 }
    }
}

/// Darkens `rgba` (`depths`' image) by ambient occlusion of `radius` model
/// units, on `workers` threads; the same at any count.
pub(crate) fn apply(rgba: &mut [u8], depths: &Depths<'_>, radius: f64, workers: usize) {
    let (w, h) = (depths.screen.width as usize, depths.screen.height as usize);
    if w == 0 || h == 0 || radius.is_nan() || radius <= 0.0 {
        return;
    }
    let workers = workers.max(1);
    let rows = h.div_ceil(workers * 4).max(1);
    let mut raw = vec![1.0f64; w * h];
    let bands: Vec<(usize, &mut [f64])> = raw.chunks_mut(rows * w).enumerate().collect();
    parallel::run_each(deal(bands, workers), &|job| {
        for (k, band) in job {
            for (i, a) in band.iter_mut().enumerate() {
                let (x, y) = ((i % w) as i64, (k * rows + i / w) as i64);
                *a = depths.occlusion(x, y, radius);
            }
        }
    });
    let bands: Vec<(usize, &mut [u8])> = rgba.chunks_mut(rows * w * 4).enumerate().collect();
    parallel::run_each(deal(bands, workers), &|job| {
        for (k, band) in job {
            for (i, px) in band.chunks_mut(4).enumerate() {
                let (x, y) = ((i % w) as i64, (k * rows + i / w) as i64);
                let a = depths.blurred(&raw, x, y, radius);
                for c in px.iter_mut().take(3) {
                    *c = (f64::from(*c) * a).round() as u8;
                }
            }
        }
    });
}

/// Bands dealt to `workers` in turn.
fn deal<T>(bands: Vec<T>, workers: usize) -> Vec<Vec<T>> {
    let mut jobs: Vec<Vec<T>> = (0..workers).map(|_| Vec::new()).collect();
    for (k, band) in bands.into_iter().enumerate() {
        if let Some(job) = jobs.get_mut(k % workers) {
            job.push(band);
        }
    }
    jobs
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;

    fn screen() -> Screen {
        Screen {
            width: 40,
            height: 40,
            orthographic: true,
            unproject: [1.0, 1.0],
            pixels: 20.0,
        }
    }

    #[test]
    fn a_flat_floor_is_not_occluded_and_a_step_darkens_its_foot() {
        // Orthographic, looking straight down on a floor at depth 10 with a
        // block (depth 9.8) over x >= 20.
        let keys: Vec<f64> = (0..40 * 40)
            .map(|i| if i % 40 >= 20 { -9.8 } else { -10.0 })
            .collect();
        let d = Depths {
            screen: screen(),
            keys: &keys,
        };
        assert_eq!(d.occlusion(5, 20, 0.5), 1.0);
        let foot = d.occlusion(18, 20, 0.5);
        assert!(foot < 0.95, "{foot}");
        // The block's own top is flat and is not darkened.
        assert_eq!(d.occlusion(30, 20, 0.5), 1.0);
    }

    #[test]
    fn nothing_drawn_is_left_alone_and_the_result_ignores_thread_count() {
        let keys: Vec<f64> = (0..40 * 40)
            .map(|i| match i % 40 {
                0..10 => f64::NEG_INFINITY,
                10..20 => -10.0,
                _ => -9.8,
            })
            .collect();
        let d = Depths {
            screen: screen(),
            keys: &keys,
        };
        let image = |workers| {
            let mut rgba = vec![200u8; 40 * 40 * 4];
            apply(&mut rgba, &d, 0.5, workers);
            rgba
        };
        let one = image(1);
        assert_eq!(one, image(5));
        assert_eq!(&one[..4], &[200, 200, 200, 200]);
        assert!(one[(20 * 40 + 19) * 4] < 200);
    }
}
