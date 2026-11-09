# Decision 221: JPEG XL encoder and the workspace rust-version (Pass 696.0)

Date: 2026-11-09. Consulted: autonomous-builder.

## Decision

Adopt `jixel =0.5.7` (BSD-3-Clause OR Apache-2.0, taken as Apache-2.0) as the
JPEG XL encoder, behind a new `pdfcer-render` feature
`jxl-write = ["jxl", "dep:jixel"]`, on by default and strippable;
`pdfcer-cli` forwards it. Raise the workspace `rust-version` from 1.92 to
**1.97**, and add `tools/check-msrv-floor.py` so the declared value stays true.

## Reasoning

- The declared 1.92 was already false: `ocrcer-core =0.2.0`, on by default
  through pdfcer-core's `ocrcer` feature, declares 1.97. jixel (1.94) is the
  dependency that exposed it. 1.97 is also the pinned toolchain (1.97.1), the
  only compiler the code is built and tested on.
- Every jixel release from 0.2.14 to 0.5.7 declares 1.94, so the MSRV-aware
  resolver has no older release to fall back to.
- A separate `jxl-write` feature keeps the encoder (7.6 MB of source, mostly
  SIMD) out of lean and decode-only builds. It implies `jxl` because every
  encoded file is decoded back with `jxl-oxide` before it is returned.
- The encoder only takes pdfcer's own pixmaps, never untrusted input, so its
  `unsafe` SIMD code is a smaller risk than a decoder's would be.

## Alternatives rejected

- Workspace 1.94: still false (1.97 is already needed).
- Override 1.94 in pdfcer-render only: false for render's default build, and
  splits the single version source.
- Encoder off by default: breaks the default-on, strippable convention.
- Defer, or libjxl through FFI: needs C++ and breaks wasm32 and the portable
  folder. zune-jxl only decodes; a hand-written encoder is a large job.

## Rules the implementation keeps

- Always `with_num_threads(1)`: jixel's default is every core, which breaks the
  single-threaded engine decision.
- Lossless by default. `export-image --jxl-distance D` makes it lossy and the
  result line reports `max_error=`. A lossless file that does not decode back
  to the exact pixmap is refused, never written.
- `jixel` default features off (drops `extraslow` and its two metric crates);
  `sse`, `avx`, `neon`, `wasm` kept. Do not offer `Speed::ExtraSlow`.
- Exact pin: an upgrade is deliberate and re-checks its `rust-version`
  against the floor.
- `check-lean-render-graph.py` bans pdfcer-render's own optional deps as well
  as pdfcer-core's.

## Risks

- Only the plain (non-SIMD) wasm path is checked; `simd128` is untested.
- Patent position: Apache-2.0's grant covers the jixel author's patents only.
  Whether libjxl's patent grant reaches a port is **unverified**; jxl-oxide
  (the decoder) already carries the same exposure. Recorded, not settled.
- Release-binary size impact to be measured at the next release.
- The OCRcer vendor sync can raise the floor silently; check-msrv-floor.py is
  the guard.
