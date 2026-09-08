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

## ★ THE STANDING EXCEPTION: when pdfcer-gui is WAITING TO TEST, cadence beats batching

**Ken, 2026-09-08 (00:15):** *"you can keep going on tasks after 2am, I just
want to be sure there are **regular releases** after that time so that I have
something to test in the GUI by 6:15 am."*

**When he names a time he wants to test by, the release cadence is a
DELIVERABLE, not a convenience** — and it outranks the batching rule above.
Batching optimises *this* machine's build cost; a testable artefact at an hour
he named optimises *his* time, and his time is the scarcer one.

**How to apply:**
- Ask what he is waiting for. A deadline like *"something to test by 6:15"*
  means **plan backwards from it**: pick release points that guarantee a
  usable artefact before the hour, not "whenever the batch feels done".
- Prefer **several smaller releases** over one large one in this mode, even
  though each costs a fought-for build. A release that lands at 06:30 for a
  06:15 deadline is worth nothing.
- **`pdfcer-gui` builds against the local path**, not a published crate (see
  [[project_gui_request_channel]]), so *pushing* already unblocks their
  compile. The **release** is what gives them a pinnable version and a
  portable CLI to check behaviour against. Do both; do not assume a push
  substitutes for the release he asked for.
- The batching rule resumes as the default the moment no such deadline is
  outstanding.

Relates to [[project_onedrive_cli_slots]] (the release mechanics this defers)
and [[project_gui_request_channel]] (who is waiting, and why they can build
from `main` before a release exists).
