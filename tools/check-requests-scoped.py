#!/usr/bin/env python3
"""Gate: an inbound request that has already been ANSWERED must not be
re-dispatched as new work, and one that was SCOPED must not go unanswered.

WHY THIS GATE EXISTS
====================
Standing rule **`R242`**: *a request does not leave `open/` when it is scoped;
it leaves when it is answered.*

That rule is right, and it has a cost the rule itself names. Because a scoped
request correctly stays in the channel's `open/`, **an audit that reads
`open/` counts it as outstanding** — and twice in one week a session did
exactly that:

  * 460th filing — `request_a_reply_can_be_read_and_never_written.md` was
    already a `ROADMAP.md` *Backlog* entry. The session that shipped it did
    not find the entry, dispatched the work as new, and asked for a fresh
    Pass ID. One feature, two IDs, caught only in the filing.
  * 461st filing — the same shape on `Pass 252.0`, caught this time, and the
    commit message records what caught it: *"found by grepping ROADMAP for
    the request's filename before writing the commit message."*

The interval between those was 33 h 56 m to 34 h 46 m. The query that would
have prevented both costs milliseconds. `R242` was minted at 19:58 on
2026-09-06; the fix landed 28 minutes later, by hand, and this tool — owed
from that day — is the mechanism that stops it depending on somebody
remembering.

    ★ `R243`: A DOCUMENTED OBLIGATION ON A FUTURE CALLER IS NOT A CONTROL.
    `R242` in the register asks a future session to check. This file is what
    turns that request into a control.

WHAT IT CHECKS
==============
For every `request_*.md` in the channel's `open/`, three facts, each decidable
from bytes:

  1. **Answered?** — does any `reply_*.md`, `done_*.md` or `notice_*.md` in
     the channel cite this request's exact FILENAME in its body? The channel's
     own convention is that a reply names the request it answers (every one of
     the six open requests satisfies it today), so this is an exact match, not
     a similarity score. A gate that guessed would be worse than none.
  2. **Scoped?** — does `docs/ROADMAP.md` cite the filename?
  3. **Neither** — genuinely new, nobody has touched it.

EXIT CODES
==========
    0  every scoped request has an answer (and the report lists what is
       already answered, which is the half a dispatching session reads)
    1  a request is SCOPED in `ROADMAP.md` but has NO reply in the channel

Only (1) is red, and the reasoning matters. A request with no reply and no
roadmap entry is not an error — it is new work, which is what the channel is
for. A request that is answered is not an error — `R242` says it correctly
stays until the far side archives it. **The one state that is ours and wrong
is having planned something and never said so**, which is the exact half of
`R242` the rule's own sentence ends on.

The informational half is not decoration: it is what a session about to
dispatch reads to discover that the thing in front of it is already done.

WHAT IT DELIBERATELY DOES NOT DO
================================
* **It does not archive anything.** Moving a file out of `open/` is the
  requesting project's step; they close their own exchanges and write their
  own index rows. A gate that tidied their directory would duplicate work in
  flight.
* **It does not match on topic keys, subject words or similarity.** Only one
  of the six current requests carries a `G0NN` key in its filename, so a
  key-based match would silently cover one request in six while looking
  thorough. Filename citation covers all six.
* **It does not read `archive/`** for the answered check. A request in `open/`
  whose only answer is archived is a state worth seeing, not hiding.

THE CHANNEL IS OUTSIDE THE REPOSITORY
=====================================
`D:/Dev/FeatureRequests/pdfce_FeatureRequests/` is not in the tree and is not
present in CI. This gate therefore CANNOT run there, and says so **loudly**,
by name, on stdout.

    ★★ That is `R255`'s shape and it is handled deliberately. A check that
    returns early on a missing input and prints nothing reports to the runner
    exactly like one that ran and passed. This one prints
    `SKIPPED — channel not present` and names the path it looked for, so the
    skip is a fact somebody can see rather than a silent pass. It is the same
    reason `check-skippable-tests-declared.py` exists.

Override the location with `PDFCER_REQUEST_CHANNEL` when the channel lives
elsewhere.

USAGE
=====
    python tools/check-requests-scoped.py
    python tools/check-requests-scoped.py --list    # every request, always

No baseline file, deliberately: a suppression list silences exactly what a
gate exists to catch, and this gate is green at baseline today (0 scoped,
6 answered), so there is nothing to suppress.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ROADMAP = ROOT / "docs" / "ROADMAP.md"

DEFAULT_CHANNEL = Path("D:/Dev/FeatureRequests/pdfce_FeatureRequests")

# Files that can carry an answer. `notice_` is included because an outbound
# notice can be the whole answer to a request that needed telling rather than
# building.
ANSWER_PREFIXES = ("reply_", "done_", "notice_")


def channel_dir() -> Path:
    override = os.environ.get("PDFCER_REQUEST_CHANNEL")
    return Path(override) if override else DEFAULT_CHANNEL


def read(path: Path) -> str:
    """Text of a file, or empty on any read failure.

    Deliberately lenient: an unreadable sibling in the channel must not take
    the whole gate down, because the channel is written by another project and
    this tool has no authority over what appears in it.
    """
    try:
        return path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


def main() -> int:
    list_all = "--list" in sys.argv[1:]
    open_dir = channel_dir() / "open"

    if not open_dir.is_dir():
        print(f"check-requests-scoped: SKIPPED — channel not present at {open_dir}")
        print("  Not an error: the request channel lives outside this repository and")
        print("  is absent in CI. Set PDFCER_REQUEST_CHANNEL to point elsewhere.")
        print("  (Announced rather than returned silently — R255: a check that")
        print("   declines to run must not look like one that ran and passed.)")
        return 0

    requests = sorted(p for p in open_dir.glob("request_*.md") if p.is_file())
    if not requests:
        print(f"check-requests-scoped: clean — no open requests in {open_dir}")
        return 0

    answers = [p for p in open_dir.iterdir() if p.name.startswith(ANSWER_PREFIXES)]
    answer_text = {p.name: read(p) for p in answers}
    roadmap = read(ROADMAP)

    answered: list[tuple[str, list[str]]] = []
    scoped_unanswered: list[str] = []
    untouched: list[str] = []

    for req in requests:
        cites = sorted(name for name, text in answer_text.items() if req.name in text)
        in_roadmap = req.name in roadmap
        if cites:
            answered.append((req.name, cites))
        elif in_roadmap:
            scoped_unanswered.append(req.name)
        else:
            untouched.append(req.name)

    if scoped_unanswered:
        print("check-requests-scoped: a SCOPED request has no answer in the channel.")
        print("  `R242`: a request leaves `open/` when it is ANSWERED, not when it is")
        print("  scoped. These are planned and unspoken-for — the far side cannot tell")
        print("  them apart from ones nobody has read:")
        for name in scoped_unanswered:
            print(f"    ! {name}")
        print("\n  Write the reply, or say plainly that it is declined and why.")
        return 1

    print(
        f"check-requests-scoped: clean — {len(requests)} open request(s); "
        f"{len(answered)} answered, {len(untouched)} not yet taken up."
    )

    if answered and (list_all or untouched):
        print("\n  ALREADY ANSWERED — do NOT dispatch these as new work (`R242`):")
        for name, cites in answered:
            print(f"    · {name}")
            for c in cites:
                print(f"        answered by {c}")

    if untouched:
        print("\n  NOT YET TAKEN UP — no reply, no roadmap entry:")
        for name in untouched:
            print(f"    + {name}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
