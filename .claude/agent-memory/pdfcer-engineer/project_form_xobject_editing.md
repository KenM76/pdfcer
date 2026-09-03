---
name: form-xobject-editing-state
description: 2026-08-20 snapshot -- form-XObject text editing shipped (119.0/119.2); shared-form default is edit-in-place; two pre-existing defects only the operator's real CAD file exposed
metadata:
  type: project
---

**Text inside a form XObject is editable as of 2026-08-20** (`Pass 119.0`
`cc57080` for `edit_text`, `Pass 119.2` `a10a5c1` for `format_text`).
`EditRequest::target` / `FormatRequest::target` take the same `EditTarget`
enum; `Auto` is the default and searches page content then each form in `Do`
order. `text_edit::forms` is the discovery module; `pdfce-cli inspect --forms`
lists them with a `paints` fan-out column.

**Why:** the operator escalated it as *"99% of the text I will want to edit"* --
on a CAD sheet the page stream holds only the producer watermark and one form
XObject holds every label. `Editability::InsideForm` is now `#[deprecated]`
and never returned, deliberately, so `pdfceGUI`'s caret guard announces itself
at compile time.

**Decided, do not re-litigate (decision 076):** a shared form edits IN PLACE,
disclosed via `form_invocations`/`form_pages`. Copy-on-write is a separate verb
(`Pass 119.1`), not a mode -- because CoW is **not always expressible** (a form
invoked from inside another form cannot be re-bound without editing the parent)
and a default whose semantics depend on nesting structure is worse than one
that always means the same thing. **Acrobat's behaviour here is unsourceable**,
and that was measured, not assumed -- do not re-run that search.

**★ THE PART TO CARRY FORWARD.** Running the shipped verb on the operator's
real drawing — `docs/NEXT_SESSION.md` §7 holds the exact path, written once,
because this one has been mangled three times by scripted patches — found
**two defects in a row, neither in the new code**, after 20 tests and the
whole fixture corpus were green:

- `Pass 121.0` -- `R-INV-4` **falsely refused invertible fonts**, printing
  *"codes 361 and 361 both map to X"*, the same number twice. Shippable since
  `R110` landed, because *a refusal message is the one string a developer never
  expects to see*.
- `Pass 121.1` -- one four-character edit reported `followers_repositioned=1676`
  and moved **34,059 pixels across the whole page**. Reflow shifted every
  following `Tm` until a `Td`-family boundary, and **a CAD stream never emits
  `Td`**. `Pass 14.1`-era bug.

**How to apply:** *a reach extension does not only find new text, it finds old
assumptions.* Before calling any reach extension shipped, run it on his real
file, and watch `followers_repositioned` -- on absolutely-placed content it
should be `0`.

Related: [[feedback_engineer_does_the_observing]],
[[project_cad_export_structure]], [[feedback_priority_is_a_measurement]].
