---
name: feedback_change_scoped_release_gates
description: Ken 2026-10-16 — local pre-release testing is scoped to what changed since the last tag (crates touched + fast filing gates + packaging smoke); the full 46-command run-gates.sh is CI's job, not a local prerequisite
metadata:
  type: feedback
---

Before a release, test locally ONLY what changed since the previous tag
(`git diff --name-only <last-tag>..HEAD -- crates | cut -d/ -f2 | sort -u`
gives the crates): `cargo test -p <those crates>`, the fast filing/doc
gates (check-commits-filed, check-suite-name-absent, register-entry size,
core-api verbs), then the package build + fresh-folder smoke. The full
`tools/run-gates.sh` (46 commands, every feature variant, fuzz check) is
what CI runs on push; do not block the tag on a second local copy of it.
A version-only bump after a green full run needs no crate tests at all —
the package build is the compile check.

**Why:** Ken, 2026-10-16, while a 1h+ full gate run was repeating a suite
that had passed 46/46 one commit earlier (only the version strings and
lockfiles had changed): "can you minimize your tests to only what was
changed and would be affected between releases?" The first run had also
filled the disk (51 GB of incremental caches) and died with os error 112,
costing a second full run.

**How to apply:** at release time, and whenever I reach for run-gates.sh
"to be safe" on a tree whose only delta is docs/version. Still run the
full suite locally when a change touches shared plumbing (writer, xref,
filters, the CLI arg tree) or when CI is unavailable. Read CI's colour from
GitHub after the push — that is the full sweep. Related:
[[feedback_a_gate_sweep_certifies_the_tree_it_ran_on]] (a hand-typed sweep
omitted gates — the scoped set must still include the two filing gates),
[[feedback_a_starved_test_run_looks_exactly_like_a_broken_one]].
