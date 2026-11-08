# pdfcer-jpeg-remux

Rewrites a baseline or extended-sequential JPEG (8-bit, Huffman) whose
components are coded in separate scans into the same image coded as one
interleaved scan. The quantized coefficients are decoded and re-encoded
unchanged, so a correct decoder produces identical pixels from either file.

Why: ITU-T T.81 allows both layouts. Some decoders mis-read the
non-interleaved one when chroma is subsampled; `zune-jpeg` 0.5.15 is one.
`jpeg-encoder` writes that layout whenever optimized Huffman tables are on.
Run a codestream through `interleave` before decoding, and decode its output
when it returns `Some`.

```rust
let jpeg: &[u8] = &[]; // a JPEG codestream
let fixed = pdfcer_jpeg_remux::interleave(jpeg, 1 << 20).ok().flatten();
let to_decode = fixed.as_deref().unwrap_or(jpeg);
# let _ = to_decode;
```

`Ok(None)` means the stream does not need the rewrite: progressive,
arithmetic or lossless coding, one component, no chroma subsampling, or a
single scan already. The output uses the typical Huffman tables of T.81
Annex K.3 and no restart markers; every other marker segment before the
first scan is kept byte-for-byte.

Part of [pdfcer](https://github.com/KenM76/pdfcer), an MIT-licensed PDF engine
and command-line tool. The version follows the pdfcer workspace and the API is
not yet stable (0.x). No GUI or network dependency; builds for wasm32.

Licence: MIT.
