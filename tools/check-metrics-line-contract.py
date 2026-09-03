#!/usr/bin/env python3
"""Gate: the published `render-page` metrics contract must list exactly the
keys the code emits.

WHY THIS GATE EXISTS
====================
`pdfcer render-page` prints one machine-readable line whose second half
is `key=<integer>` pairs. That line is a PUBLISHED CONTRACT — the module
docs call it "the fixed order shown" and tell a script how to parse it —
and it is written down in **three** places:

  1. `crates/pdfcer-cli/src/main.rs`, the `println!` that emits it.
     THE AUTHORITY. What the program actually does.
  2. `crates/pdfcer-cli/src/main.rs`, the `//! render-page:` template in
     the module doc block. What a consumer reads.
  3. `crates/pdfcer-cli/tests/render_page.rs`, an `assert_eq!` on the key
     list. What CI enforces.

Copy 3 is under test, so it cannot drift. Copy 2 is not, so it did — by
**28 of 87 keys, 32 %**, contiguously, from `blend_modes_applied` to
`cmyk_unbridged_images`. The template's last extension was `1e7a0be`
(2026-08-17); the next slice `bd244d9` the same day skipped it, and
sixteen later commits edited copy 3 without one of them touching copy 2.

That is standing rule **`R212`**, and its load-bearing half is the part
this gate exists to defeat: **the updated copy is evidence of nothing.**
A reviewer sees copy 3 change in the diff and reads *compulsion* as
*diligence* — and the better the argument in the test's comment, the more
convincing the misreading. Nobody was careless; the process simply had no
force acting on copy 2.

WHAT IT CHECKS, and the one thing it deliberately does not
=========================================================
Exactly one predicate, and it is decidable from bytes alone:

    the ordered key list in the `//! render-page:` template
      ==
    the ordered key list in the `println!` format string

**Copy 3 is NOT checked here**, because it already checks itself: a
mismatch there fails
`renders_a_single_page_to_png_with_the_stable_stdout_line`. Adding it
would duplicate a green test and dilute what a red run from this file
means.

It ALSO checks that every emitted key has a row in the module's per-key
table — but reports that separately, and the reasoning is worth keeping
because the first draft of this file refused to check it at all.

That refusal argued the two are DIFFERENT contracts with different
failure modes: a missing table row is an incomplete explanation, while a
missing template key is a WRONG published specification, and only the
second can break a consumer's parser. All of that is true. **None of it
is a reason not to check.** It is a reason to report the two separately,
which is what happens below — a run can fail on either, and the message
says which. The distinction argued for a better report and was used to
justify no report, which is the same substitution the template gap was
made of.

The number that settled it: when this file was written the table
documented **44 of 87 keys**. Every annotation, colour-space, shading,
blend-mode, soft-mask, transparency-group, overprint and CMYK counter had
no explanation anywhere a consumer of this CLI would look, while their
`Diagnostics` field docs were thorough throughout. Left ungated, that gap
reopens on the next slice for exactly the reason `R212` names.

    THE GENERAL FORM OF THIS PROBLEM IS UNDECIDABLE. "Does this doc block
    state a contract some test file also states?" cannot be answered by
    grep. THE SPECIFIC INSTANCE IS TRIVIAL: two regexes, two spans, one
    file. That asymmetry is the finding — a gate goes on the decidable
    instance, not on the class.

EXIT CODES
==========
    0  the template matches the println AND every key has a table row
    1  either check failed; the report names which, every key, and the
       direction of the difference

Both lists are printed in full on failure, because the useful question is
never "did it fail" but "which slice was added and where does it belong".

USAGE
=====
    python tools/check-metrics-line-contract.py

No arguments, no configuration, no baseline file. A baseline was
considered and refused for the reason `tools/commits-filed-baseline.txt`
already states about itself: a suppression list silences exactly what a
gate exists to catch. The 28-key debt this gate was written for was
repaired BEFORE it was wired, per
`D:/dev/rag/rust/ci_gate_red_at_baseline_enforces_nothing.md`.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

MAIN_RS = Path(__file__).resolve().parent.parent / "crates" / "pdfcer-cli" / "src" / "main.rs"

# The template lives inside a fenced block in the module docs; every one of
# its lines starts with `//!` and the block opens on the line beginning
# `//! render-page:`. It ends at the fence (a `//! ``` ` line).
TEMPLATE_START = re.compile(r"^//! render-page:", re.M)
FENCE = re.compile(r"^//! ```\s*$", re.M)

# `key=<n>` / `key=<0|1>` in the template, `key={}` in the println. Both are
# anchored on the `=` so a bare word in prose cannot match.
TEMPLATE_KEY = re.compile(r"\b([a-z_][a-z0-9_]*)=<[^>]*>")
PRINTLN_KEY = re.compile(r"\b([a-z_][a-z0-9_]*)=\{\}")

# A per-key documentation row: `//! | `key` | source | "question" |`. Matched
# across every table in the module docs, not only the main one — the
# `unsupported_*` family has its own sub-table and documenting a key there
# is documenting it.
TABLE_ROW = re.compile(r"^//! \| `([a-z0-9_]+)` \|", re.M)

# The println's format string, identified by its first literal bytes rather
# than by line number so an edit above it cannot silently move the window.
# Updated 2026-09-03 (`Pass 248.0`): the counter half moved into
# `render_counters_line`, a `format!` shared by `render-page` and
# `export-image`, so the prefix is no longer in the same string. The
# head is now the FIRST KEY, which is as stable as the tail anchor below.
PRINTLN_HEAD = '"substituted={} notdef={} unsupported={}'
# Updated 2026-08-26 (`Pass 130.1`) when `cmyk_native_image_pixels` was
# appended. The anchor is the LAST key on the line, so every append moves it —
# that is the intended cost: the gate's failure message names this constant so
# the edit is a two-line follow-through rather than a hunt.
# Updated 2026-09-01 (`Pass 199.2`) for `icc_unmanaged_paints`.
#
# ★ NOTE WHAT THIS CONSTANT'S HISTORY SHOWS. It was last touched for
# `cmyk_native_image_pixels`, but `rendering_intents_set` was appended after
# that and this was NOT updated -- so from that Pass until now the gate could
# not find the format string at all and exited with its "substring not found"
# message. It was failing loudly and nobody was reading it, which is a
# different disease from a gate that silently passes but has the same
# outcome: the key-order contract went unchecked across several Passes.
PRINTLN_TAIL = 'overprint_process_images_unsupported={}"'


def template_keys(src: str) -> list[str]:
    """Ordered keys from the `//! render-page:` doc template.

    Raises rather than returning empty if the block cannot be located: a
    gate that silently finds nothing and compares two empty lists PASSES,
    which is the failure mode that makes a gate worse than no gate.
    """
    m = TEMPLATE_START.search(src)
    if m is None:
        raise SystemExit("check-metrics-line-contract: no `//! render-page:` template found")
    end = FENCE.search(src, m.end())
    if end is None:
        raise SystemExit("check-metrics-line-contract: template block is not closed by a fence")
    return TEMPLATE_KEY.findall(src[m.start() : end.start()])


def println_keys(src: str) -> list[str]:
    """Ordered keys from the `println!` that emits the line.

    Same posture on failure as `template_keys`, and for the same reason.
    """
    try:
        i = src.index(PRINTLN_HEAD)
        j = src.index(PRINTLN_TAIL, i)
    except ValueError as err:
        raise SystemExit(
            "check-metrics-line-contract: could not locate the render-page `println!` "
            f"format string ({err}). If it was legitimately reworded, update "
            "PRINTLN_HEAD/PRINTLN_TAIL in this file — do not delete the check."
        ) from err
    return PRINTLN_KEY.findall(src[i : j + len(PRINTLN_TAIL)])


def undocumented_keys(src: str, emitted: list[str]) -> list[str]:
    """Emitted keys with no row in any per-key table in the module docs."""
    documented = set(TABLE_ROW.findall(src))
    return [k for k in emitted if k not in documented]


def main() -> int:
    src = MAIN_RS.read_text(encoding="utf-8")
    doc = template_keys(src)
    code = println_keys(src)
    unrowed = undocumented_keys(src, code)

    if doc == code and not unrowed:
        print(
            f"metrics-line-contract: OK — {len(code)} keys; the template matches "
            "the println and every key has a table row."
        )
        return 0

    if doc == code:
        # The machine-readable contract is intact; only the prose is short.
        # Said out loud, because "the gate is red" reads as "a consumer will
        # break" and here it does not — what breaks is a person's ability to
        # act on a number they were handed.
        print("metrics-line-contract: the template is CORRECT; the explanation is not.")
        print(f"  {len(unrowed)} of {len(code)} emitted keys have no row in any per-key table.")
        print("  A counter nobody can interpret is a number an operator cannot act on,")
        print("  which is most of the reason `Diagnostics` exists (decision 004 §6.4, R20):")
        for k in unrowed:
            print(f"    ? {k}")
        print(
            "\n  Add a row to the table in crates/pdfcer-cli/src/main.rs. The source is\n"
            "  the field's own `///` doc in crates/pdfcer-render/src/interpret.rs — say\n"
            "  whether the number is a census, a divergence, or half of a pair."
        )
        return 1

    missing = [k for k in code if k not in doc]
    extra = [k for k in doc if k not in code]

    print("metrics-line-contract: the published template and the emitted line disagree.")
    print(f"  println!  emits    {len(code)} keys")
    print(f"  //! template lists {len(doc)} keys")
    if missing:
        print(f"\n  EMITTED BUT NOT DOCUMENTED ({len(missing)}) — the template is a")
        print("  published specification, so an omission here is a WRONG promise,")
        print("  not merely a thin one:")
        for k in missing:
            print(f"    + {k}")
    if extra:
        print(f"\n  DOCUMENTED BUT NOT EMITTED ({len(extra)}) — a consumer told to")
        print("  expect these will not find them:")
        for k in extra:
            print(f"    - {k}")
    if not missing and not extra:
        print("\n  Same keys, DIFFERENT ORDER. The template claims to show the fixed")
        print("  order, so this is as wrong as a missing key and harder to spot:")
        for n, (a, b) in enumerate(zip(doc, code)):
            if a != b:
                print(f"    first divergence at position {n}: template `{a}`, println `{b}`")
                break

    if unrowed:
        print(f"\n  AND {len(unrowed)} emitted key(s) have no per-key table row:")
        for k in unrowed:
            print(f"    ? {k}")

    print(
        "\n  Fix the template in crates/pdfcer-cli/src/main.rs, and add a row for any\n"
        "  new key to the per-key table below it. Do NOT add a baseline file."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
