# pdfcer-color

Device colour-space conversion (ISO 32000-1 8.6.4), the calibrated CMYK table
and rendering intents (8.6.5.8).

Part of [pdfcer](https://github.com/KenM76/pdfcer), an MIT-licensed PDF engine
and command-line tool. `pdfcer-core` re-exports this crate. The version follows
the pdfcer workspace and the API is not yet stable (0.x).

No GUI or network dependency. The crate denies Clippy's `unwrap_used`,
`expect_used`, `panic`, `indexing_slicing`, `unreachable`, `todo` and
`unimplemented` lints; each remaining exemption is listed, with the reason it
cannot panic at its site, in the repository's `tools/panic-allows-baseline.txt`.

Licence: MIT.
