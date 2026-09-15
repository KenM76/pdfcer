---
name: project-pass-304-cad-tz-td-tolerance-551st-filing
description: Pass 304.0 (operator-direct request, no topic key) — CAD exporters restate Tz and perturb Td between fragments of one visual line; filed 551st, personal_rag/pdf count corrected via Glob not carried arithmetic
metadata:
  type: project
---

**Pass 304.0** (`2a862742`, 2026-09-14, 551st filing) shipped with **no topic
key** — it came from the operator directly in conversation, not from
`pdfce_FeatureRequests`/`iccce_FeatureRequests`. The dispatch explicitly
warned against inventing one and specifically against reusing `G017`
(pdfcer-gui's own namespace, already claimed same-day for an unrelated
`move_text_run` entry). **Always check whether a shipped Pass has a topic
key before assuming one — "every Pass answers a channel request" stopped
being true here.**

**Why:** `docs/NEXT_SESSION.md`/`SESSION_LOG.md` had trained a expectation that
every recent Pass traces to a `G###`/`E###` exchange (true for 296–303). This
one broke that pattern; filing it with a fabricated key would have been a
citation to nothing.

**How to apply:** before writing "Answers `request_...`" into a Shipped entry,
confirm the dispatch actually names a request file. If it doesn't, say so
explicitly ("No topic key — operator-direct request") rather than omitting
the sentence, so a later reader doesn't assume one was missed.

**Also confirmed this filing:** `Glob C:\personal_rag\pdf\lesson_*.md` is the
right way to get the ledger's "before" lesson-file count — the ROADMAP
ledger's carried figures for this directory have drifted before (see
[[project_r257_third_instance_and_rag_ledger_drift_547th_filing]] for the
`D:\dev\rag\rust\` analog), so re-measure with `Glob` rather than trusting the
most recent "after" figure blindly, even though in this case it did check out
(231 → 232, matching the prior filing's own "after" column).
