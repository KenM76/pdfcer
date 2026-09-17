---
name: librarian-needs-exact-hashes
description: The librarian has no shell access — always paste exact commit hashes into its dispatch; never write a placeholder or say "see git log"
metadata:
  type: feedback
---

`pdfce-librarian` (and the other doc agents) have **Read/Write/Edit/Glob/Grep/
WebSearch/WebFetch only — no Bash, no PowerShell.** They cannot run `git log`,
`git rev-parse`, or anything else to discover a commit hash.

**The rule:** run `git log --oneline` yourself and paste the *exact* hashes into
the dispatch prompt. Never write a placeholder hash, never write
"`<hash>`-equivalent", and never instruct the agent to "see `git log`".

**Why:** on 2026-08-02 the engineer dispatched a filing with
"`3a0b1f1`-equivalent (see `git log`)". The librarian could not check, so it
used the placeholder verbatim — and wrote the **non-existent** hash `3a0b1f1`
into `docs/ROADMAP.md` and `docs/SESSION_LOG.md` **12 times**, including in the
commit chain. The real hash was `c59b0c4`.

A fabricated hash in an audit trail is worse than no hash at all: the first
person to run `git show 3a0b1f1` gets "not a valid object name", and that
discredits every other claim in the record. The whole point of the ROADMAP is
that it can be trusted without re-deriving it.

**How to apply:**
- Before any librarian dispatch that references commits, run
  `git log --oneline -N` and copy the real short hashes.
- After a filing lands, spot-check it: `grep -o '[0-9a-f]\{7\}' docs/ROADMAP.md`
  piped through `git cat-file -t` catches fabricated or stale refs cheaply.
- The same caution applies to any figure the agent cannot verify — test counts,
  file counts, timings. If you did not measure it, do not hand it over as fact,
  because it will be filed as fact. See [[feedback_engineer_does_the_observing]]
  for the same principle applied to behavior rather than metadata.

---

## ★★ 2026-08-27 — AND DO NOT AMEND A COMMIT AFTER DISPATCHING ITS FILING

A new way to hand the librarian a wrong hash: **hand it a right one and then
make it wrong.**

I dispatched `Pass 136.2` at `a2f7b48`. The filing was correct. Then I noticed
that commit's message had shipped mangled (backticks eaten by the shell) and
fixed it with `git commit --amend` — which **rewrites the commit and changes the
hash**. `a2f7b48` became a dangling object off `main`, the real commit was
`5f6ac58`, and both `check-passes-filed` and `check-commits-filed` went red on a
Pass that had just been filed perfectly.

**How to apply:** once a commit has been dispatched for filing, treat its hash
as published. If it must be amended anyway, **re-dispatch the correction in the
same breath** — the librarian has no shell and cannot discover the change.

★ Better still: re-read `git log -1 --format=%B` *before* dispatching, so the
amend happens first. The mangling this fixed is itself covered by
[[windows-paths-need-literal-edits]]; the two failures compounded, one creating
the need to amend and the other making the amend expensive.

★★ Note which gate caught it: `check-passes-filed` compares `ROADMAP.md`
against **the actual git history** rather than against itself. Most of this
project's gates read one document and check it for internal consistency; this
one reads a different source than the document it validates, which is the only
reason a stale-but-well-formed hash was detectable at all.

**The session-scale shape**, third variant in one day: a dispatch is a
*snapshot* (facts can be stale by the time they are written), a dispatch is a
*write path* (its text lands in public documents), and now — a dispatch's facts
can be **invalidated by what the engineer does afterwards**. All three are the
engineer's to prevent; none is visible from the librarian's side.

---

## ★★ 2026-09-17 — AND A LIBRARIAN CANNOT TELL COMMITTED WORK FROM UNCOMMITTED WORK

The fourth variant, and the first where the agent did everything right.

I dispatched the `Pass 310.0`/`310.1` filing at `ce474dae` while `Pass 310.2`
sat **uncommitted in the working tree**. The brief said 310.2 was still open.
The librarian — correctly sceptical, per its own verification duty — checked
that claim by reading `redact.rs`, found `text_match_ranges` and both its new
tests right there, and concluded the dispatch was stale. It filed all three
Passes under one commit, `ce474dae`.

Everything it read was real. The attribution was not. 310.2 was `bb1bd994`,
committed twenty minutes later.

**A working tree shows what is on disk; only `git show <hash>` shows what a
commit contains. Reading a file cannot tell you which commit introduced it** —
and a shell-less agent has only the first of those.

★ **`check-cited-commits-exist.py` cannot catch this class.** `ce474dae` is
real and an ancestor of HEAD, so a wrong-but-existing hash passes clean. The
gate answers *does this commit exist*, never *is it the right one*. What caught
it was my own `git log` after the filing — the spot-check this memory already
prescribes.

**How to apply:** dispatch the filing **after** committing everything it
describes. If that is impossible, say in the dispatch which files carry
uncommitted work and that the tree is not the commit — otherwise the agent's
own diligence is what produces the wrong answer.

**The shape across all four variants:** placeholder hash, amended hash, stale
figures, and now a tree read as a commit. Every one is invisible from the
librarian's side and preventable only from mine.
