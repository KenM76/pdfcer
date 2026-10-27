//! Regenerates the baked sRGB → CMYK grids behind `srgb_to_cmyk_fast`.
//! Run from the workspace root, in a release build.

use pdfcer_color::color::{CmykIntent, solve_inverse_grid};

fn main() -> std::io::Result<()> {
    let dir = "crates/pdfcer-color/src/color";
    for (intent, name) in [
        (CmykIntent::Calibrated, "inverse_grid_calibrated.bin"),
        (CmykIntent::NeutralBlack, "inverse_grid_neutral_black.bin"),
    ] {
        std::fs::write(format!("{dir}/{name}"), solve_inverse_grid(intent))?;
        println!("wrote {name}");
    }
    Ok(())
}
