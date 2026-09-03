---
name: splice-end-marker-must-be-searched-from-start
description: A Python `s.index(END)` splice searched from position 0 silently DUPLICATED 141 lines of a consumer-facing doc; every gate stayed green and only a reading agent caught it
metadata:
  type: feedback
---

When patching a file by slicing (`s[:start] + new + s[end:]`), **the end
marker must be searched from `start`, never from zero**: `s.index(marker, start)`.

**Why:** on 2026-08-20 (`cc57080`) I rewrote a section of
`docs/core-api/02-editing-and-saving.md` with
`end = s.index("| I want to… | Call | Line | Returns |")`. That table header
appears in nearly every section of the file, so `end` landed ~200 lines
*before* `start` and the splice re-appended everything from §1.3 onward.
**Sections 1.4–1.9 shipped twice**, and the second copy still carried the
stale warning the commit existed to reverse — telling the consuming shell to
refuse a caret on exactly the text the Pass had just made editable.

**★ The property that makes it dangerous: it DUPLICATES rather than
corrupts.** Nothing errors, nothing is lost, nothing is reordered. Every
check was green — `check-core-api-verbs.py` counts verbs and line-count
claims, and a duplicated section has the same verbs; `cargo test` was green;
the Markdown rendered fine; `+205 lines` looked plausible for a rewrite
meant to add a hundred. **It was found by `pdfce-librarian` reading the
document while filing the Pass**, not by any gate.

**How to apply:**
- Slicing edits: pass the offset — `s.index(marker, start)`. Better still,
  use the `Edit` tool, which fails loudly on a non-unique `old_string`
  instead of guessing.
- After any bulk doc patch, **verify structure, not just content**:
  `grep -c` the section headings, or diff the heading list against `HEAD~1`.
- Treat the post-commit librarian dispatch as a **read**, not a
  transcription. Its independent read of the file is what caught this; a
  dispatch that only restates what I said would not have.

Related: [[feedback_windows_paths_need_literal_edits]] (the other way a
scripted patch silently ships wrong bytes),
[[feedback_a_gate_that_underreports_looks_green]] (same shape from the gate
side — green because it was not looking).
