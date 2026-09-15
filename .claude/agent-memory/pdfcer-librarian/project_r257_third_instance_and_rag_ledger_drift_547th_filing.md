---
name: project-r257-third-instance-and-rag-ledger-drift-547th-filing
description: 547th filing (2026-09-14) — pdfcer-gui closed the 546th filing's five unresolved verbs via their R8, R257 gained a 3rd instance, and a stale D:\dev\rag\rust ledger count (169 vs measured 369) was flagged, not reconciled.
metadata:
  type: project
---

2026-09-14, 547th filing, no code shipped. Two things worth remembering:

1. **A by-name search over a command catalogue is not a capability check when
   the claim's verb and the implementation's verb differ.** The 546th filing
   left "reorder" unresolved because no command is named `reorder` — it's
   `move_up`/`move_down` (plus drag). `pdfcer-gui`'s `R8` (command-catalogue
   membership + ribbon-manifest placement) resolved it. Kept at n=1, not
   minted as a RAG rule — watch for a second instance before writing it up.

2. **`D:\dev\rag\rust\` ledger count in ROADMAP.md's per-filing Ledger tables
   was carried as "169 findings" (from the 543rd filing) but `Glob
   D:\dev\rag\rust\*.md` on 2026-09-14 returned 369 files.** Not reconciled —
   flagged as an `index check` candidate. **Why this matters:** the per-filing
   ledger table increments by a fixed delta each time rather than recounting
   disk, so drift compounds silently across many filings without any single
   filing being "wrong." Before trusting that ledger row again, run the
   `index check` protocol (walk `docs/ROADMAP.md` + confirm `D:\dev\rag\rust\`
   file count against its own `index.md` bullets) rather than incrementing
   from the carried figure.

**How to apply:** next `index check` dispatch, prioritize reconciling the
`D:\dev\rag\rust\` ledger figure. And when closing any future "unresolved by
name" note, ask whether the implementation might use a different verb than
the claim before concluding the capability doesn't exist.

**RESOLVED, partially, 548th filing (2026-09-14), see
[[feedback_index_md_mixes_bullet_conventions_dont_trust_bullet_count_grep]].**
The 169-vs-369 gap's denominator is now correct (367 finding files). Its
"index-completeness" half is still open — deliberately not closed by hand,
because hand-checking is exactly the method that produced a false positive
(six files reported "missing" that were all already indexed under an older
bullet style). Needs a script, not a grep.
