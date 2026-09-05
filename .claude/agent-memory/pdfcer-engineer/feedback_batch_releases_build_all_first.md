---
name: batch-releases-build-all-first
description: Accumulate all pending fixes/features and cut ONE portable release per batch — do not release per-item — unless Ken says otherwise
metadata:
  type: feedback
---

Ken, 2026-09-05: **"build all before the next portable release unless I say
otherwise."**

**Rule:** do not cut a portable release after every single fix/feature. Build
the pending batch — commit and push each piece (CI validates on push), keep
`main` green and public — and cut ONE portable release covering the whole batch.
Release per-batch, not per-item.

**Why:** this machine has ~4.4 GB free RAM, so every release build is a slow,
fought-for `codegen-units=1` compile that the OS reaper keeps killing (see
[[project_onedrive_cli_slots]] and the `0xC0000142` OOM lesson in
`D:/dev/rag/rust`). A release per fix meant re-fighting that build three times in
one session (v0.38.0, v0.39.0). Batching amortises it.

**How to apply:**
- Keep committing + pushing individual fixes/features to `main` (standing push
  authority, decision 090). CI is the per-commit correctness backstop.
- HOLD the release act (version bump → tag → package → gh release → OneDrive →
  verify → release filing) until the batch is complete, or until Ken says to
  release.
- The standing "always make the newest available in a portable release"
  directive still holds — this narrows its CADENCE (batch, not per-item), it
  does not cancel it. When the batch ships, it ships as a portable release.
- "unless I say otherwise" — a specific "cut it now" overrides this for that
  release.

Relates to [[project_onedrive_cli_slots]] (the release mechanics this defers).
