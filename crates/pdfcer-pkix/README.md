# pdfcer-pkix

Public-key infrastructure for PDF signatures: DER/ASN.1 reading, CMS
`SignedData` and X.509 certificate parsing (RFC 5652, RFC 5280),
certificate-chain building and validation, revocation evidence (CRL, OCSP)
and the trust store. Depends on `pdfcer-model` for its hash and public-key
primitives and for reading trust-store files.

Part of [pdfcer](https://github.com/KenM76/pdfcer), an MIT-licensed PDF engine
and command-line tool. `pdfcer-core` re-exports its `trust_chain` and `trust_store`
modules. The version follows
the pdfcer workspace and the API is not yet stable (0.x).

No GUI or network dependency. The crate denies Clippy's `unwrap_used`,
`expect_used`, `panic`, `indexing_slicing`, `unreachable`, `todo` and
`unimplemented` lints; each remaining exemption is listed, with the reason it
cannot panic at its site, in the repository's `tools/panic-allows-baseline.txt`.

Licence: MIT.
