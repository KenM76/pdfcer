//! `print --separations`: what the plates came out as, on stderr. The
//! plates themselves are `pdfcer_render::plates`.

// Called only by the `#[cfg(windows)]` `cmd_print`; pure, so compiled and
// tested everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

use pdfcer_render::plates::Plates;
use pdfcer_render::tiny_skia::Pixmap;

/// Say which plates page `index` (0-based) printed as, and hand back the
/// images to print in order.
pub(crate) fn report_plates(index: usize, plates: Plates) -> Vec<Pixmap> {
    match plates {
        Plates::Separated(plates) => {
            let names: Vec<&str> = plates.iter().map(|p| p.ink.as_str()).collect();
            eprintln!(
                "pdfcer: page {}: {} plate(s): {}. Plate greys approximate ink density.",
                index + 1,
                plates.len(),
                names.join(", ")
            );
            plates.into_iter().map(|p| p.pixmap).collect()
        }
        Plates::Composite(pixmap) => {
            eprintln!(
                "pdfcer: page {}: separations unavailable (the ink buffer was refused for memory); printed as the composite page.",
                index + 1
            );
            vec![pixmap]
        }
        // `Plates` is non_exhaustive; nothing else is produced today.
        _ => Vec::new(),
    }
}
