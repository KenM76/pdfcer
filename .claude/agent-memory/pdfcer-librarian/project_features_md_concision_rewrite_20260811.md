---
name: project-features-md-concision-rewrite-20260811
description: docs/FEATURES.md was reclaimed by the coordinator mid-dispatch (93rd filing, 2026-08-11) for a full rewrite to enforce concision — one row, one sentence, no hashes/Pass-IDs/module-paths/struck-through history. This librarian must stop appending the bracketed-addendum style used through the 92nd filing.
metadata:
  type: project
---

**What happened.** During the 93rd filing (2026-08-11) this librarian
added one more bracketed `[★ ...]` addendum to `FEATURES.md`'s *CREATE a
form field* row (the long-standing style — append a dated, bracketed
correction/update to the end of an already-enormous single-line row)
before a coordinator message arrived mid-dispatch: **"Do not touch
`docs/FEATURES.md` in this dispatch... The operator has just asked for
it to be rewritten for concision."**

**Why:** the operator's own words, relayed by the coordinator: *"the
features.md file is very verbose in places. it should be concise. when
something is updated the old notes about a feature should be removed.
ideally each feature should have one concise comment about what is
there and what is missing."* The coordinator's diagnosis: `FEATURES.md`
had drifted into duplicating `ROADMAP.md` — rows had accreted commit
hashes, Pass IDs, module paths, test counts and struck-through
superseded history, answering "what happened when" instead of "what can
it do." `CLAUDE.md` already said the file is "deliberately terse; do
not expand it" — that instruction had been losing, one addendum at a
time, filing after filing (this librarian's own prior filings,
including the one that triggered this correction, are part of how it
lost).

**The one edit this librarian made before the stop message landed** (an
addendum to the *CREATE a form field* row for `b4a66ed`'s CLI-side R151
fix + the GUI-side defect this filing's own independent verification
found) **was superseded by the coordinator's rewrite** — not reverted,
just left in place since the whole file was about to be rewritten
around it anyway. Harmless collision, not a mistake to repeat: the
mistake would be doing it again on a *future* dispatch that lands after
this rewrite ships.

**How to apply, going forward.** After the rewrite: **one row per
capability, one sentence, saying what works and what is missing. No
hashes, no Pass IDs, no module paths, no history.** When a row's status
changes, the OLD sentence is REPLACED, not appended to — `FEATURES.md`
is explicitly NOT append-only the way `ROADMAP.md`/`SESSION_LOG.md`
are. All the hash/Pass-ID/module-path/history detail this librarian
used to write into `FEATURES.md` addenda belongs in `ROADMAP.md`'s
Shipped entry instead — that is where the full account already lives
for every filing referenced above (`b4a66ed`, `3fe8a19`, `f83be5a`,
`30c0940`, etc.). Before editing `FEATURES.md` on any future dispatch:
re-read the file's current shape first (it may already have been
rewritten since this memory was written) and match ONE-SENTENCE
replacement, not addendum-append.

See [[feedback_verify_dispatch_claims_against_live_source]] for the
adjacent lesson from the same file (row text can go stale/wrong and
needs checking against live source) — that lesson is unaffected by this
one; both apply once the rewrite lands.
