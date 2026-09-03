---
name: state-your-premises-in-a-dispatch
description: Write a dispatch's premises out explicitly — four agents in one session returned corrections to the thing that sent them, and every one landed on a stated premise; a vague dispatch cannot be audited
metadata:
  type: feedback
---

**Write a dispatch's premises out explicitly, as claims, even the ones that
feel like background.** That is what makes a dispatch auditable, and the audit
is worth more than the filing you asked for.

**Why (2026-08-29):** four subagent dispatches in one session, and **every one
came back with a correction to the dispatch itself** — not to the work:

- *"You have NO shell"* → it did, and used it to verify every hash.
- *"Retire `ARCHITECTURE.md` §4.1"* → already done, one filing earlier.
- *"Here is the outbound reply's path"* → the sibling had archived it
  mid-session; the content was real, the locator stale.
- *"Both are shipped settings"* → one of them did not exist
  (`render.hairline_clamp_policy`: 0 hits workspace-wide).
- *"Neither reading contradicts any `shall`"* → **three sentences did**, and
  the correction changed what pdfce owes the operator: an *ambiguity* and a
  *divergence from the standard* are different claims, and only the second
  owes a rule-4 disclosure.
- *"`FEATURES.md`: zero rows change"* → zero **checkboxes**; one row's **prose**
  carried the withdrawn claim near-verbatim.

★ **Four of that session's eight Passes came from these audits**, and **none of
the defects was caught by any of the 29 gates** — they were in prose, in
documents another project builds against.

★★ **The mechanism, and it is why this is a rule rather than luck:** an agent
cannot check a premise the dispatch did not state. Every correction above
landed on a sentence I wrote down. The ones I left implicit went unexamined.
**A vague dispatch produces confident work on unexamined footings.**

**How to apply:**
- State the environment ("you have a shell"), the prior state ("§4.1 was
  retired in the 328th filing"), the file locations, and the *reasoning* behind
  any characterisation — not just the conclusion.
- Say which numbers are **relayed** and which are measured, and invite re-runs:
  *"the numbers below are relayed and you should not take them on trust."*
- End with *"report back with anything in this dispatch you find false — that
  matters more than the filing."* It works; it has produced a correction every
  time.
- Corollary: **do not act on an audit's finding without checking it yourself**
  when it will change shipped prose. I verified the three spec sentences in
  the corpus before rewriting a doc comment on their word — see
  [[feedback_external_llm_research_gets_assessed]] for the same discipline
  applied to research the operator pastes in.

★★★ **AN OUTBOUND-REPLY DISPATCH IS A REVIEW STEP, AND IT IS THE CHEAPEST ONE
THIS PROJECT HAS** (2026-09-01, fourth instance). An agent sent purely to
*explain a just-shipped fix to the consuming project* came back having found
**three defects in that fix that no gate caught**:

1. a **Pass-number collision** — the number was already minted the same day by
   the filing that ran in parallel;
2. the doc fix landed on the **prose box** and left the **verb table row**
   saying the old signature — the row being the index a consumer reads first;
3. a rustdoc sentence, *"it is literally the cache key"*, that was **the defect
   stated as a reassurance** and read as an argument that no further check was
   needed.

**Why it works:** writing the explanation forces a re-read *from the consumer's
position*, which is the one position the author never occupies. Gates check
what someone thought to check; a reader hits whatever the document actually
says first.

⇒ **When a change is worth announcing, dispatch the announcement BEFORE
considering the change done** — and read its findings as review, not as
courtesy. Note the shape: the defect being fixed was "a repair applied to one
route and not the other", and the fix itself repeated it. That is not irony, it
is the base rate.
