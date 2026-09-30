#!/usr/bin/env python3
"""check-control-bytes.py -- no stray C0 control byte in a prose or config file.

WHY THIS EXISTS
===============
On 2026-09-03 the 392nd filing found three literal BACKSPACE bytes (0x08) in
the project's own record: ``docs/ROADMAP.md`` rendered ``D:\\builds`` as
``D:uilds``, and ``docs/SESSION_LOG.md`` carried a regex word-boundary token
``\\b`` that had been swallowed the same way. All three were planted by the
same mechanism this project has written down four times already: text with
a backslash routed through a Python heredoc in the Bash tool loses one level
of escaping, so ``\\b`` becomes backspace, ``\\n`` becomes a newline, ``\\r``
a carriage return.

The written rule ("route any backslash through the Write tool") did not
stop it, because the failure is silent: the file looks right in a terminal
that honours backspace, ``git diff`` shows a line that changed, and nothing
fails. **This gate is the mechanical inverse of that rule** -- it does not
try to prevent the mistake, it makes the mistake fail.

WHAT IT CHECKS
==============
Every tracked (and untracked, non-ignored) file under ``docs/``,
``.claude/``, ``tools/``, ``fixtures/`` (its Markdown; the PDFs are
binary by suffix), the whole ``crates/`` tree and the repository-root
Markdown files is scanned for bytes in ``0x00-0x08``,
``0x0B``, ``0x0E-0x1F`` and ``0x7F``. TAB (0x09), LF (0x0A), FF (0x0C) and
CR (0x0D) are legitimate and skipped. Binary files (a NUL in the first 8 KiB,
or a suffix in the binary list) are skipped, because a PNG legitimately
contains every byte value.

WHY THESE PATHS
===============
The bytes only matter where a human or a parser reads TEXT. Source files are
included because a Rust byte-string literal that has swallowed its escape
(``b'\\n'`` becoming a literal LF inside quotes) compiled by accident the day
this was written, and ``grep`` answered "Binary file matches" instead of
showing the line. Fixture PDFs, model weights and images are not text and
are excluded by suffix.

OUTPUT
======
Exit 0 and one line when clean. Otherwise one line per offending file with
the first offending line number and the byte, in ``\\xNN`` form (never the
raw byte -- a CI log is text too), and exit 1.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

BINARY_SUFFIXES = {
    ".pdf", ".png", ".jpg", ".jpeg", ".gif", ".ico", ".bmp", ".tif", ".tiff",
    ".jp2", ".jpx", ".j2k", ".ttf", ".otf", ".pfb", ".cff", ".woff", ".woff2",
    ".rten", ".bin", ".zip", ".gz", ".bundle", ".exe", ".dll", ".pdb", ".icc",
    ".icm", ".fdf", ".jb2", ".jbig2", ".dat", ".wasm", ".lock",
    ".der", ".cer", ".crl", ".pfx", ".p12", ".p7s", ".key",
}

TEXT_ROOTS = ("docs/", ".claude/", "tools/", "crates/", ".github/", "fuzz/fuzz_targets/", "fixtures/")
ROOT_MARKDOWN = ("README.md", "CLAUDE.md", "LICENSE", "UI_PREFERENCES.md", "THIRD_PARTY_LICENSES.md")

ALLOWED = {0x09, 0x0A, 0x0C, 0x0D}


def candidate_files() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=ROOT,
        capture_output=True,
        check=True,
    ).stdout.decode("utf-8", "surrogateescape")
    files = []
    for rel in out.split("\0"):
        if not rel:
            continue
        if not (rel.startswith(TEXT_ROOTS) or rel in ROOT_MARKDOWN):
            continue
        if Path(rel).suffix.lower() in BINARY_SUFFIXES:
            continue
        files.append(rel)
    return files


def first_offence(data: bytes) -> tuple[int, int] | None:
    """(1-based line, byte) of the first stray control byte, or None."""
    if b"\0" in data[:8192]:
        return None  # binary by content
    line = 1
    for b in data:
        if b == 0x0A:
            line += 1
            continue
        if b < 0x20 and b not in ALLOWED or b == 0x7F:
            return line, b
    return None


def main() -> int:
    bad = []
    files = candidate_files()
    for rel in files:
        path = ROOT / rel
        if not path.is_file():
            continue
        try:
            data = path.read_bytes()
        except OSError:
            continue
        hit = first_offence(data)
        if hit:
            bad.append((rel, *hit))
    if bad:
        print(f"control-bytes: {len(bad)} file(s) carry a stray control byte:")
        for rel, line, b in bad:
            print(f"    {rel}:{line}  byte \\x{b:02X}")
        print(
            "\nA backspace (\\x08) or bare CR/LF inside a literal is almost always a\n"
            "backslash escape that went through a Bash-tool heredoc and lost a level.\n"
            "Repair with the Edit/Write tool, never with sed or another heredoc."
        )
        return 1
    print(f"control-bytes: clean -- {len(files)} text file(s) carry no stray C0 byte")
    return 0


if __name__ == "__main__":
    sys.exit(main())
