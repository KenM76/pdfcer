---
name: a-gate-sweep-certifies-the-tree-it-ran-on
description: A hand-run check can be weaker than CI's by a gate, a flag or a target set; and check-commits-filed's answer is a property of the current tip, so run it AFTER your last commit
metadata:
  type: feedback
---

**A clean gate sweep certifies the tree it ran on, not the tree you push.**
Re-run the filing gates as the LAST act before pushing, after the final
commit exists.

**Why:** 2026-08-26, `v0.14.0`. I ran all 17 `tools/check-*` green, then made
two more commits (a prose correction, then the version bump), tagged, pushed,
and cut the GitHub release. CI came back red — one job of ten — because
`check-commits-filed.py` saw the prose-correction commit in no filing. The
release had to be re-tagged onto the filing commit and the asset rebuilt.
Nine of ten jobs were green; the failure was entirely self-inflicted ordering.

**The structural part, which is the bit worth carrying:**
`check-commits-filed.py` cannot be green on a code commit made *after* the
filing that was supposed to narrate it. A commit cannot cite its own hash, so
its filing is always a *later* commit. Only two orders work:

- `code → file` and then STOP, or
- `code → file → code → file` (every code commit gets a filing after it).

There is no order in which "file, then commit more code, then push" is green.
The tip commit is deferred (checked on the next run), which makes the version
bump itself safe — but anything *behind* the tip is checked in full.

**How to apply:** before `git push` on a release, run
`python tools/check-commits-filed.py` and `check-passes-filed.py` again,
right then, with nothing uncommitted and nothing left to commit. If they name
a commit, dispatch the librarian BEFORE pushing — filing after the push means
the tag points at a commit CI will reject, and moving a public tag is a fact
that then has to be recorded too.

---

## ★★ RECURRED 2026-08-27, IN THE FORM THE RULE ABOVE DOES NOT NAME

CI went red on `4c32afe`. Nothing was wrong with it: `check-commits-filed`
named `51c30d6`, the commit *behind* it, which was still unfiled.

**The sharpening: pushing TWO commits in a row with no filing between them
makes the first one permanently red.** The tip is deferred — so pushing one
unfiled commit is safe, and I had internalised that as "pushing before filing
is fine". It is fine exactly once. The moment a second commit lands on top,
the first stops being the tip and gets checked in full, and the red run on it
is now permanent history on a public repository.

**Why I did it:** decision 090's *"always push"* removed the pause that used
to make me check. It grants the push; it does not grant pushing **twice**
before the librarian has run. That is not a narrowing of Ken's ruling — it is
a fact about a gate, and the ruling was never about gates.

⇒ **One unfiled commit may sit at the tip. Never two.** If a second is ready
and the first is unfiled, dispatch the librarian first, or commit the filing
before pushing either.

The self-correcting part is a trap of its own: `HEAD` goes green on the next
run, the tree is clean, every local gate passes, and the only surviving
evidence is a red run on an intermediate commit that nobody will look at
again. It costs nothing *now* and it is exactly the kind of thing that makes
"CI is green" stop meaning anything.

---

## ★★★ RECURRED AGAIN 2026-08-27 — THREE unfiled commits, and the real cause was the SWEEP, not the ordering

CI went red on `0c48bbf`. Three code commits (`2e6235c`, `cfa2c44`,
`0c48bbf`) had been pushed with no filing anywhere behind them, so the two
below the tip were checked in full and both failed. The rule above says "never
two" and I pushed three.

**But the diagnosis "I forgot the ordering" is wrong, and the true one is
worth more.** I ran a THIRTEEN-GATE SWEEP green before every push that
session — and `check-commits-filed.py` and `check-passes-filed.py`, the two
gates whose entire job is this ordering, **were not in it.** The sweep was
hand-typed from memory into a `for g in ...` loop.

`python tools/check-ci-parity.py --list` had been printing the authoritative
list the whole time. Running it showed the hand-typed sweep omitted **five**
commands, not two: both filing gates, `check-outcome-disclosed.py`,
`check-ci-parity.py` itself, `cargo test -p pdfce-core
--no-default-features`, and `cd fuzz && cargo check --bins`.

⇒ **A SWEEP THAT OMITS A GATE IS BYTE-INDISTINGUISHABLE FROM A GREEN ONE.**
Same shape as [[a-gate-that-underreports-looks-green]], one level up: at the
SET rather than at the member. Every mitigation I had was aimed at remembering
the ordering; none was aimed at the sweep being complete, and the sweep was
the thing that failed.

**The fix is on disk: `bash tools/run-gates.sh`.** It derives its list from
`check-ci-parity.py --list` (which derives it from the workflow), runs the
filing gates LAST by construction, names anything it skips, and does not stop
at the first failure. **Use it. Do not hand-type a gate loop again** — that is
the habit, not the ordering.

★ And running the real list found a defect in the list on its first run: the
`cargo tree` denylist command had an **inverted exit sense** (`grep` exits 1
when it finds nothing, which is the passing condition), so it reported failure
on a healthy tree and success on a violated one. It survived because nothing
had ever *run* the list — it was read and retyped. Giving a documented list a
consumer is what turns its errors into red lines.

★★ **FOURTH RECURRENCE, 2026-09-02, and a NEW WAY to hand-run a weaker
check.** This time the omission was not a missing gate but a missing FLAG:
I ran `cargo clippy --workspace --all-targets` by hand. **CI runs
`--all-targets --all-features`**, and the warning lived in a feature-gated
path, so my run was clean and CI was red. `run-gates.sh` runs the
`--all-features` form; my retyped one did not.

⇒ The habit is not "type the gate names correctly", it is **do not retype the
command at all**. A hand-typed invocation can differ from CI's by a gate, by
a flag, or by a target set — and all three failure modes look identical from
here: a green local run and a red remote one.

★ **And a fifth, same day, on the ORDERING half:** `check-commits-filed.py`
DEFERS the tip commit, so it answered `exit=0` — and then committing a
*different* Pass's filing into the same batch moved the tip, un-deferred the
code commit behind it, and flipped the gate to `exit=1` between my check and
my push. **Its answer is a property of the current tip.** So "run it
immediately before pushing" is not enough: run it **after the last commit you
intend to push**, never before.

Related: [[feedback_never_bundle_code_into_a_filing_commit]] (the same gate,
the opposite mistake), [[feedback_gates_i_owe_myself]],
[[feedback_run_the_projects_own_gates]],
[[feedback_repo_scoped_gates_run_before_every_push]].
