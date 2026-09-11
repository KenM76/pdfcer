---
name: project-pass-296-batch-no-shell-and-r251-declined
description: Pass 296.0-296.4 filed (509th filing) with no shell tool granted; R252 minted; R251's "shape" checked against its actual mechanism and declined for 2 of 4 cited instances
metadata:
  type: project
---

2026-09-11, 509th filing. Filed five already-shipped Passes (296.0-296.4,
commits 69d4d67/141c989/90576a8/5943beb/8d2f6bb) answering pdfcer-gui's
G002-G006 requests. Two things worth remembering:

**No Bash tool was granted this invocation** (Read/Write/Edit/Glob/Grep/
WebSearch/WebFetch only). Per hard rule 8 and the task's own fallback
instruction, wrote the commit message to
`C:\Users\Ken\AppData\Local\Temp\pdfcer_librarian_509th_filing_commit_msg.txt`
instead of running `git commit -F`, and disclosed in both the commit
message and the ARCHITECTURE.md SS12 entry exactly which claims were
independently Grep-verified against the live tree vs relayed from the
dispatching engineer's summary. Do not assume a shell is available just
because prior filings had one — check the actual tool list every time.

**R251 was NOT mechanically extended.** The dispatch's own framing called
Pass 296.1 (`remedy_faces` as data) and 296.2 (`Display` impl) instances of
"R251's shape recurring across all five" Passes. Checked against R251's
actual minted mechanism (a re-export gap `check-reexport-closure.py`
catches) and neither matches — a report existing only as prose and a
missing trait impl are not re-export gaps. Declined to file them as R251
dated instances; filed a cross-cutting *observation* in ROADMAP.md's
Standing rules instead, citing this project's own
[[feedback_pattern_naming_needs_shared_mechanism_not_shared_moral]] finding
(a shared *moral* — "invisible to the defining crate" — is not a shared
*mechanism* unless a fix for one would have caught the others). Worth
checking on a future filing whether a third/fourth instance finally shares
an actual fix, at which point a real mint (R151-adjacent, or new) would be
warranted.

Also confirmed via Grep (not asked to, but worth remembering as a working
method): `docs/core-api/index.md`'s 227-verb count was independently
plausible-checked rather than taken purely on the requester's word, by
cross-referencing the two new verb names from Pass 296.3 against
`crates/pdfcer-core/src/edit.rs` and `crates/pdfcer-cli/src/main.rs`.
