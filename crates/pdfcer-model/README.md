# pdfcer-model

The pdfcer COS model layer: objects, lexer, parser, filters, xref tables and
streams, object streams, recovery, `Document`, the read-only `DocumentView`,
the writer (full and incremental save), standard-handler crypto,
content-stream parsing and the page tree. Depends on nothing else in the
workspace.

Part of [pdfcer](https://github.com/KenM76/pdfcer), an MIT-licensed PDF engine
and command-line tool. `pdfcer-core` re-exports this crate. It builds for
`wasm32-unknown-unknown`. The version follows
the pdfcer workspace and the API is not yet stable (0.x).

No GUI or network dependency. The crate denies Clippy's `unwrap_used`,
`expect_used`, `panic`, `indexing_slicing`, `unreachable`, `todo` and
`unimplemented` lints; each remaining exemption is listed, with the reason it
cannot panic at its site, in the repository's `tools/panic-allows-baseline.txt`.

Licence: MIT.
