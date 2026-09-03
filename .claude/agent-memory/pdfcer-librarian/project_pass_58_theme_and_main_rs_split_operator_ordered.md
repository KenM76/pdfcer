---
name: project-pass-58-theme-and-main-rs-split-operator-ordered
description: 2026-08-10 — operator-initiated (not roadmap) work shipped as Pass 58.0 (theme.rs, three presets) + Pass 58.1 (main.rs split); operator set a 3-step work order himself; theme-preset default is an open question.
metadata:
  type: project
---

**What happened (2026-08-10, eightieth filing).** Ken asked the engineer
directly whether the GUI's look would ever improve and whether the
codebase was modular enough to restyle safely. The engineer investigated
and reported honestly: strings and icons were already centralized+gated
(`ui_text.rs`/`check-ui-strings.sh`, `icons.rs`), but colour was not —
26 scattered `Color32` literals, no `egui::Style` call anywhere, inside
a 27,647-line `main.rs`. **Ken then chose the work order himself**:
theme extraction first (`Pass 58.0`, `2387a58`), then the `main.rs` split
(`Pass 58.1`, `255cf86`→`3a699cf`→`fc137e2`, 27,647→25,511 lines into
`canvas_overlay.rs`/`panels_structure.rs`/`ribbon_ui.rs`), then back to
roadmap feature work. Visual direction was deliberately deferred —
"show me options first" — which is why `Pass 58.0` ships three presets
(Quiet/Airy/Dark) with **no redesign**, not a chosen new look.

**Why this is worth a project memory, not just the ROADMAP/SESSION_LOG
filing.** The pattern (ask honestly when asked a direct architecture
question, then let the operator set the sequence rather than assuming
one) is exactly [[feedback-dispatch-should-carry-git-evidence-when-no-shell]]'s
sibling on the ENGINEER side rather than the librarian side — worth
recognizing if a future session sees the engineer defer a sequencing
call to Ken again.

**Open item, unnumbered, filed to `ROADMAP.md` Backlog, not yet a
decision record.** Which theme preset (if any) becomes the shipped
default. This is a genuine design-direction question, not a spec/
behaviour ruling, so it does not get an Open-operator-question letter
the way spec ambiguities do — check `ROADMAP.md` Backlog directly, not
the lettered-question list, when Ken answers it.

**Load-bearing invariant this Pass established, worth knowing before
touching ANY colour in `pdfce-gui` going forward:** chrome is themed,
document colour is never touched by a theme — `markup_color`/
`prop_color`/one colour-operator comparison write into the saved PDF
and are marked `// DOCUMENT COLOUR:`, explicitly excluded from
`tools/check-theme-colors.sh`. Full record: `ARCHITECTURE.md` §12,
eightieth-filing entries (two, one per Pass); §3 body sync in the same
filing.

**How to apply.** When Ken next raises GUI-appearance work, this
already answers "is it safe to restyle" (yes — one file, one gate) and
"has a look been chosen" (no — still open). Don't re-derive either from
scratch.
