# pdfcer-vec3

Three-component vector arithmetic on `[f64; 3]`: add, sub, scale, dot,
cross, and a length and normalize that scale before squaring so huge and tiny
vectors do not overflow. No dependencies. Shared by `pdfcer-3d` (as
`pdfcer_3d::vec3`), `pdfcer-3d-nav` and `pdfcer-core`.

Part of [pdfcer](https://github.com/KenM76/pdfcer), an MIT-licensed PDF engine
and command-line tool. The version follows the pdfcer workspace and the API is
not yet stable (0.x).

No GUI or network dependency; builds for wasm32.

Licence: MIT.
