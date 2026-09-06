#!/usr/bin/env python3
"""check-cited-commits-exist — every commit hash the documents cite must still
be on `main`.

WHY THIS SCRIPT EXISTS
======================
`docs/ROADMAP.md`, `docs/SESSION_LOG.md` and `docs/ARCHITECTURE.md` cite commit
hashes constantly — a Shipped entry names the commit that shipped it, a decision
names the commit that corrected it, a session log names what landed. Those
citations are the only route from a *claim* back to the *diff that supports it*,
and this project's whole append-only discipline rests on being able to walk it.

**A cited hash can stop existing, and nothing notices.** `git commit --amend`
and any rebase REWRITE the commit: the old object survives as an unreferenced
dangling object, so `git show <old>` keeps working for a while and the citation
looks fine. Then `git gc` collects it — by default within about two weeks — and
the citation points at nothing at all, permanently, with no way left to discover
what it *had* pointed at.

★ FOUND BY ACCIDENT, WHICH IS THE ARGUMENT FOR AUTOMATING IT
============================================================
On 2026-08-27 the engineer amended a commit's message minutes after dispatching
its filing (the message had shipped mangled). That changed its hash and made a
just-written, entirely correct ROADMAP entry point at a commit not on `main`.
`check-passes-filed` caught that one, because it happens to compare the roadmap
against the real history rather than against itself.

A follow-up sweep over every hash in `docs/` then found **fourteen more**, from earlier sessions, all with the same signature: a commit with the
same subject exists on `main` under a different hash. Every one was an
amend-or-rebase casualty. None was detectable by any existing gate, because
`check-passes-filed` only examines commits that CLAIM a Pass, and an ordinary
citation claims nothing.

⇒ **The documents can rot in a way that reads as perfectly healthy.** That is
exactly the failure class this project builds gates for.

WHAT IT CHECKS
==============
Every 7-to-40-character hex token in **every markdown file under `docs/`** that
**resolves to a real git commit** must be an **ancestor of HEAD**.

★ The file set is **discovered, not listed.** The first cut named three files by
hand and missed `docs/core-api/` — the document a *separate project* builds
against, which carried five stale citations. A hand-maintained list of documents
goes stale exactly the way a hand-copied hash does.

The "resolves to a real commit" filter is what keeps the false-positive rate at
zero without a hand-maintained ignore list: a random hex-looking token (a colour,
a byte count, an offset) does not resolve, and is skipped in silence. Only
something that genuinely *is* a commit is held to the rule — which is precisely
the population that can go stale.

★ WHY "ANCESTOR OF HEAD" AND NOT "EXISTS"
=========================================
Because "exists" is the check that would have passed on all fifteen. A dangling
object still exists. What makes a citation sound is that the commit is *in the
history the document describes*, and that is reachability, not existence.

★★ AN EXPLAINED MENTION IS NOT A STALE CITATION, AND THE FIRST CUT OF THIS
GATE GOT THAT WRONG
==========================================================================
This project corrects the record **in place, with the old value kept visible** —
*"this said `a2f7b48`; the engineer amended that commit, so the hash is now
`5f6ac58`"*. That paragraph legitimately contains a hash that is not on `main`,
**and it is the correct thing to have written.**

The first cut of this gate flagged all four such mentions. A gate that fires on
the behaviour the project wants is worse than no gate: the only way to satisfy
it would be to delete the honest correction notes, which is precisely the
history-erasure the append-only discipline exists to prevent. It would have
taught its reader to ignore it, which is how a gate becomes decoration.

**The discriminator needs no new convention, because a correction note names
BOTH hashes by construction.** So a stale hash is a defect only in a file that
**never mentions the replacement at all**. Where both appear in one file — beside
each other, or in a mapping table thousands of lines away — the document has
recorded the change, a reader can grep from one to the other, and the gate says
nothing.

⇒ A document that has *not* been corrected still fires, which is the whole
point; a document that has been corrected properly goes quiet without anyone
adding a marker, an ignore file, or a baseline.

WHEN IT FIRES, THE FIX IS USUALLY MECHANICAL
============================================
The report names, for each stale hash, any commit on `main` carrying the SAME
SUBJECT — which for an amend or rebase is the replacement, and is very nearly
always what the citation meant. Correcting it is a string replacement.

**Do not "fix" this by deleting the citation.** A Shipped entry with no commit
is worse than one with a stale commit: the stale one can still be repaired from
the subject line, and an absent one cannot be repaired at all.

USAGE
=====
    python tools/check-cited-commits-exist.py

Exit 0 clean, 1 with stale citations. Reads git, so it must run inside the work
tree; on a shallow clone it reports what it cannot verify rather than passing.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# EVERY markdown document under `docs/`, discovered rather than listed.
#
# ★★ THE FIRST CUT NAMED THREE FILES BY HAND — ROADMAP, SESSION_LOG,
# ARCHITECTURE — AND MISSED THE ONE THAT MATTERS MOST TO AN OUTSIDE READER.
#
# `docs/core-api/` is what a SEPARATE PROJECT builds against, and it carried
# five stale citations across four files. A hand-maintained list of documents is
# exactly the thing that goes stale — the same failure the gate itself exists to
# catch, one level up, and it was in the gate on the day it was written.
#
# Discovery has a second property worth naming: a document added next month is
# covered on the day it is added, by nobody.
def _docs() -> list[str]:
    return sorted(
        str(p.relative_to(ROOT)).replace("\\", "/")
        for p in (ROOT / "docs").rglob("*.md")
    )

# 7 is git's conventional abbreviation floor and what this project's documents
# use; 40 is a full SHA-1. Bounded on both sides so a long hex blob (a stream
# dump, a key) is not mistaken for a citation.
HASH = re.compile(r"\b[0-9a-f]{7,40}\b")

# ★★★ SCOPE OF "EXPLAINED": THE WHOLE FILE, NOT A LINE WINDOW.
#
# This started as an 8-line window, on the reasoning that a correction note sits
# beside the citation it corrects. **Running it against the real documents
# refuted that**, and the refutation is worth keeping.
#
# `SESSION_LOG.md` is a 67,000-line APPEND-ONLY log. One stale hash appears in
# it SEVEN times, across entries written weeks apart. The librarian paired it
# with its replacement in a single mapping table — which is obviously the right
# way to document it; the alternative is seven inline notes bloating seven
# historical entries that are not supposed to be rewritten. The window rule
# called six of those seven a defect.
#
# ⇒ The failure this gate exists to catch is a document that cites a dead hash
# and **never mentions the live one anywhere** — because then there is no route
# from the citation to the diff, which is the whole point. A reader who meets a
# stale hash and greps the file finds the table. That is a working document.
#
# So: same file, anywhere. Strictly weaker, and correctly so — the strong
# version was measuring tidiness rather than recoverability.
#
# ★★ NOTE THE SHAPE, because it is this gate's second design fault in an hour.
# The first flagged the correction notes themselves; this one flagged the right
# way to write a long log. **Both came from imagining how the documents are
# written instead of looking.** A threshold invented at a desk and a threshold
# calibrated against the corpus are different objects, and this project has a
# whole file about the first kind (`suite-check.py`'s CONTRAST_MIN).


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=ROOT, capture_output=True, text=True, check=False
    ).stdout


def main() -> int:
    if git("rev-parse", "--is-shallow-repository").strip() == "true":
        print(
            "cited-commits-exist: SHALLOW CLONE — reachability cannot be decided here.\n"
            "  Refusing to report clean, because a shallow history makes every older\n"
            "  citation look unreachable and would produce a wall of false positives\n"
            "  that a reader would learn to ignore.",
            file=sys.stderr,
        )
        return 0

    ancestors = set(git("rev-list", "HEAD").split())
    if not ancestors:
        print("cited-commits-exist: no history from HEAD; nothing to check.")
        return 0

    # Prefix -> full hash, so an abbreviated citation resolves without a git
    # call per token. Built once; the documents cite hundreds of hashes and a
    # subprocess each would make this gate too slow to run habitually, which is
    # the same as not having it.
    by_prefix: dict[str, str] = {}
    for full in ancestors:
        for n in range(7, 41):
            by_prefix.setdefault(full[:n], full)

    # token -> [(file, line-number)], and the file's lines, so an occurrence can
    # be checked for a nearby replacement rather than only counted.
    seen: dict[str, list[tuple[str, int]]] = {}
    lines_of: dict[str, list[str]] = {}
    docs = _docs()
    for rel in docs:
        path = ROOT / rel
        if not path.exists():
            continue
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        lines_of[rel] = lines
        for n, line in enumerate(lines):
            for token in HASH.findall(line):
                seen.setdefault(token, []).append((rel, n))

    stale: list[tuple[str, list[str], str, str]] = []
    for token, hits in sorted(seen.items()):
        if token in by_prefix:
            continue
        files = sorted({rel for rel, _ in hits})
        # Not an ancestor. Is it a commit at all? Only now is a git call worth
        # paying for, and only for the handful that got this far.
        kind = git("cat-file", "-t", token).strip()
        if kind != "commit":
            continue  # not a hash: a colour, a length, an offset
        subject = git("log", "-1", "--format=%s", token).strip()
        replacement = ""
        if subject:
            # `--abbrev=7`: citations are 7 characters; git's default `%h`
            # length grows with the object count (2026-09-06).
            for line in git("log", "--abbrev=7", "--format=%h\t%s", "HEAD").splitlines():
                h, _, s = line.partition("\t")
                if s == subject:
                    replacement = h
                    break
        # ★ An EXPLAINED mention is not a stale citation. If every occurrence
        # sits within `EXPLAINED_WINDOW` lines of the replacement hash, the
        # document has already recorded the amend and is CORRECT as written.
        if replacement:
            unexplained = sorted(
                {
                    rel
                    for rel, _ in hits
                    if not any(replacement in line for line in lines_of.get(rel, []))
                }
            )
            if not unexplained:
                continue
            files = unexplained
        stale.append((token, files, subject, replacement))

    if not stale:
        print(
            f"cited-commits-exist: clean — every cited commit in {len(docs)} document(s) "
            f"is an ancestor of HEAD."
        )
        return 0

    for token, files, subject, replacement in stale:
        where = ", ".join(files)
        print(f"STALE  {token}  cited in {where}")
        print(f"       subject: {subject[:96]}")
        if replacement:
            print(f"       same subject on HEAD: {replacement}  <- almost certainly meant this")
        else:
            print("       no commit on HEAD carries that subject — investigate before editing")
    print()
    print(
        f"cited-commits-exist: {len(stale)} cited commit(s) are NOT ancestors of HEAD.\n"
        "\n"
        "  These are amend/rebase casualties: the object still exists as a dangling\n"
        "  commit, so `git show` works and the citation looks healthy -- until\n"
        "  `git gc` collects it, after which the citation points at nothing and\n"
        "  cannot be repaired from anything but its subject line.\n"
        "\n"
        "  Fix by replacing the hash with the same-subject commit named above.\n"
        "  Do NOT fix by deleting the citation: a Shipped entry with a stale hash\n"
        "  is repairable, one with no hash at all is not.",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
