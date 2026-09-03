---
name: project-gui-interaction-model-dispatch
description: a new dispatch shape (2026-08-08) — cataloging Acrobat's/PDF-XChange's INTERACTION-MODEL behavior (context menus, multi-select semantics, tool-tab docking, KeyTips, tree verbs) to ground pdfce's own GUI-redesign Passes, not a document-processing feature bucket
metadata:
  type: project
---

2026-08-08: `pdfce-engineer` dispatched a session to ground five
GUI-redesign Passes (decision 033 §6 — `Pass 47.5` right-click context
menus, `47.6` canvas-selectable widgets, `47.7` contextual tool tab,
`47.9` KeyTips, `47.10` object-tree verbs) before they get scoped past
"Backlog." This is a THIRD distinct dispatch shape, alongside
[[project-bucket-building-pattern]] (deep single-bucket cataloging) and
[[project-cross-cutting-comparison-dispatch]] (wide-shallow verdict
pass): it catalogs Acrobat's/PDF-XChange's **interaction-model
behavior** — what a right-click menu offers under which selection state,
how docking/undocking works, whether a KeyTips-equivalent exists — not a
document-processing FEATURE the way every prior bucket did. Handled by
writing 6 new files under a NEW, non-standard-taxonomy prefix
(`gui_interaction__*`), with `roadmap_bucket: GUI usability P1/P2`
(the exact `ROADMAP.md` Backlog-bucket bullet text for decision 033 §6,
which is NOT one of the 16 classic Acrobat-feature-area bucket names) —
flagged explicitly in `index.md`, same judgment-call precedent as the
pre-registered "Measuring tools" bucket.

**Why this is a genuinely different shape, worth recognizing next time
it recurs**: every prior bucket asked "what can Acrobat's [redaction /
forms / measuring / …] feature DO." This dispatch asked "how does
Acrobat's/PDF-XChange's GUI BEHAVE" — selection semantics, menu
disabled-vs-omitted conventions, docking rules, keyboard-accelerator
systems. This sits closer to the line this whole RAG exists to stay off
("never catalog GUI mechanics") than any prior session — the resolution
that worked: describe the underlying INTERACTION LOGIC (what verb set a
selection state produces, whether a control disables or vanishes, what
capability a keyboard system offers) and explicitly omit screen
appearance/positioning/wording (handle glyph shape/colour, exact menu
item text, panel layout) — same discipline as always, just applied to a
subject matter that requires more care to keep on the right side of it.
If asked for this shape again, hold that line explicitly rather than
re-deriving it: capability/behavior-of-the-interaction-MODEL is in
scope; the interaction's on-screen APPEARANCE is not.

**Two-reference-product framing was central and should be reused.**
Unlike most prior sessions (Acrobat-only, PDF-XChange only appears in
the drag-and-drop session and prepress bucket), this dispatch explicitly
named PDF-XChange Editor as a co-equal reference (the operator "named it
alongside MS Office" for ribbon work) — every file in this cluster
reports BOTH products' behavior and states explicitly which one is more
relevant when they diverge, rather than defaulting to Acrobat as
primary. **The single most decisive finding of the session came from
this framing, not from either product alone**: both products
STRUCTURALLY AVOID heterogeneous cross-domain multi-selection
(PDF-XChange by explicit rule — content/comments/fields are mutually
exclusive; Acrobat by tool-based hit-test separation, reasoned
inference) — meaning pdfce's decision 033 unified-hit-tester choice
(Pass 47.6) is genuinely novel relative to BOTH references, not a
parity question. Neither product alone would have surfaced this; only
comparing them did. See
`gui_interaction__multi_selection_heterogeneous_behavior.md`.

**A second reusable finding: sometimes the reference-product-of-record
flips per sub-question, and that's fine to state plainly rather than
force a single "the reference is X" framing across the whole
dispatch.** KeyTips (`Pass 47.9`) is the clean example: Acrobat has
genuinely abandoned the whole paradigm in its current UI (its legacy
classic-menu Alt-mnemonics were REMOVED post-Acrobat-11, replaced by
Tab-sequential focus, a structurally different model) — so PDF-XChange,
which has a natively-named "KeyTips" feature with directly-sourced
implementation lessons (badges render on Alt key-UP not key-down, fixing
a real hotkey-conflict bug), is the ONLY usable reference-product
precedent for that one Pass, even though Acrobat was the primary
reference for every other file in the same session.

**Sourcing profile, worth setting expectations on next time**: this
cluster leaned more heavily on WebSearch-snippet and community-forum
sourcing than any prior bucket, and that's inherent to the subject —
neither vendor publishes a spec for "what does the right-click menu
show under selection state X." 5/9 WebFetch attempts succeeded this
session (2 forum.pdf-xchange.com, 2 community.adobe.com, both
notably reliable non-Adobe-root domains — consistent with
[[feedback-helpx-fetch-reliability]]); the one helpx.adobe.com attempt
timed out, same standing pattern. Don't expect a cleaner sourcing
profile for a repeat of this dispatch shape — budget for
community-thread cross-corroboration (3+ independent threads agreeing)
as the practical confidence ceiling, not single-source Adobe
documentation.

**Existing already-decided pdfce design content should be READ FIRST,
not re-derived** — decision 033 itself already answers several
questions a naive researcher might otherwise treat as open (e.g. its
own §5.1 R124 already specifies context-menu disabled-vs-omitted
behavior, CITING MICROSOFT's ribbon guidance, not Acrobat). The value
this session added wasn't re-deciding those — it was independently
CROSS-VALIDATING them against actual Acrobat/PDF-XChange behavior
(Acrobat's own Copy/Paste greys-out-not-omits under restriction,
independently agreeing with the Microsoft-sourced rule) and finding the
handful of places where the reference products' actual behavior adds
something the internal decision doc didn't have (the union/intersection
verb-class split; the reveal-on-canvas on-demand-not-live-sync
precedent; the drag-reliability defect as motivation, not just
preference, for a reliable `/Rect` readout). Read the relevant decision
record's own §3/§5 sections before researching, so the session
surfaces NEW grounding rather than re-answering what's already decided.
