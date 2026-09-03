---
name: never-rm-rf-a-directory-you-only-assumed-you-created
description: mkdir -p then rm -rf on the same path destroys pre-existing siblings; delete the file you made, never the folder you put it in
metadata:
  type: feedback
---

**Delete the file you created, never the directory you put it in.**
`mkdir -p X && write X/mine.rs && ... && rm -rf X` destroys everything else in
`X` — and `mkdir -p` succeeding tells you nothing, because it succeeds whether
the directory existed or not.

**Why:** 2026-08-29, I wrote a throwaway debug binary to
`crates/pdfce-core/examples/dbg_paste.rs` with `mkdir -p`, then cleaned up with
`rm -rf crates/pdfce-core/examples`. That folder already held two committed
probes (`gray_equivalence_probe.rs`, `orphan_probe.rs`). **Nothing noticed**:
the build was green, `cargo test` was green, clippy was clean — examples are
not compiled by any of them. It surfaced only as two ` D ` lines in
`git status` while staging, and was restored with `git checkout --`.

The near-miss is the lesson, not the recovery. Had the deletion been staged in
the same `git add` as the feature, it would have been committed and pushed as
part of an unrelated change, with a commit message that did not mention it.

**How to apply:** for scratch artefacts inside the repo, `rm` the exact file
(`rm -f crates/pdfce-core/examples/dbg_paste.rs`). Better: put scratch work
outside the tree entirely — this repo is public
([[feedback_git_add_all_is_unsafe_with_live_subagents]]), so the temp-folder
convention is load-bearing rather than tidy. And **read `git status --porcelain`
before every `git add`**, looking for ` D ` lines you did not intend, not just
for the files you meant to stage.
