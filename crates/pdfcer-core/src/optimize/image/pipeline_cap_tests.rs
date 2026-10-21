#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::pipeline::capped;
use super::pipeline_tests::{AT_100, doc, grey, plan, skip};
use crate::optimize::{ImagePatch, SettingSource, SkipReason};

fn size_after(actions: &str, content: &str) -> Option<(u32, u32)> {
    let (_, p) = plan(doc(content, &[grey("")]), &[(actions, None)]);
    let c = p.changes.iter().find(|c| c.id.num == 5)?;
    Some(c.image.as_ref().unwrap().size_after)
}

#[test]
fn an_unplaced_image_is_capped_to_max_pixels_keeping_its_aspect() {
    assert_eq!(size_after("image(max-pixels=10000)", ""), Some((100, 100)));
    let (_, p) = plan(
        doc("", &[grey("")]),
        &[("image(ppi=72,max-pixels=10000)", None)],
    );
    let img = p.changes[0].image.as_ref().unwrap();
    assert_eq!(img.settings.max_pixels.unwrap().value, 10000);
    assert!(matches!(
        img.settings.max_pixels.unwrap().source,
        SettingSource::Streams | SettingSource::Images
    ));
    // At or under the cap: nothing to do.
    assert_eq!(size_after("image(max-pixels=90000)", ""), None);
    // Without the cap an unplaced image is still skipped with the reason.
    let (_, p) = plan(doc("", &[grey("")]), &[("image(ppi=72)", None)]);
    assert_eq!(skip(&p, 5), Some(SkipReason::Unplaced));
}

#[test]
fn max_pixels_leaves_a_placed_image_to_ppi() {
    // 300 px over 100 pt is 216 ppi: under no ppi target, the cap does not apply.
    assert_eq!(size_after("image(max-pixels=10000)", AT_100), None);
    assert_eq!(
        size_after("image(ppi=72,max-pixels=10)", AT_100),
        Some((100, 100))
    );
}

#[test]
fn the_cap_floors_each_axis_and_keeps_at_least_one_pixel() {
    assert_eq!(capped(400, 100, 10000), Some((200, 50)));
    assert_eq!(capped(301, 7, 1000), Some((207, 4)));
    assert_eq!(capped(7, 5000, 50), Some((1, 50)));
    assert_eq!(capped(10_000, 1, 10), Some((10, 1)));
    assert_eq!(capped(10, 10, 100), None);
    for (w, h, max) in [(301, 7, 1000), (999, 333, 12345), (7, 5000, 50)] {
        let (a, b) = capped(w, h, max).unwrap();
        assert!(a * b <= max, "{w}x{h} cap {max} -> {a}x{b}");
    }
}

#[test]
fn max_pixels_parses_prints_and_rejects_zero() {
    let p = ImagePatch::parse("image(max-pixels=250000)").unwrap();
    assert_eq!(p.max_pixels, Some(250_000));
    assert_eq!(p.to_string(), "image(max-pixels=250000)");
    assert!(ImagePatch::parse("image(max-pixels=0)").is_err());
    assert!(ImagePatch::parse("image(max-pixels=lots)").is_err());
}
