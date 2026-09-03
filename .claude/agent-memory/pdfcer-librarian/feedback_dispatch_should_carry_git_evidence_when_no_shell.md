---
name: feedback-dispatch-should-carry-git-evidence-when-no-shell
description: When this librarian is dispatched without a shell and a filing turns on git history (which commit shipped what, whether two commits are ancestors of each other), the dispatch should carry the raw git/grep output directly rather than leaving the librarian to flag the question as unresolvable.
metadata:
  type: feedback
---

Confirmed 2026-08-09 (fifty-fifth filing), stated explicitly by the
dispatching engineer after resolving two consecutive open questions
this librarian had flagged as unresolvable without a shell:
`d2d03a5`'s hash-verification chain (fifty-third/fifty-fourth filings)
and the `ae59ce3`/Pass-24.0 contradiction (fifty-fourth filing, closed
same day it was raised once evidence arrived).

**The pattern.** This librarian frequently has no `Bash`/`PowerShell`
tool on a given dispatch (hard rule 8 forbids inferring commit content
or git state from documents alone in that case — the correct move is
to flag the question and stop, not guess). Both open questions above
were, in the engineer's own words, "shell-shaped, not evidence-shaped"
— closed by three `git` invocations and one `grep` once someone with a
shell ran them. Two filings in a row could only flag what a shell
closes in under two minutes.

**Why this matters:** flagging-and-waiting is the CORRECT behavior per
hard rule 8 — the failure mode isn't guessing, it's that the gap then
sits open across filings (a session, sometimes several) until someone
happens to have a shell and remembers to check. The fix is not "give
this librarian a shell" (that's the dispatcher's call, not mine to
request) — it's that when the engineer already knows a filing will
turn on git history AND knows this dispatch of the librarian won't
carry a shell, the fastest closure is pasting the relevant `git
show`/`git log -S`/`grep` output directly into the dispatch prompt,
the same way the coordinator message that unblocked `ae59ce3` did.

**How to apply:** don't ask the engineer to change this — it is
already the fix, self-identified and self-applied the same session it
was needed. This memory exists so a FUTURE session (mine, without
context of this one) doesn't re-flag the identical class of question
as unresolvable when the dispatch could have simply included the
evidence. If a future dispatch again leaves a git-history question
open with no evidence attached, it's fine to note in the filing that
carrying the raw output would have closed it faster — that's
information for the dispatcher, not a rule this librarian enforces.

See also [[project_uncommitted_repo_worktree_risk]] for the sibling
discipline (hard rule 8) this pattern sits next to — both are about
this librarian's evidentiary boundary and what's on the correct side
of it.

**Second confirmation, 2026-08-09 (fifty-sixth filing), the very next
dispatch.** The `c58cca1` filing's dispatch quoted the full commit
message verbatim and stated the hash was checked against `git log`
directly, unprompted — no re-request needed. Result: no open question
had to be flagged this filing; every figure in the entry was sourced
from the quoted commit text rather than inferred. Two for two now.
Nothing to change here — this is a working pattern, not a hypothesis;
keep noting IN THE FILING when it's used (as both the fifty-fifth and
fifty-sixth entries do) so the pattern's track record stays visible in
`SESSION_LOG.md` itself, not only in this memory file.

**Third confirmation, 2026-08-09 (fifty-seventh filing, `a3ba0f8`).**
Same shape again: dispatch text explicitly opened with "Git evidence
carried in the dispatch, per your own agent-memory note — third time,
still working," quoted the full commit message, and stated the
engineer verified the hash against `git log` this session. Three for
three. This is no longer worth a fourth confirmation note in the
ordinary case — treat it as the settled default for this project's
librarian dispatches rather than something to keep re-verifying is
still true. Only write a new dated confirmation here again if the
pattern is ever DROPPED (a dispatch reverting to bare hash citation
with no evidence) — that would be the surprising event worth recording,
not another instance of it working.

**Partial regression noted, 2026-08-10 (sixty-eighth filing) — not a
full drop, but not the settled pattern either.** The dispatch for
`079394f`/`aac321c` carried rich prose summaries (defect, fix, reasoning
all present) but not the raw commit messages themselves, and this
librarian ended up doing more direct-source verification than usual as
a result — which is how the FEATURES.md self-staling catch happened
(see `feedback_verify_dispatch_claims_against_live_source.md`), so the
narrower sourcing was not purely a cost this time. Worth watching
whether this was a one-off (the dispatching engineer working from its
own summary rather than pasting `git show`) or a genuine drift back
toward pre-fifty-fifth-filing behavior — one instance is not yet a
second data point either way.

**Second data point, confirms the drift, 2026-08-10 (eightieth
filing).** The `Pass 58.0`/`58.1` dispatch went further than the
sixty-eighth filing's partial regression: it explicitly INSTRUCTED this
librarian to run `git log -1 --format=%B <sha>` and `git show --stat
<sha>` for all four commits "before writing," with no shell tool
present on the dispatch — the instruction assumed shell access that
this invocation did not have. Rich prose summary was carried (as in the
sixty-eighth filing), no raw commit text. Handled the same way as
before: direct source verification (file line counts, doc-comment
content, specific line ranges) rather than commit-graph confirmation,
stated plainly in the filing's Sourcing paragraph rather than treated as
a blocker. **Two consecutive non-git-evidence dispatches is enough to
call this a real pattern, not a one-off** — worth surfacing to the
dispatching engineer directly next time it recurs (this file's own
guidance says to note it "in the filing," which was done both times, but
a THIRD occurrence should probably get a spoken flag in the librarian's
final report as well, not just a buried memory note).

**Third occurrence, 2026-08-11 (ninety-third filing), the flag now
delivered as promised above.** Dispatch text read *"Run
`python tools/check-commits-filed.py` yourself for the current list — I
am not pasting a list that will be stale by the time you read it,"*
with no shell tool present on the invocation — the instruction again
assumed shell access this dispatch did not have, same shape as the
eightieth filing. Handled by: filing the one commit (`b4a66ed`) whose
full content the dispatch DID carry inline (no git needed — content was
narrated directly, not sourced from a hash), independently verifying
that content against live source with `Read`/`Grep` (which surfaced a
real, previously-unrecorded defect the dispatch text had not
mentioned — see `ROADMAP.md`'s `b4a66ed` entry), and explicitly
declining to add hash citations for three further spot-checked commits
whose *topic* had a plausible candidate paragraph but whose *diff*
could not be confirmed without `git show`. **This is the spoken flag
promised above** — stated plainly in this dispatch's own final report
to the engineer, not just filed here. Net effect across three
occurrences: this librarian does not need a shell to do useful,
well-sourced work — `Read`/`Grep` against live source substitutes for
`git show` on any claim that live source can settle — but it cannot
substitute for `git log`/`git show` on claims that are inherently about
COMMIT HISTORY rather than current-tree content (which hash introduced
which change, what a specific historical diff contained). Those
remain flag-and-wait per hard rule 8, and a large hash-list sweep
(this dispatch's other 17 commits) is exactly that shape.
