---
name: project-pass-455-underline-strikethrough-915th-filing
description: Pass 455.0 (G085, underline/strikethrough tied to text) shipped and filed, 915th filing, b602ab9b; Pass 455.1 (Tagged-PDF TextDecorationType) stays open, Next up
metadata:
  type: project
---

2026-10-03, 915th filing, commit `b602ab9b` (14 files, +1224/-2). Pass 455.0
shipped: `FormatRequest::decoration(DecorationSet)` ties underline/
strikethrough to the text via a `/pdfc_Deco` marked-content pair + a
`refresh_decorations(page)` recompute inside every text-editing verb's undo
entry (`move_text_run`/`delete_text_run`/`edit_text`/`reflow_block`/
`format_text`), so the rule follows the text instead of staying behind as
orphaned page content (the GUI's previous `MarkupSpec::TextMarkup` approach).
CLI `format-text --underline`/`--strikethrough`/`--no-decoration`.

**Why:** this was filed plan-only in commit `48d66136` (914th filing region),
then the code shipped in a separate commit (`b602ab9b`) — a two-step filing
(plan, then code) rather than the usual single-commit Pass completion.

**How to apply:** `docs/FEATURES.md`'s Planned row for "Underline/
strikethrough tied to the text" moved to Implemented (Text editing &
formatting section, inserted after the `whole_operator` restyle/edit row);
`core [x]`/`cli [x]`/`gui [ ]`. A NEW, narrower Planned row was written for
the still-open `Pass 455.1` (Tagged-PDF `TextDecorationType` structure
attribute, ISO 32000-1 §14.8.5.4.4 Table 345) rather than deleting the whole
capability's Planned presence — when a bundled Pass ships only part of its
scope, split the residue into its own row/entry rather than losing it (same
pattern as [[project_bundled_backlog_entry_partial_ship_needs_residue_split]]).

Owed items NOT yet scoped into a Pass (carried in SESSION_LOG "For next
session"): refresh inside form XObjects; OS/2 `yStrikeout` as a strike-source;
standard-14 AFM `XHeight` for non-embedded fonts; multi-content-stream pages
aren't refreshed.

No decision opened (stays 188), no standing rule minted (stays `R263`). No
shell available this filing — all figures relayed from the dispatching
engineer's report (hard rule 8).
