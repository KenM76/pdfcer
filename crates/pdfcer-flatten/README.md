# pdfcer-flatten

Resolves PDF transparency over flat colours into opaque vector regions. Part
of [pdfcer](https://github.com/KenM76/pdfcer).

- Input: a tree of paints (a polygon, a flat colour in the blending space, a
  constant alpha, a blend mode, an optional overprint) inside isolated,
  knockout or non-isolated transparency groups.
- Output: the atomic regions of the overlay, each with the colour and alpha
  ISO 32000-2 §11.4.8 composites there, or a refusal when the scene is over
  the caller's complexity ceiling.

The crate is pure Rust and platform-free, has no network or GUI dependency,
and builds for `wasm32-unknown-unknown`. It knows nothing about PDF files:
the caller supplies device-space geometry and colours.

Licence: MIT.
