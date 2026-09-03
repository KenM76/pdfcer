#!/usr/bin/env python3
"""Generate the synthetic annotation fixtures Pass 6.0 uses.

WHY THIS EXISTS
---------------
`docs/decisions/008` Pass 6.0 acceptance criterion 4: the §12.5.5
appearance-placement algorithm is a **silent-wrongness** class — a wrong
composition of `/BBox`, `/Matrix` and `/Rect` renders beautifully in the
wrong place, and pdfcer's self-comparison oracle cannot catch it. So the
placement is pinned from BOTH directions with fixtures whose geometry is
known exactly, plus a pdfium raster differential on the corpus subset.

`docs/LEGAL.md` §5 permits only synthetic or rights-cleared PDFs in
`fixtures/`. Every file this script writes is constructed byte by byte
here, from nothing, so its provenance is this file and nothing else. It
follows the existing `tools/gen-*-fixtures.py` pattern: a deliberately
minimal writer with no PDF library behind it, so the fixtures cannot
inherit a bug (or a normalization) from the code they are meant to test.
It emits a **classic** cross-reference table (ISO 32000-1 §7.5.4).

WHAT IT WRITES
--------------
``fixtures/synthetic/annot/``

Each file is a single 200x200 page. Every appearance stream is a form
XObject that fills its ENTIRE ``/BBox`` solid black, so the painted
region on the page is exactly the image of the ``/BBox`` under the
§12.5.5 placement — which is what makes the placement assertable from the
raster alone.

``placement-identity.pdf``
    ``/BBox [0 0 20 20]``, identity ``/Matrix``, ``/Rect [40 30 60 50]``.
    The fill lands one-to-one in the 20x20 rect.

``placement-nonorigin-bbox.pdf``
    ``/BBox [100 100 120 120]`` (far from the origin), ``/Rect
    [0 0 40 40]``. Step b must TRANSLATE the transformed box onto the
    rect; a reader that ignored the box origin would misplace it.

``placement-bbox-larger.pdf``
    ``/BBox [0 0 80 80]`` into ``/Rect [10 10 30 30]``: scaled DOWN.

``placement-bbox-smaller.pdf``
    ``/BBox [0 0 5 5]`` into ``/Rect [0 0 200 200]``: scaled UP to fill
    the page.

``placement-matrix-scale.pdf``
    ``/BBox [0 0 10 10]`` with ``/Matrix [2 0 0 2 0 0]`` into ``/Rect
    [0 0 40 40]``. The Matrix is applied ONCE (the fit absorbs its
    scale); a double-apply misplaces or clips the fill.

``placement-matrix-rotate.pdf``
    ``/BBox [0 0 20 20]`` with a 90-degree ``/Matrix`` into ``/Rect
    [20 20 60 60]``. Step a takes the axis-aligned bounds of the rotated
    box; the fill must stay inside the rect.

``placement-inverted-rect.pdf``
    ``/Rect [60 50 40 30]`` (corners reversed, §7.9.5): same target box
    as ``[40 30 60 50]`` — normalized, no divide-by-negative.

``placement-degenerate-bbox.pdf``
    ``/BBox [10 10 10 90]`` (zero width): the transformed box is
    degenerate, the step-b fit matrix is singular, and pdfcer must paint
    NOTHING and name the refusal — never a divide-by-zero (risk X2).

``flags-hidden.pdf``
    A stamp covering the whole page with ``/F 2`` (Hidden). Painting it
    would be unmistakable; it must stay invisible AND be counted (R50).

``flags-noview.pdf``
    The same, ``/F 32`` (NoView): screen-suppressed on this path.

``popup-not-painted.pdf``
    A ``/Popup`` carrying a (malformed) full-page ``/AP``: must never be
    painted as page content (§12.5.6.14, risk X4).

``no-ap-circle.pdf``
    A ``/Circle`` with ``/IC`` (interior colour) and NO ``/AP``: R43
    named-not-painted — pdfcer synthesises nothing from ``/IC``.

``as-state-checkbox.pdf``
    A ``/Widget`` whose ``/AP /N`` is an On/Off subdictionary, ``/AS
    /On``: the On appearance is selected and painted.

``as-missing-state.pdf``
    The same subdictionary but ``/AS /Maybe`` (absent state): display
    nothing (§12.5.5 NOTE 3), counted, never guessed.

``ap-resources-own-font.pdf``
    The page's ``/Resources`` and the appearance's ``/Resources`` both
    define ``/F1`` as DIFFERENT fonts (Helvetica vs Times-Roman). The
    appearance's text must resolve ``/F1`` against ITS OWN resources
    (risk X8) — pinned via the substituted-font diagnostic.

``demo-annotated.pdf``
    A single page combining a painted stamp, a hidden stamp, a no-/AP
    circle, and a popup — the file the Pass 6.0 completion demonstration
    renders (with and without ``--no-annotations``) and lists.
"""

from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "annot"

PAGE = 200


def serialize(objects: dict[int, bytes]) -> bytes:
    """Lay out `objects` into a complete classic-xref file (§7.5.4).

    Entry format is exactly 20 bytes: ten digits, a space, five digits, a
    space, the keyword, a two-byte EOL — written longhand so the byte
    count is visible.
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


def fill_ap(bbox: tuple[int, int, int, int], matrix: str | None = None) -> bytes:
    """A form-XObject appearance stream filling its whole BBox black."""
    x0, y0, x1, y1 = bbox
    body = f"0 0 0 rg {x0} {y0} {x1 - x0} {y1 - y0} re f".encode("ascii")
    m = f" /Matrix [{matrix}]" if matrix else ""
    return stream(
        f"/Type /XObject /Subtype /Form /BBox [{x0} {y0} {x1} {y1}]{m} /Resources << >>",
        body,
    )


def one_page(page_extra: str, extra: dict[int, bytes]) -> bytes:
    """A single-page 200x200 document; page is object 3, extras from 4."""
    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            b"/MediaBox [0 0 200 200] /Resources << >> >>"
        ),
        3: f"<< /Type /Page /Parent 2 0 R {page_extra} >>".encode("ascii"),
    }
    objects.update(extra)
    return serialize(objects)


def annot_with_stream_ap(subtype: str, rect: str, bbox, matrix=None, flag=None) -> bytes:
    """A single-annotation page: annot obj 4, its /AP /N stream obj 5."""
    f = f" /F {flag}" if flag is not None else ""
    annot = (
        f"<< /Type /Annot /Subtype /{subtype} /Rect [{rect}]{f} "
        f"/AP << /N 5 0 R >> >>"
    ).encode("ascii")
    return one_page("/Annots [4 0 R]", {4: annot, 5: fill_ap(bbox, matrix)})


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    files: dict[str, bytes] = {}

    # -- placement, pinned from both directions (acceptance crit 4) -----
    files["placement-identity.pdf"] = annot_with_stream_ap(
        "Stamp", "40 30 60 50", (0, 0, 20, 20)
    )
    files["placement-nonorigin-bbox.pdf"] = annot_with_stream_ap(
        "Stamp", "0 0 40 40", (100, 100, 120, 120)
    )
    files["placement-bbox-larger.pdf"] = annot_with_stream_ap(
        "Stamp", "10 10 30 30", (0, 0, 80, 80)
    )
    files["placement-bbox-smaller.pdf"] = annot_with_stream_ap(
        "Stamp", "0 0 200 200", (0, 0, 5, 5)
    )
    files["placement-matrix-scale.pdf"] = annot_with_stream_ap(
        "Stamp", "0 0 40 40", (0, 0, 10, 10), matrix="2 0 0 2 0 0"
    )
    files["placement-matrix-rotate.pdf"] = annot_with_stream_ap(
        "Stamp", "20 20 60 60", (0, 0, 20, 20), matrix="0 1 -1 0 20 0"
    )
    files["placement-inverted-rect.pdf"] = annot_with_stream_ap(
        "Stamp", "60 50 40 30", (0, 0, 20, 20)
    )
    # Degenerate: zero-width BBox — a named refusal, not a divide-by-zero.
    files["placement-degenerate-bbox.pdf"] = one_page(
        "/Annots [4 0 R]",
        {
            4: b"<< /Type /Annot /Subtype /Stamp /Rect [0 0 40 40] /AP << /N 5 0 R >> >>",
            5: stream(
                "/Type /XObject /Subtype /Form /BBox [10 10 10 90] /Resources << >>",
                b"0 0 0 rg 0 0 200 200 re f",
            ),
        },
    )

    # -- flags + non-goals (acceptance crit 5, 6) -----------------------
    files["flags-hidden.pdf"] = annot_with_stream_ap(
        "Stamp", "0 0 200 200", (0, 0, 200, 200), flag=2
    )
    files["flags-noview.pdf"] = annot_with_stream_ap(
        "Stamp", "0 0 200 200", (0, 0, 200, 200), flag=32
    )
    files["popup-not-painted.pdf"] = annot_with_stream_ap(
        "Popup", "0 0 200 200", (0, 0, 200, 200)
    )
    # R43: a /Circle with /IC and no /AP synthesises nothing.
    # -- /RD, which no other fixture carries (Pass 151.0) ---------------
    # A `/Square` with rect differences AND a `/BS /W`, so one fixture
    # exercises both halves of the resize asymmetry: an inset is a length in
    # the space being scaled and travels with it; a border width is a drafting
    # convention and does not. Table 175 orders /RD [left, top, right, bottom],
    # so the four values are deliberately NOT all equal — a bug that scales
    # every slot by sx is invisible against [2 2 2 2] and obvious against this.
    files["rect-differences-square.pdf"] = one_page(
        "/Annots [4 0 R]",
        {
            4: (
                b"<< /Type /Annot /Subtype /Square /Rect [100 100 200 160] "
                b"/RD [2 4 2 4] /BS << /W 3 /S /S >> /C [0 0 0] "
                b"/AP << /N 5 0 R >> >>"
            ),
            5: fill_ap((0, 0, 100, 60)),
        },
    )

    files["no-ap-circle.pdf"] = one_page(
        "/Annots [4 0 R]",
        {4: b"<< /Type /Annot /Subtype /Circle /Rect [40 40 160 160] /IC [1 0 0] >>"},
    )

    # -- appearance-state selection (§12.5.5) ---------------------------
    checkbox = lambda as_state: one_page(
        "/Annots [4 0 R]",
        {
            4: (
                f"<< /Type /Annot /Subtype /Widget /Rect [40 40 160 160] /AS /{as_state} "
                f"/AP << /N << /On 5 0 R /Off 6 0 R >> >> >>"
            ).encode("ascii"),
            5: fill_ap((0, 0, 20, 20)),
            6: stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Resources << >>", b" "
            ),
        },
    )
    files["as-state-checkbox.pdf"] = checkbox("On")
    files["as-missing-state.pdf"] = checkbox("Maybe")

    # -- X8: appearance uses its OWN /Resources, not the page's ---------
    files["ap-resources-own-font.pdf"] = serialize(
        {
            1: b"<< /Type /Catalog /Pages 2 0 R >>",
            2: (
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] "
                b"/Resources << /Font << /F1 8 0 R >> >> >>"
            ),
            3: (
                b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R "
                b"/Resources << /Font << /F1 8 0 R >> >> /Annots [5 0 R] >>"
            ),
            4: stream("", b""),
            5: b"<< /Type /Annot /Subtype /Stamp /Rect [10 10 190 190] /AP << /N 6 0 R >> >>",
            6: stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 200 200] "
                "/Resources << /Font << /F1 7 0 R >> >>",
                b"BT /F1 40 Tf 10 90 Td (T) Tj ET",
            ),
            7: b"<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman >>",  # appearance /F1
            8: b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",  # page /F1
        }
    )

    # -- the completion-demonstration file ------------------------------
    files["demo-annotated.pdf"] = serialize(
        {
            1: b"<< /Type /Catalog /Pages 2 0 R >>",
            2: (
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] "
                b"/Resources << >> >>"
            ),
            3: (
                b"<< /Type /Page /Parent 2 0 R /Annots [4 0 R 6 0 R 7 0 R 8 0 R] >>"
            ),
            # A painted stamp in the lower-left quadrant.
            4: b"<< /Type /Annot /Subtype /Stamp /Rect [20 20 90 90] /AP << /N 5 0 R >> >>",
            5: fill_ap((0, 0, 20, 20)),
            # A Hidden stamp over the whole page (must not appear; counted).
            6: b"<< /Type /Annot /Subtype /Stamp /Rect [0 0 200 200] /F 2 /AP << /N 5 0 R >> >>",
            # A no-/AP circle (R43 named-not-painted).
            7: b"<< /Type /Annot /Subtype /Circle /Rect [110 110 180 180] /IC [1 0 0] >>",
            # A popup (never page content).
            8: b"<< /Type /Annot /Subtype /Popup /Rect [100 100 190 190] /Open true >>",
        }
    )

    for name, data in files.items():
        (OUT_DIR / name).write_bytes(data)
        print(f"wrote {name} ({len(data)} bytes)")
    print(f"{len(files)} annotation fixture(s) in {OUT_DIR}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
