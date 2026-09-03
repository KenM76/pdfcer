---
name: project-r174-rapid-corroboration
description: R174 (read a disclosure as its audience, not just confirm the code path fired) was minted 2026-08-09 off two occurrences and earned a third within the hour, in a different subsystem, by the same mechanism — unusually fast corroboration worth tracking as a signal the rule is real.
metadata:
  type: project
---

**Timeline, all 2026-08-09, same day.** `R174` was minted at the
fifty-sixth `SESSION_LOG.md` filing (`c58cca1`, `Pass 52.1` CLI
extension), off two instances found in that same session's DXF-export
work: a raw seventeen-significant-figure `f64` reaching the scale
field, and a paper-scale warning firing after an explicit `--scale 1`.
Both passed every automated test and satisfied R86's "observed running"
bar; neither had been read as its recipient would read it.

**Third instance, fifty-seventh filing (`a3ba0f8`, `Pass 53.0`, form
field rename GUI), within the hour, in the forms subsystem — nothing
to do with DXF export.** A rename confirmation read *"Renamed
'Personal.Name (p. 1)' to 'Personal.NameX'"* — the page-number suffix
reads as part of the old name, and the label prefers `/TU`, which a
rename does not touch, so on any field with an accessible name the
sentence announces a change to a string that is untouched. Found by
the engineer applying R174's own prescription the same day it was
written, not by a new failure category.

**Why this is worth tracking as its own memory rather than folding into
a "rules I've cited" list.** A freshly minted rule earning a third,
independent, cross-subsystem instance within the SAME DAY is unusually
fast corroboration — most standing rules in this project's ledger take
much longer (often sessions or weeks) to accumulate a second confirmed
instance, let alone a third. That speed is itself evidence the rule
names something structural about how this project's disclosures get
written (code-path-correct but audience-unread), not an artefact of one
session's mood or one engineer's momentary carelessness. Worth
revisiting if a future audit of standing-rule health is ever done —
`R174` is a strong candidate for "rules that are pulling real weight,"
distinct from rules that get minted, cited once defensively, and never
seen again.

**How to apply.** No action item — this is an observation for whoever
next reviews standing-rule health or decides whether a rule deserves
promotion into tooling (a lint, a pre-ship checklist item). Don't
re-derive this timeline from `ROADMAP.md`/`SESSION_LOG.md` greps if
asked "has R174 been useful" — this memory already has the answer with
its sourcing.
