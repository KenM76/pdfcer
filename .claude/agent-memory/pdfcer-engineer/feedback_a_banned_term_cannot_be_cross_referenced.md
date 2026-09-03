---
name: a-banned-term-cannot-be-cross-referenced
description: A term an operator ruling bans from the repo is invisible to researchers AND uncheckable against our own assets — it produced a licence violation and a wrong research conclusion from one fact, on 2026-08-28
metadata:
  type: feedback
---

**A dispatch that does not name a ban cannot transmit it, and a term that may
not be written is a fact that cannot be cross-referenced.** Both halves fired
from a single fact on 2026-08-28.

**Why:** the operator's 2026-08-25 ruling (`ROADMAP.md` open question `(bt)`,
enforced by `tools/check-suite-name-absent.py`) keeps the licensed
print-conformance suite's name out of this **public** repository — contents
and filenames, base64-encoded needles so the gate is not its own first
violation. On 2026-08-28 `pdfce-spec-librarian` researched conformance test
corpora, found that suite listed in the PDF Association's **public** index as
an ordinary PDF/X corpus, and reported it by name. The engineer pasted the
phrase into a librarian dispatch. It landed in `docs/ROADMAP.md` twice and in
`docs/NEXT_SESSION.md` once. **`tools/run-gates.sh` caught it before the push
— nothing else would have.**

**Two failures, one cause, and the second is the expensive one:**

1. **Licence/ruling violation.** The researcher never read the ruling; nothing
   in the dispatch named it. Same class as *a subagent cannot check a
   constraint the dispatch omits* — but sharper, because the term looked
   completely ordinary in its source context.
2. **★ A WRONG RESEARCH CONCLUSION, caused BY the ban.** The same research
   concluded PDF/X was **corpus-blocked** — no test corpus available. False:
   that suite **is** pdfce's own PDF/X corpus, on disk at
   `D:\Dev\pdfce-private\suite\`, patches labelled `…_x1a.pdf` / `…_x3.pdf`,
   already driven by the render-parity harness. **Because the name may not be
   written here, no document in this repo could say "we already have this"** —
   so an outside researcher correctly concluded from the visible record that it
   was unavailable. The ban did not merely hide a string; it hid a **capability
   pdfce owns**, and the scrub gate was the only thing that surfaced the
   contradiction.

**How to apply:**

- **Name the ban in the dispatch** whenever dispatching research or filing that
  could touch conformance corpora, print/prepress, render parity, or fixtures.
  Say *"the print-conformance suite"* — the established masked phrasing used
  in `ARCHITECTURE.md`, `FEATURES.md` and `ROADMAP.md` — and say explicitly
  that the real name must not be written. Do not spell it out in the dispatch
  either; a dispatch is quoted into documents.
- **Before accepting any "we don't have X" conclusion about test data, check
  the private map** (`D:\Dev\pdfce-private\suite\manifest.json`). Absence from
  the public record is not absence from the machine, and this repo is
  *structurally incapable* of recording that particular asset.
- **Separate "cannot be committed" from "cannot be reached."** They are
  different claims and collapsing them loses a real capability.
- **A private licensed corpus is still a weak oracle if it is all-conforming**
  — it detects a validator that wrongly *fails* a good file, never one that
  wrongly *passes* a bad one. Say which kind of oracle it is.
- **Run `bash tools/run-gates.sh`, never a hand-picked subset.** The scrub gate
  is public-facing and is the one that catches this; see
  [[a-gate-sweep-certifies-the-tree-it-ran-on]].

Related: [[read-architecture-every-session]],
[[a-claim-about-callers-is-a-measurement]],
[[an-unticked-box-is-unfalsifiable]].

**2026-09-02 — a third leak surface: DISPATCH TEXT.** I named the operator's
OneDrive output folder verbatim in a librarian dispatch; its last path
component contains the licensed suite's name; the librarian filed it
faithfully into ROADMAP.md and SESSION_LOG.md, and only the release-tree
gate sweep caught it (before the push). A commit message, an untracked file
and now a subagent prompt: anything a documentation agent will copy into a
tracked file is a publication surface. Describe the folder ("the operator's
OneDrive `pdfTests` output folder"), never spell its path.
