//! The depth prepass's reference shaders: the visible depth of every opaque
//! draw, so the main pass skips lighting fragments that end up hidden.
//!
//! The main fragment shader writes logarithmic `gl_FragDepth`, which turns
//! off the hardware's early depth test: every fragment of every overlapping
//! surface runs the whole lamp loop before the depth test throws it away.
//! Rasterised (vertex-side) log depth would restore early-Z, but it is
//! interpolated linearly across a triangle and is wrong by centimetres on a
//! long wall, bringing back the z-fighting log depth exists to stop. So the
//! early-out is done in the shader instead, against a prepass depth texture
//! the main pass samples, and the main pass's own depth test and depth
//! writes are unchanged: the image is the same with or without a prepass.
//!
//! Host contract, per frame:
//!
//! 1. Allocate a `DEPTH_COMPONENT24` or `DEPTH_COMPONENT32F` texture the
//!    size of the main pass's target in pixels (single-sample even when the
//!    main target is multisampled), `NEAREST` both ways, no compare mode. A
//!    16-bit depth texture is not allowed: its rounding exceeds the
//!    shader's tolerance and visible fragments would be dropped.
//! 2. Draw into it, depth cleared to 1.0, `LESS`, depth writes on, no
//!    colour attachment (or colour masked off), no culling, every draw whose
//!    [`crate::Draw::translucent`] is false, with [`GLSL_ES_300_PREPASS_VERTEX`]
//!    / [`GLSL_ES_300_PREPASS_FRAGMENT`] and the main pass's `u_view`,
//!    `u_projection`, `u_depth_range` and `u_orthographic`. Leaving a draw
//!    out is safe (fewer fragments are skipped); putting in anything that
//!    does not hide what is behind it (a translucent draw, a line) is not.
//! 3. Draw the main pass exactly as without a prepass, with the depth
//!    texture bound as `u_prepass_depth`, `u_prepass` true and
//!    `u_prepass_spread` 0 for a single-sample target, 1 for a multisampled
//!    one.
//!
//! The main pass discards a fragment only when its own surface, carried
//! along its screen-space depth slope to its pixel and to every pixel within
//! `u_prepass_spread`, is behind the prepass's nearest-surface depth at each
//! of them by more than 2⁻²⁰ plus a quarter of that slope. A fragment within
//! the tolerance is lit and resolved by the depth test as before.
//!
//! Single-sample targets are exact: on the Bancroft plan (181k triangles,
//! 64 lamps, 1280×800, Intel Arc) the image is pixel-identical and the main
//! pass drops from 9.1 ms to 0.3 ms prepass + 2.8 ms shading. On a
//! multisampled target the prepass sees one depth per pixel, so at a
//! silhouette a surface can show at edge samples of a pixel whose centre a
//! nearer surface owns. Comparing against the nearest depth alone dropped
//! it and left the clear colour as a light outline; carried along its slope,
//! the surface matches its own depth at a neighbour whose centre it does
//! own, and is kept. What can still be dropped is a sliver no pixel centre
//! within the spread sees: on the Bancroft plan at 4× MSAA and spread 1 that
//! is 331 of 1,024,000 pixels, max 44/255 off (against 2,582 and 120 for the
//! nearest-depth test), at 11.4 → 3.2 ms. A host that needs an exact
//! multisampled image (a screenshot) draws it without the prepass.

/// The prepass vertex shader: location 0 position (vec3) from
/// [`crate::DrawData::vertices`], the rest of the vertex ignored; uniforms
/// `u_view` and `u_projection`, as [`crate::GLSL_ES_300_VERTEX`]'s. Its
/// position is computed by the same invariant expression, so the two
/// passes rasterise the same fragments.
pub const GLSL_ES_300_PREPASS_VERTEX: &str = r"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
uniform mat4 u_view;
uniform mat4 u_projection;
out float v_w;
invariant gl_Position;

void main() {
    gl_Position = u_projection * u_view * vec4(a_position, 1.0);
    v_w = gl_Position.w;
}
";

/// The prepass fragment shader: depth only, the same `gl_FragDepth` as
/// [`crate::GLSL_ES_300_FRAGMENT`] (uniforms `u_depth_range`,
/// `u_orthographic`).
pub const GLSL_ES_300_PREPASS_FRAGMENT: &str = r"#version 300 es
precision highp float;
in float v_w;
uniform vec2 u_depth_range;
uniform bool u_orthographic;

void main() {
    gl_FragDepth = u_orthographic
        ? gl_FragCoord.z
        : clamp(log2(v_w / u_depth_range.x) / log2(u_depth_range.y / u_depth_range.x), 0.0, 1.0);
}
";

#[cfg(test)]
// A missing shader marker is the test failure itself.
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::{GLSL_ES_300_FRAGMENT, GLSL_ES_300_VERTEX};

    /// The text from `marker` to the end of its statement.
    fn statement<'a>(shader: &'a str, marker: &str) -> &'a str {
        let at = shader.find(marker).expect(marker);
        let rest = &shader[at..];
        &rest[..rest.find(';').expect("statement end")]
    }

    #[test]
    fn the_prepass_writes_the_main_pass_depth() {
        let main = statement(GLSL_ES_300_FRAGMENT, "return u_orthographic");
        let pre = statement(GLSL_ES_300_PREPASS_FRAGMENT, "gl_FragDepth = ");
        assert_eq!(
            main.trim_start_matches("return "),
            pre.trim_start_matches("gl_FragDepth = ")
        );
        assert!(GLSL_ES_300_FRAGMENT.contains("gl_FragDepth = depth;"));
    }

    #[test]
    fn both_passes_place_vertices_the_same_invariant_way() {
        let position = "gl_Position = u_projection * u_view * vec4(a_position, 1.0)";
        for shader in [GLSL_ES_300_VERTEX, GLSL_ES_300_PREPASS_VERTEX] {
            assert!(shader.contains(position));
            assert!(shader.contains("invariant gl_Position;"));
            assert!(shader.contains("v_w = gl_Position.w;"));
        }
    }

    #[test]
    fn a_neighbour_is_compared_with_this_surface_carried_along_its_slope() {
        let hidden =
            &GLSL_ES_300_FRAGMENT[GLSL_ES_300_FRAGMENT.find("bool prepass_hidden").unwrap()..];
        let hidden = &hidden[..hidden.find("\n}\n").unwrap()];
        assert!(hidden.contains("float here = d + float(x) * slope.x + float(y) * slope.y;"));
        assert!(
            hidden.contains("if (here <= texelFetch(u_prepass_depth, q, 0).r + tol) return false;")
        );
        // The nearest depth over the neighbourhood is not this surface's depth at
        // any of them; testing against it drops silhouette edge samples.
        assert!(!hidden.contains("max("));
        assert!(
            GLSL_ES_300_FRAGMENT.contains("vec2 depth_slope = vec2(dFdx(depth), dFdy(depth));")
        );
    }

    #[test]
    fn the_main_pass_skips_hidden_fragments_before_any_lighting() {
        for name in ["u_prepass", "u_prepass_depth", "u_prepass_spread"] {
            assert!(
                GLSL_ES_300_FRAGMENT.contains(&format!(" {name};")),
                "{name}"
            );
        }
        let main = &GLSL_ES_300_FRAGMENT[GLSL_ES_300_FRAGMENT.find("void main()").unwrap()..];
        let skip = main
            .find("if (prepass_hidden(depth, depth_slope)) discard;")
            .unwrap();
        // Derivatives are taken before the discard, lighting after it.
        assert!(main.find("dFdx").unwrap() < skip);
        for after in ["local(", "lit(f)", "extra(", "finish("] {
            assert!(main.find(after).unwrap() > skip, "{after}");
        }
    }
}
