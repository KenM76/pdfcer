---
name: project-pass-288-1-edit-source-and-r243-instance
description: Pass 288.1 (stamp-pack --stamps-from) + tools/edit-source.py filed together (492nd filing); R243 dated-instance pattern (written warning hit anyway = missing tool) and R250's owed master-list entry
metadata:
  type: project
---

**2026-09-10, 492nd filing.** Two commits filed together because a
pre-push gate blocked the first (`aeeecb5`, `tools/edit-source.py`) until
this filing landed: `4b45a96` = `Pass 288.1` (`stamp-pack --stamps-from`,
a name-list file so a 113-page stamp sheet doesn't need 113 `--stamp`
flags — no capability-box change in `FEATURES.md`, row 288's `cli` was
already `[x]`). `aeeecb5` = `tools/edit-source.py`, a line-ending-agnostic
exact-replace tool built after a multi-line `str.replace` silently
no-op'd on a CRLF file three times in one session **despite an existing
written warning** in `docs/NEXT_SESSION.md`.

**Filed the CRLF-tool finding as a dated instance of `R243`** ("a
documented obligation on a future caller is not a control"), not a new
mint — `R243`'s own mechanism already covers it one layer out (a warning
failing to stop a *repeated manual action*, not two call sites failing to
agree on a value). The remedy differs from `R243`'s usual "extract into a
shared function" (no function to extract from a human/agent re-typing by
hand) — recorded as its own dated-instance sub-reasoning, same shape as
the 469th-filing `# Errors`-doc-block note already on `R243`.

**Flagged, not written:** the CRLF/`str.replace` gotcha itself belongs to
`troubleshooting-librarian` (`personal_rag/claude_code` or
`personal_rag/python`) — general Python-scripting-under-Claude-Code, not
PDF-domain, not Rust/egui-ecosystem. Outside every tier this role owns;
see [[pdfcer_librarian_feedback_session_log_continuation_style]] for the
sibling-boundary pattern this follows (flag another role's corpus, don't
edit it).

**Owed-item discipline confirmed working:** `R250` was minted 491st
filing but only in the Shipped-section banner — its *Standing rules*
master-list entry was still owed. Caught by re-reading the master list
(not just grepping "R250") before filing this entry; discharged this
filing, right before "## Update protocol". **Lesson for future filings:**
when a rule/decision is minted, check whether its *master-list* entry
(not just the Shipped-banner mention) actually landed — the two are
separate edits and the banner can ship without the list entry.

**No shell this filing** (hard rule 8) — both commit hashes came from the
coordinator's dispatch text (one relayed mid-task via a coordinator
system message after the initial dispatch). Existence of
`tools/edit-source.py`, the `--stamps-from` wiring in `main.rs`, and the
3 named tests in `stamp_pack.rs` were verified independently via
`Read`/`Grep` before citing them, per this role's own established
practice.
