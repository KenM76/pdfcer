#!/usr/bin/env python3
"""print-parity-report: does each page of a PDF print as vector, and does the
vector metafile look like the raster it stands in for?

Per page it runs the release `pdfcer` three ways:

  1. `pdfcer print PDF --vector-transparency --vector-shadings` (a DRY RUN:
     nothing is queued) for the route the print verb takes: `vector`, or
     `raster (<reason>)`.
  2. `pdfcer export-image --format emf --patch-approximations
     --vector-transparency --vector-shadings` for the metafile and its
     counters (`emf-vector:`, `emf-vector-not-flat:`, `emf-patches:`,
     `emf-shadings:`).
  3. `pdfcer export-image` (PNG, opaque white) for the raster at the same DPI.

The metafile is played through GDI's own `PlayEnhMetaFile` with the player
the pdfcer-emf tests use (`crates/pdfcer-emf/tests/gdi_player.ps1`) and
compared with the raster.

Pixel metric, sRGB 8-bit, both composited on white: per pixel, the largest
of the three channel differences. Reported: its max and mean, the percentage
of pixels over 8 and over 32, and the same over 8x8-pixel block means (which
takes edge antialiasing out: a vector edge is hard, the raster's is not).
Pixels inside a bitmap the metafile embeds are reported apart from the rest.

Each patched op's cause is classified:

  * genuinely raster: `image` and `soft_mask_image` (a mask whose group
    holds an image: a drop shadow or feather). Professional flatteners
    rasterise these too.
  * vector gap: every other cause, `patched_too_complex` and
    `patched_unrecorded`, including `soft_mask_varying` (a gradient or
    shading mask that could not be cut into constant bands), and every
    shading `emf-shadings:` declined (left as a bitmap).

A page meets the close condition when its route is `vector` and it has no
vector-gap patches.

The report (`report.md`, `report.json`, per-page diff heat maps) goes to
`--out`, which must lie outside this repository: the input is a licensed
file and nothing derived from it is committed.

Usage: print-parity-report.py PDF --out DIR [--pdfcer EXE] [--dpi 150]
       [--pages 1-6] [--printer NAME] [--no-route]
Windows only (GDI). Needs numpy and Pillow.
"""

from __future__ import annotations

import argparse
import json
import re
import struct
import subprocess
import sys
from pathlib import Path

import numpy as np
from PIL import Image

REPO = Path(__file__).resolve().parents[1]
PLAYER = REPO / "crates" / "pdfcer-emf" / "tests" / "gdi_player.ps1"
FLAGS = ["--patch-approximations", "--vector-transparency", "--vector-shadings"]
GENUINE = {"image", "soft_mask_image"}
NOT_FLAT = [
    "soft_mask_flat", "soft_mask_varying", "soft_mask_image",
    "soft_mask_unrecorded", "image",
    "shading", "spot", "spot_roster", "spot_all_group", "cmyk", "blend",
    "knockout", "geometry",
]
EMR = {114: "alphablend", 81: "stretchdibits", 76: "bitblt", 77: "stretchblt",
       118: "gradientfill"}


def fields(line: str) -> dict[str, int]:
    return {k: int(v) for k, v in re.findall(r"(\w+)=(\d+)", line)}


def stable_lines(stdout: str) -> dict[str, dict[str, int]]:
    """The `prefix: k=v ...` lines of an export-image EMF run, by prefix."""
    out = {}
    for line in stdout.splitlines():
        m = re.match(r"(emf(?:-[a-z-]+)?): (.*)", line)
        if m:
            out[m.group(1)] = fields(m.group(2))
    return out


def run(cmd: list[str]) -> subprocess.CompletedProcess:
    p = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8",
                       errors="replace")
    if p.returncode != 0:
        sys.exit(f"failed ({p.returncode}): {' '.join(cmd[:3])} ...\n{p.stderr[-2000:]}")
    return p


def routes(exe: str, pdf: str, printer: str | None) -> dict[int, str]:
    cmd = [exe, "print", pdf, "--vector-transparency", "--vector-shadings"]
    if printer:
        cmd += ["--printer", printer]
    err = run(cmd).stderr
    return {int(n): r for n, r in
            re.findall(r"^pdfcer: page (\d+): (vector|raster \([^)]*\))$", err, re.M)}


def emf_bitmaps(emf: bytes, dpi: int) -> dict:
    """Bitmap and gradient records in a metafile, with each bitmap's
    destination rectangle in page-raster pixels (the metafile's logical
    unit is 0.01 mm)."""
    counts = {name: 0 for name in EMR.values()}
    rects = []
    px = dpi / 2540.0
    off = 0
    while off + 8 <= len(emf):
        kind, size = struct.unpack_from("<II", emf, off)
        if size < 8:
            break
        name = EMR.get(kind)
        if name:
            counts[name] += 1
            if kind in (114, 81, 76, 77):
                x, y = struct.unpack_from("<ii", emf, off + 24)
                # STRETCHDIBITS keeps its destination extent after the source.
                w, h = struct.unpack_from("<ii", emf, off + (72 if kind == 81 else 32))
                rects.append(tuple(round(v * px) for v in (x, y, w, h)))
        if kind == 14:  # EMR_EOF
            break
        off += size
    return {"counts": counts, "rects": rects}


def play(emf_path: Path, w: int, h: int, raw: Path) -> np.ndarray:
    run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
         str(PLAYER), "-Path", str(emf_path), "-W", str(w), "-H", str(h),
         "-Out", str(raw)])
    bgra = np.frombuffer(raw.read_bytes(), dtype=np.uint8).reshape(h, w, 4)
    return bgra[:, :, 2::-1].astype(np.int16)


def mask_of(rects: list, w: int, h: int) -> np.ndarray:
    m = np.zeros((h, w), dtype=bool)
    for x, y, rw, rh in rects:
        x0, y0 = max(x, 0), max(y, 0)
        m[y0:max(y + rh, 0), x0:max(x + rw, 0)] = True
    return m


def stats(delta: np.ndarray) -> dict:
    if delta.size == 0:
        return {"pixels": 0}
    return {
        "pixels": int(delta.size),
        "max": int(delta.max()),
        "mean": round(float(delta.mean()), 3),
        "pct_over_8": round(100.0 * float((delta > 8).mean()), 3),
        "pct_over_32": round(100.0 * float((delta > 32).mean()), 3),
    }


def block_stats(a: np.ndarray, b: np.ndarray, n: int = 8) -> dict:
    h, w = (a.shape[0] // n) * n, (a.shape[1] // n) * n
    def means(x):
        return x[:h, :w].reshape(h // n, n, w // n, n, 3).mean(axis=(1, 3))
    d = np.abs(means(a) - means(b)).max(axis=2)
    y, x = np.unravel_index(int(d.argmax()), d.shape)
    return {
        "block": n,
        "max": round(float(d.max()), 2),
        "pct_over_8": round(100.0 * float((d > 8).mean()), 3),
        "pct_over_32": round(100.0 * float((d > 32).mean()), 3),
        "worst_at_px": [int(x * n), int(y * n)],
    }


def compare(played: np.ndarray, raster: np.ndarray, bitmaps: np.ndarray,
            heat: Path) -> dict:
    delta = np.abs(played - raster).max(axis=2)
    Image.fromarray(np.clip(delta * 4, 0, 255).astype(np.uint8)).save(heat)
    return {
        "all": stats(delta),
        "inside_bitmaps": stats(delta[bitmaps]),
        "vector_area": stats(delta[~bitmaps]),
        "blocks": block_stats(played, raster),
        "bitmap_area_pct": round(100.0 * float(bitmaps.mean()), 3),
    }


def classify(lines: dict) -> dict:
    v = lines.get("emf-vector", {})
    nf = lines.get("emf-vector-not-flat", {})
    genuine = {k: nf.get(k, 0) for k in NOT_FLAT if k in GENUINE and nf.get(k)}
    gaps = {k: nf.get(k, 0) for k in NOT_FLAT if k not in GENUINE and nf.get(k)}
    for k in ("patched_too_complex", "patched_unrecorded"):
        if v.get(k):
            gaps[k] = v[k]
    sh = lines.get("emf-shadings", {})
    for k in ("declined_translucent", "declined_too_many", "declined_other"):
        if sh.get(k):
            gaps[f"shading_{k}"] = sh[k]
    return {"genuinely_raster": genuine, "vector_gap": gaps,
            "beneath": nf.get("beneath", 0)}


def export_page(exe: str, pdf: str, page: int, dpi: int, out: Path,
                extra: list[str]) -> dict:
    emf_path, png_path = out / f"p{page}.emf", out / f"p{page}.png"
    common = [exe, "export-image", pdf, "--pages", str(page), "--dpi", str(dpi)]
    lines = stable_lines(run(common + ["--format", "emf", *FLAGS, *extra, "-o",
                                       str(emf_path)]).stdout)
    run(common + ["-o", str(png_path)])
    raster = np.asarray(Image.open(png_path).convert("RGB")).astype(np.int16)
    h, w = raster.shape[:2]
    played = play(emf_path, w, h, out / f"p{page}.bgra")
    records = emf_bitmaps(emf_path.read_bytes(), dpi)
    diff = compare(played, raster, mask_of(records["rects"], w, h),
                   out / f"p{page}_diff.png")
    return {"page": page, "size_px": [w, h], "counters": lines,
            "emf_records": records, "causes": classify(lines), "diff": diff}


def page_list(spec: str, count: int) -> list[int]:
    if spec == "all":
        return list(range(1, count + 1))
    pages = []
    for part in spec.split(","):
        a, _, b = part.partition("-")
        pages += range(int(a), int(b or a) + 1)
    return pages


def verdict(page: dict) -> str:
    gaps = sum(page["causes"]["vector_gap"].values())
    route = page.get("route", "unknown")
    if route != "vector":
        return f"NOT MET: route {route}"
    return "met" if gaps == 0 else f"NOT MET: {gaps} vector-gap patch(es)"


def markdown(dpi: int, pages: list[dict]) -> str:
    rows = [
        f"# Print parity report at {dpi} dpi", "",
        "The input file is not named here: its name may be licensed.", "",
        "Pixel delta = max channel difference, sRGB 8-bit, on white.", "",
        "| page | route | not_flat | unrecorded | bitmaps | bitmap area % "
        "| max | mean | %>8 | %>32 | vector-area %>32 | block max | block %>8 "
        "| genuinely raster | vector gap | verdict |",
        "|" + "---|" * 16,
    ]
    for p in pages:
        v = p["counters"].get("emf-vector", {})
        d = p["diff"]
        c = p["causes"]
        rows.append("| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |".format(
            p["page"], p.get("route", "-"), v.get("patched_not_flat", 0),
            v.get("patched_unrecorded", 0), len(p["emf_records"]["rects"]),
            d["bitmap_area_pct"], d["all"]["max"], d["all"]["mean"],
            d["all"]["pct_over_8"], d["all"]["pct_over_32"],
            d["vector_area"].get("pct_over_32", "-"), d["blocks"]["max"],
            d["blocks"]["pct_over_8"], cause_text(c["genuinely_raster"]),
            cause_text(c["vector_gap"]), verdict(p)))
    rows += ["", "## Raster regions (EMF bitmap records, x y w h in px)", ""]
    for p in pages:
        rects = "; ".join(f"{x} {y} {w} {h}" for x, y, w, h in p["emf_records"]["rects"])
        rows.append(f"- page {p['page']}: {rects or 'none'}")
    return "\n".join(rows) + "\n"


def cause_text(causes: dict) -> str:
    return ", ".join(f"{k} {n}" for k, n in causes.items()) or "-"


def parse_args() -> argparse.Namespace:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("pdf")
    ap.add_argument("--out", required=True, type=Path,
                    help="report directory, outside this repository")
    ap.add_argument("--pdfcer", default=str(REPO / "target" / "release" / "pdfcer.exe"))
    ap.add_argument("--dpi", type=int, default=150)
    ap.add_argument("--pages", default="all")
    ap.add_argument("--printer", help="printer for the route dry run")
    ap.add_argument("--no-route", action="store_true",
                    help="skip the print dry run (needs --pages)")
    ap.add_argument("--emf-flag", action="append", default=[], metavar="FLAG",
                    help="another export-image EMF flag (repeatable), e.g. "
                         "--emf-flag=--tight-mask-patches")
    return ap.parse_args()


def main() -> int:
    a = parse_args()
    out = a.out.resolve()
    if out == REPO or REPO in out.parents:
        sys.exit("--out must lie outside the repository: the report is never committed")
    out.mkdir(parents=True, exist_ok=True)
    route = {} if a.no_route else routes(a.pdfcer, a.pdf, a.printer)
    if a.pages == "all" and not route:
        sys.exit("--pages is required with --no-route")
    pages = []
    for n in page_list(a.pages, max(route, default=0)):
        page = export_page(a.pdfcer, a.pdf, n, a.dpi, out, a.emf_flag)
        page["route"] = route.get(n, "not measured")
        pages.append(page)
        print(f"page {n}: {verdict(page)}", file=sys.stderr)
    (out / "report.json").write_bytes(json.dumps(pages, indent=1).encode())
    report = markdown(a.dpi, pages)
    (out / "report.md").write_bytes(report.encode())
    sys.stdout.write(report)
    met = all(verdict(p) == "met" for p in pages)
    print(f"close condition: {'MET' if met else 'NOT MET'}", file=sys.stderr)
    return 0 if met else 1


if __name__ == "__main__":
    sys.exit(main())
