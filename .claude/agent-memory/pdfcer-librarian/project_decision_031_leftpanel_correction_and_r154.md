---
name: project-decision-031-leftpanel-correction-and-r154
description: Decision 031 §4's "LeftPanel" dock mechanism was never built and could not compile as specified; corrected 2026-08-05 across 4 documents; standing rule R154 minted (decision-record prose vs. compiled reality). Check before citing the dock's pane-enum shape.
metadata:
  type: project
---

**2026-08-05, correction dispatch (no continuation number of its own in
this memory — filed as SESSION_LOG continuation 93).** Decision 031 §4
decided "a new, small `LeftPanel` enum, a SEPARATE `Tree<LeftPanel>`
instance, and the SAME `DockBehavior` mechanism reused as-is." That
mechanism cannot compile: `Tree<LeftPanel>` needs
`impl Behavior<LeftPanel>`; `DockBehavior` is `impl Behavior<DockPanel>`.
§4 also contradicted itself independently of the code, three times over
(headline "not duplicated" vs. its own bullet 3 "two small `Behavior`
impls" vs. §5's recap repeating the duplication claim vs. §4's "does NOT
do" list ruling out widening `DockPanel` — which is exactly what
shipped). §4's own section HEADER already had the coherent answer: "one
`DockPanel` enum, two `Tree` instances, one `DockBehavior`."

**What actually shipped (Pass 34.1 slice 1, `e15f55b`) — no `LeftPanel`
type exists anywhere under `crates/`.** `DockPanel`
(`crates/pdfce-gui/src/dock.rs:165`) was widened with two variants
(`Pages`, `ToolOptions`; `ALL: [Self; 6]`); a second `Tree<DockPanel>` is
built by `dock::default_left_tree()` under tree id `"pdfce-dock-left"`;
one `DockBehavior` is genuinely reused, unchanged, across both trees.
Type-level per-tree safety comes from a test,
`no_panel_is_mounted_in_both_docks`, not from a dedicated pane enum.

**Corrected, all append-only:** `docs/decisions/031-...md` new §7 (full
account, original §4 left untouched); `ARCHITECTURE.md` §12 (two bracket
corrections); `ROADMAP.md` Pass 34.1's Next-up entry (three bracket
corrections) + new standing rule **R154**; `SESSION_LOG.md` new
"Same-day continuation 93" note (continuation 92's text not rewritten).

**R154 minted:** a decision record naming a concrete Rust type is prose,
not code — nothing type-checks it. Mechanical check: once a Pass ships
against a decision record naming a type, grep that identifier under
`crates/`; absence needs either a "not built as specified" note or a
correction. Fourth sibling to R151 (call-graph audit)/R152 (confirmed-
caller audit)/R153 (fuzz-harness-coverage audit) — all three are
code-against-code; R154 is prose-against-code. **R154 claimed the number
R153 had reserved for a 4th unlisted candidate; decision 030's three
original contingent candidates (§6.2(a), §4.5, "date and label every
contract statement") were bumped to R155**, not R154, when any is
promoted.

**UPDATE 2026-08-05 (continuation 94, same day) — R155 already spent on
something else; decision 030's three candidates bumped again, to R156.**
The same continuation that shipped Pass 34.1 slices 2–3 also superseded
decision 024 §3.3 Family A (see [[project_decision024_family_a_superseded_and_r155]])
and, per the same reserved-slot transfer mechanism, minted **R155** for
that unrelated finding (a pre-dispatch search-discipline rule) instead of
letting decision 030's queue claim it. **Decision 030's three original
contingent candidates now take R156**, not R155. If you're reading this
file to find out what's currently reserved for decision 030's candidates,
check the live `ROADMAP.md` Standing rules ceiling before citing a
number — this queue has moved five times in one calendar day (R150→
R151→R152→R153→R154→R155) and will keep moving.

**How to apply:** if a future session (or a memory file, or a stale
context summary) says the left dock uses `LeftPanel` or a dedicated pane
enum, that's the pre-correction error — the real type is `DockPanel`,
widened, with two `Tree<DockPanel>` instances and one shared
`DockBehavior`. Don't re-propagate the old claim; grep
`crates/pdfce-gui/src/dock.rs` to re-verify current shape before citing
it, since this file itself is a snapshot, not a live source.
