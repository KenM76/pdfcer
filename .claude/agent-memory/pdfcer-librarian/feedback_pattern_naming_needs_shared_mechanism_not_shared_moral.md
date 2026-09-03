---
name: feedback-pattern-naming-needs-shared-mechanism-not-shared-moral
description: When asked to judge whether several findings share a nameable pattern worth a standing rule, require a shared failure MECHANISM, not just a shared moral ("re-check a stale premise") — used 2026-08-18 (181st filing, pageops/mod.rs survivor closure)
metadata:
  type: feedback
---

The engineer dispatched a docs-only correction (`3f8cc1e`, closing the
`pageops/mod.rs` Hard Rule 11 survivor) and explicitly invited a
judgment call: three findings from the same session — this one (a
design rationale's scope silently widened from "this approach needs
X" to "any approach needs X"), the `moxcms` boundary-decision error
(175th filing, a decision cited without being re-read), and `Pass
82.0`'s `/I` criterion (a spec table's range idiom misread as an
enumeration) — were offered as possible instances of one pattern. The
prompt explicitly allowed "a refusal with a stated warrant" as a fine
answer.

**Declined to unify them**, with the warrant: the three share only a
generic moral ("re-check a stale premise before trusting it"), which
is already the animating idea behind Hard Rule 11 and several other
declined-candidate notes already in `ROADMAP.md`. They do not share a
specific *mechanism* — one is scope-widening in a design rationale,
one is a citation nobody re-read, one is a table-idiom misidentified
at authoring time. Naming a rule after a moral rather than a mechanism
produces something too broad to apply precisely (everything in
engineering is "someone should have double-checked").

**Why this matters for future judgment calls of this shape:** the
project has a real, working precedent for *how* a pattern earns a
standing rule — see `[[project_r174_rapid_corroboration]]` and Hard
Rule 11's own adoption history (flagged as a candidate at the 170th
filing, corroborated at 171st–174th, adopted at 175th). The bar that
worked was **repeated occurrences of the same mechanism**, not a
single session producing three superficially similar-sounding misses.
Recommended standard going forward: when asked to judge whether N
findings are "the same pattern," ask whether the *fix* for one would
have prevented the others. If the fixes are unrelated (narrow a doc
comment vs. re-read a cited decision vs. re-derive a spec table), the
findings are not the same pattern even if the English description
rhymes.

**How to apply:** the next time a "worth naming?" question comes up
across multiple findings in one session, check for shared mechanism
first. If only the moral is shared, decline with this reasoning cited,
and recommend watching for a second true instance of the *narrowest*
individual finding before naming anything.
