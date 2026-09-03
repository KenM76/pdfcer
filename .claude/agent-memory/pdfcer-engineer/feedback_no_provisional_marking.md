---
name: no-provisional-marking
description: Ken, 2026-08-13 — inferred content renders NORMALLY; commit point is Save; disclosure goes off-canvas. Never badge/tint/flag applied content as pending.
metadata:
  type: feedback
---

Inferred content (OCR text, typed edits, reflow, snaps, substituted fonts)
must render **exactly as saved content will render** — live, normal, reflowed.
The commit point is **Save**; **undo rejects**. Disclosure of what pdfce
guessed lives **off-canvas** (status line, results panel, post-command report),
never as a badge, tint, red flag, dashed outline or "provisional" layer drawn
into the page view, and never as an accept/reject gate.

**Why:** Ken, verbatim — *"As a user I just want to type in an existing gui
text box and have it look normal and reflow normal. I want OCRed stuff to look
normal when the command is executed too. I only expect things to be committed
when I hit save, and not commited if I hit undo. The nagging and red flagging
in the original GUI made for a lot of extra bugs in the visibility when
editing."*

That last sentence makes it a **correctness** rule, not a taste one: every
provisional-state marking is a **second rendering path for the same content**,
and two paths drift. Deleting the marking machinery removes a bug class. A
future session that reads this as friction-reduction will trade it back for a
"helpful" highlight and reintroduce the defects.

**How to apply:** this is the second narrowing of project rule 4 (the first was
decision 024 §4.4, on confirm boxes positioned relative to the page). Both were
prompted by Ken noticing shipped friction rather than by review — so **rule 4
has been consistently over-read as a mandate for visible machinery when it was
only ever a mandate for non-silence.** Full record: decision **059**,
`ARCHITECTURE.md` §12, and `CLAUDE.md` rule 4.

What survives, and it is the point of the rule: inferences Ken **cannot see**
still owe an off-canvas report — invisible OCR text at render mode 3, a
plausible font substitution, a best-fit residual, an over-eager snap. **Render
normally; report separately. Both.** The CLI's obligation is the mirror image
and untouched: its invocation *is* the commit, so it prints what it inferred.
What rule 4 forbids is **silence**, in both shells.

One distinction that keeps this workable: a **pre-commit affordance** is not
content marking. Snap indicators, hover highlights, rubber-bands and selection
handles are the *cursor* — they describe what is about to happen, and they are
welcome. What is forbidden is styling content **already applied** as pending.

One-line test: *would a screenshot of the editing canvas differ from a
screenshot of the same document saved and reopened?* If yes, and the difference
is pdfce marking its own uncertainty, that is the defect.

Related: [[project_gui_request_channel]], [[project_gui_work_paused]].
