---
name: never-type-a-reverting-git-verb-in-a-chain
description: A `git checkout -- <file>` typed into a command chain as a "no-op" reverted an hour of uncommitted work; reverting verbs never go in chains, and every multi-line edit stays in a script file until committed
metadata:
  type: feedback
---

**Never put `git checkout -- <path>`, `git restore`, `git reset --hard` or
`git clean` in a command chain, and never type one "as a no-op" or "to be
safe".** If a revert is genuinely wanted, it is its own command with its own
line, run alone, after `git diff --stat` has shown what it will destroy.

**Why:** 2026-09-02, after a sabotage-and-revert cycle on the group-merge half
of Pass 239.0, I appended `git checkout crates/pdfce-render/src/cmyk_buffer.rs
2>/dev/null; echo "NO - do not checkout"` to a test command — the echo was
me talking myself out of it while the shell ran it anyway. It reverted every
uncommitted change in that file (the entire knockout/group spot-plane work).
It was recoverable in one command ONLY because the edit had been applied from
a script file under `D:\Dev\temp\` that could be re-run; a heredoc edit would
have been gone. Same shape as the `rm -rf` lesson: a destructive verb costs
nothing to type and everything to undo.

**How to apply:**
- Sabotage/revert cycles: the revert is `python <script> revert` that puts the
  exact original text back, never a git verb.
- Keep every non-trivial edit in a re-runnable script under `D:\Dev\temp\`
  until it is committed. That is what saved this one.
- Before any `git checkout`/`restore` on a tracked file: `git diff --stat --
  <path>` first, read the number, then decide.
