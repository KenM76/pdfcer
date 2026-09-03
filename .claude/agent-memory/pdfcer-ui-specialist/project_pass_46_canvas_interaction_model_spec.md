---
name: project_pass_46_canvas_interaction_model_spec
description: Pass 46 canvas interaction model spec (docs/ui_specs/pass-46-canvas-interaction-model.md) — supersedes pass-6.1-markup-tools.md, answers the operator's "tools drop into the center, I can't drag/resize" complaint with a draw-gesture fix + a new post-hoc select/move/resize model.
metadata:
  type: project
---

Spec delivered: `D:\Dev\pdfce\docs\ui_specs\pass-46-canvas-interaction-model.md`
(2026-08-12), dispatched off the operator's direct complaint that markup
tools "drop things into the center of the pdf window" with no drag/resize,
and his expectation "click tool → options in sidebar → draw where I point,
just like every program."

**Root cause, confirmed by reading code, not assumed:** `pass-6.1-markup-
tools.md` (2026-07-31) designed its own parallel `MarkupTool`/`DrawState`
state machine one day BEFORE `CanvasTool` (Pass 12.0) shipped and was never
implemented against it. What shipped instead is `PdfceApp::add_markup_shape`
(`main.rs:5548`) — a placeholder that computes a rect from `page.media_box`'s
centre plus a jitter counter and never calls `screen_to_page`/`active_tool`/
any gesture machinery at all. It is invisible to every substrate rule
(Escape, pan-suppression, `TOOL_PRECEDENCE`) other tools obey.

**Load-bearing findings for future UI work in this project:**

1. **Compliance audit found Markup is the ONLY non-compliant `CanvasTool`
   candidate in the whole app — not a systemic pattern.** Checked all seven
   shipped tools (TextEdit, AddText, PlaceField, the three Measure tools,
   VectorEdit) against the formal "arm→options appear→gesture targets the
   pointer→commit" contract by reading each one's actual gesture function
   (`run_place_field_tool`, `run_vector_edit_tool`, etc.), not assuming from
   the enum's doc comments. All six already comply, built correctly on
   first attempt. **General lesson: before recommending "bring existing
   tools into line" as part of a fix, audit each one directly — the fix here
   turned out to be narrower and safer than the dispatch brief's framing
   suggested.**

2. **`active_tool` is no longer a single `Option<CanvasTool>` as of
   2026-08-06** — `CanvasTool` derives `Ord`/`Hash` so `BTreeSet<CanvasTool>`
   holds the SET of independently-toggled-on tools, with `OpenDoc::
   TOOL_PRECEDENCE` (a `[CanvasTool; 7]` array, `main.rs:3466`) deciding
   which one answers a click. `active_tool()` = first enabled tool in that
   array. Any future CanvasTool design must place the new variant in this
   array with reasoned position, not just add it to the enum. **Check this
   array's current shape before designing any new tool — it materially
   changed after Pass 12.0's original single-`Option` design and is easy to
   assume is simpler than it is.**

3. **`ObjectModelProvider` (`decompose_page`) does NOT see annotations at
   all** — it walks the page CONTENT STREAM only (paths/text/images/form
   XObjects); `/Annots` is a structurally separate part of the PDF object
   model. This is why Pass 9a's already-shipped, fully-working general
   object-selection substrate (`canvas_selection`, click/marquee/cycling,
   confirmed live in "no tool armed" view mode by reading the actual canvas
   image `Response` construction and `run_dimension_drag`'s unconditional
   per-frame call) cannot be reused as-is for markup annotations — a genuinely
   new `AnnotationTargetProvider` + a genuinely new, INDEPENDENT selection
   field (`doc.selected_annotation`, not folded into `canvas_selection`) is
   needed, following the SAME "each selectable kind owns its own state
   shape" pattern `doc.selected_dimension` and `TextEditState` already
   established — this is the third instance of that pattern, not a new
   architecture.

4. **The load-bearing core-model gap is bigger than the dispatch brief's own
   grep found**: `pdfce_core::annot::Annotation`'s doc comment states
   explicitly it does NOT model per-subtype geometry (`/L`, `/Vertices`,
   `/InkList`, `/QuadPoints`) — only `/Rect` — "under R43 they are neither
   painted nor... generated from." This splits post-hoc annotation editing
   into two genuinely different-difficulty families, which became the
   spec's central technical finding:
   - **Family A (Rect-bounded: Square/Circle/quad-as-rect/Stamp/FreeText/
     Widget)** — PDF's own §12.5.5 appearance-placement algorithm maps an
     appearance stream's `/BBox` through `/Matrix` into `/Rect`, so move AND
     resize is, in general, a PURE `/Rect` edit with `/AP` untouched —
     appearance-preserving for pdfce-authored AND foreign annotations alike.
     One small core verb (`set_annotation_rect`) covers the whole family.
     **Flagged, not asserted as settled** — this is spec-governed behavior
     this agent recalled rather than sourced from `D:\Dev\Rag-Specialized\
     PDF_Spec\`; the spec requires a `pdfce-spec-librarian` confirmation
     pass before the engineer builds on this premise (rule 1 discipline —
     don't implement spec-governed behavior from training recall).
   - **Family B (point-list: Line/Polygon/PolyLine/Ink/true QuadPoints)** —
     the appearance stream's content IS the point geometry, so move/reshape
     needs the new geometry-read accessor PLUS a reshape verb that
     regenerates `/AP` via the ALREADY-EXISTING `build_appearance` (confirmed
     shipped in `annot_author.rs` — the appearance-generation half of Pass
     6.1's own P1 gating is closed, a genuine good-news correction to file).
     **Honest, disclosed scope cut**: reshaping a FOREIGN Family-B annotation
     via pdfce's own `build_appearance` discards any cosmetic property beyond
     `MarkupSpec`'s model (dash pattern, cloud border, non-modeled line
     endings) — the spec requires a persistent (non-toast) disclosure caption
     before this happens, reusing pass-6.1 §3.3's "quality disclosure, not
     data-provenance" weight class rather than inventing a fourth honesty
     pattern.
   This Family A/B split is also what drove the slicing order (§8) — Family
   A ships as Slice 2 with one small core verb; Family B is Slice 4 with the
   largest core lift in the spec. General lesson: when a "select this and
   edit it" feature spans several PDF object subtypes, check whether the
   underlying spec's OWN placement/appearance model already gives some of
   them a free, appearance-preserving edit path before assuming uniform
   difficulty across subtypes.

5. **`MarkupSpec`/`add_markup`/`build_appearance` in `pdfce-core` already
   ship ALL ten pass-6.1 markup kinds** (Square, Circle, Line, Ink, Polygon,
   PolyLine, and the full Highlight/Underline/StrikeOut/Squiggly quad
   family) — confirmed by reading `annot_author.rs` directly. Pass 6.1's own
   §3.5 "two blockers before P1 can ship" (QuadPoints ordering, appearance
   generation) are BOTH closed. This means the entire Pass 46 draw-gesture
   fix (Slice 1+3) needs **zero new `pdfce-core` work** — it is purely a GUI
   gesture-wiring exercise reusing an already-complete core surface. Only
   the POST-HOC select/move/resize half (Slice 2+4) needs new core work.
   **Always check whether a "spec says core work is needed" note from an
   older spec has since been quietly closed by a later Pass** — two real,
   independent instances of this in one session (this one, and the
   `annot::Annotation` Contents/T/M correction filed against
   [[project_shell_redesign_spec]] the same session).

6. **`run_place_field_tool` (`main.rs:19361`) is the direct, complete
   template for the new Markup drag-rect gesture** — same bridge calls
   (`canvas_to_pdf_space`/`screen_to_page`/`pdf_space_to_canvas`/
   `page_to_screen`), same live rubber-band paint pattern, same
   `MIN_DRAG`-threshold click-vs-drag distinction, same drag-anchor state
   field shape. Read it before writing any new drag-rect gesture in this
   app; it is more directly reusable than re-deriving from the Pass 12.0
   substrate docs alone.

7. **New three-way click-priority chain, extending an existing one-line
   rule**: ce dimension → annotation → content-stream object, first hit
   wins (was previously only a two-way "ce dimension beats object selection"
   rule, from the tool-options-dock spec). Reasoned from §12.5.2's paint
   order (annotations paint over content) plus pdfce's own dimensions being
   an authored overlay on top of everything else.

8. **`MeasureCircular`'s "one tool, one display toggle" AND `PlaceField`'s
   "one tool, a type selector in Tool Options" are BOTH cited as the
   precedent for collapsing all ten markup kinds into ONE `CanvasTool::
   Markup` variant** (not ten `TOOL_PRECEDENCE` entries) — this is the third
   use of that specific reasoning pattern in this project's tool-design
   history (`MeasureCircular`, `PlaceField`, now `Markup`), worth citing
   directly the next time a "should this be N tools or 1 tool with a
   selector" question comes up rather than re-deriving the argument.

9. **`GuiMarkupKind`/`Action::AddMarkupShape`/`add_markup_shape` are
   specified for DELETION, not deprecation-beside** — the spec names this
   explicitly (no case where the old centered-jitter placement stays useful
   once a real gesture exists) and flags to the engineer that this touches
   shipped code/tests, grep before deleting.

Read the full spec at `docs/ui_specs/pass-46-canvas-interaction-model.md`
before implementing or reviewing any part of it — §0's supersession table
against `pass-6.1-markup-tools.md` is the map between the two documents and
should be read first by anyone who has pass-6.1 in mind as "the" markup spec.
