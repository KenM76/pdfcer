//! Grayscale image input and the binarize/deskew/component-labelling stages.

pub mod binarize;
pub mod components;
pub mod deskew;

/// A borrowed grayscale image: row-major, one byte per pixel, `width * height` long.
pub struct Gray<'a> {
    pub width: u32,
    pub height: u32,
    pub data: &'a [u8],
}
