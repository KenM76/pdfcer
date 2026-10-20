# PDF size-reduction survey: non-Adobe tools

- **Purpose:** scope for pdfcer's optimiser. The aim is to match or exceed every surveyed tool's controls, including control over individual objects.
- **Scope:** every tool except desktop Adobe Acrobat. Adobe's online compressor is included.
- **Method:** public manuals, help pages and official docs only (surveyed 2026-10-20).
- **Licensing:**
  - Ghostscript, MuPDF, cpdf (AGPL) and pdfsizeopt (GPL) are described from their user documentation only. No source code was read.
  - Their behaviour is a reference only, never a code source (`LEGAL.md` §6.1).

**Schema.** Each tool has the same eight fields:

| # | Field | Covers |
|---|---|---|
| 1 | IMG | image controls |
| 2 | FONT | font controls |
| 3 | STRUCT | structure controls |
| 4 | CS | content-stream optimisation |
| 5 | PRESET | presets, profiles and "only if smaller" safeguards |
| 6 | AUDIT | space audit or report |
| 7 | GRAN | per-object or per-page granularity |
| 8 | PREVIEW | preview of the quality impact |

**ND** means not documented in the official sources fetched. It means "not verified", not "known to be absent".

---

## 1. PDF-XChange Editor / Pro: Optimize PDF ("Save As Optimized")

Sources:
- https://help.pdf-xchange.com/pdfxe9/save-options_ed.html
- https://help.pdf-xchange.com/pdfxp11/optimizing-pdfs.html
- https://help.pdf-xchange.com/pdfxt8/optimize-document_t.html
- https://www.pdf-xchange.com/knowledgebase/289-reduce-the-file-size-of-documents-in-PDF-Xchange

1. **IMG**
   - Image classes: there are four, Color, Grayscale, **Indexed** and Monochrome. PDF-XChange is the only GUI tool with a separate indexed class.
   - Resampling: bicubic downsampling or subsampling, to a target dpi, applied above a minimum dpi (the threshold).
   - Codecs: JPEG, JPEG2000, JBIG2, CCITT, ZIP, with a quality setting where it applies.
   - "Optimize images only if there is a reduction in size" is on by default.
   - Other image options:
     - "Detect and merge image fragments"
     - discard alternate images
     - discard duplicate images (Cleanup tab)
   - ND: MRC, conversion to grey or bilevel, JBIG2 lossy versus lossless, and the default dpi values.
2. **FONT**
   - Unembed: none, "recommended", all, or chosen by hand, with a custom list of fonts to keep.
   - "Merge fonts subsets" merges duplicate subsets.
   - "Discard duplicates of fonts".
   - ND: Type1→CFF conversion and subsetting of every embedded font.
3. **STRUCT**
   - Discard Objects tab:
     - form submit, import and reset actions
     - JavaScript actions
     - alternate images
     - thumbnails
     - embedded print settings
     - bookmarks
   - Discard User Data tab:
     - comments, forms and multimedia
     - metadata
     - attachments
     - "application private data" (PieceInfo)
     - hidden layers, plus flattening of visible layers
   - Cleanup tab:
     - discard duplicate fonts, images and Form XObjects
     - Flate-encode unencoded streams
     - convert LZW to Flate
     - remove invalid bookmarks
     - remove unreferenced named destinations
     - remove content outside the CropBox
   - ND: object and xref streams, linearisation, tag removal, and general removal of unused objects.
4. **CS:** ND.
5. **PRESET**
   - Built-in "Standard Mode" and "Mobile Mode".
   - Custom profiles can be saved, deleted, imported and exported.
   - A "Make compatible with" setting picks the PDF version.
   - Images are only replaced if smaller (the option above).
6. **AUDIT:** "Audit Space Usage" gives a breakdown of space used. How fine-grained it is: ND.
7. **GRAN:** per image class only. No choice of individual images or page ranges.
8. **PREVIEW:** ND.

## 2. Foxit PDF Editor: PDF Optimizer / Reduce File Size

Sources:
- https://resource-cms.foxit.com/foxit-pdf-editor-for-windows/advanced-optimization-options
- https://resource-cms.foxit.com/foxit-pdf-editor-for-windows/reduce-file-size-with-default-settings
- https://resource-cms.foxit.com/foxit-pdf-editor-for-windows/optimize-scanned-pdf
- Manual v13 (pages 122 and 169–176): https://cdn01.foxitsoftware.com/pub/foxit/manual/phantom/en_us/foxit-pdf-editor-user-manual-13.pdf

1. **IMG**
   - Image classes: colour, grey and mono.
   - Downsampling: Off, Average, Subsampling or Bicubic, to a target dpi, applied "For images above" a threshold dpi.
   - Codecs for colour and grey: ZIP, JPEG, JPEG2000 (lossy or lossless), and "High Compression" (Pro only, not explained; it may be MRC, ND).
   - Codecs for mono: ZIP, JBIG2 lossless or lossy, CCITT G3/G4, Run Length, and High Compression.
   - Quality levels: Minimum, Low, Medium, High, Maximum. JPEG2000 tile size defaults to 256.
   - "Compress images only if there is a reduction in size".
   - Other image options:
     - merge image fragments (applies when there are more than 25)
     - discard alternate images
     - "Convert smooth lines to curves"
   - Scanned-PDF optimiser: JPEG2000 lossless or lossy, JBIG2, CCITT G4 or High Compression, plus deskew, OCR and removal of blank pages. Its settings can be saved as a preset.
   - ND: image deduplication and conversion to grey or bilevel.
2. **FONT**
   - Fonts are chosen by name in two lists, using Unembed and Retain buttons.
   - "Subset all embedded fonts".
   - ND: merging subsets and Type1→CFF conversion.
3. **STRUCT**
   - Discard Objects panel:
     - form actions
     - flatten fields
     - JavaScript
     - alternate images
     - thumbnails
     - document tags
     - print settings
     - search index
     - bookmarks
   - Discard User Data panel:
     - comments, forms and multimedia
     - external cross-references
     - Info dictionary and metadata
     - object data (tag-node attributes)
     - attachments
     - private data of other applications
     - hidden layers, plus flattening of visible layers
   - Clean Up panel:
     - Flate-encode unencoded streams
     - convert LZW to Flate
     - remove invalid bookmarks and links
     - remove unreferenced destinations
     - **linearise ("fast web view")**
   - Transparency flattening, chosen from presets.
   - Attachments are never optimised.
   - ND: object streams and removal of unused or duplicate objects.
4. **CS:** "Optimize page content" removes end-of-line characters to help Flate. No operator or precision work.
5. **PRESET**
   - Named predefined settings can be saved and deleted.
   - A "Make compatible with" version setting.
   - Reduce File Size offers the levels "Retain existing" or a chosen version, and has a batch mode.
   - The Action Wizard has optimisation actions.
   - Images are only replaced if smaller.
6. **AUDIT:** "Audit space usage". How fine-grained it is: ND.
7. **GRAN:** per image class and per named font. No choice of individual images or pages.
8. **PREVIEW:** **"Estimate Compression Size"** shows the estimated file size for each level. No visual preview.

## 3. Nitro PDF Pro

Sources:
- https://www.gonitro.com/user-guide/pro/article/optimize-a-pdf-file
- https://www.gonitro.com/user-guide/optimize
- https://www.gonitro.com/resources/how-to-reduce-pdf-file-size (marketing article)

1. **IMG**
   - Windows Custom: "Resample images" with one compression level. Its dpi, method and codec are ND.
   - Mac has a profile "Scanned Pages (MRC)", whose parameters are ND.
   - The marketing article claims colour, grey and mono downsampling. This is not in the manual.
2. **FONT:** ND in the optimiser. The creation side offers Subset, Full or None.
3. **STRUCT**
   - Remove:
     - interactive content
     - "document content"
     - bookmarks
     - attachments
     - thumbnails
     - named destinations
     - "unused objects" (JavaScript and annotation objects)
     - invalid links only
   - ND: metadata, PieceInfo, object streams and linearisation.
4. **CS:** ND.
5. **PRESET**
   - Windows: Reduced Size, Reduced Size and Print Ready, Custom.
   - Mac: Web, Archive, Print, Minimal Size, Scanned Pages (MRC), Custom.
   - Saving presets: ND.
6. **AUDIT:** ND.
7. **GRAN:** whole document only.
8. **PREVIEW:** "Show Details" lists the changes as text. There is no visual preview.

## 4. Wondershare PDFelement (v11)

Sources:
- https://pdf.wondershare.com/pdfelement/PDFelement-v11-user-guide-win.pdf (pages 137–138)
- https://pdf.wondershare.com/guide/compress-pdf.html

1. **IMG:** one setting, "Compress image resolution to" a dpi (72–600, per a how-to page). Method, codec, quality and image class: ND.
2. **FONT:** ND.
3. **STRUCT**
   - "Remove duplicate resources" (on by default).
   - "Compress content streams" (on by default).
   - Discard:
     - bookmarks
     - attachments
     - Info dictionary and metadata
     - comments and forms
   - ND: JavaScript, thumbnails, tags, PieceInfo, object streams and linearisation.
4. **CS:** Flate compression of content streams only.
5. **PRESET**
   - High, Medium and Low, plus Custom.
   - A guide page lists five levels instead: Super Fast, Fast, Normal, Maximum, Ultra.
   - Batch mode exists.
   - Saving named presets: ND.
6. **AUDIT:** ND.
7. **GRAN:** whole document only.
8. **PREVIEW:** a how-to page (not the manual) says each level shows its estimated size.

## 5. ABBYY FineReader PDF (14/15/16)

Sources:
- https://help.abbyy.com/en-us/finereader/16/user_guide/reducefilesize/
- https://help.abbyy.com/en-us/finereader/16/user_guide/pdf_settings
- https://help.abbyy.com/en-us/finereader/14/user_guide/pdf_settings

1. **IMG**
   - Image-quality presets:
     - Best: keeps the original resolution
     - Balanced
     - Compact: 300 dpi
     - Custom: resolution, colour and the allowed quality loss
   - **MRC compression:**
     - needs OCR and the "text under page image" mode
     - works on recognised pages only
     - ABBYY claims files up to 10× smaller than JPEG and warns that some images may blur
   - PreciseScan smooths characters.
   - ND: choice of codec, thresholds for each image class, and image deduplication.
2. **FONT:** embed and subset at creation. No unembedding or merging.
3. **STRUCT**
   - Keep or drop: comments, links and media, actions and scripts, form data, bookmarks, attachments, metadata.
   - Create tags.
   - Fast Web View (FR14).
   - PDF/A and PDF/UA output.
   - ND: object streams and dedup.
4. **CS:** ND.
5. **PRESET:** the four image presets. The result opens in a new window, leaving the original intact. An "only if smaller" check: ND.
6. **AUDIT:** ND.
7. **GRAN:** whole document only. MRC is limited to recognised pages.
8. **PREVIEW:** none before the change. You inspect the output afterwards.

## 6. Kofax / Tungsten Power PDF

Sources (Optimize and Reduce help):
- https://docshield.tungstenautomation.com/PowerPDF/en_US/3.1.0_llem1i5177/help/Optimizing_PDF.htm
- https://docshield.tungstenautomation.com/PowerPDF/en_US/2025.3-jlrwz2ja2j/help/PowerPDF_cloudhelp/PowerPDF_cloudhelp/t_optimizepdf.html
- .../5.1.0-cyu29ubkgp/help/PowerPDF_help/c_compressioncontrols.html
- .../c_mrcpdfconversionsettings.html
- .../t_removedocumentelements.html

Power PDF has three separate features:
- **Reduce:** lossless removal of "duplicated, unseen" data. It changes no images and keeps comments.
- **Optimize:** an Acrobat-style dialog.
- **Compression Controls:** settings used when a PDF is created.

1. **IMG**
   - Optimize has separate colour, grey and mono settings, each with:
     - sampling: average downsampling, subsampling, or keep
     - target ppi
     - a **resolution threshold**
     - codec: JPEG, JPEG2000 or ZIP for colour and grey; JBIG2 for mono
     - quality
   - Other Optimize options:
     - **MRC Layers** (three-layer, for scanned pages)
     - deskew and despeckle
     - discard alternate images
     - **"Optimize images only in case of size reduction"**
   - Creation levels (Compression Controls):
     - Lossless 100, High Quality 75, Medium 50, Low 25, Placeholder 5, No Image 0
     - custom codec: JPEG or ZIP for colour; ZIP, CCITT or RunLength for mono
     - average or centre-pixel resampling
     - MRC quality: Minimal, Good or Superb
   - ND: conversion to grey or bilevel, and explicit image dedup.
2. **FONT:** unembed fonts (with a warning). Subset and merge: ND.
3. **STRUCT**
   - Optimize dialog:
     - compatibility level
     - fast web view
     - Flate-encode unencoded streams
     - remove tags
     - remove private application data
     - remove thumbnails
     - remove invalid bookmarks and links
     - discard comments, attachments, form and JavaScript actions, external cross-references, and alternate images
   - Remove Document Elements:
     - metadata
     - attachments
     - comments
     - bookmarks
     - JavaScript
     - hidden layers
     - hidden text
     - form fields
     - overlapping objects
     - links
     - embedded indexes
   - ND: object streams.
4. **CS:** ND. Reduce's "unseen information" is not explained.
5. **PRESET**
   - Optimize has a Reset button.
   - Creation controls can be created, edited and deleted.
   - The image "only if smaller" option above.
6. **AUDIT:** ND.
7. **GRAN:** per image class. Reduce can run as a batch.
8. **PREVIEW:** ND.

## 7. Apple Preview / Quartz filters / ColorSync Utility

Sources:
- https://support.apple.com/guide/preview/prvw1509/mac
- https://support.apple.com/en-lk/guide/colorsync-utility/csync006/mac
- https://meyerweb.com/eric/thoughts/2010/02/25/better-pdf-file-size-reduction-in-os-x/
- https://www.macscripter.net/t/thoughts-and-examples-about-using-the-quartzfilter-tool/51833

1. **IMG**
   - Preview's Export "Reduce File Size" filter has the options "Optimize images for screen" and "Save images as JPEG".
   - ColorSync's Image Effects component offers:
     - Image Sampling: scale %, plus maximum and minimum pixel size
     - Image Compression: a JPEG quality slider
   - Confirmed `.qfilter` plist keys: `Compression Quality` (0.0–1.0) and `ImageSizeMax` (a pixel cap on the longest side). `ImageScaleFactor` is unverified.
   - Downsampling works by pixel cap or scale, **not ppi**.
   - Conversion to grey is possible through the Color Management component (the "Gray Tone" filter).
   - ND: other codecs, MRC and dedup.
2. **FONT:** ND.
3. **STRUCT:** "Create Linearized PDF" only.
4. **CS:** ND.
5. **PRESET**
   - Filters are plist files in `/System/Library/Filters` and `~/Library/Filters`. They can be duplicated, locked and commented.
   - They can be applied from a script through PDFKit.
   - An "only if smaller" check: ND.
6. **AUDIT:** ND.
7. **GRAN:** whole document only.
8. **PREVIEW:** ColorSync can preview a filter's effect (unverified). Preview itself has none.

## 8. PDF Squeezer (Witt Software; Mac, iOS, Windows)

Sources:
- https://www.witt-software.com/pdfsqueezer/
- https://www.witt-software.com/en/faq/
- https://witt-software.com/the-new-pdf-squeezer-4/
- https://www.macworld.com/article/234641/pdf-squeezer-4-review.html

1. **IMG**
   - Profile settings:
     - quality and dpi
     - colour conversion: original, RGB, CMYK or **greyscale**
     - "colour complexity reduction"
     - **clip the invisible parts of images**
     - remove alternate images
     - force recompression
     - an "Optimize images" on/off switch
   - Removal of invisible images (iOS).
   - ND: codec choice, MRC and thresholds.
2. **FONT**
   - Remove unused glyphs (subset).
   - Strip the standard-14 fonts.
   - **Convert to CFF**.
   - **Merge compatible font programs**.
   - This is the richest font set among the GUI tools.
3. **STRUCT**
   - Remove or edit Info fields.
   - Strip object-level metadata.
   - Remove duplicate resources.
   - Remove thumbnails.
   - Remove application data.
   - Remove the structure tree.
   - Remove annotations (except fields and links), form fields, or links only.
   - Encryption is kept.
   - ND: object streams and linearisation.
4. **CS:** ND. A separate tool rasterises pages.
5. **PRESET**
   - Built-in Light, Medium and Strong, plus custom profiles exported as `.pdfscp` files.
   - Automation through Shortcuts, Automator, folder actions and a CLI.
   - The original is kept until you save, and the original modification date can be kept.
   - An automatic "only if smaller" check: ND.
6. **AUDIT:** the Inspector shows image count and **average image resolution**, and the app tracks total space saved. A breakdown per object: ND.
7. **GRAN:** one profile per run, with batch and recursive folder processing. Per-image control: ND.
8. **PREVIEW**
   - **Compare mode** (button or Space bar) shows the original and compressed versions side by side, including across different profiles.
   - This is the only true visual before/after preview found in the survey.

## 9. Online: Smallpdf, iLovePDF, Adobe Acrobat online

Sources:
- https://smallpdf.com/compress-pdf
- https://www.ilovepdf.com/uk/blog/how-to-reduce-pdf-file-size-online
- https://iloveapi.com/docs/pdf-guides/compress-pdf-api
- https://www.adobe.com/acrobat/online/compress-pdf.html
- https://helpx.adobe.com/document-cloud/help/compress-pdf.html

1. **IMG:**
   - **Smallpdf:** Basic, Moderate and Strong; Strong lowers image resolution.
   - **iLovePDF:** Less, Recommended and Extreme (API `compression_level=low|recommended|extreme`). iLovePDF says compression "isn't gradual", so there is no custom level.
   - **Adobe online:** High, Medium and Low; it lowers "image resolution where applicable".
   - None documents dpi, codec or MRC.
2. **FONT:**
   - **Smallpdf:** says it does not alter fonts.
   - **Adobe online:** says it may remove embedded fonts.
3. **STRUCT:** Adobe online says it removes unused metadata and flattens interactive layers, but keeps links, fields and signatures. The others: ND.
4. **CS:** ND for all three.
5. **PRESET:** level only. An "only if smaller" check: ND.
6. **AUDIT:** ND for all three.
7. **GRAN:** whole file only.
8. **PREVIEW:** Adobe online shows an **estimated output size for each level**. There is no visual preview.

## 10. qpdf (Apache-2.0)

Source: https://qpdf.readthedocs.io/en/stable/cli.html

1. **IMG**
   - `--optimize-images` converts images to JPEG (DCT) **only if the result is smaller**. It does not touch images that are already DCT.
   - `--jpeg-quality=0..100`.
   - Size thresholds: `--oi-min-width` / `--oi-min-height` (128 px) and `--oi-min-area` (16384 px).
   - Inline images:
     - `--keep-inline-images` leaves them out of image optimisation
     - `--externalize-inline-images` (with `--ii-min-bytes`, default 1024) turns them into image objects
   - It does no downsampling, no conversion to grey or bilevel, no JBIG2 and no image dedup.
2. **FONT:** none.
3. **STRUCT**
   - Object streams: `--object-streams=preserve|disable|generate`.
   - Stream compression:
     - `--compress-streams`
     - `--stream-data=compress|preserve|uncompress`
     - `--decode-level=none|generalized|specialized|all`
     - **`--recompress-flate`**
     - **`--compression-level=1..9`**
   - Unused data:
     - `--remove-unreferenced-resources=auto|yes|no`
     - unreachable objects are dropped by default (`--preserve-unreferenced` keeps them)
   - Removal options:
     - `--remove-info` and `--remove-metadata`
     - `--remove-structure` (removes `/StructTreeRoot` and `/MarkInfo`)
     - `--remove-page-labels`
     - `--remove-acroform` (removes the catalog entry only)
     - `--remove-attachment=key`
   - `--flatten-annotations=all|print|screen`.
   - **`--linearize`**.
   - ND: thumbnails, JavaScript, PieceInfo, optional content and object dedup.
4. **CS**
   - `--coalesce-contents` merges a page's content streams.
   - `--normalize-content` normalises newlines only.
   - `--flatten-rotation`.
   - No precision rounding.
5. **PRESET:** none (no config file). The image "only if smaller" rule is built in.
6. **AUDIT**
   - `--json` (keys: objects, pages, attachments, outlines, acroform, objectinfo).
   - `--show-object`.
   - `--show-pages --with-images`.
   - There is no breakdown of bytes by category.
7. **GRAN:** `--pages` selects pages and ranges, and `--show-object=N` inspects one object. Image optimisation applies to the whole file.
8. **PREVIEW:** none.

## 11. cpdf: Coherent PDF (AGPL or commercial; documentation only)

Sources:
- https://coherentpdf.com/cpdfmanual/cpdfmanual.html (chapter 5 squeeze, chapter 13 images, chapter 14 fonts, chapter 19 structure)

1. **IMG**
   - `-process-images [range]` uses ImageMagick, plus jbig2enc for JBIG2. Options:
     - existing JPEGs: `-jpeg-to-jpeg q`, `-jpeg-to-jpeg-scale %`, `-jpeg-to-jpeg-dpi n`
     - lossless images to a lossy codec: `-lossless-to-jpeg q` and `-lossless-to-jpeg2000 q`
     - lossless images, downsampled: `-lossless-resample[-dpi]` and `-resample-interpolate`
     - existing JPEG2000: `-jpeg2000-to-jpeg2000 q`
     - 1-bit images: `-1bpp-method JBIG2|JBIG2Lossy|CCITTG4|CCITTG3`, with `-jbig2-lossy-threshold` (lossy mode uses shared JBIG2Globals)
   - Thresholds:
     - `-pixel-threshold` (25)
     - `-length-threshold` (100 B)
     - `-dpi-threshold`: an image is processed only if it is above this dpi **at every place it is used**
     - **`-percentage-threshold` (99)**: the new image is kept only if it is under 99% of the original's size
   - `-draft` strips images. **`-draft-remove-only <name>`** removes a single named image.
   - `-rasterize` uses Ghostscript.
   - ND: conversion to grey and image dedup.
2. **FONT**
   - `-remove-fonts` unembeds.
   - `-embed-missing-fonts` uses Ghostscript.
   - ND: subset, merge and Type1→CFF conversion.
3. **STRUCT**
   - **`-squeeze`**: lossless, about 20% smaller on average. Its steps are not listed; the log shows the object count falling, then "Squeezing page data", then "Recompressing document".
   - Compression: `-compress` and `-decompress`.
   - Object streams: `-create-objstm` and `-no-preserve-objstm`.
   - Linearisation: `-l`, using an external linearizer.
   - Removal options:
     - `-remove-metadata`, `-remove-all-metadata`
     - `-remove-annotations`
     - `-remove-files`
     - `-remove-javascript`
     - `-remove-struct-tree`
     - **`-remove-page-piece`** (PieceInfo)
     - `-remove-article-threads`, `-remove-web-capture`, `-remove-procsets`, `-remove-output-intents`
     - `-remove-page-labels`, `-remove-id`
     - **`-remove-dict-entry <key> [-dict-entry-search]`**, which removes any dictionary key
   - `-ocg-coalesce-on-name` merges optional-content groups that share a name.
   - Garbage collection is the default.
4. **CS:** squeeze's "page data" step (not explained). Its `-blacktext` and similar options change appearance, not size.
5. **PRESET:** control files can script runs. `-percentage-threshold` is the "only if smaller" safeguard; `-process-images-force` overrides it.
6. **AUDIT**
   - **`-composition[-json]`**: bytes and % for images, fonts, content streams, structure info, attachments, xref, PieceInfo and unclassified.
   - **`-list-images[-json]`**: object number, pages, bytes, bits per pixel, colour space, filter and mask for each image.
   - `-image-resolution`.
   - `-process-images-info`: % reduction per image.
   - `-squeeze-log-to`.
7. **GRAN:** page ranges (including `NOT`, `odd`/`even`, `annotated`) and a single image by name (`-draft-remove-only`). Per-image processing options: ND.
8. **PREVIEW:** none.

## 12. pdfsizeopt (GPL-2.0; documentation only)

Sources:
- https://github.com/pts/pdfsizeopt (README)
- Design paper: https://github.com/pts/pdfsizeopt/releases/download/docs-v1/pts_pdfsizeopt2009.psom.pdf

1. **IMG**
   - **Lossless only.** Each image goes through many optimisers (sam2p, jbig2, pngout, zopflipng, optipng, advpng, ECT and imgdataopt, chosen with `--use-image-optimizer=`). The **smallest candidate wins**, and the original is one of the candidates.
   - Dedup compares the decoded RGB pixels, so duplicates are found even when stored with a different filter, colour space or bit depth.
   - No downsampling and no JPEG work.
2. **FONT**
   - **Type1→CFF conversion** (through Ghostscript).
   - **Merging of subsets of the same CFF font** (`--do-unify-fonts`).
   - `--do-optimize-fonts`.
3. **STRUCT**
   - Removes duplicate objects by equivalence classes, so duplicate cycles are also collapsed.
   - Removes unreachable objects.
   - Compact serialisation:
     - no whitespace or comments
     - hex strings written as binary
     - object numbers renumbered without gaps
     - objects referenced most often get the lowest numbers
   - Xref streams use a TIFF predictor.
   - Interactive features are kept.
   - ND: linearisation.
4. **CS:** extracting shared content into XObjects is listed only as future work.
5. **PRESET:** flags only. The smallest-candidate rule is built in, and it restores the original image when Multivalent's output is bigger.
6. **AUDIT:** `--stats` (bytes per category) is described in the 2009 paper. Whether it still exists: unverified.
7. **GRAN:** whole file only.
8. **PREVIEW:** not applicable (lossless).

## 13. Ghostscript pdfwrite (AGPL; documentation only)

Sources:
- https://ghostscript.readthedocs.io/en/latest/VectorDevices.html
- https://ghostscript.readthedocs.io/en/latest/Use.html

**Behaviour.** pdfwrite **rebuilds the whole document** rather than editing it. By default it loses anything that does not mark the page: links, bookmarks unless kept by pdfmarks, comments, widget and link annotations, and the structure tree (tags). The docs never mention AcroForm. Shapes may be rewritten; for example, rectangles can become long path descriptions.

These settings keep content: `PreserveAnnots` (markup), `WantsOptionalContent`, `PreserveMarkedContent`, and form XObjects (kept unless `-dUNROLLFORMS`). Below `CompatibilityLevel` 1.4 transparency is turned into a bitmap.

1. **IMG**
   - Downsampling, set separately for **Color, Gray and Mono**:
     - `Downsample*Images` turns it on
     - `*ImageDownsampleType` is `Subsample`, `Average` or `Bicubic`; non-integer scale factors are clamped to an integer
     - `*ImageResolution` sets the target
     - `*ImageDownsampleThreshold` defaults to 1.5× the target
   - Encoding:
     - `Encode*Images`
     - `*ImageFilter`: DCT for colour and grey, CCITTFax for mono; JPX output is ND
     - **`AutoFilter*Images`** chooses DCT or Flate based on the image content
   - QFactor is set in `*ACSImageDict` / `*ImageDict`, which needs PostScript.
   - Pass-through: `PassThroughJPEGImages` and `PassThroughJPXImages` copy JPEG and JPEG2000 data without re-encoding.
   - **`-dDetectDuplicateImages`** (on by default).
   - `MaxInlineImageSize` (4000 B).
   - Colour:
     - `ColorConversionStrategy` (LeaveColorUnchanged, Gray, RGB, CMYK or UseDeviceIndependentColor)
     - `ConvertCMYKImagesToRGB`
   - ND: JBIG2.
2. **FONT**
   - `EmbedAllFonts`, `SubsetFonts`, `MaxSubsetPct` and `CompressFonts`.
   - `AlwaysEmbed` / `NeverEmbed` (PostScript only).
   - `-dNoOutputFonts` turns text into outlines.
3. **STRUCT**
   - `CompatibilityLevel` 1.3–2.0.
   - Object and xref streams: **`-dWriteObjStms`** and **`-dWriteXRefStm`** (on by default from PDF 1.5).
   - `-dFastWebView` (slightly larger output).
   - `CompressPages` and `CompressStreams`.
   - `-dOmitInfoDate`.
   - Conformance: `-dPDFA`, `-dPDFX`, `PDFACompatibilityPolicy`.
4. **CS:** every content stream is regenerated, as a side effect of the rebuild. `TransferFunctionInfo` and `UCRandBGInfo` can remove or preserve that data. There is no separate minifier.
5. **PRESET**
   - **`-dPDFSETTINGS=/screen|/ebook|/printer|/prepress|/default`**:

     | Preset | Colour / grey ppi | Mono ppi | JPEG QFactor | Colour conversion | Compatibility |
     |---|---|---|---|---|---|
     | /screen | 72 | 300 | 0.76 | RGB | 1.5 |
     | /ebook | 150 | 300 | 0.76 | RGB | 1.5 |
     | /printer | 300 | 1200 | 0.4 | UseDeviceIndependentColor | 1.7 |
     | /prepress | 300 | 1200 | 0.15 | LeaveColorUnchanged | 1.7 |
     | /default | 72 | 300 | 0.9 | LeaveColorUnchanged | 1.7 |

   - In every preset the threshold is 1.5, `AutoFilter` is on and fonts are subset.
   - The docs warn that "/prepress is not highest quality"; the best quality comes from setting no preset at all.
   - There is **no "only if smaller" check** at any level.
6. **AUDIT:** none.
7. **GRAN:** page selection (`-dFirstPage`/`-dLastPage`, `-sPageList`), `-dShowAnnots=false` and `-dShowAcroForm=false`. Settings apply per image class for the whole job.
8. **PREVIEW:** none. You would have to render the output separately.

## 14. MuPDF: mutool clean / recolor / audit (AGPL; documentation only)

Sources:
- https://mupdf.readthedocs.io/en/latest/tools/mutool-clean.html (1.28.5)
- https://mupdf.readthedocs.io/en/latest/reference/common/pdf-write-options.html
- https://mupdf.readthedocs.io/en/1.27.1/tools/mutool-audit.html
- https://man.archlinux.org/man/mutool.1.en.txt

**Behaviour.** clean rewrites the file object by object, so it keeps objects rather than rebuilding the whole document.

1. **IMG**
   - Each image option exists for **color, gray and bitonal × lossy or lossless source images**:
     - `--…-image-subsample-method=average|bicubic`
     - `--…-image-subsample-dpi=dpi[,dpi]` (the second value's meaning is ND; it is probably a threshold)
     - `--…-image-recompress-method=never|same|lossless|jpeg|j2k|fax|jbig2`
   - **`--recompress-images-when=smaller|always`** (default: smaller).
   - `-i` compresses images.
   - `mutool recolor -c gray|rgb|cmyk` converts the colour of the whole document.
   - ND: a numeric JPEG quality.
2. **FONT:** `-S` subsets fonts (marked experimental), and `-f` compresses fonts. `-v` turns all text into vectors.
3. **STRUCT**
   - Garbage collection:
     - `-g`: remove unused objects
     - `-gg`: also compact the xref
     - `-ggg`: also merge duplicate objects
     - `-gggg`: also merge duplicate streams
   - **`-Z`**: object streams plus an xref stream.
   - Compression:
     - `-z`
     - `compress=flate|brotli` (Brotli is a *proposed* filter)
     - **`-e` / `compress-effort` 0–100**
   - `--structure=keep|drop` keeps or drops tags.
   - `-m` keeps metadata.
   - Linearisation: `-l` is "no longer supported".
   - Encryption: `-D` removes it, `-E` re-encrypts.
   - Write options: `incremental`, `reproducible`, `regenerate-id`.
4. **CS**
   - **`-s` sanitize** cleans graphics commands.
   - `-c` pretty-prints them.
   - `-A` / `-AA` creates missing appearance streams or recreates all of them.
   - `mutool bake` flattens annotations and widgets into the page.
   - `mutool trim` removes content outside a page box.
5. **PRESET:** none. The image "only if smaller" rule is the default.
6. **AUDIT:** **`mutool audit`** writes an HTML report with three parts:
   - objects versus overhead, split by inside and outside object streams, as % of the file
   - usage by object type
   - **how often each graphics operator is used**
7. **GRAN:** the page list `1,5,10-15,20-N`. Image options apply per class. Choosing a single object: ND.
8. **PREVIEW:** none. You would render with `mutool draw` and compare the images yourself.

## 15. pdfcpu (Apache-2.0)

Sources:
- https://pdfcpu.io/core/optimize
- https://pdfcpu.io/config/config_list
- https://pdfcpu.io/images/images_list
- https://pdfcpu.io/images/images_update

1. **IMG**
   - `optimize` removes redundant (duplicate) images.
   - `images list` gives, for each image: page, object number, id, size, filters and colour space.
   - **`images update`** replaces one image, chosen by object number or by page plus resource id.
   - No recompression and no downsampling.
2. **FONT:** removes redundant (duplicate) fonts. Subset, unembed and Type1→CFF: ND.
3. **STRUCT**
   - Config keys:
     - `optimizeResourceDicts`: removes unused resources by analysing the content streams
     - **`optimizeDuplicateContentStreams`**: dedupes content streams that pages share
     - `writeObjectStream` and `writeXRefStream`
   - `optimize` also has `rmenc` and `rmsig`.
   - It drops linearisation and does not linearise.
   - ND: removal of metadata, JavaScript or PieceInfo as part of optimise.
4. **CS:** only the cross-page dedup above.
5. **PRESET:** `config.yml` is used as a profile (`-c`). An "only if smaller" check: ND.
6. **AUDIT**
   - **`optimize -stats file.csv`**: one row per file, with source and destination size, binary data split into images, fonts and other, linearised/objstm/xrefstm flags, and about 60 flags for catalog and page entries that are present (`R_PieceInfo`, `P_Thumb`, `R_StructTreeRoot`, `R_OCProperties` and so on).
   - `-v` prints an original-versus-optimised breakdown.
7. **GRAN:** `images update` works on a single object. Whether `optimize` accepts `-p` for a page selection: ND.
8. **PREVIEW:** none.

## 16. callas pdfToolbox

Sources:
- https://help.callassoftware.com/m/pdftoolbox/l/1478828 (downsample)
- https://help.callassoftware.com/m/pdftoolbox/l/1477036 (JPEG2000)
- https://help.callassoftware.com/m/pdftoolbox/l/1332219 (save optimisation)
- https://help.callassoftware.com/m/pdftoolbox/l/1923705 (space usage)
- https://help.callassoftware.com/m/pdftoolbox/l/1617865 (Apply-to)
- https://help.callassoftware.com/m/pdftoolbox/l/1086545 (conditions)
- https://help.callassoftware.com/m/pdftoolbox/l/1919184 (modify images)
- https://help.callassoftware.com/m/pdftoolbox/l/946820 (subset)

1. **IMG**
   - "Downsample/compress images" (and an Upsample fixup):
     - methods: **Average, Subsampling, Bicubic, Lanczos, BlackmanSinc**
     - a target ppi, applied "for images above" a threshold
   - Codecs: retain, retain but change quality, JPEG2000, JPEG or ZIP. Quality is 0–100, mapped to Minimum–Maximum.
   - "Apply recompression even if not downsampled".
   - **Each use of a shared image is checked separately.**
   - The JPEG2000 action has a **"Discard results if bigger than original"** option.
   - Identical images and forms are merged by MD5 on every save.
   - **Modify images** (v16): export the images with a JSON map, edit them outside pdfToolbox, then put them back in place.
   - ND: JBIG2, MRC and conversion to bilevel. Conversion to grey uses the general "Convert colors" fixup.
2. **FONT**
   - Subset (a full rebuild).
   - Embed, substitute, unembed all.
   - Convert to outlines, including Type 3.
   - **"Merge identical fonts and encodings"** (save option `--optimizepdf`).
   - ND: Type1→CFF conversion.
3. **STRUCT**
   - Every save: removes unreferenced objects, merges identical images and forms, and optimises content streams.
   - Optional: merge fonts, linearise. `--nooptimization` turns this off.
   - The space-usage dialog has checkboxes to remove:
     - attachments
     - annotations
     - form data
     - **private data**
     - XFA
     - multimedia
     - object metadata
     - structure
   - ND: object streams.
4. **CS:** content streams are optimised by building shared substreams. No precision work.
5. **PRESET**
   - **Profiles** (checks plus fixups), **Libraries** and **Process Plans**.
   - Process Plans run steps in order, can branch on whether a check succeeded, failed or found something, and can switch steps with variables.
   - "Only if smaller" exists for each image, in the JPEG2000 action only.
6. **AUDIT:** **Space usage** (v16) gives bytes and % for each category (content, ExtGState, colour spaces, fonts, images, annotations, shadings and more). Each category row has a removal checkbox, so the audit doubles as the action. Drilling down to single objects: ND.
7. **GRAN: selecting by condition.**
   - A fixup's **"Apply to"** setting takes a Check. The fixup then runs only on the objects that Check matches.
   - A Check's conditions combine with AND or OR (both can be mixed since v10).
   - For example: images where colour space = X and resolution > Y, then Downsample.
   - There is no way to pick one image by hand in the GUI.
8. **PREVIEW:** no preview inside the fixups. The separate **Compare** feature shows pixel or ΔE differences between two PDFs (https://cdn-www.enfocus.com/sites/default/files/media/images/appstore/product_documentation/1689105793/callas_pdftoolbox_compare.pdf).

## 17. Enfocus PitStop Pro / Server

Sources:
- https://cdn.enfocus.com/manuals/Extra/GlobalChanges/18/en-us/common/glch/concept/glch_resample_images.html
- https://cdn.enfocus.com/manuals/Extra/GlobalChanges/18/en-us/common/glch/concept/glch_minimize_file_size.html
- https://cdn.enfocus.com/manuals/Extra/Actions/18/en-us/common/act/concept/25.html
- https://cdn.enfocus.com/manuals/Extra/Actions/18/en-us/common/act/concept/377.html
- https://cdn.enfocus.com/manuals/Extra/Actions/18/en-us/common/act/concept/196.html
- https://cdn.enfocus.com/manuals/Extra/Actions/12/en-us/common/act/concept/290.html
- https://cdn.enfocus.com/manuals/Extra/Actions/12/en-us/common/act/concept/228.html
- https://cdn.enfocus.com/manuals/ReferenceGuide/PP/11/enUS/en-us/task/t_aa1135571.html
- https://www.enfocus.com/en/pitstop-2507-brings-significant-productivity-boosts

1. **IMG**
   - "Resample Images" sets each image class (colour, grey, 1-bit) separately:
     - methods: keep, Average, Subsample, **Bicubic, Bilinear, Bicubic B-Spline**
     - a target ppi, with a "Resample above" threshold
   - Codecs:
     - colour and grey: keep, none, ZIP, JPEG, JPEG2000 (can be lossless)
     - 1-bit: keep, none, ZIP, CCITT G3/G4, RunLength
   - A trap: any change to an image re-encodes it at the default settings (JPEG at maximum quality). The "Override default image recompression" action fixes this.
   - Other actions:
     - convert to an indexed colour space
     - **crop images to their visible area**
     - remove alternate images (keep the default, the lowest or the highest)
     - rasterise
   - v25.07 adds "optimise stream compression" and ZIP on top of JPEG.
   - ND: JBIG2, MRC and image dedup.
2. **FONT**
   - Embed fully or as a subset.
   - Unembed.
   - Strip embedded OpenType.
   - Convert to outlines.
   - The Minimize File Size global change has "Subset all fonts".
   - ND: merging and Type1→CFF conversion.
3. **STRUCT**
   - "Optimize document structure" (options not documented).
   - The Remove metadata action removes:
     - annotations
     - thumbnails
     - article threads
     - page labels
     - Capture info
     - bookmarks
     - structure
     - job ticket
     - named destinations
     - metadata streams
   - Further removals:
     - actions
     - attachments
     - form fields (or flatten them)
     - JavaScript
     - marked content
     - unused destinations
     - Certified PDF data
     - empty layers
     - invisible objects
     - objects outside the page box
     - OPI
     - PostScript
   - ND: linearisation and object streams.
4. **CS:** v25.07 refines lines "for efficiency". No precision controls are documented.
5. **PRESET**
   - The **Minimize File Size** global change.
   - The "Optimize PDF" Action List (outside-media-box → discard objects → discard user data → clean up → images).
   - Action Lists and Preflight Profiles; Server runs them from hot folders.
   - An "only if smaller" check: ND.
6. **AUDIT:** no byte report. Preflight checks ("Check resolution", "Check optimal compression") list problem objects in the Navigator.
7. **GRAN** (the most granular tool surveyed):
   - **Selection actions** choose objects to act on: by effective resolution (a ppi range, for each colour class), by pixel size, by filter type, by image type, by alternates, by OPI, and more.
   - **Logical operators** combine selections with AND, OR and NOT. Later actions apply only to the selection.
   - **Inspector (Pro):** select one image by hand, then Resample (threshold, method, dpi) or Compress (JPEG quality or ZIP) just that image.
   - From preflight results you can fix all flagged objects, or each one with its own values.
8. **PREVIEW:** none. You re-run preflight to check.

---

## Cross-tool capability matrix

Key:
- **Y** documented
- **P** partial or limited
- **–** not documented or absent

Tool abbreviations: XC PDF-XChange, FX Foxit, NI Nitro, PE PDFelement, AB ABBYY, KP Power PDF, AP Apple, SQ PDF Squeezer, ON online tools, QP qpdf, CP cpdf, SO pdfsizeopt, GS Ghostscript, MU MuPDF, PC pdfcpu, CA callas, PS PitStop.

| Capability | XC | FX | NI | PE | AB | KP | AP | SQ | ON | QP | CP | SO | GS | MU | PC | CA | PS |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Downsample by class (colour/grey/mono) | Y+idx | Y | P | P | P | Y | P(px) | P | P | – | P | – | Y | Y(×lossy/lossless) | – | P | Y |
| Resolution threshold | Y | Y | – | – | – | Y | – | – | – | P(px) | Y | – | Y | P | – | Y | Y |
| Resample methods beyond avg/sub/bicubic | – | – | – | – | – | – | – | – | – | – | – | – | – | – | – | Y(Lanczos, BlackmanSinc) | Y(bilinear, B-spline) |
| Choice of JPEG / JPEG2000 / Flate | Y | Y | – | – | – | Y | P | – | – | P | Y | – | P | Y | – | Y | Y |
| Content-adaptive codec choice | – | – | – | – | – | – | – | – | – | – | – | Y(smallest wins) | Y(AutoFilter) | – | – | – | – |
| JBIG2 (lossy or lossless) | Y | Y | – | – | – | Y | – | – | – | – | Y | Y | – | Y | – | – | – |
| CCITT G4 | Y | Y | – | – | – | P | – | – | – | – | Y | – | Y | Y | – | – | Y |
| MRC / segmentation | – | P? | P | – | Y | Y | – | – | – | – | – | – | – | – | – | – | – |
| Convert to grey or bilevel | – | – | – | – | P | – | P | Y | – | – | – | – | Y | Y | – | P | P(indexed) |
| Image dedup | Y | – | – | Y | – | P | – | Y | – | – | – | Y(pixel-level) | Y | Y(-gggg) | Y | Y(MD5) | – |
| Crop clipped or outside-page image data | P | – | – | – | – | – | – | Y | – | – | – | – | – | P(trim) | – | – | Y |
| Font subset | – | Y | – | – | Y | – | – | Y | – | – | – | P | Y | Y(exp.) | – | Y | Y |
| Font unembed | Y | Y | – | – | – | Y | – | Y(std-14) | – | – | Y | – | Y | – | – | Y | Y |
| Merge duplicate font subsets | Y | – | – | – | – | – | – | Y | – | – | – | Y | – | – | P | Y | – |
| Type1 → CFF | – | – | – | – | – | – | – | Y | – | – | – | Y | – | – | – | – | – |
| Object and xref streams | – | – | – | – | – | – | – | – | – | Y | Y | P | Y | Y | Y | – | – |
| Flate recompression at higher effort | – | – | – | – | – | – | – | – | – | Y | P | Y | – | Y | – | – | P |
| Remove unused objects or resources | P | – | P | – | – | P | – | P | – | Y | Y | Y | Y | Y | Y | Y | P |
| Object or stream dedup | P | – | – | Y | – | P | – | Y | – | – | – | Y | – | Y | Y | Y | – |
| Strip metadata, thumbnails and JavaScript | Y | Y | P | P | P | Y | – | Y | P | P | Y | – | P | P | – | Y | Y |
| Strip PieceInfo / private data | Y | Y | – | – | – | Y | – | Y | – | – | Y | – | Y* | – | – | Y | P |
| Strip tags | – | Y | – | – | Y | Y | – | Y | – | Y | Y | – | Y* | Y | – | Y | Y |
| Optional content (flatten or remove hidden layers) | Y | Y | – | – | – | Y | – | – | – | – | P | – | P | – | – | – | Y |
| Linearise | – | Y | – | – | Y | Y | Y | – | – | Y | Y(ext) | – | Y | –(dropped) | – | Y | – |
| Content-stream minify or sanitise | – | P | – | – | – | – | – | – | – | P | P | – | Y* | Y | P | Y | P |
| Number-precision rounding | – | – | – | – | – | – | – | – | – | – | – | – | – | – | – | – | – |
| Saved custom profiles | Y | Y | – | – | – | P | Y | Y | – | – | P | – | P | – | Y | Y | Y |
| Only if smaller (per image) | Y | Y | – | – | – | Y | – | – | – | Y | Y | Y | – | Y | – | P | – |
| Only if smaller (whole file) | – | – | – | – | – | – | – | – | – | – | – | – | – | – | – | – | – |
| Space audit by category | P | P | – | – | – | – | – | – | – | – | Y | P | – | Y | Y | Y | – |
| Per-image listing (bytes, ppi, filter) | – | – | – | – | – | – | – | P(avg) | – | P | Y | – | – | – | Y | – | P(preflight) |
| Rule-based object selection | – | – | – | – | – | – | – | – | – | P(px) | P(dpi, %) | – | – | P(class) | – | Y | Y |
| Pick and handle one image by hand | – | – | – | – | – | – | – | – | – | – | P(remove) | – | – | – | Y(replace) | P(export) | Y |
| Page-range scope | – | – | – | – | – | – | – | – | – | Y | Y | – | Y | Y | ? | P | P |
| Size estimate before running | – | Y | – | P | – | – | – | – | Y(Adobe) | – | – | – | – | – | – | – | – |
| Visual before/after preview | – | – | – | – | – | – | P? | Y | – | – | – | – | – | – | – | P(Compare) | – |

\* Ghostscript "strips" by side effect: the rebuild drops anything not explicitly kept. That is a destructive default, not a deliberate choice of what to remove.

---

## Capabilities no surveyed tool offers well

Each gap below was observed in the survey above. None is assumed.

1. **Per-image table with predicted savings.**
   - What exists:
     - cpdf `-list-images` and pdfcpu `images list` list images but predict nothing.
     - cpdf `-process-images-info` reports savings only after the run.
     - PitStop's Inspector acts on one image but shows no estimate.
   - Gap: no tool shows a sortable table (object, pages, effective ppi at each use, colour class, filter, bytes, % of file) with a **predicted size under the current settings** and a per-row override (keep, use profile, or custom).
2. **Visual diff tied to the optimiser.**
   - What exists:
     - Only PDF Squeezer has a side-by-side view.
     - callas Compare (pixel or ΔE) is a separate tool.
   - Gap: no one shows a per-page or per-image difference heatmap with a zoom to the worst region **inside** the optimise flow.
3. **Quality target instead of a quality number.**
   - What exists: every tool exposes a fixed JPEG Q or a named level (callas 0–100, Foxit Minimum–Maximum).
   - Gap: none offers "the largest compression that keeps SSIM, butteraugli or ΔE at or above X" for each image, or a search for the codec and quality that meets a target.
4. **Whole-file "only if smaller" plus a per-step ledger.**
   - What exists:
     - The per-image guard is common (XC, FX, KP, qpdf, cpdf, MuPDF, pdfsizeopt).
     - pdfsizeopt's smallest-candidate rule covers images only.
   - Gap: no tool guarantees the output never grows (Ghostscript's `-dFastWebView` documents a *larger* file). No tool reports which step saved how many bytes, or rolls back a step that did not pay off. pdfsizeopt's "try every candidate, keep the smallest" applies only to lossless images; no one generalises it to fonts, structure, object streams and Flate level.
5. **Signature-preserving incremental optimisation.**
   - What exists:
     - Every tool rewrites the whole file.
     - qpdf `--remove-restrictions` and pdfcpu `rmsig` invalidate or remove signatures.
     - Adobe online claims to keep signatures but documents no mechanism.
   - Gap: no tool documents optimising only the objects after the last signature's `/ByteRange`, by incremental update, while reporting which signed revisions are untouched.
6. **Per-use-point effective resolution.**
   - What exists:
     - callas evaluates each placement of a shared image.
     - cpdf's `-dpi-threshold` requires an image to be above the threshold at every use.
   - Gap: no tool resamples to the **maximum effective ppi across all uses**, then explains that one image was kept at 412 ppi because page 7 scales it up.
7. **Content-stream precision and operator simplification.**
   - What exists: no tool documents rounding numbers to a precision based on device space (for example 0.01 pt), merging redundant `q/Q` pairs, collapsing repeated `cm`, or removing no-op state changes. Foxit removes only newlines; MuPDF's `-s` sanitise is undocumented in detail; PitStop's "refine lines" is vague.
   - Why it matters: this is a large, unexplored saving for CAD and vector-heavy PDFs, which are pdfcer's core audience. It must stay within the visual tolerance that Rule 4 requires to be disclosed.
8. **Audit that drills down and leads to action.**
   - What exists:
     - Audits stop at categories: callas, cpdf `-composition`, MuPDF `audit`, pdfcpu `-stats`.
     - Only callas attaches removal checkboxes, and only per category.
   - Gap: none drills category → object → page with byte attribution (shared objects split by use), nor lets you act on a single row.
9. **Font dedup across differing subsets, with re-subsetting.**
   - What exists: merging is offered by XC, SQ, pdfsizeopt (same name, CFF only) and callas.
   - Gap: none documents merging subsets of the same base font **into one rebuilt subset that covers every glyph used**, with a check of glyph and width equivalence and a report of what differed. Type1→CFF exists only in pdfsizeopt and PDF Squeezer.
10. **Disclosure of every lossy change.**
    - What exists: lossy changes are silent everywhere. The worst case is Ghostscript's dropped structure and forms, which is documented in the manual but never reported at run time.
    - Gap: a post-run report listing every lossy or removing action (image X: 600→150 ppi, JPEG Q≈0.82, SSIM 0.987; tags removed; 3 fonts unembedded) is absent everywhere. pdfcer's Rule 4 requires it anyway.
11. **MRC with documented, tunable parameters.**
    - What exists:
      - ABBYY needs OCR.
      - Power PDF offers only Minimal, Good or Superb.
      - Nitro and Foxit's "High Compression" are undocumented.
    - Gap: no tool exposes foreground/background resolution, mask codec (JBIG2 or CCITT), background JPEG or JPEG2000 quality, or segmentation thresholds, nor shows the layers.
12. **Unified rule engine and manual override in one model.**
    - What exists: callas (declarative Check plus "Apply to") and PitStop (imperative AND/OR/NOT selections) are the only rule engines. Only PitStop has hand-picked overrides, and they are separate from its rules.
    - Gap: a rule set where a manual per-object override sits on top of the rule result and is saved in the profile (by stable object fingerprint) is absent.
13. **Scope by page range for structure work.**
    - What exists: page ranges exist in CLI tools for images and rewriting, but no GUI optimiser scopes to page ranges.
    - Gap: no tool scopes structure removals (annotations, layers) to a page selection.
14. **Byte budget.**
    - What exists: Smallpdf hints at "compress to chosen size" (unverified).
    - Gap: no tool documents "fit under N MB" through an optimiser that lowers quality step by step and reports the quality reached.
15. **Better entropy coding with selectable effort.**
    - What exists: only qpdf (`--compression-level`), MuPDF (`compress-effort`, Brotli marked as proposed) and pdfsizeopt (zopfli-class PNG optimisers) go beyond default Flate effort.
    - Gap: no GUI tool exposes zopfli-class Flate for content, font and xref streams, with a cost/time estimate.

**For pdfcer's spec-fidelity check (Rule 1):**
- Brotli (`/BrotliDecode`) is a proposed PDF feature, not ISO 32000-2. Any use must be opt-in and labelled as non-standard.
- JBIG2 lossy symbol substitution has a known risk of swapping characters. It must be disclosed (Rule 4) and should default to off for text-bearing scans.
