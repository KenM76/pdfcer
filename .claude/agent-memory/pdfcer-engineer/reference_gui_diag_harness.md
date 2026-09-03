---
name: gui-diag-harness
description: GONE since Pass 247.0 (2026-09-03) — the in-repo GUI crate and its PDFCE_DIAG harness left with it; GUI defects are pdfcer-gui's, driven by that project's own harness
metadata:
  type: reference
---

**REMOVED in Pass 247.0 (2026-09-03)** with `crates/pdfce-gui`. Readable at
`git -C D:\Dev\pdfce show cce414e:crates/pdfce-gui/src/diag.rs` if pdfcer-gui
ever wants the design. Kept for the lessons (two window sizes, 45 idle frames).

`crates/pdfce-gui/src/diag.rs` + `tools/gui-drive.ps1` + `tools/gui-shot.ps1`
(built 2026-08-04). Three environment variables, all off by default:

- `PDFCE_DIAG=1` — trace to **stderr**, `key=value` lines prefixed
  `pdfce-diag`. Call sites are permanent and cost nothing when off.
- `PDFCE_DIAG_VIEWPORT=x,y,w,h` — place the window there and mark it
  inactive. `-4000,-4000,1600,1000` puts it off every plausible monitor
  while it still lays out and interacts normally.
- `PDFCE_DIAG_SCRIPT="wait;move:x,y;down:x,y;up:x,y;zoom:2.0;mdown:x,y;mup:x,y;delete;tool:obj;tool:none"`
  — one step per frame, injected through eframe's `raw_input_hook`. The
  script running dry closes the window, so a run lasts exactly as long as
  its script.

**Why this exists rather than screenshots.** R86 says a GUI defect is settled
in the running app, but Ken is usually working at that machine. This makes the
oracle available without touching his screen. Check idle time first if unsure:
`GetLastInputInfo` via P/Invoke — under ~60 s means he is actively at the
keyboard.

**How to use it well.** Print the geometry (`rect=`, `zoom=`) from one run,
then compute the screen point you actually want to click from PDF coordinates:
`canvas_y = page_height - pdf_y`, `screen = image_rect.min + canvas * zoom`.
Hard-coded screen points silently stop hitting anything when the layout
changes — that happened the same day, after the status panel gained a fixed
height.

**Traps, each of which cost a run.**
- **Both scripts default to `target/release/pdfce-gui.exe`.** `cargo build -p
  pdfce-gui` builds DEBUG, so a run after that drives the *old* binary and your
  new trace lines simply never appear — which reads exactly like "the code path
  is never reached". Cost a full misdiagnosis on 2026-08-05 (Pass 34.2).
  `cargo build --release -p pdfce-gui` before every drive, or pass `-Exe`.
  Confirm with `strings target/release/pdfce-gui.exe | grep <your-trace-tag>`.
  Kill stray `pdfce-gui` processes first or the link step fails with
  `Access is denied. (os error 5)`.
- **Trace what you want to CLICK, not just what you want to know.** To drive a
  widget you need its rect; add a `diag::trace` of `response.rect` and read the
  point off one run, then click it in the next. Guessing a widget's position and
  missing is indistinguishable from a control that does not work.
- **Gate a per-frame trace on pointer activity** (`i.pointer.any_down() ||
  any_released()`), the way the `dim-rects` line does — an ungated per-frame
  line buries the events worth grepping for. But note the consequence: the LAST
  traced state is from the release frame, i.e. *before* that frame's mutation
  lands. To read back the resulting state, drive one more harmless click.
- `PostMessage(WM_LBUTTONDOWN)` to the window does NOT work off-screen: winit
  calls `TrackMouseEvent`, Windows answers `WM_MOUSELEAVE` because the real
  cursor is elsewhere, and egui-winit drops the button because it emits
  `PointerButton` only when it knows the pointer position. Inject at egui's
  seam instead.
- An empty trace is ambiguous. There is an unconditional `start …` line for
  exactly this reason — without it, "diag was never enabled" and "nothing
  happened" look identical.

**`gui-shot.ps1` photographs the SCREEN REGION, not the window.** Two distinct
failures, both of which produce a file that looks like evidence and is not:

1. **Not foreground → you photograph another app.** On 2026-08-05 it returned a
   pixel-perfect screenshot of SolidWorks while pdfce ran correctly behind it.
   Fixed in-script with `SetForegroundWindow` (best-effort by Windows' rules).
2. **The DISPLAY IS ASLEEP → uniform white/black.** `CopyFromScreen` reads the
   composited desktop; a powered-down display has nothing to read. Ken fixed it
   by setting the display to stay on.

**I got (2) wrong first, and the way I got it wrong is the lesson.** I saw the
blank, invented a DWM-recomposite race, raised the settle 700 ms → 2500 ms,
saw it work, and wrote the cause into a comment as fact. Re-measured after Ken
identified the real cause: **three consecutive captures at 700 ms, all fine.**
The sleep bought nothing. The tell I ignored at the time — blanks came back
later at a *20 s* wait, which a recomposite race cannot explain.

`gui-shot.ps1` now **refuses a near-uniform capture with a loud warning** that
lists the known causes in the order they have occurred. That is the fix that
matters, not the sleep: a silent failure is what lets a plausible story get
attached to an unexamined symptom. Verified by making it fail on purpose
(capture an off-screen region → warns; real capture → silent).

Generalisable: **a change that appears to fix something is not evidence for
why.** If the cause was not tested, say so instead of naming one. Lives in
`D:/dev/rag/rust/trust_but_verify_doc_comments_are_not_evidence.md`, which now
holds **seven** occurrences on this project.

**Postscript, and it is not a joke.** In the commit correcting myself for
asserting an untested CAUSE, I asserted an untested COUNT — said "five times"
from memory of when I had last read that file, twice, including to Ken. It held
six. Same failure, one paragraph later, about the very file that documents it.
**Check the artifact, not your memory of the artifact** — the cost of `grep` is
seconds and the cost of a confident wrong number is that someone believes it.

Related: [[feedback_engineer_does_the_observing]] — this is the tool that makes
that rule cheap to follow. [[reference_clap_windows_stack]] for the other
Windows-specific launch gotcha.
