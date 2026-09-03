#!/usr/bin/env python3
"""cmyk-operand-census — WHICH `DeviceCMYK` operands do real pages actually ask
for, and how often?

WHY THIS EXISTS
===============
pdfcer's `DeviceCMYK -> sRGB` conversion is a fitted lattice, and every
evaluation of it so far has been **one colour at a time** — a green on one
conformance patch, a grey on another. That is enough to find a defect and not
enough to say whether a table is good, because it says nothing about whether
the colours being tested are colours anybody paints.

The sibling `iccce` project offered, on 2026-08-29, to return what real ICC
profiles say for a **list** of operands rather than for one green at a time:

  > *"give me a list of the CMYK operands your table is actually asked for on
  > a real page, and I will return what the profiles say for all of them at
  > once."*

This produces that list. It is a **characterisation set**, not a test: its
output is an input to somebody else's measurement.

★ It also answers a question pdfcer could not previously ask itself — *is the
conversion's accuracy concentrated anywhere?* A table that is 20 counts out on
a corner of the hypercube nobody paints is a different problem from one that is
5 counts out on the twenty operands that cover most of the ink on real pages.

WHAT IT CAN SEE, AND WHAT IT CANNOT — READ THIS BEFORE QUOTING A NUMBER
=======================================================================
It scans inflated content streams for the `k` and `K` operators — the two
places a `DeviceCMYK` colour is written **literally**, as four numbers followed
by an operator, with no colour-space resolution required.

**It deliberately does NOT see, and the census is therefore a LOWER BOUND:**

  * `sc` / `scn` under a colour space that RESOLVES to `DeviceCMYK` — an
    `/ICCBased` with `/N 4`, a `/Separation` or `/DeviceN` over a `DeviceCMYK`
    alternate, an `/Indexed` over any of those. Resolving those needs the
    document's resource dictionaries and its tint transforms, i.e. a real
    parser and a real function evaluator.
  * **Images.** Every texel of a `DeviceCMYK` image is an operand, and a single
    photograph contributes more distinct operands than a whole corpus of vector
    art. Including them would drown the vector population rather than extend
    it, and they are a different question (an image's colours are sampled from
    a continuum; a fill's are typed by a human or emitted by a preset).
  * Shading function outputs, and `Type 3` glyph procedures' own colours.

**Why a lower bound is still the right deliverable.** The question is *"what is
the table asked for"*, and every operand this finds genuinely is asked for. A
list that is incomplete but sound is directly usable; a list padded with
guesses is not. The omissions are named here so a consumer can weight the
answer rather than discover the gap later.

★★ **AND ONE OMISSION IS DELIBERATELY LOAD-BEARING.** The `k` operator is
exactly the population whose conversion pdfcer controls end to end and whose
errors an operator SEES as flat wrong colour — a logo, a rule, a fill. It is
the population the shipped default's justification was written about. Starting
anywhere else would be starting somewhere easier to measure and harder to act
on.

THE PARSE, AND WHY IT IS A TOKEN SCAN RATHER THAN A REGEX
=========================================================
A regex for `num num num num k` over an inflated stream matches inside binary
payloads — an embedded font program, an inline image's data, a compressed
XObject that happens to inflate to bytes that look like numbers. So the stream
is TOKENISED (whitespace-delimited, PDF numeric syntax only) and the operator
is accepted only when the four preceding tokens are all valid PDF numbers in
`[0, 1]` after clamping, and the token before those is NOT itself a number that
would make this a five-operand sequence.

**Inline images are skipped explicitly** (`BI` … `ID` … `EI`): their binary
payload sits in the middle of the content stream and is not tokenisable.
Getting this wrong is how a scan reports thousands of nonsense operands and
looks like it worked.

USAGE
=====
    python tools/cmyk-operand-census.py [--corpus DIR] [--top 40]
                                        [--tsv out.tsv] [--json out.json]
                                        [--quantise 100]

`--quantise` bins operands to `1/N` before counting, so `0.4999` and `0.5`
are one operand rather than two. Default 100 (a hundredth of full ink), which
is finer than any press can hold and coarser than the noise a producer's
float formatting introduces. The RAW unquantised count is reported alongside
so the effect of the binning is visible rather than assumed.

★★ **THE `#` HEADER NAMES FILES. THE OPERAND TABLE DOES NOT.** If a corpus
you point this at is licensed material whose name may not appear in this
repository, do **not** paste the header into a tracked file, a commit message
or a channel note — `tools/check-suite-name-absent.py` will fire, and by then
the term is in a CI log. `--set` exists for exactly this: it writes the
operand list ALONE, four numbers per line, with no provenance of any kind.
A frequency table of CMYK tuples is not anybody's copyrightable expression;
a list of their file names is a different question and is not worth having.

Out-of-tree tooling like `tools/render-parity` and `tools/flat-color-parity.py`:
never shipped, never in `cargo test`, no GUI-core dependency. Pure standard
library plus `zlib`; no numpy, no renderer, no corpus mutation.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import zlib
from collections import Counter
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# PDF numeric syntax (§7.3.3): optional sign, digits, optional fraction. No
# exponent -- PDF has no exponential notation, and accepting `1e3` here would
# silently admit PostScript-ish content a conforming reader would reject.
NUMBER = re.compile(rb"^[+-]?(?:\d+\.?\d*|\.\d+)$")

# A conservative upper bound on how much one file may contribute, so a single
# pathological page cannot dominate the census. Reported when it fires --
# a silent cap is exactly the "no silent caps" failure this project keeps
# re-learning.
MAX_PER_FILE = 200_000


def inflate_streams(data: bytes):
    """Yield every stream payload in `data`, inflated where it is Flate.

    Deliberately structural-free: it does not parse the cross-reference table,
    resolve objects, or care which stream is a page's `/Contents`. A census
    wants every content-bearing stream including form XObjects, `Type 3` glyph
    procedures and annotation appearances, and those are reached by walking
    the file rather than the page tree.

    A stream that will not inflate is skipped silently HERE and counted by the
    caller: most of them are images, and an image is not a parse failure.
    """
    for m in re.finditer(rb"stream\r?\n?", data):
        start = m.end()
        end = data.find(b"endstream", start)
        if end < 0:
            continue
        raw = data[start:end]
        try:
            yield zlib.decompress(raw)
        except zlib.error:
            # Not Flate, or truncated. An uncompressed content stream is
            # common enough to be worth trying as-is, and a binary payload
            # will simply produce no valid `k` sequences.
            yield raw


def strip_inline_images(tokens: list[bytes]) -> list[bytes]:
    """Drop everything between `ID` and `EI`.

    An inline image's samples are raw binary in the middle of the operator
    stream (§8.9.7). Tokenising them produces garbage that occasionally looks
    like four numbers and an operator, which is how a scan invents operands
    nobody wrote.
    """
    out: list[bytes] = []
    skipping = False
    for t in tokens:
        if skipping:
            if t == b"EI":
                skipping = False
            continue
        if t == b"ID":
            skipping = True
            continue
        out.append(t)
    return out


def operands(stream: bytes):
    """Yield `(c, m, y, k, stroking)` for every `k` / `K` in one stream."""
    tokens = strip_inline_images(stream.split())
    for i, tok in enumerate(tokens):
        if tok not in (b"k", b"K"):
            continue
        if i < 4:
            continue
        four = tokens[i - 4 : i]
        if not all(NUMBER.match(t) for t in four):
            continue
        # ★ The five-operand guard. `0 0 0 0 1 k` is not a DeviceCMYK fill --
        # it is malformed, or it is four operands preceded by an unrelated
        # number, and reading the last four of five silently invents a colour
        # the document never wrote.
        if i >= 5 and NUMBER.match(tokens[i - 5]):
            continue
        try:
            vals = [float(t) for t in four]
        except ValueError:
            continue
        if any(v < 0.0 or v > 1.0 for v in vals):
            # Out of §8.6.4.4's stated range. Counted separately by the
            # caller rather than clamped: a clamp would fold a malformed
            # operand into a legitimate one and hide it.
            yield (None, None, None, None, tok == b"K")
            continue
        yield (*vals, tok == b"K")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--corpus",
        type=Path,
        action="append",
        default=None,
        help="a directory to scan, repeatable. Defaults to fixtures/external.",
    )
    ap.add_argument("--top", type=int, default=40, help="rows to print")
    ap.add_argument("--quantise", type=int, default=100, help="bin to 1/N; 0 disables")
    ap.add_argument("--tsv", type=Path, help="write EVERY distinct operand here")
    ap.add_argument("--json", type=Path, help="write the summary here")
    ap.add_argument(
        "--set",
        type=Path,
        help="write the OPERAND LIST ALONE here -- `c m y k`, one per line, no "
        "counts, no file names, no header. The shareable artefact: safe to hand "
        "to another project or paste into a note even when the corpus behind it "
        "is licensed.",
    )
    ap.add_argument(
        "--set-coverage",
        type=int,
        default=95,
        help="percentage of paint events --set must cover (default 95)",
    )
    args = ap.parse_args()

    corpora = args.corpus or [REPO / "fixtures" / "external"]
    corpora = [c for c in corpora if c.is_dir()]
    if not corpora:
        sys.exit("SKIP: no readable corpus directory (none of these are redistributable)")

    counts: Counter = Counter()
    raw_distinct: set = set()
    files_with = 0
    files_scanned = 0
    out_of_range = 0
    capped_files = []

    per_file: Counter = Counter()
    for pdf in sorted(p for c in corpora for p in c.rglob("*.pdf")):
        files_scanned += 1
        try:
            data = pdf.read_bytes()
        except OSError:
            continue
        # Cheap pre-filter: no `k`/`K` operator can exist without the byte.
        # Saves inflating several thousand files that cannot contribute.
        if b"k" not in data and b"K" not in data:
            continue
        here = 0
        found = False
        for stream in inflate_streams(data):
            for c, m, y, k, _stroking in operands(stream):
                if c is None:
                    out_of_range += 1
                    continue
                found = True
                here += 1
                if here > MAX_PER_FILE:
                    capped_files.append(pdf.name)
                    break
                raw_distinct.add((c, m, y, k))
                if args.quantise:
                    q = args.quantise
                    key = tuple(round(v * q) / q for v in (c, m, y, k))
                else:
                    key = (c, m, y, k)
                counts[key] += 1
            if here > MAX_PER_FILE:
                break
        if found:
            files_with += 1
            per_file[str(pdf)] = here

    total = sum(counts.values())
    if total == 0:
        print("no `k`/`K` operands found", file=sys.stderr)
        return

    ranked = counts.most_common()
    # How many distinct operands cover 50 % / 80 % / 95 % of all paints. This
    # is the number that decides whether a characterisation set is 20 entries
    # or 20,000, and it is the one a consumer actually asks for.
    coverage = {}
    running = 0
    for pct in (50, 80, 95, 99):
        coverage[pct] = None
    running = 0
    for idx, (_op, n) in enumerate(ranked, start=1):
        running += n
        for pct in (50, 80, 95, 99):
            if coverage[pct] is None and running * 100 >= total * pct:
                coverage[pct] = idx

    for c in corpora:
        print(f"# corpus            {c}")
    print(f"# files scanned     {files_scanned}")
    print(f"# files with k/K    {files_with}")
    print(f"# paint events      {total}")
    print(f"# distinct operands {len(counts)} quantised to 1/{args.quantise}"
          f"  ({len(raw_distinct)} raw)")
    print(f"# out of [0,1]      {out_of_range}")
    if capped_files:
        print(f"# CAPPED at {MAX_PER_FILE} operands: {len(capped_files)} file(s)"
              f" -- {', '.join(capped_files[:5])}")
    for pct in (50, 80, 95, 99):
        print(f"# {pct:>2}% of paints covered by the top {coverage[pct]} operand(s)")
    # ★★ WHICH FILES SUPPLIED THE POPULATION, and it is not decoration.
    # A census whose top operand is 58 % of all paints is describing ONE FILE
    # unless it says otherwise, and "58 % of paints" and "58 % of pages" are
    # different claims that read identically. The breakdown below is what
    # tells them apart, and it prints unconditionally so nobody has to think
    # to ask for it.
    top_files = per_file.most_common(3)
    dominant = top_files[0][1] / total if top_files else 0.0
    print("#")
    print(f"# top contributing file(s), of {files_with} with any operand:")
    for name, n in top_files:
        print(f"#   {n:>6} paint(s)  {n / total:6.2%}  {Path(name).name}")
    if dominant > 0.25:
        print(f"# * ONE FILE SUPPLIES {dominant:.0%} OF THE POPULATION. Read the")
        print("#   ranking as a description of this corpus, not of PDFs in general.")
    print("#")
    print("rank\tcount\tshare\tc\tm\ty\tk")
    for rank, (op, n) in enumerate(ranked[: args.top], start=1):
        print(f"{rank}\t{n}\t{n / total:.4f}\t" + "\t".join(f"{v:.4f}" for v in op))

    if args.tsv:
        lines = ["count\tc\tm\ty\tk"]
        lines += [f"{n}\t" + "\t".join(f"{v:.4f}" for v in op) for op, n in ranked]
        args.tsv.write_text("\n".join(lines) + "\n", encoding="utf-8")
        print(f"# wrote {len(ranked)} row(s) to {args.tsv}", file=sys.stderr)
    if args.set:
        # Take operands in rank order until the requested share of paint
        # events is covered. Rank order rather than a lattice, because the
        # question is "what is the table ASKED FOR", and a lattice answers
        # "what could it be asked for" -- a different and much larger set
        # whose extra points are exactly the ones nobody paints.
        chosen, running = [], 0
        for op, n in ranked:
            chosen.append(op)
            running += n
            if running * 100 >= total * args.set_coverage:
                break
        rows = [" ".join(f"{v:.4f}" for v in op) for op in chosen]
        args.set.write_text("\n".join(rows) + "\n", encoding="utf-8")
        print(
            f"# wrote {len(chosen)} operand(s) covering {running / total:.1%} "
            f"of paint events to {args.set}",
            file=sys.stderr,
        )
    if args.json:
        args.json.write_text(
            json.dumps(
                {
                    "files_scanned": files_scanned,
                    "files_with_operands": files_with,
                    "paint_events": total,
                    "distinct_operands": len(counts),
                    "distinct_operands_raw": len(raw_distinct),
                    "quantise": args.quantise,
                    "out_of_range": out_of_range,
                    "coverage": coverage,
                    "capped_files": capped_files,
                    "top_files": [
                        {"file": Path(f).name, "paints": n} for f, n in per_file.most_common(5)
                    ],
                    "top": [
                        {"count": n, "cmyk": list(op)} for op, n in ranked[: args.top]
                    ],
                },
                indent=2,
            ),
            encoding="utf-8",
        )


if __name__ == "__main__":
    main()
