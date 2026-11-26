# 226 — Image decode ceilings sized for 600 dpi copier scans

**Trigger:** request G336. A 600 dpi colour scan of a legal page
(5100 × 8400 DeviceRGB DCT, 42.8 Mpx) rendered blank: it exceeded the 32 Mpx
`MAX_IMAGE_PIXELS`.

## Decision

- `MAX_IMAGE_PIXELS` = 2^27 (134,217,728). `MAX_IMAGE_SAMPLE_BYTES` is
  derived as pixels × 4 (512 MiB).
- The JPX/JXL working budget becomes its own fixed constant,
  `MAX_DECODER_WORKING_BYTES` = 512 MiB. It is not derived from the pixel
  ceiling.
- The JPEG remux coefficient budget becomes its own fixed constant of 2^22
  blocks.
- Optional per-image CMYK, overprint and spot planes are bounded by a 1 GiB
  per-image working budget. A plane that does not fit takes the existing
  disclosed fallback.
- Large allocations are fallible. Running out of memory is a named,
  disclosed refusal, not an abort; on wasm32 an abort traps the module.
- `upscale-images`' output ceiling is tied to `MAX_IMAGE_PIXELS`, so pdfcer
  never writes an image it then refuses to read.
- A size refusal names the ceiling that fired, with the needed and allowed
  figures.

## Why 2^27

- It covers legal (42.8 Mpx) and A3 (69.6 Mpx) at 600 dpi, the copier
  defaults.
- It still refuses the crafted 65535 × 65535 header (4.3 Gpx) by a factor
  of 32.
- It equals the upscale writer's existing output ceiling.

## Why the JPX/JXL and remux budgets are fixed

- These working sets do not scale with the RGBA factor.
- At 4 components, the fixed 512 MiB JPX/JXL budget is exactly the old
  2^25 px, so no JPX/JXL that renders today is newly refused.

## Rejected or deferred

- **Decode at draw size** (JPEG's 1/2, 1/4 and 1/8 IDCT). It only helps
  JPEG, and only viewing; OCR, export and replace-image need full
  resolution. It would also add a second decoder whose pixels differ past
  1:1. **Deferred** to a "display-resolution image texels" Pass: decode
  once, downsample to the draw size, drop the full buffer. That Pass is
  also the only route to large-format bilevel scans (D-size at 600 dpi is
  269 Mpx, still refused).
- **A stricter mode now.** A renderer-only lower ceiling would leak, because
  the codec checks the codestream's own geometry against the constant. The
  sound form is a `DecodeLimits` that can only lower the ceilings, threaded
  through the codecs, plus a CLI `--max-image-pixels`. It waits for a
  consumer that needs it.
- **A runtime ceiling that can be raised:** rejected, because it removes
  the guard.

## Known limits carried

- `filters::MAX_DECODED_LEN` stays at 256 MiB because it also guards content
  streams. Flate, LZW and raw images therefore stay capped below the pixel
  ceiling: 8-bpc RGB at 89.5 Mpx, 8-bpc CMYK at 67.1 Mpx.
- Display lists keep full-resolution texels and do not count them in
  `memory_bytes`. A GUI page cache must budget for them: about 171 MB per
  cached legal scan page.
- Images over 2^24 px never take the GPU path.

## Amendment — operator ruling

The operator rejected a fixed image-size ceiling imposed on end users. The
2^27 value above is interim. Pass 780.1 replaces it with guards grounded in
memory, in evidence of malformation, or in an operator cap that is off by
default (§12, decision 226 amendment).
