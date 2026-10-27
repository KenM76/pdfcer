# Decision 202: an additive colour entering a CMYK blending space uses the inverse of pdfcer's own calibrated CMYK table

Recorded at `Pass 646.10` (`X005`), 2026-10-26. Engineer-measured figures; stays inside decision 064.

## Decides

1. When no output-intent profile manages the conversion (the icccer ink/PCS bridges still run first when one does), an additive colour (DeviceRGB, ICC RGB, CalRGB, Lab) entering a DeviceCMYK blending space is converted with the inverse of pdfcer's own calibrated CMYK to sRGB table, the one the buffer is displayed through: `pdfcer_color::color::srgb_to_cmyk_with` (`Pass 617.8`).
2. New black-generation choice `BlackGeneration::Medium`: K at a fixed 15% of the way from the least to the most black that draws the colour.
3. `rgb_to_cmyk` (max-GCR, k = 1 - max(r,g,b)) stays as the fallback and in `overprint.rs`'s RGB-backdrop round trip (`snapshot_srgb_backdrop` and `composite_srgb`), which needs its exact inverse `cmyk_to_rgb` and must not change.
4. The solver is too slow per pixel; the render side uses a lazily built RGB to CMYK lookup grid per `CmykIntent`.

## Why

The max-GCR formula puts all the black in K, so under `/BM /Difference` over pure K a 50% grey renders near-black (22 18 21) where Acrobat, going through its CMYK profile, gives 137 112 106. Measured with the existing solver: Least gives grey 159 128 121, Most gives 12 9 14, Medium(15%) gives rows 13 141 194 / 184 90 47 / 137 109 103 against Acrobat 7 139 193 / 186 105 49 / 137 112 106. Using the inverse of the table the buffer is displayed through keeps a round trip consistent.

## Stated deviation

Row 2 (0.2 0.6 0.9) is out of gamut: pdfcer clips by least squares in sRGB, Acrobat by its profile's gamut mapping; the G channel differs by about 15. A measured deviation, not a defect.

## Body-section effect

None beyond the Decision log. New standing rule: none; `R263` stays next free. Next free decision: `203`.
