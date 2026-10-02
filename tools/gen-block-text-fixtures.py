#!/usr/bin/env python3
"""Generate fixtures/synthetic/reflow/block_text.pdf: word-processor-shaped
paragraphs for EditSession::edit_block_text (Pass 433.0).

Every byte is constructed here (LEGAL.md §5 category (a)); the classic-xref
writer is the one in gen-reflow-fixtures.py. Helvetica, WinAnsi, no font
program embedded. Deterministic: two runs produce identical bytes.

Pages:

1. tagged: inside `q ... Q`, a three-line paragraph under one
   `/P <</MCID 0>> BDC ... EMC`, one `BT ... ET` per line with an absolute
   `Tm` and a `TJ`, then, at y=560, a two-line paragraph under MCID 1.
2. per-line tags: a three-line paragraph whose every line is its own
   `/Span <</MCID n>> BDC BT ... ET EMC`, then a one-line paragraph.

    python tools/gen-block-text-fixtures.py
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("gen_reflow", HERE / "gen-reflow-fixtures.py")
base = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(base)

OUT = HERE.parent / "fixtures" / "synthetic" / "reflow" / "block_text.pdf"

FIRST = [
    "Synthetic placeholder words make up this",
    "first paragraph so that the edit has three",
    "lines that wrap at one width.",
]
SECOND = [
    "A second paragraph follows the first one",
    "and must not move when the first is edited.",
]
LEADING = 14


def line(y: int, text: str) -> bytes:
    return f"BT /F1 11 Tf 1 0 0 1 72 {y} Tm 0 g [({text})] TJ ET\n".encode("ascii")


def tagged_page() -> bytes:
    out = bytearray(b"q\n/P <</MCID 0>> BDC\n")
    y = 700
    for text in FIRST:
        out += line(y, text)
        y -= LEADING
    out += b"EMC\n/P <</MCID 1>> BDC\n"
    y = 560
    for text in SECOND:
        out += line(y, text)
        y -= LEADING
    out += b"EMC\nQ\n"
    return bytes(out)


def per_line_page() -> bytes:
    out = bytearray(b"q\n")
    y = 700
    for i, text in enumerate(FIRST):
        out += f"/Span <</MCID {i}>> BDC\n".encode("ascii")
        out += line(y, text)
        out += b"EMC\n"
        y -= LEADING
    y -= 2 * LEADING
    out += line(y, SECOND[1])
    out += b"Q\n"
    return bytes(out)


def build() -> bytes:
    contents = [tagged_page(), per_line_page()]
    font = 3 + 2 * len(contents)
    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        font: b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    }
    kids = []
    for i, body in enumerate(contents):
        page, content = 3 + 2 * i, 4 + 2 * i
        kids.append(f"{page} 0 R")
        objects[page] = (
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {content} 0 R >>"
        ).encode("ascii")
        objects[content] = base.stream(body)
    objects[2] = (
        f"<< /Type /Pages /Kids [{' '.join(kids)}] /Count {len(kids)} "
        f"/Resources << /Font << /F1 {font} 0 R >> >> >>"
    ).encode("ascii")
    return base.serialize(objects)


def main() -> int:
    data = build()
    OUT.write_bytes(data)
    print(f"wrote {OUT} - {len(data)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
