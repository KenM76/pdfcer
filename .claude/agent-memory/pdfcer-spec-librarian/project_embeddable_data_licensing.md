---
name: pdf-spec-embeddable-data-licensing
description: Font/spec-data license patterns this corpus keeps hitting — data-vs-document, WIDTHS-vs-SHAPES, the document-embedding-only trap in font exception clauses, and which PDF_Spec material MAY legally cross into the pdfce repo as generated tables.
metadata:
  type: project
---

**1. Some `PDF_Spec` source material is legitimately embeddable in pdfce source.**
The corpus's default posture ("`_sources\` never crosses into `D:\Dev\pdfce\`")
is about *spec documents*. It does **not** apply to the redistributable *data
files* those specs depend on:

- Core 14 AFM metrics (**WIDTHS**) → **APAFML** (SPDX `APAFML`; Fedora: Free +
  GPL-compatible)
- Adobe Glyph List / AGLFN / zapfdingbats list → **BSD-3-Clause** (**not**
  Apache-2.0, despite the widespread assumption — verified from `LICENSE.md`
  and the `glyphlist.txt` header)
- pdfium's Foxit base-14 faces (**SHAPES**, `core/fxge/fontdata/chromefontdata/`)
  → **BSD-3-Clause**, but **chain of title runs through pdfium only** — Foxit
  never published these standalone. Weaker provenance than a first-party grant;
  flag to the user before public release rather than treating it as settled.

Both are compatible with every candidate pdfce license, so neither reopens
`LEGAL.md` §1. **Why:** ISO 32000-1 mandates standard-14 support and AGL-based
glyph resolution but contains neither the widths nor the mapping — they are
unavoidable external data dependencies, and the engineer needs a licensing
answer before choosing an implementation strategy.

**How to apply:** the raw `.afm`/`.txt` files still stay in `_sources\`; what
crosses into pdfce is a **generated table** with the upstream copyright notice
in its header, plus a **manual** `THIRD_PARTY_LICENSES.md` entry (`cargo-about`
can't see non-Cargo dependencies — the one documented exception to `LEGAL.md`
§6.3's "generated, never hand-maintained"). Verdicts live in the RAG's
`index.md` § "Licensing verdicts" and in `fonts\font__std14_afm_licensing.md` /
`fonts\font__agl.md`.

**2. The data-vs-document license split — check for it every time.** Adobe
repeatedly publishes permissive *data* alongside an all-rights-reserved
*document describing the data*:

| Data (permissive) | Document (restricted) |
|---|---|
| Core 14 AFMs — APAFML | TN #5004 AFM format spec — "No part of this publication may be reproduced" |
| AGL data files — BSD-3-Clause | AGL Specification — verbatim-or-derivative-but-don't-impersonate |

**How to apply:** never infer the document's license from the data's (or vice
versa). A file may be `license_basis: free_primary` overall for its data while
its *algorithm/format restatement* is paraphrase-grade — say so explicitly in
the body rather than letting one frontmatter field imply both. Expect the same
pattern for TN #5014 (CMap/CIDFont) and Adobe's predefined CMap resource files
when those get ingested.

**3. Font data splits a THIRD way: WIDTHS vs SHAPES — different files, different
licenses.** Metrics (advance widths) and outlines are separate dependencies with
separate grants, and conflating them produces a wrong verdict. pdfce takes
advances from the APAFML AFMs and outlines from the BSD-3-Clause Foxit set —
deliberately **not** the Foxit faces' own widths (5 glyphs diverge from the AFMs;
`Euro` in Times by 222/1000, i.e. 22% of an em).
**How to apply:** whenever a font question arrives, ask which of the two it is
before quoting a license verdict.

**4. A font "exception" clause is almost always DOCUMENT-EMBEDDING-ONLY, not
app-bundling.** `PS-or-PDF-font-exception-20170817` (URW/Nimbus, on top of
AGPL-3.0-only) permits including the font in "a Postscript or PDF file that
consists of a document" — so embedding a face into a PDF **pdfce produces** is
fine, while `include_bytes!`-ing it into `pdfce.exe` is **outside** the exception
and pulls in AGPL §5 + §13. **Why:** the exception's obvious purpose is to stop
copyleft leaking into end users' documents; it was never a bundling grant, and
reading it as one is the natural mistake. **How to apply:** for any font pdfce
might ship, ask "does the exception cover the *application* or only the
*output document*?" before proposing it as a bundled substitute face.

**6. AVAILABILITY ≠ REDISTRIBUTION LICENCE — and `LEGAL.md` §2's table conflates
them for Adobe Supplements.** Established 2026-07-31 ingesting the **Adobe
Supplement to ISO 32000, ExtensionLevel 3** (the only free source for AES-256
`/V 5`/`/R 5`/`AESV3`/`/OE`/`/UE`). `LEGAL.md` §2 lists "Adobe
Supplements/Extensions to ISO 32000" under **"Yes — historically freely
published"**, which is true of *getting a copy* and says nothing about
*reproducing it*. Its copyright page reads: *"no part of this guide may be
reproduced, stored in a retrieval system, or transmitted"* — i.e. **exactly the
TN #5004 posture** (memory item 2's "restricted document" column), not the
ISO-32000-1 posture.

**Why this matters more than it looks:** ISO 32000-1 *is* treated `free_primary`
with sentence-level quotation, and the supplement is formally an *amendment to
the same document*, published by the same company, describing the same clause.
The instinct to inherit ISO 32000-1's licence basis is strong and wrong.

**How to apply:** for any Adobe "Supplement"/"Extension"/technical note, **read
its own copyright page before choosing `license_basis`** and default to
`free_secondary_paraphrase` + "paraphrase only, never bulk-quote". Record the
deviation from `LEGAL.md` §2 in `index.md`'s licensing table **and surface it to
the user** rather than silently resolving it — this is the
"stop-and-ask-if-unsure" rule, discharged by applying the conservative reading
*and* reporting it.

**5. When a mainstream source is copyleft, sidestep rather than argue.** The
Ghostscript-bundles-the-AFMs question (AGPL *collection*, APAFML *files*) has a
defensible answer, but the cheaper move was to source from two non-copyleft
mirrors and cross-verify. Do that instead of writing a licensing argument.

See [[spec-source-extraction-toolchain]], [[pdf-spec-corpus-state]].
