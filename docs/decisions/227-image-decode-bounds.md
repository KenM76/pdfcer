# 227 — Image decode bounds come from memory, data evidence and operator mode

Supersedes the policy constants of decision 226. Consultant: autonomous-builder.

## Decision

Image decode limits come only from three sources:

- **(a) Memory.** A predicted peak is checked against a memory budget that
  the shell supplies, before any allocation the decoder cannot make
  fallibly. The budget defaults to half the physical RAM, cgroup-limited,
  and the operator can override it up or down with no ceiling.
- **(b) Evidence.** Declared output that the supplied data cannot encode is
  refused.
- **(c) Operator strict mode.** `--max-image-pixels`, off by default.

Fixed pixel and byte ceilings are removed. Facts stay: format field widths,
dependency hard limits and `isize::MAX`. Every refusal is named and states
the needed and allowed figures. Fallible allocation (decision 226) stays as
a backstop.

## Why

- **The operator ruling of 2026-11-26.** G336 is a 13 MB scan small enough
  to email, and a policy number refused it.
- **`try_reserve` cannot guard against overcommit.** On Linux and macOS an
  oversized request still succeeds. Our dependencies also allocate
  infallibly: zune-jpeg allocates its output before reading entropy data,
  and in lenient mode it fills missing data and returns `Ok`. Only a
  prediction made before the call prevents an abort.
- **No error detection is lost.** Crafted headers are refused by evidence or
  by the budget before anything is allocated. Decompression bombs are
  bounded by the declared geometry times the expansion ratios. Tile and scan
  bombs stay walk-guarded.

## Formulas

**Predicted peak**

```
peak = working(codec, header) + w*h*out_components*bytes_per_sample
       + w*h*downstream_bpp
```

Working memory by codec:

- **DCT, progressive or non-interleaved:** sum of the i16 coefficient
  planes, combined with the remux grid as max(remux, zune).
- **JPX:** k * w * h * channels * 4, with k measured in 780.1.
- **JBIG2:** 2 * bitmap.
- **CCITT:** O(w).
- **JXL:** no prediction. It is bounded by `AllocTracker::with_limit`.

`downstream_bpp` adds the following, and render retries without the
optional planes before refusing:

- RGBA: 4
- CMYK ink plane: 4
- each spot plane: 1
- alpha or SMask: 1

**Budget.** The default is total physical memory / 2, not available memory,
so that results are reproducible. When no shell supplies a figure, the
library falls back to 4 GiB on 64-bit targets and 1 GiB on 32-bit and
wasm32. The GUI divides the budget by its number of concurrent decode
workers.

**DCT evidence** (Huffman coding only; arithmetic coding is already refused
by name):

```
B_c  = ceil(ceil(X*Hc/Hmax)/8) * ceil(ceil(Y*Vc/Vmax)/8)
bits = k * min over first-scan components of B_c
       k = 2 sequential (DC code >= 1 bit + EOB >= 1 bit), 1 progressive
require 8 * data.len() >= bits
```

A progressive frame whose first scan has Ss != 0 is malformed, because the
DC scan must precede the AC scans.

**Other evidence:**

- **JPX:** tiles <= max(4096, len/14). A tile-part takes a 12-byte SOT plus
  a 2-byte SOD.
- **CCITT:** rows <= 8 * len + 1, because each row is at least 1 bit.
- **Image streams:** the expected length is h * ceil(w * nc * bpc / 8),
  plus h when a PNG predictor applies. It must not exceed the product of
  each stage's maximum expansion: Flate 1032, RunLength 64, ASCII85 4,
  ASCIIHex 1/2, raw 1. A chain that contains LZW or Brotli relies on memory
  alone. Decoding stops at the expected length.

**Citations to verify in the spec RAG before they go into doc comments:**

- T.81: B.2.2, Annex C, F.1.2.1–2, G.1.1.1.1, G.1.2.2
- T.800: A.4.2, A.5.1
- T.6 (V0)
- RFC 1951 §3.2.5

## Facts versus policy

| Item | Class | Action |
|---|---|---|
| `MAX_IMAGE_PIXELS`, `MAX_IMAGE_SAMPLE_BYTES`, `MAX_DECODER_WORKING_BYTES` | policy | remove (780.5) |
| `MAX_REMUX_BLOCKS` | policy | derive from the remaining budget / 128 |
| 65535 per side, JPEG | fact (T.81 16-bit SOF fields) | keep, `SizeLimit::FormatField` |
| 65535 per side, JBIG2 | dependency fact (hayro-jbig2) | keep, and name the dependency |
| 65535 per side, JPX/CCITT/JXL | policy | remove |
| CCITT `row_ceiling` | policy | replace with the evidence bound |
| JPX `MAX_TILES` 4096 | work bound | scale by evidence |
| `MAX_PROGRESSIVE_SCANS`, `MAX_MARKERS_WALKED` | walk guards | keep |
| `MAX_DECODED_LEN` 256 MiB | policy | image streams get the expected length (780.6); content streams are a follow-on |
| `MAX_DISPLAY_LIST_BYTES` | policy | shell-derived share of the budget (780.7) |
| GPU 2^24 px | fact (f32 mantissa) | keep, with CPU fallback |

## Public API

- **`pdfcer-image-codec`**, re-exported by core and render:
  - `DecodeLimits { memory_budget, max_pixels: Option<u64>, downstream_bytes_per_pixel }`, `#[non_exhaustive]`, with `new`, `for_physical_memory`, `with_max_pixels`, `with_downstream_bytes_per_pixel`, `FALLBACK_MEMORY_BUDGET` and `Default`.
  - `DecodeOptions { cmyk_jpeg_polarity, limits }`, which replaces the polarity parameter of `decode_image_view_with`.
  - `SizeLimit { MemoryBudget, OperatorPixelCap, FormatField, AddressSpace }`.
  - `CodecKind`. Reuse an existing enum if one exists.
  - `ImageCodecError::LimitExceeded { limit, needed, allowed }` and `DeclaredGeometryExceedsData { codec, min_bytes, have_bytes }`.
  - `TooLarge` is removed in 780.5.
  - Stable refusal keys, for example `image/limit/memory-budget` and `image/evidence/dct`.
- **Render:**
  - `RenderPolicy::decode_limits` and `with_decode_limits`.
  - The matching `ImageError` variants.
  - In 780.8, `RenderPolicy::image_texel_dpi: Option<f32>`, where `None` means full resolution, which the CLI and export always use.
- **Settings:** `max_image_memory` and `max_image_pixels`.
- **CLI:** `--max-image-memory <SIZE>` and `--max-image-pixels <N>`. Verbose output prints the derived budget.
- **New shell-side crate `pdfcer-host`.** Core and render never depend on it. It provides:
  - `physical_memory_bytes() -> Option<u64>`, using `sysinfo` (MIT), with `default-features = false, features = ["system"]`, cgroup-aware. It needs a rule-13 filing.
  - `default_decode_limits()`.

## Pass split

The old constants stay until 780.5, so protection never weakens partway
through the arc.

- **780.1: plumbing and measured predictors (inert).**
  - The measured peak must not exceed the predicted peak, and predicted must
    stay within 1.5 times measured. Cover every sampling factor,
    progressive, non-interleaved, CMYK/YCCK, multi-component JPX, and
    JBIG2 generic, text and halftone.
  - Use a peak-counting allocator in an integration-test binary. It is a
    dev-dependency and must be licence-classified.
  - wasm32 builds of core and render.
- **780.2: evidence bounds.**
  - A crafted 65535² header backed by about 600 bytes is refused before any
    allocation over 1 MiB.
  - The corpus shows zero new refusals.
  - The 310-byte JPX that declares 65,536 tiles is refused.
  - Boundary tests: `bits - 1` is refused and `bits` is admitted.
- **780.3: the budget goes live.**
  - `pdfcer-host`, the settings, the CLI flags and the `RenderPolicy` field.
  - `cargo tree` for core and render shows no sysinfo.
  - A refusal names the needed and allowed figures and the override flag.
  - Tests pin the budget and never query the machine.
- **780.4: fuzz limits and the tripwire.**
  - The harness passes `DecodeLimits::new(rss_limit - 1 GiB)`. The fast
    configuration adds `with_max_pixels(Some(1 << 24))`, and a nightly run
    uses the budget alone.
  - The allocator asserts that peak <= predicted + 16 MiB.
  - Each target runs clean for one hour, and a deliberately undercounting
    predictor is caught.
- **780.5: remove the policy constants.**
  - Amend §10.1, update core-api, and amend decision 226.
  - A synthetic 5100×8400 baseline JPEG and a synthetic D-size 600 dpi
    bilevel image decode under the default budget.
  - A flat but valid 65535² JPEG under a 1 GiB budget is refused with
    `MemoryBudget` before anything is allocated.
- **780.6: image-stream byte bounds.**
  - A 1 KB Flate stream that declares 2 Gpx is refused by evidence.
  - A legitimate 90 Mpx RGB Flate image decodes.
  - Data past the expected length is not decoded.
- **780.7: texel accounting.**
  - `DisplayList::memory_bytes` counts each distinct `Arc<Pixmap>` once.
  - The display-list budget is derived from the shell's budget.
- **780.8: display-resolution texels**, behind `image_texel_dpi`.
  - At a given zoom, a screenshot must match the full-resolution render at
    the same device resolution.
  - CLI and export output are byte-identical to today's.

## §10.1 amendment text

> Every decoder takes an explicit output bound. It has three parts:
> (a) a predicted peak, checked against the caller's
> `DecodeLimits::memory_budget` before any allocation the decoder cannot
> make fallibly; (b) every evidence bound the format admits; and
> (c) optional operator caps, off by default. Fixed size constants are
> allowed only as facts: a format field width, a dependency's hard limit,
> or `isize::MAX`. Walk guards stay under the depth-guard clause, scaled by
> data length where the format allows.

## Rejected

- **Raising the constants again.** That only moves the arbitrary line.
- **`try_reserve` alone.**
- **"Available memory" as the budget.** The result would not be
  reproducible.
- **Querying memory inside core.** It hurts determinism and embedder
  control, and adds an OS dependency.
- **`set_alloc_error_hook`.** It is unstable and cannot recover.
- **Subprocess isolation.** It is not possible on wasm.
- **zune strict mode.** It refuses legitimate truncated scans and still
  allocates first.
- **A fixed `MAX_TILES`.**

## Known limits

- **Decode time is O(declared pixels).** That is up to 8192 px per byte for
  DCT, and zune cannot be cancelled. The GUI decodes off-thread; the web
  fork blocks.
- **A crafted flat image that fits the budget decodes,** by design.
- **Acceptance depends on the machine.** The refusal names the override.
- **JPX, JBIG2 and JXL have no evidence bound.**
- **JPEG and hayro-jbig2 stay capped at 65535 per side.**
- **Content-stream `MAX_DECODED_LEN` and `MAX_PAGE_CONTENT` are not
  re-grounded yet** (follow-on Pass).
- **Predictors must be re-measured on every zune or hayro version bump.**
  The 780.1 tests fail if they drift.
- **Risks:** a predictor that undercounts (mitigated by the allocator tests
  and the fuzz tripwire); evidence false positives from odd sampling or DNL
  (mitigated because the check lands while the old ceiling is still in
  place, plus corpus and boundary tests); GUI oversubscription (mitigated
  by splitting the budget per worker); and the API break in
  `decode_image_view_with` and `TooLarge`, which core-api must record for
  pdfcer-gui.
