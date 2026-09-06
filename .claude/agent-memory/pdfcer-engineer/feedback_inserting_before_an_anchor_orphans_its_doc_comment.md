---
name: inserting-before-an-anchor-orphans-its-doc-comment
description: Splicing code in "before `fn foo(`" or "before `Variant {`" lands INSIDE the preceding doc comment; recurred FIVE times on 2026-09-06 — the fix is structural (walk back over `///`/`#[` lines before inserting), and the public-fns gate now catches the fn case
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

**★ RECURRED FIVE TIMES IN ONE SESSION (2026-09-06)** — `looks_like_pdf_date`,
`std14_by_base_font`, `RealFaceAvailable` (an enum variant: its `#[error]`
attribute was split from its name → "only one #[error] allowed"), `is_none`
(a `#[must_use]` duplicated onto the wrong fn), `locate_hole` (twice). Three
were caught by `tools/check-public-fns-documented.py`, two by the compiler.
Reading this memory did not prevent it: the anchor is chosen at script-writing
time, when the item's line is the natural search key. The rule that held:

- **The patch script anchors on the item but INSERTS at the doc block's start**
  — after locating `fn name(`, walk back while the previous line starts with
  `///`, `#[`, or (for a `thiserror` variant) the multi-line `#[error(` body,
  and insert there. `move_block()` in the 2026-09-06 scripts is the shape.
- **Prefer inserting AFTER a complete item** (after its closing `}` + blank
  line) over inserting before the next one — nothing above a closing brace is
  a doc comment.
- Run `python tools/check-public-fns-documented.py` right after any splice
  into `crates/`; it names the orphaned fn in one line.
