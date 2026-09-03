//! How much does overprint's RGB round trip actually cost?
//!
//! # Why this number decides whether the n-channel buffer is worth building
//!
//! `overprint.rs` implements ISO 32000-1 §11.7.4.3 Table 149 completely and
//! carefully — the decision logic is not the gap. The gap is what it decides
//! *on*. The compositing buffer is RGBA8, so `interpret.rs` reconstructs the
//! source ink set with `overprint::rgb_to_cmyk` before applying the rule:
//!
//! ```text
//!   painted CMYK  ->  RGB (the framebuffer)  ->  reconstructed CMYK  ->  Table 149
//! ```
//!
//! That middle arrow is exactly the conversion ISO 32000-1 §8.6.5.7 NOTE 2
//! names: a 4→3→4 round trip is *"unnecessary and results in a loss of
//! fidelity in the black component."*
//!
//! **And Table 149 is a per-component test.** Its rules turn on *which
//! components are nonzero* — so an error in the reconstruction is not a
//! slightly-wrong colour, it can be a **different branch of the rule**: a
//! component reconstructed as nonzero overprints when it should knock out,
//! or the reverse.
//!
//! This measures both: how far the reconstructed inks are from the painted
//! ones, and — the part that matters — how often the **zero/nonzero
//! pattern** changes, because that pattern is the rule's actual input.
//!
//! Run: `cargo run -p pdfcer-render --example overprint_roundtrip_probe`

use pdfcer_core::color::cmyk_to_srgb_with;
use pdfcer_core::settings::CmykIntent;
use pdfcer_render::overprint::rgb_to_cmyk;

/// The zero/nonzero pattern Table 149 branches on, as four bits.
fn pattern(v: [f32; 4]) -> [bool; 4] {
    [v[0] > 0.0, v[1] > 0.0, v[2] > 0.0, v[3] > 0.0]
}

fn main() {
    let intent = CmykIntent::default();
    println!("overprint's RGB round trip, under the shipped intent ({intent:?})\n");

    // Ink sets a prepress file actually contains.
    let cases: &[(&str, [f32; 4])] = &[
        ("pure K line art", [0.0, 0.0, 0.0, 1.0]),
        ("registration black", [1.0, 1.0, 1.0, 1.0]),
        ("rich black", [0.6, 0.4, 0.4, 1.0]),
        ("75% K (suite 23.0)", [0.0, 0.0, 0.0, 0.75]),
        ("cyan solid", [1.0, 0.0, 0.0, 0.0]),
        ("magenta solid", [0.0, 1.0, 0.0, 0.0]),
        ("cyan+magenta", [1.0, 1.0, 0.0, 0.0]),
        ("50% cyan alone", [0.5, 0.0, 0.0, 0.0]),
        ("light warm grey", [0.05, 0.04, 0.06, 0.10]),
        ("paper white", [0.0, 0.0, 0.0, 0.0]),
    ];

    println!(
        "  {:<20} {:>22}  {:>22}  {:>6}  rule input",
        "ink set", "painted", "reconstructed", "worst"
    );
    let mut pattern_changes = 0usize;
    let mut worst_overall = 0.0f32;
    for (label, painted) in cases {
        let rgb = cmyk_to_srgb_with(intent, painted[0], painted[1], painted[2], painted[3]);
        let back = rgb_to_cmyk(rgb[0], rgb[1], rgb[2]);
        let worst = (0..4)
            .map(|i| (painted[i] - back[i]).abs())
            .fold(0.0f32, f32::max);
        worst_overall = worst_overall.max(worst);
        let changed = pattern(*painted) != pattern(back);
        if changed {
            pattern_changes += 1;
        }
        println!(
            "  {:<20} {:>22}  {:>22}  {:>6.3}  {}",
            label,
            fmt(*painted),
            fmt(back),
            worst,
            if changed {
                "★ CHANGED — different Table 149 branch"
            } else {
                "same"
            }
        );
    }

    println!("\n  worst single-component error: {worst_overall:.3}");
    println!(
        "  ink sets whose zero/nonzero pattern changed: {pattern_changes} of {}",
        cases.len()
    );
    println!(
        "\n  A changed pattern is not a colour error — it is the rule reading\n\
         \x20 a different row. Table 149 selects the source component for any\n\
         \x20 component whose value is nonzero and the backdrop otherwise, so a\n\
         \x20 component that gains or loses nonzero-ness overprints when it\n\
         \x20 should knock out, or the reverse."
    );

    // Distinctness: two ink sets that differ on press must not arrive at the
    // same reconstruction, or overprint cannot tell them apart at all.
    println!("\n  Distinctness — do different ink sets survive as different?");
    let mut collisions = 0usize;
    for (i, (la, a)) in cases.iter().enumerate() {
        for (lb, b) in cases.iter().skip(i + 1) {
            let ra = {
                let c = cmyk_to_srgb_with(intent, a[0], a[1], a[2], a[3]);
                rgb_to_cmyk(c[0], c[1], c[2])
            };
            let rb = {
                let c = cmyk_to_srgb_with(intent, b[0], b[1], b[2], b[3]);
                rgb_to_cmyk(c[0], c[1], c[2])
            };
            if pattern(ra) == pattern(rb) && pattern(*a) != pattern(*b) {
                collisions += 1;
                println!("    ★ {la} and {lb} reconstruct to the SAME rule input");
            }
        }
    }
    if collisions == 0 {
        println!("    (none among these)");
    } else {
        println!(
            "    {collisions} pair(s) collapsed. Overprint cannot distinguish\n\
             \x20   them, whatever Table 149 says."
        );
    }
}

fn fmt(v: [f32; 4]) -> String {
    format!("{:.2}/{:.2}/{:.2}/{:.2}", v[0], v[1], v[2], v[3])
}
