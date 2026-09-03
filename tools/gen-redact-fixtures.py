#!/usr/bin/env python3
"""Generate synthetic redaction fixtures (ISO 32000-1 §12.5.6.23).

Emits a two-page PDF (`fixtures/synthetic/redact/demo-secret.pdf`) whose
text contains the literal word "SECRET" in two contexts — a heading and a
body line — plus surrounding "PUBLIC" text that must survive a redaction in
place. Standard-14 Helvetica (no /Widths) so extraction/redaction use the
accurate AFM advance widths. A third fixture places an image XObject that a
region will intersect, to exercise the refuse-or-clear behaviour.

Synthetic, self-authored — LEGAL §5 compliant (no third-party content).
"""
import os

OUT = "fixtures/synthetic/redact"


def stream_obj(dict_prefix, content):
    return dict_prefix + b" /Length %d >>\nstream\n" % len(content) + content + b"\nendstream"


def assemble(objs, root=1, extra_trailer=b""):
    buf = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"
    off = {}
    for n in sorted(objs):
        off[n] = len(buf)
        buf += b"%d 0 obj\n" % n + objs[n] + b"\nendobj\n"
    xref_at = len(buf)
    size = max(objs) + 1
    buf += b"xref\n0 %d\n0000000000 65535 f \n" % size
    for n in range(1, size):
        buf += b"%010d 00000 n \n" % off[n]
    buf += b"trailer\n<< /Size %d /Root %d 0 R %s>>\nstartxref\n%d\n%%%%EOF\n" % (
        size, root, extra_trailer, xref_at,
    )
    return buf


def demo_secret():
    """Two pages, each showing SECRET + PUBLIC text; /Info duplicates SECRET."""
    font = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"
    c1 = (b"BT /F1 24 Tf 40 150 Td (SECRET dossier) Tj "
          b"/F1 14 Tf 0 -40 Td (This SECRET line and PUBLIC text.) Tj ET")
    c2 = b"BT /F1 18 Tf 40 150 Td (Account SECRET and PUBLIC balance.) Tj ET"
    objs = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        3: (b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 220] "
            b"/Resources << /Font << /F1 7 0 R >> >> /Contents 5 0 R >>"),
        4: (b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 220] "
            b"/Resources << /Font << /F1 7 0 R >> >> /Contents 6 0 R >>"),
        5: stream_obj(b"<<", c1),
        6: stream_obj(b"<<", c2),
        7: font,
        8: b"<< /Title (SECRET dossier) /Author (pdfcer test) >>",
    }
    return assemble(objs, extra_trailer=b"/Info 8 0 R ")


def demo_image():
    """A page whose only content is a raster image a region will intersect."""
    img = stream_obj(
        b"<< /Type /XObject /Subtype /Image /Width 2 /Height 2 "
        b"/BitsPerComponent 8 /ColorSpace /DeviceGray",
        b"\x00\x40\x80\xff",
    )
    content = b"q 200 0 0 120 100 60 cm /Im1 Do Q"
    objs = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        3: (b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 220] "
            b"/Resources << /XObject << /Im1 5 0 R >> >> /Contents 4 0 R >>"),
        4: stream_obj(b"<<", content),
        5: img,
    }
    return assemble(objs)


def main():
    os.makedirs(OUT, exist_ok=True)
    for name, data in [
        ("demo-secret.pdf", demo_secret()),
        ("demo-image.pdf", demo_image()),
    ]:
        path = os.path.join(OUT, name)
        with open(path, "wb") as f:
            f.write(data)
        print("wrote", path, len(data), "bytes")


if __name__ == "__main__":
    main()
