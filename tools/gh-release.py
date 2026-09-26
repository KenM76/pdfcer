"""Publish a GitHub release so a transient upload failure cannot lose it.

`gh release create <tag> <assets...>` is not atomic: an HTTP 5xx while
uploading an asset makes gh delete the release it just created, and
`gh release view` then reports "release not found" (v0.55.0 cost three
attempts). This script makes creation and upload separate acts:

1. create the release with NO assets, unless it already exists;
2. `gh release upload --clobber`, retried with backoff (idempotent);
3. read `gh release view --json assets` and require every named asset to
   be `uploaded` at its local byte size.

Exit 0 only when step 3 holds. Re-running after any failure is safe.
Run `tools/verify-release.py <tag>` afterwards for the release's content.

usage: python tools/gh-release.py <tag> --title TITLE --notes-file FILE
                                  [--retries N] [--dry-run] ASSET...
"""

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path


def gh(args: list[str], dry: bool) -> subprocess.CompletedProcess:
    print("+ gh " + " ".join(args), flush=True)
    if dry:
        return subprocess.CompletedProcess(args, 0, "", "")
    return subprocess.run(["gh", *args], capture_output=True, text=True)


def release_exists(tag: str) -> bool:
    r = subprocess.run(["gh", "release", "view", tag, "--json", "tagName"],
                       capture_output=True, text=True)
    return r.returncode == 0


def verify(tag: str, assets: list[Path]) -> list[str]:
    r = subprocess.run(["gh", "release", "view", tag, "--json", "assets"],
                       capture_output=True, text=True)
    if r.returncode != 0:
        return [f"`gh release view {tag}` failed: {r.stderr.strip()}"]
    remote = {a["name"]: a for a in json.loads(r.stdout)["assets"]}
    bad = []
    for p in assets:
        a = remote.get(p.name)
        if a is None:
            bad.append(f"{p.name}: not on the release")
        elif a.get("state") != "uploaded":
            bad.append(f"{p.name}: state {a.get('state')}")
        elif a.get("size") != p.stat().st_size:
            bad.append(f"{p.name}: {a.get('size')} bytes remote, {p.stat().st_size} local")
    return bad


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("tag")
    ap.add_argument("--title", required=True)
    ap.add_argument("--notes-file", required=True, type=Path)
    ap.add_argument("--retries", type=int, default=5)
    ap.add_argument("--dry-run", action="store_true",
                    help="print the gh commands without running them")
    ap.add_argument("assets", nargs="+", type=Path)
    a = ap.parse_args()

    missing = [str(p) for p in [a.notes_file, *a.assets] if not p.is_file()]
    if missing:
        print("FAIL no such file: " + ", ".join(missing))
        return 1

    if not a.dry_run and release_exists(a.tag):
        print(f"release {a.tag} exists; uploading only")
    else:
        r = gh(["release", "create", a.tag, "--verify-tag", "--title", a.title,
                "--notes-file", str(a.notes_file)], a.dry_run)
        if r.returncode != 0:
            print(f"FAIL create: {r.stderr.strip()}")
            return 1

    for attempt in range(1, a.retries + 1):
        r = gh(["release", "upload", a.tag, "--clobber", *map(str, a.assets)], a.dry_run)
        if r.returncode == 0:
            break
        wait = 5 * 2 ** (attempt - 1)
        print(f"upload attempt {attempt}/{a.retries} failed: {r.stderr.strip()}")
        if attempt < a.retries:
            print(f"retrying in {wait}s", flush=True)
            time.sleep(wait)
    else:
        print("FAIL upload: retries exhausted; the release is left standing, re-run to resume")
        return 1

    if a.dry_run:
        print("dry run: verification skipped")
        return 0
    bad = verify(a.tag, a.assets)
    for b in bad:
        print("FAIL", b)
    if bad:
        return 1
    print(f"PASS {a.tag}: {len(a.assets)} asset(s) uploaded at their local sizes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
