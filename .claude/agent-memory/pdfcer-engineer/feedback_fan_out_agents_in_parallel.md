---
name: fan-out-agents-in-parallel
description: Ken, 2026-08-31 — use more agents in parallel to speed things up; independent investigation threads should be dispatched together, not worked serially
metadata:
  type: feedback
---

**Dispatch agents in parallel, freely, whenever threads are independent.**

Ken, 2026-08-31, unprompted, mid-session: *"feel free to use more agents in
parallel to speed things up."*

**Why:** he had watched a session that shipped three Passes almost entirely
serially — read the request, verify it, fix it, test it, sabotage it, document
it, then start the next one. Several stretches were pure waiting (a 14-minute
gate sweep, a 7-minute fuzz run, a 23-minute librarian filing) with nothing
running beside them. The work was right; the wall-clock was not.

This is the *throughput* half of the standing global rule that subagents never
need permission ([[dispatch-subagents-without-asking]]). That rule removed the
asking; this one removes the seriality.

**How to apply:**

- **Send independent dispatches in ONE message**, several tool calls at once.
  Two agents launched together finish in the time of the slower one.
- **Fan out on investigation before converging on implementation.** Verifying
  three inbound requests against the code, auditing three separate defect
  candidates, or surveying three crates are all independent reads and should go
  out together.
- **Use the waiting.** A long gate sweep, a fuzz run or a librarian filing is
  dead time on my side; that is exactly when a parallel investigation should be
  in flight. `CLAUDE_CODE_MAX_SUBAGENTS_PER_SESSION` is 2000 — the machine is
  configured for it deliberately.
- **What still stays serial**, and the distinction matters: anything that
  writes to the *same files*. Two agents editing `edit.rs` will clobber each
  other, and `git add -A` beside a live subagent picks up its scratch files
  ([[git-add-all-is-unsafe-with-live-subagents]]). Parallelise **reads,
  audits, surveys and independent-file work**; serialise **writes to one
  file**, or give each agent its own worktree.
- **Do not parallelise a thing whose result changes the next step.** A sabotage
  run and its follow-up fix are one thread by nature. Fanning those out
  produces confident answers to questions that have already moved.

**The judgement to keep:** parallel is for breadth. It does not replace running
one thing properly — the measure-then-fix, sabotage-then-believe discipline is
unchanged, it just happens on several tracks at once.
