---
name: project-bundled-backlog-entry-partial-ship-needs-residue-split
description: When a Backlog entry bundles several deliverables (e.g. "retarget A, then B, then C") and only A ships, split B/C into a fresh Pass ID rather than closing the whole entry — don't assume the dispatch already did this.
metadata:
  type: project
---

2026-08-20 (210th filing), `Pass 119.2` -> `a10a5c1`. The Backlog entry
read "retarget `format_text` (and then `reflow_block` / `add_text`)
into form XObjects" — three deliverables under one Pass ID. Only
`format_text` shipped; `reflow_block`/`add_text` remained deliberate
non-goals (the dispatch flagged `add_text` by name as needing its own
disclosure design, not just unfinished).

**Why:** ship-and-delete (this project's convention for closing a
Backlog entry once its Pass ships) assumes the whole entry is done. A
bundled entry that ships partially and gets fully deleted anyway
silently loses the residue — nobody would know `reflow_block`/`add_text`
retargeting was ever scoped. The engineer's own dispatch flagged this
explicitly this time ("the residue needs to survive as its own entry
rather than being closed with it"), but a future dispatch may not think
to say so.

**How to apply:** before deleting a shipped Backlog entry per
ship-and-delete, re-read what it originally promised against what the
Shipped write-up says actually landed. If the entry bundled more than
one verb/deliverable and only some shipped, mint a fresh Pass ID
(next available sub-ID in the family) for the residue rather than
letting it vanish with the parent entry's deletion. Same discipline
applies to `docs/FEATURES.md` Planned rows built from the same bundled
entry — split them the same way (see [[project_features_md_concision_rewrite_20260811]]
for the file's own no-history-in-row rule, which still applies to the
split row).
