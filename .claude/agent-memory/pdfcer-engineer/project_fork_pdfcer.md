---
name: fork-pdfcer
description: 2026-09-03 the project forked BY CLONE to D:\Dev\pdfcer (product name pdfcer); D:\Dev\pdfce is the untouched backup and must not be written to; where each 247.x step stands
metadata:
  type: project
---

**The live project is `D:\Dev\pdfcer`** (a `git clone` of `D:\Dev\pdfce` at
`cce414e`, 2026-09-03, decision 128). `D:\Dev\pdfce` is the operator's
untouched backup — the ONLY write it ever receives is the README pointer
commit in `Pass 247.2`. A session that opens in `D:\Dev\pdfce` must `cd`
to the clone before doing anything.

**Why:** Ken wanted the product renamed `pdfcer` ("pdf-see-er": create,
edit, read) with the obsolete in-repo GUI stripped, and judged a new folder
the safest way, the old one staying as backup. A clone (not a fresh repo)
keeps the 2,040 cited commit hashes and 30 tags that CI checks.

**How to apply:**
- `Pass 247.0` (strip the GUI crate, three GUI-only gates, harness scripts;
  4,730/0 tests = 5,114 − 384 GUI tests) — DONE 2026-09-03 in the clone.
- `Pass 247.1` (rename; CLI binary `pdfcer`, crate `pdfcer-cli`; v0.28.0;
  sidecar reads legacy `/pdfce` key, decision 131) — DONE 2026-09-03.
- `Pass 247.2` — DONE 2026-09-03: `KenM76/pdfcer` created and pushed with
  all tags; `KenM76/pdfce` archived (two pointer commits, its CI ends
  green); v0.28.0 released (GitHub + OneDrive pdfcer1; pdfcer2 seeded with
  a copy of pdfce2's v0.27.0). Repo creation/archiving was a ONE-Pass
  authority — not standing.
- The GUI is `pdfcer-gui` at `D:\dev\pdfcer-gui` (its own engineer); as of
  2026-09-03 it still depends on `file:///D:/Dev/pdfce` (frozen); the exact
  re-point lines were posted on the channel. Check whether they switched.
- `pdfceF{n}` resource names are a deliberate KEEP (opaque /Resources keys).
- `check-string-gaps.sh` is NOT GUI-only (plan premise was wrong); it stays.
- Global `~/.claude/CLAUDE.md` still says `D:\Dev\pdfce\` — Ken's to edit.
