"""Census: how often does a PDF carry a rendering intent, and does it switch?

Answers iccce's question 2.3 (2026-08-25 request): what is the realistic worst
case number of distinct (source, destination, intent, BPC) combinations one
page can produce.  The half pdfcer can measure is INTENT: how many distinct
rendering intents a document declares, and whether more than one appears.

Two passes, deliberately separated because they are different evidence:

  RAW   -- substring search over the file bytes only.  No inflate, no parse.
           Very cheap.  UNDERCOUNTS by construction: any /RI inside an object
           stream or any `ri` operator inside a FlateDecode content stream is
           invisible to it.
  DEEP  -- the same search after inflating every FlateDecode stream that will
           inflate.  Run on a SAMPLE, to calibrate the undercount.

Emits TSV so the numbers can be re-derived rather than quoted.
"""
import os, re, sys, zlib, random

INTENTS = [b"AbsoluteColorimetric", b"RelativeColorimetric", b"Saturation", b"Perceptual"]
RI_KEY = re.compile(rb"/RI\s*/([A-Za-z]+)")
RI_OP  = re.compile(rb"/([A-Za-z]+)\s+ri\b")

def scan(buf):
    """Return set of intent names named by /RI <name> or `<name> ri`."""
    found = set()
    for m in RI_KEY.finditer(buf):
        found.add(m.group(1))
    for m in RI_OP.finditer(buf):
        if m.group(1) in INTENTS:
            found.add(m.group(1))
    return found

STREAM = re.compile(rb"stream\r?\n", re.S)

def inflate_all(buf, budget=64 * 1024 * 1024):
    """Yield inflated payloads for every `stream`..`endstream` that inflates.

    Deliberately dumb: tries zlib on each stream body and ignores failures.
    That is correct for this question -- a stream that is not Flate cannot be
    hiding a Flate-compressed /RI -- and it avoids a full xref/parse pass.
    """
    spent = 0
    pos = 0
    while True:
        m = STREAM.search(buf, pos)
        if not m:
            return
        end = buf.find(b"endstream", m.end())
        if end < 0:
            return
        body = buf[m.end():end]
        pos = end + 9
        if not body or len(body) > 8 * 1024 * 1024:
            continue
        try:
            out = zlib.decompressobj().decompress(body, 4 * 1024 * 1024)
        except Exception:
            continue
        spent += len(out)
        if out:
            yield out
        if spent > budget:
            return

def main(root, sample_n, out_tsv):
    files = []
    for dirpath, _, names in os.walk(root):
        for n in names:
            if n.lower().endswith(".pdf"):
                files.append(os.path.join(dirpath, n))
    files.sort()
    random.seed(20260825)
    sample = set(random.sample(files, min(sample_n, len(files))))

    rows = []
    raw_hits = deep_hits = raw_multi = deep_multi = 0
    for i, p in enumerate(files):
        try:
            sz = os.path.getsize(p)
            if sz > 40 * 1024 * 1024:
                continue
            with open(p, "rb") as fh:
                buf = fh.read()
        except Exception:
            continue
        r = scan(buf)
        d = set(r)
        deep = p in sample
        if deep:
            for payload in inflate_all(buf):
                d |= scan(payload)
        if r:
            raw_hits += 1
            if len(r) > 1:
                raw_multi += 1
        if deep and d:
            deep_hits += 1
            if len(d) > 1:
                deep_multi += 1
        if r or (deep and d):
            rows.append((p, "deep" if deep else "raw",
                         ",".join(sorted(x.decode() for x in r)) or "-",
                         ",".join(sorted(x.decode() for x in d)) or "-"))
        del buf
        if i % 500 == 0:
            print(f"  {i}/{len(files)}", file=sys.stderr, flush=True)

    with open(out_tsv, "w", encoding="utf-8") as fh:
        fh.write("path\tclass\traw_intents\tdeep_intents\n")
        for r in rows:
            fh.write("\t".join(r) + "\n")
    print(f"files scanned      : {len(files)}")
    print(f"deep-scan sample   : {len(sample)}")
    print(f"RAW  any intent    : {raw_hits}")
    print(f"RAW  >1 intent     : {raw_multi}")
    print(f"DEEP any intent    : {deep_hits} of {len(sample)}")
    print(f"DEEP >1 intent     : {deep_multi}")

if __name__ == "__main__":
    main(sys.argv[1], int(sys.argv[2]), sys.argv[3])
