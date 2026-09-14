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
     own convention is that a reply names the request it answers (all ten open
     requests across both channels satisfy it today), so this is an exact
     match, not a similarity score. A gate that guessed would be worse than none.
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

WHAT IT CANNOT SEE, FOUND BY BEING IN THAT STATE THE DAY AFTER IT SHIPPED
========================================================================
**A request that has been WORKED but not answered is invisible to this gate.**
Red fires only on *scoped in `ROADMAP.md` AND unanswered*. A request that is
neither scoped nor answered reads as "not yet taken up" — which is also what a
request nobody has opened reads as. The two are indistinguishable from here.

That is not hypothetical. `G015` was fixed and committed (`025d703d`) with no
reply written and no roadmap entry yet; the consuming project discovered the
delivery by reading the engine's `git log` while checking something else, and
said so:

    A delivery can reach you as a COMMIT before it reaches you as a reply.

This gate was green throughout, correctly by its own rule and uselessly in
fact. Widening it would mean reading `git log` for commits citing a topic key,
which is a different and much softer predicate than "a file cites this
filename" — deliberately not attempted, because a fuzzy match here would fire
on ordinary work and teach its reader to ignore it.

⇒ So the habit stands where the gate cannot: **write the reply.** The gate
catches the case where a plan exists and nobody was told; it cannot catch the
case where the work is DONE and nobody was told.

WHAT IT DELIBERATELY DOES NOT DO
================================
* **It does not archive anything.** Moving a file out of `open/` is the
  requesting project's step; they close their own exchanges and write their
  own index rows. A gate that tidied their directory would duplicate work in
  flight.
* **It does not match on topic keys, subject words or similarity.** Only one
  of the six current requests carries a `G0NN` key in its filename, so a
  key-based match would silently cover one request in ten while looking
  thorough. Filename citation covers all ten.
* **It does not read `archive/`** for the answered check. A request in `open/`
  whose only answer is archived is a state worth seeing, not hiding.

THERE IS MORE THAN ONE CHANNEL, AND THIS GATE SHIPPED NOT KNOWING THAT
=====================================================================
pdfcer answers **two** request channels, and the first cut of this file knew
only one:

  * `pdfce_FeatureRequests` — the `pdfcer-gui` shell (6 open requests);
  * `iccce_FeatureRequests` — the ICC colour-management partner (4 open).

★★ The second was found within the hour of this gate shipping, and by the same
blind spot it would itself have had. A reply-citation audit reported 19, then
15, then 10 "missing" replies from `ROADMAP.md`; every one of the ten was
present, four of them in `iccce_FeatureRequests`, because the audit was scoped
to one channel while the register cites both. **A gate that scans one of two
directories is not 50 % of a gate — it is a gate that reports "clean" about a
place it never looked.**

The channel set is an explicit LIST, not a glob over
`D:/Dev/FeatureRequests/`. That directory also holds `ComBridge_`, `SWFormat_`
and `ScripTree_FeatureRequests`, which belong to other projects entirely; a
glob would audit somebody else's correspondence and report on obligations that
are not pdfcer's. Adding a channel here is a deliberate act, which is the
right cost for a question whose wrong answer is silent.

THE CHANNELS ARE OUTSIDE THE REPOSITORY
=======================================
None of them is in the tree, and none is present in CI. This gate therefore
CANNOT run there, and says so **loudly**, per channel, by name, on stdout.

    ★★ That is `R255`'s shape and it is handled deliberately. A check that
    returns early on a missing input and prints nothing reports to the runner
    exactly like one that ran and passed. This one prints
    `SKIPPED — channel not present` and names the path it looked for, so the
    skip is a fact somebody can see rather than a silent pass. It is the same
    reason `check-skippable-tests-declared.py` exists.

Override with `PDFCER_REQUEST_CHANNEL`, which takes one or more paths
separated by the platform's path separator and REPLACES the built-in list.

USAGE
=====
    python tools/check-requests-scoped.py
    python tools/check-requests-scoped.py --list    # every request, always

No baseline file, deliberately: a suppression list silences exactly what a
gate exists to catch, and this gate is green at baseline today (0 scoped,
10 answered across two channels), so there is nothing to suppress.
"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ROADMAP = ROOT / "docs" / "ROADMAP.md"

# Every channel pdfcer is a party to. A list, not a glob — see the module
# docs for why scanning `D:/Dev/FeatureRequests/` wholesale would be wrong.
DEFAULT_CHANNELS = (
    Path("D:/Dev/FeatureRequests/pdfce_FeatureRequests"),
    Path("D:/Dev/FeatureRequests/iccce_FeatureRequests"),
)

# Files that can carry an answer.
#
# A REGEX over the whole name, not a prefix tuple, because the two channels do
# not agree on naming and neither is wrong:
#
#     pdfce_FeatureRequests   reply_G013_....md        done_G013_CONSUMED.md
#     iccce_FeatureRequests   2026-08-21-reply-....md  note_boundary_....md
#
# ★ The prefix tuple this replaced matched the first column and silently
# missed the second. It still reported every request answered -- because some
# OTHER file happened to cite each one -- which is the failure mode worth
# naming: a matcher that is wrong about HOW it found something still prints
# "clean", and the next file named the other way would have been invisible.
#
# `note` is included alongside `notice` because the second channel spells it
# that way; an outbound note can be the whole answer to a request that needed
# telling rather than building.
ANSWER_RE = re.compile(r"(^|[-_])(reply|done|notice|note)([-_]|$)", re.IGNORECASE)


def is_answer(name: str) -> bool:
    """Could this file carry an answer? A request never can, whatever it cites."""
    return not name.startswith("request") and bool(ANSWER_RE.search(name[:-3]))


def channels() -> tuple[Path, ...]:
    """The channels to audit — the override replaces the list, never adds to it.

    Replacing rather than extending is deliberate: a test or a relocated
    checkout needs to say "look HERE and nowhere else", and an override that
    silently kept auditing two absent default paths would print two SKIP lines
    nobody asked for.
    """
    override = os.environ.get("PDFCER_REQUEST_CHANNEL")
    if not override:
        return DEFAULT_CHANNELS
    return tuple(Path(p) for p in override.split(os.pathsep) if p)


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


def audit(open_dir, roadmap):
    """Classify one channel's open requests. Returns (answered, scoped, new)."""
    requests = sorted(p for p in open_dir.glob("request_*.md") if p.is_file())
    answers = [p for p in open_dir.iterdir() if p.suffix == ".md" and is_answer(p.name)]
    answer_text = {p.name: read(p) for p in answers}

    answered: list[tuple[str, list[str]]] = []
    scoped_unanswered: list[str] = []
    untouched: list[str] = []
    for req in requests:
        cites = sorted(name for name, text in answer_text.items() if req.name in text)
        if cites:
            answered.append((req.name, cites))
        elif req.name in roadmap:
            scoped_unanswered.append(req.name)
        else:
            untouched.append(req.name)
    return answered, scoped_unanswered, untouched


def main() -> int:
    list_all = "--list" in sys.argv[1:]
    roadmap = read(ROADMAP)

    total_req = 0
    all_answered: list[tuple[str, str, list[str]]] = []
    all_scoped: list[tuple[str, str]] = []
    all_new: list[tuple[str, str]] = []
    looked_at = 0

    for chan in channels():
        open_dir = chan / "open"
        if not open_dir.is_dir():
            print(f"check-requests-scoped: SKIPPED — {chan.name} not present at {open_dir}")
            continue
        looked_at += 1
        answered, scoped, new = audit(open_dir, roadmap)
        total_req += len(answered) + len(scoped) + len(new)
        all_answered += [(chan.name, n, c) for n, c in answered]
        all_scoped += [(chan.name, n) for n in scoped]
        all_new += [(chan.name, n) for n in new]

    if looked_at == 0:
        print("  Not an error: the request channels live outside this repository and")
        print("  are absent in CI. Set PDFCER_REQUEST_CHANNEL to point elsewhere.")
        print("  (Announced rather than returned silently — R255: a check that")
        print("   declines to run must not look like one that ran and passed.)")
        return 0

    if all_scoped:
        print("check-requests-scoped: a SCOPED request has no answer in its channel.")
        print("  `R242`: a request leaves `open/` when it is ANSWERED, not when it is")
        print("  scoped. These are planned and unspoken-for — the far side cannot tell")
        print("  them apart from ones nobody has read:")
        for chan, name in all_scoped:
            print(f"    ! [{chan}] {name}")
        print("\n  Write the reply, or say plainly that it is declined and why.")
        return 1

    print(
        f"check-requests-scoped: clean — {looked_at} channel(s), {total_req} open "
        f"request(s); {len(all_answered)} answered, {len(all_new)} not yet taken up."
    )

    if all_answered and (list_all or all_new):
        print("\n  ALREADY ANSWERED — do NOT dispatch these as new work (`R242`):")
        for chan, name, cites in all_answered:
            print(f"    · [{chan}] {name}")
            for c in cites:
                print(f"        answered by {c}")

    if all_new:
        print("\n  NOT YET TAKEN UP — no reply, no roadmap entry:")
        for chan, name in all_new:
            print(f"    + [{chan}] {name}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
