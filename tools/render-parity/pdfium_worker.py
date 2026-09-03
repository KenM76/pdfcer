#!/usr/bin/env python3
"""pdfium_worker.py — the crash-isolated reference-renderer half of render-parity.

WHY THIS FILE EXISTS
====================
`render_parity.py` used to `import pypdfium2` and drive it **in its own
process**. That is fatal at corpus scale, for a reason no amount of Python
`try/except` can fix:

    fixtures/external/pdfium/testing/resources/bug_457855936.pdf

is a fuzzer artefact (759 bytes, no `%PDF-` header, a `startxref` pointing at
garbage) that trips an internal `CHECK()` inside PDFium's own C++ during
`FPDF_LoadDocument` — i.e. **at open, before any page is touched**. A firing
`CHECK` calls `abort()`/`__debugbreak()`. On Windows the process exits with
`0x80000003` (`STATUS_BREAKPOINT`); on POSIX it dies on `SIGABRT`/`SIGTRAP`.
Either way the **whole interpreter is gone**: no Python exception is raised,
no `except Exception` runs, no `finally` runs, no traceback is printed, and
every result accumulated in memory up to that point is lost. Measured
behaviour, reproduced 2026-08-08:

    $ python -c "import pypdfium2 as p; p.PdfDocument('...bug_457855936.pdf')"
    (no output)      # not even a Python-level error
    ExitCode: 0x80000003

pdfcer itself handles the same file **correctly and cleanly** — it refuses it
with `not a PDF: no %PDF- header in the first 759 bytes`, exit code 4. So this
is unambiguously a *reference-renderer* fault, and it must be bucketed as one.

THE ISOLATION TECHNIQUE (ported, not invented)
==============================================
`tools/cmyk-calibration/corpus_cmyk.py` already proved the fix in this repo:
run PDFium **in a child process**, so an abort kills the child and the parent
merely observes a non-zero exit code. This module ports that technique and
adds the one thing a corpus sweep needs that a per-file `python -c` does not:

    the child is PERSISTENT.

`corpus_cmyk.py` spawns a fresh interpreter per file. That is fine for the few
dozen DeviceCMYK files; over 4,023 corpus files it would add a fresh Python
startup **plus** a `pypdfium2` import (which dlopen()s the PDFium binary) to
every single file — on the order of 0.3–0.5 s each, i.e. 20–35 minutes of pure
overhead. So instead this worker is a long-lived request/response server:
spawned once, reused for every file, and **respawned only when it dies**.

PROTOCOL (newline-delimited JSON over stdin/stdout)
===================================================
The parent writes one JSON object per line to the worker's stdin and reads
exactly one JSON object per line back from its stdout. `stderr` is left free
for PDFium's own chatter and is captured separately by the parent.

Requests:

    {"op": "ping"}
        -> {"ok": true, "pong": true}

    {"op": "count", "path": "<abs path>"}
        -> {"ok": true, "npages": <int>}
        -> {"ok": false, "error": "<message>"}     # a *catchable* failure

    {"op": "render", "path": "<abs path>", "page": <0-based int>,
     "scale": <float>, "annots": <bool>, "out": "<abs path>"}
        -> {"ok": true, "w": <int>, "h": <int>}
        -> {"ok": false, "error": "<message>"}

    {"op": "quit"}
        -> (no response; the worker exits 0)

RAW RGBA, NOT PNG — and why
---------------------------
`render` writes the raster to `out` as **raw, uncompressed, top-left-origin
RGBA8** (`w * h * 4` bytes, row-major, no header) and returns `w`/`h` in the
JSON reply. PNG would cost an encode in the child and a decode in the parent
on every one of ~4,000 pages for zero benefit — nothing but the parent ever
reads the file, and it is deleted immediately.

Correctness note: the parent reconstructs the buffer with
`Image.frombytes("RGBA", (w, h), data)` and hands it to the *same*
`to_white_rgb()` compositing routine the in-process version used. The pixels
are therefore **bit-identical** to the pre-isolation harness — the child
process boundary changes the failure mode, not the measurement.

FAILURE SEMANTICS — the whole point
===================================
There are exactly three outcomes the parent must distinguish, and this
protocol makes all three observable:

1. `{"ok": true, ...}`   — the render succeeded.
2. `{"ok": false, ...}`  — PDFium failed in a way Python could catch (a
                           malformed but *survivable* file, e.g. "Failed to
                           load document"). Historically this became a
                           `skip` and still does.
3. **the pipe closes**   — the worker is *gone*. An internal `CHECK`, a
                           segfault, a heap corruption. This is the case that
                           used to end the run; the parent now records it as
                           `reference-aborted` and respawns.

A hang is handled parent-side (it cannot be handled here — a wedged child
cannot report its own wedge): the parent applies a read timeout and kills.

INVARIANTS
==========
* This module imports `pypdfium2`. `render_parity.py` **must not** — that
  import is what put PDFium in the parent's address space. The parent's only
  contact with PDFium is this subprocess.
* Never print anything to stdout except protocol lines. Any diagnostic goes
  to stderr, which the parent drains separately.
* Every request gets exactly one response line, or the process dies. There is
  no "no reply" success case, because the parent uses a missing reply as its
  abort signal.
* Documents are closed after every request so the worker's memory does not
  grow across 4,000 files.
"""

from __future__ import annotations

import json
import sys


def _emit(obj: dict) -> None:
    """Write one protocol response line and flush.

    Flushing is mandatory, not hygiene: the parent blocks on a line read, and
    a buffered reply is indistinguishable from a dead worker (the parent's
    timeout would fire and kill a perfectly healthy child).
    """
    sys.stdout.write(json.dumps(obj, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def _count(pdfium, path: str) -> dict:
    """Page count. Isolated because `PdfDocument()` is where the known abort
    fires — the harness never even reaches a render on the hostile file."""
    doc = pdfium.PdfDocument(path)
    try:
        return {"ok": True, "npages": len(doc)}
    finally:
        doc.close()


def _render(pdfium, req: dict) -> dict:
    """Render one page to raw RGBA8 at `req['out']`.

    `page` is 0-based (PDFium's own convention; the parent converts from its
    1-based user-facing page numbers). `scale` is DPI/72 — passed through
    verbatim so the child never re-derives it and cannot drift from the
    pdfcer-side scale.
    """
    doc = pdfium.PdfDocument(req["path"])
    try:
        page = doc[int(req["page"])]
        try:
            bitmap = page.render(scale=float(req["scale"]), draw_annots=bool(req["annots"]))
            img = bitmap.to_pil()
            if img.mode != "RGBA":
                img = img.convert("RGBA")
            with open(req["out"], "wb") as fh:
                fh.write(img.tobytes())
            return {"ok": True, "w": img.width, "h": img.height}
        finally:
            page.close()
    finally:
        doc.close()


def main() -> int:
    # Imported here, not at module scope, so an import failure is reported as
    # a clean startup error on stderr rather than an opaque dead pipe.
    try:
        import pypdfium2 as pdfium
    except Exception as exc:  # noqa: BLE001
        sys.stderr.write(f"pdfium_worker: cannot import pypdfium2: {exc}\n")
        return 2

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except Exception as exc:  # noqa: BLE001
            _emit({"ok": False, "error": f"bad request json: {exc}"})
            continue

        op = req.get("op")
        if op == "quit":
            return 0
        try:
            if op == "ping":
                _emit({"ok": True, "pong": True})
            elif op == "count":
                _emit(_count(pdfium, req["path"]))
            elif op == "render":
                _emit(_render(pdfium, req))
            else:
                _emit({"ok": False, "error": f"unknown op {op!r}"})
        except Exception as exc:  # noqa: BLE001
            # A *catchable* PDFium failure. Distinct from the uncatchable
            # abort, which never reaches this line — it takes the process.
            _emit({"ok": False, "error": f"{type(exc).__name__}: {exc}"})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
