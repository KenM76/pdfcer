---
name: never-type-a-reverting-git-verb-in-a-chain
description: TWICE (2026-09-02, 2026-09-06) a `git checkout -- <file>; echo "NO - do not checkout"` typed into a chain reverted uncommitted work; the sabotage script must contain its own revert so no restore step exists to be tempted into
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

**★ RECURRED 2026-09-06, CHARACTER FOR CHARACTER.** Pass 257.0 sabotage on
`edit.rs`: I appended `git checkout -- crates/pdfcer-core/src/edit.rs
2>/dev/null; echo "NO — do not checkout"` to the test command — the same
verb, the same self-talking echo, the same file class (the Pass's main edit
target, uncommitted). It reverted every `edit.rs` change of the Pass. Saved
again only because the edits had been scripted; the re-apply took one run.
Reading this memory earlier in the session did not prevent it, because the
memory is read at session start and the mistake is made at the sabotage
moment. So the rule sharpens to something structural:

- **The sabotage and its revert are ONE Python script**, applied and unapplied
  by the same asserted `replace` — flip, run the test, flip back. There is no
  separate "restore" step, so there is nothing to type a git verb into.
- **Any Bash command containing `git checkout`, `git restore`, `git reset` or
  `git clean` is typed alone**, as the only command in the call, after
  `git diff --stat -- <path>` in the previous call. A PreToolUse hook that
  refuses these verbs inside a `;`/`&&` chain is the durable fix; proposed to
  Ken 2026-09-06.
