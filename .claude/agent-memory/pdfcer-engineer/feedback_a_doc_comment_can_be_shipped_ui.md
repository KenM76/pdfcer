---
name: a-doc-comment-can-be-shipped-ui
description: Text that reaches an operator — a clap --help derived from a doc comment, an error message naming the verb to use instead — is UI that no compiler, linter or test checks; grep for the class, don't wait to spot instances
metadata:
  type: feedback
---

## ★★ The general rule, of which the clap case below is one instance

**Some text in this codebase is shipped operator-facing UI, and nothing checks
any of it.** Not the compiler, not clippy, not `missing_docs`, and no test —
because no test reads help strings or error strings. Two families found so far,
both on 2026-08-29, both live at HEAD:

| family | what it is | gate |
|---|---|---|
| a `///` on a clap `Command` variant | the subcommand's `--help` | `tools/check-clap-help.py` |
| `use_instead: "…"` in a refusal | *"use `X` instead"*, read at the moment the operator is blocked | `tools/check-cited-verbs-exist.py` |

**The second is worse than a dangling doc link**, and that is the part to
carry: rustdoc at least *warns* about a broken intra-doc link. **Nothing warns
about a `&'static str`.** It compiles, it is a literal, no test asserts on it,
and the operator is the first reader who finds out — while blocked, being told
the way out.

`rotate_widget` had **never existed** and had been cited that way for Passes.
The gate written to catch it found a **second** on its first run,
`set_dimension_label`, which I had not gone looking for.

⇒ **The fix is not "point at a real verb" — sometimes there isn't one.** It is
*name a verb that exists, or say plainly that it does not.* A named-but-unbuilt
capability is a search term; a bare "unsupported" is a dead end. The gate
accepts a `NOT BUILT YET` marker for exactly that reason.

★ **And a gate lesson from writing it:** the first version read a **fixed
six-line window** after the citation to find that marker, and produced a false
positive within the hour — a comment inserted between `use_instead:` and `why:`
pushed the marker past the window. **A fixed-size window over source is a guess
about what a human will write in the gap.** Read to the end of the enclosing
construct instead. Same shape as the doc-comment walk-back that terminated at
zero steps because an `#[allow(...)]` intervened.

**In a `clap`-derive CLI, a doc comment is not documentation — it is shipped
operator-facing UI.** `clap` turns the `///` on a `Command` variant into that
subcommand's `--help` description. A variant with no doc comment ships a
**blank** description, in the subcommand list and at the top of its own
`--help`.

**Why this needs its own rule:** *nothing* catches it. Not the compiler, not
`clippy`, not `missing_docs` (these are private items in a binary crate), and
**no test, because no test reads help text**. The build is green and the
operator sees an empty line.

**How to apply:** when a doc-comment defect class shows up anywhere, immediately
ask whether any of that project's doc comments are *rendered to a user*. If
they are, write the mechanical check rather than continuing to find instances by
reading. `pdfce` has `tools/check-clap-help.py`; the same shape applies to any
`clap`-derive tool, and to `#[derive(Parser)]` field docs (which become
per-flag help).

## The measurement that produced it

2026-08-29. `pdfce` had found **six** instances of doc-comment orphaning by eye
over several weeks — a splice anchored on `pub fn name(` lands *inside* the
preceding item's doc block, welding two together. The sixth was the first that
was operator-facing: `ExtractText`'s entire help sat 800 lines away on
`ListOutline`, so `list-outline --help` printed the *text-extraction*
description and `extract-text --help` printed nothing.

A gate written in twenty minutes found **two more within seconds**
(`print-preview`, `render-page`), both shipping blank, **neither caused by a
splice**. That is the load-bearing part: the class had **more than one cause**,
so the existing remedy — *"insert after a closing brace, never before a named
anchor"* — could never have closed it, and no amount of careful reading would
have either.

## ★ What did NOT work, so it is not re-derived

A structural detector for the **weld itself** — *"a doc line whose predecessor
is non-empty and whose successor is a blank `///`"* — produced **8,136
candidates** across the crate. That is also the shape of every ordinary
paragraph ending. Abandoned rather than shipped noisy.

⇒ The gate catches the **donor** of a weld (an item left with nothing), never
the **recipient** (an item left with two). Six of eight instances left a donor;
two did not. **Ship the half that is exactly checkable and state the limit in
the script's header** rather than shipping a fuzzy check for the whole class.

Related: [[feedback_inserting_before_an_anchor_orphans_its_doc_comment]],
[[feedback_a_gate_that_underreports_looks_green]],
[[feedback_an_unticked_box_is_unfalsifiable]].
