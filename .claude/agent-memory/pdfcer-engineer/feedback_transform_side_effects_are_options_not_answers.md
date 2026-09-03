---
name: transform-side-effects-are-options-not-answers
description: Ken 2026-08-28 — when a transform raises "does X scale too?", ship an OPTION with the safe default, matching Inkscape's transform toggles; do not settle it as a fixed answer even when every reference program agrees
metadata:
  type: feedback
---

**When a transform raises a "does *this* scale too?" question — stroke width,
corner radii, insets, gradients, patterns — the answer is an OPTION with a
sensible default, never a fixed behaviour.** Ken, 2026-08-28, verbatim:

> *"default should be what it said, but there should be an option that they do
> scale with resize. Inkscape has options for this and I want the same."*

**Why:** Inkscape puts these on the selector tool's control bar as toggles —
*Scale stroke width*, *Scale rounded corners*, *Move gradients*, *Move
patterns* — plus the matching Preferences → Behavior → Transforms entries.
Illustrator has *Scale Strokes & Effects*. **The existence of the toggle is the
industry answer**, not the value it defaults to. [[inkscape-parity-scope]] is a
stated scope target, so matching the toggle set is parity work, not a
nice-to-have.

**★ What made this worth a memory:** `pdfceGUI` answered pdfce's own design
question with *"stroke width does NOT scale"*, and backed it with three good
arguments — a CAD line weight is a drafting standard; a non-uniform scale makes
the value ill-defined; Acrobat and Illustrator both agree. **Every argument was
sound and the conclusion was still too narrow.** I was about to accept it as
settled because it was well-reasoned and convergent across reference programs.

⇒ **Convergence among reference implementations argues for a DEFAULT, not
against an option.** "Everyone does X" and "nobody should be able to do Y" are
different claims, and the first does not establish the second.

**How to apply:** on any transform verb, enumerate the properties that *could*
travel with the geometry and expose each as a flag, defaulting to whatever the
reference programs do. Say so in the outcome type either way, so the operator
knows which branch ran — the same disclosure `geometry_keys_moved` provides for
a move. Where a caller does not set them, behaviour is unchanged from before
the option existed.

This is the same shape as [[spec-ambiguity-defaults-are-mine]] (two defensible
answers → ship both, pick the default) and
[[security-defaults-lean-safe-plus-stricter-mode]] (he takes the safe default
*and* wants the other rung reachable). Three instances now: spec ambiguity,
security posture, transform side effects. **Ken's consistent preference is a
ladder with a safe default, not a decision made on his behalf.**
