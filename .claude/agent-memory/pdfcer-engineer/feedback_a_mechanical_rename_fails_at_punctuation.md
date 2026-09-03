---
name: a-mechanical-rename-fails-at-punctuation
description: a regex rename sweep (pdfce→pdfcer, 11,960 hits) missed exactly three shapes, all glued to punctuation an escape or a quote supplies; gates and tests found them, reading did not
metadata:
  type: feedback
---

**After a mechanical rename, grep for the word glued to `\n`, `\t`, a quote
or a path separator — `\b` does not see a boundary there — and run every
gate before believing the sweep.**

**Why:** the 2026-09-03 rename (Pass 247.1) scripted 11,960 substitutions and
was green on build; the survivors were (1) `"  2\nPDFCE_TEXT\n"` and
`"\tpdfce\tdelta"` — a source escape puts a word char before the name;
(2) `"crates" / "pdfce-cli" / "src"` — a quoted path component that the
"`pdfce-cli` means the TOOL → `pdfcer`" rule rewrote to the binary name, so
four gates pointed at a directory that did not exist; (3) test expectations
that must equal FIXTURE BYTES (certificate subjects, `/Reason`) — renamed
along with the code, and a fixture generator that would have regenerated a
different fixture. Every one was found by a test or a gate, none by reading
the diff.

**How to apply:** protect names that must keep the old spelling BEFORE the
sweep (removed crates, fixture content, archived URLs, backup paths); after
it, grep the `[\\"'/]pdfce` forms explicitly; and treat "equals fixture
bytes" strings as content, not code — restore them and tell the generator
why. (This file itself was first written through a Bash heredoc and lost
every backslash — the Write tool is the only safe route for text with `\`.)
