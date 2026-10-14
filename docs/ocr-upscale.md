# OCR upscale and magnify: measurements behind the shells' defaults

What a shell should default to when it offers `pdfcer_core::ocr::upscale`
(`Upscale`, `UpscaleFilter`) and `pdfcer_core::settings::MagnifyFilter`
before an OCR run, and the numbers that chose those defaults. The CLI's
`pdfcer ocr --upscale / --upscale-filter / --magnify` help states the
same defaults. Source records: ROADMAP `Pass 566.0` and `Pass 580.0`.

## Defaults

| Setting | Default | Why |
|---|---|---|
| Upscale | **off** (factor 1) | It helps sharp low-resolution scans and hurts blurred ones (table 1, last row). The engine cannot tell the two apart, so there is no automatic mode. |
| Upscale filter | **bicubic** | It is as good as or better than Lanczos in most measured cases. Lanczos won one case by one word (table 1). |
| Magnify (render above the scan's own resolution) | **smooth** (bilinear) for an OCR render; **point-sample** for display | Point-sampling feeds the engine square pixel blocks (table 2). Display keeps point-sample so a viewer looks the way other viewers do. |
| Factor range | 1 to 4 (`MAX_FACTOR`), output at most 2^27 pixels (`MAX_OUTPUT_PIXELS`) | Hard limits, refused rather than clipped. |

**Advice a shell can surface:**
- For a small-text scan (fax, or a sharp scan at 50 to 75 dpi), render at the scan's own dpi and upscale by 2 to 3. Do not raise the dpi.
- `paddle` shrinks pages over 2000 px on the long side for detection, so a large factor buys it little.
- `paddle-vl` refuses upscaling (`RunnerError::UpscaleUnsupported`), because it resizes each region itself.

## Method

The test page is a synthetic 47-word page rendered as a scan. The score is `tools/ocr-accuracy.py` `content_pct`: the percent of truth words read. Each figure is one page, so a gap of 2 to 4 points is a word or two.

## Table 1: upscale (`Pass 566.0`)

| Page, engine | 1:1 | Upscaled |
|---|---|---|
| sharp 50 dpi, tesseract | 25.5 | 95.7 (x3 bicubic); 97.9 (x3 Lanczos) |
| sharp 50 dpi, ocrs | 74.5 | 85.1 (x2) |
| fax-thresholded 50 dpi, tesseract | 14.9 | 63.8 (x2) |
| already-blurred 75 dpi, ocrs | 83.0 | 76.6 (x2): **worse** |

For sharp 50 dpi with tesseract, the median word-box offset was 4.25 pt at 1:1 and 0.66 pt upscaled. The words are mapped back to the caller's raster either way.

## Table 2: route for a 50 dpi scan (`Pass 580.0`)

| Engine | 1:1 at 50 dpi | `--dpi 150` point-sample | `--dpi 150` smooth | 50 dpi `--upscale 3` |
|---|---|---|---|---|
| ocrs | 74.5 | 42.6 | 61.7 | 66.0 |
| tesseract | 25.5 | 87.2 | 87.2 | 95.7 |
| paddle | 100.0 | 91.5 | 100.0 | 95.7 |

- **Smooth magnify** never scored below point-sample.
- **Native dpi plus upscale** beat a raised dpi for ocrs and tesseract. Paddle did best at 1:1 or with smooth magnify.
- **Bicubic magnify** was tried instead of bilinear and gave mixed results (ocrs 68.1, tesseract 91.5, paddle 93.6), so bilinear stays.
- **The degraded 50 dpi variant** read 2 to 6 percent on every route and engine, so it does not separate them.

## Not measured

- Real scans, as opposed to synthetic pages rendered as scans.
- Factors other than 2 and 3.
- PP-OCRv5 Latin.
- Any page over one.

Treat the defaults as chosen on this evidence, not tuned.
