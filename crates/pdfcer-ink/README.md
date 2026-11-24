# pdfcer-ink

Composites paints into subtractive colorant planes and evaluates ink
coverage rules. Part of [pdfcer](https://github.com/KenM76/pdfcer).

- Input: a device-pixel page size, the colorants (four process inks, then
  spots by name), and paints: a coverage mask or span list, a tint per
  colorant it sets, a constant alpha, a blend mode, overprint and overprint
  mode, inside isolated or knockout transparency groups.
- Output: one plane per colorant holding the ink as it lands on the sheet,
  total-area-coverage and rich-black masks with flagged counts, or a refusal
  when the page is over the caller's memory or colorant ceiling.

The crate is pure Rust and platform-free, has no network or GUI dependency,
and builds for `wasm32-unknown-unknown`. It knows nothing about PDF files:
the caller supplies device-space coverage and tints.

Licence: MIT.
