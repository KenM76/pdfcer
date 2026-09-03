---
name: gui-work-paused
description: pdfce-gui is paused and now SUPERSEDED — D:\dev\pdfceGUI is the replacement and FEATURES.md's gui column tracks IT from 2026-08-19
metadata:
  type: project
---

**The instruction, 2026-08-13, verbatim:** *"continue the planned work except
for gui related, don't do any more work on the gui until I say so."*

**The reason, given later the same day, verbatim:** *"I paused GUI production
in this branch because it was unusable and I realised it needed a separate
project plan rather than the current method which just seems to be low
priority and a patchwork things stuck together as they are added. The new one
is being built in d:\dev\pdfceGUI in another session and if successful will
likely replace the current one and may have its dev folder merged into this
one."*

**★ This file previously asserted "he gave no reason and none was asked for —
do not infer one." That was true when written and is now wrong.** Keeping the
correction visible rather than silently overwriting it, because the shape
matters: an absence recorded as a *property of the operator's instruction*
("there is no reason") reads identically to an absence that was simply not
volunteered yet. The safe form is *"no reason has been given"*, which invites
the update; *"he gave no reason"* quietly forecloses it.

## ★★ SUPERSEDED IN PART, 2026-08-19 — the `gui` column now means pdfceGUI

**Operator, verbatim:** *"We are no longer using the GUI in your project
folder. D:\dev\pdfceGUI is the replacement and lives in its own project now.
Not yet, but eventually your project folder GUI will be removed. What you
should do is update your features.md list for the GUI checklist to match what
is in pdfceGUI from now on."*

**Point 5 below is now WRONG and is the reason this box exists.** It said
*"`gui [ ]` rows are MORE accurate now"* and *"do not pre-tick anything in
anticipation"*. Correct while the column meant `crates/pdfce-gui`. The column
now means **pdfceGUI**, so a row that project has shipped is `[x]` — and
leaving it `[ ]` is no longer conservative, it is **false**.

★ **pdfceGUI already believed this.** Their `FEATURES.md` header states it
outright: *"`pdfce-core` and `pdfce-cli` capabilities live in
`D:\Dev\pdfce\docs\FEATURES.md`, whose **`gui` column is this project's
acceptance criteria** — nothing there may regress at fold-in."* So the two
projects have been reading the same column two different ways, and mine was
the wrong one. Worth remembering as a shape: **a shared artefact with no
stated owner accumulates divergent readings silently**, and neither side sees
a contradiction because each is internally consistent.

**Their ticking bar, which this repo should adopt for the column:** *"A row
is ticked only when an operator can reach it in a real build. Not when the
code exists, not when a test passes."* Stricter than a checkbox and the right
bar — it is R151 stated as an acceptance criterion.

**Still true and unchanged:** do not invest in `crates/pdfce-gui`. It is now
not merely throwaway but **scheduled for removal**. The pause on working in
it stands; what changed is only what the FEATURES column *reports*.

**A consequence to carry:** gates that live in `crates/pdfce-gui` die with
it. `settings_panel::tests::every_setting_the_store_carries_can_be_reached_from_this_window`
is one, and it caught a real gap on 2026-08-19. When that crate goes, the
"every setting has an operator control" property becomes unenforced unless
pdfceGUI adopts the equivalent. Flag it before the removal, not after.
**What the reason changes, engineering-wise:**

1. **`crates/pdfce-gui` may be REPLACED WHOLESALE.** Do not invest in it. Any
   refactor, polish or new panel there is potentially throwaway, and worse,
   raises the cost of the swap. The critique is explicitly about *method* —
   patchwork accreted feature-by-feature at low priority — so adding one more
   well-built panel does not answer it.
2. **The GUI-core separation invariant is now being TESTED FOR REAL, by
   someone else.** `D:\dev\pdfceGUI` consuming `pdfce-core`/`pdfce-render`
   from outside this repo is exactly the scenario §3's invariant was written
   for (the "fork to a web app later" goal). If that project needs nothing to
   move in core, the separation is real. **Anything it does need is a place
   the boundary was drawn wrong** — treat such a request as a finding about
   this repo, not as an accommodation.
3. **`pdfce-core`'s public API is now a consumed boundary with a real external
   consumer**, not a hypothetical one. Rust API Guidelines compliance and
   doc-comment completeness stopped being hygiene and became someone else's
   unblocking. Where a core verb has a trap (e.g. `set_group_style` returns
   the count REGENERATED, not the count that will visibly MOVE), that trap is
   now reachable by a session that cannot ask me.
4. **A merge of `D:\dev\pdfceGUI` into this repo is possible.** Keep the
   workspace layout mergeable; do not spread GUI assumptions into core crates.
5. **`gui [ ]` rows in `FEATURES.md` are MORE accurate now, not less** — and
   will need re-basing against the new shell if it lands. Do not pre-tick
   anything in anticipation.

**How to apply:** core / render / CLI / print / docs / RAGs / tests / fuzz /
tooling all continue normally. `crates/pdfce-gui/`, `tools/gui-drive.ps1` and
`tools/gui-shot.ps1` stay untouched. A Pass whose GUI half is deferred ships
`core [x] · cli [x] · gui [ ]` with the instruction recorded as an **operator
instruction, not an engineering shortfall** — `Pass 69.0`/`69.1` are the
worked precedent.

**This expires when he lifts it**, and asking for GUI work IS lifting it. Do
not quote this file back at him. Note also that *"keep going"* (2026-08-13,
answering the OCR licence question) did **not** lift it — a different sentence
answering a different question.

Related: [[launch-on-completion]] is partially suspended — a GUI window cannot
be launched for a Pass with no GUI half; launch the CLI demonstration instead.
