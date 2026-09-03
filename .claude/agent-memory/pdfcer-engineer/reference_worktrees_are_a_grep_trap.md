---
name: worktrees-are-a-grep-trap
description: .claude/worktrees/ accumulates full repo copies (hit 28 GB before being cleared 2026-08-29) — any recursive grep/find/du from the pdfce repo root must exclude it or it hangs
metadata:
  type: reference
---

> **★ CLEARED 2026-08-29** on Ken's instruction (*"clean up the scratch
> copies"*): 7 worktrees and 10 leftover `worktree-agent-*` branches removed,
> `.claude/` down from **28 GB to 1.4 MB**, `git worktree list` showing only
> `main`. **The directory refills** — every `isolation: "worktree"` agent run
> adds another full copy — so the trap below is dormant, not gone. Re-check
> with `du -sh .claude` when a recursive command starts feeling slow.
>
> Cleanup that worked: `git worktree remove --force <path>` per entry (it
> deregisters but **fails to delete the files** — "Permission denied", the
> usual Windows read-only attributes on git objects), then **`rm -rf
> .claude/worktrees`**, then `git branch -D` the `worktree-agent-*` branches,
> then `rm -rf .git/worktrees && git worktree prune`.

`D:\Dev\pdfce\.claude\worktrees\` contains **full working copies of the repo**,
one per `isolation: "worktree"` agent run, each with its own `target/`. Measured
2026-08-29 before clearing: **7 worktrees, 28 GB.**

**They are gitignored** (`.gitignore:34`, `/.claude/worktrees/`) and
`git ls-files .claude/worktrees` returns **0**, so they are no risk to the
repository. The cost is entirely operational.

## The trap

Any recursive command rooted at the repo that does not exclude them will walk
28 GB of duplicated source and build artefacts:

```bash
grep -rn "needle" docs/ .claude/     # ← hung, 2-minute timeout, 2026-08-29
find . -name "*.pdf"                 # ← returns worktree copies, not repo files
```

The `find` case is the sneakier one: it *succeeds* and returns paths under
`.claude/worktrees/...`, which look plausible. On 2026-08-29 a search for
`format_family.pdf` returned **only** worktree copies and none from
`fixtures/`, which reads as "the fixture moved" when it had not.

**How to apply:** scope recursive searches to real subdirectories
(`crates/`, `docs/`, `tools/`, `fixtures/`), or exclude explicitly:

```bash
grep -rn "needle" . --exclude-dir=.claude --exclude-dir=target
find . -name "*.pdf" -not -path "./.claude/*" -not -path "./target/*"
```

Prefer the **Grep tool** over shell `grep -r` here — it respects ignore files.

## Deleting them is Ken's call, not mine

`git worktree list` registers all seven, each pinned to a commit from
2026-08-04 … 2026-08-18. Four carry a single untracked `docs/decisions/NNN-*.md`
and one carries two — **all six of those files were verified present in the main
repo already**, so nothing would be lost. But 28 GB is a destructive operation
outside the working tree, so **flag it, do not run it**.

The cleanup, if he says yes:
`git worktree remove --force <path>` per entry, then `git worktree prune`.

Related: [[feedback_git_add_all_is_unsafe_with_live_subagents]] — same
directory, different hazard (that one is about scratch files reaching a commit;
this one is about search results and wall-clock).
