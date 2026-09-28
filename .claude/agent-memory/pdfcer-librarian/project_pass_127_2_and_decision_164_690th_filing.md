---
name: project-pass-127-2-and-decision-164-690th-filing
description: Pass 127.2 (redact-mark stdout diagnostics field) shipped + decision 164 minted, 690th filing; a Backlog entry's own filed text cited a stale decision number (088) against a ceiling that had moved on
metadata:
  type: project
---

2026-09-28, `30a5c6c5`, 690th filing: `Pass 127.2` closed a Backlog entry
filed 2026-08-26 (263rd filing) that owed both CLI code (stdout diagnostics
field on `redact-mark`, mirroring `find-text`) and a decision (whether an
unreadable-text finding should exit non-zero — answered **no**, minted as
**decision 164**).

**Why worth remembering:** the Backlog entry's own filed text had predicted
the closing decision would be numbered `088`, because that was the ceiling
back in the 263rd filing. By the 690th filing the ceiling had moved to `163`
independently (unrelated Passes), so the actual closing decision is `164`,
not `088`. **Never trust a decision/Pass number predicted inside an old
Backlog entry** — always re-grep the live ceiling at filing time, even when
the entry you're closing already named a number. Handled here by keeping the
old entry's prediction verbatim (citation-valid) inside a `<details>` block
and adding a dated correction note pointing at the real number, rather than
editing the stale prediction in place.

**How to apply:** when discharging any Backlog entry that names a specific
future decision/Pass/rule number, grep the current ceiling before using that
number — treat the entry's own prediction as untrusted until reverified.

See [[feedback_verify_relayed_corrections_via_grep_before_accepting]] for the
sibling discipline (verify relayed claims against live files).
