#!/usr/bin/env python3
"""Every verb an error message tells the operator to use must actually exist.

WHY THIS GATE EXISTS
====================

`EditError::AnnotationMoveWrongVerb` and its neighbours refuse an operation by
pointing at the verb that *does* handle it:

    "this annotation is a form widget; use `rotate_widget` instead — ..."

On 2026-08-29 three of those four names resolved to a real `pub fn` and one --
`rotate_widget` -- **had never been written**. The message was read at the exact
moment an operator is blocked, and it named the way out. There was no way out.

This is the same class as a dangling intra-doc link (one of those was also live
at HEAD the same day, `EditSession::move_outline_item`, cited by a doc comment
since `Pass 156.0`) but strictly worse in one way: rustdoc at least *warns*
about a broken doc link. **Nothing at all warns about a string.** It compiles,
it is a `&'static str`, no test asserts on it, and the operator is the first
reader who finds out.

WHAT THIS CHECKS
================

Every `use_instead: "..."` literal in `pdfcer-core` names something that exists
as a `pub fn` / `pub(crate) fn` in the crate.

A value may carry a call-shaped hint rather than a bare name --
`edit_widget(fqn, index, &WidgetEdit::new().with_rect(..))` -- so only the
leading identifier is checked. That is the part the operator greps for.

WHAT IT DELIBERATELY DOES NOT CHECK
===================================

Prose. A `why:` string may name a verb in passing, and a message may describe a
capability that is genuinely unbuilt as long as it SAYS so. The
`rotate_widget` message is now exactly that shape -- it still names the verb,
because "unsupported" with no name is a dead end while "unsupported, and here
is what it will be called" is a search term -- and it is allowed by the
`NOT BUILT YET` marker below.

So the rule is: **name a verb that exists, or say plainly that it does not.**

EXIT CODES
==========

0 -- every cited verb resolves, or is explicitly marked as unbuilt.
1 -- one or more cite a verb that does not exist and do not say so.
2 -- the source could not be read, or the citation pattern found nothing,
     which is reported as a failure rather than a pass: a gate that silently
     finds nothing to check is worse than no gate.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CORE = ROOT / "crates" / "pdfcer-core" / "src"

CITATION = re.compile(r'use_instead:\s*"([^"]+)"')
DEFINITION = re.compile(r"^\s*pub(?:\(crate\))?\s+(?:const\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)")
# The escape hatch: the surrounding message states the verb is not built.
UNBUILT = re.compile(r"NOT BUILT YET|NOT YET BUILT", re.IGNORECASE)


def main() -> int:
    if not CORE.is_dir():
        print(f"check-cited-verbs-exist: cannot read {CORE}", file=sys.stderr)
        return 2

    sources = sorted(CORE.rglob("*.rs"))
    defined: set[str] = set()
    for f in sources:
        for line in f.read_text(encoding="utf-8").splitlines():
            m = DEFINITION.match(line)
            if m:
                defined.add(m.group(1))

    citations: list[tuple[Path, int, str, str]] = []
    for f in sources:
        lines = f.read_text(encoding="utf-8").splitlines()
        for n, line in enumerate(lines, 1):
            m = CITATION.search(line)
            if not m:
                continue
            # The `why:` that accompanies it is elsewhere in the same struct
            # literal, and the unbuilt marker usually lives there rather than
            # on this line. Read to the END OF THE LITERAL, not a fixed number
            # of lines.
            #
            # ★ A fixed window was the first implementation and it was wrong
            # within the hour: adding an explanatory comment between
            # `use_instead:` and `why:` pushed the marker past a six-line
            # lookahead and the gate reported a false positive on a message it
            # had just been made to accept. Same shape as the doc-comment
            # walk-back that terminated at zero steps because an attribute
            # intervened — **a fixed-size window over source is a guess about
            # what a human will write in the gap.**
            context_lines = []
            for probe in lines[n - 1 :]:
                context_lines.append(probe)
                if probe.strip().startswith("});"):
                    break
                # Don't run away if the literal is malformed or the pattern
                # changes: a citation is inside one `return Err(...)`, and 40
                # lines is far past any real one.
                if len(context_lines) > 40:
                    break
            citations.append((f, n, m.group(1), "\n".join(context_lines)))

    if not citations:
        print(
            "check-cited-verbs-exist: found no `use_instead:` citations at all "
            "-- the pattern changed and this gate is now blind.",
            file=sys.stderr,
        )
        return 2

    bad: list[tuple[Path, int, str]] = []
    for f, n, cited, context in citations:
        # Only the leading identifier: a value may be call-shaped.
        name = re.match(r"[A-Za-z_][A-Za-z0-9_]*", cited)
        if not name:
            continue
        if name.group(0) in defined:
            continue
        if UNBUILT.search(context):
            continue
        bad.append((f, n, cited))

    if bad:
        print(
            f"check-cited-verbs-exist: {len(bad)} of {len(citations)} refusal "
            "message(s) name a verb that DOES NOT EXIST:",
            file=sys.stderr,
        )
        for f, n, cited in bad:
            print(f"  {f.relative_to(ROOT)}:{n}  -> `{cited}`", file=sys.stderr)
        print(
            "\nThese strings are read by an operator at the moment an operation was\n"
            "refused, and they name the way out. Nothing else catches them: they\n"
            "compile, they are &'static str, and no test asserts on them.\n"
            "\n"
            "Either build the verb, point at one that exists, or state plainly in\n"
            "the same message that it is NOT BUILT YET -- which this gate accepts,\n"
            "because a named-but-unbuilt capability is a search term while a bare\n"
            '"unsupported" is a dead end.',
            file=sys.stderr,
        )
        return 1

    print(
        f"check-cited-verbs-exist: PASS — all {len(citations)} cited verb(s) "
        "resolve or are marked unbuilt."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
