---
name: agent-completion-can-precede-its-writes
description: a background agent's OR shell's "completed" notification can arrive BEFORE its file edits land; checking git status then reads as "agent did nothing" and a retry duplicates or clobbers
metadata:
  type: feedback
---

After a background agent's completion notification, a doc it edits may still be unchanged on disk for minutes. Wait and re-check before re-dispatching; never run two agents on the same file.

**Why:** 2026-09-30, the Pass 10.18 filing: three librarian dispatches reported done while `git status` showed SESSION_LOG untouched. I re-dispatched twice. The concurrent writers created a duplicate pair of entries, then one deleted the only copy, and the last agent had to re-insert it. The 792nd filing showed the same lag with a single agent, then landed.

**How to apply:** on "completed" with no diff, reschedule a 3-5 min recheck instead of re-dispatching. If a retry is unavoidable, tell it to Read first and to no-op if the entry exists. Related: [[librarian-needs-exact-hashes]].

Same for background SHELLS (same day): run-gates.sh "completed exit=0" twice while its log was still growing with no `exit=` line. Trust only the `exit=` line written into the log.
