# Edge antialiasing — the three modes and how to reproduce each

`EdgeAntialiasing` (`crates/pdfcer-render/src/font/mod.rs`), set with
`RenderOptions::with_edge_antialiasing` and on the CLI with
`--edge-antialiasing` (`print`, `render-page`, `export-image`). A raster
setting only: no export writes it into a file.

## `On` — the default for screen rasters (the Acrobat-matching output)

Every shape, clip, coverage mask, pattern mask and image edge is
antialiased by `tiny_skia` at the requested resolution. The interpreter
reads `policy.edge_antialiasing.is_on()` into `Interpreter::edge_aa`
(`interpret.rs`) and passes it to every `fill_path`/`stroke_path`
(`brush.anti_alias`), every `intersect_clip` (`device_clip.rs`), every
coverage-mask `fill_path_fit`, the tiling mask (`interpret/tiling.rs`), and
images through `image_edge_needs_antialiasing`.

Shapes are composited one after another, so two shapes meeting mid-pixel
each give that pixel half coverage, and `½ over ½` lets a quarter of the
backdrop through: a faint seam the page does not contain. On the licensed
print test file's overprint, knockout and isolated-group patches this
draws the failure "X" at screen resolutions even though the overprint
logic is correct. Acrobat draws the same seams at the same places, so this
mode stays the screen default and reproduces today's output exactly:
`pdfcer render-page FILE --edge-antialiasing on` (the default).
Pinned by `tests/edge_antialiasing_off_leaves_no_seam.rs::antialiased_edges_leave_a_seam_where_two_shapes_touch`.

## `Off` — hard edges

Each pixel whose centre the shape covers is painted (ISO 32000-1 §10.6.4's
scan-conversion rule), so abutting shapes tile exactly and no seam can
form. Text and diagonals stair-step, which is invisible at print
resolution. `pdfcer print`'s default `auto` uses it at 300 DPI and above.

## `Supersampled` — smooth and seam-free (`--edge-antialiasing seamless`)

`render_impl` (`crates/pdfcer-render/src/lib.rs`) renders with hard edges at
`SUPERSAMPLE_FACTOR` (4)× the resolution: `render_impl_rasterize` post-scales
the base CTM and the raster size by 4 after the region geometry is resolved
and before any CMYK band is cut (`supersample::fine_geometry`), so bands and
the region lattice anchor are in fine pixels. `supersample::finish` then
takes the 4 × 4 box mean of the premultiplied raster, which is the
coverage-weighted mix of what is really at each output pixel.

Device-pixel thresholds keep meaning one OUTPUT pixel through
`EdgeAntialiasing::pixel_factor()`: the thinnest-stroke floor
(`stroke_params`, `k / scale`) and `SUBPIXEL_CULL_PX`. The ink probe is
asked at its block's centre sample and answered in output pixels; the pixel
tallies in `Diagnostics` are divided by 16. A fine raster past
`MAX_PIXMAP_EDGE` is refused (`BadRasterSize`), never silently reduced.

Cost: 16× the pixels and the time. Measured on the eight patches of request
X068 at 108 dpi: every seam "X" is gone and text stays smooth.

## What edge mode does NOT fix

Two of X068's patches show a 3–6 level residual that survives hard edges,
because it is colour, not geometry:

- 16.4 (ICCBased CMYK, Luminosity): the X is painted in an embedded CMYK
  profile different from the output intent, so its CMYK→CMYK conversion
  lands 6 levels from the DeviceCMYK square it is designed to match.
- 16.1 (ICCBased RGB): residuals ≤ 5 levels; the test's own legend says a
  faint X from colour-management differences is not a failure.

Conversion belongs to iccce (decision 064). A pdfcer-side option to keep
CMYK numbers for an `ICCBased` N = 4 paint inside a DeviceCMYK blending
space would be a meaning-in-context policy, not colour tuning; it is not
built.
