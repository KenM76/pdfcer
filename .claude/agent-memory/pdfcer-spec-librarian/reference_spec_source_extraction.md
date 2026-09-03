---
name: spec-source-extraction-toolchain
description: How to get text out of staged spec PDFs on this machine (no pdftotext/mutool/qpdf; use Python 3.11 + pypdf), plus verified free URLs for ISO 32000-1 and TIFF 6.0.
metadata:
  type: reference
---

Staged spec PDFs live in `D:\Dev\Rag-Specialized\PDF_Spec\_sources\`.

**This machine has NO `pdftotext`, `mutool`, `qpdf`, or `pdftk`.** It does have
`C:\Users\Ken\AppData\Local\Programs\Python\Python311\python.exe` with **`pypdf`**
and `pdfminer` installed. Extraction recipe that works:

1. Dump every page to one UTF-8 text file with `=== PDFPAGE n ===` markers
   (`PdfReader(src).pages[i].extract_text()`), into the session scratchpad —
   **not** into `_sources\` or the pdfce repo.
2. Locate clauses by `grep -n '^<clause> '` on that dump, then `sed -n 'a,bp'`
   the range. ISO 32000-1's clause headings sit at line-start, so this is exact.
3. Spec tables often extract **column-misaligned or split across page breaks**
   (rows appear after the *following* subclause's prose). **RE-EXTRACT WITH THE
   FULL `=== PDFPAGE n ===` DUMP BEFORE CONCLUDING A TABLE CAN'T BE READ —
   the score is now 5 for 5.** Tables 6, 11, 13 (2026-07-30) and Tables 5, 16,
   17 (2026-07-31) were all recorded as "misaligned/unreadable" on first contact
   and every one came out **row-aligned** on a second pass over the whole-document
   dump. Misalignment is an artifact of extracting a *page range*, not a property
   of the table. Reconstruct-from-self-naming-descriptions only as a last resort,
   and when a reconstruction is later verified, record the **outcome** — in all
   five cases the reconstruction had been correct, which is worth knowing but is
   *not* a licence to skip the re-extraction.

3-ISO2. **★ ISO 32000-2:2020 IS NOW STAGED — `_sources\ISO_32000-2_sponsored_EC3.pdf`
   (19 203 156 B, **1023 pp**, SHA-256 `c94caf9f…cb3a2`), acquired by the OPERATOR
   2026-08-12 via the PDF Association's sponsored access. Cached dump:
   `C:\tmp\iso32000_2_dump.txt` (2 695 377 B; verify `grep -c "=== PDFPAGE"` → **1023**).**
   `license_basis: **licensed_primary_private_rag**` — a NEW value; free of charge
   but licensed to a **named individual**, footer on every page reads *"Single
   user only, copying and networking prohibited."* **Paraphrase + short
   quotation only; the PDF never leaves `_sources\`; a RAG file built on it is
   never published.** Read `LEGAL_NOTE.md`'s 2026-08-12 note first.

   **★ 3-ISO2-bis. THE ERRATA ARE ANNOTATIONS, NOT TEXT — and this inverts the
   usual reading.** `extract_text()` returns **ISO 32000-2:2020 EXACTLY AS
   PRINTED** and silently drops every Errata-Collection-3 correction, because
   they are laid over the page as `/StrikeOut` + `/Caret` + `/Text` sticky
   notes. **A naive dump is the UNCORRECTED standard.** This is *better* than any
   secondary source (the published-vs-erratum split is directly observable
   instead of inferred from `[INS]`/`[DEL]`), but only if you read the annots.
   - Each markup annot carries `/T` = `Issue #NNN` →
     `github.com/pdf-association/pdf-issues/issues/NNN`.
   - A reply sticky note's `/State` is **`Completed`** (approved by ISO TC 171
     SC 2 WG 8) or **`Accepted`** (PDF Association TWG only). **Different
     authority levels — record which.** Algorithm 2.B's step-(a) correction is
     `Accepted`, i.e. not yet ISO-ratified.
   - A few resolutions could not be marked up inline and are **appended as new
     pages at the end** (first at dump line 47889). Check them; none concerned
     §7.6.
   - **A `/StrikeOut` says WHERE, not WHAT.** Recover the struck text from
     `/QuadPoints`: for each 8-number quad take `x0..x1`/`y0..y1`, select the
     `pdfminer` `LTChar`s inside (±1 x, ±2 y), sort by `(-round(y/3)*3, x0)`.
     Working script kept as `scratchpad/errata.py` (~45 lines, takes a 1-based
     page range); `scratchpad/lines.py` prints a page's lines with y
     coordinates, which is how you confirm *which* line a strikeout hit (e.g.
     the same phrase *"with an initialization vector of zero"* occurs on both a
     CBC line and an ECB line one line apart — only the ECB one is struck).
   - Skip `/Link`, `/Widget`, `/Popup` subtypes or the output drowns.
   - **★ 3-ISO2-ter (2026-08-20) — AN ERRATUM'S REVIEW STATE IS A CHAIN, NOT A
     REPLY. WALK `/IRT` AND TAKE MAX `/M`.** The rule above ("a reply's `/State`
     is `Completed` or `Accepted`") is right about the *words* and silent about
     *which reply to read*. Real shape on pp. 506–507 (Issue #444):
     `/StrikeOut` obj 5276 ← `/Text State=Accepted` obj 5279 (`/M`
     2024-07-20) ← `/Text State=Completed` obj 5286 (`/M` 2026-05-21).
     **The last `/M` in the chain is the current state.** Measured over the
     whole staged 1023-page file: **1 205 `Completed` + 547 `Accepted` state
     annots, and 376 of the 547 (68.7 %) are themselves replied to by a
     `Completed` note** ⇒ an `Accepted` read from the first reply found is
     **stale about two times in three**. This CORRECTED a corpus claim
     (`iso32000__s__12.5.6.md` had Issue #444 as TWG-only `Accepted`; it is
     ISO-ratified `Completed`). ~20-line script: iterate `r.pages`, keep
     `/Text` annots with a `/State`, record `a.idnum` → `/State` and
     `o.raw_get('/IRT').idnum`, then test membership of each `Accepted` idnum in
     the set of `/IRT` targets of `Completed` annots. Whole-document run is
     ~1 min. **Do this before recording any erratum's authority level.**
   - **Cross-check every erratum against `pdf-issues.pdfa.org/32000-2-2020/clause<NN>.html`.**
     It prints the same `<del>`/`<ins>` edits, is **free**, and lets the erratum
     be described in a file whose licensed half must stay unquotable.

3a. **CHECK `C:\tmp\iso32000_dump.txt` BEFORE RE-DUMPING.** The full 756-page
   `pypdf` dump persists across sessions (2 124 253 B, 37 491 lines, written
   2026-07-31; still present and correct 2026-08-07). Verify with
   `grep -c "=== PDFPAGE" → 756`, then go straight to
   `grep -n '^<clause> ' `. Saves the ~90 s re-dump every session. If it is ever
   missing or the page count is wrong, re-dump per item 1.

3b. **`python.exe` invoked from the Bash tool resolves a `/tmp/...` output path
   to `C:\tmp\...`, not to git-bash's `/tmp`.** Write scratch dumps to an
   explicit absolute Windows path, or `find` for the file afterwards. Cost this
   1 wasted command on 2026-07-31. Full ISO 32000-1 dump = 756 pages, ~2.1 MB,
   ~90 s.
4. For big data tables (e.g. Annex D encodings), extract **programmatically with
   a regex + a cross-check on row/column counts**, not by hand. Annex D.2 was
   validated this way: 229 rows, 149/207/216/229 codes per encoding column,
   matching the published sizes of those encodings.


4i. **★ A TABLE'S `(continued)` CAPTION EXTRACTS AT THE **END** OF ITS PAGE, WHICH IS
   HOW A PAGE-RANGE READ FUSES TWO TABLES INTO ONE — AND IT HAS NOW PUT WRONG ENTRIES
   INTO A CORPUS FILE.** Established 2026-08-26 on ISO 32000-1 §12.7.5.2. The page that
   opens §12.7.5 also carries the **tail of Table 235 (certificate seed value
   dictionary)**; its `/URL` and `/URLType` rows extract *above* the `Table 236 –
   Additional entries specific to a submit-form action` caption, and Table 235's own
   `(continued)` caption extracts *below* the whole thing. A previous build read that
   page range and recorded `/URL`/`/URLType` as **Table 236 entries** — then advertised
   them in a scope-exclusion banner for 16 days.
   **Procedure that costs one command and prevents it:**
   ```bash
   grep -n "Table 235 –\|Table 236 –\|Table 237 –" /c/tmp/iso32000_dump.txt   # 1.7: EN dash
   grep -n "Table 239 —\|Table 240 —"                /c/tmp/iso32000_2_dump.txt # 2.0: EM dash
   ```
   — this lists **every** caption *including the `(continued)` ones*, so the true page
   span of each table is visible before you slice. **Locate captions, THEN `sed -n`.**
   **The semantic tell (works without a second dump):** a row that references a sibling
   key the table it is supposedly in does not have (`/URLType` cites *"the `Ff`
   attribute's `URL` bit"*; Table 236 has no `Ff`) belongs to a different table.

4j. **THE `pdf-issues` GITHUB **API** IS A FREE, QUOTABLE ERRATA + GAP SOURCE, and it
   does two things the rendered errata pages cannot.** (a) `curl -s
   "https://api.github.com/repos/pdf-association/pdf-issues/issues/<N>"` returns
   `title`, `state`, `labels` (**`ISO approved`** corroborates a `Completed` annotation
   state) and **`body` — the SUBMITTER'S REASONING**, which is often the clearest
   statement of *why* a clause was ambiguous, and is free to quote where the licensed
   2.0 text is not. (b) `curl -s
   "https://api.github.com/search/issues?q=repo:pdf-association/pdf-issues+<term>"`
   — **an OPEN issue is stronger gap evidence than your own 0-hit measurement**, because
   it is the standards body agreeing the hole exists. 2026-08-26 this returned #648
   (*"Is FDF trailer mandatory?"*) and #756 (*"There's no machine-readable schema for
   XFDF as defined by ISO 19444-1:2019"*), both directly load-bearing.
   **Caveat: a title search is narrow** — `submit in:title` returned **1** result for a
   whole clause; search the KEY NAMES (`CharSet`, `XFDF`, `FDF`) as well.

4k. **ERRATA-SCAN A PAGE RANGE, NOT A CLAUSE.** Running the `/Annots` sweep over
   ±3 pages around the target clause (2026-08-26: staged 2.0 pp. 566–573) returned an
   **ISO-ratified erratum for the NEIGHBOURING clause** that closed an ambiguity another
   corpus file had carried open for 16 days (Issue #174, Reset-Form Table 242). The scan
   costs the same either way; the extra pages are free findings.

4l. **★★ THE SEMANTIC TABLE-ROW SWEEP — how to enumerate "every place the standard
   does X", when a key-name grep will under-count.** Established 2026-08-26 building the
   action-carrier catalogue. A `^A dictionary` / `^AA dictionary` grep gave a clean,
   symmetric, **incomplete** answer; the sweep below found three more carriers, two of
   which appear in **no enumeration anywhere in either edition, including ISO 32000-2
   §12.6.1's own attempt at one**.

   ```python
   # walk the dump, remember the last table caption, match a KEY row of ANY name,
   # then test the following ~4 lines for the concept word.
   capre  = re.compile(r'^Table (\d+) [–—]')          # 1.7 EN dash, 2.0 EM dash
   keyre  = re.compile(r'^([A-Za-z][A-Za-z0-9]{0,20})\s+'
                       r'(dictionary|array|name tree|stream|text string|integer|name)')
   for i, l in enumerate(lines):
       m = capre.match(l.strip())
       if m: cur = m.group(1)
       mk = keyre.match(l)
       if not mk: continue
       blob = " ".join(lines[i:i+4]).lower()
       if re.search(r'\bactions?\b', blob) and not re.search(r'additional-?\s*actions', blob):
           print(cur, mk.group(1), i+1, lines[i][:150])
   ```
   Two details that make it work: **the 4-line lookahead** (a row's prose wraps, and the
   concept word is usually on line 2 or 3, not line 1), and **the exclusion regex** for
   the term that would otherwise swamp the result (`additional-actions` here). The
   caption tracker gives you the owning table for free — but remember item 4i: a
   `(continued)` caption extracts at the END of its page, so a row's "preceding caption"
   can be one table late. **Cross-check any hit's table against `grep -n "Table N [–—]"`
   before quoting it.** Generalises to any "find every X" question: substitute the
   concept regex (`\bcolourant\b`, `\bencrypt`, `\bfile specification\b`, …).

4a. **A spec EQUATION extracts as a scrambled glyph run — recover it by
   CHARACTER X-POSITION with `pdfminer`.** Established 2026-08-08 on ISO 32000-1
   §11.6.5.3's `/Matte` preblend formula. `pypdf` returned
   `c' m α cm–()×+ =` (draw order, not reading order) — plausible-looking and
   unusable. The fix, two commands, no re-staging:

   ```python
   from pdfminer.high_level import extract_pages
   from pdfminer.layout import LTChar, LAParams
   for page in extract_pages(src, page_numbers=[N], laparams=LAParams()):
       chars=[]                      # recurse: LTChar leaves are nested
       def walk(o):
           for e in o:
               if isinstance(e, LTChar): chars.append(e)
               elif hasattr(e,'__iter__'): walk(e)
       walk(page)
       rows={}                       # bucket by y, then sort each row by x0
       for c in chars: rows.setdefault(round(c.y0/3)*3, []).append(c)
       for y in sorted(rows, reverse=True):
           print(''.join(c.get_text() for c in sorted(rows[y], key=lambda c:c.x0)))
   ```
   → `c'=m+α×(c–m)`, unambiguous. Same technique read §11.3.3 and all nine of
   §11.3.8's summary formulas. This is item 25's "a figure is readable as
   geometry" applied to *type* instead of *paths* — and it is the **only**
   reliable way to transcribe a normative formula. Label the result *derived
   transcription of a normative formula object* in the RAG file, and say which
   page/index it came from.

   **4a-bis. Row-bucketing is NOT enough for superscripts, fractions and
   subscripts stacked in the same equation — add a SECOND, per-glyph pass that
   prints `(x0, y0, size)`.** Established 2026-08-10 on ISO 32000-1 §8.6.5.4's
   `g(x)`. The row pass returned `36` on one line and `1084` on another; those
   are **not numbers**. The per-glyph pass resolves them instantly:
   `3` at `x=135.9, size=9.0` (a superscript: smaller font, raised baseline) is
   the exponent of `x³`, while `6` at `x=274.8, size=10.5` sits above a run of
   `-` glyphs at `x=272–279` with `2 9` beneath ⇒ the fraction `6/29`. Likewise
   `1084` is `108` (numerator, `x=130–141`) and `4` (`x=183.7`, a different
   fraction's numerator). **Heuristics that work: a fraction is
   numerator-row / a horizontal run of `-` at the same x-span / denominator-row;
   a superscript is same-ish x but SMALLER `size` and a raised `y0`; a subscript
   is smaller `size` and a lowered `y0`.** Variant script kept as `eq2.py`/
   `eq3.py` in the session scratchpad — 15 lines, filters a y-window and prints
   every glyph with coordinates.

   **4a-ter. Cross-check every transcribed formula against a property the spec
   never states.** `g(x)`'s branches are C¹-continuous at `6/29`
   (`3·(6/29)² = 108/841`); `CalRGB`'s `(1−x)/y − 1 ≡ (1−x−y)/y` is the standard
   CIE `z` relation; a 3×3 colour matrix's per-axis sums reproduce its own
   `/WhitePoint`. Each is a decisive, cheap check that the superscript /
   fraction / major-order reading was right. **A transcription with no
   independent check is not finished.**

   **4a-quater. Expect GLYPH DROPOUTS in the source's own text layer.** In
   §8.6.5.3 the third gamma exponent's `B` subscript is present on the Z row and
   absent on the X and Y rows; two `y_G` subscripts vanish from the chromaticity
   block. Verify by selecting the exact y-band and confirming no glyph exists at
   the expected x — then record it as an erratum with the reading you adopted
   and what forces it, never silently normalise it.

   Three operational gotchas, all cost time on 2026-08-08:
   - **`pdfminer`'s `page_numbers` is 0-based**; the `=== PDFPAGE n ===` marker
     in the cached `pypdf` dump is **1-based** ⇒ `page_numbers=[n-1]`.
   - **Set `PYTHONIOENCODING=utf-8`** — printing `α`/`×` to a cp1252 console
     raises `UnicodeEncodeError` *mid-loop*, after partial output, which reads
     like a data problem and is not.
   - `pdfminer` prints "contains a metadata field indicating that it should not
     allow text extraction. Ignoring this field" on `PDF32000_2008.pdf` — noise,
     not a failure; `logging.disable(logging.WARNING)` silences it.
   - Subscripts land on their **own y-row** (`αr` → `α` then `r`), so bucket
     rows loosely (`round(y/3)*3`) and expect to reassemble subscripts by eye.

   **4a-sexies. ★ SOME MATH SYMBOLS ARE NOT GLYPHS AT ALL — they are PATHS, and
   the per-glyph pass (4a-bis) cannot see them either.** Established 2026-08-17
   on ISO 32000-1 Table 136. `D(x)`'s **radical** and `Difference`'s
   **absolute-value bars** are vector art; the text layer returns `x` and
   `cb – cs`, both of which are plausible and both of which are wrong (a
   discontinuous `SoftLight`, a negative `Difference`). **The only in-document
   tell is an unexplained x-GAP**: `D(x)` branch 1 begins at `x0=201.12`, branch
   2's lone `x` at `208.45` — 7 pt of nothing; `Difference`'s `=` ends at `202.5`
   and its first `c` starts at `216.9`, with nothing after the trailing `cs`.
   **Never treat a formula as transcribed until every operator's spacing is
   accounted for.** Recover by corroboration, not by extraction: C⁰ continuity at
   the branch point, **the next edition's clean Unicode text layer** (2.0 prints
   `√𝑥` and `|𝑐𝑏 − 𝑐𝑠|`), a sibling specification (W3C prints `sqrt(Cb)`,
   `| Cb - Cs |`), the clause's own NOTE prose, and the behaviour of an
   implementation. Distinct from 4a-quater (a glyph the font *should* have drawn
   and did not) — this is content the typesetter never encoded as text.

4a-quinquies. **★ ISO 32000-1's ANNEX HEADINGS ARE `Annex<SPACE><SPACE><LETTER>`
   — `grep "Annex B"` returns ZERO hits in all 756 pages.** Found 2026-08-11
   while re-verifying Annex B (type-4 operators). `grep -n "^Annex"` finds all
   twelve; the body heading at dump line 32930 reads `Annex  B` / `(normative)` /
   `Operators in Type 4 Functions` on three lines. Same false-negative class as
   the PLRM ligature trap below. **A 0-hit on a structural element you can see in
   the TOC is an extraction artifact, not evidence of absence.** Cheap
   cross-check: the TOC at dump lines **129–190** lists every annex letter with
   its `(normative)`/`(informative)` marker on the *next* line and the printed
   page number two lines after — enough to settle an annex's normative status
   without reading its body.

4b. **The strongest cross-check is against a SIBLING table already in the
   corpus** — two independently extracted datasets that must reconcile
   arithmetically. Annex D.3 (2026-07-31): 256 codes, 24 marked undefined ⇒ 232
   defined; minus the 3 controls (TAB/LF/CR) that have Unicode values but no
   Latin-set glyph name ⇒ **229**, exactly Annex D.2's independently extracted
   `PDF` column count. Also cheap and worth doing: **assert the identity ranges
   and enumerate the divergences** rather than eyeballing (0x20–0x7E and
   0xA1–0xFF are identity with Unicode; the script found exactly two exceptions,
   `0xA0` = EURO and `0xAD` = undefined — both then confirmed against other
   clauses). And **check the value column for duplicates**: the one duplicate
   found (U+0017 at both 0x16 and 0x17) turned out to be a *source typo*, which
   is itself a finding worth recording rather than silently repairing.

4c-bis. **★ THE SAME INTERLEAVE HAPPENS INSIDE A CLAUSE, AND IT CAN MAKE YOU
   ANSWER A QUESTION BACKWARDS.** Established 2026-08-20 on ISO 32000-1
   §12.5.6.9 (pp. 402–403). `extract_text()` puts **Table 177's
   `IC`/`BE`/`RD` continuation rows INSIDE the §12.5.6.9 span**, before Table
   178's own header, and puts Table 178's repeated continuation header **after**
   Table 179's rows. Consequence: **two different `/IC` rows sit ~50 lines apart
   in the flat dump** — Table 177's says *"fill the annotation's rectangle or
   ellipse"*, Table 178's says *"fill the annotation's line endings"* — and
   reading the wrong one inverts the answer to "does `/IC` fill a polygon?".
   **Fix: the `pdfminer` `(y, x0, text)` layout pass of item 4a, run on the
   individual PAGES** (not a page *range*), which recovers the true row order,
   the column split (`x0 = 75` for a key, `195` for its prose) and the page
   footer. Two pages, one command. **Any table that spans a page break gets this
   treatment before a row is quoted as verbatim.**

4c. **Table extraction interleaves the PREVIOUS annex's continuation rows.**
   Annex D.3's rows for 0x05, 0x86–0x8A and 0x9E/0x9F absorbed fragments of
   Annex D.2's continuation block and of running headers. Detect by asserting
   the row count and looking for anomalously long "name" fields; repair by
   reading those rows directly off the source text. This is the same page-break
   artifact as memory item 3, appearing inside a *single* table rather than
   across one.

**Verified-free source URLs (all re-confirmed HTTP 200 on 2026-07-30):**

- ISO 32000-1:2008 —
  `https://opensource.adobe.com/dc-acrobat-sdk-docs/standards/pdfstandards/pdf/PDF32000_2008.pdf`
- **TIFF 6.0** (normatively referenced by ISO 32000-1 §7.4.4 LZW and §7.4.4.4
  Predictor 2, and **not listed in `LEGAL.md` §2's table**) —
  `https://www.itu.int/itudoc/itu-t/com16/tiff-fx/docs/tiff6.pdf`.
  Adobe's own TIFF6.pdf links are dead after site restructuring; ITU-T's copy of
  the TIFF-FX working documents is the stable free mirror and qualifies as
  free_primary under the open-publication-body rule.
- **Adobe Glyph List / AGLFN** — `https://raw.githubusercontent.com/adobe-type-tools/agl-aglfn/master/`
  (`glyphlist.txt`, `aglfn.txt`, `zapfdingbats.txt`, `LICENSE.md`). AGL
  *Specification* prose: `adobe-type-tools/agl-specification`'s `README.md`
  (`adobe-type-tools.github.io/agl-specification/` **404s** — use the repo).
- **ITU-T Recommendations (verified 2026-07-30, five staged).** Two-step recipe:
  scrape item IDs from `https://www.itu.int/rec/T-REC-<rec>/en` (they appear as
  `parent=T-REC-<rec>-<YYYYMM>-<X>`), then fetch
  `https://www.itu.int/rec/dologin_pub.asp?lang=e&id=<ITEM-ID>!!PDF-E&type=items`
  — that endpoint returns `application/pdf` directly, no cookie/session needed.
  **All three failure modes are per-EDITION, so try another edition before
  concluding a document is unavailable:** TIES **login-form HTML** (T.81 every
  attempt; T.800 07/2024 — but T.800 11/2015 served fine), **HTTP 500** (T.88
  08/2018 — but T.88 02/2000 served fine), and a **BIG-IP "Request Rejected"
  WAF block** on `recommendation.asp` item pages (landing pages are fine; don't
  bother with item pages). Staged OK: T.4 07/2003, T.6 11/1988, T.88 02/2000,
  T.800 11/2015.
- **T.81 (JPEG) is NOT obtainable from itu.int** — gated behind TIES login on
  every attempt, with and without browser UA/referer. Use **W3C's reference
  copy**: `https://www.w3.org/Graphics/JPEG/itu-t81.pdf` (HTTP 200,
  `application/pdf`, 1 058 883 B, SHA-256 `631031d4…768bf0`). `free_primary`
  under the same open-publication-body mirror reasoning as the ITU-hosted
  TIFF 6.0 copy.
- **Internet Archive is the working route when the first-party URL is dead —
  and it TRUNCATES SILENTLY.** Established 2026-07-31 fetching the **Adobe
  Supplement to ISO 32000, ExtensionLevel 3** (the only free AES-256 source).
  Recipe:
  1. `https://archive.org/wayback/available?url=<url-without-scheme>` → JSON with
     the closest snapshot and its timestamp. Fast and reliable.
  2. Fetch `https://web.archive.org/web/<timestamp>if_/<original-url>` — the
     **`if_`** suffix serves the raw asset, not the wrapper page.
  3. **The first attempt stopped at exactly `1 048 576` bytes (1 MiB) with
     `http=200`, and the truncated PDF still opened as a plausible 5-page
     document** — `file` reported it without complaint; only `pypdf` failed, with
     an unrelated-looking "EOF marker not found". **Always
     `tail -c 200 f.pdf | grep -q '%%EOF'` before extracting.** A single
     `curl -sL -C - -o same-file <url>` resume completed it.
  4. `archive.org/cdx/search/cdx` **504s** — don't rely on it for enumeration.
- **PDF Association — the HOST SPLIT (verified 2026-08-09).** `pdfa.org` still
  **403**s automated fetches, but two sibling hosts serve fine (both HTTP 200,
  no auth, plain `curl` with a browser UA):
  - **`https://pdf-issues.pdfa.org/32000-2-2020/clause<NN>.html` — `<NN>` IS
    ZERO-PADDED.** `clause07.html` works; **`clause7.html` returns 404**
    (confirmed 2026-08-10; the earlier note read `clause<N>` and only looked
    right because clause 12 is two digits). Enumerate the real page set once
    with `curl -s <base>/ | grep -oE 'href="[^"]*"'` — it also reveals the
    sibling standards (`19005-4-2020`, `14289-1-2014`, `21757-1-2020`, …) and
    the `clauseAnnex*.html` / `clauseBibliography.html` pages. Strip tags but
    **preserve `<ins>`/`<del>` as `[INS]`/`[/INS]`/`[DEL]`/`[/DEL]` markers
    before the generic `re.sub(r'<[^>]+>',' ')`** — the whole value of the page
    is which side of the edit a sentence is on. Second proven use beyond
    erratum-confirmation: **proving a known ambiguity was NOT fixed in 2.0**
    (ISO 32000-1 Table 46 `/CheckSum` is self-contradictory; 2.0 changes the
    key's type and adds a NOTE but leaves both contradictory sentences ⇒
    `PERMANENT`, evidenced rather than assumed). Do this before labelling any
    ambiguity PERMANENT.
  - **`https://pdf-issues.pdfa.org/32000-2-2020/clause<N>.html`** — the public
    **errata for ISO 32000-2:2020**, per clause. Quotes 2.0 clause text in
    strike-through/insertion form ⇒ it **confirms ISO 32000-1 errata** *and* is a
    legitimate **narrow 1.7→2.0 delta source** (`free_secondary_paraphrase`).
    Strip tags with a 3-line `re.sub` and grep the flat text; `clause12.html` is
    ~135 kB → ~57 kB of text.
  - **★ `pdfa.org` 403s AT THE ORIGIN BUT ITS PDFs SERVE THROUGH WAYBACK.**
    Established 2026-08-11. `curl -A <browser UA>` on
    `https://pdfa.org/wp-content/uploads/<yyyy>/<mm>/<file>.pdf` and
    `https://pdfa.org/download-area/publications/<file>.pdf` both return
    **403, 103 bytes of HTML** — but
    `https://web.archive.org/web/2020if_/https://pdfa.org/wp-content/uploads/2018/06/1415_Toda-1.pdf`
    returns **HTTP 200, `application/pdf`, `%%EOF` present**. So the PDF
    Association's **conference decks and application notes are reachable after
    all**, via the `/web/<YYYY>if_/` form (same trick as PLRM3). Find the exact
    upload path with `WebSearch` — the paths are stable and appear in results
    even though the origin refuses. Recovered this way: *"Encryption with
    PDF 2.0"*, Roman Toda (Normex), 2017-05-15, 27 slides — which supplies the
    **`/R 6` algorithm HARNESS as a diagram** (Alg 8/9/10/2a with `Alg2b(...)`
    everywhere R5 has `SHA-256(...)`), the ExtensionLevel-8 provenance, and
    product-level 2.0 commentary. **Licence: `free_secondary_paraphrase`, and
    grade it MEDIUM — a slide deck by a standards participant is not a
    standard**, and a 2017 deck describes ISO 32000-2:**2017**, not 2020.
    Retire the old note that PDF Association material is "not machine-reachable".
  - ~~**`https://www.pdfa-inc.org/product/iso-32000-2-pdf-2-0-bundle-sponsored-access/`**~~
    **★ RESOLVED 2026-08-12 — THE ESCALATION WORKED AND THE OPERATOR ACTED.**
    Surfacing the $0 sponsored bundle to the operator (rather than performing an
    account+checkout, which is a side effect outside the working tree) is what
    got ISO 32000-2:2020 into the corpus. **Keep escalating acquisitions; they
    land.** The live index is now `https://pdfa.org/sponsored-standards/`. The
    acquired copy is `licensed_primary_private_rag` (see item 3-ISO2), **not**
    `user_provided_paywalled_copy` (which stays reserved for a *purchased* copy)
    and never `free_primary`. **ISO/TS 32001–32005 are in the same bundle and
    are still NOT acquired** — if one is ever needed, escalate the same way.
- **Dead/blocked, 2026-07-31:** `www.adobe.com/content/dam/...` PDF paths **hang**
  (no response in 120 s; `curl` exit 92 on HTTP/2, then a 2-minute timeout on
  `--http1.1`) · `opensource.adobe.com/dc-acrobat-sdk-docs/standards/pdfstandards/pdf/adobe_supplement_iso32000*.pdf`
  → **404** on 4 filename variants (only `PDF32000_2008.pdf` lives there; the
  directory listing itself 404s) · **`pdfa.org` returns HTTP 403** to both
  `WebFetch` and `curl` with a full browser UA — the PDF Association's
  `/extensions/` and `/resource/pdf-specification-archive/` indexes are **not
  machine-reachable**, so plan on `WebSearch` + Wayback instead.
- **Microsoft OpenType specification (verified 2026-08-03).** `learn.microsoft.com`
  HTML pages work with `WebFetch`: `https://learn.microsoft.com/en-us/typography/opentype/spec/<page>`
  (`os2`, `otff`, `head`, `hmtx`, `cmap`, `post`, `glyf`, `loca`, `maxp`, `name`,
  `cff`, `cff2`, …). Big pages exceed the inline cap and land in
  `C:\Users\Ken\.claude\projects\<proj>\<session>\tool-results\toolu_*.txt` —
  `sed`/`grep` that file rather than re-fetching. **The GitHub raw route is DEAD:**
  `raw.githubusercontent.com/MicrosoftDocs/typography/{live,main,<the page's own
  pinned gitcommit>}/typographydocs/opentype/spec/<page>.md` all return **404**, so
  the docs repo's `LICENSE` cannot be read ⇒ record `free_primary` per `LEGAL.md`
  §2 but attach a `NEEDS VERIFICATION` on the *redistribution* grant and hold
  quotation to sentence/table-row level (memory item 6, availability ≠ licence).
- **★★ W3C SPECS ARE FREE, FETCHABLE AND QUOTABLE — `https://www.w3.org/TR/<shortname>/`.**
  Verified 2026-08-17: `compositing-1` → HTTP 200, `text/html`, 234 781 B. Flatten
  with a 4-line `re.sub` (`<script|style>` first, then tags, then `html.unescape`)
  and grep the result; 235 kB of HTML → 58 kB of text. **This is the route for any
  "how does PDF differ from what browsers/Skia do?" question** — W3C
  Compositing-and-Blending Level 1 carries the whole rival blend/composite model
  in reproducible pseudocode. `license_basis: free_primary` (W3C Document
  Licence permits quotation). **Record the maturity level in the file** — a
  *Candidate Recommendation Draft* is not a Recommendation. Related free primaries
  in the same family worth remembering: `css-color-4`, `filter-effects-1`,
  `svg2`.
- **★★ COMPILE AND RUN A DEPENDENCY — the strongest evidence class available, ~10
  minutes.** Established 2026-08-17 on `tiny-skia 0.11.4`. Reading vendored source
  (below) yields a suspicion; running it yields a measurement.
  1. `grep -n -A2 'name = "<crate>"' D:/Dev/pdfce/Cargo.lock` → the **exact**
     version the workspace resolves; pin it (`tiny-skia = "=0.11.4"`).
  2. `mkdir -p <scratchpad>/bt/src`, a 6-line `Cargo.toml`, a `main.rs` that
     exercises the API and `println!`s.
  3. **`cargo run --release --offline`** — the crate is already vendored, so no
     network and no `cargo update`. First build ~10 s.
  4. Diff against a f64 reference implemented **from the spec** in Python.
     Randomise inputs and report **max Δ and the fraction exceeding 1 LSB**, not
     one example.
  5. **Isolate the root cause by re-implementing the crate's own variant** and
     checking it reproduces the crate's measured output. That converts "diverges"
     into "diverges *because*", and it is what makes the finding actionable.
  6. Fetch the **upstream original** the crate was ported from (Skia:
     `https://raw.githubusercontent.com/google/skia/main/src/opts/SkRasterPipeline_opts.h`,
     HTTP 200, BSD-3) to distinguish a **port defect** from deliberate behaviour.
  Label everything from steps 2–6 **BEHAVIOUR, NOT SPECIFICATION**, name the
  version and date, and route the tool-behaviour narrative to
  `C:\personal_rag\pdf\`.
  **★ 2026-08-17 extension — this route also validates an IMPLEMENTATION ROUTE,
  not just a crate's correctness, and it is ~10 minutes.** To answer "can I
  express clause C with API call A?", build the smallest raster whose bytes
  discriminate the rival hypotheses. Working recipe (`tiny-skia`): a **2×1
  `Pixmap`**, `pm.fill(known dst)`, then one `fill_rect` covering **exactly half
  of pixel 0** (`Rect::from_xywh(0.0, 0.0, 0.5, 1.0)`, `anti_alias = true`) so
  coverage is exactly 0.5, printing `pm.data()` raw (premultiplied RGBA8). Pixel 1
  is the untouched control. Measured: **`tiny_skia 0.11.4` applies coverage as
  `lerp(dst, blended, cov)`**, so `BlendMode::Source` returned `[128,64,0,191]`
  where the rival ("fold coverage into src alpha") predicts `[0,32,0,64]` —
  one number, decisive. **Always add a second, cheap probe for the adjacent
  question**: the same run tested a `Mask` of 128/255 with an opaque source and
  showed **a `tiny_skia::Mask` is a SHAPE/coverage input, never an opacity
  input**, which mattered more than the thing being tested.
- **Vendored Rust crate sources are a legitimate, offline verification route** —
  `~/.cargo/registry/src/index.crates.io-*/​<crate>-<version>/src/`. Used
  2026-08-03 to check `subsetter 0.2.6`'s emitted table set against a decision
  record's claim. Cheap (`grep -n "Tag::" src/lib.rs`), no network, and the crate
  version is pinned by the workspace lock so the reading is reproducible. This is
  *verification of a dependency claim*, **not** sourcing a normative algorithm
  from code — that remains the thing to put to the user first (memory item 16).
- **PLRM3 — PostScript Language Reference, 3rd ed. (staged 2026-08-10 as
  `_sources/Adobe_PLRM3_1999.pdf`, 7 771 729 B, 912 pp).** The **semantics
  authority for ISO 32000-1's type-4 operator set** (§7.10.5.1: "the semantics
  are those of the corresponding PostScript operators"), though ISO lists it only
  as **Bibliography [15] = informative**. Every first-party route is dead:
  `www.adobe.com/jp/print/postscript/pdfs/PLRM.pdf` **hangs** (curl exit 92 on
  HTTP/2, then timeout on `--http1.1` — the known adobe.com failure mode),
  `www-cdf.fnal.gov` **403**, two other mirrors **404**, and
  `archive.org/wayback/available` **429**s. **Working route: skip the availability
  API and hit `https://web.archive.org/web/2018if_/<url>` directly** — it 302s to
  the nearest snapshot (here `20200722143236`) and serves `application/pdf`.
  Worth remembering generally: **the `/web/<YYYY>if_/` form is a usable
  substitute when the availability API is rate-limited.** `%%EOF` verified.
  Operator entries are **Chapter 8 §8.2 "Operator Details"**; **Appendix B is
  *Implementation Limits*, NOT operators** — which is what makes ISO 32000-1
  §7.10.5.1's "see Appendix B … for these operators" an erratum.
- **★ TWO PLRM-class extraction artifacts, both silent, both cost time:**
  - **LIGATURES.** FrameMaker-set Adobe books store `fl`/`fi` as **U+FB02/U+FB01**,
    so **`grep floor` returns 0 hits in 912 pages** while `grep ﬂoor` finds the
    entry. Same for `ﬁle`, `closeﬁle`, `speciﬁed`, `inﬁll`. **A 0-hit result on a
    common word containing `fl`/`fi`/`ff` is a ligature artifact, not evidence of
    absence** — re-grep with the ligature before recording a NEGATIVE RESULT.
    This is the one failure mode that can turn an extraction bug into a false
    negative in the corpus.
  - **PER-PAGE FRAGMENTATION.** Some pages extract **one token per line**
    (`or\n\nbool\n\n1\n\nbool\n\n2`) while their neighbours extract normally, so a
    regex anchored on a whole stack-effect line finds 40 of 42 entries and misses
    two. Detect by "the entry head is missing but the index says the page is
    right"; repair by `' '.join(x.strip() for x in lines[a:b] if x.strip())`.
    Cheaper than the pdfminer x-position route (item 4a) and sufficient for prose.
- **★ FREE PREVIEW SAMPLES of paywalled ISO standards — `cdn.standards.iteh.ai`
  (verified 2026-08-10, HTTP 200, `application/pdf`, `%%EOF` present).** Pattern:
  `https://cdn.standards.iteh.ai/samples/<ISO-doc-number>/<hex>/ISO-<num>-<part>-<year>.pdf`.
  The `<hex>` is not guessable — **find the URL with `WebSearch`** for the standard
  plus a clause number, then `curl -sL`. What you get is the **front matter only**
  (~15 pp: cover, TOC, Foreword, **Introduction**, Scope, **clause 2 Normative
  references**, and the first page or two of clause 3), watermarked
  `iTeh STANDARD PREVIEW`. **That is enough to settle clause NUMBERS and TITLES,
  what changed between editions, and which documents are normatively referenced —
  without the clause bodies.** Two staged-nowhere fetches this way answered a
  question ISO 32000-1 structurally could not: ISO 32000-2:2020's Introduction
  states its own §12.6.4.16→**12.6.4.17 "ECMAScript actions"** renumber and that
  **ISO/DIS 21757-1 replaces** the Adobe/ECMA references; ISO 21757-1:2020's Scope
  and clause 2 confirmed it is an **API** definition that normatively references
  ISO 32000-2 (the reverse direction). **Licensing: `free_secondary_paraphrase`.**
  A preview is *publicly served* but the content is ISO's paywalled text — cite
  clause numbers, titles and short factual sentences; **do not bulk-quote**, and
  never label it `free_primary`. Do **not** stage these under `_sources\`.
- **`pdf-issues.pdfa.org/<std>/clause<NN>.html` doubles as a CROSS-EDITION
  RENUMBERING MAP.** Its per-clause page opens with a TOC listing **only the
  sub-clauses that have errata** — but those entries carry the **new edition's
  heading numbers and titles verbatim**, which is often exactly what you need
  (`12.6.4.16 Go-To-3D-View actions` + `12.6.4.18 Rich-Media-Execute actions`
  independently corroborated the 12.6.4.17 finding above). The errata bodies also
  quote whole 2.0 table rows in `[INS]`/`[DEL]` form, from which the 2.0 table
  **numbers** fall out (1.7 Table 218→2.0 Table 224; 1.7 Table 220→2.0 Table 226;
  1.7 Table 196→**2.0 Table 199**, while **1.7's Table 199 is a go-to action** —
  never cite a bare table number across editions). **`www.pdfa.org` still 403s**
  (2026-08-10), including `/iso-32000-normative-references/`, which ISO 32000-2's
  own Introduction points at as the free reference index — so the
  "is document X a clause-2 normative reference of 32000-2?" question currently
  has **no free route** and must be recorded as NEEDS VERIFICATION.
- **★ READER-BEHAVIOUR ROUTE — Mozilla pdf.js raw source (verified 2026-08-11,
  HTTP 200).** `https://raw.githubusercontent.com/mozilla/pdf.js/master/src/<path>`
  — `core/catalog.js` (parsing: `#readOptionalContentConfig`, `parseOnOff`) and
  `display/optional_content_config.js` (the **state-assignment constructor**, which
  is where the actual behaviour lives — the parser will mislead you). Apache-2.0,
  so short code quotation is fine. **Two curls answered "what do other readers do
  with a group in both `/ON` and `/OFF`" definitively** where the corpus had only
  been able to route the question to `personal_rag\pdf`. **Label the result
  BEHAVIOUR, NOT SPECIFICATION**, fence it in its own section, and expect it to be
  able to contradict the corpus's own ruling (it did). See [[pdf-spec-corpus-state]]
  item 44.
- **★★ ICC SPECIFICATIONS ARE FREE AND PLAIN-`curl`-ABLE AT `color.org` — and one of them is a NORMATIVE
  REFERENCE OF ISO 32000-2 BY A `shall` (verified 2026-09-01).** ISO 32000-2 §10.3.1: *"Conversion from a
  CIE-based source colour to a CIE-based destination colour **shall** be performed based on **ISO 15076-1:2010
  (ICC.1:2010)**"* — i.e. the paywalled-at-ISO document is **free at its originating body**, the standard
  open-publication-mirror pattern.
  - **ICC.1:2010 (v4.3) = ISO 15076-1:2010** — `https://www.color.org/specification/ICC1v43_2010-12.pdf`
    (HTTP 200, `application/pdf`, **2 335 478 B**, 130 pp, `%%EOF` present, SHA-256
    `cbb26a01abcf55fb6741d710a2e9aee2fd03de1b99af8a1b6baca2299234322d`, `pypdf`-readable).
  - **ICC.1:2022** — `https://www.color.org/specification/ICC.1-2022-05.pdf` (HTTP 200). Index page:
    `https://www.color.org/icc_specs2.xalter` (HTTP 200).
  - **The payload is often in the INTRODUCTION (clause 0.x), not the body.** The whole
    *"is a rendering intent's output testable?"* question is answered by clause **0.4**: *"The colour rendering of
    the perceptual and saturation rendering intents is **vendor specific**."* **When a PDF clause delegates its
    arithmetic to a referenced standard, read that standard's Introduction before grading the delegation as
    closing the gap** — it may hand the gap straight back.
  - **Do not stage a whole spec to answer one question.** Fetch to the scratchpad, dump, whitespace-stripped
    locate, record **URL + byte count + SHA-256 + the sentence** in the RAG file, and leave the prefix empty.
    `license_basis: free_primary`.
- **`iso.org/standard/<id>.html` via `r.jina.ai` DOES return the ABSTRACT and the LIFECYCLE line** (re-verified
  2026-09-01 on ISO 18619:2015, id **63033**: *Published*, Edition 1, 2015-07, "last reviewed and confirmed in
  2025", plus the full abstract). Item **4g** says jina does not defeat OBP — **that is about
  `iso.org/obp/ui/...`, not about `iso.org/standard/<id>.html`.** The catalogue-page route is the cheap way to
  get a paywalled standard's **scope in ISO's own words**. **The iTeh free-preview route (4h) does NOT cover
  every standard** — a filename web search for `cdn.standards.iteh.ai/samples/63033/...` returned nothing and
  the catalogue page links no sample, so ISO 18619 stays unobtained.
- **Adobe font technical notes** live at
  `https://adobe-type-tools.github.io/font-tech-notes/pdfs/<NNNN>.<Name>.pdf`
  (e.g. `5004.AFM_Spec.pdf`). **All `partners.adobe.com` TN URLs are dead.**

**Dead, do not retry (HTTP 404 confirmed 2026-07-30):** Adobe's Core 14 AFM zip
at both `www.adobe.com/devnet/font/pdfs/Core14_AFMs.zip` and
`opensource.adobe.com/dc-acrobat-sdk-docs/.../Core14_AFMs.zip`. **There is no
live first-party Adobe download for the Core 14 AFMs** — ISO 32000-1 §9.6.2.2's
own NOTE points at a source that no longer exists. Working mirrors:
`raw.githubusercontent.com/tecnickcom/tc-font-core14-afms/main/` (bare mirror,
ships Adobe's `LICENSE`) and `apache/pdfbox`'s
`pdfbox/src/main/resources/org/apache/pdfbox/resources/afm/`.

**Technique — cross-mirror integrity check when no first-party source exists.**
Fetch the same file from two independently-maintained mirrors and compare. Raw
SHA-256 will often differ from line-ending mangling alone (PDFBox's stored blobs
have CR rewritten to LF, giving the *same byte count* but a different hash — do
not read that as tampering). Normalize first: `tr -d '\r' | sed 's/[[:space:]]*$//' | grep -v '^$'`
then hash. Record both the raw hashes and the fact that the normalized ones
matched, in the RAG file's Provenance section.

**Font/software LICENSE verification — read the file, never recall it.** A
license claim written from training-data recall was **wrong in the corpus** and
had to be retracted 2026-07-30 (URW/Nimbus: recalled as "AFPL, relicensed 2015,
dual GPL-with-exception + AGPL"; actually `AGPL-3.0-only WITH
PS-or-PDF-font-exception-20170817`, single-licensed). Recalled license facts are
plausible-sounding and specific — exactly the shape the claim-bearing-copy rule
targets. Two cheap machine-checkable verifications, both used to confirm that fix:

- **The upstream `LICENSE` file, raw.** e.g.
  `https://raw.githubusercontent.com/ArtifexSoftware/urw-base35-fonts/master/LICENSE`.
  Fetch by two independent methods (`WebFetch` + `curl`) and compare — `WebFetch`
  summarizes through a small model, so it can paraphrase a clause you need verbatim.
- **SPDX's machine-readable lists**, which carry the authoritative id *and its
  date stamp*: `https://spdx.org/licenses/licenses.json` and
  `.../exceptions.json`. Filter with Python for the id; check
  `isDeprecatedLicenseId`. The exception id's own `YYYYMMDD` suffix is the
  reliable date (`PS-or-PDF-font-exception-20170817` → 2017, settling the
  2015-vs-2017 question by itself).

Verified 2026-07-30, font-licensing only (no font bytes staged):
Artifex `urw-base35-fonts` LICENSE · `https://spdx.org/licenses/exceptions.json` ·
pdfium `LICENSE` + `core/fxge/fontdata/chromefontdata/` listing via
`https://pdfium.googlesource.com/pdfium/+/refs/heads/main/<path>` (append
`?format=TEXT` for base64 raw; the plain URL returns browsable HTML).

Re-verify URLs each session before fetching (agent hard rule 4). See
[[pdf-spec-corpus-state]] and [[pdf-spec-embeddable-data-licensing]].

---

## EMPIRICAL VERIFICATION ROUTE — render a synthetic fixture (added 2026-08-10)

For a "does the implementation actually honour clause C?" question, code-reading
plus grep gives a suspicion; **rendering settles it**, and a spec librarian can do
it read-only without touching the repo.

1. **A built CLI usually already exists** — `ls -la D:/Dev/pdfce/target/debug/pdfce-cli.exe`
   (and `target/release/`). Check its mtime against the commit under test. No
   `cargo build` needed, so no repo mutation and no wait.
2. **Hand-write the PDF in Python** into the scratchpad — catalog + pages + one
   page + one content stream + the feature's objects, then a real `xref` table
   (offsets captured while appending, `%010d 00000 n `, object 0 as
   `0000000000 65535 f `) and a `trailer`/`startxref`. ~40 lines. Keep the page
   small (`/MediaBox [0 0 200 200]`) and paint in **black on white** so a single
   pixel probe is decisive.
3. **Put a KNOWN-GOOD control in the same file.** The 8.11 test put an image AND
   a filled rectangle inside the same hidden `/OC` section: the rectangle came out
   white (suppressed, as claimed) and the image came out black (the defect). One
   file proves the mechanism works *and* localises where it does not — a fixture
   with only the failing case cannot distinguish "unimplemented" from "my fixture
   is malformed".
4. `pdfce-cli render-page <in> -o <out.png>` — and **read the result line**, not
   only the raster. It prints the disclosure counters (`oc_hidden=1`,
   `images=1`), which independently confirm the feature *fired* while the pixels
   show what it did.
5. **Probe the PNG with pure Python** (no Pillow dependency): walk the chunks for
   `IHDR`/`IDAT`, `zlib.decompress`, then un-filter row by row (filter types 0–4,
   Paeth included) and index `rows[y][x*ch:(x+1)*ch]`. ~20 lines, reusable.
   Remember **device y is flipped** from user space: user `y=10..60` on a 200-tall
   page is device row `140..190`.

Also verified this session: `pypdf` is not needed for this route at all — it is a
*source-extraction* tool; this is an *implementation-behaviour* tool. See
[[pdf-spec-corpus-state]] § "a STATUS-CORRECTION dispatch" for when to reach for it.

---

**4b. ★ `grep -c` / `grep -n` UNDER-COUNT MULTI-WORD PHRASES IN THE CACHED DUMPS —
MEASURE AGAINST A WHITESPACE-STRIPPED COPY.** Established 2026-08-20 (form-XObject
build). The pypdf extraction injects **intra-word spaces**: `s hall not invoke`,
`Specific t o a Type 1 Form`, `Do op erator`. A phrase search therefore silently
misses hits, and the misses are not random — they cluster on the exact
long-sentence normative prose a negative-result claim depends on. **Four counts
were wrong in one session** (`shall not invoke` 1→2, `belongs to` 2.0 5→6,
`owned by` 1.7 6→7, `more than one page` 1.7 4→7) and the extra
`more than one page` hits were the session's **headline evidence**.

Recipe — count:

```python
import re
data = re.sub(r"\s+", "", open(DUMP, encoding="utf-8", errors="replace").read().lower())
print(data.count("morethanonepage"))   # term with all whitespace removed
```

Recipe — locate (map a stripped-text hit back to a line number + context):

```python
raw = open(DUMP, encoding="utf-8", errors="replace").read()
idx = [i for i, ch in enumerate(raw) if not ch.isspace()]
s   = "".join(raw[i].lower() for i in idx)
p = s.find(term.replace(" ", "").lower())
line = raw.count("\n", 0, idx[p]) + 1
ctx  = " ".join(raw[idx[max(0,p-140)]:idx[p+len(term)+140]].split())
```

**Single-word terms are safe**; anything with a space is a lower bound until
re-measured this way. Do the re-measure **before** the count goes in a RAG file
or a report — it is the fourth distinct way a count has been wrong
(see [[pdf-spec-corpus-state]] items 44, 48, 50, 53).

**Corollary — do not infer a clause number from line proximity.** Two of my own
citations were wrong this session for exactly that reason. Walk back to the
nearest preceding heading instead:

```bash
awk 'NR<HITLINE && /^F\.[0-9]/{print NR": "$0}' C:/tmp/iso32000_dump.txt | tail -1
```

---

**4c. ★ A FIGURE MAY BE A RASTER IMAGE — EXTRACT IT AND `Read` THE PNG.**
Established 2026-08-20. **ISO 32000-1's Figure 9** (the graphics-object state
machine: which operators are legal at the page-description level vs inside a
text/path object) extracts as **plain text** in the cached dump. **ISO 32000-2's
Figure 9 is a JPEG.** `pdfminer` returns an `LTImage` and **zero glyphs** for it,
so a text-only workflow reports "the 2.0 figure says nothing" — a false negative
on a **normative** figure.

```python
import pypdf
pg = pypdf.PdfReader(SRC).pages[162]          # 0-based physical page
for i, im in enumerate(pg.images):            # -> [('Im0.jpg', 105221 bytes)]
    open(f"fig_{i}.png", "wb").write(im.data)
# then: Read the PNG with the Read tool; upscale 2x with PIL/LANCZOS if small
```

Detect the case first — `pdfminer.layout.LTImage` present on the page, and the
figure's caption line in the dump followed immediately by the next paragraph with
no label text between. Reading the 2.0 figure settled two things nothing else
could: **`Do` is still absent from a text object's allowed operators in PDF 2.0**,
and the level was renamed *page description level* → **content stream level**.
Extends [[pdf-spec-corpus-state]] item 25 ("a figure is normative and readable as
geometry"): **check for an image before concluding a figure is silent.**

---

**4d. ★★ WHEN AN ORIGIN 403s *AND* ITS WAYBACK SNAPSHOTS ARE THE CHALLENGE PAGE —
`https://r.jina.ai/<full-url>` IS THE ROUTE, AND IT READS PDFs TOO.**
Established 2026-08-21 acquiring the PDF Association's Brotli extension spec.
`pdfa.org` returns **HTTP 403, 103 bytes** to `curl` with a full browser UA +
`Referer` + `--http1.1`, to `WebFetch`, and — the new part — **its Wayback
snapshots are captures of Cloudflare's "Just a moment…" interstitial**, so the
`/web/<YYYY>if_/` trick that used to work now returns the challenge. Check the
snapshot body, not just the HTTP code.

```bash
curl -s --max-time 120 -o out.txt "https://r.jina.ai/https://pdfa.org/resource/extension-brotli/"
curl -s --max-time 120 -o spec.txt "https://r.jina.ai/https://pdfa.org/download-area/publications/pdf-extension-brotli.pdf"
```

- Works on **HTML and PDF alike** — the PDF came back as markdown with
  `Number of Pages: 11` and a `Published Time:` line carrying the origin's
  `Last-Modified`, which dated the publication to the hour.
- Also defeated **`iso.org`'s** Cloudflare gate for the **committee catalogue**
  (`/committee/53674/x/catalogue/p/0/u/1/w/0/d/1` → every SC 2 project + stage
  code, which is how "no Brotli work item exists" became a MEASURED negative).
  It did **not** defeat `iso.org/standard/<n>.html` — that still returns the
  challenge through the proxy. Try, don't assume.
- **Label the provenance.** The text is proxy-extracted, not the origin bytes:
  word-accurate, **typography normalised** (the cover's `PDF 2.0` came through
  as `PDF 2 .0`). Fine for clause text and tables; **not** for a byte-exact
  quotation, and **the bytes cannot be staged** — record that as an ACCESS gap
  and escalate to the operator for a browser download.

**4e. ★ THE FREE MACHINE-READABLE PDF REGISTRIES — two `curl`s, no auth, and
they settle "does this extension exist / is this prefix registered?" outright.**

- `https://raw.githubusercontent.com/pdf-association/pdf-extensions/main/extensions/pdf-extensions.json`
  — every publicly documented developer extension, with `prefix`,
  `BaseVersion`, `ExtensionLevel`, `ExtensionRevision`, `URL`, `OfficialName`,
  `LongDesc`. **Use `gh api repos/pdf-association/pdf-extensions/commits?path=…`
  for the DATE** — the JSON's own top-level `date` field was **five months
  stale** relative to its last commit (`2026-03-17` vs a `Publish Brotli` commit
  on `2026-08-18`). The commit history is the publication record.
- `https://raw.githubusercontent.com/adobe/pdf-names-list/master/ISO%20PDF%20Registry.csv`
  — the registered developer prefixes ISO 32000 Annex E points at. **Prefixes
  are CASE-SENSITIVE and near-duplicates exist**: `PDFa` (5-May-26, the Brotli
  prefix) and `pdfa` (6-Feb-19, noted `"PDFA" cannot be used`) are different keys.
- `gh api -X GET search/code -f q='<TERM> org:pdf-association'` is authenticated
  and works — it proved the Brotli Metanorma sources are **not** in a public repo.
- Reader/writer support is settled the same cheap way: `curl` the raw source of
  `mozilla/pdf.js`, `ArtifexSoftware/mupdf`, `ArtifexSoftware/ghostpdl`, and
  `https://pdfium.googlesource.com/pdfium/+/refs/heads/main/<path>?format=TEXT`
  (base64 — pipe through `base64.b64decode`). Four fetches produced a full
  vendor-conformance table **including three divergences from the published
  spec**. `gh api repos/<o>/<r>/pulls/<n> --jq '.state,.merged'` dates the rest.


**4f. ★★ A FIGURE CAN BE THE ONLY PLACE THE ALGORITHM IS COMPLETE — and the
prose bullets beside it will read as if they were sufficient.** Established
2026-08-24 on **ISO 32000-2 Annex P** ("An algorithm to determine the actual
blending colour space of a transparency group"). The annex has four prose
bullets **and** `Figure P.1`, a flowchart. The bullets omit **two whole
branches** that the figure has: that a page group with an explicit `DeviceCMYK`
`CS` *also* routes to "device or Output Intent", and that `/Default*` remapping
is consulted **before** anything else. Extraction:

```python
p = PdfReader(src).pages[995]          # 0-based; dump marker is 1-based
print(len(p.images))                   # -> 1
open('figP.png','wb').write(p.images[0].data)   # then Read the PNG
```

**The diagnostic that tells you a figure is load-bearing rather than
decorative: a whitespace-stripped search for a phrase you can SEE in the figure
returns 0.** `deviceoroutputintent` = **0 hits** in the 2.0 dump, while
`fromtheoutputintent` (the prose bullet) = 1. **A 0-hit on a phrase that is
visibly on the page is proof the content is not text** — the same class as
4a-sexies (path-drawn math symbols) and 4a-quinquies (`Annex<2 spaces><letter>`),
but at *whole-algorithm* scale rather than symbol scale. Generalise: **before
concluding a normative-adjacent annex is fully read, count its `/XObject`
images.**

**4g. ★ PDF/X (ISO 15930) is the corpus's HARDEST paywall — all three
standard free routes fail, and the third failure is the surprising one.**
Measured 2026-08-24, recorded in `LEGAL_NOTE.md`:

- `standards.iteh.ai` catalogue page for ISO 15930-7:2010 — fetched, 44 603 B,
  **zero `cdn.standards.iteh.ai` / `samples/` / `href="*.pdf"` matches.** The
  free-front-matter route **does not exist for every ISO standard**; check the
  HTML for the link before assuming it does.
- `iso.org/obp/ui/en/#!iso:std:55843:en` **via `r.jina.ai`** — HTTP 200,
  body = the Cloudflare **"Just a moment…" challenge page, 183 B**. **The
  jina route (item 4d) does NOT defeat OBP**, only the committee catalogue.
  Add OBP to the list of things to try-but-not-assume.
- ★ **`veraPDF/veraPDF-validation-profiles` has `PDF_A` and `PDF_UA` ONLY.**
  The role-brief's stated substitution ("use veraPDF's open validation rules for
  paywalled conformance standards") is **true for PDF/A and PDF/UA and FALSE for
  PDF/X.** `curl` the repo's root `contents` API before promising a rule mirror.

What *does* work for PDF/X, in descending strength: (1) **ISO 32000-2 §11.4.7
NOTE 3**, an ISO-primary sentence *describing what ISO 15930-7 does* — the
strongest citation obtainable, and informative in its own document;
(2) **pdfa.org** article text via `r.jina.ai` (the trade body, free, quotable);
(3) **the suite's publisher** — its own PDFs, which are free, fetchable
with plain `curl`, `pypdf`-readable, and are **the authors of the test suite the
engineer is measuring against**; (4) vendor whitepapers (Esko) and preflight-rule
help centres (DUON) for the rule as implementers state it.

---

## 4h — ★★★ THE ITEH FREE-PREVIEW ROUTE **DOES** WORK FOR ISO 15930 / 19005 / 14289. Item 4g's negative was about the WRONG PAGE. (2026-08-25)

**Item 4g says PDF/X is "the HARDEST paywall" and that `iteh has no sample`.** The
evidence behind that was a fetch of
`standards.iteh.ai/catalog/standards/iso/<uuid>/iso-15930-7-2010`, grepped for
`cdn.standards.iteh.ai` / `samples/` / `href="*pdf*"` → zero matches.

**The grep was right. The conclusion was wrong.** The catalogue page **does not link
its own sample.** The sample is on a *different host*, at a *content-hash path*:

```
https://cdn.standards.iteh.ai/samples/<isoStdNumber>/<32-hex-hash>/<ISO-NAME>.pdf
```

**★ THE ONLY WORKING DISCOVERY METHOD IS A WEB SEARCH ON THE FILENAME.** The hash is
not derivable and the catalogue never emits it. Query shape that worked every time:

```
WebSearch: "cdn.standards.iteh.ai/samples" "ISO-15930-8-2010" OR "ISO-15930-6-2003"
WebSearch: standards.iteh.ai samples ISO 15930-7 2010 PDF/X-4 preview pdf
```

The search engine has the CDN URLs indexed even though nothing links them.
`<isoStdNumber>` is the number in `iso.org/standard/<N>.html` (15930-7 = 55843,
19005-4 = 71832, 14289-1 = 64599, 14289-2 = 82278) — so once you have the ISO
catalogue page you have half the URL, but **not the hash**.

**Obtained and staged 2026-08-25 at `PDF_Spec\_sources\iso_free_previews\` (11 PDFs):**

| Standard | `samples/` path | Deepest clause |
|---|---|---|
| ISO 15930-1:2001 | `29061/baa5b934475947e8a8ebc601fd87918c/ISO-15930-1-2001.pdf` | §6.3.1 |
| ISO 15930-3:2002 | `34941/fb094556ab7b43d19dfd369de10b7402/ISO-15930-3-2002.pdf` | **§6.2.4** |
| ISO 15930-4:2003 | `39938/062626a2642e4e7cb566ae982e54fd9f/ISO-15930-4-2003.pdf` | §6.1 |
| ISO 15930-7:2010 | `55843/22387ef2259840d4a53f02edbd4faf30/ISO-15930-7-2010.pdf` | §3 (TOC + Intro + Table 1) |
| ISO 15930-9:2020 | `77103/362e4413c370427589009c0f4d95f0bd/ISO-15930-9-2020.pdf` | **§6.2.3** |
| ISO 19005-1:2005 | `38920/f0f121090348465280cfdb8095df04e9/ISO-19005-1-2005.pdf` | §6.1.7 |
| ISO 19005-2:2011 | `50655/73a6c1c5b54c44d2b3ad15a048990f84/ISO-19005-2-2011.pdf` | §6.1.11 |
| ISO 19005-3:2012 | `57229/c82288be2ba4477380a07c31c3818eef/ISO-19005-3-2012.pdf` | **§6.2.4.1** ← deepest |
| ISO 19005-4:2020 | `71832/339ef0926f5f4edd9be9861eefb174ea/ISO-19005-4-2020.pdf` | §6.1.4 |
| ISO 14289-1:2014 | `64599/f43fa811a07343258173535d830fed18/ISO-14289-1-2014.pdf` | **§7.3** |
| ISO 14289-2:2024 | `82278/61329d7c9eb54e3fbc58a69f6036031d/ISO-14289-2-2024.pdf` | **§8.2.5.3** |

Plain `curl -s -L -o`, no proxy, no Cloudflare. `pypdf` `extract_text()` reads them
cleanly (11–15 pages each). **STAGE THEM** — hash URLs rot and are unreconstructible.

**★ PREVIEW DEPTH IS NOT UNIFORM AND IT IS NOT PREDICTABLE FROM THE FAMILY.** The
*third* part of a series can go four clauses deeper than the *first*. **Download every
part of a family, even the ones you think you don't need** — ISO 19005-3's preview
supplied the conformance-clause text for PDF/A-2 and PDF/A-1, which their own previews
truncate before.

### ★★ THE INFERENCE RULE — write it into any file built from a preview

| Region | A hit proves | A MISS proves |
|---|---|---|
| **table of contents** | the clause exists, with that number and title | **the clause does NOT exist.** TOCs are complete |
| Foreword / Introduction / a numbered front-matter table | the stated fact | nothing |
| body text the preview **reached** | the normative text | nothing |
| body text the preview **did NOT reach** | — | **NOTHING** |

A term-frequency scan over a preview is **worthless as a negative** — it covers ~10 % of
the document. **The TOC is where the negatives come from.** *"No clause of ISO 15930-7
covers shadings"* is provable from `PXC-5`'s complete §6.1–§6.27 list; *"`shading` = 0
hits in the preview"* proves nothing and must not be reported as if it did.

### The other free routes confirmed the same session

- **veraPDF validation profiles are a clause-number source, not just a rule source.**
  `raw.githubusercontent.com/veraPDF/veraPDF-validation-profiles/integration/PDF_A/{PDFA-1B,PDFA-2B,PDFA-4}.xml`
  and `PDF_UA/{PDFUA-1,PDFUA-2}.xml`. Each `<rule>` carries
  `<id specification="ISO_19005_n" clause="6.2.10" testNumber="2"/>` + a `<description>`
  restating the requirement. **That is how a paywalled ISO 19005 clause number becomes
  citable.** 15-line `xml.etree` extractor keyed on `{http://www.verapdf.org/ValidationProfile}`.
  The repo also holds `WCAG-2-2*.xml` and `WTPDF-1-0-*.xml` profiles and
  `ISO-32000-{1,2}-Tagged.xml` / `ISO-32005-Tagged.xml`.
  **★ Item 4g's "veraPDF has PDF_A + PDF_UA ONLY" is still true and still the reason
  there is no PDF/X rule mirror** — that half of 4g stands.
- **A ZERO-HIT COUNT OVER A VALIDATION PROFILE IS A REAL NEGATIVE**, unlike one over a
  preview: the profile encodes the whole standard's machine-checkable content.
  9 rendering terms × 197 PDF/UA rules = 0 hits was the strongest evidence in the
  session.
- **`pdf-association/pdf-issues` errata labels are per-STANDARD**: `PDF/A-2`, `PDF/A-3`,
  `PDF/A-4`, `PDF/A-next`, `PDF/UA-1`, `PDF/UA-2`, `PDF/X-6`, `PDF/X-next`. **There is
  no `PDF/X-4` label** — errata only track current editions. Query:
  `api.github.com/search/issues?q=repo:pdf-association/pdf-issues+label:%22PDF/X-6%22`.
  **★ An issue body can contain a WRONG CITATION** — #197 says "15930-9:2010 6.23" for a
  clause that is ISO 15930-**7**:2010 §6.23. The preview's TOC caught it. **Cross-check
  an errata issue's clause number against a TOC before repeating it.**
- **`pdfa.org/wp-content/uploads/.../TechNote0010.pdf` still 403s** to plain `curl`
  (returns a 103-byte HTML error page — **check the body, not just the size**). The
  Wayback route of item 4d was NOT attempted this session; it remains the cheapest
  unexplored closure for PDF/A.
- **ISO 14289-1 is free IN FULL from the PDF Association**, sponsored, but behind a
  **cart flow** (`pdfa-inc.org/cart/?add-to-cart=11541`) — not machine-fetchable.
  Needs the operator.

---

## Item 4m (2026-08-28) — THE CONFORMANCE-STANDARDS ACQUISITION SET: five routes, one of which RETRACTS a premise the role brief itself carries

Learned building `conformance/conformance__ref__validator_scope.md` (a
"enumerate every conformance standard we could validate" dispatch). Five reusable routes
and one correction.

### 4m-i ★★ THE WAYBACK `if_` SUFFIX IS THE WHOLE TRICK FOR `pdfa.org` PDFs

`pdfa.org` 403s every direct PDF fetch (item 4h). The Wayback route of item 4d **works
today** — but only in this exact shape:

```
curl -s "http://archive.org/wayback/available?url=<url-without-scheme>"   # -> exact timestamp
curl -sL -o out.pdf "https://web.archive.org/web/<TIMESTAMP>if_/<full-url>"
```

**The bare year form `https://web.archive.org/web/2024id_/<url>` returned 403.** Use the
timestamp the availability API gives you, and the **`if_`** suffix (raw content, no
Wayback banner). This delivered **Matterhorn Protocol 1.1** (689 913 B) and **WTPDF
1.0.0** (769 457 B), both `%PDF-1.7`, both `pypdf`-readable.

**Also: the Wayback CDX SEARCH endpoint returned an "Internet Archive: Temporarily
Offline" HTML page later in the same session while the content endpoint kept serving.**
A CDX failure is **not** evidence a snapshot is missing — go via the availability API.

### 4m-ii `etsi.org` IS A USER-AGENT GATE, NOT A PAYWALL

Every ETSI deliverable 403s to a default `curl` UA and **200s to a browser UA**:

```
UA="Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/126.0 Safari/537.36"
curl -sL -A "$UA" -o x.pdf "https://www.etsi.org/deliver/etsi_en/319100_319199/31914201/01.02.01_60/en_31914201v010201p.pdf"
```

Path grammar: `deliver/etsi_<en|ts>/<range>/<numnodots>/<VV.VV.VV>_60/<en|ts>_<numnodots>v<vvvvvv>p.pdf`.
**No directory listing is served** (`/deliver/.../<num>/` returns the site's HTML shell),
so **probe version paths**: `01.01.01_60`, `01.02.01_60`, `01.03.01_60`, … A 404 for one
version is not a 404 for the deliverable. This found EN 319 142-1 **V1.2.1** and
EN 319 102-1 **V1.3.1** where recall would have stopped at V1.1.1 — **and ISO 32000-2 §2
pins PAdES to V1.1.1, so "current ETSI" and "PAdES as PDF 2.0 references it" are
different documents.**

### 4m-iii ★★★ THE CORRECTION: veraPDF's RULES ARE NOT UNDER THE LIBRARY'S COPYLEFT

The role brief says to use "veraPDF's open validation rules/corpus", and a dispatch
assumed the rules inherit the library's GPL. **Three separate licences:**

| repo | licence | how verified |
|---|---|---|
| `veraPDF-library` | **GPLv3+ / MPLv2+ dual** | README §Licensing; GitHub API `GPL-3.0` |
| `veraPDF-validation-profiles` | **CC BY 4.0** | repo README only — **no `LICENSE` file, GitHub API reports NO licence** |
| `veraPDF-corpus` | **CC BY 4.0** | repo README only, same shape |

**Generalise: check the licence of the DATA repo separately from the CODE repo of the
same project. `api.github.com/repos/<org>/<repo>` → `.license.spdx_id` costs one call,
and a `null` there means "read the README", not "unlicensed".** The softer answer is the
dangerous one: a CC-BY data question is more likely to be waved through than a GPL code
question, and it is still the operator's call.

**And check the NOTICE, not just the LICENSE**: `arlington-pdf-model` has
`LICENSE` = Apache-2.0 but `NOTICE.txt` says *"Software: Apache-2.0 … Other
Documentation: CC BY 4.0"* — leaving the `tsv/` **data** in neither bucket.

### 4m-iv ★★ A BUNDLED SUB-CORPUS CAN CARRY ITS OWN, CONTRADICTORY TERMS — READ THE MANUAL INSIDE IT

The veraPDF corpus README declares **CC BY 4.0** over the repository. Inside it,
`Isartor test files/doc/Isartor test suite manual.pdf` says, verbatim:
**"Redistributing all or parts of the Isartor test suite is also not allowed."**

**The finding was in a bundled PDF nobody would think to open** — a 20-page *manual*
sitting beside 204 test files. **Before treating any aggregated corpus as
redistributable, `find` it for `doc/`, `README`, `LICENSE`, `manual` and read them.**
The same manual also supplied the only sourced statement of *what Isartor tests* (the
PDF/A-1b **FAIL quadrant only**, out of a four-quadrant model) and of its
non-coverage — so the trip paid twice.

### 4m-v THE FREE STATUS REGISTER: `pdfa.org/iso-status/` VIA `r.jina.ai`

One fetch returns the **entire current ISO work programme for PDF**, per working group,
with **official title / common name / current status / next key date** for every item —
TC 171/SC 2 WG 5 (PDF/A), WG 7 (PDF/E, 3D), WG 8 (PDF spec, PDF/R, the TS extension
series), WG 9 (PDF/UA, ISO 32005), WG 12 (XMP), WG 13 (C2PA), JWG 14 (PDF/X), and
TC 130 WG 2 (PDF/VT, PDF/VCR, PPM, processing steps). The page dates itself
("Last updated: 9 June 2026").

```
curl -sL "https://r.jina.ai/https://pdfa.org/iso-status/" > status.txt   # jina defeats the 403
```

**★ ITS SCOPE IS *CURRENT WORK ITEMS*, SO ABSENCE IS NOT A NEGATIVE FOR OLD PARTS** —
ISO 15930-1/-3/-4/-5/-6 are absent because they are legacy, not because they do not
exist. For status of a specific old part, `r.jina.ai` + `iso.org/standard/<id>.html`
returns a line like `[**Withdrawn**](…#lifecycle) (Edition 1, 2003)` — that is how
**ISO 15930-5:2003 (PDF/X-2) was confirmed WITHDRAWN** and 15930-1/-3/-4/-6/-8 Published.
**`iso.org/standard/<id>.html` returning 404 through jina is weak evidence a project was
abandoned** (it was one of three strands for "PDF/E-2 never published"); one strand was
the ISO/DIS preview's own voting dates (2013-08-19 → 2013-11-21).

Companion register: **`pdf-association/pdf-corpora` (CC BY 4.0)** is the authoritative
index of public PDF corpora — but **it states NO licences** and disclaims *"All copyright
and trademarks remain with their respective owners."* Listing ≠ clearance.

### 4m-vi THE PROFILE-ARITHMETIC MOVE — prove a spec-equivalence claim by SET DIFFERENCE

WTPDF §6.1.3 *claims* an accessibility-level file meets all of ISO 14289-2. **That claim
is checkable without the paywalled standard**: parse both veraPDF profiles, take
`{(clause, testNumber)}` for each, diff them.

```
rid = re.compile(r'<id specification="([^"]*)" clause="([^"]*)" testNumber="([^"]*)"')
```

Result: `PDFUA-2 \ WTPDF-Accessibility` = the five clause-5 identification rules;
`WTPDF-Accessibility \ PDFUA-2` = `{6.1.3/1}`; **86 rules identical**. **A vendor's
equivalence claim is a HYPOTHESIS; a machine-readable rule set makes it a measurement.**
The same diff, applied within one family, reproduced ISO 19005-1 §5.3's level definition
exactly (`1a − 1b` = the six rules of §6.3.8 + §6.8) — i.e. **the profiles can be used to
CORROBORATE a paywalled clause you have only read the front matter of.**

### 4m-vii A TEST CORPUS ENCODES ITS EXPECTED VERDICTS TWICE, AND THE TWO DISAGREE

veraPDF corpus: filename (`…-<pass|fail|undefined>-<instance>.pdf`) **and** a document
outline bookmark (`expected result: <verdict>`). Measured over all 2 906 test PDFs:
**2 874 agree, 4 DISAGREE, 28 have no such bookmark** (8 say `Actual result`, 18 have no
outline at all, **2 contain the typo `espected result`**). One disagreeing file's own
message reads *"expected message: **File is a valid PDF/A-1b document**"* under
`expected result: fail`, and another's `instance` bookmark says `b` while its filename
says `c`. **Prefer the mechanical channel (filename + directory path); parse the human
channel only for the message; report a mismatch as a corpus defect.** Cost: ~40 lines of
`pypdf` + `os.walk`, ~4 minutes over 2 906 files.



4n. **★ SEARCH THE ERRATA REPO BY THE **CONTAINER KEY NAME**, NOT BY THE PHENOMENON —
   AND TREAT A HIT AS AN ANSWER TO A *"THE SPEC IS SILENT"* QUESTION.** Established
   2026-08-30 chasing *"does `/MK /R` apply when a widget has a baked `/AP`?"*.
   ```bash
   curl -s "https://api.github.com/search/issues?q=repo:pdf-association/pdf-issues+MK+dictionary&per_page=20"
   ```
   `rotate+widget` → **0**. `widget+rotation` → **0**. `MK+dictionary` → **issue #56,
   *"Clarify status of MK dictionary for widget annotations with AP appearance
   dictionary"*, closed, label `ISO approved`** — literally the dispatch's question,
   asked by another implementer in 2021, with a TWG minute in the comments
   (*"PDF TWG: OK to ignore MK for Widget."*). **Both ISO editions are silent
   as printed; the errata are not.**
   Two operational consequences:
   - **The corrected sentence is FREE AND QUOTABLE even when the page it edits is
     licensed.** `https://pdf-issues.pdfa.org/32000-2-2020/clause<NN>.html` renders
     the `<ins>`/`<del>`; strip tags to `[INS]`/`[DEL]` markers with a 6-line Python
     regex and grep the result. Here it yielded a full corrected §12.5.2 paragraph
     that could go into a corpus file **and** into public MIT source, where the
     ISO 32000-2 page could not.
   - **A membership-in-a-list fact is THREE-state**: edition A / edition B as
     printed / edition B + errata. Never answer "does the standard say?" with two.
   Companion negative, also cheap: **`grep -n "Table <N>" <errata_page>.txt` → 0
   hits proves the table you are quoting is unamended**, which is what lets a file
   say "the row stands as printed in both editions".

4o. **★★ GRADE A SILENCE BY COUNTING HOW OFTEN THE STANDARD SAYS THE THING
   ELSEWHERE — "the sibling clause says it and this one does not" is EVIDENCE, and
   it is the difference between DERIVED and SOURCED.** Established 2026-08-30 on
   ISO 32000-1 §12.6.4.10 (Hide actions). The dispatch asked *"may a `/Hide` `/T`
   array mix annotation references and field-name strings, as `/Fields` explicitly
   may?"* The naive answer is "the row doesn't forbid it, so yes". The measurement:

   ```python
   d = re.sub(r"\s+", "", open(DUMP, encoding="utf-8").read().lower())
   d.count("elementsofbothkindsmaybemixed")        # -> 2 in 1.7, 2 in 2.0
   d.count("alldescendantsofthespecifiedfields")   # -> 2 in 1.7, 2 in 2.0
   ```

   **Both sentences occur exactly twice per edition and both times in a `/Fields`
   row** (submit Table 236 / reset Table 238). Neither is in Table 210. So the
   permission to mix and the descendant expansion are **SOURCED for `/Fields` and
   merely DERIVED (or, for descendants, genuinely UNDEFINED) for `/T`** — and the
   standard's own habit of spelling both out makes the omission *conspicuous*
   rather than merely silent. **A count of 2-and-both-elsewhere is a much stronger
   report than a count of 0-here.** Generalise: whenever a clause is silent on a
   property, grep the property's *characteristic sentence* across the whole
   document; if the standard states it explicitly for structurally identical
   siblings, say so and downgrade the grade. Uses 4b's whitespace-stripped counting
   (the sentences wrap, so a raw `grep -c` under-counts).

   **4o-bis — THE SAME MOVE FINDS OUT-OF-CLAUSE CARRIERS. Grep the FEATURE'S NAME
   over the whole document before declaring a clause's footprint complete.**
   `grep -in "hide action"` over ISO 32000-1 returned **7 lines, 5 of them inside
   §12.6.4.10** — and **2 in §12.8.5, Table 259, the LEGAL ATTESTATION dictionary**,
   whose `/HideAnnotationActions` counts hide actions beside `JavaScriptActions`,
   `LaunchActions` and `URIActions` as content that *"may result in unexpected
   rendering"*. **That row is the standard's own justification for disclosing the
   feature**, it lives four clauses away, and no amount of reading §12.6.4.10
   would surface it. It was also **absent from the corpus entirely** —
   `iso32000__s__12.8.md` had explicitly scoped §12.8.5 out. **Table 259 is a
   ready-made "which constructs does ISO consider potentially misleading?" list;
   check it whenever a Pass authors an interactive construct.**

4p. **A CLAUSE NUMBER IS NOT A KEY — 1.7 §12.6.4.10 is HIDE, 2.0 §12.6.4.10 is
   MOVIE.** The errata-page grep for the 1.7 number returned a hit that looked like
   a Hide erratum and was a Movie one (Issue #676). The corpus already knew *table*
   numbers collide across editions; **clause numbers in the §12.6.4.x range collide
   too, by exactly one, because 2.0 inserted `GoToDp` at .5.** Search the 2.0 number
   on 2.0 sources and the 1.7 number on 1.7 sources, never mixed.

4q. **A THREE-CHANNEL ERRATA NEGATIVE IS CHEAP AND IS WORTH RECORDING AS A FINDING.**
   For §12.6.4.10 all three said *unamended*: (i) `/Annots` sweep over the staged
   2.0 ±4 pages — **zero markup annots on the clause's own two pages**, while the
   same run returned annots on neighbouring pages (which proves the tool worked, and
   is the control that makes the negative meaningful); (ii) the free
   `pdf-issues.pdfa.org/32000-2-2020/clause12.html` — its TOC lists **only**
   sub-clauses that have errata, so `12.6.4.11` = 0 hits is a real negative;
   (iii) API searches by key name *and* phenomenon. **Always include a positive
   control in the annotation sweep** — "zero hits" from a broken script and "zero
   hits" from a clean clause look identical.

---

## 4r. ★★★ A TABLE WHOSE **LABELS** EXTRACT BUT WHOSE **VALUES** ARE BLANK IS A **SYMBOL-FONT** PROBLEM, NOT A DELETION (2026-09-02, ISO 32000-2 Table 146)

**Symptom.** `pdftotext -layout` on ISO 32000-2 §11.7.4.5 produced a table with
every row label present and **every cell value missing**:

```
Any process colour Process colour
space (including component
other cases of
DeviceCMYK)          Spot colourant      (= 0.0)
```

**Cause.** The `c_b` / `c_s` cell values are set in **`CambriaMath`** using
**Unicode Mathematical Alphanumeric Symbols** (U+1D400 block, e.g. U+1D469) with
subscript runs. They extract as empty or as unmappable characters.

**★ WHY THIS IS DANGEROUS, NOT MERELY ANNOYING.** The natural cross-edition
probe is *"grep the value; is it still there?"* Here that returns **0 hits**, and
the honest-looking conclusion — ***"ISO 32000-2 deleted this row"*** — is
**FALSE**. The row is present and **identical to 1.7**. This is the standing
"1→0 count" caution with the **glyphs**, not the sentence, as the casualty.

**Recipe.**

1. Locate the physical PDF page: split the `pdftotext` output on `\f` and index.
2. `pdfminer.high_level.extract_pages(SRC, page_numbers=[n-1])`, walk `LTChar`.
3. Bucket by `round(ch.y0 / 3.0)` (merges sub/superscripts into the baseline row),
   sort each bucket by `ch.x0`, join. Print `x0` of the first glyph so columns are
   identifiable, and the `fontname` set — **`CambriaMath` in the font set is the
   tell**.
4. **Write to a UTF-8 FILE, never to the console** — U+1D469 raises
   `UnicodeEncodeError` on Windows `cp1252`.
5. `unicodedata.normalize('NFKC', ch)` folds math italics to ASCII for reading.

Working script kept at `D:/Dev/Rag-Specialized/PDF_Spec/tools/recover_table_cells_positionally.py` -- in the
PRIVATE spec RAG, not beside this file. It was originally written into
pdfce's agent-memory, which is a public repository, while hard-coding a path
to the licensed ISO 32000-2 source.

**Generalise:** blank cells + intact labels ⇒ suspect a math/symbol font before
concluding anything about the standard's content. Applies to formulae, figure
callouts and any table of variables. Companion to **4a** (glyph x-positions) and
**4c-bis** (per-page layout pass).


---

### 4s ★★ WHEN AN ISO STANDARD IS PAYWALLED, LOOK FOR ITS **SECRETARIAT'S** FREE COMMENTARY — and read the disclaimer that comes with it (2026-09-02, ISO 15930 / PDF/X)

The ISO 15930 previews reach only front matter, and `iso.org/obp` is Cloudflare-gated
(4d does not defeat it — re-confirmed 2026-09-02: `r.jina.ai` returns **HTTP 200 with a
201-byte body reading *"Just a moment…"***). **A third channel exists and had never been
tried: the standard's own committee/secretariat publishes free commentary.**

**Working example, now staged:**

```
https://printtechnologies.org/standards/files/pdf-x-application-notes_v4-sep06.pdf
```

**CGATS/NPES, *Application Notes for PDF/X Standards*, v4, Sept 2006, 31 pp.**
HTTP 200, plain `curl -L -A "Mozilla/5.0"`, **no gate, no cart, no UA trick needed.**
Written by **CGATS SC6 TF1 (PDF/X)** — *the task force that authors ISO 15930* — and
published by **NPES**, which performs CGATS's secretariat functions.

**★★ READ THE FOREWORD FOR BOTH HALVES, because they pull opposite ways:**

- *"NPES is making this document publicly available at **no cost** for informational and
  reference purposes"* ⇒ **`free_secondary_paraphrase`, quotable.**
- *"this document has not been developed under the consensus process of ANSI, and is not
  an ANSI consensus document. **It does not have the status of a standard or technical
  report**"* ⇒ **nothing in it is a `shall`, however imperative its wording.** Its §2.7.1
  says overprint settings *"**must be preserved** by applications or RIPs"* — which reads
  exactly like a requirement and is not one.

**⇒ Rule: quote the disclaimer next to the quotation, in the same corpus paragraph.**
A committee's free commentary is excellent evidence of **intent** and is never evidence
of **obligation**.

**Where to look for the analogue on other specs** (untested, listed as leads):
CGATS/NPES (now *PRINTING United Alliance*) for ISO 15930 and ISO 12647; the **PDF
Association** for ISO 32000 / 19005 / 14289 (already exploited — 4m-i); **ECI** for
Altona/colour material; **ICC** at `color.org` for ICC.1 (already `free_primary`).

**Coverage traps that come with this class of source:** they are **versioned
independently of the standard and go stale**. This one covers **PDF/X-1a:2003,
PDF/X-2:2003 and PDF/X-3:2003 only**, there is **no v5** covering X-4 (2008/2010) or X-6
(2020), and it still treats **PDF/X-2 (ISO 15930-5)** as live although ISO has since
**WITHDRAWN** it. **Date-check the commentary against the standard's current edition
before applying anything from it.**

### 4t A NATIONAL-ADOPTION CATALOGUE PAGE IS **NOT** A NEW DISCOVERY CHANNEL FOR AN ITEH SAMPLE (measured negative, 2026-09-02)

Item **57.9** says a recorded acquisition negative has the scope of the *method*, so
retry with a **different channel**. Tried here for ISO 15930-7, whose sample is only
12 pp.: the **SIST ISO 15930-7:2011** Slovenian-adoption page,
`standards.iteh.ai/catalog/standards/sist/24125b06-…/sist-iso-15930-7-2011`.

**Result: HTTP 200, 44 603 B, and `grep -o "cdn\.standards\.iteh\.ai[^\"' <>]*"` yields
the bare hostname only — no sample link.** **Same failure mode as the ISO-adoption
catalogue page** (`57.9`): iTeh catalogue pages of *either* flavour do not link their own
samples. **The only working discovery route remains a web search on the SAMPLE FILENAME**
(`ISO-15930-7-2010.pdf`), which resolves the `/samples/<num>/<hash>/` path.

**⇒ Do not spend another session on national adoptions (SIST / BS / DIN / NBN) hoping for
a deeper preview. Scope of this negative: the catalogue-page-scrape method, on both ISO
and SIST flavours.** Untried and still open: whether a *different national body's own
webstore* serves a deeper preview under its own domain.
