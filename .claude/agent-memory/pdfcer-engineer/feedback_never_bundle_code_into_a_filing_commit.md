---
name: never-bundle-code-into-a-filing-commit
description: A librarian filing commit that also touches crates/ or tools/ cannot file itself and leaves check-commits-filed.py red; repairs go in their own commit
metadata:
  type: feedback
---

**Repairs go in their own commit, never inside the librarian's filing.**

**Why:** `check-commits-filed.py` counts a commit as needing a filing if it
touches **code**. A librarian filing is the thing that files other commits —
so the moment you bundle a code repair into it, that commit becomes a code
commit **in no filing**, and the gate goes red on the very commit that was
supposed to make it green. It cannot file itself.

**The instance (2026-08-22, `c24ad7a`).** The librarian's filing of three
deep-zoom commits was committed together with three hard-rule-11 repairs
it had reported: a wrong Pass family in a README, a superseded zoom
ceiling in `main.rs`'s `--region` doc block, and a carriage-return-corrupted
RAG path in an agent file. Because `main.rs` was in the diff, the gate
immediately reported *"1 code commit is in no filing"* and a second
librarian dispatch was needed to close a loop that should never have
opened.

**How to apply:**

- When the librarian reports survivors it cannot edit (`crates/`, `tools/`
  and `.claude/` are outside its remit), fix them in a **separate commit**
  — before the filing is fine, after is fine, inside is not.
- A filing commit should be **docs-only**. If it is docs-only, the gate
  does not count it and the loop terminates in one step.
- The same applies to any commit whose subject begins `librarian:` — that
  prefix is a promise about the diff's contents, and mixing code in breaks
  it for a reader as well as for the gate.
- **Do not reach for `tools/commits-filed-baseline.txt`.** The gate's own
  output says so: that file is pre-existing debt, and extending it
  silences exactly what the gate exists to catch.

★ **The shape, because it will recur in other forms:** a checker whose
input is *the commit list* can be defeated by the commit that carries its
own remedy. This project has met that shape before — see
`D:/dev/rag/rust/a_gate_whose_input_is_the_commit_list_is_vacuous_when_the_pre_commit_sweep_runs_it.md`.
Anything that both *reports* and *is reported on* needs its two roles kept
in separate commits.

**★ THE RULE RUNS IN BOTH DIRECTIONS, learned 2026-08-22.** Everything
above is about *code inside a filing*. The mirror image happened the same
day: `eca07ee`, a commit about `--no-annotations`, swallowed a completed
librarian's uncommitted output — **+808 `ROADMAP.md`, +233
`SESSION_LOG.md`** — via `git add -A`.

**No gate went red, and none can:** `check-commits-filed.py` counts *code*
commits, and that was one, correctly. The damage is attribution rather
than enforcement — `git show <hash>` no longer isolates the filing, and
`git log -- docs/ROADMAP.md` credits an engineering commit with 808 lines
of librarian prose.

So the pair is: **a filing must contain no code, and a code commit must
contain no filing.** Only the first has a gate. The second is caught by
running `git status --short` and staging explicit paths — see
[[git-add-all-is-unsafe-with-live-subagents]], which now carries the
detail that a *finished* agent leaves exactly the same hazard as a live
one.

**And the converse, ruled explicitly so it is not over-applied:** bundling
nine unrelated stale-doc repairs *into* `PASS 74.8`'s code commit was
**correct**. `d4721d8` forbids code in a librarian filing; it says nothing
against grouping repairs in a code commit, and grouping them there is what
keeps the filing docs-only.

See [[librarian-needs-exact-hashes]] and [[run-the-projects-own-gates]].

**★ A THIRD DIRECTION, learned 2026-08-29 — the PUSH, not the commit.**

Everything above is about what goes *in* a commit. The push order is a
separate decision with the same gate behind it, and it now matters more,
because pushing `main` became standing-authorized on 2026-08-27
(decision 090, *"always push"*).

**Instance:** `Pass 167.0` (`d59ce99`) and a preceding defect fix
(`c54f582`) were pushed, then the librarian was dispatched, then the filing
(`dfdfb7e`) was pushed. CI on the first push went **red by construction** —
`check-commits-filed.py` correctly reported `c54f582` as *"in no filing"*,
because at that instant it was. The filing push then went green.

Nothing was wrong with the code, the gates or the filing. **The red run was
a true statement about a state that existed for twenty minutes**, and it
sits in the run history for anyone reading CI colour later.

**How to apply:** when a session produces code commits *and* a filing,
**push once, after the filing commit lands.** Local `run-gates.sh` will
also report the code commit as unfiled in that window — that is the same
true-but-transient signal, not a defect to chase. If a push before the
filing is unavoidable, say so, and check the *next* run rather than
reporting the first one's colour.

The tip commit is exempt (`commits-filed` defers it: *"a commit cannot cite
its own hash"*), so pushing a **single** code commit and filing it next
session is fine. It is the **second** unfiled code commit that turns the
run red.

**★★ 2026-08-30 — THE ADJACENT RULE, AND I BROKE IT HOURS AFTER WRITING IT
DOWN.** This file is about not putting code *into* a filing commit. `R217` is
about the other side: **do not land a code commit ON TOP OF an unfiled one.**

What happened: `Pass 185.1` was pushed and dispatched for filing. While the
librarian worked, I fixed the follow-up defect, committed it as `Pass 185.2`
and pushed — so `185.1` stopped being the tip, lost its deferral, and
`check-commits-filed` went red naming it. CI followed.

Three things worth keeping:

- **I had written the correct rule into `NEXT_SESSION.md` §D that same
  afternoon**, including the sentence *"`R217` constrains what may land on top
  of an unfiled commit"*. Knowing it, and having just corrected a wrong version
  of it, did not stop me. The failure was **momentum**, not ignorance: the fix
  was ready, the gates were green *at that moment*, and the gate only turns red
  at the NEXT commit.
- **`run-gates.sh` was PASS immediately before the commit and the commit itself
  is what made it fail.** So a green sweep is not evidence you may commit — it
  certifies the tree, not the act. That is this file's own thesis pointed one
  step forward in time.
- **The cost is small and self-healing** (the next filing clears it), which is
  precisely why it keeps happening. It is a rule with no teeth at the moment
  you break it.

**How to apply:** after pushing a Pass and dispatching its filing, **the next
code commit waits for the filing commit to land.** If the fix cannot wait,
commit it and dispatch BOTH in one filing — the wrong move is to push the
second one and hope the first is still deferred.

**Second occurrence, 2026-09-02 (`e7b1b7c`), recorded by the librarian and
appended here at its request.** Pass 236.0's two `crates/` fixes were committed
in the SAME commit as the librarian's docs-only filing of Passes 234.0/235.0,
so `check-commits-filed.py` correctly flagged the commit as unfiled -- and it
was pushed anyway because the command chain inspected the gate's exit code
without consuming it. Two shapes in one act: the bundle, and a green-by-eye
push past a red gate. **Stage a filing commit by naming only `docs/` paths;
stage a code commit by naming only the code paths; never let one `git add`
line cover both.**
