---
name: write-overwrites-check-the-verb
description: The Write tool says "updated" not "created" when a file already exists — that one word is the only warning before you silently delete committed work
metadata:
  type: feedback
---

**Before `Write`-ing a "new" file, check whether it already exists.** The tool's
result says **"has been updated successfully"** for an existing file and
**"File created successfully"** for a new one. That single word is the only
signal, and it appears *after* the content is already gone.

**Why:** 2026-09-01. I wrote `crates/pdfce-render/tests/shading_ink.rs` as a
"new" test file. It already existed and held **three tests from `Pass 137.0`** —
a shading-vs-fill ink-agreement suite with a long doc comment recording why the
defect stayed invisible. I destroyed all of it. In the same minute I did it
again to `tools/gen-shading-ink-fixtures.py`, the generator for two fixtures
that suite depends on — so the tests *and* their inputs went together.

**What actually caught it, and what did not.** Not the tool result, which I read
past twice. Not `cargo test`, which reported **0 failures** — the file I wrote
compiled and passed. It surfaced only because the total pass count fell by
**one** while I had *added* two tests, a −3 discrepancy I nearly waved through
as noise. `git diff --stat` then showed `250 +++/---` with **146 deletions** on
a file I believed I had created.

⇒ **An unexplained count is the whole warning.** A green suite is compatible
with having deleted a different green suite.

**How to apply:**
- `ls` or `git log -- <path>` before `Write` on anything you think is new. A
  plausible name is not evidence of absence — `shading_ink.rs` is exactly what
  the existing file would be called, which is *why* I picked it.
- **Read the verb in the tool result.** "updated" on a supposedly new file means
  stop and `git diff` before doing anything else.
- Prefer **appending** to an existing test file over replacing it, and reuse its
  helpers — the restored file's `render()` and `RenderedPage` idiom were better
  than the ones I had written anyway.
- Recovery is cheap *if the file was committed*: `git checkout -- <path>`, then
  re-apply your addition. Save your version aside first.
- ★ This is [[never-rm-rf-a-directory-you-only-assumed-you-created]] in a second
  carrier. That one was `mkdir -p` + `rm -rf` eating two committed files; this is
  `Write` eating two more. **Same root: acting as if creating, when the thing
  already existed.** Two carriers now, so treat the class as established rather
  than the instance.

Related: [[count-what-committed-not-what-you-intended]] — the discrepancy that
exposed this was a count, and I almost dismissed it.
