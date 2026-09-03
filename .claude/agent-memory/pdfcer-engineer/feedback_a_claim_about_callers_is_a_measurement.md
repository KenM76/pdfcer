---
name: a-claim-about-callers-is-a-measurement
description: A doc comment asserting who calls something (or that nothing does) is a measurement that goes stale silently — re-run the command it quotes before repeating OR correcting it
metadata:
  type: feedback
---

A sentence in a comment or document that says **who calls a function** — or
that **nothing does** — is a *measurement*, not a fact about the design. It
goes stale **silently**, because nothing recompiles when it does and no gate
can see it.

**Before repeating such a claim, or correcting it, re-run the command the claim
itself quotes.** It takes a second.

**Why:** on 2026-08-27 one comment in `ocr/layer.rs` was found to have been
wrong about its own callers **three times**:

1. *"called by the CLI"* — when nothing called it.
2. *"There is NO OCR subcommand — `grep -rn "ocr" crates/pdfce-cli/src/main.rs`
   returns nothing. An R151 instance: a capability with no shell caller."*
3. *"what still has no caller is THIS one-shot"*

All three were false when written. **(3) was mine, written while explicitly
correcting (2)**, and it went into a commit whose own message criticised
exactly this behaviour. The grep quoted in (2) does not return nothing and
probably never did — `pdfce-cli` had an `ocr` subcommand *and* a
`fetch-ocr-models` subcommand, "ocr" appeared 71 times in `main.rs`, and the
function was called from `main.rs:8673`.

I then repeated (3) into a Pass 135.0 commit message as *"CLI: not shipped, and
BLOCKED rather than skipped"* — a claim to the operator, sourced from a comment
rather than from the tree. **The librarian caught it by grepping** when it was
dispatched to file the Pass.

**★ A SECOND AND THIRD INSTANCE, 2026-08-27, `Pass 144.0` — and this pair
shows the OTHER way the claim goes stale: a LATER CALLER falsifies it.**

- `synth.rs`'s `name_claims_bold` said the heuristic *"is used only in the
  direction where being wrong is safe: `detect` uses it to say 'this looks
  synthesized', **never to refuse an edit**."* **True when written.**
  `format::gate_synthesis` was then written as a second caller and it refuses
  edits. Nothing reported it; `cargo doc` cannot check a claim about callers.
  It was found only because the Pass fixing the refusal read the function it
  was calling.
- `gate_synthesis`'s own doc said asking for synthetic bold on `Times-Bold`
  *"is refused with that same face named"*. Reading the source: `!is_self`
  **skips** the run's own resource, so it is never named. Nobody had reported
  this one either, and it had been wrong since the function was written.

⇒ So the class has two halves: a claim goes stale when a caller is **added**,
and it can be **wrong on the day it is written** if the author described the
intent rather than the code. Both are invisible to every tool.

**When you correct one, keep the old wording STRUCK rather than replacing it.**
The failure mode is worth more than the correction, and a silent replacement
erases the evidence that the class exists.

**The mechanism, which is the transferable part:** I knew a new caller had
appeared, so I *reasoned* about what the sentence should now say instead of
re-running the check. Correcting a stale measurement by inference produces a
new stale measurement that reads as freshly verified — worse than the original,
because it carries a recent date.

**How to apply:**
- Editing any sentence about callers, reachability, "no shell caller", `R151`
  status, or "X does not exist"? **Run the grep first**, even when — especially
  when — you are correcting it.
- Same for a claim of *absence* sent to another project. On 2026-08-27 I told
  `pdfceGUI` twice, emphatically, that `unshare_form` did not exist. It shipped
  hours later and the note had to be publicly withdrawn. **A note saying "X
  does not exist" has a shelf life**; if it advises suppressing a UI control,
  say so, or the control stays suppressed for months.
- Related but distinct: [[feedback-absence-needs-an-unscoped-query]] is about a
  query being *wrongly scoped*; this is about a *correct* query's result being
  quoted long after it was run.

---

## ★ 2026-08-30 — the class widened, and I hit it again in a session ABOUT it

The rule above is about claims that a comment **states**. The same failure
happens with claims I make **to myself while reading**, and those leave no
artefact at all.

Sweeping for `R235` (a verb that acts on an identifier without checking it
exists), I read `EditSession::set_group_style`, saw its guard, and concluded
its neighbour `set_group_scale` was guarded too. It was not. I then wrote that
conclusion into a working note as settled.

**It was found by DRIVING the binary, not by reading harder** — eight probes
with a bogus id, one minute. Seven verbs refused; that one printed a success
line naming a group it had not touched. The reading had taken far longer and
produced the wrong answer.

**How to apply:** when the question is *"does this code path check X?"* across
a **family** of similar functions, probe the family against the shipped binary
rather than reading them. Two properties make it strictly better: it is
faster per item, and it **cannot** generalise from a sibling — which is the
specific mistake reading makes. Reading is for *why*; running is for *whether*.

**The tell that you are about to make this error:** the words *"and its
neighbour does the same"* — or any conclusion about function B reached while
looking at function A.

Related: [[verify-each-instance-not-the-class]] is the same shape one level up
(an oracle run on one of two new variants). This is that lesson arriving from
the reading side rather than the testing side.

