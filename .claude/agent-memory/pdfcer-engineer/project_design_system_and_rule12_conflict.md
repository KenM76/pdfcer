---
name: design-system-and-rule12-conflict
description: UI_PREFERENCES.md lives at the REPO ROOT not docs/ (a path error twice mistaken for a missing file); the governing rule is chrome-theme-aware vs canvas-overlay-theme-INVARIANT, enforced by theme.rs + check-theme-colors.sh; and Ken's design handoff conflicts with CLAUDE.md rule 12
metadata:
  type: project
---

**Why:** the design work has one counter-intuitive rule that a future engineer
will "fix" into a bug, and one live conflict that is Ken's to settle and must
not be routed around.

## ★ THE PATH — get this right, it has been got wrong twice

**`D:\Dev\pdfce\UI_PREFERENCES.md` — REPO ROOT, not `docs/`.** Verified
2026-08-11: 34 KB, git-tracked, first committed in `b0f57af`. It exists.
`ARCHITECTURE.md` §9 has had the correct path since 2026-08-06.

**Why this warning is here.** On 2026-08-11 both `pdfce-ui-specialist` and
then *this engineer* independently concluded the file was **missing** — the
specialist by globbing, the engineer by running `ls docs/UI_PREFERENCES.md`
and `git log --all -- docs/UI_PREFERENCES.md` and getting nothing. That
`--all` is the trap: it *feels* exhaustive, but it is still **path-scoped**,
so a wrong path returns empty with exactly the confidence of a true negative.
The engineer then wrote "never existed in git history" into this memory as a
measured fact and reported it to the operator.

The lesson is not "check twice." It is: **a negative result from a
path-scoped query is a fact about the path, not about the repository.** Use
`git log --all -- '*UI_PREFERENCES.md'`, or `git ls-files | grep`, before
concluding a tracked file is absent.

Several documents cite the stale `docs/` path, which is what makes the wrong
conclusion so easy to reach. `pdfce-librarian` corrected `ROADMAP.md` and
footer-corrected `SESSION_LOG.md` on 2026-08-11. **Still stale, flagged and
unfixed:** `docs/ui_specs/ribbon-groupings-and-customization-architecture.md`
line ~39.

## The counter-intuitive rule

`UI_PREFERENCES.md` §1 (2026-08-05, pdfce-ui-specialist) states it; Pass
58.0's `crates/pdfce-gui/src/theme.rs` + the `tools/check-theme-colors.sh`
CI gate now *enforce* it. Both are authoritative and they agree — prefer the
gate when you need a mechanical answer, the document when you need the why:

- **Chrome** (panels, tabs, buttons, text, separators) is **theme-aware** —
  must route through `ui.visuals()`, never a bare `Color32`.
- **Canvas overlay** (node marks, ce-dimension outlines, live previews,
  redaction ink) is **theme-INVARIANT by design** — a bare named `Color32` is
  the RIGHT mechanism. Overlays draw on the rendered PDF page, which is
  near-white whatever the app chrome is set to. Making them `ui.visuals()`-aware
  would make them vanish against a white page under a dark chrome theme.

So "audit all hard-coded colours for theme-awareness" is right for the first
domain and **a regression if applied to the second**. 25 literals measured;
most are duplicates to collapse onto 8 named overlay tokens, not bugs.

One real drift flagged, unresolved: `SUBPATH_OUTLINE_COLOR` (210,140,40) claims
kinship in its own doc comment with the preview-orange family (210,90,40) but
is numerically different. Needs a screenshot-verified decision, not a silent
merge — it was deliberately tuned in Pass 36.3.

## The conflict — Ken's call, do not route around it

Ken's design handoff (`…\D--Dev-KenAgent\…\scratchpad\pdfce_gui_design_handoff.md`)
recommends auditing **Acrobat Pro's ribbon/panel/GUI structure**, via
`pdfce-acrobat-librarian`.

**CLAUDE.md rule 12 forbids exactly that.** That RAG catalogs
capability/behaviour/limits ONLY and "must never describe or inform copying
Acrobat's GUI structure (menu paths, panels, dialogs)". The agent definition
says the same.

Dispatching it for GUI structure would require Ken to amend rule 12. Filed as
an open operator question. The underlying goal — differing from Acrobat
deliberately rather than accidentally — may be reachable without that RAG.

## Also unanswered by Ken

Ribbon specificity (how close to Acrobat's layout vs pdfce's own — a
product-identity call), and font-asset bundling/licensing (pdfce-gui installs
ZERO custom fonts today; rule 13 would apply to a bundled font file even though
`cargo-about` will not catch it).

## Handoff-doc caveat worth not re-deriving

It is a philosophy/process document, not a spec — it says nothing about panels
or tool options. And several mechanisms are web-only (CSS custom properties,
`@media (prefers-color-scheme)`, `data-theme`, `@font-face` data URIs). The
principles transfer; the mechanisms needed egui translations, which
UI_PREFERENCES.md §2 records one by one.

See [[rung-ladder-state]] for the Pass that produced the two newest overlay
literals (`NODE_MARK_COLOR`/`NODE_MARK_FILL`) and why they exist for a
correctness reason rather than a decorative one.
