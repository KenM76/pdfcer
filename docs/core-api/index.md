# `pdfcer-core` — consumer API map

**For a shell being built against this crate from outside this repository**
(the new GUI project at `D:\dev\pdfcer-gui`, a future WASM shell, any other
consumer). Written 2026-08-13; **the figures in the table below were last
re-derived at `e194b46`, 2026-08-18** — see the note under it.

This is **not** rustdoc. Rustdoc already exists and is good at *"what does
this function do."* These three files answer the question rustdoc cannot:
***"I want to do X — what do I call, in what order, and what will bite
me?"*** Every section leads with a grep-able *"I want to…" → "call this"*
index and ends with **Traps**.

| file | covers | size |
|---|---|---|
| [`01-reading-and-model.md`](01-reading-and-model.md) | loading, the COS object model, pages, content streams, text extraction, fonts, vector picking/snapping, filters, colour, navigation, metadata | 2,799 lines · 148 clauses cited |
| [`02-editing-and-saving.md`](02-editing-and-saving.md) | `EditSession` end to end — **all 191 public verbs**, the command/undo contract, the dirty set, the save path, the guard/refusal model, `EditError`'s 118 variants | 4,576 lines · 138 clauses cited |
| [`03-capabilities.md`](03-capabilities.md) | ce dimensions, forms, markup, redaction, OCR, print/imposition, rasterising, raster export — each with **★ what the UI must disclose** | 3,038 lines · 74 clauses cited |

> ### ★ Every figure above was stale, and the verb count caused an incident
>
> Corrected 2026-08-18. The verb count read **108** when the real number was
> **116**, and the three line counts were each short.
>
> The verb count is the one that cost something. `pdfcer-gui` wired
> `insert_pages` — one of the eight verbs missing from part 2 — and shipped a
> **wrong operator disclosure** about it, because with the verb absent from
> the document a chat reply was the only description of it in existence.
>
> **What makes this worth a paragraph rather than a silent edit** is that the
> figure was *sourced*: it said "verified against the source at `6c5124c`".
> A published number with its provenance attached reads as **maintained**,
> and that is exactly what deters the check that would catch it. A vague
> "many verbs" would have invited someone to count. `R197` was minted from
> this.
>
> **`tools/check-core-api-verbs.py` re-derives the verb count and the verb
> list on every run** and fails if either drifts. It reads this file too —
> which it did not when it was first written, and this line is why: the gate
> was built while correcting part 2 and its input was part 2, so it went
> green while the front door of the same directory still said 108.

## Read these four things before writing any code against this crate

1. **Coordinate spaces.** PDF user space is **y-UP**; image and screen
   space are **y-DOWN**. Every geometry function states which it takes.
   Getting it wrong is silent — the page looks perfect until someone
   selects a line and gets a different one.
2. **Hit-test and snap tolerances are PAGE-space radii, and nothing checks
   them.** Pass raw screen pixels and it compiles, runs, and merely drifts
   with zoom.
3. **Rule 4, "fuzzy never sneaky."** Anything pdfcer *inferred* — an OCR
   result, a best-fit circle and its residual, a snapped point, a
   substituted font, a near-parallel classification — must be visible
   **before** it becomes document state, and rejectable without undoing
   anything else. Part 3 names, per capability, exactly which values are
   inferences. A shell that does not know which ones they are ships a
   rule-4 violation and will not find out.
4. **A returned count is not always the count you want to show.** The
   worked example is `set_group_style`, which returns members
   *regenerated* (all of them), not members that will visibly *move*.

## How these were built, and what that means for trusting them

Every symbol was enumerated from source and its `file:line` **machine-checked
against HEAD** — a pass that caught 23 wrong line numbers and one false
claim about a re-export. Anything that could not be verified is written as
`UNVERIFIED — <what to check>` rather than guessed: **an honest gap is
useful; a confident wrong answer costs a day.** 18 such markers survive in
part 2 alone, and they are content, not omissions.

**Source is authoritative when these disagree with it.** They are a dated
snapshot of a moving crate; re-verify anything load-bearing before relying
on it, and prefer a `file:line` citation over prose.
