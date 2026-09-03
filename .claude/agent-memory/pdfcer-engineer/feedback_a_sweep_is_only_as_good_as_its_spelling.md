---
name: a-sweep-is-only-as-good-as-its-spelling
description: When sweeping for a stale CLAIM, punctuation is the failure surface — narrow the file set and widen the pattern, never the reverse; two instances in one day, one of them inside a gate that argued it was immune
metadata:
  type: feedback
---

When sweeping for a stale claim, **narrow the FILE SET and widen the PATTERN**.
Never the reverse. The words of a claim are stable; the **punctuation around
them is what a writer varies**, and that is where a sweep goes blind.

**Why (2026-08-29, twice in one day, both in colour work):**

1. `pdfce-librarian` swept for the claim *"the §8.6.7 ambiguity"* after a Pass
   retired it, found **six survivors nobody else had seen** — excellent work —
   and **missed a seventh**, because it grepped the phrase *with the section
   sign* and one line spelled it without one.
2. Same day, `tools/check-ledger-numbers.py` printed *"decision records: 103 →
   next free is 104"* while **decision 104 already existed**. Its pattern
   required whitespace between the word and the digits; the declaration spells
   it `` decision `104` `` — **the project's own house style**. Third
   occurrence in that one tool, and nothing else detects a duplicate decision
   number, so it was actively inviting one.

★ **The compounding detail in #2 is the transferable part.** The pattern sat
under a **forty-line comment arguing this could not happen**: *"it cannot
under-report, whatever spelling a future filing invents."* That argument was
sound about the **source** (any mention, anywhere in the file) and completely
silent about the **separator**. ⇒ **A claim in a comment is not a check.** The
better the argument in the comment, the more convincing the blind spot.

★★ **And the fix's own first run caught the fix.** I widened the pattern, added
a `_self_check()` asserting it against real spellings — and the assertion
**failed immediately** on `**decision _103_**`, because `_` is a *word*
character so `\b` never matched. **Underscore is markdown italics, i.e. one of
the very wrapping styles the widening existed to tolerate: the bug and the fix
were the same character.** A checker that asserts its own coverage is the only
thing that would have caught that.

**How to apply:**
- A global grep for a bare word returns dozens of correct uses and is
  unreadable. The **same grep over six files is six seconds of reading** — so
  scope by file, not by phrase.
- Never end a markdown-tolerant number pattern in `\b`; use `(?!\d)`.
- If a checker makes a coverage claim, **make it assert that claim at run
  time**, with the spelling that broke it copied verbatim into the list.
  Exit `2`, not `1` — a checker that cannot see its own subject has found a
  fault in *itself*, which is a different answer to a different person.
- Related: [[feedback_absence_needs_an_unscoped_query]] is the same shape with
  the scope in the path rather than in the pattern, and
  [[feedback_a_gate_that_underreports_looks_green]] is what this produces.

**★★ 2026-08-30 — TWO SWEEPS OF THE SAME CLAIM, BY TWO AGENTS, AND THE SETS
WERE DISJOINT.** `Pass 184.0` made a rename REPAIR the button actions naming a
field, falsifying a sentence that had been true for months. Both the librarian
and I swept for survivors. It swept six keywords; I swept the phrase
**"submit mapping"**. **Neither keyword set contained the other's spelling**,
so each of us would have missed *every one* of the other's survivors — and each
sweep reported clean in the meantime. Two clean reports, one incomplete
document.

Two things this adds to the rule above:

- **The failure surface is not only punctuation, it is SYNONYM.** The same
  claim was spelled *"submit mapping"*, *"submit mappings"*, and via the
  neighbouring nouns *"FDF"*, *"JavaScript reference"*. Widening the pattern
  around one noun does nothing about the sentence that used a different one.
- **★ A second sweeper is not redundancy unless the sets overlap.** Two agents
  sweeping independently *feels* like double cover and can be zero cover. If
  you dispatch one, **state the keyword set you already used**, so the other
  can deliberately choose a different one rather than accidentally choosing the
  same one.

⇒ **When a sweep returns clean, the question is not "did I find them all?" but
"what could my keyword set not have matched?"** Answer it by naming the set out
loud before believing the result.

**A third scoping failure from the same day, different axis:** a criterion
asked that a limitation be documented "in its rustdoc and in `docs/core-api/`".
It was documented in every file the FIX touched and in none of the files the
CHECKER lives in — because the sweep started from *"what did I change?"* rather
than from *"who reads this claim?"*. **The reader who most needs a limitation
stated is the one who never opens the file where you fixed it.**

**★★ 2026-08-30, same day, the counting version: A SOURCE GREP OVER A
DOCUMENTATION-FIRST CODEBASE COUNTS THE CODEBASE'S OWN PROSE ABOUT THE
CONSTRUCT.**

Censusing `debug_assert` invocations across `pdfce-core` + `pdfce-render`
produced **three different published answers before the right one**: 34 (the
librarian's), 27 (mine), and 24 (true; 12 + 12). Mine counted **grep hits**,
three of which were **comment lines** — `edit.rs:12085`, `12092`,
`xref_out.rs:282`, the careful reasoning about *why* that guard is a
`debug_assert!` and not a `panic!`.

⇒ **The best-written lines in the region are the ones that corrupt the
census**, and the inflation scales with exactly the documentation discipline
this project mandates. In a codebase where every construct is explained in
prose beside itself, `grep -c <construct>` is systematically high and the error
grows with quality.

★ **And the noun is where it went wrong, not the command.** I published the
exact grep I ran, which made it look auditable — the sourcing was perfect and
the word "invocations" was false, because what I had was "hits". A sourced
figure with the wrong noun is *more* convincing than an unsourced one.

**How to apply:**
- **Publish the DECOMPOSITION, never the total**:
  `56 mentions = 24 invocations + 6 cfg(...) + 2 fn defs + 24 prose`. A total
  invites re-derivation with a different pattern, which is how three answers
  happened; a decomposition invites arithmetic, which cannot disagree with
  itself.
- **Filter comment lines explicitly** when counting code constructs:
  `grep -vE "^\S+:[0-9]+:\s*//"` at minimum, and check the residue by eye.
- Say **hits** when you counted hits. Reserve *invocations*, *call sites* and
  *occurrences* for numbers you actually separated.
