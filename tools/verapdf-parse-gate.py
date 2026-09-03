#!/usr/bin/env python3
"""Judge pdfcer's OWN OUTPUT with an independent PDF parser (veraPDF).

WHY THIS EXISTS
---------------
Every test pdfcer has reads pdfcer's output with **pdfcer's own parser**.
``round-trip`` reloads through ``pdfcer-core``; the forms tests assert
through ``parse_acroform``; the redaction tests read back with the same
lexer that wrote the bytes. That is a closed loop, and a closed loop
cannot see a defect that both halves share.

This is not hypothetical. **R159** was minted on 2026-08-07 after
exactly that failure: ``flatten`` left ``/AcroForm /Fields`` naming
objects it had deleted, and *every* forms test passed — because
``parse_acroform`` silently drops entries that no longer resolve. The
model looked right while the file was wrong. No amount of in-house
discipline closes that gap, because the discipline and the defect live
in the same codebase.

So this gate hands pdfcer's bytes to a **completely independent
implementation** — veraPDF, a Java PDF parser written by people who
have never seen this repository — and asks one question: *can you read
it at all?*

WHAT IT DOES **NOT** DO, AND WHY
--------------------------------
It does **not** check PDF/A conformance, even though PDF/A conformance
is veraPDF's entire reason for existing. pdfcer does not write PDF/A yet
(``to-pdfa`` is unimplemented), and running a PDF/A profile against
ordinary PDF output reports a wall of failures that are **not defects**
— no XMP metadata, no ``/OutputIntent``, unembedded fonts. Every one of
those is correct behaviour for a file that never claimed to be PDF/A.

``--off`` turns validation off and runs the parser alone, which is
precisely the question worth asking today. When ``to-pdfa`` ships, the
conformance gate is a *separate* tool, not a flag on this one: the two
answer different questions and a failure in each means something
different.

THE TRAP THIS TOOL EXISTS TO AVOID (read before "simplifying" it)
-----------------------------------------------------------------
The obvious implementation is ``verapdf --off <file>`` plus a check on
the exit code. **That gate passes everything, forever, including a file
with no xref table.** Measured 2026-08-07:

    verapdf --off <valid.pdf>    -> exit 0
    verapdf --off <garbage.pdf>  -> exit 0     <-- both zero

``--off`` suppresses the failure signal along with the validation.
The parse verdict lives **only in the XML report body**, never in the
exit status. This is **R162** ("an assertion that something is ABSENT
proves nothing until the container has been shown capable of holding
it") in the wild, on the day the rule was written.

``self_test()`` below exists so that trap cannot silently reopen. It
runs **both directions**, and both are necessary:

* a deliberately broken file **must** be detected — otherwise every
  "clean" result the gate has ever printed is vacuous;
* a known-good document **must not** be — otherwise the gate flags
  everything, and detecting the broken file proves nothing. A detector
  that never says no is not a detector.

Run it with ``--self-test``.

WHY ``--mode full`` IS THE DEFAULT (the second trap)
-----------------------------------------------------
``round-trip --mode incremental`` with an empty dirty set promises
**whole-file byte identity** — the output *is* the input, byte for
byte. Validating that output tells you the INPUT parses. It says
nothing whatsoever about pdfcer.

``full`` is a complete rewrite: every object definition, the xref
table, the trailer, all emitted by pdfcer. That is the only mode where
a veraPDF verdict is a verdict on *pdfcer's writer*. ``append-identity``
is also meaningful (it exercises the real append writer), and is
offered. ``incremental`` is offered too, but see ``MODE_NOTES`` — the
tool says out loud when the mode it was given cannot prove anything.

LICENSING — WHY THIS IS A SEPARATE PROCESS AND WHY IT SKIPS
------------------------------------------------------------
veraPDF is dual-licensed **GPLv3+ / MPLv2+** (verified against every
component repo's ``README`` and the presence of ``LICENSE.MPL`` in
``veraPDF-apps``). pdfcer **elects MPL-2.0** — see ``docs/LEGAL.md``.

Its startup banner states **both** branches::

    Released under the GNU General Public License v3
    and the Mozilla Public License v2 or later.

An earlier version of this file claimed the banner named only GPL and
was misleading. That was wrong, and the correction is kept here rather
than quietly deleted because the *mechanism* is worth knowing: the
licence sentence is **line-wrapped**, so a truncated read (``head -5``,
cutting at exactly line 5) returns a complete, plausible, wrong
sentence. Mid-word truncations announce themselves; a cut at a wrap
point does not — and wrapping puts clause boundaries on line boundaries
by construction, so the safe-looking truncation is the likely one.
**Do not "fix" the banner or report it upstream.**

pdfcer therefore:

* invokes veraPDF as a **separate process** over its documented CLI —
  never links, embeds, or vendors it;
* never redistributes it (hence the out-of-tree install path);
* keeps it **dev-time only** — it appears in no ``Cargo.toml`` and
  correctly never appears in ``THIRD_PARTY_LICENSES.md``, which
  ``cargo-about`` generates from Cargo dependencies alone. Do not
  "fix" that by adding it.

**And this gate SKIPS — never fails — when veraPDF is absent.** That is
a licensing requirement, not a convenience: a gate that *required*
veraPDF would make it a de facto build dependency of pdfcer, which
muddies the arms-length position and breaks anyone who clones the repo
without installing a GPL/MPL Java application. A skip is reported
loudly on stderr so it cannot be mistaken for a pass.

USAGE
-----
    python tools/verapdf-parse-gate.py <pdf-or-dir> [...] [options]

    --mode {full,append-identity,incremental}   default: full
    --limit N              stop after N inputs
    --batch N              files per veraPDF invocation (default 32)
    --keep                 keep the produced PDFs for inspection
    --self-test            prove the gate can fail, then exit
    --verapdf PATH         override veraPDF discovery
    --timeout SECONDS      per-file budget before pdfcer counts as HUNG
                           (default 120). A hang is the worst class the
                           gate reports: without a budget one
                           non-terminating input stalls the sweep and
                           everything after it is silently never tested.

Discovery order for veraPDF: ``--verapdf``, then ``$PDFCER_VERAPDF``,
then ``D:\\tools\\verapdf\\verapdf.bat``, then ``verapdf`` on ``PATH``.

EXIT CODES
----------
0   no regressions and no hangs, **or** veraPDF is not installed (skip)
1   pdfcer made a file WORSE than its input, or never terminated on one
2   the harness itself failed (pdfcer missing, bad arguments)

A parse failure is printed with the file, the mode that produced it,
and veraPDF's own exception message — counted, never rounded away.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from pathlib import Path

# Where veraPDF is expected when nothing overrides it. Deliberately
# OUT of the repository tree: pdfcer must never redistribute it.
DEFAULT_VERAPDF = Path(r"D:\tools\verapdf\verapdf.bat")

# What each round-trip mode proves about pdfcer's WRITER, which is the
# only thing this gate is trying to judge.
MODE_NOTES = {
    "full": None,  # the meaningful default; every byte is pdfcer's
    "append-identity": None,  # exercises the real append writer
    "incremental": (
        "MODE WARNING: 'incremental' with no edits promises whole-file "
        "byte identity, so the file handed to veraPDF IS the input. A "
        "pass proves the INPUT parses and says nothing about pdfcer's "
        "writer. Use --mode full for a verdict on pdfcer."
    ),
}


@dataclass
class ParseFailure:
    """One file pdfcer wrote that an independent parser could not read."""

    source: Path
    mode: str
    message: str


def find_verapdf(override: str | None) -> Path | None:
    """Locate the veraPDF CLI, or return None so the caller can SKIP.

    Returning None rather than raising is the whole point — see the
    licensing note in the module docstring. Absence is a legitimate,
    non-failing state.
    """
    for candidate in (
        override,
        os.environ.get("PDFCER_VERAPDF"),
        str(DEFAULT_VERAPDF),
    ):
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    found = shutil.which("verapdf") or shutil.which("verapdf.bat")
    return Path(found) if found else None


def collect_inputs(paths: list[str], limit: int | None) -> list[Path]:
    """Expand the given files and directories into a list of PDFs."""
    out: list[Path] = []
    for raw in paths:
        p = Path(raw)
        if p.is_dir():
            out.extend(sorted(q for q in p.rglob("*.pdf") if q.is_file()))
        elif p.is_file():
            out.append(p)
        else:
            print(f"warn  not found, skipped: {p}", file=sys.stderr)
    return out[:limit] if limit else out


def build_cli(workdir: Path) -> Path:
    """Build `pdfcer` once and return a PRIVATE COPY of the binary.

    # Why a copy, and why not `cargo run` per file

    The obvious implementation calls ``cargo run -p pdfcer-cli`` for each
    input. That is wrong in two compounding ways, both measured on
    2026-08-07 rather than predicted:

    1. **It holds the build artifact hostage.** A sweep of a few hundred
       files keeps ``target/debug/pdfcer.exe`` in use for many
       minutes, and any concurrent ``cargo test`` in the same repository
       dies with ``failed to remove file ... Access is denied
       (os error 5)`` on Windows, because it cannot relink a running
       binary. That failure names a *file permission* problem and gives
       no hint that another job is the cause — a genuinely confusing
       error for anyone who did not start the sweep.
    2. **It re-runs cargo's dependency resolution every single time**,
       which dominates the runtime of the actual work.

    Building once and running a copy out of the sweep's own temp
    directory fixes both: ``target/`` is untouched for the whole run, so
    a developer can build and test normally while a long sweep is in
    flight.
    """
    proc = subprocess.run(
        ["cargo", "build", "-q", "-p", "pdfcer-cli"],
        capture_output=True,
        text=True,
        # Decode as UTF-8 with replacement, NEVER the platform locale.
        # `text=True` alone decodes with cp1252 on Windows, and a single
        # byte outside that codepage (0x8f, hit on a real corpus file)
        # raises UnicodeDecodeError inside a subprocess READER THREAD --
        # so the traceback names threading.py and encodings/cp1252.py and
        # never mentions this tool at all. A sweep exists to run bytes
        # from producers we do not control; it must not assume its own
        # locale can spell their output.
        encoding="utf-8",
        errors="replace",
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"cargo build -p pdfcer-cli failed:\n{proc.stderr or proc.stdout}"
        )
    exe = "pdfcer.exe" if os.name == "nt" else "pdfcer"
    built = Path("target") / "debug" / exe
    if not built.is_file():
        raise RuntimeError(f"built pdfcer not found at {built}")
    private = workdir / exe
    shutil.copy2(built, private)
    return private


def produce(cli: Path, src: Path, mode: str, dest: Path, timeout: float) -> str | None:
    """Have pdfcer WRITE `src` to `dest` in `mode`.

    Returns None on success, or a reason string. A pdfcer refusal is
    NOT a gate failure: refusing by name is correct behaviour (R27),
    and a sweep that counted refusals as failures would push the
    implementation toward guessing rather than refusing.

    Raises [`Hang`] if pdfcer does not terminate within `timeout`.

    # Why a per-file timeout is not optional

    Without one, a single non-terminating input stalls the whole sweep
    and **everything after it is silently never tested** — while the
    tool prints nothing at all, because results are only reported at the
    end. That is R162 at the harness level: a sweep that stopped at file
    87 of 331 and a sweep that passed all 331 look identical from the
    outside.

    This is not theoretical. On 2026-08-07 a sweep of pdfium's corpus sat
    on `bug_455199.pdf` for over thirty minutes; the remaining 244 files
    were never examined, and the only reason anyone noticed was that a
    process listing showed one `pdfcer.exe` alive far longer than any
    file should take. A hang is now a **reported finding** with the file
    that caused it — the most severe class the gate can report, because
    a hang in the GUI is an unrecoverable freeze.
    """
    proc = subprocess.run(
        [
            str(cli),
            "round-trip", "--mode", mode, "-o", str(dest), str(src),
        ],
        capture_output=True,
        timeout=timeout,
        text=True,
        # Decode as UTF-8 with replacement, NEVER the platform locale.
        # `text=True` alone decodes with cp1252 on Windows, and a single
        # byte outside that codepage (0x8f, hit on a real corpus file)
        # raises UnicodeDecodeError inside a subprocess READER THREAD --
        # so the traceback names threading.py and encodings/cp1252.py and
        # never mentions this tool at all. A sweep exists to run bytes
        # from producers we do not control; it must not assume its own
        # locale can spell their output.
        encoding="utf-8",
        errors="replace",
    )
    if proc.returncode != 0 or not dest.is_file():
        detail = (proc.stderr or proc.stdout or "").strip().splitlines()
        return detail[-1] if detail else f"exit {proc.returncode}"
    return None


# Parse outcomes, ORDERED worst-last so they can be compared directly.
#
# # Why there are only TWO, after an attempt at three
#
# veraPDF really does distinguish "a PARSE taskException" from "a job
# counted in batchSummary/@failedToParse" — an earlier version of this
# file modelled both, as WARNED and FAILED.
#
# That was wrong, and wrong in the way that quietly destroys a gate.
# veraPDF reports the failure COUNT in the batch summary but does not say
# WHICH job it belongs to, so promoting exceptions to FAILED required
# guessing from `counted == len(results)`. In a 32-file batch with one
# counted failure and two exceptions, neither got promoted — meaning **a
# file's tier depended on what else happened to be in its batch**. The
# input scan and the output scan batch differently, so the same file could
# come out WARNED before and FAILED after and be reported as a regression
# purely from batching.
#
# Measured 2026-08-07 on qpdf's `c-empty.pdf`, a perfectly valid zero-page
# document (`/Type /Pages /Count 0 /Kids []`) that this gate accused pdfcer
# of breaking. veraPDF reports `failedToParse="1"` for its input AND its
# output; nothing regressed.
#
# The distinction also bought nothing. This gate is COMPARATIVE: all it
# needs to know is whether the output hit a parse problem the input did
# not. Collapsing to a boolean is batch-independent and answers exactly
# that question. A gate that cries wolf is one nobody reads, which is how
# the next real finding gets missed.
OK = 0
FAILED = 1  # veraPDF hit a PARSE problem, counted or not
TIER_NAME = {OK: "ok", FAILED: "parse-failure"}


def verapdf_parse_report(verapdf: Path, files: list[Path]) -> dict[str, tuple[int, str]]:
    """Run veraPDF in parse-only mode; return {file: (tier, message)}.

    # Two failure tiers, because veraPDF has two and conflating them lies

    Measured 2026-08-07 on `PDFBOX-6040-nodeloop.pdf`: veraPDF can emit
    a ``taskException type="PARSE"`` for a document that it nonetheless
    does **not** count in ``batchSummary/@failedToParse``. The first
    version of this function treated any PARSE exception as a failure,
    which made it stricter than veraPDF's own counter — and the
    cross-check below caught the disagreement rather than letting either
    number be believed.

    So:

    * ``FAILED``  — veraPDF raised a PARSE exception for it.
    * ``OK``      — nothing to report (the file simply does not appear).

    **There used to be a third tier, ``WARNED``**, for a PARSE exception
    veraPDF does not count in ``failedToParse``. It was removed, and the
    reason is worth keeping: veraPDF reports the failure COUNT without
    saying which job it belongs to, so promoting an exception to the
    harder tier meant guessing from ``counted == len(results)`` — which
    made **a file's grade depend on what else was in its 32-file batch.**
    Input and output scans batch differently, so one file could grade
    differently on each side and surface as a regression purely from
    batching. An aggregate must not be attributed to an individual.

    The verdict is read from the XML body and never from the exit
    status — ``--off`` returns 0 for a file with no xref table. See the
    module docstring; this is the tool's central hazard.
    """
    proc = subprocess.run(
        [str(verapdf), "--off", *[str(f) for f in files]],
        capture_output=True,
        text=True,
        # Decode as UTF-8 with replacement, NEVER the platform locale.
        # `text=True` alone decodes with cp1252 on Windows, and a single
        # byte outside that codepage (0x8f, hit on a real corpus file)
        # raises UnicodeDecodeError inside a subprocess READER THREAD --
        # so the traceback names threading.py and encodings/cp1252.py and
        # never mentions this tool at all. A sweep exists to run bytes
        # from producers we do not control; it must not assume its own
        # locale can spell their output.
        encoding="utf-8",
        errors="replace",
    )
    try:
        report = ET.fromstring(proc.stdout)
    except ET.ParseError as exc:
        # veraPDF produced something that is not a report at all. That
        # is a harness problem, not a verdict, and must not be silently
        # read as "nothing failed to parse".
        raise RuntimeError(
            f"veraPDF did not return a parseable report ({exc}). "
            f"stderr: {(proc.stderr or '').strip()[:300]}"
        ) from exc

    results: dict[str, tuple[int, str]] = {}
    for job in report.iter("job"):
        item = job.find("item")
        name_el = item.find("name") if item is not None else None
        name = (name_el.text or "").strip() if name_el is not None else "<unknown>"
        for task in job.findall("taskException"):
            if task.get("type") == "PARSE":
                msg_el = task.find("exceptionMessage")
                msg = (msg_el.text or "").strip() if msg_el is not None else "parse failed"
                results[name] = (FAILED, msg)

    # Cross-check against veraPDF's OWN count.
    #
    # This is NOT used to assign tiers any more (see the note on OK/FAILED
    # for why that was a false-positive generator). It is kept purely as a
    # sanity check: veraPDF must never count more failures than we found
    # exceptions for, because a counted failure with no exception attached
    # is one this gate would report as CLEAN.
    #
    # It has already earned its keep once — it caught the original
    # exception/count mismatch instead of letting either number be
    # believed.
    summary = report.find("batchSummary")
    if summary is not None:
        counted = int(summary.get("failedToParse", "0"))
        if counted > len(results):
            raise RuntimeError(
                f"extraction disagrees with veraPDF: batchSummary says "
                f"failedToParse={counted} but only {len(results)} PARSE "
                f"exception(s) were found. A failure with no exception "
                f"attached would be reported as clean. Fix the parser "
                f"rather than trusting either number."
            )
    return results


def self_test(verapdf: Path) -> int:
    """Prove the gate can FAIL. Exits non-zero if it cannot.

    R162: an assertion that something is absent proves nothing until
    the container has been shown capable of holding it. This gate's
    whole output is "no parse failures", which is exactly the shape
    that passes vacuously — so the tool ships with the proof attached.
    """
    with tempfile.TemporaryDirectory(prefix="verapdf-selftest-") as tmp:
        broken = Path(tmp) / "deliberately-broken.pdf"
        broken.write_bytes(b"%PDF-1.7\nthis is not a pdf\n")
        failures = verapdf_parse_report(verapdf, [broken])
        if not failures:
            print(
                "SELF-TEST FAILED: veraPDF reported no parse failure for a "
                "file with no xref table. The gate cannot detect anything "
                "and every 'clean' result it has ever printed is vacuous.",
                file=sys.stderr,
            )
            return 1
        (_name, (_tier, msg)), = failures.items()

        # THE SECOND DIRECTION, and it is the half that makes the first
        # half mean anything.
        #
        # A previous version asserted here that the broken file's TIER was
        # `FAILED` — which became unreachable the moment the two-tier model
        # collapsed to a boolean, because `FAILED` is now the only value
        # that can enter the results dict at all. The assertion could not
        # come out false, so it tested nothing while reading like a real
        # guard. **R162 committed by the fix for an R162 finding**, which is
        # exactly how this class survives: the dead branch was created by
        # a correct change and inherited its author's confidence.
        #
        # A tier assertion cannot be revived honestly, so the missing
        # discrimination is supplied where it actually exists — the gate's
        # real claim is "broken files appear, sound files do not", and a
        # gate that reported EVERY file as unreadable would pass a
        # one-directional test. So a known-good document is scanned too and
        # must come back clean.
        sound = Path(tmp) / "sound.pdf"
        shutil.copy2(Path("fixtures/synthetic/forms/demo-form.pdf"), sound)
        if verapdf_parse_report(verapdf, [sound]):
            print(
                "SELF-TEST FAILED: a known-good document was reported as a "
                "parse failure. The gate flags everything, so its 'broken "
                "file detected' result above proves nothing — a detector "
                "that never says no is not a detector.",
                file=sys.stderr,
            )
            return 1

        print(f"self-test ok — broken file detected, sound file clean: {msg}")
        return 0


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Validate pdfcer's own output with an independent PDF parser.",
    )
    ap.add_argument("paths", nargs="*", help="PDF files or directories")
    ap.add_argument("--mode", default="full", choices=sorted(MODE_NOTES))
    ap.add_argument("--limit", type=int, default=None)
    ap.add_argument("--batch", type=int, default=32)
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--verapdf", default=None)
    ap.add_argument(
        "--timeout",
        type=float,
        default=120.0,
        help="per-file seconds before pdfcer is treated as HUNG (default 120)",
    )
    args = ap.parse_args()

    verapdf = find_verapdf(args.verapdf)
    if verapdf is None:
        # SKIP, never fail. See the licensing note: a required gate
        # would make veraPDF a build dependency of pdfcer.
        print(
            "SKIP  veraPDF not found — this gate is optional by design "
            "(dev-time only, never a pdfcer dependency). Install it and "
            "set PDFCER_VERAPDF, or pass --verapdf PATH.",
            file=sys.stderr,
        )
        return 0

    if args.self_test:
        return self_test(verapdf)

    if not args.paths:
        ap.error("give at least one PDF or directory (or use --self-test)")

    note = MODE_NOTES[args.mode]
    if note:
        print(note, file=sys.stderr)

    inputs = collect_inputs(args.paths, args.limit)
    if not inputs:
        print("no input PDFs found", file=sys.stderr)
        return 2

    workdir = Path(tempfile.mkdtemp(prefix="verapdf-gate-"))
    produced: dict[Path, Path] = {}
    refused = 0
    hangs: list[Path] = []
    try:
        try:
            cli = build_cli(workdir)
        except RuntimeError as exc:
            print(f"harness error: {exc}", file=sys.stderr)
            return 2
        for i, src in enumerate(inputs):
            dest = workdir / f"{i:05d}-{src.name}"
            try:
                reason = produce(cli, src, args.mode, dest, args.timeout)
            except subprocess.TimeoutExpired:
                # A HANG, not a slow file. Recorded and reported by name
                # rather than stalling the sweep — see `produce`.
                hangs.append(src)
                continue
            if reason is not None:
                # A refusal is a correct outcome, not a gate failure.
                refused += 1
                continue
            produced[dest.resolve()] = src

        # Judge pdfcer against the INPUT'S baseline, not against perfection.
        #
        # This corpus is full of DELIBERATELY broken files — that is what a
        # conformance corpus is for. Asking "does pdfcer's output parse?"
        # blames pdfcer for damage it faithfully preserved from a file that
        # never parsed to begin with. The question worth asking is the
        # comparative one: **did pdfcer make it worse?**
        #
        # Measured 2026-08-07 on `PDFBOX-6040-nodeloop.pdf`, which is why
        # this is written this way: veraPDF cannot open the ORIGINAL at all
        # ("can not locate xref table"), while pdfcer's full rewrite of it
        # opens fine and only reaches the page-tree loop the file genuinely
        # contains. pdfcer RECOVERED the xref. Under the absolute reading
        # that file is a failure; under the comparative reading it is an
        # improvement, and the comparative reading is the true one.
        def scan(files: list[Path]) -> dict[str, tuple[int, str]]:
            out: dict[str, tuple[int, str]] = {}
            for start in range(0, len(files), args.batch):
                out.update(verapdf_parse_report(verapdf, files[start : start + args.batch]))
            return out

        after = scan(list(produced))
        before = scan(list(produced.values()))

        regressions: list[ParseFailure] = []
        improvements = 0
        preserved = 0
        for out_path, src in produced.items():
            tier_out = after.get(str(out_path), after.get(out_path.name, (OK, "")))[0]
            msg_out = after.get(str(out_path), (OK, ""))[1]
            tier_in = before.get(str(src.resolve()), before.get(str(src), (OK, "")))[0]
            if tier_out > tier_in:
                regressions.append(
                    ParseFailure(
                        src,
                        args.mode,
                        f"{TIER_NAME[tier_in]} -> {TIER_NAME[tier_out]}: {msg_out}",
                    )
                )
            elif tier_out < tier_in:
                improvements += 1
            elif tier_out != OK:
                preserved += 1

        # Hangs first, and loudest. A hang outranks a regression: a bad
        # file can be inspected, but a non-terminating save is an
        # unrecoverable freeze in the GUI — no error, no cancel, no save.
        for src in hangs:
            print(
                f"HANG        {src}  [--mode {args.mode}]\n"
                f"            pdfcer did not terminate within {args.timeout:g}s. "
                f"This outranks every other finding below."
            )
        for f in regressions:
            print(f"REGRESSION  {f.source}  [--mode {f.mode}]\n            {f.message}")

        print(
            f"\nverapdf-parse-gate: {len(produced)} file(s) written by pdfcer "
            f"and read back by veraPDF {verapdf.name}.\n"
            f"  {len(hangs)} hang(s)         <- pdfcer never terminated; worst class\n"
            f"  {len(regressions)} regression(s)   <- pdfcer made a readable file worse\n"
            f"  {improvements} improved (pdfcer's output parses better than its input)\n"
            f"  {preserved} pre-existing defect(s) faithfully preserved (not a failure)\n"
            f"  {refused} refused by pdfcer (refusals are not failures)"
        )
        return 1 if (regressions or hangs) else 0
    finally:
        if args.keep:
            print(f"produced files kept in {workdir}", file=sys.stderr)
        else:
            shutil.rmtree(workdir, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
