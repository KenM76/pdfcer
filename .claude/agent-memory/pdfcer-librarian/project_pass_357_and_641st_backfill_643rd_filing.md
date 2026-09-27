---
name: project-pass-357-and-641st-backfill-643rd-filing
description: Pass 357.0 shipped (D4 fully closed, audit item complete); the 641st filing's Pass 355.0 shipped without updating the Backlog's audit narrative — caught and backfilled at the 643rd filing.
metadata:
  type: project
---

**Pass 357.0** (`a6695bae`), 2026-09-27, 643rd filing — a re-encoded `/DR`
font (MacRoman/Standard/WinAnsi+`/Differences`, own `/Widths`) now binds and
draws its own codes. Closes D4's last remainder from the Backlog's "Audit
every `FieldEdit`/`WidgetEdit` property" item (filed 2026-09-16, 562nd
filing) — **that whole audit item is now CLOSED**, all of D1–D8/D4b/X1/X2
resolved across `Pass 335.0`–`357.0` (twenty-two dated fixes). `FEATURES.md`
row 319 (not 318 — a prior filing miscited it) widened in place, confirmed
under the 1,200-char cap via `Grep ^.{N,}$` bisection (no shell tool
available to run `check-register-entry-size.py` directly).

**Why this is worth keeping:** the 641st filing shipped `Pass 355.0` (the
`/Ascent` vertical-metrics fix, also part of D4) but never added a dated
"fix shipped" entry to `ROADMAP.md`'s Backlog audit narrative, and never
updated the D4 bullet to reflect it — the narrative and the bullet still
read as if only the encoding half was outstanding, when both halves were
already partly progressed. Caught only because I re-read the full audit
narrative before appending my own entry, rather than trusting the
engineer's dispatch prose (which cited D4 correctly as "the remainder"
but didn't flag that ROADMAP's own record was stale).

**How to apply:** when a dispatch says "Pass N closes/narrows finding X
from audit/backlog item Y," always re-read Y's own narrative section in
`ROADMAP.md` before appending — don't assume the prior filing kept it in
sync just because the Shipped entry for that Pass looks complete. Backfill
gaps found this way with an explicit note ("belatedly recorded... caught
while filing Pass N+1") rather than silently inserting the missing entry
as if it had always been there — see hard rule 10's correction-is-a-claim
discipline in `.claude/agents/pdfcer-librarian.md`.

Related: [feedback_index_md_mixes_bullet_conventions_dont_trust_bullet_count_grep](feedback_index_md_mixes_bullet_conventions_dont_trust_bullet_count_grep.md) —
same genus of lesson (verify the record directly, don't infer completeness).
