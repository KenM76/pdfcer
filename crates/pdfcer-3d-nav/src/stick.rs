//! Shaping a thumbstick's tilt into a walking speed.

/// The tilt a resting thumb may give, ignored.
const DEAD: f32 = 0.08;

/// A stick's tilt, -1 to 1, shaped for fine control: a small dead zone,
/// then the square of the rest, so half a tilt is a quarter speed. A tilt
/// past ±1 counts as ±1 and NaN as no tilt, so the result is always in
/// `[-1, 1]`.
///
/// ```
/// use pdfcer_3d_nav::shaped;
/// assert_eq!(shaped(0.05), 0.0);
/// assert_eq!(shaped(-1.0), -1.0);
/// assert_eq!(shaped(f32::NAN), 0.0);
/// ```
pub fn shaped(tilt: f32) -> f64 {
    if tilt.is_nan() {
        return 0.0;
    }
    let tilt = tilt.clamp(-1.0, 1.0);
    let rest = ((tilt.abs() - DEAD) / (1.0 - DEAD)).max(0.0);
    f64::from(tilt.signum() * rest * rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stick_ignores_a_resting_thumb_and_eases_in() {
        assert!(shaped(0.05).abs() < f64::EPSILON);
        assert!((shaped(1.0) - 1.0).abs() < 1e-6);
        assert!((shaped(-1.0) + 1.0).abs() < 1e-6);
        assert!(shaped(0.5) < 0.3);
    }

    #[test]
    fn a_stick_reading_past_its_range_or_nan_stays_in_range() {
        assert_eq!(shaped(f32::NAN), 0.0);
        assert_eq!(shaped(f32::INFINITY), 1.0);
        assert_eq!(shaped(-7.0), -1.0);
    }
}
