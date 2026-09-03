---
name: project-pass-166-onedrive-deploy-and-r229
description: Pass 166.0 (6dbe953) shipped tools/deploy-onedrive.py + standing rule R229; also caught a dispatch's false "already filed" claim about v0.16.0 (921ac2a) per R228 and filed it properly.
metadata:
  type: project
---

**2026-08-29, 325th filing.** `Pass 166.0` (`6dbe953`) shipped
`tools/deploy-onedrive.py` — every release publishes `pdfce-cli.exe` + OCR
models + licence/readme files to whichever of two OneDrive slots
(`pdfce1`/`pdfce2`) has the OLDER `VERSION.txt` (derived, never remembered
as state), refusing a same-version re-deploy without `--force` (the guard
that matters more than the alternation — a double-deploy would destroy the
previous-version property the whole scheme exists for). `verify-release.py`
gained two checks: current tag deployed, AND a previous version still
present in the other slot (the second is the operator's actual ask; the
first alone is a naive pass). **New standing rule R229** codifies this as
binding on every future release, not just this one — check `ROADMAP.md`'s
Standing rules section before assuming a future release skipped it.

**Ledger after this filing:** Pass ceiling `166.0` (next free `166.1`/major
`167.0`); standing rules `R229` (next free `R230`); decisions unchanged at
`098` (next free `099` — no decision minted, this is release tooling not an
architectural choice); filing ordinal `325`.

**★ R228 fired on this filing's own inbound dispatch.** The dispatch that
asked me to file Pass 166.0 also claimed `v0.16.0` (`921ac2a`) "is already
filed in the 324th filing." Checked directly (two greps) before repeating
it: the 324th filing only named `v0.16.0` under "For next session" as a
forward-looking note ("the operator is cutting v0.16.0 immediately... no
action needed here") — never as a Shipped entry, never with the hash
anywhere in `ROADMAP.md`. The claim was false and checkable in under a
minute. Filed the `v0.16.0` release properly (honestly thin — no fuller
detail than the hash was supplied), flagged the gap back rather than
inventing detail. **Lesson: an engineer/dispatch's own "already filed"
claim about ROADMAP/SESSION_LOG state gets the same R228 treatment as any
other inherited characterization — it is not exempt just because it comes
from the same session that's asking me to file something else.**

See [feedback_dispatch_should_carry_git_evidence_when_no_shell](feedback_dispatch_should_carry_git_evidence_when_no_shell.md)
— this is a related but distinct failure: not missing evidence, but an
actively wrong claim about document contents, which R228 exists to catch.
