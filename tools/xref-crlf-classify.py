#!/usr/bin/env python3
# xref-crlf-classify.py — Pass A (decision 013) diagnostic / MEASUREMENT tool.
#
# PURPOSE
#   Disambiguate WHY real-world classic-`xref` PDFs whose failures correlate
#   with CRLF line endings fail to load in pdfcer-core. Given the corpus sweep's
#   per-file error message plus the raw bytes, it assigns each failure a ROOT
#   CAUSE, so pdfcer decision 013's split — Pass A (classic-table strict
#   correctness) vs Pass B (rebuild-by-scan recovery) — is EVIDENCED, not
#   asserted.
#
#   This script performs NO recovery and writes NO pdfcer-core code. Its
#   whole-file `xref`-scan is used only to LOCATE the real marker for
#   classification (is the stored `startxref` pointing at it or not?), never
#   to load the document. Recovery lives in Pass B.
#
# ROOT-CAUSE LABELS (per §7.5.4 / §7.5.5)
#   OFFSET_SHIFT      stored `startxref N` does NOT land on `xref`/`N G obj`;
#                     a real `xref` keyword exists elsewhere in the file. The
#                     file's stored offsets are stale — the LF->CRLF byte-count
#                     shift signature. => Pass B (rebuild-by-scan), NOT a bug.
#   OBJ_OFFSET_SHIFT  xref table parsed, but a per-object offset is stale so an
#                     object header does not parse where xref points. Same
#                     stale-offset family. => Pass B.
#   GEN_65536         table reached via a CORRECT startxref, every entry a
#                     well-formed 20-byte §7.5.4 record, but a generation field
#                     holds 65536 (exceeds the spec max of 65535). pdfcer's
#                     u16::try_from rejects it => whole load fails. SPEC-NON-
#                     CONFORMANT data; strict rejection is correct. A distinct
#                     tolerance finding, NOT the CRLF/EOL story and NOT a
#                     class-(b) "reject a spec-valid table" bug.
#   DEVIANT_19        BadEntry from a 19-byte entry (single-char EOL). The
#                     documented 19-byte deviant pdfcer refuses (module doc).
#   DEVIANT_21_CRLF   BadEntry from a 21-byte `SP CR LF` entry — the LF->CRLF
#                     mangling of an `SP LF` table. Genuinely malformed
#                     (violates exactly-20-bytes); strict rejection correct.
#   NO_REAL_XREF      startxref not on a marker and no `xref` keyword anywhere
#                     (xref-stream-only, encrypted, or truncated tail).
#   NOT_STREAM        startxref lands on `N G obj` that is not a stream.
#   PARSER_BUG        table reached via a correct startxref, every entry a
#                     SPEC-CONFORMANT 20-byte record (legal EOL, gen<=65535),
#                     yet pdfcer rejects it. THIS is the class-(b) bug Pass A
#                     exists to catch. (Expected count: 0.)
#   OTHER             anything not matching the above (non-xref error kinds).
#
# INPUT : a sweep TSV mapping `path<TAB>pdfcer-message` (see fixtures/external/
#         realworld-*.tsv, pre-flattened). Reads each listed PDF from disk.
# OUTPUT: TSV `path  crlf  klass  detail` + a stderr cross-tab of crlf x klass.
#
# PROVENANCE: synthetic diagnostic script, CC0. Reads only fixtures already
# present under fixtures/external (rights per fixtures/README.md).

import re
import sys

WINDOW = 4096
LEGAL_EOL = (b" \r", b" \n", b"\r\n")


def last_startxref(buf):
    start = max(0, len(buf) - WINDOW)
    w = buf[start:]
    pos = w.rfind(b"startxref")
    if pos < 0:
        return None
    after = start + pos + 9
    m = re.match(rb"\s*(\d+)", buf[after:after + 64])
    return int(m.group(1)) if m else None


def at_marker(buf, off):
    if off is None or not (0 <= off < len(buf)):
        return "oob"
    if buf[off:off + 4] == b"xref" and (off + 4 >= len(buf) or buf[off + 4] in b" \r\n\t\f"):
        return "xref"
    if re.match(rb"\d+\s+\d+\s+obj", buf[off:off + 40]):
        return "objhdr"
    return "other"


def real_xrefs(buf):
    out = []
    for m in re.finditer(rb"(?:^|[\r\n\x00\t\f ])xref(?:[\r\n\t\f ])", buf):
        s = m.start()
        if buf[s:s + 4] != b"xref":
            s += 1
        out.append(s)
    return out


def crlf_dominant(buf):
    crlf = buf.count(b"\r\n")
    lone_lf = buf.count(b"\n") - crlf
    return crlf >= max(1, lone_lf)


def entry_at(buf, off):
    """Classify the 20/21-byte record starting at `off`."""
    rec = buf[off:off + 21]
    m = re.match(rb"(\d{10}) (\d{5}) ([nf])", rec)
    if not m:
        return ("nonstd", None)
    gen = int(m.group(2))
    eol2 = rec[18:20]
    if len(rec) >= 21 and rec[18:21] == b" \r\n":
        return ("dev21", gen)          # SP CR LF (21 bytes)
    if eol2 in LEGAL_EOL:
        return ("ok20", gen)           # valid 20-byte record
    if rec[18:19] in (b"\r", b"\n") and rec[19:20] not in (b"\r", b"\n", b" "):
        return ("dev19", gen)          # single-char EOL (19 bytes)
    return ("badeol", gen)


def classify(path, msg):
    try:
        buf = open(path, "rb").read()
    except OSError as e:
        return ("n", "OTHER", f"readerr {e}")
    crlf = "Y" if crlf_dominant(buf) else "n"
    off = None
    mo = re.findall(r"byte (\d+)", msg)
    if mo:
        off = int(mo[-1])

    # BadEntry family: inspect the aligned record.
    if "malformed 20-byte xref entry" in msg and off is not None:
        kind, gen = entry_at(buf, off)
        if kind == "dev21":
            return (crlf, "DEVIANT_21_CRLF", f"SP CR LF 21-byte entry gen={gen}")
        if kind == "dev19":
            return (crlf, "DEVIANT_19", f"single-char-EOL 19-byte entry gen={gen}")
        if kind == "ok20" and gen is not None and gen > 65535:
            return (crlf, "GEN_65536", f"20-byte record, generation {gen} > 65535")
        if kind == "ok20":
            return (crlf, "PARSER_BUG", f"20-byte spec-valid record rejected gen={gen}")
        return (crlf, "DEVIANT_19", f"nonstandard record {buf[off:off + 20]!r}")

    # Object-level stale offset (xref parsed; object header off).
    if re.search(r"object .*xref offset \d+.*malformed indirect-object header", msg) or \
       re.search(r"object at xref offset \d+ declares", msg):
        return (crlf, "OBJ_OFFSET_SHIFT", msg.split(":", 1)[0])

    # startxref-target classification errors.
    if ("expected an xref keyword" in msg or
            "malformed indirect-object header" in msg or
            "startxref offset missing or out of range" in msg):
        sx = last_startxref(buf)
        mk = at_marker(buf, sx)
        reals = real_xrefs(buf)
        if mk in ("other", "oob"):
            if reals:
                near = min(reals, key=lambda r: abs(r - (sx or 0)))
                return (crlf, "OFFSET_SHIFT", f"startxref@{sx} not xref; real xref@{near} (shift {near - (sx or 0)})")
            return (crlf, "NO_REAL_XREF", f"startxref@{sx} not a marker; no xref keyword in file")
        if mk == "objhdr":
            return (crlf, "OFFSET_SHIFT", f"startxref@{sx} lands on N G obj (shifted or stream)")
        # startxref DID land on xref but still failed classification here
        return (crlf, "PARSER_BUG", f"startxref@{sx} on real xref yet rejected: {msg.split(':',1)[-1].strip()}")

    if "startxref target is not a stream object" in msg:
        return (crlf, "NOT_STREAM", "objhdr not a stream")
    if "no startxref found" in msg:
        return (crlf, "NO_REAL_XREF", "no startxref in scan window")

    return (crlf, "OTHER", msg.split(":", 1)[0])


def main(argv):
    if len(argv) < 2:
        sys.exit("usage: xref-crlf-classify.py <sweep.tsv> [--only-xref]")
    rows = []
    for line in open(argv[1], encoding="utf-8", errors="replace"):
        line = line.rstrip("\n")
        if "\t" not in line:
            continue
        path, msg = line.split("\t", 1)
        rows.append((path, msg))

    print("path\tcrlf\tklass\tdetail")
    tab = {}
    for path, msg in rows:
        crlf, klass, detail = classify(path, msg)
        tab.setdefault(klass, {"Y": 0, "n": 0})
        tab[klass][crlf] = tab[klass].get(crlf, 0) + 1
        print(f"{path}\t{crlf}\t{klass}\t{detail}")

    sys.stderr.write("\n=== root-cause x line-ending cross-tab ===\n")
    sys.stderr.write(f"{'klass':22} {'CRLF':>6} {'LF':>6} {'total':>6}\n")
    for k in sorted(tab, key=lambda k: -(tab[k]['Y'] + tab[k]['n'])):
        y, n = tab[k]["Y"], tab[k]["n"]
        sys.stderr.write(f"{k:22} {y:>6} {n:>6} {y + n:>6}\n")


if __name__ == "__main__":
    main(sys.argv)
