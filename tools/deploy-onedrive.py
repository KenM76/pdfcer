#!/usr/bin/env python3
"""Publish the CLI to OneDrive, alternating `pdfcer1` / `pdfcer2`.

Operator instruction, 2026-08-29, verbatim:

    "can you always put a new version on onedrive? cycle between folders
     pdfcer1 and pdfcer2 when you make new versions so there is always a
     previous version available. Just need the CLI tool available."

Three requirements, and the third is the one with teeth:

  1. **every new version** goes to OneDrive -- so this runs from the release
     procedure, not from somebody remembering;
  2. **the CLI only** -- no `pdfce-gui.exe`;
  3. **there is ALWAYS a previous version available** -- which is a constraint
     on what may be OVERWRITTEN, not merely on where to write.

WHY THE ALTERNATION IS DERIVED, NEVER REMEMBERED
================================================

The obvious implementation keeps a "last used" marker somewhere and flips it.
That breaks the moment anything happens outside this script -- a manual copy, a
deploy that half-finished, a folder restored from OneDrive's own version
history -- and it breaks SILENTLY, by overwriting the copy the operator was
relying on.

So the target is computed from the folders themselves: **write into whichever
side is older.** Each folder carries a `VERSION.txt` with an ISO timestamp,
version and commit. The rule is self-correcting -- if a deploy is skipped, the
next one still overwrites the older side -- and it needs no state of its own.

An unreadable or missing `VERSION.txt` counts as **infinitely old**, so a fresh
or damaged folder is chosen first. That is the safe direction: the worst case
is overwriting something already broken.

★ THE GUARD THAT MATTERS MORE THAN THE ALTERNATION

Running this twice for the SAME version would put that version in both folders
and **destroy the previous version the whole scheme exists to preserve**. The
folders would look healthy -- two populated directories, recent timestamps --
while the property the operator asked for was gone.

So a deploy is REFUSED when the version being published is already present in
the *other* folder, unless `--force` is passed. Re-releasing the same version
number after a rebuild is legitimate, which is why the escape hatch exists;
doing it by accident is not, which is why it is not the default.

WHAT SHIPS
==========

`pdfcer.exe`, the `models/ocrs` folder, `LICENSE`,
`THIRD_PARTY_LICENSES.md`, `README.md`, and a generated `VERSION.txt`.

The models are ~12 MB and are included deliberately. Without them the CLI
refuses OCR by name and explains itself -- it does not crash -- but "just the
CLI tool" means *not the GUI*, not a CLI that cannot do a job it advertises.

USAGE
=====

    python tools/deploy-onedrive.py                 # from the newest build
    python tools/deploy-onedrive.py --build DIR     # from a specific one
    python tools/deploy-onedrive.py --dry-run
    python tools/deploy-onedrive.py --force         # re-deploy a version

EXIT CODES
==========

0 deployed (or a clean dry run) -- 1 refused (same version already opposite,
or no build found) -- 2 the environment is wrong (no OneDrive, no build root).
"""

from __future__ import annotations

import argparse
import datetime as _dt
import os
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
BUILD_ROOT = Path(r"D:\builds")
SLOTS = ("pdfcer1", "pdfcer2")

# Relative to a portable build folder. Anything absent is skipped with a note
# rather than aborting -- a build without OCR models is still a usable CLI.
PAYLOAD = ("pdfcer.exe", "models", "LICENSE", "THIRD_PARTY_LICENSES.md", "README.md")


def onedrive_root() -> Path | None:
    for var in ("OneDrive", "OneDriveConsumer", "OneDriveCommercial"):
        v = os.environ.get(var)
        if v and Path(v).is_dir():
            return Path(v)
    fallback = Path.home() / "OneDrive"
    return fallback if fallback.is_dir() else None


def newest_build() -> Path | None:
    if not BUILD_ROOT.is_dir():
        return None
    builds = [p for p in BUILD_ROOT.glob("pdfcer-*") if (p / "pdfcer.exe").is_file()]
    return max(builds, key=lambda p: p.stat().st_mtime) if builds else None


def read_version(slot: Path) -> dict[str, str]:
    """Parse a slot's `VERSION.txt`. Missing or unreadable == infinitely old."""
    f = slot / "VERSION.txt"
    if not f.is_file():
        return {}
    out: dict[str, str] = {}
    try:
        for line in f.read_text(encoding="utf-8").splitlines():
            if ":" in line:
                k, _, v = line.partition(":")
                out[k.strip().lower()] = v.strip()
    except OSError:
        return {}
    return out


def deployed_at(slot: Path) -> str:
    """Sort key. Empty string sorts first, i.e. oldest, which is intended."""
    return read_version(slot).get("deployed", "")


def cli_version(build: Path) -> tuple[str, str]:
    """`(version, commit)` straight from the binary that is about to ship.

    Read from the EXE rather than from `Cargo.toml`, because what gets copied
    is the exe: a stale build directory would otherwise be labelled with the
    working tree's version and the label would be a lie.
    """
    exe = build / "pdfcer.exe"
    try:
        out = subprocess.run([str(exe), "--version"], capture_output=True, text=True,
                             timeout=120).stdout
    except (OSError, subprocess.SubprocessError):
        return ("unknown", "unknown")
    version, commit = "unknown", "unknown"
    for line in out.splitlines():
        s = line.strip()
        if s.startswith("pdfcer ") and version == "unknown":
            version = s.split()[1]
        if s.lower().startswith("revision:"):
            commit = s.split(":", 1)[1].strip()
    return (version, commit)


def _empty_slot(target: Path, *, attempts: int = 8, delay: float = 0.5) -> None:
    """Empty a OneDrive slot **in place**, without removing the slot itself.

    WHY NOT ``shutil.rmtree``
    =========================

    Measured 2026-08-30 while cutting ``v0.17.0``. ``shutil.rmtree`` on a slot
    fails with ``PermissionError: [WinError 5] Access is denied`` -- and the
    path it names is a **directory**, never a file. Retrying for six seconds
    did not help; the failure simply walked outward as the tree collapsed
    (``models/ocrs``, then ``models``, then the slot root).

    Then the discriminating test: in that same folder, deleting a file
    succeeded, and ``rmdir`` of an empty subdirectory succeeded. **Only the
    slot ROOT refuses**, because it is a top-level synced folder and the sync
    engine holds it open for as long as OneDrive is running. It is not a
    transient lock and no amount of retrying clears it.

    So the removal was never necessary. ``rmtree`` insists on unlinking the
    root it was given; the requirement is only that **no stale file survives**
    a payload that shrank between versions. Emptying the directory satisfies
    that exactly, and asks the filesystem for nothing it has refused.

    ★ WHAT THE FAILED ATTEMPTS LEFT BEHIND, WHICH IS THE REAL HAZARD

    The first failure left the slot INCONSISTENT -- ``LICENSE`` gone,
    ``models/ocrs`` emptied, but the previous ``pdfcer.exe`` and its
    ``VERSION.txt`` still in place. That folder still looks populated and its
    ``VERSION.txt`` still names a version an operator would trust, while the
    payload beside it is no longer that version's payload.

    The entire point of two slots is that the other one is a **working**
    fallback. A half-emptied slot passes a glance -- the same quiet-failure
    shape the double-deploy guard exists for, arriving through the filesystem
    instead of through the alternation. Hence: raise rather than continue, and
    say plainly that the slot is now untrustworthy.

    Empty subdirectories left behind are harmless and are tolerated: the
    payload copy recreates or overwrites them, and refusing to proceed over a
    directory with nothing in it would fail a deploy that is actually fine.
    """
    import time

    def _unlink(path: Path) -> None:
        last: OSError | None = None
        for _ in range(attempts):
            try:
                path.unlink()
                return
            except FileNotFoundError:
                return
            except PermissionError as exc:  # noqa: PERF203 - retry is the point
                last = exc
                time.sleep(delay)
        raise RuntimeError(
            f"could not delete {path} after {attempts} attempts. The slot "
            f"{target} is now PARTIALLY EMPTIED and must not be relied on as "
            f"the previous version until this script completes. Close anything "
            f"using it (a running pdfcer.exe cannot be replaced on Windows) "
            f"and re-run."
        )

    # Bottom-up so a directory is only attempted after its contents are gone.
    for root, dirs, files in os.walk(target, topdown=False):
        for name in files:
            _unlink(Path(root) / name)
        for name in dirs:
            try:
                (Path(root) / name).rmdir()
            except OSError:
                # Tolerated -- see the docstring. An empty directory is not a
                # stale file and cannot be mistaken for part of this build.
                pass

    # The contract this function actually owes: no FILE survives. Directories
    # are allowed to; asserting on them would reintroduce the failure this
    # exists to avoid.
    leftover = [
        str(Path(r) / f) for r, _d, fs in os.walk(target) for f in fs
    ]
    if leftover:
        raise RuntimeError(
            f"{len(leftover)} file(s) survived emptying {target}, e.g. "
            f"{leftover[0]} -- refusing to write a payload over an unknown "
            f"mixture of two versions."
        )


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--build", help="portable build folder (default: newest in D:\\builds)")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--force", action="store_true",
                    help="deploy even if this version already occupies the other slot")
    args = ap.parse_args()

    root = onedrive_root()
    if root is None:
        print("deploy-onedrive: no OneDrive folder found", file=sys.stderr)
        return 2

    build = Path(args.build) if args.build else newest_build()
    if build is None or not (build / "pdfcer.exe").is_file():
        print(f"deploy-onedrive: no portable build with a CLI in {BUILD_ROOT}", file=sys.stderr)
        return 1

    version, commit = cli_version(build)
    slots = [root / s for s in SLOTS]

    print(f"deploy-onedrive: publishing {version} ({commit})")
    print(f"  from  {build}")
    for s in slots:
        v = read_version(s)
        state = (f"{v.get('version', '?')} deployed {v.get('deployed', '?')}"
                 if v else ("empty" if not s.is_dir() else "no VERSION.txt"))
        print(f"  slot  {s.name:8} {state}")

    # Oldest wins. `deployed_at` returns "" for missing/unreadable, which sorts
    # first, so a fresh or damaged slot is filled before a healthy one.
    target = min(slots, key=deployed_at)
    other = next(s for s in slots if s != target)

    # ★ The guard. Same version in both slots destroys the previous version.
    other_version = read_version(other).get("version")
    if other_version == version and version != "unknown" and not args.force:
        print(
            f"\ndeploy-onedrive: REFUSED -- {other.name} already holds {version}.\n"
            "  Deploying it again would put the same version in BOTH slots and\n"
            "  destroy the previous version this scheme exists to preserve.\n"
            "  Pass --force if you are deliberately re-deploying after a rebuild.",
            file=sys.stderr,
        )
        return 1

    print(f"\n  -> writing {target.name} (the older slot); {other.name} keeps "
          f"{other_version or 'its contents'} as the previous version")

    if args.dry_run:
        print("deploy-onedrive: dry run, nothing written")
        return 0

    # Replace the slot's contents. Removed first so a payload that shrank
    # between versions cannot leave a stale file behind that the operator would
    # reasonably read as part of this build.
    if target.is_dir():
        _empty_slot(target)
    target.mkdir(parents=True, exist_ok=True)

    copied, missing = [], []
    for name in PAYLOAD:
        src = build / name
        if not src.exists():
            missing.append(name)
            continue
        dst = target / name
        if src.is_dir():
            # `dirs_exist_ok` is REQUIRED, not defensive. `_empty_slot`
            # deliberately leaves empty subdirectories behind and documents
            # them as harmless "because the payload copy recreates or
            # overwrites them" -- and that sentence was never true of
            # `copytree`, which refuses an existing destination outright.
            #
            # Measured cutting v0.19.0: the deploy died with
            # `FileExistsError: ... \pdfcer2\models`, AFTER `_empty_slot` had
            # already run. That is precisely the half-emptied slot the helper's
            # own docs call "untrustworthy" -- reached through the copy step
            # rather than the removal step, so its raise-rather-than-continue
            # guard never fired.
            #
            # ⇒ A comment asserting what a later line does is a claim about
            # that line, and nothing checks it. The payload gained `models/`
            # after this code was written, so the first directory entry in
            # PAYLOAD is what exposed it.
            shutil.copytree(src, dst, dirs_exist_ok=True)
        else:
            shutil.copy2(src, dst)
        copied.append(name)

    stamp = _dt.datetime.now(_dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    (target / "VERSION.txt").write_text(
        "\n".join([
            f"version:  {version}",
            f"commit:   {commit}",
            f"deployed: {stamp}",
            f"source:   {build}",
            f"slot:     {target.name}",
            "",
            "The pdfcer command-line tool. The GUI is deliberately not here --",
            "see this folder's sibling for the previous version.",
            "",
            "OCR needs the models/ocrs folder beside the exe; it is included.",
            "",
        ]),
        encoding="utf-8",
    )

    total = sum(f.stat().st_size for f in target.rglob("*") if f.is_file())
    print(f"deploy-onedrive: wrote {len(copied)} item(s), {total:,} bytes to {target}")
    if missing:
        print(f"  note: not in this build, skipped: {', '.join(missing)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
