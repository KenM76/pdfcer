#!/usr/bin/env python3
"""Fail if a persisted operator setting is stored but never read.

WHY THIS EXISTS
===============

Standing rule **R83 — no affordance without capability**. A setting that
round-trips through ``userdata/settings.txt``, is documented in that
file's own comments, and is then read by nothing is worse than no setting
at all: the operator changes it, sees no effect, and reasonably concludes
pdfce is broken rather than that the knob is decorative.

This is not hypothetical. The Pass that introduced the settings store
shipped **two** such fields in its very first commit — ``separations`` and
``word_gap_ratio``. Both parsed, both written back out, both described in
the generated settings file, both consumed by zero call sites. The third
field, ``cmyk_intent``, was threaded all the way to the pixels and had a
whole integration-test file defending it, which is precisely what makes
the other two instructive: the discipline was applied deliberately where
it was being thought about and forgotten where it was not.

Vigilance already failed once here. So it is a gate.

WHAT IT CHECKS
==============

For every ``pub`` field of ``Settings`` in
``crates/pdfce-core/src/settings/mod.rs``:

1. The field must be **parsed** — its key must appear in an ``apply`` arm,
   or it can never be set from the file.
2. The field must be **written** — it must appear in
   ``write_to_string``, or a saved file silently loses it.
3. The field must be **consumed** — read at least once from a
   ``settings.<field>`` / ``.settings.<field>`` expression somewhere
   OUTSIDE the settings module itself. A read inside ``settings/`` does
   not count: round-tripping a value through its own tests proves the
   parser works, not that the program does anything with it.

★ WIDENED 2026-08-18, AFTER IT MISSED THE DEFECT IT EXISTS FOR
=============================================================
`DeviceSettings::pick_tray_by_page_size` in ``pdfce-print`` was declared,
documented, plumbed through ``spool``, **bound to a checkbox that shipped in
the GUI**, and read by nothing. Exactly R83's failure: the operator ticks a
box and nothing happens.

This gate did not catch it, and could not have. It was hard-scoped to the
``Settings`` struct in ``pdfce-core/src/settings/mod.rs`` — one struct, in
one file, in one crate — while the defect class it was written for is
"a `pub` option field that no code reads", which is not confined to that
struct. **A gate scoped to one instance of a class reports clean on the
class.**

So there are now two checks. The `Settings` one is unchanged and still
demands parse + write + consume, because a persisted setting has a file
format to honour. The second demands only **consume**, over a declared list
of OPTION structs — structs a caller fills in and hands to an API. They have
no file format, so parse/write do not apply; the only way they fail is by
having a field nobody reads, which is the whole of what went wrong.

Adding a struct to ``OPTION_STRUCTS`` is how this gate grows. It does not
discover them, and that is a real limit: a new options struct is invisible
until someone lists it here.

WHAT THIS GATE STILL CANNOT SEE, enumerated rather than gestured at:

* **A test-only read counts as consumption.** There is no ``#[cfg(test)]``
  awareness, so a field read only by its own unit test satisfies the check
  while production ignores it. Narrower than the defect this closed, but the
  same family.
* **A field read through a re-borrow it cannot follow** — ``let d =
  &settings.device; d.field`` — is invisible; the pattern is textual.
* **It does not check the read is CORRECT**, or that it reaches the spooler.
  That is a test's job.

Three things had to be right before it caught anything, and each was found
by sabotage after the previous one looked sufficient: the struct list, the
read-vs-write distinction (a ``&mut`` borrow handed to a checkbox is the
PRODUCER, not a consumer), and ``CONSUMER_ROOTS`` — which had never included
``pdfce-print`` at all, so no pattern could have found a reader in the crate
the setting lives in.

WHAT IT DELIBERATELY DOES NOT CHECK
===================================

That the consumer is *correct*, or that it reaches the pixels/bytes. That
is a test's job — see ``crates/pdfce-render/tests/cmyk_intent.rs``, which
proves the CMYK intent survives the whole distance from ``RenderOptions``
to a rendered pixel. This gate only catches the cheaper, dumber failure:
nobody wired it at all. A grep-based gate that tried to judge semantics
would produce false confidence, which is the one outcome worse than no
gate.

EXIT CODES
==========

``0`` clean, ``1`` at least one setting is unreachable, ``2`` the gate
could not run (missing files, unparseable struct) — never confused with
"clean", because a check that cannot run must not look like one that
passed.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# Windows consoles default to a code page that cannot encode the em-dashes,
# arrows and stars this file prints, so Python substitutes "?" for exactly
# the characters that make a failure message readable. One reconfigure fixes
# every message in the file without flattening the typography.
#
# This is not theoretical: `check-commits-filed.py` was observed printing
# "each commit's full message ? they carry" while doing its job correctly.
# Found by reading a gate's output as its audience (R174), not by reading
# its source.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

# ★ Fields whose ONLY consumer is the desktop GUI, which since Pass 247.0
# (decision 128) is the separate `pdfcer-gui` project and not on this tree.
# This gate can scan only what is on disk under this repository, so a field
# read exclusively by that shell now looks READ BY NOTHING. Rather than let
# the gate go red on a promise that IS kept, each such field is listed here
# with the out-of-tree reader that keeps it — a claim a reader can check with
# one grep in the other project. An entry here is NOT an allowlist for a
# dead setting: if the cited reader disappears, the entry is wrong and the
# setting is dead, exactly as R83 describes.
CONSUMED_BY_OUT_OF_TREE_GUI: dict[str, str] = {
    "theme": (
        "pdfcer-gui: crates/pdfcer-gui/src/app/frame.rs (`self.settings.theme`), "
        "settings_window.rs (`Preset::from_key(&self.settings.theme)`)"
    ),
}


ROOT = Path(__file__).resolve().parent.parent
SETTINGS = ROOT / "crates" / "pdfce-core" / "src" / "settings" / "mod.rs"

# Where a setting may legitimately be consumed. The settings module itself
# is excluded on purpose (see the module docstring).
CONSUMER_ROOTS = [
    ROOT / "crates" / "pdfce-cli" / "src",
    ROOT / "crates" / "pdfce-render" / "src",
    ROOT / "crates" / "pdfce-core" / "src",
    # pdfce-print and pdfce-fetch were ABSENT until 2026-08-18, and that
    # absence — not the pattern, not the struct list — is the deepest reason
    # `DeviceSettings::pick_tray_by_page_size` shipped inert. The gate did
    # not scan the crate the setting lives in, so no regex it used could
    # have found a reader there. A gate's INPUT SET is part of the gate, and
    # this one silently excluded two of the six crates.
    ROOT / "crates" / "pdfce-print" / "src",
    ROOT / "crates" / "pdfce-fetch" / "src",
]


# Option structs: a caller fills one in and hands it to an API. Unlike
# `Settings` they have no file format, so only the CONSUME half applies.
#
# (path relative to repo root, struct name).
#
# ★ NOTE WHAT IS *NOT* EXCLUDED HERE, because the first draft got it wrong.
# The `Settings` check ignores reads inside `settings/` -- round-tripping a
# value through its own module proves the parser works, not that the program
# does anything with it. Carrying that rule over to option structs is a
# mistake: an option struct is an INPUT TO its own crate, so the crate
# reading it IS the consumption. Excluding `crates/pdfce-render/src` made
# the gate report `RenderOptions.annotation_scope` unread when
# `effective_annotation_scope()` reads it three lines from its declaration.
#
# So an option field need only be read SOMEWHERE. That is weaker than the
# Settings rule and it is still exactly strong enough for the defect class:
# `pick_tray_by_page_size` was passed through `spool` inside a struct and
# never accessed, so `.pick_tray_by_page_size` appeared nowhere at all. A
# field declaration is `pub name: Type` and does not match `.name`, so a
# struct that only declares a field cannot satisfy this check by accident.
OPTION_STRUCTS = [
    ("crates/pdfce-print/src/lib.rs", "DeviceSettings"),
    ("crates/pdfce-render/src/font/mod.rs", "RenderOptions"),
]


def fail(message: str) -> None:
    print(f"settings-consumed: {message}", file=sys.stderr)


def struct_fields(source: str) -> list[str]:
    """Every `pub` field name of the `Settings` struct, in order."""
    match = re.search(
        r"pub struct Settings \{(.*?)\n\}", source, re.DOTALL
    )
    if not match:
        return []
    body = match.group(1)
    # `pub name: Type,` — doc comments and attributes are skipped by the
    # anchor on `pub `.
    return re.findall(r"^\s*pub ([a-z_][a-z0-9_]*):", body, re.MULTILINE)


def named_struct_fields(source: str, name: str) -> list[str]:
    """Every `pub` field of a named struct, in order."""
    match = re.search(rf"pub struct {re.escape(name)} \{{(.*?)\n\}}", source, re.DOTALL)
    if not match:
        return []
    return re.findall(r"^\s*pub ([a-z_][a-z0-9_]*):", match.group(1), re.MULTILINE)


def check_option_structs() -> list[str]:
    """Every `pub` field of each OPTION_STRUCTS entry must be read somewhere
    outside its own module.

    Deliberately NOT checking parse/write: these structs have no file
    format. The only failure mode available to them is the one that shipped
    an inert GUI checkbox.
    """
    problems: list[str] = []
    for rel, name in OPTION_STRUCTS:
        path = ROOT / rel
        if not path.is_file():
            problems.append(f"`{name}`: {rel} not found — this gate's list is stale")
            continue
        fields = named_struct_fields(path.read_text(encoding="utf-8"), name)
        if not fields:
            problems.append(
                f"`{name}`: no `pub` fields found - either the struct moved or "
                f"this gate's parser is stale. Refusing to report clean."
            )
            continue
        texts: list[str] = []
        for root in CONSUMER_ROOTS:
            if not root.is_dir():
                continue
            for f in root.rglob("*.rs"):
                try:
                    texts.append(f.read_text(encoding="utf-8"))
                except OSError:
                    continue
        for field in fields:
            # `.field` is enough: these are read through a binding whose
            # name the gate cannot know (`settings.`, `opts.`, `cfg.`…).
                        # A READ, NOT AN ASSIGNMENT - and this distinction is the
            # entire gate. `settings.pick_tray_by_page_size = true` and
            # `if settings.pick_tray_by_page_size` both contain
            # `.pick_tray_by_page_size`; the first is the GUI SETTING
            # the field, the second is the engine HONOURING it.
            # Counting the first as consumption is exactly how the
            # original defect stayed invisible - the checkbox wrote the
            # field, nothing read it, and a naive `.field` search finds
            # the write and reports clean.
            #
            # Verified by sabotage: with every genuine read removed
            # from `pdfce-print`, the naive pattern still matched
            # `pdfce-gui/src/print_flow.rs` and the gate PASSED. It
            # only goes red once assignments are excluded.
            #
            # `[^=]` after `=` keeps `==` a read.
            # ★ THE WHITESPACE BELONGS INSIDE THE LOOKAHEAD. This line read
            # `\b\s*(?!=[^=])` until 2026-08-20 and that spelling is BROKEN:
            # `\s*` outside the lookahead BACKTRACKS TO ZERO, so the engine
            # matches `.field`, consumes no space, and then asks whether the
            # very next character is `=`. For `settings.field = true` the next
            # character is a SPACE, the negative lookahead succeeds, and the
            # assignment is counted as a read — the filter that is the entire
            # point of the line silently does nothing on the one spelling that
            # matters. (`settings.field=true`, with no space, was correctly
            # rejected, which is how it survived.)
            #
            # The comment block above says this was sabotage-verified, and it
            # was — but against `pick_tray_by_page_size`, whose write form is a
            # `&mut` BORROW, caught by the SEPARATE borrow filter below. The
            # `=` filter was never the thing under test. It was found by
            # `tools/check-outcome-disclosed.py`'s own self-test, which drove a
            # spaced assignment as a decoy on its first run and went green when
            # it should have gone red.
            #
            # ★★ NOTE THE SHAPE, because this project keeps paying for it: a
            # verified gate, with a comment explaining at length why it is
            # right, that was verified against a DIFFERENT case than the one
            # the comment describes. A stated derivation reads exactly like a
            # maintained one.
            pattern = re.compile(rf"\.\s*{re.escape(field)}\b(?!\s*=[^=])")
            # ...and a `&mut` BORROW is a write too, which is the form
            # the defect actually took. `pdfce-gui` binds its checkbox
            # with `&mut pending.device.pick_tray_by_page_size` -- no
            # `=` anywhere, so the assignment filter above sails past
            # it, and the gate counts it a reader. Handing a field to a
            # widget by mutable reference is the canonical PRODUCER in
            # this codebase; it is the thing that made the control
            # exist, not the thing that honoured it.
            #
            # Found the same way as the last two: by sabotage. Removing
            # every genuine read from `pdfce-print` left the gate GREEN
            # twice -- once on the raw `.field` pattern and again after
            # assignments were excluded -- because this one borrow kept
            # matching.
            def is_read(text: str) -> bool:
                for m in pattern.finditer(text):
                    before = text[max(0, m.start() - 24) : m.start()]
                    if re.search(r"&\s*mut\s*[A-Za-z_][A-Za-z0-9_.]*$", before):
                        continue
                    return True
                return False

            if not any(is_read(t) for t in texts):
                problems.append(
                    f"`{name}.{field}` is a public option field READ BY "
                    f"NOTHING. R83: a caller who sets it sees no effect. "
                    f"Wire it to the behaviour it names, or remove it."
                )
    return problems


def section(source: str, signature: str) -> str:
    """The body of one function, by its signature line."""
    at = source.find(signature)
    if at < 0:
        return ""
    # Functions in this module are separated by a de-indented `}`; taking
    # everything up to the next `\n    }\n` is exact enough for a gate and
    # does not need a Rust parser.
    end = source.find("\n    }\n", at)
    return source[at : end if end > 0 else len(source)]


def main() -> int:
    if not SETTINGS.is_file():
        fail(f"cannot read {SETTINGS}")
        return 2

    source = SETTINGS.read_text(encoding="utf-8")
    fields = struct_fields(source)
    if not fields:
        fail(
            "no `pub` fields found on `Settings` — either the struct moved "
            "or this gate's parser is stale. Refusing to report clean."
        )
        return 2

    apply_body = section(source, "fn apply(")
    write_body = section(source, "pub fn write_to_string(")
    if not apply_body or not write_body:
        fail(
            "could not locate `apply` and/or `write_to_string` in the "
            "settings module. Refusing to report clean."
        )
        return 2

    # Gather every consumer file's text once.
    consumers: dict[Path, str] = {}
    for root in CONSUMER_ROOTS:
        if not root.is_dir():
            continue
        for path in root.rglob("*.rs"):
            if SETTINGS.parent in path.parents or path == SETTINGS:
                continue
            try:
                consumers[path] = path.read_text(encoding="utf-8")
            except OSError:
                continue

    problems: list[str] = []
    for field in fields:
        if f'"{field}"' not in apply_body:
            problems.append(
                f"`{field}` has no arm in `apply`, so it can never be set "
                f"from the settings file"
            )
        if field not in write_body:
            problems.append(
                f"`{field}` is not written by `write_to_string`, so saving "
                f"settings would silently drop it"
            )

        pattern = re.compile(rf"settings\s*\.\s*{re.escape(field)}\b")
        readers = sorted(
            str(path.relative_to(ROOT)).replace("\\", "/")
            for path, text in consumers.items()
            if pattern.search(text)
        )
        if not readers and field in CONSUMED_BY_OUT_OF_TREE_GUI:
            print(
                f"settings-consumed: `{field}` has no in-tree reader; consumed "
                f"out of tree by {CONSUMED_BY_OUT_OF_TREE_GUI[field]}"
            )
            continue
        if not readers:
            problems.append(
                f"`{field}` is parsed and written but READ BY NOTHING. "
                f"R83: an operator who changes it will see no effect. "
                f"Either wire it to the behaviour it names, or take it out "
                f"of `Settings` and out of the generated file"
            )

    problems.extend(check_option_structs())

    if problems:
        fail(f"{len(problems)} problem(s):")
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        print(
            "\n  A setting is a promise. Storing one that does nothing "
            "breaks it silently.",
            file=sys.stderr,
        )
        return 1

    n_opt = sum(
        len(named_struct_fields((ROOT / rel).read_text(encoding="utf-8"), name))
        for rel, name in OPTION_STRUCTS
        if (ROOT / rel).is_file()
    )
    print(
        f"settings-consumed: clean - {len(fields)} persisted setting(s) "
        f"parsed, written and read; {n_opt} option field(s) across "
        f"{len(OPTION_STRUCTS)} struct(s) read by at least one caller."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
