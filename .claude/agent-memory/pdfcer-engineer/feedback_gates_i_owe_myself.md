---
name: gates-i-owe-myself
description: The fuzz-target gate is the one I skip; and a harness that enumerates a module's entry points goes stale every time the module grows
metadata:
  type: feedback
---

Two gates are easy to declare met without meeting them. Both bit on 2026-08-05.

**1. The cargo-fuzz gate is the one that gets skipped.** ARCHITECTURE.md §10.2
and the engineer role's "Always" list both require: new code touching
untrusted-input parsing extends a `cargo-fuzz` target. On Pass 36.1 I shipped
`plan_delete_node` with 10 fixture tests, ran fmt/clippy/tests/ui-strings/
ledger/cargo-tree, and reported the Pass complete — **without the fuzz arm.**
The librarian caught it, not me. Fixture tests feel like enough; they are not
the gate that was asked for.

**Why:** the other gates are one command with a pass/fail line. Fuzzing needs a
DLL on PATH ([[fuzz-asan-dll]]), a minute of runtime, and a judgement about
which branches to drive — so it reads as optional in a way `cargo clippy`
never does. It is not optional.

**How to apply:** before declaring ANY Pass done that adds a function taking
parsed-from-file data, grep the fuzz targets for the module. If the module is
listed and the new entry point is not, that is the gate, unmet.

**2. A fuzz/test harness that enumerates a module's entry points goes stale
silently.** Adding the owed arm exposed that `fuzz/fuzz_targets/vector_edit.rs`
still drove only the three Pass 9c-min planners it was written for:
`plan_delete_subpath` (Pass 25.2), `plan_move_subpath` (Pass 28.0) and
`plan_move_handle` (Pass 30.1) had **never been fuzzed at all**. Three Passes
each added a planner beside them and none extended the target.

This is the same family as R151 (a `pub fn` with no caller) and R152 (a caller
that confirms nothing): the harness has no way to complain about what it does
not mention. The check is cheap — a planner in the module's public surface that
appears in no fuzz target is findable by grep.

**How to apply:** when extending a harness, do not just add your own arm — diff
the harness's list against the module's current public surface. The gap will be
older than your change.

---

## ★ AMENDED 2026-08-21 — BOTH RECURRED, sixteen days later, IN THE SAME FILE

This memory was correct and did not prevent either recurrence. That is
worth more than the recurrences themselves, and it is the warrant the
librarian minted **`R209`** on.

**1 again.** `cargo fuzz build` had been **red for three days** on a
one-line compile break — `MarkupSpec::Square` gained a `border_effect`
field (`Pass 82.0`) and `fuzz/fuzz_targets/annot_author.rs`, which
constructs that variant, was not updated. Found while verifying CI before
tagging `v0.7.0`, not by any local run.

★ **The mechanism this memory missed, and it is not carelessness.** "Run
the gates" in this project means `for g in tools/check-*`. That is
**sixteen** scripts and **it is ONE of CI's nine jobs.** A green local sweep
and a red CI were **never a contradiction** — there was simply no place
where the two were compared. Grepping the fuzz targets, which the "how to
apply" above tells you to do, would not have caught a *compile* break
either.

**2 again.** Reading the file to fix the compile error found the dispatch
at `match c.byte() % 8` against an **eight-variant enum with one arm spent
twice**, so `MarkupSpec::Cloud` had **never been fuzzed at all**. Exactly
the `vector_edit.rs` shape, in a different target.

★★ **And this paragraph contained one of its own, which is the best
evidence in it.** It said *"that is **fourteen** scripts"* — `ls tools/check-*`
is **sixteen**, and had been since earlier that same day. Worse, **14 was the
CI-WIRED count one commit earlier**, so the number inverted the sentence's own
point: the local sweep is the **larger** set, not the smaller one. And the same
commit message carried "fourteen scripts" and "all 15 CI-wired gates green"
thirteen lines apart, **neither labelled** — inside the one carrier that cannot
be amended afterwards. Found by the librarian, not by me, in the amendment that
names this defect class.

★ **And a sharper form of the check:** the modulo **is** a coverage claim,
phrased as an integer. A claim asserting a **boundary** rather than a
**member** — a count, a modulo, an absence, a closed either/or — contains
no token tying it to what changed, so no grep for the new thing finds it.
`% 8` and *"…are **not** counted as `overprint_refused`"* are the same
defect in two spellings.

**How to apply, updated:**

* **`python tools/check-ci-parity.py --list`** prints the local stand-ins
  for every CI job. The fuzz one is **`cd fuzz && cargo check --bins`** —
  six seconds, no nightly, no ASan, and it catches the entire class that
  has ever broken that job.
* When you touch a fuzz target, **diff its dispatch arity against the
  enum**, not just its arms against your change.

---

## THIRD RECURRENCE, 2026-08-25 -- AND THE STAND-IN THIS FILE RECOMMENDS CANNOT SEE IT

The whole fuzz harness had been **unbuildable on this machine for weeks**
and nothing said so. `pdfce-core`'s default features include `ocrs`, which
pulls the `rten` inference runtime; `rten` declares
`crate-type = ["lib", "cdylib"]`, and cargo-fuzz applies libFuzzer's
`/include:main` to every link in the graph, so the cdylib is asked to export
a `main` it does not have. Every target in the directory died with
`LNK2001: unresolved external symbol main`. Confirmed pre-existing by
building `parse_object`, untouched since long before OCR landed.

It surfaced only because a new Pass added a target and I tried to build it.

**★★ AND MY FIRST EXPLANATION FOR WHY NOBODY NOTICED WAS WRONG, WHICH IS THE
MORE USEFUL HALF.** I wrote — into a commit message, a filing and this file —
that *"`cargo fuzz build` is not one of CI's jobs"*. **It is.** The job is
`fuzz targets build (nightly)`, it runs `cargo +nightly fuzz build`, and it
was **green** at the tip the day before. Checked with
`gh run view <id> --json jobs`; I had asserted it from the shape of the
problem instead.

**The real reason is better: the job runs on `ubuntu-latest`, and the break is
MSVC-only.** The same cdylib on Linux is a `.so`, and no linker asks a `.so`
for a `main`. So CI was not asleep — it was **structurally incapable** of
seeing this, and the local build was the only place it could surface.

⇒ **The usual assumption is that CI is the stricter of the two and a green CI
absolves a red local. Here it was exactly backwards.** A single-platform job
is single-platform *evidence*, and the platform it does not cover is the one
the code is written on.

⇒ And note how close I came to filing the wrong lesson permanently: the
correction arrived only because I read `ci.yml` for an unrelated reason while
cutting a release. **Before asserting that a check does not exist, open the
file that would contain it.**

**The new information, and it is a correction to the "how to apply" above:**

> `python tools/check-ci-parity.py --list` names the local stand-in for the
> fuzz job as **`cd fuzz && cargo check --bins`**, and this memory recommends
> it as the cheap catch-all.

**A/B'd on 2026-08-25: `cargo check --bins` passes in BOTH states** -- with
the broken feature set and with the fixed one. `cargo check` **never links**,
and the break is a *link* break. So the stand-in is a compile check wearing a
build check's name, and it is structurally incapable of catching the entire
class this recurrence belongs to.

★ Note the shape rather than the fix: the previous amendment identified "the
local sweep is not CI's set" and prescribed a stand-in. The stand-in was
cheaper than the real thing **because it did less**, and the thing it did not
do is exactly where the next failure lived. **A cheap proxy for a gate is a
proxy for the part of the gate that is cheap.**

**How to apply, corrected:** for the fuzz job the only honest local check is
`cargo +nightly fuzz build <target>` -- about four minutes warm, and it must
be run at least once per session that touches `fuzz/` **or any crate's
Cargo.toml**. A dependency change is a fuzz-harness change, which is not
obvious and is how this one landed.

---

Related: [[run-the-projects-own-gates]] (the gate set is wider than fmt/clippy/
tests), [[fuzz-asan-dll]] (why running one is not one command on this machine).
