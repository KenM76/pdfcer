---
name: feedback-verify-dispatch-claims-against-live-source
description: When a dispatch reports "docs/FEATURES.md row N is rewritten" or hands over a figure, read the CURRENT source the row describes before filing — the dispatch's own restatement can already be stale by the time it reaches this librarian, even when the dispatch is accurate about everything it actually checked.
metadata:
  type: feedback
---

**2026-08-10 (sixty-eighth filing).** A dispatch reported `aac321c` as
having corrected `docs/FEATURES.md` row 155's inverted rich-text-fill
direction, and asked this librarian to "verify my replacement against
your own reading rather than accepting it." Taken literally rather than
as a formality: reading `crates/pdfce-cli/src/main.rs` directly (not
just the row text) found the row's own just-written CLI clause ("CLI
does not expose the downgrade") was **already false**, because the same
commit that corrected the row also shipped `fill-field
--downgrade-rich-text` — a live R180 recurrence inside the very commit
meant to fix a disclosure, which the dispatch's own summary did not
mention and plausibly did not know about (it may have been drafted
before the CLI-flag half of the same commit landed).

**Why this is a pattern worth keeping, not a one-off catch:** the
dispatch was not wrong about what it checked — it was incomplete about
what the commit it was summarizing actually contained. A restatement of
"the row is now correct" is a claim about the ENTIRE row, but the
dispatcher's own attention was on the direction-inversion half of the
fix; nothing forces a re-read of every clause in a multi-clause row
against every change in a multi-part commit. Also independently verified this session, same discipline: a reported
"42" guard-site count did not survive a direct `grep` (actual: 44 total
occurrences, 26 raise sites) — full record in `ROADMAP.md`'s Encryption
Backlog bucket, not duplicated into memory since the disk already
carries it in full.

**How to apply:** when a dispatch says a doc row/section "is corrected"
or "is rewritten," read the CURRENT state of whatever the row describes
(the function, the flag, the call site) before filing — not just the
row's new text, and not just the specific claim the dispatch called out
as fixed. Treat "please verify" as an instruction to re-derive the fact,
not to proofread the prose. This is the same discipline hard rule 8
demands for backup/git state, generalized to any claim this librarian
did not itself measure.
