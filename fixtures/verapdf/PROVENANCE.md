# `fixtures/verapdf/` — files taken verbatim from the veraPDF corpus

Governed by `docs/LEGAL.md` §5, which names **"veraPDF's open corpus"** as an
approved source (b) for fixtures checked into `fixtures/`.

## ★★ This directory is the FIRST category-(b) tracked fixture, and that is worth knowing

Everything under `fixtures/synthetic/` is category (a) — authored here, or
**modelled** on a corpus file and reduced to the one thing that matters
(`xref-recover/PROVENANCE.md`: *"models qpdf's `bad6.pdf`, reduced to the one
thing that matters"*). That practice is better where it is achievable: smaller
files, self-documenting, no second licence in the tree.

It is not achievable here. See the entry below for why.

## ★ Licence — CC BY 4.0, and this repository is MIT

The veraPDF corpus is licensed **Creative Commons Attribution 4.0
International (CC BY 4.0)** — stated in the corpus's own `README.md`.
Redistribution is permitted **with attribution**, which this file is.

⇒ **A file here is not MIT.** It is CC BY 4.0, it is a test fixture, and it is
not part of any shipped artifact — `tools/package-portable.py` ships no
fixtures. A reader auditing this repository's licensing should know that
`fixtures/verapdf/` is the one place a non-MIT file lives, and why.

**Attribution:** veraPDF test corpus, © the veraPDF Consortium, CC BY 4.0.
Source: <https://github.com/veraPDF/veraPDF-corpus>.

---

## `object-streams.pdf`

**Taken from:** `PDF_A-2b/6.1 File structure/6.1.12 Permissions/veraPDF test
suite 6-1-12-t02-pass-a.pdf`, unmodified, 11,516 bytes.
**Added:** 2026-09-12.

**Why this file, and why not a synthetic one.** It is here for exactly one
property — **it uses object streams**. `pdfcer inspect` reports PDF 1.6 and
`export-structure` reports `objects=30 objstm_expanded=5`, which is the
predicate the tests need (`structure::layout(&doc).object_streams` non-empty).

★★★ **A synthetic equivalent cannot be produced by this project's own tooling,
which is the whole reason a corpus file is used here.** Measured 2026-09-12:

* **pdfcer's writer only ever DEcompresses.** Every `/Type /ObjStm` is promoted
  to file level on save (`writer/save.rs:1018`), so a document cannot be
  round-tripped through this crate to acquire one.
* **No fixture under `fixtures/synthetic/` contains one** — every `*.pdf` was
  checked for `/Type /ObjStm`; zero hits.
  `xref-recover/xref-stream-corrupt.pdf` mentions it and is deliberately
  damaged, so it is not a substitute.
* Authoring one by hand means an `/ObjStm` with `/N`, `/First` and its pair
  table **plus** a cross-reference **stream** carrying type-2 entries —
  exacting enough that a subtly wrong fixture would make its tests pass for the
  wrong reason, which is worse than the silent `SKIP` it replaces.

**What it replaced.** `editable_roundtrip.rs` (2 tests) and
`structure_inspect.rs` (1) sourced this predicate from
`fixtures/external/qpdf/…/big-ostream.pdf` — a corpus that is untracked, not
fetched by `fetch-corpora.sh`, and not licence-cleared. Those three tests
printed `SKIP` and **passed**, since they were written. They run now.

★ **Chosen as a `pass-a` file deliberately.** The veraPDF corpus is largely
*conformance* files, many of which are deliberately malformed; a `fail-`
variant would have made these tests measure damage rather than object streams.
Of the 78 corpus files containing an `/ObjStm`, this is the smallest valid one.

**Not used for its own subject.** The file's own purpose in the corpus is a
PDF/A-2b permissions test. Nothing here tests permissions; the directory it
came from is recorded only so the file can be located again upstream.
