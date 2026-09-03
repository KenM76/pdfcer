---
name: project_pass_82_criterion_correction_and_be_survivor
description: Pass 82.0/82.1 shipped (revision clouds, vertex-count boundary) — the filed acceptance criterion for /I's range was inverted, and hard rule 11's sweep found a doc-comment survivor that needed narrowing, not reversal.
metadata:
  type: project
---

2026-08-18 (177th filing). Two reusable patterns from this filing, both
likely to recur.

**1. A Pass's own filed `ROADMAP.md` acceptance criterion can be
factually wrong, and gets corrected IN the Shipped entry, not silently.**
`Pass 82.0` criterion 2 said `/I` (Table 167/169's cloudy-border
intensity) was refused outside `{0, 1, 2}` — an enumeration. The table
actually types it `number`, "in the range 0 to 2" — a continuous range.
**Why:** the sibling `/S` row in the same table uses the enumeration
idiom; `/I` uses the range idiom. One table, two idioms, easy to
conflate on a skim. When a criterion like this is wrong, correct it
in-place in the Shipped write-up with a dated ★ note and a `~~struck~~`
quote of the original — don't just quietly ship the right behavior and
leave the record saying something false.

**2. Hard rule 11 survivors aren't always wrong outright — some need a
NARROWING, not a reversal.** `crates/pdfce-core/src/edit.rs`'s
`DroppedProperty::BorderEffect` doc comment said "pdfce authors straight
edges only" — a blanket claim, false after `Pass 82.0` added cloudy-
border AUTHORING via `add_markup`. But the *operational* fact the
comment exists to state (the RESTYLE/regeneration path, `MarkupStyle`,
still has no `border_effect` field and still drops `/BE` on restyle) is
still true and unaffected. **Read for what the sentence is actually
claiming before flagging it as simply wrong** — the fix here is
"narrow the scope of the claim," not "delete/reverse it." Reported to
the engineer as owed (`crates/` stays out of this role's remit per hard
rule 11's own text — report, don't edit).

**Also confirmed as house convention** (matches the `Pass 92.0`/`93.0`
precedent): when a `### Pass NN` entry ships, it is FULLY DELETED from
*Next up*/*Backlog* — no remnant pointer text left behind. The enduring
record is a freshly written Shipped entry, not the relocated Next-up
prose. Any OTHER document that pointed at "`Pass NN` in *Next up*" (a
cross-reference elsewhere in `ROADMAP.md`, a decision-log entry's status
line) needs its own dated amendment — same-filing propagation duty,
not an afterthought.
