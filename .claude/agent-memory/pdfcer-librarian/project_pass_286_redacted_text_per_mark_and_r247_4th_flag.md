---
name: project-pass-286-redacted-text-per-mark-and-r247-4th-flag
description: Pass 286.0 (redacted_text grouped per mark, not per glyph) shipped as 488th filing; R247 reservation now flagged unresolved for a 4th consecutive filing
metadata:
  type: project
---

Pass 286.0 (`369d4de`, 2026-09-09) shipped as the **488th filing**:
`RedactionReport::redacted_text` is now grouped per `/Redact` mark instead
of per show operator, closing `ROADMAP.md` owed item 17. Fixes a real bug:
GPL Ghostscript 8.15 draws one glyph per show operator, so a per-glyph
producer's redacted text used to be single characters and a consuming
absence proof refused correct redactions on finding ordinary digits/letters
on every page. `Surgeon::glyph` now returns a region index instead of a
bare `bool`; `box_marks` folds a mark's characters into one string. Three
consumers read this field (absence proof, `carrier_info`,
`residual_sweep`'s `redaction_evidence`) — joining is safe only because it
strictly lengthens entries, never shortens them.

**Why this matters for next time:** if a future change to `redacted_text`'s
granularity is proposed, check whether it lengthens or shortens entries —
shortening would need re-verification against all three consumers, not just
the one that prompted the change. This is the same three-consumer trap
`carrier_info`'s own fix (`Pass 282.0`) already hit once.

**`R247` reservation is now flagged unresolved across FOUR consecutive
filings** (475th, 483rd, 486th, 487th, 488th) — two candidates still
contesting the slot (a second `///`-doc-guarantee-with-no-enforcing-code
instance; the "alternate route" `R225`-family sabotage cause at `n=3`).
`R248` and `R249` were both minted past it deliberately, so whoever
reconciles `R247` should treat both as already spoken for. This librarian
has no authority to reconcile it unilaterally — it needs an engineer
session with time to sit down and decide which candidate (or a merge)
claims it. **Check `ROADMAP.md`'s Standing rules section and the most
recent SESSION_LOG entry before assuming this is still open** — it may
have been resolved by the time this memory is read.

**How to apply:** on the next filing, if `R247` is still unclaimed, keep
escalating visibly (a dedicated flagged box in the ROADMAP entry, not just
a ledger-row mention) rather than letting the flag go quiet — the operator
explicitly asked for visible escalation at the 4th-consecutive-filing mark.
