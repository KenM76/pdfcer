//! Print-time colour output: composite grey and black-only.
//!
//! Applied to the page raster after rendering, so the document is never
//! touched. Composite grey is BT.709 luma on the encoded values, which is
//! linear and so exact on premultiplied RGBA. Black-only turns every
//! non-white pixel solid black, keeping its coverage.

/// How the rendered pages are coloured before they are sent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum OutputColor {
    /// As rendered.
    #[default]
    Color,
    /// Composite grey: each pixel's luminance.
    Grayscale,
    /// Every pixel that is not white prints solid black.
    BlackOnly,
}

/// A channel at or above this fraction of its alpha counts as white for
/// [`OutputColor::BlackOnly`], so antialiased white-on-white noise does not
/// print.
const WHITE_FRACTION: u32 = 250;

/// Recolour `rgba` (RGBA8, premultiplied, as `tiny_skia` renders) in place.
/// Returns how many pixels changed.
pub fn apply_output_color(rgba: &mut [u8], mode: OutputColor) -> usize {
    let mut changed = 0;
    if mode == OutputColor::Color {
        return 0;
    }
    for px in rgba.chunks_exact_mut(4) {
        let Ok(px): Result<&mut [u8; 4], _> = px.try_into() else {
            continue;
        };
        let [r, g, b, a] = *px;
        let out = match mode {
            OutputColor::Color => *px,
            OutputColor::Grayscale => {
                let y = (2126 * u32::from(r) + 7152 * u32::from(g) + 722 * u32::from(b) + 5000)
                    / 10_000;
                let y = u8::try_from(y.min(u32::from(a))).unwrap_or(a);
                [y, y, y, a]
            }
            OutputColor::BlackOnly => {
                let white_at = u32::from(a) * WHITE_FRACTION / 255;
                let white = a == 0 || [r, g, b].iter().all(|c| u32::from(*c) >= white_at);
                if white { [r, g, b, a] } else { [0, 0, 0, a] }
            }
        };
        if out != [r, g, b, a] {
            *px = out;
            changed += 1;
        }
    }
    changed
}

#[cfg(test)]
mod tests;
