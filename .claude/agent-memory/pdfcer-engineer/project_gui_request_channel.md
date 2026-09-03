---
name: gui-request-channel
description: MANDATORY every session — check D:\Dev\FeatureRequests\pdfce_FeatureRequests for requests from the pdfceGUI project; it is the only channel between the two sessions
metadata:
  type: project
---

**Operator instruction, 2026-08-13:** notes for the `pdfceGUI` project go
in **`D:\Dev\FeatureRequests\pdfce_FeatureRequests`**, and *"you should
also set yourself to check that folder for anything that it finds and needs
you to fix or work on."*

**How to apply — this is a session-start obligation, not an if-you-think-of-it:**

1. **At the start of every pdfce session, list that folder.** It sits
   alongside reading `docs/NEXT_SESSION.md` and `docs/ROADMAP.md`.
   `request_*.md` = something the GUI session needs from core/CLI.
2. **Triage each request into the roadmap** — dispatch `pdfce-librarian` to
   file it as a Pass or Backlog entry, so it is tracked where all other
   work is tracked rather than living only in a folder nobody greps.
3. **Rename to `done_*.md` when closed**, and write a `note_*.md` back if
   the answer is "here is what to call instead" rather than a code change.
4. **Requests are FINDINGS, not favours.** Decision 058: anything the GUI
   project needs moved in core is *a place the boundary was drawn wrong*.
   Treat a workaround the GUI had to invent as a defect report about
   `pdfce-core`, and say so when filing it.
5. The folder's `README.md` is the outbound briefing — it points at
   `D:\Dev\pdfce\docs\core-api\index.md` (the consumer API map), the five
   traps most likely to bite a new shell, the two known defects
   (`Pass 72.0` redaction proof, `Pass 73.0` R58 layer), and the two
   non-negotiables the shell inherits (rule 4, rule 15). **Keep it current
   when core changes under it** — a stale briefing is worse than none,
   because it is trusted.

**Why a channel exists at all:** the two sessions cannot ask each other
questions in real time, and the GUI is being rebuilt from scratch at
`D:\dev\pdfceGUI` after the current shell was judged unusable — see
[[gui-work-paused]] and `ARCHITECTURE.md` decision 058.
