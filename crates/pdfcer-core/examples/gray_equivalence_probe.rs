//! Do suite patch 23.0's four "same black" panels actually render the same
//! colour through pdfcer's sRGB path?
//!
//! # The question, and why it is pdfcer's rather than iccce's
//!
//! `iccce`'s `note_gray_black_routing_is_yours.md` (2026-08-17, third
//! amendment 2026-08-18) reclassified suite patch 23.0 from a colour-management
//! problem to a **PDF device-routing** one, and handed it back. The patch
//! draws one tone four ways and they must match:
//!
//! | panel | value | operator |
//! |---|---|---|
//! | `DeviceGray` | 25 % | `.25 g` |
//! | `DeviceCMYK` | 0 / 0 / 0 / 75 | `0 0 0 .75 k` |
//! | `Separation` | 75 | `.75 scn` |
//! | `DeviceN` | 75 | `.75 scn` |
//!
//! Those are not arbitrary: ISO 32000-1 cl. 10.3.3 says the CMYK equivalent
//! of a gray is `c = m = y = 0`, `k = 1 − gray`, and `1 − 0.25 = 0.75`. The suite
//! authored the patch on the device-space rule.
//!
//! ★ **Earlier iccce documents circulated 50 % / 0-0-0-50 for these panels
//! and those figures are WRONG** — corrected at source before they could
//! become a fixture here. The values above were read two independent ways:
//! from the patch's own decompressed content stream, and from the readme.
//!
//! # What this probe actually settles
//!
//! Clause 10.3.2's routing rule is conditioned on *"if the native device
//! colour space is CMYK"*. **pdfcer's rasteriser targets sRGB** — `color/mod.rs`
//! exposes `gray_to_srgb`, `rgb_to_srgb` and `cmyk_to_srgb` and nothing else —
//! so that condition never holds and the cl. 10.3.2 / cl. 10.4.2.1 ambiguity
//! iccce asked about does not arise for the current renderer.
//!
//! What *does* arise is narrower and testable here: **two independent
//! transfer functions have to agree at one point.** `gray_to_srgb(0.25)` and
//! `cmyk_to_srgb(0, 0, 0, 0.75)` are separate code paths — one a transfer
//! curve, the other a colorimetric table — and nothing makes them consistent
//! by construction. If they disagree, patch 23.0 fails in pdfcer for a reason
//! that has nothing to do with ICC.
//!
//! Run: `cargo run -p pdfcer-core --example gray_equivalence_probe`

use pdfcer_core::color::{cmyk_to_srgb, cmyk_to_srgb_with, gray_to_srgb};
use pdfcer_core::settings::CmykIntent;

fn to8(c: [f32; 3]) -> [u8; 3] {
    [
        (c[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (c[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (c[2].clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

fn main() {
    println!("suite 23.0 — the four-way gray equivalence, through pdfcer's sRGB path\n");

    // The patch's own values.
    let gray = gray_to_srgb(0.25);
    let cmyk = cmyk_to_srgb(0.0, 0.0, 0.0, 0.75);
    println!("  DeviceGray  .25 g        -> {:?}  {:?}", gray, to8(gray));
    println!("  DeviceCMYK  0 0 0 .75 k  -> {:?}  {:?}", cmyk, to8(cmyk));

    let d = to8(gray)
        .iter()
        .zip(to8(cmyk).iter())
        .map(|(a, b)| i32::from(*a) - i32::from(*b))
        .collect::<Vec<_>>();
    println!("\n  8-bit difference: {d:?}");
    if d.iter().all(|v| *v == 0) {
        println!("  => the two paths AGREE at this point.");
    } else {
        println!(
            "  => they DISAGREE by up to {} levels. Patch 23.0 cannot pass\n\
             \x20    while that holds, and the cause is entirely inside pdfcer:\n\
             \x20    two independent transfer functions, no ICC involved.",
            d.iter().map(|v| v.abs()).max().unwrap_or(0)
        );
    }

    // Every intent, since `cmyk_to_srgb_with` exposes the choice and the
    // answer may depend on it.
    println!("\n  DeviceCMYK 0/0/0/.75 under each intent:");
    for intent in [CmykIntent::NeutralBlack, CmykIntent::Calibrated] {
        let c = cmyk_to_srgb_with(intent, 0.0, 0.0, 0.0, 0.75);
        let diff = to8(gray)[0] as i32 - to8(c)[0] as i32;
        println!(
            "    {:<18?} -> {:?}   gray-minus-cmyk on R: {diff:+}",
            intent,
            to8(c)
        );
    }

    // The whole ramp, because agreeing at one point and nowhere else would be
    // a coincidence rather than a property.
    println!("\n  Across the ramp (gray g vs cmyk k = 1-g), 8-bit R channel:");
    println!(
        "    {:>6}  {:>5} {:>5}  {:>5}",
        "gray", "gray", "cmyk", "diff"
    );
    let mut worst = 0i32;
    for i in 0..=10 {
        let g = i as f32 / 10.0;
        let a = to8(gray_to_srgb(g))[0] as i32;
        let b = to8(cmyk_to_srgb(0.0, 0.0, 0.0, 1.0 - g))[0] as i32;
        worst = worst.max((a - b).abs());
        println!("    {g:>6.2}  {a:>5} {b:>5}  {:>+5}", a - b);
    }
    println!("\n  worst 8-bit divergence across the ramp: {worst}");
}
