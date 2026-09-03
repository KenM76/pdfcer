---
name: anchor-on-the-doc-block-not-the-item
description: When splicing code before an item, anchor the patch on the FIRST LINE OF ITS DOC BLOCK, never on the `fn`/`struct`/variant line — anchoring on the item lands the new code between that item's doc comment/attributes and the item itself
metadata:
  type: feedback
---

When inserting code before an existing item, **anchor the replacement on the
first line of that item's doc comment**, not on its `fn` / `pub struct` /
variant line. Anchoring on the item line splices the new code **between the
existing item's `///` block (and its `#[derive]`/`#[error]` attributes) and the
item itself**.

**Why:** Rust attaches a doc comment and its attributes to whatever follows
them. Splicing in between silently re-parents them onto the new code. Two
distinct failure shapes, both seen:

- **Doc comment only** → the existing item loses its docs and the new item
  inherits them. In `clap`-derive this is *shipped UI* — the `///` IS the
  `--help` text (see [[a-doc-comment-can-be-shipped-ui]]).
- **Doc + `#[derive(...)]`** → a hard compile error (`E0119` conflicting trait
  implementations), because the derives now apply to the new struct as well.

**How to apply:** every `python -c` / `Edit` splice that inserts *before*
something. Concretely — anchor on `` "/// **Delete a ce dimension**…" ``, not
on `` "DimensionDelete {" ``; on `` "/// What `edit_widget` changed." ``, not
on `` "pub struct WidgetEditOutcome {" ``.

**This recurred THREE TIMES in the 2026-08-30 session alone**, in three
different shapes — a clap subcommand variant, a CLI handler `fn` (caught by
clippy's `doc_lazy_continuation` and by `tools/check-cli-help-leads.py`), and a
`pub struct` with derives (caught by `E0119`). The first two were caught by
gates; the third by the compiler. **A splice before an item is the single most
error-prone edit shape in this codebase**, and the fix costs nothing if the
anchor is chosen right the first time.

Related: [[inserting-before-an-anchor-orphans-its-doc-comment]] recorded the
original instance; this is the generalised rule plus the derive-collision
variant it did not cover.

**★★ 2026-08-30 — THE CLASS SPLITS IN TWO, AND ONLY THE CHEAP HALF IS GATED.**
Three instances in one day, then the librarian measured `clippy-driver` against
hand-built fixtures. The result inverts the reassuring reading:

| shape | what ships | clippy |
|---|---|---|
| **A — blank line** between the spliced doc run and the item | the item ends up **undocumented**; the orphaned run is a bare comment | **CAUGHT** (`empty_line_after_doc_comments`) — even **through** an intervening `#[inline]` |
| **B — contiguous weld**: the run attaches to the *wrong* item | that item ships a **WRONG DESCRIPTION**, and in clap-derive that is shipped `--help` | **SILENT. Zero warnings.** |

Two consequences worth carrying:

- **The dangerous variant is the invisible one.** A missing description is a
  gap somebody notices; a *confidently wrong* one on a `pub` item is read and
  believed.
- **★ The observed catch rate is inflated by the very mechanism that hides B.**
  Both of my instances that day were variant A, agents produce A far more often
  (a blank line is the natural artefact of a splice), so "clippy caught it both
  times" reads as *the class is gated* when only half of it is. **Do not let a
  green clippy run stand in for reading the splice site.**

**A third variant, from the same day, that neither lint sees:** inserting a doc
block and forgetting the `pub` line under it. The doc lands on top of the NEXT
field's doc block and the struct is simply missing a field. The **compiler**
caught it — but only because a constructor referenced the missing name. **A
doc-only insertion of that shape ships silently.**

**How to apply, unchanged and now with a reason:** anchor on the FIRST LINE OF
THE DOC BLOCK, and after any splice into a type or an enum, read the three
lines above and below the insertion. `cargo clippy` is not the check here.
