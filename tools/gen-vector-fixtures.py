#!/usr/bin/env python3
"""Generate the synthetic vector-object fixtures Pass 9a uses.

WHY THIS EXISTS
---------------
`docs/decisions/011-first-beta-scaled-measurement-dimensioning-tool.md`
Appendix A **Pass 9a** requires proving four things about the read-only
vector object/selection model (`pdfcer_core::vector`):

  1. the decomposition is **byte-inert** (the content-identity gate stays
     green — nothing here changes any output bytes);
  2. the object node geometry **matches the renderer's own walk** (the Z2
     "agree by construction" cross-check — `pdfcer-render`'s `trace_paths`
     vs `pdfcer_core::vector::decompose`, compared on these fixtures);
  3. the **filled-rectangle centerline** derivation fires on thin bars,
     is flagged, and does NOT fire on genuine rectangles (the Z3
     false-positive guard);
  4. selection (via the object model) works for path / text / image
     objects.

`docs/LEGAL.md` §5 permits only synthetic or rights-cleared PDFs in
`fixtures/`. Every file this script writes is constructed byte by byte
here, from nothing, with no PDF library behind it — the same discipline
as the sibling `tools/gen-*-fixtures.py` — so the fixtures cannot inherit
a bug (or a normalization) from the very code they test. Classic §7.5.4
cross-reference table.

WHAT IT WRITES
--------------
``fixtures/synthetic/vector/``

``paths.pdf``  (300x300)
    A single content stream exercising every path shape the model must
    decompose: an open stroked polyline (m/l/S), a filled rectangle
    (re/f), a THIN FILLED BAR line (re/f, aspect 25:1 — a centerline
    candidate), a stroked closed triangle (m/l/l/h/S), an even-odd donut
    (two re / f*), and a stroked cubic curve (c/S) plus the v and y
    implicit-control-point operators. The primary geometry cross-check
    fixture (pure paths, no fonts/images needed).

``curves.pdf``  (300x300)
    A circle drawn as four kappa=0.5523 cubic Beziers (m/c/c/c/c/h/S) —
    the shape a radius/diameter dimension fits — plus explicit v and y
    operator subpaths, so the shared v/y primitives are cross-checked
    against the renderer on real Bezier geometry.

``mixed.pdf``  (300x300)
    A stroked line + a text object (BT..ET, Helvetica) + an image XObject
    (Do on a 2x2 DeviceGray image). Proves text and image objects are
    decomposed as selectable-for-move/delete (bbox + token range only),
    not node-editable, while the stroked line still cross-checks.

``centerline.pdf``  (400x400)
    Thin filled bars at aspect 50 (horizontal, vertical, and a 45-degree
    rotated bar via `cm`), a genuine 60x60 square, and a below-threshold
    aspect-4 bar. The centerline derivation must offer a candidate for
    each thin bar (rotation-correct) and NONE for the square or the
    aspect-4 bar (Z3 false-positive guard).

``overlap.pdf``  (300x300)
    Three CONCENTRIC filled squares, painted outermost first. The
    click-through / all-hits fixture: at the centre all three are under
    the pointer, and only the innermost is reachable by a topmost-only
    hit test, so `hit_test_point_all` (and the GUI's Alt+click cycling)
    is the only way objects 1 and 0 can ever be selected there. Points
    further out give stacks of two and of one, so a hit list's LENGTH is
    a real answer about geometry rather than a constant.
"""

from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "vector"


def serialize(objects: dict[int, bytes]) -> bytes:
    """Lay out `objects` into a complete classic-xref file (§7.5.4).

    Entry format is exactly 20 bytes, written longhand so the byte count is
    visible — identical to the sibling generators so the fixtures share one
    provenance style.
    """
    out = bytearray(b"%PDF-1.7\n")
    out += b"%\xe2\xe3\xcf\xd3\n"  # §7.5.2 binary marker
    highest = max(objects)
    offsets: dict[int, int] = {}
    for num in range(1, highest + 1):
        body = objects.get(num)
        if body is None:
            continue
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode("ascii")
        out += body
        out += b"\nendobj\n"
    xref_at = len(out)
    out += f"xref\n0 {highest + 1}\n".encode("ascii")
    out += b"0000000000 65535 f \n"
    for num in range(1, highest + 1):
        if num in offsets:
            out += f"{offsets[num]:010d} 00000 n \n".encode("ascii")
        else:
            out += b"0000000000 65535 f \n"
    out += (
        f"trailer\n<< /Size {highest + 1} /Root 1 0 R >>\n"
        f"startxref\n{xref_at}\n%%EOF\n"
    ).encode("ascii")
    return bytes(out)


def stream(dict_body: str, data: bytes) -> bytes:
    """A stream object body: dictionary + /Length + raw data."""
    head = f"<< {dict_body} /Length {len(data)} >>\nstream\n".encode("ascii")
    return head + data + b"\nendstream"


def one_page(media: int, content: bytes, resources: str, extra: dict[int, bytes]) -> bytes:
    """A single-page document.

    Objects: 1 Catalog, 2 Pages, 3 Page, 4 content stream, plus `extra`.
    `resources` is the page's ``/Resources`` dictionary body (without the
    enclosing ``<< >>``).
    """
    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            f"/MediaBox [0 0 {media} {media}] >>"
        ).encode("ascii"),
        3: (
            f"<< /Type /Page /Parent 2 0 R /Contents 4 0 R "
            f"/Resources << {resources} >> >>"
        ).encode("ascii"),
        4: stream("", content),
    }
    objects.update(extra)
    return serialize(objects)


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    files: dict[str, bytes] = {}

    # -- paths.pdf: every path shape, pure paths -----------------------
    paths = b"\n".join(
        [
            b"2 w 0 0 1 RG",                       # blue stroke, width 2
            b"20 20 m 100 40 l 60 120 l S",        # open stroked polyline
            b"1 0 0 rg",                           # red fill
            b"150 40 60 40 re f",                  # filled rectangle
            b"0 1 0 rg",                           # green fill
            b"20 150 100 4 re f",                  # THIN FILLED BAR (25:1)
            b"0 0 0 RG 1 w",                       # black stroke, width 1
            b"200 200 m 260 200 l 230 260 l h S",  # stroked closed triangle
            b"40 200 80 80 re 60 220 40 40 re f*", # even-odd donut
            b"0.5 0.5 0.5 RG",
            b"20 260 m 60 300 120 220 160 280 c S", # stroked cubic
            b"200 20 m 260 20 220 70 v S",         # v operator (ctrl1=current)
            b"200 90 m 260 20 240 90 y S",         # y operator (ctrl2=endpoint)
        ]
    )
    files["paths.pdf"] = one_page(300, paths, "", {})

    # -- curves.pdf: a kappa circle + v/y beziers ----------------------
    # Circle center (150,150), r=80, kappa*r = 0.5523*80 = 44.184.
    k = 44.184
    cx, cy, r = 150.0, 150.0, 80.0
    circle = (
        f"1 w 0 0 0 RG\n"
        f"{cx + r:.3f} {cy:.3f} m\n"
        f"{cx + r:.3f} {cy + k:.3f} {cx + k:.3f} {cy + r:.3f} {cx:.3f} {cy + r:.3f} c\n"
        f"{cx - k:.3f} {cy + r:.3f} {cx - r:.3f} {cy + k:.3f} {cx - r:.3f} {cy:.3f} c\n"
        f"{cx - r:.3f} {cy - k:.3f} {cx - k:.3f} {cy - r:.3f} {cx:.3f} {cy - r:.3f} c\n"
        f"{cx + k:.3f} {cy - r:.3f} {cx + r:.3f} {cy - k:.3f} {cx + r:.3f} {cy:.3f} c\n"
        f"h S\n"
        f"20 20 m 60 20 20 60 v S\n"
        f"20 20 m 60 20 60 60 y S\n"
    ).encode("ascii")
    files["curves.pdf"] = one_page(300, circle, "", {})

    # -- mixed.pdf: line + text + image XObject ------------------------
    content = b"\n".join(
        [
            b"1 w 0 0 0 RG",
            b"20 20 m 280 20 l S",                 # stroked line (cross-checks)
            b"BT /F1 14 Tf 30 150 Td (Vector) Tj ET",  # text object
            b"q 60 0 0 40 30 250 cm /Im0 Do Q",    # image object
        ]
    )
    image = stream(
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 "
        "/ColorSpace /DeviceGray /BitsPerComponent 8",
        b"\x00\xff\xff\x00",
    )
    resources = (
        "/Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> "
        "/XObject << /Im0 5 0 R >>"
    )
    files["mixed.pdf"] = one_page(300, content, resources, {5: image})

    # -- centerline.pdf: thin bars + square + below-threshold bar ------
    centerline = b"\n".join(
        [
            b"0 0 0 rg",
            b"20 350 200 4 re f",                  # horizontal bar, aspect 50
            b"100 50 4 200 re f",                  # vertical bar, aspect 50
            b"250 300 60 60 re f",                 # square (NO candidate)
            b"250 200 40 10 re f",                 # aspect 4 (NO candidate)
            # rotated thin bar (100 x 2), 45 degrees, translated to (200,100)
            b"q 0.70711 0.70711 -0.70711 0.70711 200 100 cm 0 0 100 2 re f Q",
        ]
    )
    files["centerline.pdf"] = one_page(400, centerline, "", {})

    # -- edit.pdf: three isolated, easily-indexed objects (Pass 9c-min) -
    # A single content stream whose paint-order object indices are obvious,
    # so the 9c-min move/delete/drag-node surgery (decision 011 Appendix A)
    # has a predictable target for its CLI + render-fidelity tests:
    #   object 0 = a stroked line (m/l/S, two anchors),
    #   object 1 = a filled rectangle (re/f, an `re`-corner node-refusal case),
    #   object 2 = a stroked closed triangle (m/l/l/h/S, three anchors).
    editable = b"\n".join(
        [
            b"1 w 0 0 0 RG",
            b"50 50 m 150 150 l S",                 # object 0: stroked line
            b"1 0 0 rg",
            b"200 50 80 60 re f",                   # object 1: filled rectangle
            b"0 0 1 RG",
            b"50 200 m 150 200 l 100 280 l h S",    # object 2: stroked triangle
        ]
    )
    files["edit.pdf"] = one_page(300, editable, "", {})

    # -- overlap.pdf: three CONCENTRIC filled squares (click-through) ---
    # The fixture for `hit_test_point_all` / the GUI's Alt+click cycling.
    # Each square is painted strictly INSIDE the previous one, so:
    #
    #   * a click at the centre (150,150) is inside all three, and the
    #     front-most (index 2) is the only one a topmost-only query can
    #     ever return — objects 1 and 0 are unreachable without an
    #     all-hits query, which is the whole reason that query exists;
    #   * a click at (35,35) is inside object 0 ONLY, so the list length
    #     is a real answer about the geometry rather than a constant;
    #   * a click at (85,85) is inside objects 0 and 1 but not 2, giving a
    #     partial stack that catches an implementation that returns
    #     "everything on the page" instead of "everything under the point".
    #
    # Distinct fill colours so a rendered check (and a human looking at the
    # page) can tell which one a cycle step landed on.
    overlap = b"\n".join(
        [
            b"0.2 0.4 0.9 rg",
            b"20 20 260 260 re f",                  # object 0: outermost
            b"0.9 0.6 0.2 rg",
            b"70 70 160 160 re f",                  # object 1: middle
            b"0.2 0.7 0.3 rg",
            b"120 120 60 60 re f",                  # object 2: innermost/top
        ]
    )
    files["overlap.pdf"] = one_page(300, overlap, "", {})

    for name, data in sorted(files.items()):
        (OUT_DIR / name).write_bytes(data)
        print(f"wrote {name} ({len(data)} bytes)")
    print(f"\n{len(files)} fixtures -> {OUT_DIR}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
