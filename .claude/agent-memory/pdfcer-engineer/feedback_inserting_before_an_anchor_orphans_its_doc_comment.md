---
name: inserting-before-an-anchor-orphans-its-doc-comment
description: Splicing code in "before `fn foo(`" or "before `Variant {`" lands INSIDE the preceding doc comment — nothing compiles wrong, and only running the binary shows it
metadata:
  type: feedback
---

**A doc comment sits ABOVE the thing it documents, so anchoring an insertion on
`fn name(` or on an enum variant's opening line puts your block between the
comment and its owner.** The comment then attaches to whatever you inserted,
and the original item is left undocumented. **Nothing errors.** `rustc` is
happy, `cargo fmt` is happy, `clippy -D warnings` is happy, every test passes.

**Why:** it bit **twice in one session** (2026-08-20) in
`crates/pdfce-cli/src/main.rs`, and the first instance **shipped a visibly
wrong `--help`**: `clap` derives its subcommand description from the doc
comment, so `dimension-vertex` displayed `dimension-offset`'s text and
`dimension-offset` displayed *nothing*. It was caught by **running the binary**
— `pdfce-cli --help` — not by any gate, test, formatter or lint. With `clap` the
damage is operator-visible; elsewhere it is merely silent.

**How to apply:**
- Anchor an insertion on the **blank line or the closing brace BEFORE the
  target's doc comment**, not on the target's own first line. If the anchor
  must be the item, capture the doc block and re-emit it after your insertion.
- After any splice into a `clap` `Command` enum or a documented `fn`, run
  `<binary> --help` and read the two entries either side of the new one. That is
  a five-second check that no gate performs.
- When splicing with a script, print the three lines above the insertion point
  and look for `///` before writing.

The general shape: **a doc comment has no syntactic tie to its item, so
"insert before X" is not the same as "insert before X's documentation."**

Related: [[windows-paths-need-literal-edits]] (the other way patch tooling
silently changes what you wrote), [[engineer-does-the-observing]] (running it
is the check).
