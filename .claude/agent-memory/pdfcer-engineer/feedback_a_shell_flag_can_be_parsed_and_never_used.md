---
name: a-shell-flag-can-be-parsed-and-never-used
description: Unit tests hit the core API directly, so a CLI flag that is parsed, validated and then never attached to the request passes every one of them — only driving the binary finds it
metadata:
  type: feedback
---

**A new shell flag can be declared, parsed, validated with its own error
message — and never attached to the request it configures.** Every unit test
passes, because they call the core API directly and never go through the
shell. The only detector is running the binary.

**Why:** 2026-08-27, `Pass 145.0`. `pdfce-cli edit-text --pin-span 37:2` was
declared in the `clap` variant, threaded into the args struct, parsed by
`parse_pin_span`, and checked by a guard that refused an empty `--find`
without it. Then `cmd_edit_text` built its `EditRequest` and **never set
`pinned_span`**. The refusal read *"empty find text"* — **the exact message
the feature existed to eliminate**. Eight core tests for the same affordance
were green, because they construct `EditRequest` themselves.

The sibling `format-text` path *did* attach it, so a reviewer comparing the
two subcommands would have caught it — but nothing compares them, and the
half-wired one looked complete at every step: variant ✓, args field ✓, parse ✓,
validate ✓, use ✗.

**The same run found a second one:** a shipped refusal string carrying fourteen
baked-in spaces that `check-string-gaps.sh` reported PASS on. Both defects were
in that session's own new code, both invisible to `cargo test`, and both took
one terminal command to see.

**How to apply:** when a Pass adds or changes anything an operator types or
reads, **run the binary on every new path before committing** — the success
path, each refusal, and a malformed input. Read the output as text, not as an
exit code. This is not a substitute for tests; it catches a different class,
and the class is "the wiring between the shell and the engine", which no
core-level test can reach by construction.

Corollary for the test itself: assert the flag's *effect*, not its
acceptance. `pin_span.rs` now gets a span out of `extract-text --json --spans`
and hands it to `format-text --pin-span` in **one** test — testing either half
alone would pass with the two ends disagreeing about what a span is.

Related: [[engineer-does-the-observing]] (verify operator-facing behaviour in
the running app yourself), [[a-gate-that-underreports-looks-green]] (the second
defect from the same run), [[only-an-out-of-crate-test-feels-a-consumers-constraints]]
(the same shape one layer up).
