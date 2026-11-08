# ocrcer-core

The OCRcer recogniser: an OCR engine for printed documents and CAD drawing
text. It segments a page into lines, words and glyphs, matches each glyph
against a prototype bank, decodes with a lexicon and character bigrams, and
returns every word with its box and a calibrated confidence.

- `#![forbid(unsafe_code)]`, zero dependencies, builds for
  `wasm32-unknown-unknown`.
- MIT, code and model alike.
- Narrow by design: printed Latin text. Not handwriting, scene text or
  non-Latin scripts.

## The model is not in this crate

The engine reads a `.ocrw` model file at run time. Download `ocrcer.ocrw`
from the [GitHub releases](https://github.com/KenM76/ocrcer/releases) and
check its sha256 against the release notes. This crate version reads model
container version 1 (model release `v0.1.0`, sha256
`1aed06b27ca842fb1fe922849ac5929adf278c95a38bf3eda251ccc0adb5cd04`). A file
with a different container version is refused at load, not partially read.

## Use

```rust,no_run
let model = std::fs::read("ocrcer.ocrw")?;
let engine = ocrcer_core::Engine::from_bytes(&model)?;

// 8-bit greyscale, row-major, top-down, width * height bytes.
let (width, height, pixels): (u32, u32, Vec<u8>) = load_page();
for word in engine.recognize_bytes(width, height, &pixels)? {
    println!("{} {:.2} {:?}", word.text, word.confidence, word.rect);
}
```

Boxes are in the input image's pixel coordinates, y-down. `confidence` is in
`0.0..=1.0`: the match margin against the nearest rival of a different class,
passed through a calibration curve and averaged geometrically over the word.
Two look-alike glyphs that both match well still report low confidence.

A blank or unreadable page returns an empty list, not an error. Errors come
from loading a bad model file (wrong magic, truncation, unknown version, CRC
mismatch) or from a pixel buffer whose length is not `width * height`.

## Features

- `parallel`: reserved for multi-threaded recognition; currently adds nothing.

Project, architecture and measured accuracy:
<https://github.com/KenM76/ocrcer>.
