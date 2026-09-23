# Memory index — pdfcer-spec-librarian

- [Spec source extraction toolchain](reference_spec_source_extraction.md) — how to GET a spec and get text out of it: 21 routes (4a–4u), verified free URLs, paywall workarounds, errata recipes.
- [PDF_Spec corpus conventions + dispatch-shape playbook](project_corpus_state.md) — 81 items, one per past dispatch. **Find the item matching your dispatch's SHAPE and read it before working.**
- [Font + spec-data licensing patterns](project_embeddable_data_licensing.md) — what may cross into pdfcer's MIT tree; data-vs-document, availability ≠ redistribution licence.

## Routing — find your dispatch's shape, then READ THE NAMED ITEM (the detail is there, not here)

| Dispatch shape | Read |
|---|---|
| get me spec S / a URL 403s / text extracts wrong | extraction **4a–4u**; esp. **4d** `r.jina.ai`, **4h** iTeh previews, **4m-i** Wayback `if_/`, **4s** secretariat commentary, **4t** national-adoption pages |
| a table extracts misaligned / values blank / a figure looks empty | extraction **4i**, **4c-bis**, **4r** (symbol font, NOT a deletion), **4a**/**4c**/**4f** |
| a phrase or term COUNT is going into a file | extraction **4b** — whitespace-stripped; a raw `grep -c` on a multi-word phrase is a LOWER BOUND |
| is this amended? is there an erratum? | extraction **4j**/**4k**/**4n**; corpus **63d** (three channels + a positive control) |
| **"pdfcer must IMPLEMENT the rule, not just classify it — is the geometry/arithmetic defined?"** | corpus **79** |
| **"what makes the OTHER implementation right? — here are its pixels"** | corpus **66** + **67** |
| **"I ran your falsifications and they all failed"** | corpus **67** |
| **"which of these two readings is right? — I narrowed it to one clause"** | corpus **64** |
| **"should the CONFORMANCE PRESET pin this axis?" / "does standard S constrain X?"** | corpus **68** |
| **"verify or REFUTE each premise" / "I am about to add a verb that mutates array A"** | corpus **69** |
| **"ingest the clause my corpus only SUMMARISED" / a downstream project filed it** | corpus **72** |
| **"here is a MEASUREMENT that contradicts one sentence — correct it"** | corpus **71** |
| **"pdfcer REFUSES a file Acrobat opens — is clause C a `shall` or a `should`?"** | corpus **74** |
| **"is my STANDING RULE applied outside its territory?" / "the other reader draws it, we render nothing"** | corpus **76** |
| **"a REQUIRED key is missing and every reader opens the file" / "is requirement A conditioned on key B?"** | corpus **77** |
| **"a REDACTED object SURVIVED" / "is carrier X unique?" / "may a writer DROP an unreferenced object?"** | corpus **75** |
| **"is key K constrained to a VOCABULARY?" / "I am about to write a literal token into files"** | corpus **78** |
| **"this ONE value is wrong — now check the whole column"** | corpus **73** |
| **"is clause C advisory or mandatory?"** | corpus **65** |
| **"verify a SHIPPED CITATION — a third party couldn't source our cited claim"** / an erratum's real number, text, status, date | corpus **80** + extraction **4v**/**4w** |
| **a NON-PDF format (EMF, SVG, …) dispatched "into PDF_Spec"** / "does format F embed fonts?" | corpus **81** |
| "close the exclusion banner" / "ingest C AND give me the step list" | corpus **70** + **58** |
| "is the base standard silent on X?" and a later edition fixes it | corpus **70e** — check whether a PROFILE of the base edition also fixes it |
| a PDF/UA or PDF/A conformance question | corpus **69e** (answer PER PART — a later part can WIDEN a rule) + **69f** (free-quotation route) |
| "does feature F apply inside context C?" | corpus **64d** |
| "the spec is silent on X" | corpus **62c**, **63a**, **63b** |
| "where can X be set / which carrier wins?" | corpus **65d**, **65e** |
| "enumerate every X" / build a census | corpus **59**, **63e** |
| verify or refute the dispatch's own premise (incl. claims about pdfcer's code) | corpus **62**, **55**, **61f** |
| ingest clause C in full before a Pass is written from it | corpus **63**, **58** |
| licensing: may this text/data cross into pdfcer or a public repo? | `project_embeddable_data_licensing.md`; corpus **45**, **60**; extraction **4m-iii**/**4m-iv** |
| a corpus file and another document disagree | corpus **50** — the corpus's own one-line compression is usually the ancestor |
| filing mechanics / `index.md` upkeep | corpus **61h**, **63j**, **73e**, **79k** |

## Standing cautions — headlines only; the evidence is in the cited item

**Mechanics**

- **`Write` the patch script, then run it** — Bash heredocs break on spec punctuation at file size.
- **~22% of corpus files are CRLF and the set GROWS. Never rely on a list** — detect per file (`b'\r\n' in raw`), round-trip, and check the diff hunk count after. Corpus **71f**, **72g**, **73e**.
- **`assert s.count(old) == 1` before every `str.replace`.** Corpus **70j**.
- **Re-print the heading map after inserting a heading inside a clause.** Corpus **71f**.
- **A staged SOURCE costs TWO registrations** (`LEGAL_NOTE.md` + `index.md`); a new licence tier costs three. Corpus **68h**, **57.10**.
- **Strip HTML tags before grepping an errata page, and run a positive control.** Corpus **69i**.
- **Recount `ls <subdir>/<prefix>*.md | wc -l` before touching an index count cell. Run every search recipe you add.**

**Reasoning traps**

- **★★ NEVER HAND-COMPUTE A DERIVED VALUE INTO A FILE — three occurrences now.** "Cite your source" does not protect it; the source is silent on that axis. Label derived columns and state their invariant. Corpus **73a**, **78f**.
- **Bit-numbering is per SOURCE**: ISO 32000 = 1-based (`2^(N-1)`); OpenType = 0-based. Corpus **73b**.
- **★★ A `shall` BINDS A PARTY — ask WHO before HOW STRONG.** ISO 32000-1 §2.1/§2.3 bind the FILE and WRITER; §2.2 scopes the READER's duty to *conforming files*; clause 1 excludes conformance validation. `shall not` ⇏ a reader must refuse. Corpus **74a**/**74b**/**74d**.
- **★ "the CLAUSE excludes/states nothing" ⇏ "the STANDARD does".** Corpus **79e**, **76e**.
- **★ "X shall be determined by entry E" is NOT silence about X** — read E's table row, including its `Default value:`. Corpus **79b**.
- **A 1→0 phrase count proves the SENTENCE was deleted, never that the RULE was.** Corpus **65g**, **66f**, **70b**, **79d**.
- **A NOTE THAT PRESUPPOSES A PRACTICE IS EVIDENCE FOR IT**; and SCOPE the fallback sentence a project leans on. Corpus **76e**/**76f**.
- **A critical NOTE is not a prohibition.** Corpus **67e**.
- **"UNREFERENCED" IS NOT A PDF CONCEPT** — 0 hits both editions; Annex F.3.3 `shall`-REQUIRES an unreferenced object. Corpus **75c**.
- **`X is required` ⇏ `a non-X encoding is rejected`** when the consumer does not validate canonicity — and the leniency is often SAFER. Corpus **71a**/**71b**.
- **Modality is per-SUBTYPE, per-EDITION and per-BULLET.** Corpus **76b**, **79f**.
- **A clause number is not a key across editions; neither is a table number.** Put the map in the file's §0. Corpus **63c**, **52**, **62h**, **70i**; range-dependent shift **65f**.
- **A one-way cross-reference is a DEFECT, not a contradiction** — check for the three enumeration formulas before calling it one. Corpus **79g**, **69c**.
- **A test suite's expected results measure a DEVICE, not a rule** (check its LICENCE); a search-engine summary of a paywalled standard is a LEAD; a committee's free "application notes" can disclaim being normative. Corpus **68f**, **68g**, **68i**.
- **An OPEN `pdf-issues` issue, and a `wontfix` CLOSED one, both beat a self-measured silence.** Search by KEY NAME and by the dispatch's QUESTION (`in:title`). Corpus **70g**, **77a**. **An OPEN issue that QUOTES an erratum is evidence FOR it — read its KEY LIST, not its title.** Corpus **80e**.
- **★★ ONE PARAGRAPH CARRIES EDITS FROM SEVERAL ERRATA — attribute per edit (`data-issue` in the RAW HTML), never by paragraph.** A corpus one-liner saying "the same edit" seeded a wrong issue number in three downstream docs. And **"ISO approved" ≠ published**; **the odd-looking element of a citation is not the unsourced one.** Corpus **80a**/**80b**/**80d**; extraction **4v**/**4w**.
- **A corpus sentence about what pdfcer IS is a DATED MEASUREMENT** — strike through with a dated correction, never delete; and re-measure the WHOLE "corrections owed" section, not only the rows you were told about. Corpus **67d**, **79j**.
- **A shipped SETTING has a scope**; a register entry citing two tables jointly is a scope smell. Corpus **64e**, **61e**, **54**.
- **A scope-exclusion banner is an untested claim about material its author deliberately did not read.** Corpus **58**.
- **A "no free route exists" negative has a scope: the METHOD you tried.** Corpus **57**.
- **A clause's EXAMPLEs are normative-adjacent** — grep the discriminating TOKEN. Corpus **64a**/**64b**.
- **The ASN.1 layer is a corpus dependency and it is FREE** — ITU-T X.690 staged; §11.6 is the `SET OF` order rule, §10.3 is a DECOY. Extraction **4u**; corpus **71c**.

**Filing judgment**

- **A re-dispatch of an answered question is worth real work if it carries a new measurement — PROMOTE the existing entry, don't write a parallel one. When the OUTCOME survives but the REASONING dies, AMEND, and splice the redirect into the TOP of the superseded section.** Corpus **66b**, **66g**, **67g**.
- **Audit blockquote-block LENGTH mechanically before filing a `spec: multi` file.** Corpus **70f**.
- **★ THE FLAT-GREP CHECK HAS FAILED THREE TIMES (72g, 76j, 79k).** Standard fix: end every file with a `## FLAT-GREPPABLE QUOTATION INDEX` of unmarked copies, and run the check BEFORE filing. Two causes: inline `**` inside a quoted sentence, and `pdftotext -layout` interleaving a table's KEY column into the VALUE column — **verify a miss on HALF the sentence before doubting the quotation.**
- **★ THIS MEMORY DIRECTORY IS INSIDE THE PUBLIC `KenM76/pdfcer` REPO.** `licensed_primary_private_rag` (ISO 32000-2) and `free_iso_preview_primary` (iTeh) are **clause-reference-and-paraphrase only here**; the verbatim wording stays in `D:\Dev\Rag-Specialized\PDF_Spec\`. `free_primary` (ISO 32000-1, ITU-T, ETSI, W3C, Adobe) and `free_secondary_paraphrase` may be quoted. **Items 57.2/57.3 predate this note and carry short ISO 15930 preview quotations — scrub on the next maintenance pass.** Corpus **66c**, **68**.
- **The crate rename `pdfce-core` → `pdfcer-core` has NOT propagated** through older files' `pdfcer_relevance:` frontmatter. Its own dispatch. Corpus **79j**.
