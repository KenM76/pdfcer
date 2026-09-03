---
name: git-add-all-is-unsafe-with-live-subagents
description: Never `git add -A` while a subagent is running — their scratch files and their in-flight document edits land in your code commit, and this repo is public
metadata:
  type: feedback
---

**Stage explicit paths. Never `git add -A` in a session with live
subagents.**

**Why:** on 2026-08-21 a `git add -A` swept, into a commit whose message
described a rendering fix:

- **`jina.txt`** — a scraped PDF Association web page, a subagent's working
  file. Third-party content, and `CLAUDE.md` rule 8 makes that non-trivial:
  **`github.com/KenM76/pdfce` is public, so anything committed is published
  by default.**
- **681 lines of `ROADMAP.md`, plus `FEATURES.md`, `SESSION_LOG.md` and
  `ARCHITECTURE.md`** — the librarian's filing, mid-flight. A reader would
  have attributed the librarian's prose to the engineering commit, and the
  librarian was still writing when it was captured.

Caught by reading `git commit`'s own file list. The commit was reset and
re-made from explicit paths; the scratch file was deleted rather than
committed.

**How to apply:** `git status --short` first, then `git add <path> <path>`.
If the list is long enough that typing it is annoying, that is the signal —
a code commit should not touch that many files. The librarian's documents
get their **own** commit, made after the agent reports back, so its work is
attributable to it.

**Related:** the subagent working directory is the repo root, not a temp
dir. Anything a dispatched agent writes lands where your commit will find
it.

**★ IT RECURRED ON 2026-08-22, and the reason it did is the useful part:
the librarian was NOT LIVE.** It had reported back, complete, some time
earlier — and had **written its documents without committing them**. So
`git status` was quiet in the way a finished agent looks quiet, and
`git add -A` swept **+808 `ROADMAP.md`, +233 `SESSION_LOG.md`, +236
`NEXT_SESSION.md`, +39 `ARCHITECTURE.md`** into a commit about
`--no-annotations`.

The rule above says *"with live subagents"* and *"made after the agent
reports back"* — and both of those were satisfied. **The hazard is not the
agent running; it is uncommitted work in the tree that is not yours**, and
a finished agent leaves exactly that.

**Nothing went red.** `check-commits-filed.py` counts *code* commits, and
this was one, correctly. No gate can see it. It was caught by the
librarian reading `git show --numstat` on the commit it was asked to file
— a second reader, after the fact.

**What it costs is attribution, in both directions.** `git show <hash>` no
longer isolates that filing, and `git log -- docs/ROADMAP.md` credits an
engineering commit with 808 lines of librarian prose. It also inverts
`d4721d8`'s rule — that one forbids *code inside a filing*; this is a
*filing inside code*, same damage, opposite direction.

**How to apply, with the scope taken off:** run `git status --short`
before every commit and stage explicit paths — **not because an agent is
running, but always.** If `docs/` appears in the list of a code commit and
you did not edit `docs/`, a librarian left it there: commit that
separately, first, with a `librarian:` subject.
