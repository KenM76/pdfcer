---
name: read-architecture-every-session
description: CLAUDE.md requires reading docs/ARCHITECTURE.md every session; skipping it cost a recommendation that contradicted a day-old decision
metadata:
  type: feedback
---

**Read `docs/ARCHITECTURE.md` at session start. Every session. It is not
optional and it is not covered by reading `NEXT_SESSION.md` + `ROADMAP.md`.**

**Why:** on 2026-08-18 I read `NEXT_SESSION.md`, `ROADMAP.md` excerpts and the
spec RAG, skipped `ARCHITECTURE.md`, commissioned research on the CMYK→sRGB
collapse, and recommended **`moxcms`** as the ICC dependency "candidate of
record" — with a `PRIOR_ART.md` row filed to match. **Decision 064, made the
previous day and living only in `ARCHITECTURE.md` §12, had already assigned
all colour conversion to the sibling `iccce` project.** There was no candidate
slot. See [[iccce-boundary]].

The decision log is where **cross-project boundaries** live, and those are
exactly the constraints that do not appear in a Pass entry, a failing test, or
a `tools/check-*.py` gate. Nothing would have caught this.

**How to apply:**
- `ARCHITECTURE.md` is long. Reading the **§12 decision log tail** (the most
  recent ~5 entries) is the cheap version and would have caught this one.
- Before proposing **any** dependency, grep `ARCHITECTURE.md` and
  `PRIOR_ART.md` for the capability area first — not just the crate name.
- **★ A SUBAGENT CANNOT CHECK A CONSTRAINT THE DISPATCH OMITS.** My research
  brief asked the agent to state a licence for any library it named and never
  mentioned `iccce`, so the researcher could not possibly have flagged it. A
  well-executed subagent task built on an incomplete brief returns a
  confidently wrong answer, and **the brief is mine**. When dispatching, state
  the standing decisions the answer must live inside — not just the
  constraints (licence, wasm32) but the **boundaries** (what another project
  already owns). See
  [[feedback-external-llm-research-gets-assessed]].
- Related shape, same day: a citation makes a claim look checked. See
  [[feedback-two-modes-one-pattern-is-one-measurement]].
