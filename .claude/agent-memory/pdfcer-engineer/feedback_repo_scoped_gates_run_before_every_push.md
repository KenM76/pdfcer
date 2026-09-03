---
name: repo-scoped-gates-run-before-every-push
description: A gate whose subject is the REPOSITORY (not the build) must run before every push, including docs-only pushes; standing push authorization removed the pause that used to enforce this
metadata:
  type: feedback
---

**A gate that guards the *repository* runs before every PUSH, not before
every build.** In pdfce that is `tools/check-suite-name-absent.py`; the
same shape applies to anything checking what the public repo contains.

**Why:** 2026-09-02 I pushed the licensed conformance suite's real filename
into the **public** repository, inside `docs/NEXT_SESSION.md`. The gate had
been run before every *code* commit that session and was clean each time.
It was never run before the two *docs* commits, because the gate had become
associated in my head with **shipping code** rather than with **publishing
anything** — and a handoff document is not code.

Two compounding factors, both worth carrying:

1. **Pushing `main` became standing-authorized (decision 090, 2026-08-27).**
   That removed the pause that used to make someone stop and think before a
   push. This was the first observed cost of that change. The authorization
   is correct and should not be re-litigated — the fix is to make the scrub
   mechanical instead of remembered.
2. **The gate reads untracked files and your own commit message.** Both
   tripped while I was writing the incident report *about* the leak. An
   explanation of a leaked string is itself a place the string leaks, and
   quoting it to describe the mistake is not an exemption from the rule the
   mistake broke. Mask it or describe it without reproducing it.

**How to apply:** `python tools/check-suite-name-absent.py && git push`.
Before **every** push, including docs-only and handoff-only ones. Run it
again after `git add` and again after writing the commit message, because
staged content and the message are both in scope.

★★ **AND AS OF 2026-09-02, RELEASING IS STANDING-AUTHORIZED TOO** —
decision 121, from *"always go ahead and push the latest one."* That removes
the last per-act pause in the publish path: pushing was already standing,
and now cutting a tag, packaging and deploying to OneDrive are as well.

⇒ **The scrub matters more, not less, again.** Every remaining stop between
a keystroke and the public repo is now a gate I choose to run. Nothing will
ask me. The authorisation covers the ACT, explicitly not skipping the gates:
a release still owes a green `tools/run-gates.sh`, a fresh-folder smoke test
and `verify-release.py`.

**What it does not fix:** the name is in published history and stays there.
Removing it means rewriting published history, which is Ken's call
(project rule 8, `ROADMAP.md` open question `(ca)`) and which this project
has direct evidence breaks every document citing a commit hash. Never
force-push to resolve one of these — report it and let him rule.

Related: [[never-bundle-code-into-a-filing-commit]],
[[a-gate-sweep-certifies-the-tree-it-ran-on]],
[[run-the-projects-own-gates]].

**RECURRED 2026-09-03, by a pipe.** `python tools/check-suite-name-absent.py | tail -1 && git push`
— the `&&` saw `tail`'s exit code, not the gate's, so a docs commit carrying a
temp-folder path with the licensed name was pushed (`c180aac`), then scrubbed
in `bd40ffa`. The line is now in published history. Rule: the gate runs
BARE before `&& git push` — never through a pipe — or run it, read its
last line, and push in a SEPARATE command.
