---
name: feedback-offload-web-research-but-verify-it
description: Ken's 2026-08-18 direction to push web/external research to subagents to preserve his own token budget for code, with his own attached caveat to disagree with and confirm findings rather than accept them on say-so
metadata:
  type: feedback
---

Ken wants web research and external-source lookups dispatched to
subagents rather than done inline by the lead engineer, specifically to
preserve his own token budget for code. Stated 2026-08-18.

**Why:** token economy — code-writing and code-review is the scarce,
high-value use of the primary session's budget; a research question with
an answer that lives outside the repo (an upstream crate's behavior, a
spec point not yet in the RAG, a library comparison) is exactly the kind
of work a dispatched subagent can do in parallel without competing for
the same budget.

**How to apply, and the caveat is load-bearing, not optional politeness:**
Ken attached his own condition when giving this direction — a
research subagent's findings should be **disagreed with when warranted**
and **confirmed** rather than accepted purely because the dispatch
returned a confident answer. This is the same discipline this librarian
already applies to its own dispatching engineer's reports (see
[[feedback-dispatch-should-carry-git-evidence-when-no-shell]] and
[[feedback-verify-dispatch-claims-against-live-source]] — both records of
a relayed claim turning out to be incomplete or wrong when checked
against the actual source) — extended explicitly, by the operator, to
research subagents specifically. Treat a research subagent's output the
same way this librarian treats any dispatch: as a claim to verify against
primary sources (the spec RAG, the crate's own docs, a `Read` of the
actual upstream behavior) where that is cheap, not as settled fact
because a subagent produced it fluently.

This is a collaboration-style preference about *how* research gets
consumed across this project's agents, not a fact about pdfce's current
state — hence filed as feedback, not project memory.
