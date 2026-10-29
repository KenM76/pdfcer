# Decision 205 — nested overflow leaders/trailers: innermost cut subform by default, `EachAsserting` as a setting

Recorded 2026-10-28 (1285th filing), at `Pass 647.2` eighth cut `cac24637`. Applies decision 204.

**Question.** When a top-to-bottom split cuts a subform nested inside another, and several enclosing subforms assert an overflow leader or trailer, which lays it? XFA 3.3 Ex 8.43 (pp.323-324) is ambiguous.

**Decides.** `NestedOverflow { Innermost (default), EachAsserting }` (`#[non_exhaustive]`), set with `LayoutOptions::with_nested_overflow`.
- `Innermost`: the innermost cut subform lays the trailer at the end of its top fragment (cut containers grow to hold it) and the leader at the top of its rest. It spends its own chain, then the levels out to the instance, then the instance's.
- `EachAsserting`: every enclosing cut subform asserting its own kind also lays it; trailers innermost first, leaders outermost first.

Trailer heights are reserved, re-splitting up to 4 times; use counts are per nested instance; the instance's own trailer still follows the instance in flow.

**Why a setting.** The standard leaves the choice open and the operator's standing rule is that spec ambiguity becomes a setting with a default, never a hard-coded choice.

**Code.** `crates/pdfcer-core/src/xfa/layout/nested.rs`; documented in `docs/core-api/03-capabilities.md`. Core only; the CLI flag arrives with `Pass 647.5`.
