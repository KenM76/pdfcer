#!/usr/bin/env python3
"""A re-exported type's own field types must be re-exported too.

WHY THIS EXISTS
===============

`Pass 295.0` added `StyleLadder::passed_over: Vec<PassedOver>` and re-exported
`StyleLadder` — but not `PassedOver`. The consuming shell hit it within the
hour:

    error[E0432]: unresolved import `pdfcer_core::text_edit::PassedOver`

Nothing was broken and nothing was blocked: the type is `pub`, and
`pdfcer_core::text_edit::format::PassedOver` resolves. It is a **naming
asymmetry** — the struct that a re-exported struct's field is made of is not
itself re-exported — and the consumer has to either reach through a module
path its neighbours do not use, or alias it.

★ The shape is worth naming because it recurs by construction: a Pass adds a
field, re-exports the type that GAINED the field, and never thinks about the
type the field is MADE of. The author cannot see it — inside the crate every
path resolves.

WHAT IT CHECKS
==============

For each `pub use <module>::{ ... }` list in a `mod.rs`, every type named in
it is looked up in that module, and each of its `pub` fields' types is
decomposed to the identifiers it mentions. Any identifier that is defined in
the same module and is NOT in the re-export list is a finding.

It deliberately checks only:

* types defined in the SAME module as the re-exported type — a type from
  elsewhere has its own list and its own answer;
* `pub` fields — a private field is not part of the surface a consumer meets;
* `struct` and `enum` definitions, found by name.

Generic wrappers are seen through: `Vec<PassedOver>`, `Option<Refusal>` and
`BTreeMap<String, FontSibling>` all mention what they are made of.

WHAT IT DOES NOT CHECK
======================

Method return types. A verb returning an unexported type is the same class of
defect, but finding it needs real parsing rather than a field scan, and this
gate is meant to be a second's work. If that case bites, widen it then — with a
measurement, the way `check-string-gaps.sh` was widened.

USAGE
=====

    python tools/check-reexport-closure.py [--stats]

Exit 0 clean, 1 on a finding.
"""

from __future__ import annotations

import io
import pathlib
import re
import sys

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")

ROOT = pathlib.Path(__file__).resolve().parent.parent

# Rust keywords and primitives a field type mentions that are never local types.
IGNORE = {
    "Vec", "Option", "Box", "String", "str", "bool", "char", "f32", "f64",
    "u8", "u16", "u32", "u64", "usize", "i8", "i16", "i32", "i64", "isize",
    "BTreeMap", "BTreeSet", "HashMap", "HashSet", "Cow", "Arc", "Rc", "Range",
    "PathBuf", "Path", "Result", "Self",
}

REEXPORT = re.compile(r"pub use (\w+)::\{([^}]*)\};", re.S)
DEFN = re.compile(r"^pub (?:struct|enum) (\w+)", re.M)


def field_types(body: str) -> set[str]:
    """Identifiers mentioned by the `pub` field types in one item body."""
    out: set[str] = set()
    for m in re.finditer(r"^\s*pub \w+: ([^,\n]+),", body, re.M):
        for ident in re.findall(r"\b([A-Z]\w*)\b", m.group(1)):
            out.add(ident)
    return out


def item_body(text: str, name: str) -> str | None:
    """The braces-delimited body of `pub struct/enum <name>`."""
    m = re.search(rf"^pub (?:struct|enum) {re.escape(name)}\b", text, re.M)
    if not m:
        return None
    start = text.find("{", m.end())
    if start < 0:
        return None
    depth, i = 0, start
    while i < len(text):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return text[start : i + 1]
        i += 1
    return None


def main() -> int:
    findings: list[tuple[str, str, str, str]] = []
    checked = 0
    for mod in ROOT.glob("crates/*/src/**/mod.rs"):
        text = mod.read_text(encoding="utf-8", errors="replace")
        for m in REEXPORT.finditer(text):
            module, names = m.group(1), m.group(2)
            src = mod.parent / f"{module}.rs"
            if not src.exists():
                src = mod.parent / module / "mod.rs"
            if not src.exists():
                continue
            body = src.read_text(encoding="utf-8", errors="replace")
            exported = {n.strip() for n in names.split(",") if n.strip()}
            defined = set(DEFN.findall(body))
            for name in sorted(exported & defined):
                checked += 1
                item = item_body(body, name)
                if item is None:
                    continue
                for used in sorted(field_types(item)):
                    if used in IGNORE or used in exported or used not in defined:
                        continue
                    rel = str(mod.relative_to(ROOT)).replace("\\", "/")
                    findings.append((rel, module, name, used))

    if "--stats" in sys.argv:
        print(f"  re-exported types checked : {checked}")
        print(f"  findings                  : {len(findings)}")

    if findings:
        print("check-reexport-closure: FINDINGS —")
        for rel, module, name, used in findings:
            print(f"  {rel}: `{name}` is re-exported, its field type `{used}` is not")
            print(f"      add `{used}` to `pub use {module}::{{ … }}`")
        print()
        print(
            "  A consumer meeting the parent type meets its field types too, and\n"
            "  reaching one through a module path its neighbours do not use is a\n"
            "  seam the author cannot see -- inside the crate every path resolves."
        )
        return 1

    print(f"check-reexport-closure: clean — {checked} re-exported type(s), every field type reachable")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
