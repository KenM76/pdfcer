---
name: an-unticked-box-is-unfalsifiable
description: A `[ ]` in FEATURES.md is a negative existential about our own code — nothing can fail when it goes stale, so the file decays one way only; check the Implemented section before reporting any gap
metadata:
  type: feedback
---

**Before reporting that pdfce cannot do something, run the verb.** A `[ ]` row
in `docs/FEATURES.md` is not evidence of absence.

**Why, and this is the mechanism rather than a caution:**

> **`[x]` is falsifiable by the build.** Delete the function and tests go red.
> **`[ ]` is falsifiable by nothing.** No test, no gate, no compiler can notice
> that a capability arrived.

So the document **decays in exactly one direction** — true → false as things
get built, never the reverse — and it is most wrong precisely where it is most
consulted, because a capability nobody ships is a row nobody revisits.

**What it cost, 2026-08-28.** Ken asked how many things were not completely
editable. I answered from `FEATURES.md`. Of the three ce-dimension gaps I gave
him, **two did not exist**: `dimension-vertex --op move` re-measures
(`5.000 m` → `6.250 m`, disclosed), and `dimension-offset` sets extension-line
standoff. Both had shipped in core *and* CLI. He had been steering on it, and
`pdfceGUI` reads the same file.

**★ And the refutation was closer than the binary.** The librarian found each
false row contradicted by an *Implemented* row **in the same file** — 178 and
184 lines away — and `docs/decisions/026-…` is **titled** *"the offset that
makes extension lines possible"*, with a section headed *"Extension lines,
drawn"*. A whole decision document is named after a capability a row declared
unbuilt.

⇒ So the check is cheaper than I claimed when I called it "two CLI
invocations": **grep the Implemented section for the capability before
believing the Planned one.**

**How to apply:**

- Reporting a gap to Ken or to a consuming project is a **measurement**. Run
  the verb, or at minimum grep *Implemented* and `docs/decisions/`.
- When a Pass lands, the `[ ]` rows it falsified are **your** job to find — no
  gate will. The librarian sweeps, but the sweep is seeded from what the
  dispatch names.
- Two rows for one capability is the signature: it is how the contradiction
  grew, and the fix is to **fold them**, not to tick one.
- The same asymmetry applies to any "not supported" / "unbuilt" / "no route
  for this" sentence in a doc comment or a `docs/core-api/` limit. Four of
  those were found stale in one day, one of them **instructing a consuming
  project to tell its operator something untrue**.

Related: [[a-claim-about-callers-is-a-measurement]] and
[[priority-is-a-measurement]] — same family. This is the negative case, and it
is worse than both, because the others can at least be contradicted by
something.
