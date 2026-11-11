//! Bounce settings as a caller outside the crate builds them.

#![cfg(feature = "render")]

use pdfcer_3d::{Bounce, BounceSky, IndirectOptions, MAX_BOUNCE_STRENGTH, RenderOptions};

#[test]
fn a_caller_sets_each_bounce_field_on_the_default() {
    let mut o = IndirectOptions::default();
    assert_eq!(o.bounce, Bounce::ModelLights);
    assert_eq!(o.bounce_sky, BounceSky::Dark);
    o.bounce = Bounce::EveryLight;
    o.bounce_strength = 9.0;
    o.bounce_sky = BounceSky::Background;
    assert_eq!(o.strength(), MAX_BOUNCE_STRENGTH);
    o.bounce_strength = f64::NAN;
    assert_eq!(o.strength(), 0.0);
    let render = RenderOptions {
        indirect: o,
        ..RenderOptions::default()
    };
    assert_eq!(render.indirect.bounce, Bounce::EveryLight);
}
