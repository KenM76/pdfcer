---
name: engineer-does-the-observing
description: Ken will not be the beta tester — the engineer must verify operator-facing behavior in the running app itself, and never block production waiting for Ken to test
metadata:
  type: feedback
---

A Pass that adds or changes operator-facing behavior is not done until that
behavior has been **observed working in the running application** — and
**the engineer does the observing, not Ken.**

**Why:** Ken accepted this as a standing rule on 2026-08-02 with an explicit
condition: *"Yes, but only if you can do the observing. I don't want to be a
beta tester. Otherwise, don't hold up production by waiting on me to test."*

The rule exists because of the decision-018 defect: the GUI rendered the
**base** PDF revision instead of the `EditSession` overlay, so every editing
feature from Pass 3.1 through 16.2 was authored correctly and was **invisible**
in the GUI. Every one of those Passes met its stated gates — headless tests
green, invariants verified, release binary launched — and shipped a feature the
operator could not see. It was a *gate* defect, not an engineering defect. See
[[project_pass17_live_edit_rendering]].

**How to apply:**
- Build and use an automated observation capability (window screenshots read
  back visually, and/or an `egui_kittest`-style harness). Treat that capability
  as a prerequisite of the rule, not a nice-to-have — Ken's "yes" was
  conditional on it.
- Never end a turn with "please test this and tell me if it works" as the
  verification step. Verify first, then report what was observed.
- If observation is genuinely impossible for some behavior, say so explicitly
  and ship anyway rather than stalling — "don't hold up production" is the
  other half of the instruction and carries equal weight.
- Launching the app is still expected (see [[feedback_launch_on_completion]]),
  but launching is not observing. Confirming a process exists proves nothing
  about what is on screen.
