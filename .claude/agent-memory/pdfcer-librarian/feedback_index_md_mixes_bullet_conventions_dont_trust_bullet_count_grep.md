---
name: feedback-index-md-mixes-bullet-conventions-dont-trust-bullet-count-grep
description: D:\dev\rag\rust\index.md (and likely egui\index.md) mixes at least two bullet-authoring conventions across its growth history; a single-pattern grep undercounts index entries and produces false "unindexed" positives.
metadata:
  type: feedback
---

**Never conclude a `D:\dev\rag\<tool>\` finding is unindexed from a single
bullet-style grep count.** `D:\dev\rag\rust\index.md` (570 KB, 369 files as
of 2026-09-14) mixes at least two conventions from different points in its
history: the current `- [Title](file.md) — hook` markdown-link style (362
occurrences), and an older `` - `file.md` `` bare-backtick style followed by
unindented prose (9 occurrences found, likely more). `grep -c '^- \['`
matches only the first. A count built that way will always read low, and
low against a disk count reads as "N files missing" when the true state is
"the file uses two styles."

**Why it matters more than an ordinary undercount:** acting on the false
positive means *writing duplicate entries* into an append-only, LLM-facing
index — the exact failure the project's own "grep before writing a lesson"
discipline exists to prevent, and a 570 KB file is expensive to de-duplicate
later. One of the six files nearly duplicated this way
(`a_sabotage_can_only_be_as_discriminating_as_the_fixture_it_runs_on.md`)
already carries 17 dated instances in its real entry.

**How to apply.** Before reporting any filename as unindexed:
1. `Grep` the bare filename (no bullet prefix assumed) against the whole
   `index.md`, not a bullet-prefixed pattern.
2. If that returns nothing, only then treat it as a real candidate — and
   even then, prefer *reading* the surrounding lines over trusting the grep
   alone, since a mention can exist in prose without a leading `- `.
3. **Index-completeness (proving zero files are unindexed) cannot be done
   reliably by hand** once a file mixes conventions or exceeds a few hundred
   KB — this role has no shell tool to script the extraction/diff, so the
   honest answer when asked to verify completeness fully is "needs a script
   run outside this role," not a number produced by hand-grepping.
4. `D:\dev\rag\egui\index.md` showed the identical shape at a glance (203
   bracket-bullets against ~215 candidate finding files) when this was
   found (2026-09-14) — not investigated further, flagged for the same
   reason. Check it the same cautious way before ever citing a gap there.

**Recommended fix, not yet built:** a script living in `D:\dev\rag\` itself
(not any single project's `tools/`, since the tree is shared across
projects and outside any one repo's CI) that extracts every referenced
`*.md` filename from `index.md` under any bullet convention, dedupes, and
diffs both directions against `ls *.md` minus named meta files
(`index.md`, `rust-style-guide-and-api-guidelines.md`). Until it exists,
treat any "N files unindexed" claim for these trees — including ones I
myself produce — as unverified until each candidate is read directly at
its cited location.

See [[project_r257_third_instance_and_rag_ledger_drift_547th_filing]] for
the incident this was learned from (548th filing, 2026-09-14).
