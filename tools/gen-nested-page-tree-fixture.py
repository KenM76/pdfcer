#!/usr/bin/env python3
"""Generate a synthetic THREE-LEVEL nested /Pages tree fixture.

Why this exists: `delete_pages` (and the removal path `page-copy --cut`
shares) decremented `/Count` only on nodes that lost a direct `/Kids`
entry, leaving ANCESTOR counts stale on a nested tree — invisible on the
flat one-level trees every prior fixture had (pdfcer-gui bug, 2026-09-05,
against a real SolidWorks drawing). A test needs a tree at least THREE
levels deep so it can tell "no upward walk at all" from "a walk that
stops one short".

Tree (12 pages, 7 /Pages nodes):

    root  /Count 12  /Kids [A, B]
      A   /Count  6  /Kids [A1, A2]
        A1 /Count 3  /Kids [p1 p2 p3]
        A2 /Count 3  /Kids [p4 p5 p6]
      B   /Count  6  /Kids [B1, B2]
        B1 /Count 3  /Kids [p7 p8 p9]
        B2 /Count 3  /Kids [p10 p11 p12]

Output is a minimal but valid PDF 1.7 with a classic xref table. All
content is synthetic (LEGAL.md §5). Deterministic — no timestamps.
"""
import io
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "pageops" / "nested-tree-3level.pdf"


def build() -> bytes:
    # Object numbering:
    # 1 catalog, 2 root Pages, 3 A, 4 A1, 5 A2, 6 B, 7 B1, 8 B2,
    # 9..20 the twelve page leaves, 21 the shared content stream.
    A1_kids = [9, 10, 11]
    A2_kids = [12, 13, 14]
    B1_kids = [15, 16, 17]
    B2_kids = [18, 19, 20]
    leaves = A1_kids + A2_kids + B1_kids + B2_kids
    content_id = 21

    objs: dict[int, str] = {}
    objs[1] = "<< /Type /Catalog /Pages 2 0 R >>"
    objs[2] = "<< /Type /Pages /Count 12 /Kids [3 0 R 6 0 R] >>"
    objs[3] = "<< /Type /Pages /Count 6 /Parent 2 0 R /Kids [4 0 R 5 0 R] >>"
    objs[4] = f"<< /Type /Pages /Count 3 /Parent 3 0 R /Kids [{' '.join(f'{k} 0 R' for k in A1_kids)}] >>"
    objs[5] = f"<< /Type /Pages /Count 3 /Parent 3 0 R /Kids [{' '.join(f'{k} 0 R' for k in A2_kids)}] >>"
    objs[6] = "<< /Type /Pages /Count 6 /Parent 2 0 R /Kids [7 0 R 8 0 R] >>"
    objs[7] = f"<< /Type /Pages /Count 3 /Parent 6 0 R /Kids [{' '.join(f'{k} 0 R' for k in B1_kids)}] >>"
    objs[8] = f"<< /Type /Pages /Count 3 /Parent 6 0 R /Kids [{' '.join(f'{k} 0 R' for k in B2_kids)}] >>"

    parent_of = {}
    for k in A1_kids:
        parent_of[k] = 4
    for k in A2_kids:
        parent_of[k] = 5
    for k in B1_kids:
        parent_of[k] = 7
    for k in B2_kids:
        parent_of[k] = 8
    for leaf in leaves:
        objs[leaf] = (
            f"<< /Type /Page /Parent {parent_of[leaf]} 0 R "
            f"/MediaBox [0 0 612 792] /Contents {content_id} 0 R "
            "/Resources << >> >>"
        )
    stream = b"BT /F1 12 Tf 72 720 Td (page) Tj ET\n"
    objs[content_id] = f"<< /Length {len(stream)} >>\nstream\n".encode() + stream + b"endstream"

    n = content_id
    buf = io.BytesIO()
    buf.write(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = {}
    for i in range(1, n + 1):
        offsets[i] = buf.tell()
        body = objs[i]
        if isinstance(body, str):
            body = body.encode()
        buf.write(f"{i} 0 obj\n".encode() + body + b"\nendobj\n")
    xref_pos = buf.tell()
    buf.write(f"xref\n0 {n + 1}\n".encode())
    buf.write(b"0000000000 65535 f \n")
    for i in range(1, n + 1):
        buf.write(f"{offsets[i]:010d} 00000 n \n".encode())
    buf.write(b"trailer\n")
    buf.write(f"<< /Size {n + 1} /Root 1 0 R >>\n".encode())
    buf.write(b"startxref\n")
    buf.write(f"{xref_pos}\n".encode())
    buf.write(b"%%EOF\n")
    return buf.getvalue()


def main() -> int:
    data = build()
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_bytes(data)
    print(f"wrote {OUT} ({len(data)} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
