"""Apply text substitutions to a source file without half-applying them.

WHY THIS EXISTS
===============

Large Rust files in this workspace (``edit.rs`` and ``pdfcer``'s
``main.rs`` are both five figures of lines) are edited by anchored
search-and-replace far more often than by hand. Two failure modes bit
repeatedly during one session on 2026-08-11 — **six** and **two** times
respectively — and both produce *plausible-looking wrongness*: nothing errors,
the resulting code reads as correct, and only a later lint or a missing test
reveals it.

Failure 1 — an anchor lands between an item and what belongs to it
------------------------------------------------------------------

An anchor matched on a declaration line cannot see that an attribute or doc
comment above it belongs to what follows. Inserting "before ``fn foo``" puts
the new text *between* ``#[test]`` and ``fn foo``.

Rust does not complain. Concretely, in one session:

* ``#[allow(clippy::too_many_arguments)]`` silently transferred from the
  function it guarded to a newly inserted one — the original then tripped the
  lint, on code nobody had touched.
* ``#[test]`` was orphaned from its function. **An un-attributed ``fn`` in a
  test module simply never runs.** Nothing fails; a test quietly stops
  existing.
* A ``#[error(...)]`` attribute was separated from its enum variant, which at
  least failed the build — the lucky case.

The mitigation is twofold: :meth:`Splicer.plan` **refuses an ambiguous
anchor** rather than silently taking the first match, and callers are expected
to anchor on a **closing brace** or on a multi-line block that already
includes the attributes.

Failure 2 — a script that dies mid-run leaves nothing applied
--------------------------------------------------------------

The natural shape — substitute, print ``ok``, substitute, print ``ok``, write
once at the end — means an assertion on the *second* anchor discards the
*first* substitution while its success line has already been printed. The run
looks half-successful and is entirely unapplied.

That is not hypothetical: it is how one change shipped implemented-but-
untested for two commits, because the run that was supposed to add its test
printed ``ok`` and wrote nothing.

The mitigation is that **every anchor is validated before any substitution
happens**, and the write is unconditional once validation passes.

WHAT IT DOES NOT DO
===================

It is not a patch format, a diff applier, or a refactoring tool. It has no
understanding of Rust. It is a guard rail around ``str.replace``, and the
judgement about *where* to anchor remains the caller's — the helper can only
refuse an anchor that is absent or ambiguous, not one that is present,
unique, and in the wrong place.

USAGE
=====

::

    import sys
    sys.path.insert(0, 'tools')
    from splice import Splicer

    sp = Splicer(r'crates/pdfcer-core/src/edit.rs')
    sp.plan(old_text, new_text, 'what this edit is')
    sp.plan(other_old, other_new, 'the second edit')
    sp.apply()          # validates everything, then writes, then reports

``plan`` raises ``AssertionError`` naming the edit if its anchor is missing or
matches more than once. ``apply`` re-checks each anchor against the
progressively-edited text, so an edit that destroys a later anchor is caught
rather than silently skipped.

ALL-OR-NOTHING IS PER FILE
==========================

One ``Splicer`` owns one file. A script touching several files should expect
that a failure on the third file leaves the first two written — the guarantee
is that no *single* file is left half-edited. Where that matters, plan the
files in dependency order so a partial run leaves the tree buildable.
"""

from __future__ import annotations


class Splicer:
    """Queued, validated, all-or-nothing substitutions against one file."""

    def __init__(self, path: str) -> None:
        self.path = path
        with open(path, encoding='utf-8') as f:
            self.text = f.read()
        self.edits: list[tuple[str, str, str]] = []

    def plan(self, old: str, new: str, note: str) -> 'Splicer':
        """Queue a substitution, validating its anchor immediately.

        Raises ``AssertionError`` if ``old`` is absent or appears more than
        once. Ambiguity is an error rather than a first-match default: an
        anchor that matches four places is not an anchor, and taking the first
        one silently edits whichever happened to come first in the file.

        Returns ``self`` so calls may be chained.
        """
        n = self.text.count(old)
        if n == 0:
            raise AssertionError(f'anchor NOT FOUND: {note}')
        if n > 1:
            raise AssertionError(f'anchor AMBIGUOUS ({n} hits): {note}')
        self.edits.append((old, new, note))
        return self

    def apply(self) -> None:
        """Apply every queued edit and write the file.

        Each anchor is re-checked against the progressively-edited text, so an
        earlier edit that destroyed or duplicated a later anchor fails here
        rather than silently skipping it. Nothing is written until every edit
        has succeeded, so the file is never left half-spliced.
        """
        text = self.text
        for old, new, note in self.edits:
            n = text.count(old)
            if n != 1:
                raise AssertionError(
                    f'anchor became {n}-way after earlier edits: {note}')
            text = text.replace(old, new, 1)
        with open(self.path, 'w', encoding='utf-8', newline='') as f:
            f.write(text)
        # Reported only after the write, so an "ok" line always means the
        # change is on disk — the whole point of failure 2 above.
        for _, _, note in self.edits:
            print('ok:', note)
        print(f'written: {len(self.edits)} edit(s) -> {self.path}')
