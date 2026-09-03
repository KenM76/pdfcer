---
name: refusals-guard-their-callers
description: Before scoping work from a refusal, re-verify its stated reason; before removing one, audit its callers — both failures happened in a single session
metadata:
  type: feedback
---

A refusal in `pdfce-core` is rarely just a refusal. Treat it as two separate
risks, and check both.

**Why:** both failed in the same session (2026-08-05), on the same Pass.

*Self-justification.* The composite-text refusal (R-INV-4) said the re-encoding
path was "single-byte end to end", and the three narrowings that made it
single-byte each cited the refusal as their justification. A prior session
surveyed four coupled pieces of work from that reasoning. Re-surveying found
most of it already built — the composite encoder existed, was tested, and was
called by nothing. The limitation had outlived its cause and was being
re-derived from its own consequences. Now standing rule **R143**.

*Incidental protection.* Pass 30.0 made `re` rectangle corners draggable. The
GUI had a node-drag gesture with no rung gate at all, and `Subpath::anchors()`
has always included those corners — so before 30.0 the drag classified and was
then *refused on release*. The refusal was the only thing stopping it. After
30.0 the same drag silently succeeded, including on clipping paths, where the
visible effect lands elsewhere on the page. Now standing rules **R144**
(the protection can vanish) and **R147** (audit the callers, not the module).

R144 was filed from the clip-path case earlier in that same session, and then
the second instance shipped anyway — because the reasoning stayed inside core
and never looked at consumers.

**How to apply:**

- Before letting a refusal scope work, read the code it claims is constrained.
  The stated reason may be describing a consequence of the refusal rather than
  a cause of it.
- Before removing a refusal, grep for its error variant AND for the gestures
  that reach the guarded call. The protection is felt where the function is
  invoked, not where it is defined — core-side reasoning cannot see it.
- A refusal acting as a *gate* should be **replaced**, not merely deleted.
- Prefer fixing the whole class: `re` corners and inherited subpath starts had
  the same root cause (no operand names that point) and the same fix
  (materialize one), so they shipped together rather than as two special cases.

See [[editing-arc-state]] for what this arc actually shipped.
