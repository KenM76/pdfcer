# Export pages to PNG / JPEG / SVG, and copy-out to other software — the plan

Written 2026-09-03 by the engineer, in response to the operator's request of
the same day, verbatim:

> *"can you add the ability to export page(es) to png, jpg, svg. note that
> there had better be full support (including transparency where supported!).
> Also I'd like to be able to copy and paste anything to other software -
> like copy and paste vector graphics into word or inkscape for example if
> possible."*

Three asks in one sentence, and one adverb doing a lot of work — *"full
support"*. This document says what "full" is taken to mean for each format,
what pdfcer can already do that the work builds on, where every piece lives,
and what each Pass owes before it ships. The Pass entries in `ROADMAP.md`
(filed by `pdfcer-librarian`) are the contract; this file is the reasoning.

## 0. What already exists, measured — do not rebuild it

| capability | where | state |
|---|---|---|
| Rasterise a page, RGBA, into a `tiny_skia::Pixmap` | `pdfcer_render::render_page_with` | shipped; the page group is composited **isolated and transparent** (§11.4.7) and the white paper is added in ONE step at the very end (`flatten_page_group_over_white`). **The transparent raster the operator asked for already exists for one line of the function's life** — it is only the last step that destroys it. |
| Write that raster as PNG | `pdfcer render-page -o out.png` | shipped; `Pixmap::encode_png` (tiny-skia's `png` feature). No `pHYs` (DPI) chunk, always white-backed. |
| JPEG encoding | `jpeg-encoder 0.7` | already a dependency of `pdfcer-core` (`/DCTDecode` authoring, `image_import/jpeg_encode.rs`); **not** in `pdfcer-render`. Rust RAG finding: it inverts CMYK on encode — irrelevant for RGB output, which is all this plan emits. |
| A page interpreted ONCE into a device-space list of finished paths, image fills, clips and transparency layers | `pdfcer_render::display_list::record_page` (`Pass 75.0`) | shipped as a **cache** for the shell's panning. Text arrives as filled glyph outlines; images as `Brush::Image` fills; groups and `/CA` as `Op::Layer { opacity, blend, nonseparable }`; clips as a parent-linked table. It **refuses by name** (`PoisonReason`) anything that reads the destination back — shadings, soft masks, tiling patterns, overprint, non-separable per-paint blends, non-isolated groups — because a cache that replays a *plausible wrong* picture is worse than none (`R211`). |
| A selection as a standalone one-page PDF | `ObjectClip::to_pdf` (`Pass 120.2`) | shipped, for a host shell's OS-clipboard interop. |
| Multi-page page-range parsing, output-dir batch pattern | `export-dxf --pages --output-dir` (`Pass 52.1`) | shipped; `parse_pages` in `main.rs`. |
| Vector export to a CAD format | `pdfcer_core::export::dxf` | shipped; walks `vector::PageObjects` (the *editing* model), not the renderer. |

## 1. Design — the one decision that shapes everything

**SVG is produced from the renderer's own recording, not from the editing
model.** Two routes were available:

- **(a)** walk `vector::PageObjects` the way DXF does. Loses everything the
  editing model does not carry — images, clips, transparency, blend modes,
  shadings, Type 3 glyphs — and would be a **second interpreter** (the trap
  this project has now written down more than three times).
- **(b)** consume the display list's `Op`s. One interpreter; the SVG contains
  exactly the geometry the raster painted, in the same device space, at
  `scale = 1` so one SVG user unit is one PDF point. Text is exported as
  outlines — which is what "renders identically everywhere" costs, and is
  what every PDF→SVG converter that people trust (pdf2svg, Inkscape's own
  import in its default mode) does.

**(b) is chosen.** Its cost is the display list's refusal list, and that
is addressed **without weakening the cache's posture**: a second
recorder mode, *export*, in which every site that today calls
`canvas.refuse(reason)` instead **rasterises that one operator at the
recording scale into a transparent scratch and records it as an image
fill** — the same scratch-and-evaluator route the subtractive (CMYK) page
path already takes for a shading. The cache mode keeps refusing; the export
mode never refuses, and **counts** what it rasterised so the disclosure can
say *"2 shadings and 1 soft-masked group are embedded as raster in this
SVG"* (rule 4: fuzzy, never sneaky). Axial and radial shadings with a
sampleable ramp are the obvious later upgrade to true `<linearGradient>` /
`<radialGradient>`; that is a refinement, not a prerequisite — a
rasterised gradient at 300 DPI in an SVG is a correct picture, and a
refused export is not.

## 2. What "full support (including transparency where supported)" means, per format

| format | transparency | resolution | colour | what pdfcer discloses |
|---|---|---|---|---|
| **PNG** | **yes** — RGBA8, straight alpha, the page group's own αg with NO white paper. `--transparent` (off by default: a page export that is see-through where the artwork is thin surprises anyone who did not ask for it — but the *operator's* case, "copy this drawing into a slide", is exactly the one that wants it). | `--dpi` (default 150 — Acrobat's default for image export; verify against the Acrobat RAG) written into a `pHYs` chunk so Word/PowerPoint paste it at physical size. | sRGB. A page whose blending space is `DeviceCMYK` collapses through the same `cmyk_to_srgb_with(intent)` as the screen, then keeps its alpha instead of flattening over white. | resolution, pixel size, whether the backdrop was kept, every counter `render-page` already prints. |
| **JPEG** | **no** (the format has none) — flattened over `--background` (default white, any `#rrggbb`). **Refuses `--transparent` by name** rather than silently flattening: the operator asked for a thing the format cannot hold, and a silently white-backed JPEG looks exactly like success. | `--dpi`, written as JFIF density. | sRGB, 4:2:0 chroma at `--quality` (default 90). | as PNG, plus the backdrop colour used and the quality. |
| **SVG** | **yes, natively** — no background element unless `--background` is given; every `fill-opacity` / `stroke-opacity` / group `opacity` / `mix-blend-mode` is written from the recorded state; images are embedded as **PNG-with-alpha data URIs** so a masked image stays masked. | resolution-free for vectors; `--dpi` governs the raster of anything the export mode had to rasterise (default 300 — it is embedded, so it should be print-grade). | sRGB (`rgb()` / `fill-opacity`). | text-as-outlines glyph count, shadings/soft-masks/patterns rasterised, overprint approximated, non-isolated groups rendered isolated, blend modes SVG cannot express (there are none of the sixteen; `mix-blend-mode` covers all of §11.3.5) — every one a counter on the machine line and a sentence on stderr. |

**SVG 1.1 with the one CSS3 property `mix-blend-mode`** (SVG 2 / CSS
Compositing Level 1). Inkscape ≥ 1.0, every browser, and LibreOffice read it;
Word's SVG importer ignores `mix-blend-mode` and shows `Normal` — the
disclosure names that, the file does not degrade itself for one consumer.

## 3. Copy-out — what reaches Word, Inkscape, LibreOffice

The **engine** owes *bytes in every format a target application will
accept*, and *nothing else*: the OS clipboard is a windowing concern, and
`pdfcer-core`/`pdfcer-render` may not touch it (the GUI-core invariant,
`ARCHITECTURE.md` §3). Who places the bytes:

- **`pdfcer-gui`** (separate project) for the interactive case — it gets a
  channel note naming the formats, in order, and the engine calls that
  produce each.
- **`pdfcer copy-page` / `pdfcer copy-selection`** in the CLI — a **shell**,
  which may carry an OS-clipboard crate, and which lets the operator (and
  this engineer, through `combridge word`) verify a paste into Word today
  rather than after the GUI project consumes the note. The clipboard crate
  is chosen from `docs/clipboard-interop-survey.md` (research dispatched
  2026-09-03) and licence-classified there before it is added.

The format set to place is decided by that survey, not by this plan. The
engine deliverables it will draw on: SVG text (§2), PNG-with-alpha (§2), and
the existing one-page PDF (`ObjectClip::to_pdf`). If the survey finds that
Word needs EMF to paste *editable* vectors, that is a follow-on Pass with
its own writer — it is not assumed here.

## 4. Pass breakdown

| Pass | slice | ships |
|---|---|---|
| **248.0** | raster export with alpha | `RenderOptions::backdrop` (`PageBackdrop::{White, Transparent}`), the CMYK buffer's transparent collapse, `pdfcer_render::export::{encode_png, encode_jpeg}` with DPI metadata, `pdfcer export-image --format png\|jpeg --pages --dpi --transparent --quality --background --output-dir`, render tests that DECODE the PNG/JPEG and assert alpha and colour, a CLI contract test. |
| **248.1** | SVG export | the recorder's export mode, `pdfcer_render::svg::export_svg`, `export-image --format svg`, an oracle test that rasterises the SVG with `resvg` (dev-dependency, MIT/Apache-2.0) and compares against pdfcer's own PNG of the same page, a fuzz target from PDF bytes to SVG. |
| **248.2** | copy-out | the CLI clipboard verbs, an end-to-end paste into Word verified through `combridge`, the channel note to `pdfcer-gui`, `FEATURES.md` rows. |

Each Pass ships its `pdfcer` subcommand in the same commit (rule 11), its
`docs/core-api/03-capabilities.md` section in the same commit (the
`pdfcer-gui` project builds from that file, not from chat), and its
`FEATURES.md` rows through the librarian.

## 5. Things that would be easy to get wrong

- **Premultiplied alpha.** `tiny_skia` stores premultiplied RGBA; PNG wants
  straight. `Pixmap::encode_png` demultiplies. A hand-written encoder that
  forgets does not fail — it ships dark fringes on every anti-aliased edge.
  The test decodes the PNG and checks a known 50 %-alpha fill comes back as
  `(r, g, b, 128)`, not `(r/2, g/2, b/2, 128)`. Rust RAG:
  `premultiplied_alpha_needs_multiply_not_clamp.md`.
- **A transparent PNG of a page whose blending space is `DeviceCMYK`.** The
  additive path is one skipped call; the subtractive path builds its pixels
  *inside* the white composite (`to_srgb_over_white`), so it needs a sibling
  that emits `(Cg·αg, αg)` and never the `1 − a` term. Missing this ships a
  `--transparent` flag that works on nine pages in ten and silently flattens
  the tenth — the shape `feedback_a_shell_flag_can_be_parsed_and_never_used`
  warns about, one level down.
- **`replay_region` (the GUI's cached route) also flattens over white.** It
  is left alone: the shell shows paper. The export path renders directly.
- **A page-level poison in export mode must not be a page-level fallback.**
  Rasterising the *whole page* because one shading refused would make every
  SVG with a gradient a bitmap in a vector costume. The scratch is per
  operator, clipped to that operator's device bounds.
- **SVG clip nesting.** A recorded clip has a parent; SVG expresses
  intersection by putting `clip-path="url(#parent)"` on the `<clipPath>`
  element itself. Emitting only the leaf loses every enclosing clip.
- **Word's SVG importer is not a browser.** No `<style>` blocks, no CSS
  classes, no `mix-blend-mode` honoured, `<image>` must be a data URI.
  Presentation attributes only.
