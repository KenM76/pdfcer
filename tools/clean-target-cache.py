#!/usr/bin/env python3
"""Delete build outputs in `target/` that a newer build has superseded.

Every version bump gives every workspace crate (and everything depending on
one) a new metadata hash, so cargo writes a fresh copy beside the old one and
never removes the old. At several releases a day that grew `target/debug/deps`
past 60 GB. This script removes a crate's output only when BOTH hold:

* it was last written before the cutoff, which is the commit time of the
  `--keep-versions`-th newest `chore: bump version` commit (default 2: the
  current and the previous release's outputs are kept); and
* the same crate has an output written after the cutoff, so it has been
  superseded. A dependency nothing has rebuilt since is still current and is
  kept, however old.

Applies to every `target[/<triple>]/<profile>/` under the repo and under
`.claude/worktrees/*/`: files in `deps/` grouped by `<name>-<16 hex>`, the
matching `.fingerprint/` and `build/` directories, and `incremental/` crate
directories. An agent worktree's whole `target/` goes when nothing in it
was written after the cutoff. Deleting something cargo still wants is safe:
it rebuilds it.

Skips a profile whose `.cargo-lock` a running build holds (a build
mid-write would lose outputs it is producing). Run automatically at the end of
`package-portable.py`; by hand: `python tools/clean-target-cache.py [--dry-run]`.
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
HASHED = re.compile(r"^(?:lib)?(?P<stem>.+?)-(?P<hash>[0-9a-f]{16})(?:\..*)?$")
INCREMENTAL = re.compile(r"^(?P<stem>.+)-(?P<hash>[0-9a-z]+)$")
PROFILES = ("debug", "release")


def cutoff_time(keep: int) -> float | None:
    """Commit time of the `keep`-th newest version bump; None if too few."""
    out = subprocess.run(
        ["git", "log", "--format=%ct", f"-{keep}", "--grep=^chore: bump version"],
        cwd=REPO, capture_output=True, text=True, check=True,
    ).stdout.split()
    return float(out[-1]) if len(out) >= keep else None


def build_running(profile: Path) -> bool:
    """True while a cargo build holds this profile's `.cargo-lock` (cargo
    locks it exclusively for the whole build)."""
    lock = profile / ".cargo-lock"
    if not lock.exists():
        return False
    try:
        with open(lock, "r+b") as fh:
            if sys.platform == "win32":
                import msvcrt

                msvcrt.locking(fh.fileno(), msvcrt.LK_NBLCK, 1)
                msvcrt.locking(fh.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                import fcntl

                fcntl.flock(fh, fcntl.LOCK_EX | fcntl.LOCK_NB)
                fcntl.flock(fh, fcntl.LOCK_UN)
    except OSError:
        return True
    return False


def profile_dirs() -> list[Path]:
    """Every `<target>[/<triple>]/<profile>` directory to sweep."""
    targets = [REPO / "target"]
    wt = REPO / ".claude" / "worktrees"
    if wt.is_dir():
        targets += [w / "target" for w in wt.iterdir()]
    found = []
    for t in (t for t in targets if t.is_dir()):
        for p in PROFILES:
            found += [d for d in (t / p, *(s / p for s in t.iterdir() if s.is_dir())) if d.is_dir()]
    return found


def newest_mtime(paths: list[Path]) -> float:
    return max(p.stat().st_mtime for p in paths)


def superseded(groups: dict[str, dict[str, list[Path]]], cutoff: float) -> dict[str, list[Path]]:
    """Per hash: the paths to delete. `groups` is stem -> hash -> paths."""
    doomed = {}
    for by_hash in groups.values():
        times = {h: newest_mtime(ps) for h, ps in by_hash.items()}
        if not any(t >= cutoff for t in times.values()):
            continue
        doomed.update({h: by_hash[h] for h, t in times.items() if t < cutoff})
    return doomed


def group(directory: Path, pattern: re.Pattern[str]) -> dict[str, dict[str, list[Path]]]:
    out: dict[str, dict[str, list[Path]]] = defaultdict(lambda: defaultdict(list))
    if directory.is_dir():
        for p in directory.iterdir():
            m = pattern.match(p.name)
            if m:
                out[m["stem"]][m["hash"]].append(p)
    return out


def size(p: Path) -> int:
    if p.is_file():
        return p.stat().st_size
    return sum(f.stat().st_size for f in p.rglob("*") if f.is_file())


def remove(p: Path, dry: bool) -> int:
    n = size(p)
    if not dry:
        if p.is_dir():
            shutil.rmtree(p, ignore_errors=True)
        else:
            p.unlink(missing_ok=True)
    return n


def sweep(profile: Path, cutoff: float, dry: bool) -> tuple[int, int]:
    """Deletes superseded outputs in one profile dir; returns (items, bytes)."""
    doomed = superseded(group(profile / "deps", HASHED), cutoff)
    items = [p for ps in doomed.values() for p in ps]
    for sub in (".fingerprint", "build"):
        d = profile / sub
        if d.is_dir():
            items += [p for p in d.iterdir() if (m := HASHED.match(p.name)) and m["hash"] in doomed]
    for ps in superseded(group(profile / "incremental", INCREMENTAL), cutoff).values():
        items += ps
    return len(items), sum(remove(p, dry) for p in items)


def stale_worktree_targets(cutoff: float) -> list[Path]:
    """Agent-worktree `target/` dirs whose top-level entries all predate the
    cutoff: nothing has built there since, so the whole cache is dead."""
    wt = REPO / ".claude" / "worktrees"
    if not wt.is_dir():
        return []
    out = []
    for t in (w / "target" for w in wt.iterdir()):
        if t.is_dir() and all(p.stat().st_mtime < cutoff for p in t.rglob("*") if p.is_file()):
            out.append(t)
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--keep-versions", type=int, default=2,
                    help="releases whose outputs are kept (default 2: current and previous)")
    ap.add_argument("--dry-run", action="store_true", help="report, delete nothing")
    a = ap.parse_args()
    cutoff = cutoff_time(max(a.keep_versions, 1))
    if cutoff is None:
        print("clean-target-cache: skipped -- fewer version bumps than --keep-versions")
        return 0
    total_items = total_bytes = 0
    for t in stale_worktree_targets(cutoff):
        freed = remove(t, a.dry_run)
        total_items += 1
        total_bytes += freed
        print(f"  {t.relative_to(REPO)}: whole stale cache, {freed / 2**30:.1f} GiB")
    for profile in profile_dirs():
        if build_running(profile):
            print(f"  {profile.relative_to(REPO)}: skipped -- a cargo build holds it")
            continue
        items, freed = sweep(profile, cutoff, a.dry_run)
        total_items += items
        total_bytes += freed
        if items:
            print(f"  {profile.relative_to(REPO)}: {items} item(s), {freed / 2**30:.1f} GiB")
    verb = "would free" if a.dry_run else "freed"
    print(f"clean-target-cache: {verb} {total_bytes / 2**30:.1f} GiB in {total_items} item(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
