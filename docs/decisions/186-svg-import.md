# Decision 186 — SVG import places vector content, via usvg

- **Date:** 2026-10-02
- **Status:** DECIDED; shipped in `Pass 445.0`.
- **Authored by:** `pdfcer-engineer`.
- **Trigger:** request G093 (pdfcer-gui): an SVG file cannot be placed on
  a page.

## 1. Parser: usvg, not a hand-written one

`usvg` 0.45.1 (Apache-2.0 OR MIT), `default-features = false`, behind the
core feature `svg-import` (on by default; forwarded by the CLI). It
resolves CSS, expands `use`, converts shapes to paths and units to
absolute user space, so the importer translates one simplified tree, not
the SVG grammar. Without `text`, no font stack is pulled in. It is
wasm32-clean and makes no network call. resvg renders the same tree, so it
is an exact parity oracle (a render dev-dependency only).

## 2. Output: one Form XObject, never a raster

The drawing is a Form XObject (ISO 32000-1 §8.10) with `/BBox [0 0 w h]`
in SVG px and a y-flip as the base matrix. Each paint is `q [gs] cm colour
path op Q`.

| SVG | PDF |
|---|---|
| linear / radial gradient | shading pattern, axial / radial (§8.7.4.5.3–4), `/Extend [true true]`; stops become a type 3 stitch of type 2 functions (§7.10.4) |
| `spreadMethod` repeat / reflect | unrolled over the periods covering the shape, at most 64; beyond that pad, disclosed |
| varying stop-opacity | Luminosity soft mask of a DeviceGray shading (§11.6.5.2) |
| `<pattern>` | tiling pattern, PaintType 1 (§8.7.3); recursion depth 4 |
| clip-path, mask, group opacity | clip operators; soft masks; transparency groups (§11) |
| embedded raster (`data:` URI) | image XObject via the `add_image` staging path |
| filter, text, external image | **skipped and named** in `SvgImportNotes`; a filtered element draws unfiltered; nothing is fetched |

## 3. Placement

`EditSession::add_svg` appends a `q sx 0 0 sy llx lly cm /Name Do Q`
stream (§7.8.2). `add_svg_stamp` makes the form a `/Stamp`'s `/AP /N`
(§12.5.6.12). Each is one undo entry. Both STRETCH to the rectangle; the
fit (contain by default, `--stretch`, `--natural` at 96 px/in) is the
shell's choice, matching `add-image`.

## 4. Ceilings (ARCHITECTURE §10)

Input 32 MiB; SVGZ inflated to 64 MiB; element nesting 256 counting
`use`/clip/mask/pattern references (pre-scanned before usvg, which
recurses); generated content 64 MiB. Each is refused by name. Fuzzed by
`fuzz/fuzz_targets/svg_import.rs`.

## 5. Known gap

pdfcer-render does not paint `PatternType 1`. A placed SVG pattern is
correct in the file (pdfium matches resvg), but pdfcer's own canvas shows
it blank until the renderer gains tiling patterns.
