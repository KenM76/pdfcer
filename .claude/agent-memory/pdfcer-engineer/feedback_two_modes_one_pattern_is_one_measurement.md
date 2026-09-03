---
name: two-modes-one-pattern-is-one-measurement
description: Re-running one grep pattern under a different counting mode is not verification; and do not override an independently-derived result with a more confident one without a genuinely different method
metadata:
  type: feedback
---

Two methods that share an input are **one measurement**. Verify with a
*different* derivation, not the same one counted differently. And when an
independently-derived figure disagrees with mine, treat that as evidence my
method is incomplete — not as an error to correct.

**Why:** on 2026-08-11 `pdfce-librarian` measured 63 filesystem-path sites in
`pdfce-gui`. I measured 51, ran it two ways — `grep -c` (lines) and `grep -o`
(occurrences) — got 51 both times, read the agreement as verification, told the
librarian its number was wrong, and **reported 51 to Ken as a measured fact**.

Both of my runs used the same pattern, `PathBuf|&Path\b`. It cannot match
`&std::path::Path`, because `std::path::` sits between the `&` and the `Path`.
That form appears **12** times. 51 + 12 = 63, disjoint. **The librarian was
right from the start.**

Two distinct failures, and the second is the worse one:

1. **Same-pattern agreement is not corroboration.** It proves the input was
   applied consistently, nothing more. "Measure, don't assert" is not
   sufficient — a measurement can be confidently wrong, and repeating it does
   not help.
2. **I talked a correct agent out of a correct result.** I spend most sessions
   insisting agents verify rather than relay. Here the librarian *did* verify
   independently, and deferred because I arrived with arithmetic. The
   verification culture failed in the direction nobody watches: not uncritical
   relaying, but a correct result being overridden by a more confident one.

**How to apply:**
- Disagreement with an independently-derived number is a signal to widen my
  own method first, not to defend it. Ask "what could my pattern *not* see?"
- For counts, the cheap different-method check is arithmetic closure: find the
  delta as its own measurable set. `51 + 12 = 63` settled this in one command
  and would have settled it before I ever contradicted anyone.
- Regex counts of Rust identifiers are especially fragile when the type has
  multiple spellings (`Path`, `&Path`, `std::path::Path`, `PathBuf`) — enumerate
  the spellings before counting.
- Say it plainly when reversing something already told to Ken. This one reached
  him as fact and needed an explicit retraction, not a quiet edit.

Minted as standing rule **R188** in `ROADMAP.md`, covering both halves.

Related: [[absence-needs-an-unscoped-query]] — same family, a query whose scope
silently excluded the answer. And note the contrast with
[[librarian-needs-exact-hashes]]: that one is about giving agents good inputs;
this one is about not overriding their good outputs.
