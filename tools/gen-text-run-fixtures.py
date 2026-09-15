#!/usr/bin/env python3
"""Byte-author the fixtures `Pass 32.0`'s per-RUN text work needs.

WHY THESE EXIST
---------------
`Pass 32.0` deletes ONE show operator out of a `BT`...`ET` that may hold
hundreds. Two properties of the model it is built on have to be
falsifiable before the verb can be trusted, and neither is exercised by
any fixture already in the corpus:

1. **Each run's BYTE SPAN.** Deleting a run means removing exactly its
   operator's bytes and re-emitting everything else verbatim. A span
   that is off by one operator deletes a DIFFERENT label from the one
   picked — and the result is well-formed, round-trips cleanly, and is
   therefore undetectable by any structural check. Only a fixture whose
   runs are individually identifiable in the source can catch it.

2. **Whether a run's position is INHERITED.** ISO 32000-1 §9.4.2: a show
   operator leaves the text matrix advanced past the string it drew, and
   the next show operator starts from there unless a positioning
   operator (`Td`, `TD`, `Tm`, `T*`, or the line move inside `'` / `"`)
   moves the pen first. A run with no positioning operator before it has
   **no coordinates anywhere in the file**, so deleting its predecessor
   MOVES it.

   The existing `text/scattered-text-one-object.pdf` cannot test this:
   it positions **both** its runs with an explicit `Tm`, so every run in
   it is `Explicit` and an implementation that hard-coded `Explicit`
   would pass against it.

``text/runs-inherited.pdf``
    One page, one ``BT``...``ET``, four runs chosen so each names a
    different case:

    ==== ============ ==================================================
    Run  Source       What it pins
    ==== ============ ==================================================
    0    ``(ALPHA)``  **Explicit** — a ``Tm`` precedes it. Also the
                      first run, which is `Explicit` even without one:
                      `BT` resets both matrices to the identity
                      (§9.4.1), and that is an origin of its own.
    1    ``(BETA)``   **Inherited** — NOTHING between it and run 0. The
                      case the whole guard exists for.
    2    ``(GAMMA)``  **Explicit** — a ``Td`` precedes it, proving the
                      latch is not `Tm`-only.
    3    ``(DELTA)``  **Inherited** again, so a one-shot latch that
                      cleared and never re-armed is caught.
    ==== ============ ==================================================

    The four strings are distinct and single-word so a test can assert
    on WHICH run it found by decoding, and so a byte span can be
    checked against a literal that appears exactly once in the stream.

``text/runs-td-relative.pdf``
    One ``BT``...``ET`` whose runs after the first are placed by ``Td``
    rather than by ``Tm``: an opening ``Tm``, then ``40 0 Td`` before each
    of the next two.

    Pins the **relative** half of `G017`'s move. A `Td`'s operands are in
    TEXT space and translate the LINE matrix, so moving the run it places
    does two things `runs-two-explicit.pdf` cannot show: the delta has to
    cross `Tm` before it can be written, and the translation survives into
    every later `Td` in the object, which is what makes the compensation of
    the NEXT run necessary rather than decorative. A move that adjusted the
    run and not its successor passes against a `Tm`-only fixture and slides
    the rest of the object here.

``text/runs-tstar-leading.pdf``
    ``1 0 0 1 72 700 Tm``, then ``0 -20 TD``, then three runs separated by
    bare ``T*``.

    Pins the **opaque** half. Neither `TD` nor `T*` offers an operand pair a
    move can adjust in isolation:

    * ``T*`` carries none at all;
    * ``TD``'s ``ty`` IS the leading (Table 108 — it sets `TL` to `−ty` and
      then translates), so nudging it would silently re-space **every later
      `T*` in the object**. The bytes stay well-formed, the file round-trips,
      and text nobody selected has moved. That is the class rule 4 exists
      for, and it is why `TD` is classified opaque rather than relative.

    So the move must INSERT a `Td` instead, and disclose that it did.

``text/runs-rotated-td.pdf``
    ``0 1 -1 0 300 300 Tm`` — a 90° text matrix — followed by a ``40 0 Td``
    run.

    Pins that the drag crosses **both** transforms (§9.4.4). A page-space
    nudge of `(5, 0)` is `(0, −5)` in this run's text space, so the `Td`
    must become ``40 -5 Td``. An implementation that converted through the
    CTM only — correct on every axis-aligned fixture above — writes
    ``45 0 Td`` and moves the text along its own baseline instead of across
    it. R162: without this fixture that bug cannot be made to fail.

``text/runs-tj-array.pdf``
    One ``BT``...``ET`` whose single run is a ``TJ`` **array** with
    kerning numbers: ``[(A) -120 (B) -120 (C)] TJ``.

    Pins that a `TJ` array is **ONE** run, not three. Its numeric
    elements are kerning within a single positioned string, not separate
    placements — splitting on them would fragment a word into per-glyph
    runs, and "delete this run" would then mean something no operator
    asked for. An implementation that counted show *strings* rather than
    show *operators* would report 3 here.

PROVENANCE
----------
100% byte-authored here, no PDF library involved, so a fixture cannot
inherit a bug from the code it tests. Project rule 7 / ``LEGAL.md`` §5:
synthetic or rights-cleared only. Non-embedded Helvetica (§9.6.2.2
permits omitting ``/Widths`` and ``/FontDescriptor`` for the standard
14), which is what lets the runs lay out and therefore be measured.

USAGE
-----
    python tools/gen-text-run-fixtures.py
"""

import sys
from pathlib import Path

OUT = Path("fixtures/synthetic/text")
PAGE_W, PAGE_H = 612, 792


def serialize(objects: dict[int, bytes]) -> bytes:
    """A classic §7.5.4 cross-reference table over a 1-based object map."""
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets: dict[int, int] = {}
    highest = max(objects)
    for num in sorted(objects):
        body = objects[num]
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode("ascii") + body + b"\nendobj\n"
    xref_at = len(out)
    out += f"xref\n0 {highest + 1}\n".encode("ascii")
    out += b"0000000000 65535 f \n"
    for num in range(1, highest + 1):
        out += (
            f"{offsets[num]:010d} 00000 n \n".encode("ascii")
            if num in offsets
            else b"0000000000 65535 f \n"
        )
    out += (
        f"trailer\n<< /Size {highest + 1} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n"
    ).encode("ascii")
    return bytes(out)


def raw_stream(body: bytes) -> bytes:
    return f"<< /Length {len(body)} >>\nstream\n".encode("ascii") + body + b"\nendstream"


def page(content: bytes) -> bytes:
    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            f"/MediaBox [0 0 {PAGE_W} {PAGE_H}] "
            f"/Resources << /Font << /F1 5 0 R >> >> >>"
        ).encode("ascii"),
        3: b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>",
        4: raw_stream(content),
        5: (
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
            b"/Encoding /WinAnsiEncoding >>"
        ),
    }
    return serialize(objects)


def runs_inherited() -> bytes:
    """Four runs: Explicit, Inherited, Explicit (via `Td`), Inherited."""
    return page(
        b"BT\n/F1 10 Tf\n"
        b"1 0 0 1 72 700 Tm\n"
        b"(ALPHA) Tj\n"       # run 0 — Explicit (the Tm above, and it is first)
        b"(BETA) Tj\n"        # run 1 — INHERITED: nothing positions it
        b"0 -20 Td\n"
        b"(GAMMA) Tj\n"       # run 2 — Explicit via Td, not Tm
        b"(DELTA) Tj\n"       # run 3 — INHERITED again: the latch must re-arm
        b"ET\n"
    )


def runs_two_explicit() -> bytes:
    """Two runs, each placed by its own ``Tm`` — neither inherits.

    The baseline for deletion: removing either must leave the other
    byte-verbatim, including its own positioning operator. Distinct from
    ``runs-inherited.pdf`` precisely because NOTHING here is refused, so a
    test can assert what a successful per-run delete looks like without
    first navigating the §9.4.2 guard.
    """
    return page(
        b"BT\n/F1 10 Tf\n"
        b"1 0 0 1 72 700 Tm\n(ALPHA) Tj\n"
        b"1 0 0 1 72 680 Tm\n(BETA) Tj\n"
        b"ET\n"
    )


def runs_single() -> bytes:
    """ONE run, next to an unrelated path.

    Deleting the only run must take the whole ``BT``...``ET`` with it (a text
    object that shows nothing is not an object) and must leave the path
    verbatim — which is what makes this fixture different from a page
    holding nothing else: without the path, "the text object was removed"
    and "the whole content stream was emptied" look identical.
    """
    return page(
        b"q 1 w 0 0 0 RG\n"
        b"72 396 m 540 396 l S\n"
        b"Q\n"
        b"BT\n/F1 10 Tf\n"
        b"1 0 0 1 72 700 Tm\n(ONLY) Tj\n"
        b"ET\n"
    )


def runs_tj_array() -> bytes:
    """One run, spelled as a kerned `TJ` array."""
    return page(
        b"BT\n/F1 10 Tf\n"
        b"1 0 0 1 72 700 Tm\n"
        b"[(A) -120 (B) -120 (C)] TJ\n"   # ONE run, three strings
        b"ET\n"
    )


def runs_td_relative() -> bytes:
    """Three runs: one ``Tm``, then two placed by ``Td``."""
    return page(
        b"BT\n/F1 10 Tf\n"
        b"1 0 0 1 72 700 Tm\n(ALPHA) Tj\n"
        b"40 0 Td\n(BETA) Tj\n"
        b"40 0 Td\n(GAMMA) Tj\n"
        b"ET\n"
    )


def runs_tstar_leading() -> bytes:
    """Three runs placed by ``TD`` then bare ``T*`` — nothing adjustable."""
    return page(
        b"BT\n/F1 10 Tf\n"
        b"1 0 0 1 72 700 Tm\n"
        b"0 -20 TD\n(ALPHA) Tj\n"
        b"T*\n(BETA) Tj\n"
        b"T*\n(GAMMA) Tj\n"
        b"ET\n"
    )


def runs_rotated_td() -> bytes:
    """A 90° text matrix with a ``Td``-placed second run."""
    return page(
        b"BT\n/F1 10 Tf\n"
        b"0 1 -1 0 300 300 Tm\n(ALPHA) Tj\n"
        b"40 0 Td\n(BETA) Tj\n"
        b"ET\n"
    )


def main() -> int:
    out_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else OUT
    out_dir.mkdir(parents=True, exist_ok=True)
    for name, data in (
        ("runs-inherited.pdf", runs_inherited()),
        ("runs-tj-array.pdf", runs_tj_array()),
        ("runs-two-explicit.pdf", runs_two_explicit()),
        ("runs-single.pdf", runs_single()),
        ("runs-td-relative.pdf", runs_td_relative()),
        ("runs-tstar-leading.pdf", runs_tstar_leading()),
        ("runs-rotated-td.pdf", runs_rotated_td()),
    ):
        p = out_dir / name
        p.write_bytes(data)
        print(f"wrote {p} ({len(data)} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
