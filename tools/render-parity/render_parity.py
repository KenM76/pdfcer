#!/usr/bin/env python3
"""render-parity — full-page pdfium (pypdfium2) pixel-parity harness (Pass 11).

WHY THIS EXISTS
===============
`docs/decisions/010` sequences render-fidelity verification (candidate C,
Pass 11) *ahead* of vector/content-stream editing (candidate A) for one
structural reason: vector editing is the first subsystem whose correctness
oracle is independent *visual* fidelity. pdfcer's existing round-trip oracle
(`tools/content-identity`, `tools/roundtrip`) proves pdfcer agrees with
ITSELF — sufficient for additive authoring, useless for proving an *edited*
page still renders *correctly*. That needs an independent reference renderer.

This harness is that oracle. It generalizes `tools/annot-pdfium-diff.py`
(an ink-bounding-box differential on 7 annotation fixtures) into a
FULL-PAGE, per-channel, per-pixel differential between:

  * pdfcer   — via the shipped `pdfcer render-page` binary (the same
              read path the GUI uses), and
  * pdfium  — via `pypdfium2` (the engine inside Chrome; the decision 006
              §3.2 tooling precedent),

over the whole loadable conformance corpus (`fixtures/external`, ~2,914
files). It is out-of-tree tooling exactly like the other corpus harnesses:
it is never shipped, never in `cargo test`, never in the GUI-core
`cargo tree` invariant, and pypdfium2 never enters pdfcer's runtime
dependency set or `THIRD_PARTY_LICENSES.md` (decision 010 acceptance;
LEGAL §6).

THE CENTRAL PROBLEM (decision 010 risk Y1) — NOISE vs SIGNAL
============================================================
Two independent renderers ALWAYS differ at the pixel level: anti-aliasing,
font hinting, sub-pixel glyph positioning, and image interpolation are all
implementation choices, not bugs. Demanding pixel-for-pixel agreement is a
category error. The analytical core of this Pass is therefore SEPARATING
benign renderer noise from real fidelity divergence, WITHOUT either of the
two forbidden failure modes:

  * W14 — tuning a threshold until a number turns green; and
  * declaring benign anti-aliasing noise a "bug".

HOW THE TOLERANCE BAND IS DERIVED (empirical, not tuned) — see README.md §3
--------------------------------------------------------------------------
The band is NOT a hand-picked number. It is derived from the data:

  1. Every page is rendered in both engines and reduced to a per-page
     divergence metric `frac_over_32` = the fraction of pixels whose
     maximum per-channel absolute delta exceeds 32/255. (Rationale:
     benign AA/hinting noise is confined to a THIN sub-pixel band around
     edges, so it touches a SMALL fraction of the page even where
     individual edge pixels swing the full 0..255; a real divergence — a
     missing shading fill, a wrong DeviceCMYK colour, a shifted glyph run —
     touches a LARGE contiguous AREA, i.e. a large fraction. Fraction-of-
     area, not max-delta, is the noise-robust discriminator.)

  2. Each page is tagged with pdfcer's OWN disclosed diagnostics (the
     `render-page` stdout tally): does it substitute glyphs, skip a Type3
     font, defer an `sh`/marked-content operator, drop an image codec,
     carry a DeviceCMYK JPEG, etc.? A page with ZERO disclosed gaps AND no
     DeviceCMYK content is "clean-by-construction": whatever it diverges by
     CAN ONLY be renderer noise, because pdfcer itself claims to render it
     fully.

  3. The benign band is the high percentile (default p99.0, configurable
     and reported) of `frac_over_32` OVER THE CLEAN-BY-CONSTRUCTION PAGES
     ONLY. The band is a property of the known-benign population, so it
     cannot be "tuned to make a bug pass" — a bug lives, by definition,
     either on a page pdfcer discloses a gap for (bucket ii) or in the
     residual tail of clean pages ABOVE their own noise floor (bucket iii).

THE THREE BUCKETS (decision 010 deliverable 3; R20 by-file-and-reason)
======================================================================
Every (file, page) is classified:

  (i)   below-band            — frac_over_32 <= band AND pdfcer disclosed
                                 nothing. NOT a verdict of "benign": it was
                                 called `benign-renderer-noise` until
                                 2026-08-09, which asserted a CAUSE from a
                                 THRESHOLD. A structural audit measured
                                 15.4% of that population as not
                                 edge-shaped, and found four confirmed
                                 pdfcer bugs inside it. Small and
                                 unexplained; nothing more.
  (ii)  disclosed-gap[-small]  — pdfcer disclosed a gap that explains it
                                 (Type3, sh shading, /SMask, /OC, image
                                 codec, DeviceCMYK, a substituted font
                                 face, ...). Checked BEFORE the band: an
                                 explanation does not stop being an
                                 explanation because the divergence it
                                 explains is small. The `-small` suffix
                                 marks the below-band half. Formerly this
                                 test ran AFTER the band, which filed
                                 1,656 explained pages as renderer noise —
                                 including a page pdfcer rendered blank.
  (ii-legacy) known-disclosed-gap — frac_over_32 > band AND pdfcer disclosed a
                                 gap that explains it (Type3, sh shading,
                                 /SMask, /OC, image codec, DeviceCMYK, a
                                 substituted font face, ...). Cross-checked
                                 against pdfcer's existing Diagnostics tally
                                 so an already-counted gap is SUBTRACTED,
                                 not re-reported as a new bug.
  (iii) unexplained-divergence — frac_over_32 > band AND no disclosed gap
                                 explains it. The genuine bug candidates —
                                 the residual after subtracting (i)+(ii).

Plus three side classifications that are NOT pdfcer errors:

  * reference-divergence — (only in --annots mode) the page carries a
    /Widget or a no-/AP annotation; pdfium needs FPDF_FFLDraw to draw
    widgets and SYNTHESIZES some no-/AP appearances (e.g. /Circle /IC fill)
    that R43 makes pdfcer correctly REFUSE (Pass 6.0 finding). Bucketed
    reference-side so pdfium's own quirks are never misattributed to pdfcer
    (decision 010 deliverable 5 / risk Y2). The DEFAULT run is content-only
    (annotations off on both sides), which structurally avoids this
    confounder entirely — the vector-editing oracle cares about page
    CONTENT, which is what an edit re-renders.

  * reference-aborted — pdfium did not "fail", it DIED: an internal CHECK,
    a segfault, a heap corruption, or a hang. See "CRASH ISOLATION" below.
    A reference renderer that aborts on a file has told us nothing about
    pdfcer, so this can never be a pdfcer bucket; it is a named, counted,
    enumerated property of the REFERENCE tool on that file.

  * skipped — pdfcer could not load/render, or pdfium reported a catchable
    failure, or the page boxes disagree past `--dim-tol`. Out of scope, like
    the roundtrip gate's unloadable files. Counted, never silently dropped.

CRASH ISOLATION — why pdfium runs in a child process
====================================================
`fixtures/external/pdfium/testing/resources/bug_457855936.pdf` (a 759-byte
fuzzer artefact with no `%PDF-` header) trips an internal `CHECK()` inside
PDFium's C++ during `FPDF_LoadDocument` — at OPEN, before any page renders.
A firing `CHECK` calls `abort()`; on Windows the process exits `0x80000003`
(`STATUS_BREAKPOINT`), on POSIX it dies on `SIGABRT`/`SIGTRAP`.

That is **not a Python exception**. It cannot be caught. When this harness
imported `pypdfium2` into its OWN process it therefore died outright at that
file (~index 300 of 4,023 in sorted order), with no traceback, no partial
report, and every accumulated result lost — which is why the corpus sweep
had been unrunnable. pdfcer, for the record, handles the same file correctly:
it refuses it with `not a PDF: no %PDF- header in the first 759 bytes`, exit
4. The fault is entirely reference-side.

The fix, ported from `tools/cmyk-calibration/corpus_cmyk.py` (which proved
the technique in this repo), is to drive PDFium from a **child process**:
`pdfium_worker.py`. An abort now kills the child; the parent observes a dead
pipe, records `reference-aborted` with the exit code, respawns, and carries
on. `render_parity.py` MUST NOT import `pypdfium2` — that import is exactly
the defect. The worker is persistent (one process reused across the whole
corpus, respawned only on death) so isolation costs no per-file interpreter
startup.

PARTIAL RESULTS SURVIVE
=======================
A sweep of 4,023 files is long enough that "it died, so you get nothing" is
itself a defect. Three mechanisms:

  * `out/per-page.partial.tsv` is streamed and flushed row-by-row DURING the
    sweep, so even a hard kill leaves every measurement taken so far.
  * `out/progress.json` is checkpointed every `--checkpoint-every` files.
  * Ctrl-C or an unexpected parent-side exception is caught, the sweep stops,
    and the FULL reporting pipeline still runs on what was collected. The
    report is stamped `"run": {"complete": false, ...}` so a partial can
    never be mistaken for a full sweep.

STALE-BASELINE GUARD (see README §11)
=====================================
`out/summary.json` doubles as the recorded baseline the gate compares
against. Bucket counts are only comparable between runs over the SAME corpus
at the SAME settings — the corpus has already grown 2,914 -> 4,023 files, and
comparing those two as though they were the same measurement is a silent
falsehood. So every report now carries a `corpus_fingerprint` (SHA-256 over
the sorted relative path list) plus the comparability-relevant config, and
the harness REFUSES to overwrite a mismatching baseline unless `--rebaseline`
is passed (which archives the old one rather than deleting it). The check
runs BEFORE the sweep, so a mismatch costs a second, not an hour.

OUTPUTS (deterministic, locale-invariant)
=========================================
  out/per-page.tsv   — one row per (file, page): dims, metrics, bucket,
                        reason, the raw pdfcer diagnostics. Sorted.
  out/per-page.partial.tsv — the streamed, crash-surviving copy (bucket
                        column blank; buckets need the whole population).
                        Removed on a clean, complete run.
  out/progress.json   — periodic checkpoint during the sweep.
  out/summary.txt     — the distribution + the bucket counts + the
                        DeviceCMYK characterization + the enumerated
                        unexplained tail (R20) + the aborted-file list.
  out/summary.json    — the same, machine-readable, for a gate/CI check;
                        carries `corpus_fingerprint` and `run.complete`.
  out/diffs/*.png     — side-by-side (pdfcer | pdfium | 8x-amplified delta)
                        panels for the top unexplained pages and any page
                        named with --diff, for eyeball triage / the demo.

GATE ROLE (decision 010 deliverable 6; the R34/R46 pattern)
===========================================================
`--gate` mode exits non-zero if the count of UNEXPLAINED pages exceeds
`--max-unexplained` (default 0 once the corpus baseline is filed). This is
the standing render-fidelity gate: it must be re-run on every render-
touching Pass — ESPECIALLY the vector-editing Pass, whose content-stream
edits re-render the very pages this harness measures. Like content-identity
and roundtrip it is a LOCAL corpus gate (pypdfium2 is not in CI), documented
as required in README.md.

USAGE
=====
    python render_parity.py [CORPUS_DIR ...] [options]

    # default: content-only, 150 DPI, <=4 sampled pages/file, full corpus
    python render_parity.py

    # bounded demo subset
    python render_parity.py --max-files 200 --emit-diffs 12

    # one specific page's diff panel (for the demo / triage)
    python render_parity.py --diff "veraPDF-corpus/.../file.pdf" --diff-page 1

    # gate mode for a render-touching Pass
    python render_parity.py --gate --max-unexplained 0

Requires: pypdfium2, numpy, Pillow, and a built `pdfcer` release binary
(`cargo build --release -p pdfcer-cli`).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np
from PIL import Image

# NOTE: `pypdfium2` is deliberately NOT imported here. PDFium aborts the whole
# process on at least one corpus file (module docstring, "CRASH ISOLATION"),
# and an abort in the parent is unrecoverable by construction. All PDFium
# contact goes through `pdfium_worker.py` in a child process. Importing
# pypdfium2 in this file would reintroduce the exact defect this harness was
# unrunnable for.

HERE = Path(__file__).resolve().parent
WORKER = HERE / "pdfium_worker.py"
ROOT = HERE.parent.parent
DEFAULT_CORPUS = ROOT / "fixtures" / "external"
CLI = ROOT / "target" / "release" / (
    "pdfcer.exe" if sys.platform == "win32" else "pdfcer"
)

# Per-pixel delta threshold (0..255) above which a pixel "differs
# substantially". 32 is ~12.5% of full range — comfortably above 8-bit
# rounding + gamma jitter, comfortably below a real colour/geometry error.
PIXEL_DELTA_T = 32
# Secondary thresholds reported for the distribution, not used for bucketing.
EXTRA_T = (16, 64)


# --- reference-renderer crash isolation ------------------------------------

# Windows NTSTATUS codes a native abort surfaces as a process exit code. Named
# so the report says WHAT killed the reference renderer, not just a hex number
# — "STATUS_BREAKPOINT" identifies a deliberate CHECK/assert, whereas
# "STATUS_ACCESS_VIOLATION" would identify a genuine memory-safety fault, and
# those are very different findings about the reference tool.
NTSTATUS_NAMES = {
    0x80000003: "STATUS_BREAKPOINT",            # a CHECK()/DCHECK()/__debugbreak()
    0xC0000005: "STATUS_ACCESS_VIOLATION",      # segfault
    0xC0000008: "STATUS_INVALID_HANDLE",
    0xC000001D: "STATUS_ILLEGAL_INSTRUCTION",
    0xC0000025: "STATUS_NONCONTINUABLE_EXCEPTION",
    0xC000008C: "STATUS_ARRAY_BOUNDS_EXCEEDED",
    0xC000008E: "STATUS_FLOAT_DIVIDE_BY_ZERO",
    0xC0000094: "STATUS_INTEGER_DIVIDE_BY_ZERO",
    0xC0000096: "STATUS_PRIVILEGED_INSTRUCTION",
    0xC00000FD: "STATUS_STACK_OVERFLOW",
    0xC0000374: "STATUS_HEAP_CORRUPTION",
    0xC0000409: "STATUS_STACK_BUFFER_OVERRUN",   # /GS cookie, i.e. __fastfail
    0xC0000602: "STATUS_FAIL_FAST_EXCEPTION",
}

# POSIX signal names for the same purpose (a child killed by a signal reports
# a negative returncode in Python: -N means "died on signal N").
POSIX_SIGNALS = {
    4: "SIGILL", 6: "SIGABRT", 7: "SIGBUS", 8: "SIGFPE",
    9: "SIGKILL", 11: "SIGSEGV", 5: "SIGTRAP",
}


def describe_exit(rc: int | None) -> str:
    """Render a dead child's exit code as a human/LLM-legible cause string.

    WHY this matters to the report rather than being cosmetic: the bucket
    `reference-aborted` is only credible if it names the fault. `exit
    0x80000003 (STATUS_BREAKPOINT)` says "PDFium's own CHECK fired", which is
    a *deliberate* self-abort on input the library refuses to process — a very
    different claim from "PDFium corrupted memory". The harness must not
    flatten those into "it crashed".
    """
    if rc is None:
        return "still running"
    if rc < 0:  # POSIX: killed by signal -rc
        sig = -rc
        return f"signal {sig} ({POSIX_SIGNALS.get(sig, 'SIG?')})"
    # Windows surfaces NTSTATUS as a large unsigned exit code; Python may hand
    # it back already-unsigned. Normalize to 32-bit unsigned for the lookup.
    u = rc & 0xFFFFFFFF
    if u in NTSTATUS_NAMES:
        return f"exit 0x{u:08X} ({NTSTATUS_NAMES[u]})"
    if u >= 0x80000000:
        return f"exit 0x{u:08X} (unnamed NTSTATUS)"
    return f"exit {rc}"


class ReferenceAborted(Exception):
    """The reference renderer process DIED servicing a request.

    Distinct from `ReferenceFailed` in the way that matters: a *failure* is
    PDFium saying "I cannot load this", which is information about the FILE
    and has always been a legitimate `skip`. An *abort* is PDFium ceasing to
    exist, which is information about PDFIUM, and used to be information the
    harness could not survive long enough to record.
    """

    def __init__(self, cause: str, stderr_tail: str = "") -> None:
        super().__init__(cause)
        self.cause = cause
        self.stderr_tail = stderr_tail


class ReferenceFailed(Exception):
    """The reference renderer reported a catchable error (survivable)."""


class ReferenceTimeout(Exception):
    """The reference renderer wedged and had to be killed.

    A hang cannot be self-reported — a wedged child cannot tell us it is
    wedged — so this is detected parent-side by a read timeout. Bucketed
    alongside aborts, because the operational consequence is identical: the
    worker must be destroyed and respawned, and the file yielded no reference
    raster.
    """


class PdfiumWorker:
    """A persistent, respawnable child process that owns all PDFium contact.

    LIFECYCLE
    ---------
    Spawned lazily on the first request and kept alive across the entire
    corpus (PDFium's `dlopen` + a Python interpreter startup is ~0.3-0.5 s;
    paying that 4,023 times would add half an hour of pure overhead, which is
    the reason `corpus_cmyk.py`'s spawn-per-file variant was not simply
    copied). It is destroyed and respawned only when it dies or wedges.

    WHY A READER THREAD
    -------------------
    The parent must be able to distinguish "the worker is thinking" from "the
    worker is gone" from "the worker is wedged". A bare blocking
    `stdout.readline()` conflates the last two: on a dead child it returns
    `''` promptly (fine), but on a wedged child it blocks forever (fatal to a
    4,000-file sweep). A daemon reader thread pushing lines into a `Queue`
    lets `request()` apply a wall-clock timeout to the read, which is the only
    place a hang can be caught.

    ATTRIBUTION HONESTY
    -------------------
    A death observed while servicing request N is attributed to request N.
    That is sound in the overwhelming case (the worker replied successfully to
    request N-1, so it was alive after it), but it is not a proof: a
    heap-corrupting file could in principle kill the worker on a *later*
    allocation. This is why `--verify-aborts` exists — every file bucketed
    `reference-aborted` is re-run afterwards in its own dedicated one-shot
    child, and the report records whether the abort reproduced in isolation.
    An unverified abort is reported as unverified, never quietly upgraded.
    """

    def __init__(self, timeout: float, python: str | None = None) -> None:
        self.timeout = timeout
        self.python = python or sys.executable
        self.proc: subprocess.Popen | None = None
        self._q: queue.Queue | None = None
        self._err: list[str] = []
        self.spawns = 0

    # -- process management -------------------------------------------------

    def start(self) -> None:
        if self.proc is not None:
            return
        self.proc = subprocess.Popen(
            [self.python, "-u", str(WORKER)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            bufsize=1,
        )
        self.spawns += 1
        self._q = queue.Queue()
        self._err = []
        threading.Thread(target=self._pump_stdout, args=(self.proc, self._q), daemon=True).start()
        threading.Thread(target=self._pump_stderr, args=(self.proc,), daemon=True).start()

    @staticmethod
    def _pump_stdout(proc: subprocess.Popen, q: queue.Queue) -> None:
        """Feed protocol lines into the queue; push `None` on EOF (= death)."""
        try:
            for line in proc.stdout:  # type: ignore[union-attr]
                q.put(line)
        except Exception:  # noqa: BLE001 — pipe torn down mid-read
            pass
        finally:
            q.put(None)

    def _pump_stderr(self, proc: subprocess.Popen) -> None:
        """Drain the child's stderr so it can never fill its pipe buffer and
        deadlock the child mid-render. Retain only a bounded tail — PDFium can
        be chatty and the report only needs the last words before a death."""
        try:
            for line in proc.stderr:  # type: ignore[union-attr]
                self._err.append(line.rstrip("\n"))
                if len(self._err) > 40:
                    del self._err[: len(self._err) - 40]
        except Exception:  # noqa: BLE001
            pass

    def kill(self) -> int | None:
        """Destroy the worker; return its exit code (None if it had none)."""
        p, self.proc = self.proc, None
        if p is None:
            return None
        try:
            if p.poll() is None:
                p.kill()
            p.wait(timeout=10)
        except Exception:  # noqa: BLE001
            pass
        try:
            for h in (p.stdin, p.stdout, p.stderr):
                if h:
                    h.close()
        except Exception:  # noqa: BLE001
            pass
        return p.returncode

    def close(self) -> None:
        """Ask the worker to exit cleanly; kill it if it will not."""
        if self.proc is None:
            return
        try:
            self.proc.stdin.write(json.dumps({"op": "quit"}) + "\n")  # type: ignore[union-attr]
            self.proc.stdin.flush()  # type: ignore[union-attr]
            self.proc.wait(timeout=5)
            self.proc = None
        except Exception:  # noqa: BLE001
            self.kill()

    # -- protocol -----------------------------------------------------------

    def request(self, req: dict) -> dict:
        """Send one request, return its reply dict.

        Raises `ReferenceAborted` if the worker died, `ReferenceTimeout` if it
        wedged, `ReferenceFailed` if it reported a catchable error. In the
        first two cases the worker has been destroyed and the next call will
        transparently respawn it.
        """
        self.start()
        assert self.proc is not None and self._q is not None
        try:
            self.proc.stdin.write(json.dumps(req, separators=(",", ":")) + "\n")  # type: ignore[union-attr]
            self.proc.stdin.flush()  # type: ignore[union-attr]
        except Exception:  # noqa: BLE001 — broken pipe == the worker is gone
            rc = self.kill()
            raise ReferenceAborted(describe_exit(rc), "\n".join(self._err[-6:]))

        try:
            line = self._q.get(timeout=self.timeout)
        except queue.Empty:
            tail = "\n".join(self._err[-6:])
            self.kill()
            raise ReferenceTimeout(f"no reply in {self.timeout:.0f}s") from None

        if line is None:  # EOF: the child exited/aborted mid-request
            # Give the process a moment to be reaped so `returncode` is real
            # rather than None (the reader thread sees EOF marginally before
            # the OS finishes tearing the process down).
            p = self.proc
            try:
                p.wait(timeout=5)  # type: ignore[union-attr]
            except Exception:  # noqa: BLE001
                pass
            tail = "\n".join(self._err[-6:])
            rc = self.kill()
            raise ReferenceAborted(describe_exit(rc), tail)

        try:
            rep = json.loads(line)
        except Exception as exc:  # noqa: BLE001
            self.kill()
            raise ReferenceAborted(f"unparseable reply ({exc})", line[:200]) from None
        if not rep.get("ok"):
            raise ReferenceFailed(str(rep.get("error", "unknown"))[:160])
        return rep

    # -- typed operations ---------------------------------------------------

    def page_count(self, path: Path) -> int:
        return int(self.request({"op": "count", "path": str(path)})["npages"])

    def render(self, path: Path, page0: int, scale: float, annots: bool, raw: Path) -> np.ndarray:
        """Render page `page0` (0-based) and return the white-composited RGB.

        The worker writes raw top-left-origin RGBA8 (no container) and returns
        the dimensions; the parent rebuilds a PIL image from that buffer and
        runs it through the SAME `to_white_rgb` compositing the in-process
        version used, so the measured pixels are bit-identical to the
        pre-isolation harness. Isolation changed the failure mode, not the
        measurement.
        """
        rep = self.request({
            "op": "render", "path": str(path), "page": page0,
            "scale": scale, "annots": bool(annots), "out": str(raw),
        })
        w, h = int(rep["w"]), int(rep["h"])
        data = raw.read_bytes()
        if len(data) != w * h * 4:
            raise ReferenceFailed(f"raster truncated: {len(data)} != {w}*{h}*4")
        return to_white_rgb(Image.frombytes("RGBA", (w, h), data))


def probe_abort_in_isolation(path: Path, page0: int, scale: float, annots: bool,
                             timeout: float) -> dict:
    """Re-run one file in its OWN one-shot child to confirm an abort is real.

    Rationale in `PdfiumWorker`'s docstring: an abort seen in the shared
    worker is attributed to the request in flight, which is sound but not
    proof. This re-runs the file alone, in a process that has touched nothing
    else, and reports what happened. Only a file that dies here is stated as a
    confirmed reference-renderer abort.
    """
    w = PdfiumWorker(timeout=timeout)
    out = {"reproduced": False, "cause": "", "note": ""}
    try:
        with tempfile.TemporaryDirectory() as td:
            w.page_count(path)
            w.render(path, page0, scale, annots, Path(td) / "probe.raw")
        out["note"] = "did NOT reproduce in isolation — attribution uncertain"
    except ReferenceAborted as exc:
        out["reproduced"] = True
        out["cause"] = exc.cause
        out["note"] = exc.stderr_tail[-200:]
    except ReferenceTimeout as exc:
        out["reproduced"] = True
        out["cause"] = f"timeout ({exc})"
    except ReferenceFailed as exc:
        out["note"] = f"failed cleanly in isolation (no abort): {exc}"
    finally:
        w.kill()
    return out


def devicecmyk_in_file(path: Path) -> bool:
    """Whether the raw file bytes mention `/DeviceCMYK`.

    WHY a byte scan and not a diagnostics counter: pdfcer's render Diagnostics
    count DeviceCMYK *JPEGs* (`dct_cmyk`) but there is no counter for
    DeviceCMYK *vector* fills/strokes, and decision 006 §3.7 established that
    the naive-additive `Rgb::from_cmyk` colorimetry gap affects ALL
    DeviceCMYK painting, not just images. A file-level byte scan is a
    tooling-only, render-unchanged way to flag "this file could exhibit the
    colorimetry gap" so the harness can characterize it (deliverable 7)
    without adding a render-side counter (a non-goal this Pass). It is a
    file-level (not page-level) over-approximation, stated honestly.
    """
    try:
        return b"/DeviceCMYK" in path.read_bytes()
    except OSError:
        return False


# --- pdfcer diagnostics -----------------------------------------------------

# Map of `render-page` stdout keys -> whether a non-zero value is a
# CONTENT-affecting disclosed gap that would legitimately diverge from
# pdfium. Keys not listed here (images=, forms=, annots_painted=, ...) are
# volume counters, not gaps.
GAP_KEYS = {
    "unsupported": "font-unsupported",          # Type3 / exotic CMap: text skipped
    "substituted": "font-substituted",          # substitute face: shapes differ from embedded
    "notdef": "glyph-notdef",                   # .notdef boxes
    "deferred": "deferred-op",                  # sh shading / BDC-EMC (OC) / Type3 proc / clip
    "images_unsupported": "image-unsupported",
    "images_codec_unsupported": "image-codec",
    "codec_features": "image-codec-feature",
    "codec_geometry_mismatch": "image-geometry",
    "jpx_preblended": "jpx-preblended",
    "lzw_anomalies": "lzw-anomaly",
    "dct_cmyk": "devicecmyk-jpeg",              # decision 006 §3.7 colorimetry (image)
    "dct_cmyk_unverifiable": "dct-polarity",
}
# Annotation keys that indicate a pdfium REFERENCE-divergence. Only ever
# non-zero in `--annots` mode, so they are consulted only there.
REF_KEYS = {
    "annots_widget": "pdfium-fflodraw-widget",  # pdfium needs FPDF_FFLDraw
    "annots_no_ap": "pdfium-synthesized-noap",  # pdfium synthesizes /IC etc.; R43 refuses
}

# Image colour-space keys where PDFIUM is the renderer that diverges from the
# standard. Consulted in EVERY mode -- unlike REF_KEYS above, these fire on
# ordinary content, not on annotations.
#
# WHY THESE ARE REFERENCE-DIVERGENCES AND NOT DISCLOSED GAPS
# ==========================================================
# The distinction between GAP_KEYS and a reference-divergence is not a matter
# of taste: it is the claim about WHO is wrong. A disclosed gap says pdfcer
# could not do something. A reference-divergence says pdfcer did it correctly
# and pdfium did not, so the comparison carries no information about pdfcer.
# Asserting the second needs an oracle that is neither renderer, and for
# these two keys there is one.
#
# `img_uncalibrated` fires for exactly /Lab, /CalGray and /CalRGB -- the three
# CIE-based non-ICC spaces, all defined by CLOSED-FORM arithmetic in ISO
# 32000-1 8.6.5.2-8.6.5.4. `tools/check-image-colorspace-truth.py` computes
# that arithmetic independently of both renderers. Measured 2026-08-17 over
# 3,600 interior texels per space:
#
#     space     pdfcer mean / max      pdfium mean / max
#     lab         0.019 / 1             40.854 / 152
#     calgray     0.000 / 0              2.000 /   9
#     calrgb      0.030 / 1              3.012 /   9
#
# pdfcer is exact to within one 8-bit code on all three; pdfium is not. The
# session handoff had recorded the opposite assumption -- that pdfcer's
# uncalibrated conversion was the CAUSE of the lab.pdf divergence -- and the
# measurement reversed it.
#
# `img_colorant_none` fires for a /Separation /None image, which 8.6.6.4 says
# "shall never be painted on the page". pdfcer leaves the backdrop untouched;
# pdfium paints the image SOLID BLACK. Measured on sep-none.pdf: every pdfcer
# pixel (255,255,255) against every pdfium pixel (0,0,0), frac32 = 1.0. That
# is the maximum divergence the harness can report, and all of it is pdfcer
# being right.
#
# THE COST, STATED RATHER THAN HIDDEN
# ===================================
# Classifying a page this way removes it from the bug-candidate pool, so a
# REAL pdfcer defect on a page that also carries a Lab image would be masked.
# That is accepted deliberately, because a page pdfium renders wrongly cannot
# yield a verdict about pdfcer either way -- but it is why the analytic oracle
# above exists as a separate, non-comparative check. Run it, not this harness,
# when the question is whether the CIE-based conversions are correct.
IMAGE_REF_KEYS = {
    "img_uncalibrated": "pdfium-cie-conversion",
    "img_colorant_none": "pdfium-paints-colorant-none",
}


def parse_diag_line(line: str) -> dict[str, int] | None:
    """Parse the `render-page` stdout stable line into {key: int}.

    Also extracts the raster dimensions from the `-> <path> WxH` clause. The
    line's contract (pdfcer module docs) is append-only key=value pairs,
    so a robust `k=v` scan survives future counter additions.
    """
    if "->" not in line:
        return None
    out: dict[str, int] = {}
    # dimensions: token of the form WxH right after the output path.
    for tok in line.replace(";", " ").split():
        if "=" in tok:
            k, v = tok.split("=", 1)
            if v.lstrip("-").isdigit():
                out[k] = int(v)
        elif "x" in tok:
            a, _, b = tok.partition("x")
            if a.isdigit() and b.isdigit():
                out["_w"], out["_h"] = int(a), int(b)
    return out or None


@dataclass
class PageResult:
    rel: str
    page: int  # 1-based
    status: str  # "ok" | "skip" | "abort"
    reason: str = ""
    w: int = 0
    h: int = 0
    mean: float = 0.0
    p95: float = 0.0
    dmax: int = 0
    frac16: float = 0.0
    frac32: float = 0.0
    frac64: float = 0.0
    dim_mismatch: int = 0
    clean: int = 0  # 1 if no disclosed gap AND no DeviceCMYK (band-derivation set)
    devicecmyk: int = 0
    gaps: str = ""  # comma-joined gap reasons (bucket ii candidates)
    # Comma-joined reasons the REFERENCE renderer is the divergent one. Fed by
    # IMAGE_REF_KEYS in every mode, plus REF_KEYS under `--annots`.
    refdiv: str = ""
    bucket: str = ""  # filled in phase 2
    # keep the delta image path only when we emit a diff panel
    _arrays: object = field(default=None, repr=False, compare=False)


def to_white_rgb(img: Image.Image) -> np.ndarray:
    """Composite any image onto a white background, return HxWx3 uint8.

    Both engines may emit alpha; a PDF's "white page" is transparent in
    neither reference. Compositing onto white normalizes transparency so the
    comparison is of visible colour, not of premultiplied alpha conventions.
    """
    if img.mode != "RGBA":
        img = img.convert("RGBA")
    bg = Image.new("RGBA", img.size, (255, 255, 255, 255))
    comp = Image.alpha_composite(bg, img).convert("RGB")
    return np.asarray(comp, dtype=np.uint8)


def render_pdfce(
    path: Path, page: int, scale: float, annots: bool, tmp: Path, timeout: float
) -> tuple[np.ndarray, dict[str, int]]:
    """Render one page via the pdfcer CLI; return (rgb array, diagnostics).

    decision 012 R63 — the gate is BUNDLED-ONLY by construction: this
    command deliberately never passes `--font-dir`, so the renderer uses
    exactly `FontEnvironment::bundled()` and no operator-supplied face can
    perturb the pixels. Supplied-font renders are machine-dependent by
    definition and are therefore outside this determinism gate; adding a
    `--font-dir` here (or reading one from the environment) would break the
    gate's reproducibility. The invariant is enforced at the render layer
    too (`render_is_font_dir_independent_for_unreferenced_supplied_faces`
    in `crates/pdfcer-render/src/lib.rs`).
    """
    out = tmp / "pdfcer.png"
    cmd = [
        str(CLI), "render-page", str(path),
        "--page", str(page), "--scale", f"{scale:.6f}", "-o", str(out),
    ]
    if not annots:
        cmd.append("--no-annotations")
    r = subprocess.run(cmd, capture_output=True, timeout=timeout)
    if r.returncode != 0:
        raise RuntimeError(
            "pdfcer render rc=%d: %s"
            % (r.returncode, r.stderr.decode(errors="replace").strip()[:200])
        )
    diag = parse_diag_line(r.stdout.decode(errors="replace")) or {}
    # `pdfcer` prints "note: N structural oddity(ies) tolerated" to
    # STDERR, and this function parsed only stdout — so the one thing
    # pdfcer says when it could not act on something was structurally
    # invisible to its own differential oracle.
    #
    # 442 of 3,714 measured pages (11.9%) emit it, and 202 of the 2,015
    # CLEAN-BY-CONSTRUCTION pages do (10.0%). Every one of those helped
    # define the band while privately reporting that pdfcer had skipped
    # something. That is the mechanism by which a silent bug raises the
    # threshold that hides other bugs.
    err = r.stderr.decode(errors="replace")
    marker = "structural oddity"
    if marker in err:
        for line in err.splitlines():
            if marker not in line:
                continue
            for tok in line.replace(":", " ").split():
                if tok.isdigit():
                    diag["tolerated"] = int(tok)
                    break
            break
        diag.setdefault("tolerated", 1)
    arr = to_white_rgb(Image.open(out))
    return arr, diag


def compare(a: np.ndarray, b: np.ndarray) -> tuple[np.ndarray, dict, int]:
    """Align two RGB rasters and compute the per-pixel max-channel delta.

    Returns (delta_map HxW uint16, stats, dim_mismatch_flag). Alignment crops
    both to the common top-left region: both engines emit a top-left-origin
    raster of the SAME page box, so a 1px rounding difference is absorbed by
    cropping to the min extent. A larger mismatch is flagged (page-box
    disagreement is a geometry finding, not a pixel one) but still measured on
    the overlap so it is never silently dropped.
    """
    dim_mismatch = int(a.shape[0] != b.shape[0] or a.shape[1] != b.shape[1])
    h = min(a.shape[0], b.shape[0])
    w = min(a.shape[1], b.shape[1])
    a = a[:h, :w, :].astype(np.int16)
    b = b[:h, :w, :].astype(np.int16)
    delta = np.abs(a - b).max(axis=2).astype(np.uint16)  # HxW, 0..255
    n = delta.size
    stats = {
        "mean": float(delta.mean()),
        "p95": float(np.percentile(delta, 95)),
        "dmax": int(delta.max()),
        "frac16": float(np.count_nonzero(delta > 16) / n),
        "frac32": float(np.count_nonzero(delta > 32) / n),
        "frac64": float(np.count_nonzero(delta > 64) / n),
    }
    return delta, stats, dim_mismatch


def gap_reasons(diag: dict[str, int]) -> list[str]:
    return [label for key, label in GAP_KEYS.items() if diag.get(key, 0) > 0]


def ref_reasons(diag: dict[str, int], annots: bool) -> list[str]:
    """Reasons the REFERENCE renderer -- not pdfcer -- is the divergent one.

    `annots` gates only the annotation half: those counters are always zero
    outside `--annots` mode, so consulting them there would be noise. The
    image colour-space half is unconditional, because a Lab image or a
    /Separation /None image is ordinary page content that arrives in every
    mode. Gating BOTH halves on `--annots` was the original defect: two keys
    added specifically to explain lab.pdf were never read, and the page kept
    reporting `unexplained-divergence` in the default mode.
    """
    keys = dict(IMAGE_REF_KEYS)
    if annots:
        keys.update(REF_KEYS)
    return [label for key, label in keys.items() if diag.get(key, 0) > 0]


def collect_pdfs(root: Path) -> list[tuple[str, Path]]:
    """Every *.pdf under root, sorted, as (relpath, abspath). Skips dotdirs
    (e.g. the corpus's own `.git`)."""
    out = []
    for p in sorted(root.rglob("*.pdf")):
        if any(part.startswith(".") for part in p.relative_to(root).parts):
            continue
        out.append((p.relative_to(root).as_posix(), p))
    # also .PDF on case-sensitive fs
    for p in sorted(root.rglob("*.PDF")):
        rel = p.relative_to(root).as_posix()
        if rel not in {r for r, _ in out} and not any(
            part.startswith(".") for part in p.relative_to(root).parts
        ):
            out.append((rel, p))
    out.sort()
    return out


# --- comparability: corpus fingerprint + stale-baseline guard --------------

def corpus_fingerprint(files: list[tuple[str, Path]], roots: list[Path]) -> dict:
    """Identify the exact corpus a report measured.

    WHY: `out/summary.json` doubles as the *recorded baseline* the gate
    compares new runs against (README §8), and bucket counts are only
    meaningful between runs over the SAME set of files. This corpus has
    already grown 2,914 -> 4,023 files; "unexplained went from 1 to 6" across
    that gap is not a regression signal, it is an arithmetic artefact of
    measuring 1,109 additional files. Without a fingerprint the two numbers
    look directly comparable, and nothing in the report says otherwise.

    The digest is over the sorted list of *relative* paths, not file contents:
    it must be cheap enough to compute on every run (a content hash of ~4,000
    PDFs is not), and it answers precisely the question that matters — "is
    this the same population of files?". A file whose CONTENT changed under a
    stable name is not caught; that is stated here rather than pretended away,
    and `--rebaseline` remains a deliberate operator act either way.
    """
    h = hashlib.sha256()
    for rel, _ in files:
        h.update(rel.encode("utf-8"))
        h.update(b"\n")
    return {
        "n_files": len(files),
        "sha256_of_sorted_relpaths": h.hexdigest(),
        "roots": [str(r) for r in roots],
    }


def comparability_config(args: argparse.Namespace) -> dict:
    """The subset of settings that changes the NUMBERS, so a baseline taken at
    other settings is flagged as incomparable too.

    DPI changes the raster size and therefore the anti-aliasing fraction;
    `pages_per_file` changes which and how many pages enter every distribution;
    `annots` switches the ANNOTATION reference-divergence confounder on (the
    image colour-space one is unconditional); the band settings
    define the bucket boundary itself. `--emit-diffs`, `--out`, `--max-files`
    and the timeouts do not change a measured value and are excluded (except
    `max_files`, which changes the POPULATION and so is folded into the
    fingerprint's file count instead).
    """
    return {
        "dpi": args.dpi,
        "pages_per_file_cap": args.pages_per_file,
        "annots": bool(args.annots),
        "band": args.band,
        "band_pct": args.band_pct,
        "pixel_delta_threshold": PIXEL_DELTA_T,
    }


def baseline_comparability(prior: dict | None, fp: dict, cfg: dict) -> dict:
    """Decide whether a prior report may be compared to the run about to start.

    Returns `{"comparable": bool, "reasons": [...], "prior": {...}}`.
    `comparable` is TRUE only when a fingerprint exists on both sides, matches,
    and the comparability config matches. Absence of a fingerprint is NOT
    treated as "probably fine" — a report written before fingerprinting cannot
    prove what it measured, so it is incomparable by default. That asymmetry is
    deliberate: the failure this guard exists to prevent is a false
    equivalence, and the safe default for a false equivalence is to refuse it.
    """
    if prior is None:
        return {"comparable": False, "reasons": ["no prior report in this output dir"],
                "prior": None, "kind": "none"}

    reasons: list[str] = []
    p_fp = prior.get("corpus_fingerprint")
    p_files = (prior.get("corpus") or {}).get("files_seen")
    if not p_fp:
        reasons.append(
            f"prior report predates corpus fingerprinting: it cannot prove which files it "
            f"measured. It records files_seen={p_files}; this run sees {fp['n_files']}."
        )
    else:
        if p_fp.get("n_files") != fp["n_files"]:
            reasons.append(
                f"corpus SIZE changed: baseline {p_fp.get('n_files')} files -> "
                f"now {fp['n_files']} files ({fp['n_files'] - (p_fp.get('n_files') or 0):+d})"
            )
        if p_fp.get("sha256_of_sorted_relpaths") != fp["sha256_of_sorted_relpaths"]:
            reasons.append(
                "corpus CONTENT changed: the sorted relative-path digest differs "
                f"({str(p_fp.get('sha256_of_sorted_relpaths'))[:12]}... -> "
                f"{fp['sha256_of_sorted_relpaths'][:12]}...)"
            )
    p_cfg = prior.get("comparability_config")
    if p_cfg is None:
        # Fall back to the legacy `config` block older reports carried.
        legacy = prior.get("config") or {}
        p_cfg = {
            "dpi": legacy.get("dpi"),
            "pages_per_file_cap": legacy.get("pages_per_file_cap"),
            "annots": legacy.get("annots"),
            "band": None,
            "band_pct": (prior.get("band") or {}).get("percentile"),
            "pixel_delta_threshold": legacy.get("pixel_delta_threshold"),
        }
    for k, v in cfg.items():
        if p_cfg.get(k) != v:
            reasons.append(f"setting {k}: baseline {p_cfg.get(k)!r} -> now {v!r}")

    return {
        "comparable": not reasons,
        "reasons": reasons,
        "prior": {
            "files_seen": p_files,
            "buckets": prior.get("buckets"),
            "unexplained_total": prior.get("unexplained_total"),
            "band": (prior.get("band") or {}).get("frac_over_32"),
            "run_complete": (prior.get("run") or {}).get("complete"),
        },
        "kind": "mismatch",
    }


def stale_baseline_banner(cmp_: dict, outdir: Path, rebaseline: bool) -> list[str]:
    """The loud text. Printed to stderr AND embedded verbatim in summary.txt.

    Being loud is the deliverable (task item 4): a mismatch that is recorded
    only in a JSON field nobody reads is the same failure as not recording it.
    """
    p = cmp_.get("prior") or {}
    L = [
        "!" * 78,
        "!! STALE BASELINE — the recorded report in this output dir is NOT COMPARABLE",
        "!" * 78,
        f"   baseline file : {outdir / 'summary.json'}",
        f"   baseline saw  : {p.get('files_seen')} files, buckets={p.get('buckets')}, "
        f"unexplained={p.get('unexplained_total')}",
        "   why not comparable:",
    ]
    L += [f"     - {r}" for r in cmp_["reasons"]]
    L += [
        "",
        "   Bucket counts from different corpora are NOT a regression signal. A rise in",
        "   `unexplained` across a corpus-size change measures the new files, not a",
        "   change in pdfcer. Do not diff these two numbers.",
        "",
    ]
    if rebaseline:
        L += ["   --rebaseline was passed: the old report is being ARCHIVED (not deleted)",
              "   alongside the new one, and the new one becomes the baseline.", ""]
    else:
        L += [
            "   This run will NOT overwrite the baseline. Choose one:",
            "     * write elsewhere and keep the baseline intact:",
            "         --out tools/render-parity/out-<label>",
            "     * deliberately re-record the baseline for the current corpus:",
            "         --rebaseline        (archives the old report first)",
            "",
        ]
    L.append("!" * 78)
    return L


def choose_pages(n_pages: int, cap: int) -> list[int]:
    """1-based page indices to sample. cap<=0 means all pages; otherwise
    sample first, last, and evenly spaced interior pages up to `cap`."""
    if n_pages <= 0:
        return []
    if cap <= 0 or n_pages <= cap:
        return list(range(1, n_pages + 1))
    if cap == 1:
        return [1]
    idxs = {1, n_pages}
    # evenly spaced fill
    step = (n_pages - 1) / (cap - 1)
    for k in range(cap):
        idxs.add(1 + round(k * step))
    return sorted(i for i in idxs if 1 <= i <= n_pages)[:cap]


def amplify_delta_panel(
    pdfcer: np.ndarray, pdfium_img: np.ndarray, delta: np.ndarray
) -> Image.Image:
    """Build a [pdfcer | pdfium | 8x-amplified delta] side-by-side panel."""
    h = min(pdfcer.shape[0], pdfium_img.shape[0], delta.shape[0])
    w = min(pdfcer.shape[1], pdfium_img.shape[1], delta.shape[1])
    a = pdfcer[:h, :w, :]
    b = pdfium_img[:h, :w, :]
    d = np.clip(delta[:h, :w].astype(np.int32) * 8, 0, 255).astype(np.uint8)
    dmap = np.stack([d, d, d], axis=2)  # grey heatmap; brighter = larger delta
    gap = np.full((h, 8, 3), 200, dtype=np.uint8)
    panel = np.concatenate([a, gap, b, gap, dmap], axis=1)
    return Image.fromarray(panel, "RGB")


class TsvSink:
    """Streams per-page rows to disk AS THEY ARE MEASURED.

    WHY: the final `per-page.tsv` cannot be written until the sweep ends,
    because the `bucket` column depends on the benign band, which is a
    percentile over the whole clean-by-construction population. That is fine
    for an orderly finish and useless for a hard kill. So this sink writes an
    unbucketed copy (`per-page.partial.tsv`), flushed after every row, whose
    only job is to survive a process that stops existing. A completed run
    deletes it; a run that did not complete leaves it as the evidence.
    """

    HEADER = (
        "file\tpage\tstatus\tw\th\tmean\tp95\tdmax\t"
        "frac16\tfrac32\tfrac64\tdim_mismatch\tclean\tdevicecmyk\tgaps\trefdiv\treason\n"
    )

    def __init__(self, path: Path) -> None:
        self.path = path
        self.fh = path.open("w", encoding="utf-8", newline="\n")
        self.fh.write(self.HEADER)
        self.fh.flush()

    def write(self, r: PageResult) -> None:
        self.fh.write(
            f"{r.rel}\t{r.page}\t{r.status}\t{r.w}\t{r.h}\t{r.mean:.3f}\t{r.p95:.1f}\t"
            f"{r.dmax}\t{r.frac16:.5f}\t{r.frac32:.5f}\t{r.frac64:.5f}\t{r.dim_mismatch}\t"
            f"{r.clean}\t{r.devicecmyk}\t{r.gaps}\t{r.refdiv}\t{r.reason}\n"
        )
        self.fh.flush()

    def close(self) -> None:
        try:
            self.fh.close()
        except Exception:  # noqa: BLE001
            pass


def run(args: argparse.Namespace) -> int:
    # Corpus filenames + spec section marks (§) can carry non-cp1252 chars;
    # force UTF-8 on the console so a print never dies on a Windows codepage.
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    # `--cli` pins the measured binary. Rebinding the module global (rather
    # than threading a path through every call site) keeps `render_pdfce`'s
    # signature stable; the resolved path is recorded in the report so a run
    # can always say WHICH build it measured.
    global CLI
    if args.cli:
        CLI = Path(args.cli).resolve()
    if not CLI.exists():
        print(f"ERROR: pdfcer not found at {CLI}\n"
              f"       build it first: cargo build --release -p pdfcer-cli", file=sys.stderr)
        return 2
    if not WORKER.exists():
        print(f"ERROR: reference-renderer worker missing: {WORKER}", file=sys.stderr)
        return 2

    corpus_dirs = [Path(d) for d in args.corpus] or [DEFAULT_CORPUS]
    files: list[tuple[str, Path]] = []
    for d in corpus_dirs:
        if not d.exists():
            print(f"ERROR: corpus dir not found: {d}", file=sys.stderr)
            return 2
        prefix = d.name
        for rel, abs_ in collect_pdfs(d):
            files.append((f"{prefix}/{rel}", abs_))
    files.sort()
    if args.max_files > 0:
        files = files[: args.max_files]

    # Optional single-page diff request (demo / triage): resolve the file.
    diff_target = None
    if args.diff:
        for rel, abs_ in files:
            if args.diff in rel:
                diff_target = (rel, abs_, args.diff_page)
                break
        if diff_target is None:
            print(f"ERROR: --diff file not found in corpus: {args.diff}", file=sys.stderr)
            return 2

    scale = args.dpi / 72.0
    outdir = Path(args.out)
    (outdir / "diffs").mkdir(parents=True, exist_ok=True)

    # ---- Phase 0: the stale-baseline guard, BEFORE any rendering.
    # Running it first is the point: a mismatch that is only discovered when
    # the report is written has already cost the operator the entire sweep.
    fp = corpus_fingerprint(files, corpus_dirs)
    cfg = comparability_config(args)
    prior = None
    prior_path = outdir / "summary.json"
    if prior_path.exists():
        try:
            prior = json.loads(prior_path.read_text(encoding="utf-8"))
        except Exception as exc:  # noqa: BLE001
            print(f"WARNING: unreadable prior report {prior_path}: {exc}", file=sys.stderr)
    cmp_ = baseline_comparability(prior, fp, cfg)
    banner: list[str] = []
    if prior is not None and not cmp_["comparable"]:
        banner = stale_baseline_banner(cmp_, outdir, args.rebaseline)
        print("\n".join(banner), file=sys.stderr)
        if not args.rebaseline:
            # Exit 3 — distinct from 1 (gate FAIL) and 2 (setup error), so a
            # script can tell "your baseline is stale" from "pdfcer regressed".
            return 3
        archive = outdir / (
            f"summary.superseded-{(prior.get('corpus') or {}).get('files_seen', 'x')}"
            f"files-{time.strftime('%Y%m%d-%H%M%S')}.json"
        )
        shutil.copy2(prior_path, archive)
        print(f"  archived prior baseline -> {archive.name}", file=sys.stderr)

    results: list[PageResult] = []
    retained: list[PageResult] = []
    retain_cap = max(args.emit_diffs * 3, 48)
    n_files_ok = 0
    n_files_done = 0
    run_complete = True
    stop_reason = ""
    t0 = time.time()
    print(
        f"render-parity: {len(files)} files, dpi={args.dpi} (scale {scale:.4f}), "
        f"pages/file cap={args.pages_per_file}, annots={'on' if args.annots else 'off'}, "
        f"pdfium=child-process (crash-isolated)",
        file=sys.stderr,
    )

    sink = TsvSink(outdir / "per-page.partial.tsv")
    worker = PdfiumWorker(timeout=args.pdfium_timeout)
    try:
        with tempfile.TemporaryDirectory() as td:
            tmp = Path(td)
            for fi, (rel, abs_) in enumerate(files):
                if fi % 200 == 0:
                    el = time.time() - t0
                    print(f"  [{fi}/{len(files)}] {el:6.0f}s {rel}", file=sys.stderr)
                if args.checkpoint_every > 0 and fi and fi % args.checkpoint_every == 0:
                    write_progress(outdir, results, fi, len(files), t0, worker)
                n_files_done = fi + 1

                # -- page count: the request that the KNOWN abort fires on.
                try:
                    npages = worker.page_count(abs_)
                except ReferenceAborted as exc:
                    pr = PageResult(rel, 0, "abort",
                                    f"pdfium-abort@open: {exc.cause}")
                    results.append(pr)
                    sink.write(pr)
                    print(f"    ABORT  {rel}: pdfium died at open ({exc.cause}); "
                          f"worker respawned", file=sys.stderr)
                    continue
                except ReferenceTimeout as exc:
                    pr = PageResult(rel, 0, "abort", f"pdfium-hang@open: {exc}")
                    results.append(pr)
                    sink.write(pr)
                    print(f"    HANG   {rel}: pdfium wedged at open; worker killed",
                          file=sys.stderr)
                    continue
                except ReferenceFailed as exc:
                    pr = PageResult(rel, 0, "skip", f"pdfium-open: {str(exc)[:80]}")
                    results.append(pr)
                    sink.write(pr)
                    continue

                dcmyk = devicecmyk_in_file(abs_)
                pages = choose_pages(npages, args.pages_per_file)
                file_ok = False
                for pg in pages:
                    pr = measure_page(rel, abs_, pg, scale, args, tmp, dcmyk, worker)
                    results.append(pr)
                    sink.write(pr)
                    file_ok = file_ok or pr.status == "ok"
                    # Bound retained rasters to the worst-N by frac32 so a full
                    # corpus sweep cannot exhaust memory (each retained page holds
                    # three full-page arrays, ~10-15 MB at 125 DPI). Evict the
                    # least-divergent when over the cap.
                    if pr._arrays is not None:
                        retained.append(pr)
                        if len(retained) > retain_cap:
                            victim = min(retained, key=lambda r: r.frac32)
                            victim._arrays = None
                            # identity filter — PageResult eq is value-based, so
                            # `.remove` could drop a different equal-metric page.
                            retained = [r for r in retained if r is not victim]
                if file_ok:
                    n_files_ok += 1
    except KeyboardInterrupt:
        run_complete = False
        stop_reason = f"interrupted (Ctrl-C) after {n_files_done}/{len(files)} files"
        print(f"\n!! {stop_reason} — reporting on what was measured", file=sys.stderr)
    except BaseException as exc:  # noqa: BLE001
        # Deliberately broad. A sweep this long must never convert a
        # parent-side surprise into "you get nothing"; the whole reporting
        # pipeline still runs below, stamped incomplete.
        run_complete = False
        stop_reason = (
            f"{type(exc).__name__}: {str(exc)[:200]} "
            f"(after {n_files_done}/{len(files)} files)"
        )
        print(f"\n!! sweep aborted: {stop_reason} — reporting on what was measured",
              file=sys.stderr)
    finally:
        sink.close()
        worker.close()
        worker.kill()

    elapsed = time.time() - t0
    ok = [r for r in results if r.status == "ok"]
    if not ok:
        print("no pages measured (all skipped/aborted) — is the corpus present?",
              file=sys.stderr)
        return 2

    # ---- Phase 1b: confirm every abort reproduces in a dedicated child.
    aborted = [r for r in results if r.status == "abort"]
    abort_records: list[dict] = []
    if aborted and args.verify_aborts:
        print(f"verifying {len(aborted)} reference-renderer abort(s) in isolation...",
              file=sys.stderr)
    for r in aborted:
        rec = {"file": r.rel, "observed": r.reason}
        if args.verify_aborts:
            abs_ = dict(files).get(r.rel)
            if abs_ is not None:
                rec.update(probe_abort_in_isolation(
                    abs_, max(r.page - 1, 0), scale, args.annots, args.pdfium_timeout))
        else:
            rec["note"] = "not verified (--no-verify-aborts)"
        abort_records.append(rec)

    # ---- Phase 2: derive the benign band from clean-by-construction pages.
    clean = [r for r in ok if r.clean]
    clean_frac = np.array([r.frac32 for r in clean]) if clean else np.array([0.0])
    if args.band is not None:
        band = args.band
        band_src = f"explicit --band {band}"
    else:
        band = float(np.percentile(clean_frac, args.band_pct))
        band_src = (
            f"p{args.band_pct:g} of frac_over_32 over {len(clean)} "
            f"clean-by-construction pages"
        )

    # ---- Phase 3: classify.
    #
    # THE GAP TEST COMES FIRST. It used to come after the band, and the
    # ordering was not a detail: 1,656 of 3,668 pages in the old "benign"
    # bucket (45.2%) were pages where pdfcer ITSELF disclosed a gap —
    # 1,032 deferred-op, 573 font-substituted, 139 font-unsupported, and
    # so on. The worst single case was a page pdfcer rendered COMPLETELY
    # BLANK (`pdfbox/.../merge/multitiff.pdf`, `image-unsupported`
    # disclosed): frac32 0.0798 under a band of 0.0882, so it was filed
    # as renderer noise.
    #
    # A disclosed gap is an EXPLANATION, and an explanation should not be
    # discarded because the divergence it explains happens to be small.
    # Scoring below the band tells you the difference is small; it does
    # not tell you the difference is anti-aliasing.
    #
    # The name changed with the ordering. "benign-renderer-noise"
    # asserted a CAUSE from a THRESHOLD, and the structural audit of
    # 2026-08-09 measured how often that assertion is wrong: 15.4% of the
    # bucket contains a contiguous over-threshold region no shared edge
    # explains, and four confirmed pdfcer bugs were living in it. The
    # buckets are now named for what is actually measured.
    for r in ok:
        if r.refdiv:
            r.bucket = "reference-divergence"
        elif r.gaps:
            # Below the band as well? Still a disclosed gap — but worth
            # separating, because "small AND explained" is a very
            # different follow-up from "large AND explained".
            r.bucket = "disclosed-gap-small" if r.frac32 <= band else "disclosed-gap"
        elif r.frac32 <= band:
            r.bucket = "below-band"
        else:
            r.bucket = "unexplained"
    for r in results:
        if r.status == "abort":
            r.bucket = "reference-aborted"

    meta = {
        "fingerprint": fp, "cfg": cfg, "cmp": cmp_, "banner": banner,
        "complete": run_complete, "stop_reason": stop_reason,
        "files_attempted": n_files_done, "elapsed_s": elapsed,
        "worker_spawns": worker.spawns, "aborts": abort_records,
    }
    write_reports(results, ok, clean, band, band_src, n_files_ok, len(files),
                  args, outdir, scale, meta)

    # ---- Diff panels: the top unexplained pages + any explicit --diff.
    emit_diff_panels(ok, diff_target, args, outdir, scale, tmp_reuse=None)

    # A complete run's streamed copy is redundant with per-page.tsv; an
    # incomplete run's is the evidence, so it is kept.
    if run_complete:
        (outdir / "per-page.partial.tsv").unlink(missing_ok=True)
        (outdir / "progress.json").unlink(missing_ok=True)

    unexplained = [r for r in ok if r.bucket == "unexplained"]
    if args.gate:
        n = len(unexplained)
        verdict = "PASS" if n <= args.max_unexplained else "FAIL"
        if not run_complete:
            # A partial sweep can only UNDER-count unexplained pages, so a
            # "PASS" from one is not a pass. Refusing to issue a verdict is
            # the honest outcome; exit 3 = "cannot adjudicate".
            print(f"\nGATE: INDETERMINATE — sweep did not complete ({stop_reason}); "
                  f"{n} unexplained seen so far, but the unmeasured remainder could "
                  f"hold more. Re-run to completion before trusting a verdict.")
            return 3
        print(f"\nGATE: {verdict} — {n} unexplained (max {args.max_unexplained})")
        return 0 if n <= args.max_unexplained else 1
    return 0 if run_complete else 3


def write_progress(outdir: Path, results: list[PageResult], fi: int, n: int,
                   t0: float, worker: PdfiumWorker) -> None:
    """Checkpoint the sweep's shape mid-flight.

    Cheap (a few counters, no percentiles) so it can run often, and enough to
    answer "how far did it get and what has it hit?" from a run that later
    dies without reaching the reporting phase.
    """
    counts: dict[str, int] = {}
    for r in results:
        counts[r.status] = counts.get(r.status, 0) + 1
    el = time.time() - t0
    (outdir / "progress.json").write_text(json.dumps({
        "files_done": fi, "files_total": n,
        "elapsed_s": round(el, 1),
        "eta_s": round(el / fi * (n - fi), 1) if fi else None,
        "rows": len(results), "by_status": counts,
        "pdfium_worker_spawns": worker.spawns,
        "aborted_files": [r.rel for r in results if r.status == "abort"],
    }, indent=2), encoding="utf-8")


def measure_page(
    rel: str, abs_: Path, pg: int, scale: float, args, tmp: Path, dcmyk: bool,
    worker: PdfiumWorker,
) -> PageResult:
    """Render one page in both engines, compute metrics, tag gaps. Never
    raises — any failure becomes a counted skip or abort (acceptance: zero
    panics, and — since the reference renderer now lives in a child — zero
    process deaths."""
    try:
        pdfcer_img, diag = render_pdfce(abs_, pg, scale, args.annots, tmp, args.timeout)
    except subprocess.TimeoutExpired:
        return PageResult(rel, pg, "skip", "pdfcer-timeout")
    except Exception as e:  # noqa: BLE001
        return PageResult(rel, pg, "skip", f"pdfcer: {str(e)[:80]}")
    try:
        pdfium_img = worker.render(abs_, pg - 1, scale, args.annots, tmp / "pdfium.raw")
    except ReferenceAborted as exc:
        return PageResult(rel, pg, "abort", f"pdfium-abort@render: {exc.cause}")
    except ReferenceTimeout as exc:
        return PageResult(rel, pg, "abort", f"pdfium-hang@render: {exc}")
    except Exception as e:  # noqa: BLE001
        return PageResult(rel, pg, "skip", f"pdfium: {str(e)[:80]}")

    delta, stats, dim_mismatch = compare(pdfcer_img, pdfium_img)
    gaps = gap_reasons(diag)
    refs = ref_reasons(diag, args.annots)
    if dcmyk:
        gaps = gaps + ["devicecmyk-file"] if "devicecmyk-jpeg" not in gaps else gaps
    # `tolerated` disqualifies a page from DEFINING the band.
    #
    # "Clean by construction" means pdfcer claims to have rendered the page
    # fully, so whatever it diverges by can only be renderer noise. A page
    # that tolerated a structural oddity is making the opposite claim: it
    # says pdfcer hit something it could not act on. Such a page may still
    # be fine, but it cannot be part of the population that DEFINES what
    # "fine" looks like.
    #
    # Deliberately NOT added to `gaps`. `tolerated` is a ~30-site catch-all
    # that conflates "unbalanced Q" (harmless) with "gs was a no-op"
    # (which was hiding a real bug until 2026-08-09), so treating it as an
    # explanation would excuse divergences it does not explain. Excluding
    # it from the band is the conservative half — it can only make the
    # band tighter and bug candidates easier to see — and splitting the
    # counter into content-affecting vs structural is the owed follow-up.
    tolerated = int(diag.get("tolerated", 0) or 0)
    clean = int(
        not gaps and not dcmyk and not refs and not dim_mismatch and not tolerated
    )

    pr = PageResult(
        rel=rel, page=pg, status="ok",
        w=min(pdfcer_img.shape[1], pdfium_img.shape[1]),
        h=min(pdfcer_img.shape[0], pdfium_img.shape[0]),
        mean=stats["mean"], p95=stats["p95"], dmax=stats["dmax"],
        frac16=stats["frac16"], frac32=stats["frac32"], frac64=stats["frac64"],
        dim_mismatch=dim_mismatch, clean=clean, devicecmyk=int(dcmyk),
        gaps=",".join(gaps), refdiv=",".join(refs),
    )
    # Retain arrays only for the worst pages so we can emit diff panels
    # without re-rendering; bounded by keeping them light (frac32 gate).
    if args.emit_diffs > 0 and stats["frac32"] > 0.001:
        pr._arrays = (pdfcer_img, pdfium_img, delta)
    return pr


def emit_diff_panels(ok, diff_target, args, outdir, scale, tmp_reuse) -> None:
    # Explicit --diff request: render fresh (may not be in the retained set).
    if diff_target is not None:
        rel, abs_, pg = diff_target
        w = PdfiumWorker(timeout=args.pdfium_timeout)
        try:
            with tempfile.TemporaryDirectory() as td:
                pdfcer_img, _ = render_pdfce(abs_, pg, scale, args.annots, Path(td), args.timeout)
                pdfium_img = w.render(abs_, pg - 1, scale, args.annots, Path(td) / "d.raw")
            delta, _, _ = compare(pdfcer_img, pdfium_img)
            panel = amplify_delta_panel(pdfcer_img, pdfium_img, delta)
            name = rel.replace("/", "_").replace("\\", "_")
            panel.save(outdir / "diffs" / f"DIFF_{name}_p{pg}.png")
            print(f"wrote diff panel: diffs/DIFF_{name}_p{pg}.png", file=sys.stderr)
        except ReferenceAborted as e:
            print(f"--diff: the REFERENCE renderer aborted on this file ({e.cause}); "
                  f"no panel is possible — there is nothing to compare against.",
                  file=sys.stderr)
        except Exception as e:  # noqa: BLE001
            print(f"--diff render failed: {e}", file=sys.stderr)
        finally:
            w.kill()

    if args.emit_diffs <= 0:
        return
    # Top unexplained (then top overall) by frac32, among pages whose arrays
    # we retained.
    have = [r for r in ok if r._arrays is not None]
    unexp = sorted(
        [r for r in have if r.bucket == "unexplained"], key=lambda r: -r.frac32
    )
    rest = sorted(
        [r for r in have if r.bucket != "unexplained"], key=lambda r: -r.frac32
    )
    chosen = (unexp + rest)[: args.emit_diffs]
    for r in chosen:
        pdfcer_img, pdfium_img, delta = r._arrays
        panel = amplify_delta_panel(pdfcer_img, pdfium_img, delta)
        name = r.rel.replace("/", "_").replace("\\", "_")
        panel.save(outdir / "diffs" / f"{r.bucket}_{r.frac32:.4f}_{name}_p{r.page}.png")
    if chosen:
        print(f"wrote {len(chosen)} diff panels to diffs/", file=sys.stderr)


def _distribution(vals: list[float]) -> dict:
    if not vals:
        return {"n": 0}
    a = np.array(vals)
    return {
        "n": len(vals),
        "mean": float(a.mean()),
        "p50": float(np.percentile(a, 50)),
        "p95": float(np.percentile(a, 95)),
        "p99": float(np.percentile(a, 99)),
        "max": float(a.max()),
    }


def write_reports(results, ok, clean, band, band_src, n_files_ok, n_files, args, outdir,
                  scale, meta) -> None:
    # per-page TSV (all rows, deterministic order already).
    tsv = outdir / "per-page.tsv"
    with tsv.open("w", encoding="utf-8", newline="\n") as f:
        f.write(
            "file\tpage\tstatus\tbucket\tw\th\tmean\tp95\tdmax\t"
            "frac16\tfrac32\tfrac64\tdim_mismatch\tclean\tdevicecmyk\tgaps\trefdiv\treason\n"
        )
        for r in results:
            f.write(
                f"{r.rel}\t{r.page}\t{r.status}\t{r.bucket}\t{r.w}\t{r.h}\t"
                f"{r.mean:.3f}\t{r.p95:.1f}\t{r.dmax}\t{r.frac16:.5f}\t{r.frac32:.5f}\t"
                f"{r.frac64:.5f}\t{r.dim_mismatch}\t{r.clean}\t{r.devicecmyk}\t"
                f"{r.gaps}\t{r.refdiv}\t{r.reason}\n"
            )

    buckets = {
        "below-band": 0,
        "disclosed-gap-small": 0,
        "disclosed-gap": 0,
        "unexplained": 0,
        "reference-divergence": 0,
    }
    for r in ok:
        buckets[r.bucket] = buckets.get(r.bucket, 0) + 1
    skipped = [r for r in results if r.status == "skip"]
    aborted = [r for r in results if r.status == "abort"]
    # `reference-aborted` is reported ALONGSIDE the three pdfcer buckets, never
    # inside them: a file the reference renderer died on yielded no comparison,
    # so it is evidence about pdfium, not about pdfcer. Folding it into `skip`
    # (the pre-isolation behaviour would have been to not get here at all)
    # would bury a reference-tool crash in a histogram of malformed fixtures.
    buckets["reference-aborted"] = len(aborted)

    # DeviceCMYK characterization: pages that are DeviceCMYK AND have NO other
    # gap (so the divergence is attributable to colorimetry), vs clean pages.
    dcmyk_only = [
        r for r in ok
        if r.devicecmyk and not any(
            g for g in r.gaps.split(",") if g and g not in ("devicecmyk-file", "devicecmyk-jpeg")
        )
    ]
    clean_frac = [r.frac32 for r in clean]
    dcmyk_frac = [r.frac32 for r in dcmyk_only]

    # unexplained enumerated by file+reason (R20), sorted worst-first.
    unexplained = sorted([r for r in ok if r.bucket == "unexplained"], key=lambda r: -r.frac32)
    # gap reasons histogram
    gap_hist: dict[str, int] = {}
    for r in ok:
        if r.bucket in ("disclosed-gap", "disclosed-gap-small"):
            for g in r.gaps.split(","):
                if g:
                    gap_hist[g] = gap_hist.get(g, 0) + 1
    skip_hist: dict[str, int] = {}
    for r in skipped:
        key = r.reason.split(":")[0]
        skip_hist[key] = skip_hist.get(key, 0) + 1

    abort_hist: dict[str, int] = {}
    for r in aborted:
        # e.g. "pdfium-abort@open: exit 0x80000003 (STATUS_BREAKPOINT)"
        abort_hist[r.reason] = abort_hist.get(r.reason, 0) + 1

    summary = {
        # `run` first, so `complete: false` is the first thing any reader —
        # human or LLM — sees. A partial sweep's numbers are floors, not
        # totals, and that has to be unmissable.
        "run": {
            "complete": meta["complete"],
            "stop_reason": meta["stop_reason"],
            "files_attempted": meta["files_attempted"],
            "files_total": n_files,
            "elapsed_s": round(meta["elapsed_s"], 1),
            "pdfium_worker_spawns": meta["worker_spawns"],
        },
        "corpus_fingerprint": meta["fingerprint"],
        "comparability_config": meta["cfg"],
        "baseline_comparison": {
            "comparable_to_prior_report_in_this_dir": meta["cmp"]["comparable"],
            "reasons_not_comparable": meta["cmp"]["reasons"],
            "prior": meta["cmp"]["prior"],
        },
        "config": {
            "dpi": args.dpi, "scale": round(scale, 6),
            "pages_per_file_cap": args.pages_per_file,
            "annots": args.annots, "pixel_delta_threshold": PIXEL_DELTA_T,
            "pdfcer_cli": str(CLI),
        },
        "corpus": {
            "files_seen": n_files, "files_with_a_measured_page": n_files_ok,
            "pages_measured": len(ok), "pages_skipped": len(skipped),
            "files_reference_aborted": len(aborted),
        },
        "band": {"frac_over_32": band, "source": band_src, "percentile": args.band_pct},
        "buckets": buckets,
        "reference_aborted": {
            "count": len(aborted),
            "histogram": dict(sorted(abort_hist.items(), key=lambda kv: -kv[1])),
            "files": meta["aborts"],
        },
        "distribution_frac32": {
            "all_measured": _distribution([r.frac32 for r in ok]),
            "clean_by_construction": _distribution(clean_frac),
            "devicecmyk_only": _distribution(dcmyk_frac),
        },
        "gap_histogram": dict(sorted(gap_hist.items(), key=lambda kv: -kv[1])),
        "skip_histogram": dict(sorted(skip_hist.items(), key=lambda kv: -kv[1])),
        "unexplained_top": [
            {"file": r.rel, "page": r.page, "frac32": round(r.frac32, 5),
             "p95": r.p95, "dmax": r.dmax, "dim_mismatch": r.dim_mismatch}
            for r in unexplained[:50]
        ],
        "unexplained_total": len(unexplained),
    }
    (outdir / "summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")

    # human/LLM-readable summary
    lines: list[str] = []
    P = lines.append
    P("=== render-parity -- full-page pdfium pixel-parity (Pass 11) ===")
    if not meta["complete"]:
        # First thing in the file, before any number, because every number
        # below it is a floor rather than a total.
        P("*" * 78)
        P("** PARTIAL RUN -- THIS IS NOT A FULL SWEEP")
        P(f"** {meta['stop_reason']}")
        P(f"** {meta['files_attempted']} of {n_files} files attempted. Every count below is")
        P("** a LOWER BOUND. Do not compare it to a complete run, and do not read a")
        P("** low `unexplained` count as a pass.")
        P("*" * 78)
        P("")
    if meta["banner"]:
        P("\n".join(meta["banner"]))
        P("")
    P(f"config: dpi={args.dpi} scale={scale:.4f} pages/file<= {args.pages_per_file} "
      f"annots={'on' if args.annots else 'off'} pixel-delta-T={PIXEL_DELTA_T}")
    P(f"corpus: {n_files} files, {n_files_ok} with a measured page, "
      f"{len(ok)} pages measured, {len(skipped)} pages skipped, "
      f"{len(aborted)} reference-aborted")
    P(f"corpus fingerprint: {meta['fingerprint']['sha256_of_sorted_relpaths'][:16]}... "
      f"over {meta['fingerprint']['n_files']} files   "
      f"(compare bucket counts ONLY against a report with this same fingerprint)")
    P(f"elapsed: {meta['elapsed_s']:.0f}s   pdfium worker spawns: {meta['worker_spawns']} "
      f"(1 = never died)")
    P("")
    P("--- tolerance band (empirical, NOT tuned -- decision 010 Y1/W14) ---")
    P(f"band(frac_over_32) = {band:.6f}")
    P(f"  source: {band_src}")
    P(f"  a page is BENIGN iff frac_over_32 <= band; the band is a property of")
    P(f"  known-clean pages, so it cannot be tuned to pass a bug.")
    P("")
    P("--- frac_over_32 distribution ---")
    for label, vals in (
        ("all measured   ", [r.frac32 for r in ok]),
        ("clean-by-constr", clean_frac),
        ("devicecmyk-only", dcmyk_frac),
    ):
        d = _distribution(vals)
        if d["n"]:
            P(f"  {label}: n={d['n']:6d} mean={d['mean']:.5f} p50={d['p50']:.5f} "
              f"p95={d['p95']:.5f} p99={d['p99']:.5f} max={d['max']:.5f}")
        else:
            P(f"  {label}: n=0")
    P("")
    P("--- buckets (by file+reason, R20) ---")
    P(f"  (i)   below-band, nothing disclosed : {buckets['below-band']}")
    P(f"  (ii)  disclosed gap, above band     : {buckets['disclosed-gap']}")
    P(f"  (ii-) disclosed gap, below band     : {buckets['disclosed-gap-small']}")
    P(f"  (iii) unexplained-divergence        : {buckets['unexplained']}")
    P("        NOTE: (i) is a MEASUREMENT, not a verdict. It means the")
    P("        divergence is small and pdfcer disclosed nothing — not that")
    P("        it is anti-aliasing. The 2026-08-09 structural audit found")
    P("        15.4% of that population is not edge-shaped, and four")
    P("        confirmed pdfcer bugs were inside it.")
    # Printed UNCONDITIONALLY. It used to be gated on `--annots`, which was
    # harmless while only annotation keys fed it and actively misleading once
    # image colour-space keys did: a page moved out of `unexplained` into a
    # bucket the report did not print, so the headline number improved and
    # nothing on screen said where the page went. A bucket that can be
    # non-zero must always be visible, or the harness is hiding its own work.
    P(f"  (ref) reference-divergence  : {buckets['reference-divergence']}")
    P(f"  (abt) reference-aborted     : {buckets['reference-aborted']}   "
      f"<- pdfium died; not a pdfcer bucket")
    P("")
    P("--- known-gap reason histogram (subtracted from bug candidates) ---")
    for g, c in sorted(gap_hist.items(), key=lambda kv: -kv[1]):
        P(f"  {c:6d}  {g}")
    P("")
    P("--- DeviceCMYK colorimetry characterization (decision 006 sec3.7 / deliverable 7) ---")
    dd = _distribution(dcmyk_frac)
    cd = _distribution(clean_frac)
    # TWO independent populations, and they need TWO guards. `_distribution`
    # returns a bare `{"n": 0}` for an empty one, so reading `cd['mean']`
    # under a guard that only tested `dd['n']` raised KeyError the first time
    # a run had DeviceCMYK pages but no clean ones. That is not a contrived
    # combination: it is what a small, deliberately-targeted fixture corpus
    # looks like once every page is explained, and it took down the whole
    # report -- including the unexplained tail printed below it -- rather
    # than degrading one line.
    if dd["n"]:
        P(f"  DeviceCMYK-only pages: n={dd['n']} mean frac32={dd['mean']:.5f} "
          f"p95={dd['p95']:.5f} max={dd['max']:.5f}")
        if cd["n"]:
            P(f"  clean pages (baseline): n={cd['n']} mean frac32={cd['mean']:.5f} "
              f"p95={cd['p95']:.5f}")
            ratio = (dd["mean"] / cd["mean"]) if cd["mean"] else float("nan")
            P(f"  => DeviceCMYK pages diverge {ratio:.1f}x the clean-page mean "
              "(naive-additive Rgb::from_cmyk vs pdfium AdobeCMYK_to_sRGB1)")
        else:
            P("  clean pages (baseline): n=0 — NO baseline population in this")
            P("     run, so the DeviceCMYK figure above is an absolute number")
            P("     and NOT a multiple of anything. Do not quote it as a ratio.")
    else:
        P("  no DeviceCMYK-only pages in this run")
    P("")
    P("--- unexplained tail (bug candidates, worst-first, R20 by file+reason) ---")
    if not unexplained:
        P("  (none)")
    for r in unexplained[:40]:
        P(f"  frac32={r.frac32:.5f} p95={r.p95:.0f} dmax={r.dmax} dimMM={r.dim_mismatch} "
          f"{r.rel} p{r.page}")
    P("")
    P("--- skip histogram (out of scope: unloadable / geometry) ---")
    for k, c in sorted(skip_hist.items(), key=lambda kv: -kv[1]):
        P(f"  {c:6d}  {k}")
    P("")
    P("--- reference-renderer aborts (pdfium DIED; NOT a pdfcer result) ---")
    P("  These files tell us nothing about pdfcer: the reference renderer ceased to")
    P("  exist while opening or rendering them, so no comparison was possible. Each")
    P("  is isolated in a child process, so one costs one file, not the run.")
    if not aborted:
        P("  (none)")
    for rec in meta["aborts"]:
        v = rec.get("reproduced")
        mark = "CONFIRMED" if v else ("UNVERIFIED" if v is None else "NOT-REPRODUCED")
        P(f"  [{mark}] {rec['file']}")
        P(f"            observed: {rec['observed']}")
        if rec.get("cause"):
            P(f"            isolated: {rec['cause']}")
        if rec.get("note"):
            P(f"            note    : {rec['note'][:160]}")
    (outdir / "summary.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))


def build_argparser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description="full-page pdfium pixel-parity harness (Pass 11)")
    p.add_argument("corpus", nargs="*", help="corpus dir(s); default fixtures/external")
    p.add_argument("--dpi", type=float, default=150.0, help="render DPI (default 150)")
    p.add_argument("--pages-per-file", type=int, default=4,
                   help="max pages sampled per file (0 = all; default 4)")
    p.add_argument("--max-files", type=int, default=0, help="cap files (0 = all)")
    p.add_argument("--annots", action="store_true",
                   help="compare WITH annotations (default off = content-only oracle)")
    p.add_argument("--band", type=float, default=None,
                   help="explicit benign band on frac_over_32 (default: derive from clean p99)")
    p.add_argument("--band-pct", type=float, default=99.9,
                   help="percentile of clean pages defining the band (default 99.9). "
                        "Principle: the clean-by-construction population is benign in "
                        "full, so the band covers essentially all of it; the tiny "
                        "residual above is the bug-candidate set to triage. NOT chosen "
                        "to hit a target unexplained count (W14).")
    p.add_argument("--emit-diffs", type=int, default=8,
                   help="write N diff panels for worst pages (default 8)")
    p.add_argument("--diff", type=str, default=None,
                   help="substring of a corpus file to emit a diff panel for")
    p.add_argument("--diff-page", type=int, default=1, help="page for --diff (1-based)")
    p.add_argument("--timeout", type=float, default=120.0,
                   help="per-page pdfcer render timeout seconds (default 120)")
    p.add_argument("--pdfium-timeout", type=float, default=120.0,
                   help="per-request reference-renderer timeout seconds (default 120). "
                        "A wedged child cannot report its own wedge, so this is the "
                        "only place a pdfium hang can be caught; on expiry the worker "
                        "is killed, the file is bucketed reference-aborted, and the "
                        "worker respawns for the next file.")
    p.add_argument("--checkpoint-every", type=int, default=100,
                   help="write out/progress.json every N files (0 = never; default 100). "
                        "Crash-survival: per-page.partial.tsv is streamed regardless.")
    p.add_argument("--verify-aborts", dest="verify_aborts", action="store_true", default=True,
                   help="re-run each reference-aborted file alone in a dedicated child to "
                        "confirm the abort reproduces in isolation (default on; cheap, "
                        "since aborts are rare)")
    p.add_argument("--no-verify-aborts", dest="verify_aborts", action="store_false",
                   help="skip abort re-verification; aborts are then reported UNVERIFIED")
    p.add_argument("--cli", type=str, default=None,
                   help="path to the pdfcer binary to measure (default "
                        "target/release/pdfcer). A full sweep takes tens of minutes; "
                        "in a repo under active development `cargo build` can relink "
                        "target/release MID-SWEEP, which turns healthy pages into "
                        "spurious `pdfcer:` skips and silently corrupts the run. Pass a "
                        "COPY of the binary to pin the measured build for the whole "
                        "sweep. The path used is recorded in summary.json.")
    p.add_argument("--rebaseline", action="store_true",
                   help="deliberately re-record the baseline in --out even though the "
                        "corpus/config no longer matches the report already there. The "
                        "old report is ARCHIVED (summary.superseded-*.json), never "
                        "deleted. Without this flag a mismatch exits 3 before rendering "
                        "anything, so a stale baseline can never be silently replaced.")
    p.add_argument("--out", type=str, default=str(Path(__file__).resolve().parent / "out"),
                   help="output directory (default tools/render-parity/out)")
    p.add_argument("--gate", action="store_true", help="exit nonzero if unexplained > max")
    p.add_argument("--max-unexplained", type=int, default=0,
                   help="gate threshold on unexplained pages (default 0)")
    return p


if __name__ == "__main__":
    raise SystemExit(run(build_argparser().parse_args()))
