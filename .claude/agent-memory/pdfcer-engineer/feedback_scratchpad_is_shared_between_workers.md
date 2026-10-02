---
name: scratchpad-is-shared-between-workers
description: parallel worktree workers in one session share ONE scratchpad dir; generic file names (sabotage.py, msg.txt) clobber each other
metadata:
  type: feedback
---

Parallel pdfcer-engineer workers dispatched from one session get the same scratchpad
directory. On 2026-10-02 my `sabotage.py` overwrote the paddle-vl worker's script of the
same name, and theirs then overwrote mine (each driver points at its own worktree, so a
late write runs sabotage against the WRONG tree).

**Why:** the scratchpad is keyed by session, not by worktree/agent.
**How to apply:** prefix every scratchpad file with the Pass ID (`p4423-sabotage.py`,
`p4423-commit-msg.txt`), and re-read a driver right before launching it.
Related: [[git-add-all-is-unsafe-with-live-subagents]].
