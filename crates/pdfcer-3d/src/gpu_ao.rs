//! Per-frame GPU ambient occlusion inputs and its two reference passes: the
//! CPU path's arithmetic ([`crate::ao`]) per pixel, read from the main
//! pass's depth.

use crate::ao::{self, Screen};
use crate::lighting::Shader;
use crate::raster::View;
use crate::render::{Bounds, Camera, RenderError, RenderOptions};

/// The vertex shader both ambient occlusion passes draw with: one triangle
/// covering the viewport, three vertices, no attributes.
pub const GLSL_ES_300_SCREEN_VERTEX: &str = r"#version 300 es
void main() {
    vec2 p = vec2(float((gl_VertexID & 1) << 2), float((gl_VertexID & 2) << 1)) - 1.0;
    gl_Position = vec4(p, 0.0, 1.0);
}
";

/// The head both passes share: their uniforms, and a pixel's view depth and
/// point from the main pass's depth texture, pixels counted from the top row.
macro_rules! shared {
    () => {
        r"#version 300 es
precision highp float;
precision highp int;
uniform highp sampler2D u_depth;
uniform ivec2 u_ao_size;
uniform bool u_ao_orthographic;
uniform vec2 u_ao_depth_range;
uniform vec2 u_ao_unproject;
uniform float u_ao_pixels;
uniform float u_ao_radius;

ivec2 pixel() {
    return ivec2(int(gl_FragCoord.x), u_ao_size.y - 1 - int(gl_FragCoord.y));
}

bool depth(ivec2 p, out float z) {
    z = 0.0;
    if (p.x < 0 || p.y < 0 || p.x >= u_ao_size.x || p.y >= u_ao_size.y) return false;
    float d = texelFetch(u_depth, ivec2(p.x, u_ao_size.y - 1 - p.y), 0).r;
    if (d >= 1.0) return false;
    float n = u_ao_depth_range.x;
    float f = u_ao_depth_range.y;
    z = u_ao_orthographic ? mix(n, f, d) : n * exp2(d * log2(f / n));
    return true;
}

bool point(ivec2 p, out vec3 v) {
    float z;
    v = vec3(0.0);
    if (!depth(p, z)) return false;
    vec2 nd = vec2((float(p.x) + 0.5) / float(u_ao_size.x) * 2.0 - 1.0,
                   1.0 - (float(p.y) + 0.5) / float(u_ao_size.y) * 2.0);
    float k = u_ao_orthographic ? 1.0 : z;
    v = vec3(nd * u_ao_unproject * k, z);
    return true;
}
"
    };
}

/// The occlusion pass's fragment shader: writes each pixel's kept share of
/// light (0–1) to red; 1 where nothing was drawn.
pub const GLSL_ES_300_AO_FRAGMENT: &str = concat!(
    shared!(),
    r"
out vec4 o_ao;

const int SAMPLES = 16;
const float STRENGTH = 2.5;
const float BIAS = 0.1;
const float GOLDEN_ANGLE = 2.399963229728653;
const float MAX_SCREEN_SHARE = 0.25;
const int PATTERN[16] = int[16](0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5);

vec3 side(bool ha, vec3 a, bool hb, vec3 b, vec3 p, out bool ok) {
    ok = ha || hb;
    if (ha && hb) return abs(a.z - p.z) <= abs(b.z - p.z) ? a - p : p - b;
    return ha ? a - p : p - b;
}

void main() {
    ivec2 c = pixel();
    vec3 p;
    o_ao = vec4(1.0);
    if (!point(c, p)) return;
    vec3 r, l, d, u;
    bool hr = point(c + ivec2(1, 0), r);
    bool hl = point(c - ivec2(1, 0), l);
    bool hd = point(c + ivec2(0, 1), d);
    bool hu = point(c - ivec2(0, 1), u);
    bool okx, oky;
    vec3 dx = side(hr, r, hl, l, p, okx);
    vec3 dy = side(hd, d, hu, u, p, oky);
    vec3 n = cross(dx, dy);
    if (!okx || !oky || length(n) == 0.0) return;
    n = normalize(n);
    vec3 eye = u_ao_orthographic ? vec3(0.0, 0.0, 1.0) : p;
    if (dot(n, eye) > 0.0) n = -n;
    float per = u_ao_orthographic ? u_ao_pixels : u_ao_pixels / p.z;
    float reach = min(u_ao_radius * per, MAX_SCREEN_SHARE * float(u_ao_size.y));
    float turn = float(PATTERN[(c.y & 3) * 4 + (c.x & 3)]) / 16.0;
    float sum = 0.0;
    for (int i = 0; i < SAMPLES; i++) {
        float t = (float(i) + 0.5) / float(SAMPLES);
        float a = float(i) * GOLDEN_ANGLE + turn * 6.283185307179586;
        float s = reach * sqrt(t);
        ivec2 q = c + ivec2(floor(s * cos(a) + 0.5), floor(s * sin(a) + 0.5));
        vec3 v;
        if (q == c || !point(q, v)) continue;
        v -= p;
        float len = length(v);
        if (len > 0.0 && len < u_ao_radius) {
            float k = len / u_ao_radius;
            sum += max(dot(v, n) / len - BIAS, 0.0) * (1.0 - k * k);
        }
    }
    o_ao = vec4(vec3(clamp(1.0 - STRENGTH * sum / float(SAMPLES), 0.0, 1.0)), 1.0);
}
"
);

/// The composite pass's fragment shader: the main pass's colour times the
/// mean occlusion over the 4 × 4 pixels from (x − 2, y − 2), top-row
/// counted, that were drawn and lie within `u_ao_radius` of this pixel's
/// depth. Uniforms beyond the occlusion pass's: `u_colour` (the main pass's
/// colour texture) and `u_ao` (the occlusion pass's output), both `NEAREST`.
/// Writes straight RGBA; a pixel where nothing was drawn passes through.
pub const GLSL_ES_300_AO_COMPOSITE_FRAGMENT: &str = concat!(
    shared!(),
    r"
uniform sampler2D u_colour;
uniform sampler2D u_ao;
out vec4 o_colour;

void main() {
    ivec2 c = pixel();
    vec4 colour = texelFetch(u_colour, ivec2(gl_FragCoord.xy), 0);
    o_colour = colour;
    float z;
    if (!depth(c, z)) return;
    float sum = 0.0;
    float count = 0.0;
    for (int y = -2; y < 2; y++) {
        for (int x = -2; x < 2; x++) {
            ivec2 q = c + ivec2(x, y);
            float w;
            if (depth(q, w) && abs(w - z) < u_ao_radius) {
                sum += texelFetch(u_ao, ivec2(q.x, u_ao_size.y - 1 - q.y), 0).r;
                count += 1.0;
            }
        }
    }
    float a = count > 0.0 ? sum / count : 1.0;
    o_colour = vec4(clamp(colour.rgb * a, 0.0, 1.0), colour.a);
}
"
);

/// One frame's ambient occlusion: the uniforms both of its passes take.
///
/// After the main pass renders into a colour texture and a depth texture
/// (at least 24 bits, sampled `NEAREST`, no compare mode) of `size`, the
/// host draws [`GLSL_ES_300_SCREEN_VERTEX`] with
/// [`GLSL_ES_300_AO_FRAGMENT`] (three vertices, no depth test) into an
/// `RGBA8` or `R8` texture of `size`, then the same triangle with
/// [`GLSL_ES_300_AO_COMPOSITE_FRAGMENT`] to the screen. Both passes bind
/// the main depth as `u_depth`. Redrawn every frame: it depends on the view.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FrameAmbientOcclusion {
    /// Viewport width and height, pixels (`u_ao_size`).
    pub size: [u32; 2],
    /// Whether the camera is orthographic (`u_ao_orthographic`).
    pub orthographic: bool,
    /// [`Camera::depth_range`] for the same viewport (`u_ao_depth_range`).
    pub depth_range: [f32; 2],
    /// View x, y per unit of normalised device x, y: per unit of view depth
    /// in perspective (`u_ao_unproject`).
    pub unproject: [f32; 2],
    /// Pixels per model unit, at view depth 1 in perspective (`u_ao_pixels`).
    pub pixels: f32,
    /// The sampling radius, model units (`u_ao_radius`).
    pub radius: f32,
}

impl FrameAmbientOcclusion {
    /// The occlusion `options` asks for over a model of `bounds` seen from
    /// `camera` in a `size` viewport; `None` when
    /// [`RenderOptions::ambient_occlusion`] is off, the scheme is unlit, or
    /// the model has no extent.
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] for a degenerate camera, as [`crate::render`];
    /// [`RenderError::Size`] for an empty viewport.
    pub fn new(
        camera: &Camera,
        options: &RenderOptions,
        bounds: &Bounds,
        size: [u32; 2],
    ) -> Result<Option<Self>, RenderError> {
        let [width, height] = size;
        if width == 0 || height == 0 {
            return Err(RenderError::Size { width, height });
        }
        let basis = camera.basis()?;
        let options = &crate::render_mode::frame_options(options);
        let shader = Shader::new(options.lighting.lights(), options.light_frame, &basis);
        let radius = bounds.radius() * ao::RADIUS_SHARE;
        if !crate::render::ambient_occlusion(options, &shader) || radius.is_nan() || radius <= 0.0 {
            return Ok(None);
        }
        let aspect = f64::from(width) / f64::from(height);
        let screen = Screen::new(&View::new(camera, aspect)?, width, height);
        Ok(Some(FrameAmbientOcclusion {
            size,
            orthographic: screen.orthographic,
            depth_range: camera.depth_range(aspect, bounds)?,
            unproject: screen.unproject.map(|v| v as f32),
            pixels: screen.pixels as f32,
            radius: radius as f32,
        }))
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::{LightingScheme, Projection};

    #[test]
    fn the_switch_and_an_unlit_scheme_turn_it_off() {
        let camera = Camera {
            eye: [3.0, -4.0, 5.0],
            target: [0.0; 3],
            up: [0.0, 0.0, 1.0],
            projection: Projection::Perspective { fov_y: 40.0 },
        };
        let bounds = Bounds {
            min: [-1.0; 3],
            max: [1.0; 3],
        };
        let size = [800, 600];
        let on = FrameAmbientOcclusion::new(&camera, &RenderOptions::default(), &bounds, size);
        let on = on.unwrap().unwrap();
        assert!(!on.orthographic);
        assert!((on.radius - 3f32.sqrt() * 0.04).abs() < 1e-6);
        for options in [
            RenderOptions {
                ambient_occlusion: false,
                ..RenderOptions::default()
            },
            RenderOptions {
                lighting: LightingScheme::None,
                ..RenderOptions::default()
            },
        ] {
            let off = FrameAmbientOcclusion::new(&camera, &options, &bounds, size);
            assert_eq!(off.unwrap(), None);
        }
    }

    #[test]
    fn the_shaders_carry_the_cpu_constants() {
        let shader = GLSL_ES_300_AO_FRAGMENT;
        assert!(shader.contains(&format!("SAMPLES = {};", ao::SAMPLES)));
        for (name, value) in [
            ("STRENGTH", ao::STRENGTH),
            ("BIAS", ao::BIAS),
            ("GOLDEN_ANGLE", ao::GOLDEN_ANGLE),
            ("MAX_SCREEN_SHARE", ao::MAX_SCREEN_SHARE),
        ] {
            assert!(shader.contains(&format!("{name} = {value:?};")), "{name}");
        }
        let pattern = ao::PATTERN.map(|p| p.to_string()).join(", ");
        assert!(shader.contains(&format!("int[16]({pattern})")));
        let common = [
            "u_depth",
            "u_ao_size",
            "u_ao_orthographic",
            "u_ao_depth_range",
            "u_ao_unproject",
            "u_ao_pixels",
            "u_ao_radius",
        ];
        for shader in [GLSL_ES_300_AO_FRAGMENT, GLSL_ES_300_AO_COMPOSITE_FRAGMENT] {
            for name in common {
                assert!(shader.contains(&format!(" {name};")), "{name}");
            }
        }
        for name in ["u_colour", "u_ao"] {
            let composite = GLSL_ES_300_AO_COMPOSITE_FRAGMENT;
            assert!(composite.contains(&format!(" {name};")), "{name}");
        }
    }
}
