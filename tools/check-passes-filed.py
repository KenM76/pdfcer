#!/usr/bin/env python3
"""check-passes-filed.py — every commit that CLAIMS a Pass must be FILED.

WHY THIS EXISTS, AND WHY IT IS A SECOND SCRIPT
==============================================
`check-ledger-numbers.py` is deliberately a **pure Markdown parse of
`docs/`**. It reads no git, no working tree, no remotes — a property its own
docs state and the librarian relies on, because the librarian has no shell and
must be able to reason about what that checker can and cannot see.

That purity is also its blind spot. On 2026-08-06 the librarian found:

  * **five commits from 2026-08-05 with no filing at all** (`7f850c9`,
    `f8bbdd4`, `e666d3f`, `2c306ec`, `c3d605b`); and
  * **two commits both claiming Pass 26.2** (`f8bbdd4` and `50ab8ec`) —

while `check-ledger-numbers.py` reported `clean`. It was not wrong. The
duplicate simply never reached the document it parses, so it was outside the
reach of every check this project owned.

That is the same shape as the Acrobat-column finding earlier the same day: a
deliverable recorded only on the producing side has not been handed off, it
has been filed. Here the producing side is `git log` and the consuming side is
`ROADMAP.md`, and nothing was comparing them.

So: a SEPARATE script, so `check-ledger-numbers.py` keeps its no-git purity
and its guarantee stays exactly as documented.

WHAT IT CHECKS
==============
For every commit whose SUBJECT LINE claims a Pass ID (`Pass 26.2:`,
`Pass 37.0 —`, …), assert that `docs/ROADMAP.md` mentions that commit's short
hash somewhere. Filing convention is to cite the hash in the entry, so the
hash is the join key — and it is a far stronger one than the Pass ID, which is
exactly what went wrong with 26.2 (two commits, one ID, one of them filed).

EXIT CODES
==========
0  every Pass-claiming commit is filed (repeated IDs are reported as
   notes, not failures — see the comment on `collisions`)
1  at least one is not (they are listed)

KNOWN WEAKNESS, stated rather than discovered later: the join is "this
commit's short hash appears somewhere in `ROADMAP.md`". A hash cited in a
*gap record* — a table listing commits that still need filing — therefore
counts as filed. That is the honest limit of a hash grep; closing it would
need the checker to understand entry structure, which is a much larger
script for a case a human reading the gap record already sees.

Deliberately NOT checked, and named so the gap is honest rather than implied:
commits that change behaviour without claiming a Pass in the subject (a `fix:`
or `harden:` line). Those are filed too by convention, but a subject-line
grep cannot tell a behaviour change from a docs-only commit, and a gate that
guesses would cry wolf until it was ignored. This catches the class that
carries an explicit, collidable NUMBER.

USAGE
=====
    python tools/check-passes-filed.py [--since <rev>] [--stats]

`--since` defaults to the WHOLE HISTORY (0.5 s over 323 commits). It used to be
the last 60 commits, which missed a Pass-claiming commit five days old — see the
project's cadence; widen it for an audit.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

# Windows consoles default to a code page that cannot encode the em-dashes,
# arrows and stars this file prints, so Python substitutes "?" for exactly
# the characters that make a failure message readable. One reconfigure fixes
# every message in the file without flattening the typography.
#
# This is not theoretical: `check-commits-filed.py` was observed printing
# "each commit's full message ? they carry" while doing its job correctly.
# Found by reading a gate's output as its audience (R174), not by reading
# its source.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")


ROADMAP = Path(__file__).resolve().parent.parent / "docs" / "ROADMAP.md"
# ★ Plus the archive (2026-09-10): the Shipped history moved to
# `docs/history/` when `ROADMAP.md` reached 168,036 lines. A Pass filed before
# the split is still filed, and this gate has to be able to see it.
HISTORY = sorted((Path(__file__).resolve().parent.parent / "docs" / "history").glob("*.md"))

# `Pass 26.2`, `Pass 9c`, `Pass 12.M2` — the same shape the sibling checker
# accepts, kept in step deliberately so the two cannot disagree about what a
# Pass ID looks like.
PASS_ID = r"\d+(?:\.\d+)?(?:\.M\d+)?[a-z]?"
SUBJECT_CLAIM = re.compile(rf"^(?:★\s*)?Pass ({PASS_ID})\b")


def git(*args: str) -> str:
    """Run git and return stdout, or exit with a clear message on failure."""
    try:
        return subprocess.run(
            ["git", *args],
            cwd=ROADMAP.parent.parent,
            capture_output=True,
            text=True, encoding="utf-8", errors="replace",
            check=True,
        ).stdout
    except (subprocess.CalledProcessError, FileNotFoundError) as exc:
        sys.exit(f"check-passes-filed: cannot run git: {exc}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--since", default=None, help="a git rev to scan from")
    ap.add_argument(
        "--strict-tip",
        action="store_true",
        help="also require the tip commit itself to be filed "
        "(see the tip-deferral comment in main)",
    )
    ap.add_argument("--stats", action="store_true")
    args = ap.parse_args()

    # ★ REFUSE TO RUN ON A SHALLOW CLONE — the same defect that made
    # `check-commits-filed.py` lie in CI on 2026-08-11, fixed here BEFORE it
    # could (that gate is in CI; this one is not, yet).
    #
    # This walks commits and reads their messages. On a shallow clone the
    # history is truncated to the fetch depth, so the walk silently covers a
    # fraction of what its own output claims — and "0 unfiled" from a
    # one-commit history looks exactly like "0 unfiled" from a complete one.
    #
    # `.strip()` is load-bearing and is the reason this comment is long:
    # `git()` above returns RAW stdout, so the naive `== "true"` compares
    # against `'true\n'` and never fires. That exact mistake shipped in the
    # sibling gate's first version of this guard — a guard against quiet
    # failure that failed quietly (R187).
    if git("rev-parse", "--is-shallow-repository").strip() == "true":
        print(
            "ERROR: this is a SHALLOW clone, so the commit walk is "
            "incomplete and its result is meaningless.",
            file=sys.stderr,
        )
        print(
            "       Fix the checkout (`fetch-depth: 0`); do not interpret "
            "any result below.",
            file=sys.stderr,
        )
        return 2

    # EXHAUSTIVE BY DEFAULT since 2026-08-09, not a rolling window.
    #
    # This was `-60` — the last sixty commits — on the reasoning, stated in
    # the usage text, that sixty "spans several days of this project's
    # history". That assumption stopped being true: this project does sixty
    # commits in about two days, and `ae59ce3` ("Pass 24.0 (part): tool
    # panels anchor to the viewport") sat five days back, claiming a Pass ID,
    # with its hash in no filing — and this gate reported CLEAN throughout,
    # because the commit was outside the window it happened to be looking at.
    #
    # A gate whose coverage is a function of how fast the project is moving
    # reports "clean" most reliably when there is most to find. The whole
    # history is 323 commits and scanning it takes 0.5 s, so the window was
    # buying nothing and costing exactly the case it exists for.
    rng = f"{args.since}..HEAD" if args.since else "--all"
    # `--abbrev=7`: the record cites SEVEN-character hashes, and git's default
    # `%h` length GROWS with the object count — a fresh CI clone crossed the
    # threshold to 8 characters on 2026-09-06 and every citation stopped
    # matching at once (20 "UNFILED", all of them filed). Pin the join key.
    raw = git("log", rng, "--abbrev=7", "--format=%h\x1f%s")

    roadmap = ROADMAP.read_text(encoding="utf-8", errors="replace")
    for extra in HISTORY:
        roadmap += "\n" + extra.read_text(encoding="utf-8", errors="replace")

    claimed: list[tuple[str, str, str]] = []
    for line in raw.splitlines():
        if "\x1f" not in line:
            continue
        short, subject = line.split("\x1f", 1)
        m = SUBJECT_CLAIM.match(subject.strip())
        if m:
            claimed.append((short, m.group(1), subject.strip()))

    # A DOCS-ONLY COMMIT CANNOT SATISFY THIS JOIN, so it is not asked to.
    #
    # The join key is "this commit's short hash appears in ROADMAP.md". A
    # commit that WRITES ROADMAP.md would have to contain its own hash to
    # pass — which is impossible, because the hash is not known until the
    # commit exists. So the filing commit for a Pass is *structurally*
    # unsatisfiable by this gate, and can only go green on the NEXT filing,
    # if one ever comes.
    #
    # Found 2026-08-07 by running the gate, not by reading it: `e7e74f2`
    # filed Pass 44.0 and was flagged UNFILED by the very check that its
    # own content satisfies for the code commits. It surfaced only because
    # that commit was subjected `Pass 44.0: …` rather than the usual
    # `docs: …` — so the defect had been latent behind a naming habit, and
    # a habit is not a guard.
    #
    # The exemption is on the DIFF, not the subject line, deliberately: a
    # commit that touches only `docs/` cannot be the code half of a Pass by
    # definition, whatever its subject says. Keying on the subject would
    # re-introduce the same "it happened to be worded safely" fragility
    # this replaces.
    def is_docs_only(short: str) -> bool:
        files = git("show", "--pretty=", "--name-only", short).split()
        return bool(files) and all(f.startswith("docs/") for f in files)

    # ★ THE TIP IS DEFERRED, for the reason the block above already gives.
    #
    # Added 2026-08-24 as a CLASS fix, not an incident fix. The comment
    # above exempts docs-only commits because a commit that WRITES
    # ROADMAP.md cannot contain its own hash. That argument is not about
    # docs at all -- it is about a commit being unable to cite a hash that
    # does not exist until it exists -- and it applies verbatim to a CODE
    # commit subjected `Pass N.N: ...` at the moment of its own CI run.
    #
    # This hole had never fired, and the reason is luck rather than design:
    # 120 commits in this history claim a Pass ID, but pushes usually batch
    # several commits so the tip is usually the librarian's filing, which is
    # docs-only and already exempt. `ff4b4bf` (`Pass 97.1f: a knockout group
    # in ink`) is a code commit claiming a Pass ID; had it been pushed alone
    # it would have failed this gate for a condition it could not satisfy.
    #
    # Fixed here at the same time as `check-commits-filed.py`'s identical
    # hole rather than after this one bites, because the two gates are
    # siblings by design and a fix applied to only the instance that was
    # OBSERVED is how the same defect gets found twice. (`check-commits-
    # filed.py`'s own header records that exact shape one stream over: a
    # stdout reconfigure applied where the problem had been SEEN rather than
    # to every stream that could have it.)
    #
    # `--all` is the scan range, so the tip is taken from HEAD explicitly
    # rather than from the first line of the log, which under `--all` is the
    # newest commit on ANY ref and need not be the commit under test.
    tip = git("rev-parse", "--short=7", "HEAD").strip()
    pending = [c for c in claimed if c[0] not in roadmap and not is_docs_only(c[0])]
    deferred = [c for c in pending if c[0] == tip and not args.strict_tip]
    unfiled = [c for c in pending if c not in deferred]

    # Multiple commits per Pass is NORMAL and correct here — Pass 34.1
    # shipped in four slices, Pass 27.2 in two ticks — so a repeated ID is
    # INFORMATIONAL, never a failure. The gate learned this the honest way: its
    # first run flagged 34.1's four legitimate slice commits alongside the one
    # real collision, and a gate that cries wolf on correct work is a gate
    # everyone learns to ignore.
    #
    # What it cannot distinguish, and does not pretend to: "one Pass shipped in
    # four commits" from "two unrelated pieces of work under one number" (the
    # real 26.2 case). That needs a human reading the subjects. Listing them is
    # still worth it — a human glancing at the list spots the odd one out
    # immediately, which is exactly how 26.2 was caught.
    by_id: dict[str, list[str]] = {}
    for short, pid, _ in claimed:
        by_id.setdefault(pid, []).append(short)
    collisions = {p: v for p, v in by_id.items() if len(v) > 1}

    if args.stats:
        print(f"commits scanned          : {len(raw.splitlines())}")
        print(f"claiming a Pass ID       : {len(claimed)}")
        print(f"unfiled (hash not in doc): {len(unfiled)}")
        print(f"IDs claimed by >1 commit : {len(collisions)}")

    # Printed on BOTH paths -- see the twin note in check-commits-filed.py.
    # A deferral visible only when the gate already fails is invisible in
    # exactly the run whose reader concludes everything is filed.
    for short, pid, subject in deferred:
        print(f"DEFERRED {short}  Pass {pid}  {subject[:88]}")
        print(
            "         tip commit: its filing is a later commit, so it "
            "cannot cite itself."
        )
    for short, pid, subject in unfiled:
        print(f"UNFILED  {short}  Pass {pid}  {subject[:88]}")
    for pid, shorts in sorted(collisions.items()):
        print(f"note  Pass {pid} claimed by {len(shorts)} commits: {', '.join(shorts)}")

    if unfiled:
        print(
            "\ncheck-passes-filed: a commit that never reaches ROADMAP.md is "
            "outside\nthe reach of every other check this project owns — "
            "including the\nduplicate-number check, which parses the document "
            "and not the history."
        )
        return 1
    print("passes-filed: clean - every Pass-claiming commit is filed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
