---
name: project-repo-is-public-since-20260809
description: pdfce's git remote exists and github.com/KenM76/pdfce is PUBLIC as of 2026-08-09 04:56Z — do not assert "no remote"/"local-only" as a current fact without checking
metadata:
  type: project
---

`github.com/KenM76/pdfce` was created **PUBLIC** 2026-08-09 04:56:14Z and
pushed (`main` @ `01b90c4`) 10:18:31Z. `docs/LEGAL.md` §1.1 — the section
written specifically to warn about publish exposure — nonetheless asserted
*"there is still no git remote configured"* that same evening, hours after
the remote existed. `CLAUDE.md` rule 8 and `docs/NEXT_SESSION.md` carried
the same false claim. Corrected by the engineer in `269361d`.

**Why:** dozens of dated, historical `ROADMAP.md`/`ARCHITECTURE.md` entries
from before 2026-08-09 correctly say "no git remote configured" — those are
true on their date and were NOT rewritten (2026-08-10 sweep, fifty-eighth
`SESSION_LOG.md` filing). Two entries in the append-only
`docs/decisions/003-distribution-posture.md` (line 69, framed as "the
framing fact everyone should read first") and
`docs/decisions/007-...md` were found stale in the same sweep and flagged
to the engineer (not editable by this librarian — outside the five storage
tiers).

**Open operator question (bh)** (git-history handling before any public
push — three options: rewrite history / squash to fresh initial commit /
accept the exposure) **CLOSED 2026-08-10, operator's ruling: ACCEPT.** The
pre-`817d518` history, including the now-removed confidential third-party
material, stays reachable by SHA (GitHub keeps unreachable objects fetchable
until Support purges them, so `git filter-repo` was never a clean fix
anyway — and rewriting would have dangled every commit hash this project's
own filings cite). 0 forks / 0 stars at decision time. **Binding on the
engineer: not reopened to be helpful, not precedent for the next
third-party-material incident** — each gets its own ruling.

**How to apply:** never write or repeat "no git remote configured" /
"local-only" / "not yet pushed" as a CURRENT fact without checking
(`git remote -v`, `gh repo view`) — this is exactly the class of claim
[[project_uncommitted_repo_worktree_risk]] and hard rule 8 already warn
about, now generalized project-wide as standing rule **R175**
(`ROADMAP.md` Standing rules, minted 2026-08-10: a document's claim about
git/CI/environment state is only as current as its last measurement).
Also: publish deny-rules were removed from `.claude/settings.json` and
`.claude/settings.local.json` in `269361d` by the operator's explicit
choice (an agent CAN push on request now) — `gh release`/`cargo publish`
remain denied, and rule 8's go-ahead requirement survives with no
mechanical fence behind it, so still don't push/release without a fresh,
explicit go-ahead in the conversation.
