# Decision 180 — A default 3D poster rendered from the model

- **Date:** 2026-10-02
- **Status:** DECIDED; shipped in `Pass 440.0`.
- **Authored by:** `pdfcer-engineer`.
- **Trigger:** pdfcer-gui requests G091 (render a default poster when none is
  supplied) and G092 (`EditSession::set_3d_poster`).
- **Clauses:** ISO 32000-1 §13.6.2 Table 298 (`/AP` is required on a 3D
  annotation; it is what prints and what a reader without 3D shows).

## 1. `pdfcer-core` depends on `pdfcer-3d`, optionally

`add_3d_annotation` returns the poster it drew, and the poster is an
`ImportedImage`, a core type. `pdfcer-3d` cannot return one without depending
on core, so the edge runs core → 3d. It sits behind core's `3d` feature,
**default on**, enabling `pdfcer-3d/render`.

- `pdfcer-3d` is in-workspace and MIT. It is pure Rust, and its only
  dependencies are `thiserror` and `flate2`, which core already has.
- It has no GUI, network or thread dependency, and is wasm32-clean. Both core
  invariants hold; check with `cargo tree -p pdfcer-core`.
- With the feature off, the verb still works. It draws the placeholder and
  reports `PlaceholderReason::NoDecoder`.
- A consumer that sets `default-features = false` on `pdfcer-core` must enable
  `pdfcer-core/3d` itself, as the CLI does.

`pdfcer_3d::assemble` and `render_default_view` moved out of the CLI's
`3d-mesh` path into `pdfcer-3d`. `3d-mesh`, `3d-render` and the poster now
assemble a model the same way.

## 2. What the default view is

- **Direction and up:** `DEFAULT_VIEW_DIRECTION` = (−1, 1, −1), which looks
  down on the model from above its front-right corner, with z up. This is
  `3d-render`'s `iso` view.
- **Projection:** a 30° perspective camera fitted to the meshes.
- **Colour:** each mesh in its colour from the model tree, grey where the tree
  gives none, on an opaque white background.
- **Size:** 2 px per point of `/Rect`, scaled down so the long side is at
  most 2048 px.

The file's own views (`/VA`, PRC views), lights and textures are not read.
That makes the poster pdfcer's guess at a useful view, so it is an inference
under rule 4:

- `ThreeDPoster::Rendered(RenderedPoster)` carries what was drawn and what was
  left out.
- The CLI prints an `inferred:` line.
- pdfcer-gui discloses it off-canvas.

The poster image is drawn exactly as it will be saved.

## 3. When the placeholder is drawn instead

The placeholder is the frame and wireframe cube, with `/C`, so a resize can
redraw it. It is drawn in these cases, each with a `PlaceholderReason`:

| Case | Reason |
|---|---|
| `render_poster = false` | `Requested` |
| A U3D model (pdfcer decodes only PRC) | `NotDecoded { format }` |
| Built without the `3d` feature | `NoDecoder` |
| A PRC model that does not parse, has only compressed meshes pdfcer cannot rebuild, or has no triangles | `Undecodable { why }` |

None of these is an error.

A rendered poster carries no `/C`. On resize it is re-fitted like a supplied
image (`three_d_poster_rebuild`), not re-rendered.

## 4. `set_3d_poster`

`set_3d_poster` takes one image and records one undo entry. It writes:

- a new image XObject;
- a new appearance stream, which fits the image the same way as a supplied
  poster in `add_3d_annotation`;
- the annotation, with only `/AP /N` replaced.

The 3D stream, `/3DD`, `/3DA`, the views and `/C` are untouched. The previous
appearance stream stays in the file, unreferenced, and is not deleted. Under
the minimal-diff rule, the edit does not reach into objects it did not author.

A RichMedia model has no `/3D` poster, so the verb refuses it as
`NotA3dAnnotation`.
