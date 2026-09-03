---
name: project-pass-232-and-r217-fifth-amendment
description: Pass 232.0 (comment-only overprint fix) + v0.20.0 release filed together (373rd filing); R217's own author pushed both commits unfiled hours after minting R217's 4th note — R217 gained a 5th amendment note, no new rule number
metadata:
  type: project
---

2026-09-02 (373rd filing), `86b83f4` (`Pass 232.0`) + `599dec6` (v0.20.0
lockfile follow-up, part of the release). No shell this filing; both commit
bodies supplied verbatim via `.librarian-two.txt`, cross-checked against live
source (`cmyk_buffer.rs:1654-1669`, `interpret.rs:5292-5314` corrected;
`interpret.rs:6339-6343` + `overprint.rs:204-210` deliberately left alone —
genuinely on the `Separation`/`DeviceN` row).

**`Pass 232.0` is comment-only** (no Pass ID needed for `599dec6` itself —
same treatment as `v0.19.0`'s release commit `d19d4e4`, no acceptance
criteria, mechanical). The release (`v0.20.0`) is recorded in `ROADMAP.md`'s
Shipped section as a titled entry with **no Pass ID**, same convention as the
`v0.19.0` precedent (`Pass 213.0`'s entry, "Release note (recorded, not
performed)").

**The notable finding: `R217`'s own author (the engineer) pushed both these
commits unfiled**, a few hours after minting `R217`'s FOURTH amendment note
("GitHub runs one job per push — pair code+filing in the SAME push"). Why it
was missed: the push *did* contain a filing commit in recent history (the
372nd filing's librarian commit) — but that commit filed `Pass 231.0`, a
DIFFERENT Pass, not these two. "A push contains a filing commit" and "a push
contains the filing commit for its own commits" are different facts, and the
first substituted for checking the second. Same shape as hard rule 11(e) (a
match that isn't *the* match reads as green).

**Fix: `R217` gained a FIFTH amendment note, not a new rule number** — *run
`tools/check-commits-filed.py` immediately before every push, not merely
"commit a filing alongside."* The engineer's own proposed sharpening,
accepted verbatim. Full text: `ROADMAP.md`'s `R217` entry (Standing rules
section), between the 4th amendment note and "Ceiling moves R216 → R217."

**Takeaway for future filings:** when a self-report of a process failure
arrives, file it as an amendment note under the rule it violates rather than
a new mint, if the failure is the same subject (deferral-window discipline)
seen from a different angle. Fourth note = timing (pair in one push); fifth
note = verification (actually run the gate, don't infer from "a filing
happened recently").

See also [[feedback_dispatch_should_carry_git_evidence_when_no_shell]] — this
filing had no shell either, and the process-failure report was relayed and
cross-checked the same way (live-source Read/Grep, not git).
