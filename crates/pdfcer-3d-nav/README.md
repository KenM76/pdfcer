# pdfcer-3d-nav

Navigation for an interactive 3D model viewer, with no GUI: the orbit
camera, first-person walking over the model's surface, the lens policy, the
operator's up axis, a file's saved view, sphere fit and visibility-filtered
picking. Every shell drives this one copy; it depends on `pdfcer-3d` only.

Part of [pdfcer](https://github.com/KenM76/pdfcer), an MIT-licensed PDF engine
and command-line tool. The version follows the pdfcer workspace and the API is
not yet stable (0.x).

No GUI or network dependency; builds for wasm32.

Licence: MIT.
