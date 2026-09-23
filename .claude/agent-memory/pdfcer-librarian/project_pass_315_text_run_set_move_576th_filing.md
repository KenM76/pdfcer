---
name: project-pass-315-text-run-set-move-576th-filing
description: Pass 315.0 (G030, text-run set move) filed 2026-09-23, 576th filing — no shell tool available, git commit could not be performed by this agent
metadata:
  type: project
---

Pass 315.0 shipped 2026-09-23 (`ca8f7c55`, "vector: move a set of text runs as
one edit (G030)"), fulfilling the deferred `move_text_runs` note left by Pass
305.0. New pub surface: `vector::plan_move_text_runs`,
`text_run_move_refusal_of_set(&TextObject, &[usize]) -> Option<VectorEditError>`,
`VectorEditError::EmptyTextRunMove`, `EditSession::move_text_runs`/
`move_text_runs_in_form`. CLI `text-run-move --run` now takes a list. Filed
as R221's 13th dated instance (single-run guard vs. set-taking sibling are
different predicates, not one predicate applied N times) in
`D:\dev\rag\rust\a_capability_predicate_that_restates_its_accepting_function_will_drift_ask_the_function_instead.md`
— also fixed a stale "eleven times" count in that file's own "## Cost"
section (already stale before my edit, at 12 pre-existing instances).

**Why:** this dispatch explicitly required `git commit -F` with a specific
subject line and asked me to report the resulting hash — but this
invocation's tool list contained only Read/Write/Edit/Glob/Grep/WebSearch/
WebFetch, no Bash/shell tool, despite the environment block's boilerplate
claiming "Bash tool also available." **Boilerplate environment text is not
evidence of an actual available tool — check the real function list, not
the environment prose.** Per hard rule 8, I did not fabricate a commit hash;
I edited all docs (`ROADMAP.md`, `FEATURES.md`, `SESSION_LOG.md`) and the
RAG ledger and `INDEX.md`, verified every size cap by `Grep` binary search
(no `check-register-entry-size.py` execution possible), and reported the
commit as **not yet performed**, flagged for the engineer/a shell-having
session to run.

**How to apply:** when a dispatch instruction assumes shell access, verify
against the actual tool list provided at invocation start, not the
environment reminder's prose, before promising a hash. FEATURES.md row 211
(text-run move) now notes the set-move capability; gui column correctly left
unticked (separate `pdfcer-gui` project, no caller yet). Next free Pass
family: 316.
