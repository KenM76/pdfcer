---
name: onedrive-cli-slots
description: Ken, 2026-08-29 — every new version publishes the CLI (not the GUI) to OneDrive, alternating pdfce1/pdfce2 so a previous version is always available; run tools/deploy-onedrive.py, verify-release.py enforces it
metadata:
  type: project
---

**Every release publishes `pdfce-cli.exe` to OneDrive, alternating between
`~/OneDrive/pdfce1` and `~/OneDrive/pdfce2`.** Run
`python tools/deploy-onedrive.py` after `tools/package-portable.py`.

His words, 2026-08-29:

> *"can you always put a new version on onedrive? cycle between folders pdfce1
> and pdfce2 when you make new versions so there is always a previous version
> available. Just need the CLI tool available."*

**Why:** he wants a working CLI on hand without waiting for a build, **and a
fallback when a new one misbehaves**. The two folders are a rollback, not a
backup — which is why the *previous* one is the load-bearing half.

**How to apply:** it is wired, not remembered.
`tools/verify-release.py` fails a release that did not deploy, and checks the
operator's actual property — *this version in one slot, a **different** version
in the other*. Both slots holding the same version passes a naive
"is it deployed?" test and fails what he asked for.

## Three decisions inside it, each with a reason worth keeping

- **CLI only, but models included.** *"Just the CLI tool"* means **not the
  GUI** — it does not mean a crippled CLI. Without `models/ocrs` the binary
  refuses OCR by name and explains itself (it does not crash), so omitting them
  would be *honest*; including them is *useful*. ~30 MB per slot.
- **The alternation is DERIVED from the folders, never stored.** Each slot
  carries a `VERSION.txt`; the script writes to whichever is **older**. A
  "last used" marker would break silently the first time anything happened
  outside the script — a manual copy, a half-finished deploy, a folder restored
  from OneDrive's own history — by overwriting the copy he was relying on.
  Missing/unreadable `VERSION.txt` counts as infinitely old, so a damaged slot
  is refilled first.
- **★ The double-deploy guard matters more than the alternation.** Deploying
  the same version twice puts it in **both** slots and destroys the previous
  version. Two populated folders with recent timestamps *look* healthy while
  the property is gone. Refused unless `--force`.

## Watch for

The failure mode is **not** "the deploy didn't run" — that is loud. It is
**both slots holding the same thing**, which is quiet and looks fine. That is
the case `verify-release.py`'s second check exists for; do not delete it as
redundant with the first.

Related: [[feedback_launch_on_completion]] — same instinct from him, wanting
the working artefact in his hands rather than a report about it.
