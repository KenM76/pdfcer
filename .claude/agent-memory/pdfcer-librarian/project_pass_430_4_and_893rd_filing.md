---
name: project-pass-430-4-and-893rd-filing
description: Pass 430.4 (decision 187's cmap rule reaches decision 177's augmented CID stream) filed as the 893rd filing, 2026-10-02
metadata:
  type: project
---

Pass 430.4 (`21bcabb1`, 2026-10-02) closed a Backlog item that had only
been filed one session earlier (892nd filing, `Pass 430.3`): decision
187's strip/share `cmap` rule for route B now also governs decision
177's augmented `/Type0`/`CIDFontType2` stream (route A composite
augmentation). No new decision number — extended decision 187 §6 and
its `ARCHITECTURE.md` §12 entry with dated amendments instead.

**Why worth remembering:** decision 187 §6 (`docs/decisions/187-route-
b-cmap.md`) already *named* this exact follow-up in its own text the
session it was minted ("Pass 430.4" called out by ID, not just
described) — so when the fix landed, the engineer's doc edit was
retitling an existing section rather than writing new prose. When a
decision doc explicitly names its own owed follow-up by future Pass
ID, expect it to resolve fast and expect the amendment to be a retitle,
not a rewrite.

**How to apply:** when filing a Shipped entry whose own Backlog
ancestor is less than a day old, grep the decision doc the ancestor
cited — it may already contain a forward-looking "Not covered"/owed
section with the Pass ID pre-named, which both confirms the ID and
tells you exactly which doc section to amend (not append fresh).

**Also flagged, not edited:** `docs/NEXT_SESSION.md` (engineer-owned)
still listed `Pass 430.4` as owed as of its last write (892nd filing) —
noted in the SESSION_LOG entry for the engineer to refresh, per the
recurring pattern in [[project_pass_261_family_closed_684th_filing]]
and similar entries of NEXT_SESSION.md lagging by one filing.
