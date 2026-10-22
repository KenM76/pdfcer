# Decision 197: zune-jpeg pinned to 0.5.16-rc3 for the 4:2:0 one-scan-per-component decode

Consultant: `autonomous-builder` (KenAgent protocol), option D.

## Decision

- Pin `zune-jpeg = "=0.5.16-rc3"` in every manifest that names it:
  pdfcer-image-codec (production), pdfcer-3d (`textures`, optional) and
  pdfcer-render (dev). zune-core moves 0.5.1 to 0.5.3 with it.
- A permanent regression test encodes Gray, RGB and CMYK through
  `jpeg_write` at every chroma and Huffman setting the writer offers, asserts
  one SOS per component for the optimized-Huffman sequential layout, and
  decodes it through pdfcer's decoder within 12 levels mean error. The bug
  read 113; legitimate 4:2:0 at q60 reads 5.2.
- **No encoder workaround.** Optimized Huffman tables stay on.
- Moving to stable 0.5.16 is a Backlog item and a **blocker for any
  crates.io publish**.

## The defect

zune-jpeg 0.5.15 mis-decodes baseline-sequential JPEG written one scan per
component (non-interleaved, legal under T.81 §B.2.3) whenever chroma is
subsampled, at every size from 16x16 up: mean error 95-128 levels, rendered
as green stripes. 4:4:4 decodes correctly. jpeg-encoder 0.7.1 writes exactly
this layout whenever `optimize_huffman_table` is on, so pdfcer's own
optimized JPEGs below quality 90, or with `chroma=half`, rendered wrong **in
pdfcer only**. The files were always valid; Pillow, libjpeg and jpeg-decoder
read them within about 1 level. Third-party PDFs with the layout were
mis-rendered too, which only a decoder fix repairs. rc3 reads them within
0.07 levels of Pillow.

## Reasoning

- The defect is in the decoder, so the fix is there.
- rc3 is measured, same licence (MIT OR Apache-2.0 OR Zlib), MSRV 1.77.1
  under pdfcer's 1.92, keeps the `forbid(unsafe_code)` cfg_attr with SIMD
  off (R24), and adds only an optional `arith` feature (off). The exact pin
  is immutable; render parity and the decoder suites are the net.
- The encoder workaround costs 5-10% size on the files the size feature
  targets, shapes legal output around one reader's bug, and removes the
  only way pdfcer's own output exercises the layout. That trades away error
  detection.

## Alternatives

| Option | Why not |
|---|---|
| Pin alone | Nothing catches the bug returning |
| Encoder workaround | Size cost, third-party files stay broken, layout leaves the tests. Fallback only if rc3 had failed the gates |
| Both | The workaround's cost for no gain |
| `[patch]` to a git commit | Harder to audit than an immutable rc; blocks publishing |

## Notes

- A caret `"0.5"` never matches a pre-release and Cargo allows one
  0.5-compatible version per graph, so all three manifests change together.
- A published crate with a pre-release pin fails to resolve beside any
  downstream `zune-jpeg ^0.5`; hence the publish blocker.
- `jpeg-decoder` as a test-only cross-check measured 0.058 levels from
  Pillow but was not adopted; the regression test's synthetic encode covers
  the layout, and its SOS assertion fails if the writer stops emitting it.
- Previously optimized files need no repair path.
- pdfcer-gui refreshes its lockfile to pick up the fix.
