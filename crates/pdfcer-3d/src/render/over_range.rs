//! [`OverRange`]: what a pixel brighter than full white draws as, the same
//! rule on the CPU path and in [`crate::GLSL_ES_300_FRAGMENT`].

/// What a lit colour with a channel above full white draws as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum OverRange {
    /// [`OverRange::TowardWhite`] for a picture drawn in lux (the options
    /// [`crate::Exposure::Fixed`] gives), else [`OverRange::Clip`].
    #[default]
    Auto,
    /// Each channel stops at full white on its own, so a warm colour over
    /// range turns more saturated: the picture this crate always drew.
    Clip,
    /// The colour is scaled down until its brightest channel is full white,
    /// then mixed with white until its three channels hold the light they
    /// held before, up to all three full ([`toward_white`]): a warm lamp
    /// lens too bright to show reads near white, as a camera sees it.
    TowardWhite,
}

impl OverRange {
    /// Every value, in declaration order.
    pub const ALL: [OverRange; 3] = [OverRange::Auto, OverRange::Clip, OverRange::TowardWhite];

    /// Whether a frame drawn with `self` rolls toward white;
    /// [`OverRange::Auto`] does not, since [`crate::Exposure::lighting`]
    /// replaces it in the options of a picture drawn in lux. The shader's
    /// `u_toward_white` ([`crate::FrameLights::toward_white`]).
    #[must_use]
    pub fn rolls(self) -> bool {
        self == OverRange::TowardWhite
    }

    /// `self`, with [`OverRange::Auto`] read as a picture drawn in lux.
    pub(crate) fn in_lux(self) -> OverRange {
        match self {
            OverRange::Auto => OverRange::TowardWhite,
            other => other,
        }
    }

    /// The name the CLI's `--over-range` takes.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            OverRange::Auto => "auto",
            OverRange::Clip => "clip",
            OverRange::TowardWhite => "toward-white",
        }
    }
}

/// [`OverRange::TowardWhite`] of linear RGB `c`, full white 1. With `m` its
/// brightest channel and `S` its sum: `c` unchanged when `m ≤ 1`, else
/// `(1 − t)·c/m + t`, `t` chosen so the sum is `min(S, 3)`. The channels'
/// total is kept, not a luminance (no colour-science weights). Negative
/// and NaN channels count as 0. Continuous at `m = 1`.
///
/// ```
/// use pdfcer_3d::toward_white;
/// assert_eq!(toward_white([0.5, 0.2, 0.1]), [0.5, 0.2, 0.1]);
/// assert_eq!(toward_white([4.0, 3.0, 2.0]), [1.0; 3]);
/// let [r, g, b] = toward_white([1.5, 1.0, 0.5]);
/// assert!((r + g + b - 3.0).abs() < 1e-12);
/// assert!(r >= g && g >= b && r <= 1.0);
/// ```
#[must_use]
pub fn toward_white(c: [f64; 3]) -> [f64; 3] {
    let c = c.map(|v| if v > 0.0 { v } else { 0.0 });
    let m = c[0].max(c[1]).max(c[2]);
    if m <= 1.0 {
        return c;
    }
    if !m.is_finite() {
        return [1.0; 3];
    }
    let base = c.map(|v| v / m);
    let held: f64 = base.iter().sum();
    if held >= 3.0 {
        return [1.0; 3];
    }
    let t = ((c[0] + c[1] + c[2]).min(3.0) - held) / (3.0 - held);
    base.map(|v| ((1.0 - t) * v + t).min(1.0))
}

/// `v`, linear with full white 255, as 8-bit channels: rolled toward white
/// when `roll`, else each clipped.
pub(crate) fn to_8_bit(v: [f64; 3], roll: bool) -> [u8; 3] {
    let v = if roll {
        toward_white(v.map(|c| c / 255.0)).map(|c| c * 255.0)
    } else {
        v
    };
    v.map(|c| c.clamp(0.0, 255.0).round() as u8)
}

/// The GLSL twin of [`toward_white`], for the fragment shader.
macro_rules! toward_white_glsl {
    () => {
        r"// toward_white() of render/over_range.rs, applied under u_toward_white.
vec3 toward_white(vec3 c) {
    c = max(c, vec3(0.0));
    float m = max(c.r, max(c.g, c.b));
    if (m <= 1.0) return c;
    vec3 b = c / m;
    float held = b.r + b.g + b.b;
    if (held >= 3.0) return vec3(1.0);
    float t = (min(c.r + c.g + c.b, 3.0) - held) / (3.0 - held);
    return min(mix(b, vec3(1.0), t), vec3(1.0));
}

"
    };
}
pub(crate) use toward_white_glsl;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_over_range_colour_keeps_its_light_and_its_order() {
        for c in [
            [1.5, 1.0, 0.5],
            [1.18, 0.86, 0.51],
            [2.0, 0.0, 0.0],
            [1.01; 3],
        ] {
            let out = toward_white(c);
            let want = (c[0] + c[1] + c[2]).min(3.0);
            assert!(
                (out.iter().sum::<f64>() - want).abs() < 1e-9,
                "{c:?} {out:?}"
            );
            assert!(out.iter().all(|&v| (0.0..=1.0).contains(&v)));
            assert!(out[0] >= out[1] && out[1] >= out[2], "{out:?}");
        }
        let near = toward_white([1.0 + 1e-9, 0.5, 0.2]);
        assert!((near[1] - 0.5).abs() < 1e-6, "continuous at full white");
        assert_eq!(toward_white([f64::NAN, -1.0, 0.3]), [0.0, 0.0, 0.3]);
        assert_eq!(toward_white([f64::INFINITY, 0.0, 0.0]), [1.0; 3]);
    }

    #[test]
    fn a_warm_lens_over_range_reads_near_white_not_amber() {
        let clip = to_8_bit([300.0, 220.0, 130.0], false);
        let roll = to_8_bit([300.0, 220.0, 130.0], true);
        assert_eq!(clip, [255, 220, 130]);
        assert_eq!(roll[0], 255);
        // The light the clip throws away is kept, as a whiter colour.
        assert_eq!(roll.map(u32::from).iter().sum::<u32>(), 650);
        assert!(roll[2] > clip[2] + 40, "{roll:?}");
        assert_eq!(to_8_bit([100.0, 50.0, 25.0], true), [100, 50, 25]);
    }
}
