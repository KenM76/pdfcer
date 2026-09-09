---
name: project-pass-283-1-r245-widened-to-affordance-and-entry-points
description: Pass 283.1 (485th filing) filed as R245's sixth dated instance, widened from "guard on a verb" to "affordance on an entry point" — no new decision, no new rule number.
metadata:
  type: project
---

`Pass 283.1` (`d8fcb68`, 2026-09-09, 485th filing) is the addendum to
`Pass 283.0`/decision 145/`R248`: the malformed-PDF-open override
(`LoadOptions`) shipped on `Document::from_bytes_with_options` only;
`Document::load_with_options(path, ...)` is new and wires `pdfcer`'s own
`open_document` to it, since every real shell opens a **file**, not bytes.

**Why:** the engineer's dispatch already framed this in
`docs/core-api/01-reading-and-model.md` §3.6b as "`R245`'s shape... applied
to an affordance rather than a guard" and explicitly left the
mint-vs-append call to this role. Decided: **append as R245's sixth dated
instance** (`ROADMAP.md` *Standing rules*, right before the R246 bullet),
not a new rule number — R245's own text already says "a family of parallel
`[X]`", so the generalisation from verbs to entry points, and from a
restriction to a capability, fits inside the existing rule's wording
without amendment. Also noted `R151`-adjacent (the affordance had test
callers, just not its intended production caller — a different shape from
R151's canonical zero-callers) without merging the two rules. **No new
`ARCHITECTURE.md` §12 decision** — filed as an addendum note on decision
145's own §10.5 body section and its §12 entry, since this is completeness
inside an existing mechanism, not a new crate-boundary/library/invariant
call. **Kept out of `C:\personal_rag\pdf\`** — this is API/testing
methodology (any Rust project with two entry points into one operation),
not PDF-domain-empirical, so it lives only in `D:\dev\rag\rust\` (a dated
footer on the EXISTING R245 founding file, not a new file) and in
`ROADMAP.md` *Standing rules*.

**How to apply:** when a future filing hands you a "is this rule X's shape
but not quite" judgment call, check whether the EXISTING rule's own stated
wording already covers the generalisation (R245's "family of parallel
`[X]`" did) before minting a new number — and check whether the dispatch's
own doc-comment update already made the same call, since an independent
engineer noticing the same shape and citing the rule by name is strong
corroboration, not something to second-guess into a fresh mint. See also
[[feedback_pattern_naming_needs_shared_mechanism_not_shared_moral]] — the
"would fixing one have prevented the other" test applied here to keep
R245 and R151 as two related-but-distinct observations rather than one.
