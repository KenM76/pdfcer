---
name: project_find_ui_spec
description: Viewer Find (Ctrl+F) UI design review — first slice of the Reader-parity campaign (printing, Find, bookmarks, attachments, general doc panel, full-screen, signature validation). Core's EditSession::find_text shipped in 04c7820; GUI unbuilt as of this review.
metadata:
  type: project
---

Review delivered inline (2026-08-10), pre-build — the engineer asked for
critique BEFORE writing the GUI, off `TextMatch`/`EditSession::find_text`
(`crates/pdfce-core/src/edit.rs` ~L9966-10003) and the Acrobat RAG's
two-surface finding (Basic Find vs Advanced Search).

**Load-bearing findings for future UI work in this project:**

1. **Placement resolved by SPLITTING "entry-point icon" from "bar surface"
   — each judged by its own applicable precedent, not a single floating
   Area.** Entry-point icon: status bar, bottom-right, beside zoom/page-nav
   — reuses `PdfceApp::status_view_controls`'s own stated reasoning
   (`main.rs` ~L12686-12704: "must never sit behind a tab switch" per R125,
   "the position an operator's hand already knows") verbatim, and my OWN
   earlier icon-set-and-toolbar spec already reserved this
   (`docs/ui_specs/icon-set-and-toolbar.md` row 8.3: "REUSE icon-search.svg
   ... a real toolbar icon-only button (Ctrl+F), not buried in the Tools
   dock, since Find is a frequent view-scoped action per the five-way
   placement taxonomy"). The BAR itself: a NEW `egui::TopBottomPanel::top`
   shown conditionally, directly under the ribbon/toolbar row — explicitly
   NOT a floating `egui::Area` (the exact pattern Pass 34.1 already retired
   for tool propbars, twice: decision 024 fixed document-relative position,
   Pass 34.1 then eliminated the floating-Area mechanism entirely in favor
   of docking) and NOT a `PaneSubject`/dock-panel entry either, because that
   would EVICT whatever else the ArmedTool pane was showing (Redact review
   mid-session, Comments, Forms) the moment Ctrl+F was pressed — a concrete
   regression a TopBottomPanel avoids by construction, since it doesn't
   compete for the same pane slot. **General pattern for future "should
   this be a floating Area, a dock pane, or fixed chrome" questions in this
   project: a `TopBottomPanel`/`SidePanel` that only renders when a bool is
   true is architecturally identical to the ribbon/status bar chrome
   already in continuous use — NOT a reintroduction of the retired
   floating-Area pattern — and is the right default whenever the surface
   would otherwise contest a `PaneSubject` slot another workflow is mid-use
   of.**

2. **CONFIRMED, pre-existing, live keyboard bug — not hypothetical, not
   Find-specific.** `collect_keyboard_actions` (`main.rs` ~L12041) runs
   BEFORE any panel is drawn (confirmed by its own later cross-reference at
   ~L17107: *"That function runs BEFORE any panel is drawn, and
   `consume_key` removes the event from the frame"*) and binds
   `PageDown`/`PageUp` with **zero focus or tool gate at all**, and
   `Home`/`End` gated only on `!tool_active` — NOT on whether a panel text
   field has focus. Since `PaneSubject::Forms`' own doc comment states
   filling a field "arms no canvas tool," the overwhelmingly common state
   while editing a multiline Forms field is `tool_active == false` — so
   Home/End typed there are consumed globally into First/Last-page jumps
   AND (because `consume_key` removes the event from the frame's queue)
   the `TextEdit` widget never sees them at all, losing line-start/line-end
   caret motion on top of the wrong page-jump. PageDown/PageUp have no
   gate whatsoever and will always steal from ANY focused panel widget.
   The function's OWN doc comment already named the general shape of this
   as "Pass 7's problem" (~L12013-12019) and it was never closed. **Do not
   defer this — the project's own standing rule (`fix-bugs-on-discovery`
   memory, Ken 2026-08-08) is to fix on discovery, not file for later, and
   this review is the discovery.** The established, ALREADY-PROVEN fix
   shape in this exact function is the Enter-key precedent
   (`main.rs` ~L17093-17134): read the key as a **non-consuming peek**
   gated on the RIGHT widget's `.has_focus()`, never a blind global
   `consume_key`. Do not reach for `ctx.wants_keyboard_input()` as the
   guard without verifying it first — the canvas itself is a focus-holding
   `Sense::click_and_drag` `Response` (`image_response.has_focus()` is
   already load-bearing at ~L17151 for arrow-key caret handling), so if
   `wants_keyboard_input()` is merely "is any widget focused" it will ALSO
   be true while the canvas legitimately holds focus for its own reasons
   and would over-suppress the PRIMARY use case (paging while looking at
   the document). No `D:/dev/rag/egui/` entry currently answers this
   precisely — verify against egui's actual source/RAG before trusting it,
   and write the finding to the RAG once resolved either way.

3. **Find's own Enter/Shift+Enter (next/prev match) and Escape-to-close
   must NOT be added as new blind global `consume_key` entries in
   `collect_keyboard_actions`, for the identical reason Enter already
   isn't.** Enter specifically is already claimed via the non-consuming
   peek pattern precisely because a global consume would eat the Forms
   panel's and field-rename editor's commit-on-`lost_focus()` flow before
   they ever see it. A find-query `TextEdit` needs the same non-consuming,
   focus-gated treatment. Escape is a genuinely new design surface, not a
   copy of an existing pattern: the CURRENT global Escape handler
   (`main.rs` ~L12179-12193) is consumed centrally every frame regardless
   of what's open and resolves through `canvas::resolve_escape`'s existing
   4-way precedence chain, which knows nothing about "is Find open" — so
   as written, opening the find bar and pressing Escape would fall through
   to whatever the canvas-selection rung currently does, and the find bar
   itself would never see the keystroke (already consumed from the frame's
   event queue). Recommended fix: Find-open becomes a NEW, outermost rung
   — a 5th input flag to `canvas::resolve_escape` and a new
   `EscapeOutcome::CloseFind`, checked BEFORE the tool/gesture/selection/
   rail chain, the same idiom `inside_object` already uses for its own
   first-checked rung. **Any future dismissible-overlay design in this app
   (a find bar, a quick-filter, anything with its own Escape-to-close)
   should extend this ladder rather than add a second, competing Escape
   consumer** — see the `gesture_commit_and_shell_audit` memory's point 6
   for why this substrate (`resolve_escape`/`GestureInterrupt`/
   `EscapeOutcome`) is deliberately the one place this logic should live.

4. **Ctrl+F itself is safe as a blind global chord** — same class as the
   already-shipped Ctrl+E (Edit Text toggle, ~L12160) and Ctrl+Shift+E (Add
   Text toggle, ~L12170): a COMMAND-modified letter chord never collides
   with plain typing, unlike a bare Enter/Escape/PageUp/Home. Recommended:
   gate `Action::ToggleFind` through `action_preserves_gesture` (cited by
   `ribbon.rs`'s `View` tab doc comment as the existing predicate for "does
   this change the document") rather than reasoning fresh about whether
   opening Find should interrupt an in-flight tool gesture — Find changes
   nothing about the document, same as every View-tab action, so it should
   coexist with a pending edit rather than force a discard/commit choice.

5. **Match navigation and the cross-page-jump disclosure question resolved
   by the viewer's own architecture, not by adding a narrator toast.**
   `ViewState` (`viewer.rs` ~L90-139) is single-page — `page_index: usize`,
   `go_to_page` sets ONE index — pdfce has no continuous-scroll canvas.
   That settles two things at once: (a) "highlight all matches" can only
   ever mean "on the current page" — a whole-document highlight-all is not
   a thing the canvas model supports, so don't scope it as if it were; (b)
   navigating to a match on a different page is ALWAYS a real page jump,
   never a scroll. The honest disclosure for that jump is a **persistent,
   always-current label in the find bar itself** ("Match 3 of 7 — page
   2"), not a one-off status-bar note — closer to the `redact_mark_row`
   precedent (`main.rs` ~L8716-8729) where the destination page is stated
   BEFORE the click (in the row label) so no separate narration is needed,
   adapted here since Find's navigation is keyboard-driven rather than a
   labeled row: the persistent counter IS the "before" state, always
   visible, always current, which is a stronger disclosure than a
   transient toast because it doesn't require having been watching the
   status bar at the right moment.

6. **The three known limitations (`/ActualText` unmatched, per-run
   splitting, page-content-only) should be a PERMANENT, unconditional line
   in the find bar's hint text, never a conditional/computed disclosure —
   and this is a case where the "smarter" conditional approach is actually
   less honest.** Direct precedent: `redact_search_hint`/
   `redact_pattern_hint` (`ui_text.rs` ~L6110, ~L6153) already state their
   scanned-page caveat UNCONDITIONALLY in both hints, specifically because
   (per that code's own comment) a silent zero-match result reads as
   "nothing here" rather than "nothing SEARCHABLE here." Find's failure
   mode is the SAME shape but harder to gate correctly: core CAN count
   `/ActualText` runs per the dispatch, but per-run splitting is a
   property of the CURRENT QUERY against the CURRENT runs, not a
   precomputable per-document fact — a document with zero `/ActualText`
   runs can still silently miss a kerning-split phrase, so gating the
   caveat on "does this doc have ActualText" would give a false all-clear
   for the more common failure mode. State it always, briefly, matching
   the redact convention exactly.

7. **Whole-Words is a genuine, load-bearing `pdfce-core` gap, not a cheap
   client-side filter — verified by reading `TextMatch`'s own fields, not
   assumed.** `TextMatch { page_index, quad, text }` carries no context
   around the match (`text` is only the matched substring, echoing the
   document's own spelling per the dispatch). Word-boundary testing needs
   the characters immediately before/after the match, which nothing in the
   current struct provides — so "just check the surrounding chars in the
   GUI" is not available without a new core capability. Recommended slice-1
   scope: ship Case-Sensitive only (a direct, trivial invert of the
   existing `case_insensitive: bool` parameter); defer Whole-Words with
   the gap named explicitly, same posture as the `/TU`-vs-`/T` and
   tolerance-zero-representation findings in earlier specs — flag the core
   gap rather than fake the feature.

8. **The operator's own borrow-trap worry (calling `&mut self`
   `find_text` from inside a render closure) is ALREADY a solved shape in
   this codebase, not a new problem** — `search_and_mark_for_redaction`
   (`main.rs` ~L8805-8835) is the exact same shape: a dedicated `PdfceApp`
   method holding `&mut self`, called from the ACTION-DISPATCH phase
   (~L10350), never from inside the panel-rendering closure that reads
   `&self`/queues actions. Find should follow this identically: the render
   closure only reads cached `Vec<TextMatch>` + current-index state,
   typing/Enter pushes an `Action::RunFind(String)` (or similar), and the
   actual `find_text` call happens in the dispatch loop, caching results
   back for the next frame to render. No new pattern needed.

9. **Genuinely open engineering question, correctly out of scope for this
   review to decide, flagged rather than silently resolved either way:**
   live-search-as-you-type (every keystroke re-runs `find_text`, which
   calls `extract_document_view` internally) vs. Enter-triggered search
   only. The former matches Reader/browser convention; the latter is
   cheap and debounce-free. Recommended the engineer decide based on
   measured `extract_document_view` cost on a realistic multi-page
   document rather than assume either is fine.

10. **Canvas highlight rendering must reuse the Pass 12.0 geometry bridge,
    not re-derive it.** `TextMatch.quad` is PDF/page-space (per the
    dispatch's own doc comment on `find_text`). Converting it to
    screen-space for drawing highlight rectangles is exactly the
    canvas-space-vs-PDF-space problem the `project_pass_12_0_canvas_
    substrate_spec` memory already names: invert `pdfce_render`'s
    Transform rather than hand-deriving a new mapping.
