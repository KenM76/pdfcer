---
name: project-decision-slicing-list-is-not-id-allocation
description: 2026-08-08 — two Pass-ID collisions in one session (Pass 47.5→47.11, Pass 47.7→48.3), both minted from memory of decision 033 §6's slicing list rather than the live ceiling checker; exposed a 3rd, distinct check-passes-filed.py blind spot; R106 got a fourth amendment.
metadata:
  type: project
---

**What happened (2026-08-08, thirty-fifth and thirty-sixth filings).**
`docs/decisions/033`'s §6 laid out a P0/P1/P2 work-breakdown list for the
GUI-usability Pass family (47). Twice in one session, the dispatching
engineer minted a Pass sub-ID by reading a position in that list from
memory rather than re-running `tools/check-ledger-numbers.py`/
`check-passes-filed.py`. Both times the position was already claimed by
something else filed by an intervening session:

- `Pass 47.5` minted for a mid-session text-edit retarget bug fix
  (commit `5ceb5f8`) — `47.5` was already right-click context menus.
  Corrected to `Pass 47.11`.
- `Pass 47.7` minted for the GUI image-drop gesture (commit `e6ad48c`) —
  `47.7` was already the contextual ribbon tab. Corrected to `Pass 48.3`
  (which was, separately, already the correct reserved ID for this exact
  capability, filed by the thirty-fifth filing's own Backlog entry).

**Why:** the engineer's own account named the mechanism both times — the
decision record's numbered slicing list was read as though it allocated
IDs. It never does; it only proposes scope and rough order. See
`D:\dev\rag\rust\a_decision_records_proposed_slicing_is_a_proposal_not_an_id_allocation.md`
for the full derivation and `D:\Dev\pdfce\docs\ROADMAP.md`'s `R106`
(Standing rules), fourth amendment, for the project-side record.

**Secondary finding, from the second collision specifically:** because
`e6ad48c`'s wrong subject line (`Pass 47.7: …`) is now permanent (commit
unpushed but citations to its hash already exist in prior filings, so
rewriting was rejected — see `ROADMAP.md`'s `Pass 48.3` Shipped entry),
this exposed a THIRD, distinct blind spot in `tools/check-
passes-filed.py`: its join key is hash-presence in `ROADMAP.md`, never a
comparison of the commit's subject-claimed ID against the ID of the entry
the hash is filed under. Not fully silent — a real future `Pass 47.7`
commit would trigger the checker's existing collision-note mechanism —
but the note cannot distinguish "legitimate multi-commit Pass" from
"stale mistaken subject line," per the checker's own documented
limitation. **Left as an open engineer decision, not resolved by this
filing**: whether to add a docstring `KNOWN WEAKNESS` bullet, extend the
collision note to diff subject text, or accept the residual risk.

**Why this is worth a project memory, not just the RAG file:** the RAG
file is the generalizable engineering finding (any project with a
decision-record-driven numbering scheme). This memory is the pdfce-
specific state: `R106` now has a fourth amendment, the Pass-ID collision
count is six (enumerable: 13.x, 18.4/18.2, 19.4/18.7, 24.0a/24-family,
47.5/47.11, 47.7/48.3), and the broader "numbering collisions" tally
(last stated as six, 2026-08-04) was deliberately left unrecomputed
pending a full re-audit — do not assume it is "eight" without doing that
audit; it may be higher if any other, non-Pass-ID collision also went
untallied in the same window.

**How to apply:** before minting any Pass/rule/decision number sourced
from a decision record's own proposed list (not just from a remembered
ceiling), run the live checker anyway — the record's list is never
sufficient evidence of what is free, no matter how carefully re-read.

**Near-miss caught, not a new collision, 2026-08-09 (fifty-seventh
filing, `a3ba0f8`).** Same root mechanism one layer earlier: the
engineer's own source comments provisionally called a new Pass
`Pass 20.7`, reading it as "the next sequential sub-ID in family 20,"
without checking that decision 020 §6 had already reserved `20.7` for
an unrelated capability (F7, `merge --on-field-collision`). The
dispatch explicitly deferred the Pass-number decision to this librarian
("Pass number is your call... if 20.7 is free, take it; if not, assign
what is and tell me") rather than asserting the number outright — the
dispatcher's own uncertainty was the tell. Grepping `ROADMAP.md` for
`Pass 20.7` before filing found the reservation; assigned `Pass 53.0`
instead (fresh top-level ID, same shape as `Pass 46.0`'s earlier R151
closure). **The difference from the six enumerated collisions above:
this one never reached a commit or a filed entry** — it was caught at
the filing gate, which is this memory's whole "how to apply" clause
working as intended. Worth recording anyway: it confirms the failure
mode recurs in a THIRD shape (not just "read a decision record's
slicing list from memory," but "assume the next sequential sub-ID is
free without checking who else claimed it") and that grepping the
target ID before filing is a five-second check that pays for itself
every time it's actually run.
