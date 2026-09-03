---
name: feedback_cross_target_check_the_cli_before_pushing
description: A Windows-only dependency block can hide a compile break on every other target; run clippy for the Linux target locally before pushing a CLI change that touches cfg(windows)
metadata:
  type: feedback
---

Before pushing any change that adds or moves a dependency under
`[target.'cfg(...)'.dependencies]` in `pdfcer-cli`, run
`cargo clippy -p pdfcer-cli --all-targets --target x86_64-unknown-linux-gnu -- -D warnings`
(and `cargo check --target aarch64-apple-darwin`). Both targets are installed
here and the check costs under a minute.

**Why:** 2026-09-03, v0.29.0 — `thiserror` was declared only under the
`cfg(windows)` block while `ClipboardError` derived it everywhere. Every
local gate was green (Windows), CI went red on ubuntu/macOS/clippy, and the
tag `v0.29.0` had already been pushed and deployed to OneDrive before CI
answered. The fix was one line; the cost was a dead tag and a second
release (v0.29.1). `run-gates.sh`'s wasm32 check covers core+render only —
nothing local crosses targets for the CLI.

**How to apply:** treat "clippy green" as "clippy green on the target I'm
on". Any `cfg(windows)` edit is the trigger. And do not deploy/tag until
CI has answered when the change is target-gated.
