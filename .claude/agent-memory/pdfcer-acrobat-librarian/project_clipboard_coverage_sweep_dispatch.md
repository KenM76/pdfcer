---
name: project-clipboard-coverage-sweep-dispatch
description: a 7th dispatch shape — "does cut/copy/paste exist for every object class" sweep; one cross-cutting coverage-matrix file, not per-class files, reusing the comparison__ precedent
metadata:
  type: project
---

2026-08-29: Ken asked, verbatim, "can you make sure we have cut, copy,
and paste available for everything and if not implement?" This is a
distinct dispatch shape from the six already on file (bucket-building,
cross-cutting comparison, GUI interaction model, second-hand claim
verification, Reader-parity sweep, GUI capability-surface audit) — it's
a single VERB (cut/copy/paste) audited across every OBJECT CLASS a
product touches, not a single object class audited across every verb.

**How I handled it**: one new cross-cutting file,
`clipboard__cut_copy_paste_coverage_matrix.md`, reusing the
`comparison__pdfce_feature_column.md` precedent (no `<prefix>__` bucket
match, `roadmap_bucket: cross-cutting`, spans the whole taxonomy) rather
than spinning up 8-10 new per-bucket files. A coverage TABLE (class ×
cut/copy/paste × same-doc/cross-doc × confidence) up top, then one
`###` subsection per class with the sourced detail, then a
"cross-cutting conclusions" section answering the operator's specific
sub-questions (any Copy-without-Cut class? what's on the OS clipboard
per class? any silent paste-loss to disclose?) as their own section
rather than burying the answers inside per-class prose.

**Why this shape and not bucket-building**: the request's grain is
"every class × these 3 verbs," and most classes only need 2-4 sentences
plus a table row — spinning up a full `_TEMPLATE.md`-shaped file per
class would have been mostly-empty boilerplate for the thin classes
(attachments: total GAP; layers: "not directly copyable at all") and
would have fragmented the cross-class conclusions (the Copy-without-Cut
pattern recurring in TWO unrelated subsystems — page content AND form
fields — is only visible if you can see both in one file).

**When to reuse this shape**: "make sure X [a single interaction/verb]
works for everything" requests — future candidates: drag-and-drop
coverage, undo/redo coverage, keyboard-navigation coverage, export
coverage. Same answer each time: one cross-cutting coverage-matrix file
with a table + per-class detail + a "cross-cutting conclusions"
section, not N per-bucket files.

**One finding worth flagging to future sessions, not just this one**:
the SAME missing-Cut gap (Copy exists, Cut doesn't, only a two-step
Copy-then-Delete workaround) showed up independently in page content
(Edit PDF/TouchUp Object) AND form fields — two structurally unrelated
Acrobat subsystems with the identical omission. When a coverage sweep
finds the same shape of gap twice, independently, that's stronger
evidence it's a real architectural pattern (worth exceeding on both at
once) than either single finding would be alone — cross-reference
[[project_bucket_building_pattern]] for the general "look for recurring
shapes across buckets" instinct.
