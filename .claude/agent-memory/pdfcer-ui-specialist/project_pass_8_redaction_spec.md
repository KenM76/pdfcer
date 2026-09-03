---
name: project-pass-8-redaction-spec
description: Key decisions from the Pass 8 (Redaction mark/review/apply) UI spec authored at docs/ui_specs/pass-8-redaction.md, 2026-07-31
metadata:
  type: project
---

Authored the full implementable Pass 8 GUI spec (redaction mark → review →
apply, plus Sanitize) at `docs/ui_specs/pass-8-redaction.md`, on dispatch
from the engineer. Correctness-is-security framing throughout — every
choice pressure-tested against "can this UI make an operator believe
content is gone when it isn't?" Key decisions, useful when later reviewing
the actual implementation against this spec:

**P0 recommendation is CLI-first + placeholder, not a cut-down GUI.**
Given this is the single most security-critical feature in the app, the
spec's minimal-but-honest P0 is: core CLI `redact mark`/`redact apply`
fully functional, GUI ships only the `tool_available_in_cli`-pattern
placeholder (same function/pattern as Pass 3.2's Split/Insert), PLUS one
non-negotiable item that ships regardless of GUI completeness: a persistent
status-bar disclosure of unapplied `/Redact` marks in the open document
(computed from the actual annotation census, not a session counter — must
fire even for CLI-authored marks opened later in the GUI). This directly
targets the Acrobat-parity RAG's #1 documented real-world failure ("missed
Apply, file shared as if finished"). **Check this shipped even if
everything else in the spec was cut** — it's the cheapest, highest-value
item in the whole spec.

**Load-bearing dependency flag discovered while reading precedent:** Pass
6.1's own `pass-6.1-markup-tools.md` spec designed a full canvas tool-mode
drag/marquee state machine (`active_tool`, `DrawState`, canvas
focusability, drag-vs-pan suppression) — but per ROADMAP's Pass 6.1
Shipped entry, that infrastructure was NOT what actually shipped (Pass 6.1
GUI shipped only a minimal centered-rect menu affordance; the full canvas
machine is a named, unshipped follow-up slice). Pass 8's own Mark-phase
canvas drag-rectangle tool depends on that SAME infrastructure. Flagged
prominently (§1.1) that before building it, the engineer must check whether
it landed yet — if not, do NOT build a second one-off drag implementation
just for redaction; ship P0 (CLI-first) instead. **This is worth checking
first thing if dispatched to review the actual Pass 8 GUI implementation** —
ask whether the canvas tool-mode infra existed when Pass 8 was built, and
if it didn't, verify the engineer correctly fell back to P0 rather than
building a parallel/diverging drag-tool implementation.

**Visual language for marked-vs-applied (the task's explicit #1 ask):**
pre-apply mark = translucent diagonal-hatch pattern + red outline + a
"MARKED" corner tag — deliberately NEVER a solid fill of any color, so it
cannot be mistaken for the post-apply result even in a quick glance or a
grayscale screenshot. Post-apply = the actual black (or configured) fill
baked directly into page content — indistinguishable from ordinary page
content, which is correct because post-apply there's no "mark" object left
to distinguish. The two states are never rendered by the same code path.

**Two new placement-taxonomy instances recorded (flagged to librarian):**
(a) the Redact review panel is a NEW seventh taxonomy instance — a
dedicated secondary panel distinct from the Tools dock, because the dock's
own intro sentence ("these tools work with files outside the one you have
open") would be falsified by redaction marking the OPEN document — same
reasoning Pass 4 already used to keep copy-text out of the dock, applied
here with more force. (b) a new THIRD confirmation-dialog convention: a
resizable, larger (760×560ish), scrollable `egui::Window` — deliberately
NOT the existing 520px-fixed convention (`signature_confirmation`/
`copy_confirmation`) — because the Apply report's length genuinely varies
and cramming it into a fixed small box would bury the exact thing the task
demanded be a centerpiece. Shared verbatim by both redaction-apply and
sanitize-apply (one new pattern class, not two).

**Genuine finding, not just a UI call — Sanitize needs R35's forced-full-
rewrite discipline too, and the standing rules don't yet say so
explicitly.** Sanitize's whole purpose is removing document-wide traces
that would otherwise be recoverable (metadata history, orphaned objects).
If it only did an incremental save, the "removed" data would still be
trivially recoverable in the prior revision — defeating its own purpose,
the same logic R35 already applies to redaction. Flagged this to the
librarian as worth a one-line addition to the R35/R38 record next time
ARCHITECTURE.md §5 is touched, so a future reader doesn't have to re-derive
it from the UI spec.

**Deliberate no-keyboard-shortcut design for the single most destructive
action in the app:** Apply (redaction and sanitize both) get NO keyboard
chord anywhere — not to open the modal, not to confirm it — unlike every
other destructive action in pdfce (Delete key, `[`/`]` for rotate), because
those are reversible pre-save and this is not once saved. Also flagged as
an implementation-check item: verify egui/eframe doesn't bind Enter to a
default button on this specific window (an operator reading a long report
and hitting Enter out of habit must never accidentally commit it).

**Refusal-acknowledgement gate (§4.4) is the single most important design
element in the whole spec:** if pdfce must refuse to destroy any carrier
under a mark (an unre-encodable image format, an XFA parallel copy, etc.),
the Apply button stays disabled until a SEPARATE checkbox (distinct from
the ordinary confirm checkbox) explicitly acknowledges the specific
residual — there is no path in the design where a partial redaction can be
mistaken for a complete one. If reviewing the shipped implementation, this
is the first thing to verify actually gates the button, not just displays
a warning.
