# pdfcer-emf

Writes a PDF page as a vector Enhanced Metafile ([MS-EMF]): for printing
through GDI, for the Windows clipboard, and as an export format. Part of
[pdfcer](https://github.com/KenM76/pdfcer).

- `export_emf` / `export_emf_view`: one page to EMF bytes, with an outcome
  that says what had to become pixels and why.
- `print_metafile` / `print_export_with`: the strict print form, which
  refuses a page that would print differently from its raster.
- `export_emf_plates`: one vector metafile per separation ink.

The crate is pure Rust and platform-free (it writes bytes; playing them
back is the caller's business), has no network or GUI dependency, and
builds for `wasm32-unknown-unknown`. It reads pages through `pdfcer-render`
and is released at the same version; take both at one version.

Licence: MIT.
