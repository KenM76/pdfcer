---
name: no-oracle-extract-and-test
description: When a GUI rule cannot be reached by the diag harness, extract it as a pure function and unit-test it — and say out loud that this substitutes for in-app verification rather than equalling it
metadata:
  type: feedback
---

Some GUI behaviour has **no in-app oracle**: the scripted-input harness cannot
produce the event that triggers it. The known case is any rule keyed on an egui
*drag* — injected pointer events do not make `drag_started()` / `drag_stopped()`
fire (recorded in
`D:/dev/rag/egui/eframe_035_raw_input_hook_synthetic_event_injection.md`), so a
`DragValue`'s commit-on-release rule is unreachable from `tools/gui-drive.ps1`.

**The rule:** pull the decision out of the UI closure into a plain function with
its inputs as parameters, unit-test every branch, and **state the limit in the
doc comment and the commit message** — "a pure function with a unit test is the
honest substitute, not equivalent to in-app verification."

**Why:** R86 says a GUI defect is settled in the running application. When that
is genuinely impossible, the failure mode is not the missing test — it is
*reporting the feature as verified*. Ken cannot audit a claim whose evidence
does not exist. Naming the gap costs one sentence; discovering it later costs
his trust in every other verification claim in the same message.

**How to apply:** first try the harness — most things ARE reachable, and
"unreachable" is often really "I did not trace the widget's rect" or "I drove
the debug binary" (see [[reference_gui_diag_harness]]). Only after the harness
genuinely cannot produce the event, extract and test. Applied at Pass 34.2
(`place_draft_commit`, 5 tests).

Related: [[feedback_engineer_does_the_observing]],
[[feedback_gates_i_owe_myself]].
