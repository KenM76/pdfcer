# Overprint simulation — architecture survey

**Written 2026-08-18** by the engineer, from a commissioned online research
pass. Same role as `docs/ocr-engine-survey.md`: a sourcing record for a
decision that has to be made once and lived with.

**Why this file exists.** The finding below is the single most expensive
thing learned this session, it was learned twice (once by measurement, once
by research), and it existed only in a conversation transcript until it was
written here. `ARCHITECTURE.md` decision 069 records what pdfcer *did*; this
records *what the field does and why the cheap route cannot work*, which is
the part a future session will otherwise re-derive.

---

## §1 — The one-line answer

**Build an n-channel compositing buffer: one plane per colorant (CMYK +
one per spot), keep the tint transform OUT of the paint path for any
colorant that owns a plane, apply Table 149 in colorant space, and collapse
to RGB exactly once, at the end.**

Seven independent implementations converge on this identical architecture.
No published alternative exists. No published approximation covers spot
colorants.

---

## §2 — The spec mandates the shape

ISO 32000-1 §11.7.4.1 NOTE 2 (already in the corpus at
`D:\Dev\Rag-Specialized\PDF_Spec\iso32000\iso32000__s__11.7.md`):

> "it is best to think of transparency as taking place in appearance space,
> but **overprinting of device colorants in device space**. This means that
> colorant overprint decisions **should be made at output time**, based on
> the actual resultant colorants of any transparency compositing operation."

Adobe, *PDF Blend Modes: Addendum* (2006-01-23),
<https://printtechnologies.org/standards/files/pdf-reference-1.6-addendum-blend-modes.pdf>:
**"Only separable blend modes can be used for blending spot colors."** Spot
colorants are *components of the compositing model*, not colours resolved
before it.

---

## §3 — What each engine does

| Engine | Architecture, in its own documentation's terms | Source |
|---|---|---|
| **MuPDF** | Draws internally to a **"CMYK + Spots pixmap"**, then *"convert[s] down… to the colorspace of the initial pixmap"* at the end. Per-separation modes: `COMPOSITE` (equivalent process mix — *"overprint is ignored, and a dedicated spot plane is not produced"*), `SPOT` (own plane), `DISABLED`. `-M 1` = *"simulate overprint (i.e. render to separations internally, and convert down to the target colorspace at the end)"* | <https://artifex.com/blog/rendering-separations-with-mupdf>, <https://ghostscript.com/~robin/mupdf_explored/mupdf_exploredse36.html>, <https://mupdf.readthedocs.io/en/latest/tools/mutool-draw.html> |
| **Ghostscript** | *"The pdf14 device buffer collects the data in a CMYK or CMYK+spots buffer and the `put_image` method… will map the buffer to the target device color space."* Overprint simulation for all devices since 9.54.0 (2021-03-30) | <https://ghostscript.com/docs/9.54.0/History9.htm>, <https://ghostscript.com/docs/9.54.0/News.htm> |
| **PDF Tools AG** | *"renders the content of a page on a **bitmap surface with separate color channels. Each of these color channels describe the color intensity of a colorant**"*, then *"transforms each pixel of the target surface to a pixel of the preview surface (RGB)"* — *"applied as a last step"* | <https://www.pdf-tools.com/pdf-knowledge/how-preview-overprinting/> |
| **Global Graphics Mako** | `renderSeparationsToFrameBuffers(...)`; recombination scanline-by-scanline, each process value multiplied by each spot's CMYK component. **"Colorspace must be CMYK for spot merging."** | <http://documentation.hybridhelix.com/mako/overprint-simulation> |
| **Harlequin** | Ships the cheap `Darken`-like approximation as `OverprintPreview false` — **with spots potentially ignored**; `/SpotsOnly` is a separate, more expensive mode *"Use when spot color overprints must be emulated"* | <http://documentation.hybridhelix.com/hqnc/understanding-overprintpreview> |
| **Acrobat** | Output Preview → *Simulate Overprinting* + Simulation Profile; Separations panel lists every ink. **Method not publicly documented** | <https://helpx.adobe.com/acrobat/using/previewing-output-acrobat-pro.html> |
| **Scribus** | Not an independent datapoint — shells out to Ghostscript | — |

**Spot discovery and plane allocation.** Ghostscript's docs state the key
enabling property, and it is a property of *PDF specifically*:

> "When rendering a PDF document, Ghostscript can determine **prior to
> rendering** how many colorants occur on a particular page. With
> Postscript, this is not possible in general."
> — <https://ghostscript.readthedocs.io/en/latest/Devices.html>

So: **pre-scan the page's resources and allocate exactly the planes needed.**
Ghostscript's ceiling is 64 process+spot (`GS_CLIENT_COLOR_MAX_COMPONENTS`),
default soft cap 10 spots; **beyond the cap, colorants fall back to the tint
transform** — i.e. pdfcer's current behaviour is what a correct engine does
*only when it has run out of planes*. Acrobat 9's Output Preview shows only
the first 27 inks.

---

## §4 — Why the cheap route cannot work (this is the load-bearing section)

**Michael Vrhel (Artifex), *"Color processing and management in
Ghostscript"*, IS&T Color and Imaging Conference 27** — peer-reviewed, and
the clearest statement of pdfcer's exact problem generalised:

> "A developer might be tempted to **pack the color transforms into one
> operation eliminating the rendering to the intermediate** Fogra 39 color
> space. **Unfortunately, in general, that approach is not possible due to
> the fact that PDF includes a drawing state called overprint** in which
> subsequent drawing of colorants might not erase colorants that have
> already been drawn in that location. This drawing state means that **the
> previous colors in the Fogra 39 color space must be maintained through the
> entire drawing of the page** to ensure the intended rendering is
> performed."

<https://library.imaging.org/admin/apis/public/api/ist/website/downloadArticle/cic/27/1/art00012>

Three more, saying the same thing:

- **Ghostscript:** *"Overprinting with spot colors is not allowed if the
  tint transform function is being used to convert spot colors."* And: the
  composite CMYK output *"because it uses the tint transformed colour
  equivalents for any spot colours, may not produce an accurate preview, if
  the job uses overprinting."* The docs insist this is *"part of the
  PostScript and PDF specifications. They are not due to a limitation in the
  implementation."*
- **MuPDF:** compositing a separation via its equivalent mix means
  *"overprint is ignored."*
- **Poppler** (mailing list): *"In the splash implementation, it fails if
  CMYK values should overprint spot colors, **because spot colors are
  converted immediately in their CMYK alternates**."* — **pdfcer's bug,
  verbatim, in another renderer.**

### The sanctioned approximation, and its ceiling

ISO 32000-1 §11.7.4.2 permits `Darken` (`B(cb,cs) = min(cb,cs)`) for
*"effects similar to overprinting"*, device-independently. That is what
Harlequin's cheap mode uses. **It cannot cover spots** — *"an orange ink
overprinting on top of the same orange ink simply cannot result in a darker
orange ink"* (<https://creativepro.com/indesigns-onscreen-untruths-overprinting-or-multiplying-spot-colors/>).

### Two datapoints on how hard the industry judges this

- **PDF-XChange has never implemented it.** Their developer, Nov 2024:
  *"Overprint preview is still not available in our products… It is not a
  small or trivial thing and it opens a whole can of worms."*
  <https://forum.pdf-xchange.com/viewtopic.php?t=28893>
- **pdf.js closed it WONTFIX** (mozilla/pdf.js#7360, verified closed):
  *"To properly make it work output medium must support CMYK natively and
  **web APIs do not**."* — **★ directly relevant to pdfcer's web-fork goal:
  overprint is the first feature that does not cross.**

### pdfcer's own prior art agrees

`PDF_Spec\iso32000\iso32000__ref__spot_colour_overprint.md` §H (written
2026-08-08) already listed stage 10 as *"n-channel compositing buffer +
CompatibleOverprint (Table 149) → overprint simulation — **large —
architectural**… Stage 10 is a different project."* The 2026-08-18 ablation
(decision 069) then proved the shortcut empirically. **Three independent
routes, one answer.**

**Novelty note:** no published source describes anyone trying a
spot-multiplier plate over a flattened RGB buffer — in either direction.
pdfcer's negative result (`ac15158`,
`C:\personal_rag\pdf\lesson_20260818_spot_ink_multiplier_plate_does_not_pay_negative_result.md`)
appears to be genuinely novel.

---

## §5 — Three places pdfcer can EXCEED the references

Per Ken's standing "parity is a floor" directive, recorded so they are not
lost:

1. **Ghostscript bug 709099 — CONFIRMED and unfixed.** The suite's DeviceN
   spaces carry `/None` colorants. Ghostscript computes a DeviceN's
   equivalent CMYK at *instantiation*, but `/None` values are not known
   until *use*, so *"overprint simulation will not work for files
   constructed that way"*; it emits `Separation preview may be inaccurate
   due to presence of /None colorants`. **Resolving spot equivalents lazily
   at use rather than at instantiation beats Ghostscript on a patch Artifex
   has given up on.**
2. **Ghostscript bug 696023** — fails PCS040 a/b/c and PCS030, regressed on
   the GS 10 codebase (Apr 2024). Artifex's own ground-truth file is named
   `Acrobat9_with_overprint.png`, i.e. **Acrobat's Simulate Overprint is the
   industry's de-facto oracle for these two patches.**
3. **Poppler #1565 (open)** — enabling overprint preview routes the whole
   page through CMYK and visibly shifts unrelated RGB raster content. A
   *scoped* rather than page-wide n-channel buffer avoids this.

*(Items 1–3 are single-source via the research agent; `bugs.ghostscript.com`
and `gitlab.freedesktop.org` bot-wall automated fetches. Re-verify before
citing externally.)*

---

## §6 — The unstandardised part: the final collapse

**The CMYK+spots → RGB collapse is not specified, and vendors disagree
materially.** Pick one deliberately and disclose it (rule 4); there is no
right answer to inherit.

| Model | Method |
|---|---|
| Ghostscript | ICC (NCLR profile if supplied, else default CMYK); on `tiffsep` the spot planes get **no colour management at all** |
| Mako | **Multiplicative** — each process value × each spot's corresponding CMYK component |
| EFI Fiery | Three switchable: **Standard** = per-channel addition, documented to suffer *"clamping errors"*; **Vivid** = computed in L\*a\*b\*/XYZ, *"avoids the typical clamping errors"*; **Natural** = to RGB then multiply (<https://help.fiery.com/cws_cs/6.7/en-us/GUID-7834E944-6C2E-41E4-BA38-4CF4E5CFB7F1.html>) |
| Acrobat | **Not publicly documented.** PDF Tools AG: overprint preview *"is not specified and therefore implemented differently or not at all"* and *"the method Acrobat uses is not publicly documented"* — they matched it empirically |

This is a **settings-shaped ambiguity** under Ken's standing "make spec
ambiguity a setting" rule.

---

## §7 — Licensing hygiene (binding on any follow-up)

Ghostscript, MuPDF, Poppler and Scribus are **AGPL/GPL**; pdfcer is **MIT**
and **cannot link them** (`LEGAL.md` §6.1). Everything in this file is
**documentation, vendor product docs, changelog prose, a peer-reviewed
paper, spec text, or tracker/mailing-list prose**. The research pass
deliberately did **not** open source files, and named the ones it declined
(`base/gsovrc.h`, `include/mupdf/fitz/separation.h`, `source/fitz/colorspace.c`,
and all `git.ghostscript.com` commitdiff pages).

**Keep it that way.** These remain *behavioural* references only.
