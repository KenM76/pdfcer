---
name: pass11-render-parity
description: Pass 11 shipped the render-parity harness (standing render-fidelity gate) + two named render residuals
metadata:
  type: project
---

Pass 11 (render-fidelity verification, decision 010 candidate C) shipped a
full-page pdfium (pypdfium2) pixel-parity harness at
`tools/render-parity/render_parity.py` (+ `README.md`). Out-of-tree Python,
mirrors tools/content-identity; drives `pdfce-cli render-page` + pypdfium2.
NO Rust touched — measurement only.

**Why:** vector editing (Pass 9, candidate A) is the first subsystem whose
oracle is independent visual fidelity; the self-comparison round-trip oracle
can't prove an edited page renders correctly. This harness is that oracle and
the standing gate A's edits inherit.

**How to apply:**
- It is the standing **render-fidelity gate** — RE-RUN on every render-touching
  Pass (esp. Pass 9). `--gate --max-unexplained <baseline>`; the R34/R46 pattern.
- Tolerance band is derived EMPIRICALLY (p99.9 of `frac_over_32` over
  clean-by-construction pages), never tuned to pass (W14). Band metric =
  fraction of pixels with max-channel delta >32 (area, not max-delta — the
  noise-robust discriminator). Default run is content-only (annots off) — the
  vector-editing oracle; `--annots` buckets pdfium's FPDF_FFLDraw/synthesized
  quirks reference-side (Y2).

**Full-corpus baseline (2914 files, 2890 pages, dpi 125, 0 panics, 24
unloadable-skips):** band=0.0294; buckets benign 2840 / known-gap 49 /
**unexplained 1**. Gate baseline = max-unexplained **1**.

**Two named residuals filed (R20/R27) — NOT fixed in-Pass (measurement-only):**
1. `TWG test suite A019-pdfa2-pass-a.pdf` — a form-XObject fills a triangle
   with a vertex at x≈3.4028e38 (≈f32::MAX). pdfium paints nothing; pdfce
   rasterizes a spurious cyan bar. Render-robustness edge case (out-of-range
   path coord overflows under CTM). Fix = a clamp/reject policy in pdfce-render
   (R34 risk) — deferred. The ONE genuine unexplained divergence corpus-wide.
2. **DeviceCMYK→RGB colorimetry** (decision 006 §3.7): naive-additive
   `Rgb::from_cmyk` (gstate.rs) vs pdfium `AdobeCMYK_to_sRGB1`. DeviceCMYK-only
   pages diverge 3.0x the clean-page mean corpus-wide; polarity identical (R29
   holds), hue/saturation differs across the whole filled area. Filed as the
   harness's FIRST named residual (a follow-up colour Pass). If fixed later:
   re-pin decision 006 §3.4 polarity matrix FIRST (006 revisit-trigger 7);
   don't confound with harness build (Y5). See [[project_pass8_redaction]] for
   the sibling by-file/by-reason R20 posture.

**Pass 1.1 remainder:** discharged at full-page corpus scale (per-channel
per-pixel, first-page coverage of every loadable file; multi-page via
`--pages-per-file 0`, demonstrated not exhaustively swept). This is the exact
"genuinely generalizes to full-page corpus scale" bar decision 010 set.
