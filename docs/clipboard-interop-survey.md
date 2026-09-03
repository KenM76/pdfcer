# Clipboard interop survey — Windows, vector + alpha-raster paste targets

Date: 2026-09-03. Scope: what pdfcer must place on the Windows clipboard so a
page-content selection pastes as **vector** into Microsoft 365 Word/PowerPoint/
Excel, Inkscape 1.3/1.4, LibreOffice 24.x, and as **alpha raster** everywhere
else. Sources are primary (application source code at pinned branches, vendor
docs, engineering PSAs) unless marked `[secondary]` or `[unverified]`. Line
numbers cite the fetched revision named in the row. Licence classes follow
`LEGAL.md` §6.1.

Format-name convention below: `CF_*` = Win32 predefined id; `"name"` =
string passed to `RegisterClipboardFormat`.

---

## 0. Windows clipboard facts that constrain everything

| Fact | Source |
|---|---|
| Same `RegisterClipboardFormat` name from two processes → same id; that is how apps share private formats. | MS Learn, Clipboard Formats §Registered ([link](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats)) |
| Place the most descriptive format **first**; pasting apps "typically retrieve a clipboard object in the first format it recognizes"; `EnumClipboardFormats` returns placement order. | same, §Multiple Clipboard Formats |
| System **synthesizes** `CF_DIB`/`CF_BITMAP`/`CF_PALETTE` from `CF_DIBV5`, and `CF_METAFILEPICT` from `CF_ENHMETAFILE` (and the reverse). "No advantage to placing the conversion format(s)." Enumeration lists the real format first, then the synthesized ones. | same, §Synthesized Clipboard Formats |
| `CF_DIBV5` is the only *standard* format carrying alpha (BITMAPV5HEADER, BI_BITFIELDS); `CF_ENHMETAFILE` handle is an `HENHMETAFILE`, not an `HGLOBAL` (`TYMED_ENHMF`). | same; LibreOffice `DataFmtTransl.cxx` 173-179 (`CF_ENHMETAFILE` → `TYMED_ENHMF`) |
| Cloud-clipboard / history opt-out formats: `"ExcludeClipboardContentFromMonitorProcessing"`, `"CanIncludeInClipboardHistory"`, `"CanUploadToCloudClipboard"` (DWORD 0/1). Relevant for multi-MB EMF/PDF payloads. | same, §Cloud Clipboard |

---

## 1. Per-application vector paste formats

### 1.1 Microsoft 365 Word / PowerPoint / Excel (Windows)

| Claim | Evidence | Confidence |
|---|---|---|
| Office reads a registered format literally named **`"image/svg+xml"`** and inserts it as a native, editable SVG graphic (same object as Insert → Pictures → .svg). | Chromium registers `SvgType()` as `RegisterClipboardFormat(CFSTR_MIME_SVG_XML)` = `"image/svg+xml"` (`clipboard_format_type_win.cc` 142-145; `clipboard_constants.h` 36-37). Blink-dev PSA by Anupam Snigdha (Microsoft), 2024-06-12: Chromium switched that format's payload from UTF-16 to **UTF-8 on Windows** because "popular native apps use UTF-8 format to read/write SVG images from/to clipboard"; Word is the reproduction example; 30 native document-editing apps inventoried; shipped M127; flag `UseUtf8EncodingForSvgImage` ([PSA](https://groups.google.com/a/chromium.org/g/blink-dev/c/bsjnNMEslYU/m/pjUxG4dhAAAJ)). Office added *copy-out* of SVG to third-party apps at build 16.0.13408.20000 ([secondary: kunal-chowdhury](https://www.kunal-chowdhury.com/2020/10/svg-support-in-office-apps.html)). PowerPoint Paste Special lists **"Picture (SVG)"** alongside PNG/EMF/JPEG/GIF/DIB/Bitmap ([secondary: nutsandbolts](https://nutsandboltsspeedtraining.com/powerpoint-tutorials/paste-special-powerpoint/)). Inkscape users report modern Office pasting Inkscape's SVG target ([Inkscape forum](https://inkscape.org/forums/questions/copy-paste-into-ms-word-or-power-point-copies-just-a-png/)). | High that the name is `"image/svg+xml"` and payload is UTF-8. Microsoft has published no spec for the format; everything is inferred from Chromium (whose author is a Microsoft engineer) and observed behaviour. |
| Payload shape Office was tested against: UTF-8 bytes of a well-formed `<svg>` document, **plus one trailing NUL byte** (Chromium `CreateGlobalData` allocates `size+1` and writes `'\0'`, `clipboard_win.cc` 182-193). | Chromium source. | High for "Office accepts this exact shape". Unknown whether the NUL is *required* or merely tolerated; match Chromium byte-for-byte. |
| A web app writing `image/svg+xml` via the Async Clipboard API before M127 was rejected by Word (2023-02 report) — consistent with the UTF-16 encoding bug, not with Word lacking support. | [MS Q&A 1183438](https://learn.microsoft.com/en-us/answers/questions/1183438/how-to-paste-a-svg-image-to-word-from-web-(the-asy) (no Microsoft answer). | Medium (inference). |
| `CF_ENHMETAFILE` is **not required** for SVG paste but remains the classic vector route: Paste Special → "Picture (Enhanced Metafile)", which Office can Ungroup into shapes. Some 2024-25 builds had regressions in that Paste Special entry. | [secondary: MS Q&A threads](https://learn.microsoft.com/en-us/answers/questions/5361060/paste-special-as-metafile-in-ppt-not-working-all-o), [nutsandbolts](https://nutsandboltsspeedtraining.com/tutorials/paste-special-powerpoint). | Medium. |
| Default Ctrl+V preference order among SVG / EMF / PNG when all are present is **not documented**. Chromium's comment shows Word chooses by its own priority for raster (PNG over DIBV5) regardless of order; assume the same for vector. | Chromium `clipboard_win.cc` 1104-1116 comment. | Low — needs empirical test matrix (see §7). |
| Excel: same SVG engine as Word/PowerPoint (M365 Windows). | [MS Support, Edit SVG images in Microsoft 365](https://support.microsoft.com/en-us/office/graphics-visuals/edit-svg-images-in-microsoft-365). | High. |

### 1.2 Inkscape 1.3.x / 1.4.x on Windows (GTK3)

Authoritative: `src/ui/clipboard.cpp` at `1.4.x` (2036 lines, fetched
2026-09-03; `1.3.x` identical in every cited construct, list at 196-203).
Both include `<gtkmm/clipboard.h>` (line 22) → GTK3 `Gtk::Clipboard`, so the
Windows format mapping is GTK3's `gdk/win32/gdkselection-win32.c` (`gtk-3-24`).

Paste preference order (`_preferred_targets`, 1.4.x lines 196-207, checked
against the clipboard's target list in that order in `_getBestTarget`, 1863-1868):

```
1  image/x-inkscape-svg
2  image/svg+xml
3  image/svg+xml-compressed
4  image/x-emf
5  CF_ENHMETAFILE            <- name GTK3 gives the predefined format (gdkselection-win32.c 887)
6  WCF_ENHMETAFILE           (Wine)
7  application/pdf
8  image/x-adobe-illustrator
```

Then, Windows-only (1871-1895): if none matched, walk `EnumClipboardFormats`
and take whichever of `CF_ENHMETAFILE` / `CF_DIB` / `CF_BITMAP` was **placed
first**; then `IsClipboardFormatAvailable(CF_ENHMETAFILE)`; then any GTK
image target (`image/x-gdk-pixbuf`, 1896); then `text/plain` (1899).

How registered names reach that list: GTK3 turns every enumerated Windows
format into a GDK target atom whose name is the registered name **verbatim**
(`_gdk_win32_add_format_to_targets`, 1005-1020: `gdk_atom_intern(format_name)`),
and on the write side registers every target name verbatim with
`RegisterClipboardFormatW` (2751-2758). Predefined ids get their `CF_*`
symbol as the name (870-900). Special-cased mappings (404-422):
`image/png`↔`"PNG"`, `image/jpeg`↔`"JFIF"`, `image/gif`↔`"GIF"`,
`text/html`↔`"HTML Format"`, `image/bmp`↔`CF_DIB`/`CF_DIBV5` (1600-1602,
1646-1652). GTK4 (Inkscape 1.5+) keeps the same mapping
(`gdkclipdrop-win32.c` main 1487-1505, 2664).

Consequences for pdfcer:

| pdfcer places | Inkscape does |
|---|---|
| `"image/svg+xml"` (UTF-8 SVG bytes) | Target #2. `_retrieveClipboard` writes the bytes to a cache file and imports through the `image/svg+xml` input extension (1568-1600). Raw XML; no length prefix, no BOM needed. A trailing NUL is written into the temp file — Chromium ships one and Inkscape paste from Chromium works per the PSA, so tolerated. |
| `"image/x-inkscape-svg"` | Target #1; used for Inkscape's own round-trip (ungroup/paste-style/size semantics, 497-506, 663-870). Not needed from a foreign app; **do not** claim it. |
| `CF_ENHMETAFILE` | Target #5 (or the Windows fallback). Read with Win32 `GetClipboardData(CF_ENHMETAFILE)` + `CopyEnhMetaFile` to a file, imported via the EMF input extension (1550-1566, 1591-1593). Historic scale bugs with PowerPoint EMF (`[secondary]` Launchpad #1248354). |
| `"application/pdf"` | Target #7; imported through the PDF input extension. `[unverified]` whether the interactive PDF-import dialog appears on paste. |
| `"PNG"` / `CF_DIBV5` | Via GTK `image/png` → pixbuf paste as an embedded image (1936 comment; 463). |
| `CF_UNICODETEXT` containing SVG source | Only consulted when nothing above matched (1899), and then only parsed as SVG when the text tool is not active (466-475, 1587). Not a useful vector channel when `"image/svg+xml"` is present. |

Inkscape's own copy-out on Windows (1906-1985): offers every output-extension
MIME as a registered format (delayed rendering) plus `image/png`; **EMF is the
one format rendered eagerly** and set with `SetClipboardData(CF_ENHMETAFILE, hemf)`
(1959-1978) because GTK cannot present `image/x-emf` as `CF_ENHMETAFILE`
(1943-1947). Same mechanism pdfcer needs.

### 1.3 LibreOffice 24.x on Windows (Writer/Impress/Draw/Calc)

Authoritative: `libreoffice-24-8` branch, fetched 2026-09-03.
Windows-side translation table `vcl/win/dtrans/ftransl.cxx` (24-8: 550 lines)
maps MIME flavors ↔ (`CF_*` id | registered name). **Formats whose registered
name is not in that table are dropped on read** (`DOTransferable.cxx` 334-372:
"we ignore all formats that couldn't be translated"; name lookup
`DataFmtTransl.cxx` 113-135 → `findDataFlavorForNativeFormatName`, ftransl
397-404).

| Flavor (`sot/source/base/exchange.cxx`) | Windows binding (`ftransl.cxx` 24-8) | Read into LO as |
|---|---|---|
| `EMF` = `application/x-openoffice-emf;windows_formatname="Image EMF"` (175) | `CF_ENHMETAFILE` (116) | `GDIMetaFile` (vector) via `GraphicConverter::Import` — `transfer.cxx` 1695-1703; synthesized `GDIMETAFILE` flavor added whenever EMF/WMF/SVG is offered (1268-1276). |
| `WMF` … `"Image WMF"` (170) | `CF_METAFILEPICT` (114) | same path 1711-1719. Windows synthesizes it from `CF_ENHMETAFILE`; **do not place separately**. |
| `PNG` = `image/png` (196) | registered `"PNG"` (339-340; native name = human name "PNG") | `GetGraphic(BITMAP)` **tries PNG first** (1727-1745) → alpha preserved. |
| `BMP`/`BITMAP` | `CF_DIB` and `CF_BITMAP` (107-108). **`CF_DIBV5` deliberately disabled** (101-105, `#i124085#`: "leads to problems at export … increased png format exchange for better interoperability"). | Opaque DIB; alpha lost unless PNG present. |
| `SVG` = `image/svg+xml;windows_formatname="image/svg+xml"` (203) | **24.8: no entry** — write side works via the `windows_formatname` parameter (`getSystemDataTypeFromDataFlavor`, 24-8 ftransl ~470-480 → `RegisterClipboardFormat("image/svg+xml")`), but a *foreign* `"image/svg+xml"` format has no name→flavor mapping and is **ignored on paste**. Fixed by commit `c4aa95310d` "tdf#160267 Fix SVG and add PNG format from the clipboard" (authored 2024-04-09, **committed 2024-11-19**) adding `FormatEntry("image/svg+xml", "image/svg+xml", …)`; present in `libreoffice-25-2` (341-342), `25-8`, master; absent in `24-2`, `24-8` (verified by GitHub compare: 24-8 does not contain it). | ≥25.2: SVG → synthesized `GDIMETAFILE` (1271-1276, 417-430) i.e. vector. **24.x: nothing.** |
| `PDF` = `application/pdf` (202) | **No entry in any branch incl. master** → not exchangeable with other apps on Windows (works on X11/Wayland/macOS where the MIME is native). | Where readable: Impress/Draw `sdview3.cxx` 683-699 (`vcl::ImportPDF`, kept as native-PDF GfxLink), `transfer.cxx` 1763-1775; sot destination table `aEXCHG_DEST_DOC_GRAPHOBJ_Def` lists PDF (formats.cxx 317-325). |

Net for LO **24.x on Windows: the only vector route is `CF_ENHMETAFILE`.**
For 25.2+ `"image/svg+xml"` also works. Raster with alpha: `"PNG"`.

### 1.4 Summary matrix — vector

| Format placed | Word/PPT/Excel M365 | Inkscape 1.3/1.4 | LibreOffice 24.x | LibreOffice ≥25.2 | Chromium/Edge ≥127 web apps |
|---|---|---|---|---|---|
| `"image/svg+xml"` UTF-8 (+NUL) | **editable SVG** (high) | **yes** (#2) | no (dropped) | yes → metafile | yes |
| `CF_ENHMETAFILE` | Paste Special "Picture (Enhanced Metafile)"; default-paste priority vs SVG unknown | yes (#5) | **yes** (only route) | yes | no |
| `"application/pdf"` | no | yes (#7) | no (Windows) | no (Windows) | no |
| `CF_UNICODETEXT` = SVG text | pastes as text — harmful | last resort only | pastes as text | pastes as text | text |

---

## 2. Can Word paste PDF bytes from the clipboard directly?

**No format is known that makes Word/PowerPoint/Excel ingest PDF from the
clipboard as a picture.**

- Office's Paste Special enumerates what it recognises: Picture (SVG / PNG /
  Enhanced Metafile / JPEG / GIF), Device Independent Bitmap, Bitmap, HTML,
  RTF, Unicode/plain text, OLE objects — no PDF entry
  (`[secondary]` nutsandbolts list above; MS Q&A threads on Paste Special).
- Adobe Illustrator's clipboard carries **`PDF`** and/or **`AICB`** (Adobe
  Illustrator Clipboard) as configurable private formats, plus an optional
  "Include SVG Code" (off by default on Windows) — vendor doc
  ([helpx, Copy artwork using clipboard](https://helpx.adobe.com/illustrator/desktop/manage-objects/edit-objects/copy-artwork-using-clipboard.html), `[secondary]` search snippet; fetch timed out). Office does not read those; Illustrator→Office interop relies on AICB/EMF/PNG (`[secondary]` Adobe community threads). Inkscape lists a target `image/x-adobe-illustrator` (#8) — whether Illustrator on Windows registers that exact name is `[unverified]`.
- Acrobat's own copy formats: `[unverified]` — Adobe's "Reusing PDF content"
  page timed out twice; a PDF-XChange forum thread states Acrobat Reader
  offers more Paste Special choices than Bitmap/DIB without naming them
  (`[secondary]`, [forum](https://forum.pdf-xchange.com/viewtopic.php?t=31328)).
- Word embeds PDF only as an **OLE object** ("Adobe Acrobat Document",
  Insert → Object) — not a clipboard paste of PDF bytes.
- LibreOffice reads `application/pdf` — but **not on Windows** (§1.3).
- Inkscape reads `"application/pdf"` (§1.2) — the one Windows target for which a
  PDF clipboard format is useful.

---

## 3. Chrome/Edge behaviour (what "copy image" of an SVG puts on the clipboard, and what Word does)

- Chromium's **native** clipboard write for SVG (`ClipboardWin::WriteSvg`,
  `clipboard_win.cc` 1066-1075): registered `"image/svg+xml"`, UTF-8 since
  M127 (UTF-16 before), NUL-terminated. Read side (`ReadSvg`, 779) reads the
  same name. `[unverified]` whether the context-menu "Copy image" on an
  `<img src=x.svg>` uses `WriteSvg` (it historically wrote a rasterised bitmap
  + HTML; the SVG path was built for the Async Clipboard API
  `ClipboardItem({'image/svg+xml': blob})`, [Chrome blog](https://developer.chrome.com/blog/svg-support-for-async-clipboard-api), [Edge blog: Edge 124](https://blogs.windows.com/msedgedev/2024/07/11/seamless-svg-copy-paste-on-the-web/)). Pasted SVG is sanitised (event-handler attributes stripped) on the *browser* read side only.
- Chromium's bitmap write (`WriteBitmap`, 1104-1130): `"PNG"` first, then
  `CF_DIBV5`; in-source rationale: *"Word support for DIBV5 is buggy and PNG
  format is needed for it. Writing order is also important as some programs
  will use the first compatible format … we want Word to choose the PNG
  format."*
- Word's reaction to the Chromium SVG payload: accepted as an SVG picture once
  the encoding was UTF-8 (PSA problem statement was exactly "pasting into Word
  fails to render"). This is the strongest evidence that `"image/svg+xml"` is
  a Word-accepted format.

---

## 4. Rust crates

### 4.1 Clipboard crates (crates.io metadata fetched 2026-09-03)

| Crate | Version / last publish | Licence (SPDX) | Class | (a) DIB/DIBV5/PNG+alpha | (b) arbitrary registered format | (c) `CF_ENHMETAFILE` | Notes |
|---|---|---|---|---|---|---|---|
| `arboard` (1Password) | 3.6.1 / 2025-08-23; 43.8 M dl | `MIT OR Apache-2.0` | permissive | **Write** `Set::image`: `"PNG"` then `CF_DIBV5` (BITMAPV5HEADER), PNG first "for compatibility" (`platform/windows.rs` 713-727, 132-165, 53-130). **Read**: `"PNG"` then `CF_DIBV5` (625-645). | **None.** Public surface = text, html (`"HTML Format"`), image, file_list, plus the three cloud-exclusion formats (809-840). | none | Already in `pdfcer-gui`'s lock (3.6.1, pulled by egui-winit). Depends on `clipboard-win 5.3.1+` and `windows-sys`. Feature `image-data` (default) pulls `image` 0.25 (png,bmp). |
| `clipboard-win` (DoumanAsh) | 5.4.1 / 2025-07-17; 56 M dl | **`BSL-1.0`** (Boost Software License 1.0) | permissive (OSI-approved; no attribution requirement on binaries) — **not in `LEGAL.md` §6.1's enumerated list but already accepted**: `pdfcer/about.toml` line 35 and `pdfcer-gui/about.toml` line 85 list `BSL-1.0`; `pdfcer-gui/THIRD_PARTY_LICENSES.md` already attributes `clipboard-win 5.4.1` (lines 4596-4599). | Constants `CF_DIB`=8, `CF_DIBV5`=17, `CF_BITMAP`=2 (`formats.rs` 41-46). `formats::Bitmap` writes `CF_BITMAP` from BMP-file bytes (205-227, `raw::set_bitmap`). DIBV5/PNG: build the bytes yourself and use `raw::set_without_clear(CF_DIBV5, &[u8])` / `RawData(png_id)` — both are `HGLOBAL` formats so the generic path is correct. | **Yes**: `register_format(&str) -> Option<NonZeroU32>` (`raw.rs` 1135-1163), `formats::RawData(id)` implements `Setter<&[u8]>`/`Getter<Vec<u8>>` (110-135), `raw::set` / `set_without_clear` copy into a `GMEM_MOVEABLE` `HGLOBAL` and call `SetClipboardData` (497-520). `Clipboard::new_attempts(n)` RAII open; `raw::empty()`. | **No setter/getter.** `CF_ENHMETAFILE`=14 exists only as a constant/name (`formats.rs` 64; `raw.rs` 1045-1091 `format_name`). `raw::set` would hand an `HGLOBAL` where an `HENHMETAFILE` is required — wrong handle type. Must call `SetEnhMetaFileBits` + `SetClipboardData(CF_ENHMETAFILE, hemf)` directly (windows-sys) while the clipboard-win guard holds the clipboard open. | Features: `std`, `monitor`. `no_std`-capable core. Windows-only crate (`cfg(windows)` deps: `error-code`, optional `windows-win`). |
| `clipboard-rs` (ChurchTao) | 0.3.5 / 2026-06-30; 392 k dl | `MIT` | permissive | `set_image`: `"PNG"` then `CF_BITMAP` via `CreateDIBitmap` (`platform/win.rs` 379-403, 711) — alpha only through PNG. Read: `"PNG"` → `CF_DIBV5` → `CF_DIB` (231-253). | **Yes**: `set_buffer(format_name, Vec<u8>)` → `register_format` + `RawData` (346-353); `ContentFormat::Other(name)`. | none | Wraps `clipboard-win 5.4.1` (feature `monitor`) + `windows 0.59` — would **duplicate** `windows` (pdfcer-gui has 0.62.2). No capability beyond clipboard-win for this task. |
| `window-clipboard` (hecrj / iced) | 0.5.1 / 2025-12-12; 3.1 M dl | `MIT` | permissive | none — `read()/write(String)` only (`lib.rs` 66-80) | none | none | README: "Very experimental". Text only. Not applicable. |

### 4.2 EMF (Enhanced Metafile) writers in Rust

crates.io search (`q=emf`, `metafile`, `wmf`, `enhanced metafile`, 2026-09-03):
**no crate named `emf` or `metafile` exists** (API 404 for both).

| Crate | Version / date | Licence | Class | Writes EMF? | Notes |
|---|---|---|---|---|---|
| `emfsdk` (KaiserY) | 0.2.0 / 2026-07-20; 4.3 k dl | `MIT OR Apache-2.0` | permissive | **Yes** — "typed, byte-preserving EMF, EMF+, and WMF parsing and writing"; `Writer`/`SdkWrite`, `metafile.write_to(&mut impl Write)`; **record-level structs only, no drawing/builder API**; optional `render` feature (fontique/image/skrifa/zeno) "targets portable previews, not pixel-identical GDI output". MSRV 1.88; pre-1.0, API may change. Deps: bitflags, emfsdk-derive, encoding_rs, thiserror. ([docs.rs](https://docs.rs/emfsdk), [repo](https://github.com/KaiserY/emfsdk)) | Youngest viable option; 2 months old. Useful at minimum as a **parser oracle in tests** for a hand-written emitter. |
| `emf-core` (mythrnr) | 0.1.0 / 2026-08-03 | `MIT` | permissive | No — parser + EMF→SVG converter per MS-EMF. | Read-only. |
| `wmf-core` (mythrnr) | 0.1.1 / 2026-08-03 | `MIT` | permissive | No — parser + WMF→SVG. | Read-only. |
| `file-format` | 0.29.0 | MIT/Apache | permissive | No — detection only. | — |
| `metrique-writer-format-emf` etc. | — | — | — | Unrelated ("EMF" = CloudWatch Embedded Metric Format). | Name collision; ignore. |

Non-crate routes:

- **Win32 GDI recording** via `windows-sys` (`MIT OR Apache-2.0`, already in both
  lockfiles; needs `Win32_Graphics_Gdi`): `CreateEnhMetaFileW(NULL, NULL, &frame_rect_0_01mm, desc)` → GDI calls (`BeginPath`/`MoveToEx`/`PolyBezierTo`/`LineTo`/`CloseFigure`/`EndPath`/`FillPath`/`StrokeAndFillPath`, `SetWorldTransform`, `ExtCreatePen`, `CreateBrushIndirect`, `StretchDIBits`, `ExtTextOutW`) → `CloseEnhMetaFile` returns the `HENHMETAFILE` that `SetClipboardData(CF_ENHMETAFILE, …)` takes directly. Windows-only, untestable on CI's wasm/Linux lanes, lives only in the GUI/CLI shell. Guarantees well-formed records.
- **Hand-emitted `[MS-EMF]`** (Microsoft Open Specification Promise; spec public): the record set a PDF-path→EMF flattening needs is small (`EMR_HEADER`, `SETMAPMODE`, `SETWINDOWEXTEX/ORGEX`, `SETVIEWPORTEXTEX/ORGEX`, `SAVEDC/RESTOREDC`, `SETWORLDTRANSFORM/MODIFYWORLDTRANSFORM`, `EXTCREATEPEN`, `CREATEBRUSHINDIRECT`, `SELECTOBJECT`, `DELETEOBJECT`, `SETPOLYFILLMODE`, `BEGINPATH/ENDPATH`, `MOVETOEX`, `LINETO`, `POLYBEZIERTO`, `CLOSEFIGURE`, `FILLPATH/STROKEPATH/STROKEANDFILLPATH`, `SELECTCLIPPATH`, `STRETCHDIBITS`, `EXTCREATEFONTINDIRECTW`+`EXTTEXTOUTW` (optional), `EOF`). Pure Rust, cross-platform, unit-testable, same shape as pdfcer's SVG writer. Then `SetEnhMetaFileBits(len, ptr)` → `HENHMETAFILE`.
- **EMF limitations** either way: classic GDI records carry **no alpha** (no soft masks, no group transparency, no blend modes; `AlphaBlend` records exist but Office/LO support is spotty); gradients only via `GRADIENTFILL` or flattening; text either as glyph outlines (exact, not editable) or `EXTTEXTOUTW` (editable after Ungroup in Office, but requires the font on the target machine). EMF+ (GDI+ records, `EMR_COMMENT_EMFPLUS`) adds alpha/anti-aliasing and Office reads dual EMF/EMF+, but doubles the writer surface. Recommend classic EMF only for v1; disclose flattening (rule 4: report what was rasterised/flattened, off-canvas).

### 4.3 SVG writers

| Option | Licence | Class | Notes |
|---|---|---|---|
| Hand-written (pdfcer's planned SVG exporter) | — | — | No dependency; pdfcer already serialises binary/text formats. Preferred. |
| `svg` (bodoni) | 0.18.0 / 2024-09-27; 7.4 M dl | `Apache-2.0 OR MIT` | permissive | Composer + parser; node/attribute builder. Adds little over `write!`. |
| `svgwriter` (msrd0) | 0.1.1 / 2026-01-05 | **non-standard** (crates.io `license` field not SPDX) | **reject** per `LEGAL.md` §6.2 (unclassifiable). | — |

---

## 5. PNG-with-alpha convention on the Windows clipboard

De-facto rule: **registered `"PNG"` (raw PNG file bytes in an `HGLOBAL`) first, then `CF_DIBV5`**, and readers prefer `"PNG"`.

| Party | Writes | Reads | Source |
|---|---|---|---|
| Microsoft Office (Word/PowerPoint/Excel) | `"PNG"` (Paint.NET: "images coming from Microsoft Office apps" arrive as PNG) | `"PNG"` preferred; `CF_DIBV5` "buggy" | Chromium `clipboard_win.cc` 1104-1116; Mozilla bug 1717306 ("Microsoft Office … prefer PNG Clipboard Format to CF_BITMAP"); Paint.NET 4.2 notes ([blog](https://blog.paint.net/2019/07/13/paint-net-4-2-is-now-available/)) `[secondary]` |
| Chromium / Edge | `"PNG"` then `CF_DIBV5` | `"PNG"` then `CF_DIB` (1193-1241) | Chromium source |
| Firefox | `"PNG"` (bug 1832396) | `"PNG"` (bug 1940790) | [Mozilla bug 1717306](https://bugzilla.mozilla.org/show_bug.cgi?id=1717306) |
| Paint.NET ≥4.2 | `CF_DIBV5` (32-bit BGRA) | `"PNG"` highest priority, then `CF_DIBV5` if alpha present, else DIB with alpha heuristics | Paint.NET 4.2 release notes `[secondary]` |
| GIMP 2.10 (GTK2) / GIMP 3 (GTK3) | `image/png` → `"PNG"` | `"PNG"` → `image/png`; `CF_DIB`/`CF_DIBV5` → `image/bmp` | GTK2 `gdkselection-win32.c` 241-250, 1254; GTK3 404-408, 1600-1602 |
| Inkscape 1.3/1.4 | `image/png` → `"PNG"` (1936) | same GTK3 path | Inkscape `clipboard.cpp`; GTK3 |
| LibreOffice 24.x | `"PNG"` + `CF_DIB` (`CF_DIBV5` disabled) | `"PNG"` first, then `CF_DIB` (opaque) | ftransl 101-108, 339-340; transfer.cxx 1727-1745 |
| Snip & Sketch, Paint 3D | `"PNG"` | — | Mozilla bug 1717306 reporter |
| arboard | `"PNG"` then `CF_DIBV5` | `"PNG"` then `CF_DIBV5` | §4.1 |

`CF_DIBV5` caveats: alpha interpretation (premultiplied vs straight) is
unspecified in practice — Mozilla's conclusion was "use PNG if available,
otherwise CF_DIBV5 treated as premultiplied" (`[secondary]`); Paint.NET
applies heuristics; Chromium writes from a premultiplied N32 bitmap
(`CreateDIBV5ImageDataFromN32SkBitmap`). Use `BI_BITFIELDS` with explicit
B/G/R/A masks and 32 bpp. PNG carries straight alpha unambiguously, which is
why it must come first.

---

## 6. Licence classification summary (per `LEGAL.md` §6.1)

| Dependency | SPDX | Class | Status in pdfcer graph |
|---|---|---|---|
| `clipboard-win` 5.4.1 | `BSL-1.0` | permissive | already present (pdfcer-gui, via arboard); `BSL-1.0` already accepted in both `about.toml`s |
| `windows-sys` (`Win32_System_DataExchange`, `Win32_Graphics_Gdi`) | `MIT OR Apache-2.0` | permissive | already present (both repos) |
| `arboard` 3.6.1 | `MIT OR Apache-2.0` | permissive | already present (pdfcer-gui) — insufficient for this task, keep for text |
| `emfsdk` 0.2.0 | `MIT OR Apache-2.0` | permissive | not present; optional (test oracle or record writer) |
| `svg` 0.18.0 | `Apache-2.0 OR MIT` | permissive | not present; not needed |
| `clipboard-rs` 0.3.5 | `MIT` | permissive | not present; not needed (duplicates `windows`) |
| `window-clipboard` 0.5.1 | `MIT` | permissive | not present; text-only |
| `svgwriter` 0.1.1 | non-standard | unclassifiable | reject |
| `emf-core` / `wmf-core` | `MIT` | permissive | read-only; not needed |

No copyleft anywhere in the candidate set. Inkscape (GPL-2.0-or-later) and
LibreOffice (MPL-2.0) were read as **behavioural references only** (R61
pattern); nothing is ported.

---

## 7. Recommendation

**Minimal format set, in placement order** (one `OpenClipboard` / `EmptyClipboard` / N × `SetClipboardData` / `CloseClipboard` transaction; order matters because several readers take the first format they recognise):

1. **`"image/svg+xml"`** (registered) — UTF-8 bytes of a standalone `<svg>` with explicit `width`/`height` (physical units) and `viewBox`, **followed by one NUL byte**, in an `HGLOBAL`. This is byte-for-byte what Chromium ≥ M127 writes and what Microsoft's engineer validated against 30 native Windows apps. Reaches: **Word / PowerPoint / Excel (M365) as an editable SVG graphic**, **Inkscape 1.3/1.4** (preferred target #2), LibreOffice ≥ 25.2, Chromium/Edge web apps.
2. **`CF_ENHMETAFILE`** — `HENHMETAFILE` from `SetEnhMetaFileBits` over pdfcer's own EMF bytes (or from GDI recording). Reaches: **LibreOffice 24.x (its only vector route on Windows)**, Office Paste Special → "Picture (Enhanced Metafile)" (ungroupable shapes), Inkscape fallback (#5), every legacy Win32 vector consumer (Visio, CorelDRAW, CAD packages). Windows synthesises `CF_METAFILEPICT` from it — do not place WMF.
3. **`"PNG"`** (registered) — RGBA PNG file bytes, straight alpha, at a disclosed DPI. Reaches: Office (its preferred raster), Paint.NET, GIMP, Inkscape, LibreOffice (tries PNG first), Firefox/Chromium, Snip & Sketch — i.e. "everything else, with alpha".
4. **`CF_DIBV5`** — `BITMAPV5HEADER`, 32 bpp, `BI_BITFIELDS`, explicit BGRA masks, premultiplied as Chromium does. Reaches readers that predate the `"PNG"` convention; Windows synthesises `CF_DIB` and `CF_BITMAP` from it, so those are never placed explicitly.
5. *(optional, cheap — pdfcer already has the bytes)* **`"application/pdf"`** (registered) — the standalone one-page PDF. Only Inkscape consumes it on Windows (target #7, after SVG); Office never; LibreOffice never on Windows. Place last; drop it if the interactive PDF-import dialog in Inkscape proves annoying.
- **Do not place** `CF_UNICODETEXT`/`CF_TEXT` containing SVG source: text-first readers (and possibly Office) would paste XML as text. Do not place `"image/x-inkscape-svg"` (Inkscape-internal semantics). Optionally place `"CanUploadToCloudClipboard"` = DWORD 0 for multi-MB payloads.

**Crates:**
- **`clipboard-win` 5.4.1** — `BSL-1.0`, **permissive**, already in `pdfcer-gui`'s dependency graph via arboard and already accepted in both `about.toml`s. Use `Clipboard::new_attempts` (RAII open) + `raw::empty()` + `register_format("image/svg+xml" | "PNG" | "application/pdf")` + `raw::set_without_clear(id, &bytes)` for formats 1, 3, 4, 5 (`CF_DIBV5` = `formats::CF_DIBV5` const). Placement order is the call order.
- **`windows-sys`** (`Win32_Graphics_Gdi`, `Win32_System_DataExchange`) — `MIT OR Apache-2.0`, **permissive**, already present. Needed only for format 2: `SetEnhMetaFileBits` + `SetClipboardData(CF_ENHMETAFILE, hemf)` — clipboard-win has no metafile setter (its generic path would pass an `HGLOBAL` where an `HENHMETAFILE` is required). Roughly 15 lines of unsafe in the shell crate.
- **`arboard`** — keep for plain-text clipboard (egui already uses it); it exposes no registered-format API, so it cannot carry formats 1, 2 or 5. **`clipboard-rs`** and **`window-clipboard`** add nothing over the above (the former would duplicate the `windows` crate at 0.59 vs 0.62; the latter is text-only).
- **EMF bytes:** hand-emit `[MS-EMF]` records in a pure-Rust module (same shape as the SVG writer; cross-platform, CI-testable, keeps `pdfcer-core`/`pdfcer-render` free of Win32 per rule 2), and add **`emfsdk` 0.2.0** (`MIT OR Apache-2.0`, permissive, pre-1.0) as a dev-dependency parser oracle to round-trip-check the emitted records; promote it to a runtime record writer only if hand emission proves error-prone. Fall back to GDI recording only as a debugging comparison. Classic EMF only (no EMF+) for v1; alpha/soft-mask/blend content is flattened and **reported off-canvas** per rule 4.
- **SVG bytes:** pdfcer's own writer; no crate.

**Where the code lives:** the clipboard transaction and EMF handle creation are OS-shell concerns → `pdfcer-gui` (and a `pdfcer copy` CLI subcommand if wanted, rule 11); the SVG and EMF *serialisers* are pure Rust → `pdfcer-core`, wasm-clean.

**Empirical test matrix still owed** (nothing above measures Office's default Ctrl+V priority among SVG/EMF/PNG when all four are present, nor Inkscape's PDF-import dialog): Word/PowerPoint/Excel M365 default paste + Paste Special listing; Inkscape 1.3 and 1.4 default paste and Paste Special; LibreOffice 24.8 Writer/Impress/Calc default paste (expect EMF) and 25.2 (expect SVG); GIMP 2.10 and 3.x; Paint.NET; Paint; Firefox/Chrome `<input type=file>`-less paste. Record results back into this file.

---

## 8. Source index

- Inkscape `src/ui/clipboard.cpp` — https://gitlab.com/inkscape/inkscape/-/raw/1.4.x/src/ui/clipboard.cpp (and `1.3.x`)
- GTK3 `gdk/win32/gdkselection-win32.c` — https://gitlab.gnome.org/GNOME/gtk/-/raw/gtk-3-24/gdk/win32/gdkselection-win32.c ; GTK4 `gdkclipdrop-win32.c` (main); GTK2 `gtk-2-24`
- LibreOffice `libreoffice-24-8`: `sot/source/base/exchange.cxx`, `sot/source/base/formats.cxx`, `vcl/win/dtrans/ftransl.cxx`, `vcl/win/dtrans/DataFmtTransl.cxx`, `vcl/win/dtrans/DOTransferable.cxx`, `vcl/source/treelist/transfer.cxx`, `sd/source/ui/view/sdview3.cxx`, `sw/source/uibase/dochdl/swdtflvr.cxx` — https://raw.githubusercontent.com/LibreOffice/core/libreoffice-24-8/… ; commit `c4aa95310d` (tdf#160267) — https://github.com/LibreOffice/core/commit/c4aa95310d
- Chromium `ui/base/clipboard/{clipboard_win.cc, clipboard_format_type_win.cc, clipboard_constants.h}` — https://chromium.googlesource.com/chromium/src/+/main/ui/base/clipboard/
- Blink-dev PSA (UTF-8 SVG on Windows, M127) — https://groups.google.com/a/chromium.org/g/blink-dev/c/bsjnNMEslYU/m/pjUxG4dhAAAJ ; Chrome blog — https://developer.chrome.com/blog/svg-support-for-async-clipboard-api ; Edge blog — https://blogs.windows.com/msedgedev/2024/07/11/seamless-svg-copy-paste-on-the-web/
- MS Learn Clipboard Formats — https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats ; MS Support Edit SVG in M365 — https://support.microsoft.com/en-us/office/graphics-visuals/edit-svg-images-in-microsoft-365 ; MS Q&A 1183438 — https://learn.microsoft.com/en-us/answers/questions/1183438/
- Mozilla bug 1717306 — https://bugzilla.mozilla.org/show_bug.cgi?id=1717306 ; Paint.NET 4.2 — https://blog.paint.net/2019/07/13/paint-net-4-2-is-now-available/
- Inkscape forum (Office reads Inkscape SVG target) — https://inkscape.org/forums/questions/copy-paste-into-ms-word-or-power-point-copies-just-a-png/ ; Inkscape issue #1467 (Linux-reported EMF regression) — https://gitlab.com/inkscape/inkscape/-/issues/1467
- Adobe Illustrator clipboard (PDF/AICB/SVG code) — https://helpx.adobe.com/illustrator/desktop/manage-objects/edit-objects/copy-artwork-using-clipboard.html ; PDF-XChange forum — https://forum.pdf-xchange.com/viewtopic.php?t=31328
- Crates: https://crates.io/crates/{arboard, clipboard-win, clipboard-rs, window-clipboard, emfsdk, emf-core, wmf-core, svg, svgwriter} ; sources https://github.com/1Password/arboard, https://github.com/DoumanAsh/clipboard-win, https://github.com/ChurchTao/clipboard-rs, https://github.com/hecrj/window_clipboard, https://github.com/KaiserY/emfsdk
