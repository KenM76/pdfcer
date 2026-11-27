#!/usr/bin/env python3
"""Cross-cutting process gate (decision 228, Pass 781.1).

`docs/decisions/228-cross-cutting-coverage.md` is the contract; this script
enforces the parts of it a compiler cannot. Stages, all run every time:

1. freshness  -- the `object_kinds!` table in `crates/pdfcer-core/src/xcut/
                 kind.rs` and the `ops!` table in `xcut/op.rs` match the rows,
                 sections and columns of `docs/cross-cutting.md`, in order.
2. grammar    -- every matrix cell is `ok[:changes]`, `remove`, `consume`,
                 `represent`, `refused:<cause>`, `n/a:"<reason>"` or
                 `pass:<id>`; a change is `translated(K)`, `rebound(K)`,
                 `regenerated(K)`, `rebaked(K)`, `dropped(K[->id])`,
                 `unbound(K->id)` or `renumbered`.
3. pending    -- every Pass id a cell names (`pass:<id>`, `->id`) appears in
                 `docs/ROADMAP.md` and is not shipped. Shipped = named in a
                 `###` heading under ROADMAP's `## Shipped` or in
                 `docs/history/roadmap-shipped-*.md` (ranges `A-B` expand).
4. features   -- `docs/FEATURES.md` rows in object-bearing sections that
                 differ from XCUT_BASE (working tree included, so an
                 uncommitted row fails locally) end with
                 `<!-- xc: <kind id>, ... -->` or `<!-- xc: none "reason" -->`,
                 placed inside the last cell. Every tag anywhere must name
                 real kinds. Every kind must be tagged somewhere, except the
                 `untagged-kind` debt in the baseline.
5. carry      -- no `*_CARRIED_KEYS` / `*_CARRY_KEYS` const or static, and no
                 `struct *Carry`, under `crates/` beyond the baseline's
                 `carry` entries (decision 228 carry rule: clone_then_patch
                 is the only carry).
6. --self-test runs synthetic bad trees; each must fail its stage.
7. counts are printed on every run, pass or fail.

OBJECT-BEARING SECTIONS of FEATURES.md (stage 4): rows that describe
creating, editing, carrying or exporting a document object -- the whole of
`## Planned, in predicted order` (where new rows land), and these
`## Implemented` subsections: Document & pages; Text; Vector objects;
ce dimensions; Annotations & markup; Forms; Redaction & security; Export.
Excluded: Fonts & rendering, Reading/navigation/printing, Shell & UX (viewer
and decoder behaviour, not object carriage), Cannot, Will not. A row in an
included section that concerns no kind uses `xc: none "reason"`.

BASELINE (`tools/cross-cutting-baseline.txt`) is DEBT, not an allowlist: a
new violation fails, and an entry that no longer occurs fails as STALE until
its line is deleted. XCUT_BASE is pinned and is never moved forward --
untagged rows older than it are legacy debt reported as a count.

Usage: check-cross-cutting.py [--root DIR] [--self-test]
Exit 0 clean, 1 on any finding.
"""

import argparse
import io
import re
import subprocess
import sys
import tempfile
from contextlib import redirect_stdout
from pathlib import Path

XCUT_BASE = "afaf89e5a2612c91fe2aa88d6fb10d20b867798c"
KIND_RS = "crates/pdfcer-core/src/xcut/kind.rs"
OP_RS = "crates/pdfcer-core/src/xcut/op.rs"
MATRIX_MD = "docs/cross-cutting.md"
ROADMAP_MD = "docs/ROADMAP.md"
FEATURES_MD = "docs/FEATURES.md"
BASELINE = "tools/cross-cutting-baseline.txt"
STAGES = ("freshness", "grammar", "pending", "features", "carry")

OBJECT_SECTIONS = {
    "Document & pages",
    "Text",
    "Vector objects (Inkscape-style editing)",
    "ce dimensions",
    "Annotations & markup",
    "Forms (AcroForm)",
    "Redaction & security",
    "Export",
}
PLANNED = "Planned, in predicted order"

PASS_ID = r"\d{1,4}\.\d{1,3}(?:\.[a-z0-9]{1,3})?"
PASS_TOKEN = re.compile(rf"(?<![\w.])({PASS_ID})(?![\w]|\.\w)")
PASS_RANGE = re.compile(rf"(?<![\w.])({PASS_ID})`?\s*-\s*`?(?:Pass\s+)?({PASS_ID})(?![\w])")
KEY = r"[A-Za-z0-9_.#]+"
CHANGE = re.compile(
    rf"(?:translated|rebound|regenerated|rebaked)\({KEY}\)"
    rf"|dropped\({KEY}(?:->({PASS_ID}))?\)"
    rf"|unbound\({KEY}->({PASS_ID})\)"
    r"|renumbered"
)
SIMPLE_CELL = re.compile(rf'^(?:remove|consume|represent|refused:[a-z0-9][a-z0-9-]*|n/a:"[^"]*\S[^"]*"|pass:({PASS_ID}))$')
TAG = re.compile(r"<!--\s*xc:\s*(.*?)\s*-->")
NONE_TAG = re.compile(r'^none\s+"[^"]*\S[^"]*"$')
CARRY_CONST = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const|static)\s+([A-Z0-9_]*_CARR(?:IED|Y)_KEYS)\b")
CARRY_STRUCT = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?struct\s+([A-Za-z0-9_]*Carry)\b")


def macro_body(text: str, macro: str) -> list[str]:
    """Lines of the `macro! {` invocation (not its `macro_rules!` definition)."""
    lines = text.split("\n")
    for i, line in enumerate(lines):
        if line.startswith(f"{macro}! {{"):
            depth, out = 0, []
            for body in lines[i:]:
                depth += body.count("{") - body.count("}")
                out.append(body)
                if depth <= 0:
                    return out
    return []


def parse_kinds(text: str) -> list[str]:
    return [m.group(1) for l in macro_body(text, "object_kinds")
            if (m := re.match(r'^\s*\w+\s*=>\s*"([^"]+)",', l))]


def parse_ops(text: str) -> list[tuple[str, list[str]]]:
    groups: list[tuple[str, list[str]]] = []
    for line in macro_body(text, "ops"):
        if m := re.match(r'^\s*\w+\s*=>\s*"([^"]+)"\s*\{', line):
            groups.append((m.group(1), []))
        elif (m := re.match(r'^\s*\w+\s*=>\s*"([^"]+)",', line)) and groups:
            groups[-1][1].append(m.group(1))
    return groups


def split_row(line: str) -> list[str]:
    """Cells of a Markdown table row; a `|` inside double quotes is text."""
    cells, cur, quoted = [], [], False
    for ch in line.strip().strip("|"):
        if ch == '"':
            quoted = not quoted
        if ch == "|" and not quoted:
            cells.append("".join(cur).strip())
            cur = []
        else:
            cur.append(ch)
    cells.append("".join(cur).strip())
    return cells


def parse_matrix(text: str) -> list[dict]:
    sections: list[dict] = []
    for line in text.split("\n"):
        if line.startswith("## "):
            sections.append({"id": line[3:].strip(), "cols": None, "rows": []})
        elif sections and line.startswith("|") and not line.startswith("|---"):
            cells = split_row(line)
            if cells[0] == "kind":
                sections[-1]["cols"] = cells[1:]
            else:
                sections[-1]["rows"].append((cells[0], cells[1:]))
    return sections


def stage_freshness(kinds, ops, matrix) -> list[str]:
    bad = []
    want, got = [g for g, _ in ops], [s["id"] for s in matrix]
    if want != got:
        bad.append(f"sections {got} != op groups {want}")
    for gid, cols in ops:
        sec = next((s for s in matrix if s["id"] == gid), None)
        if sec is None:
            continue
        if sec["cols"] != cols:
            bad.append(f"section {gid}: columns {sec['cols']} != ops {cols}")
        rows = [r for r, _ in sec["rows"]]
        if rows != kinds:
            missing = [k for k in kinds if k not in rows]
            extra = [r for r in rows if r not in kinds]
            bad.append(f"section {gid}: rows differ from ObjectKind (missing {missing}, extra {extra}, or order)")
        for kind, cells in sec["rows"]:
            if len(cells) != len(cols):
                bad.append(f"section {gid} row {kind}: {len(cells)} cells for {len(cols)} columns")
    if not kinds or not ops:
        bad.append("could not parse the object_kinds!/ops! tables")
    return bad


def parse_cell(cell: str) -> list[str] | None:
    """The Pass ids a cell names, or None if it does not parse."""
    if cell == "ok":
        return []
    if cell.startswith("ok:"):
        ids = []
        for part in cell[3:].split(","):
            m = CHANGE.fullmatch(part)
            if not m:
                return None
            ids += [g for g in m.groups() if g]
        return ids
    m = SIMPLE_CELL.match(cell)
    if not m:
        return None
    return [m.group(1)] if m.group(1) else []


def stage_grammar(matrix) -> tuple[list[str], dict[str, int]]:
    bad, refs = [], {}
    for sec in matrix:
        for kind, cells in sec["rows"]:
            for col, cell in zip(sec["cols"] or [], cells):
                ids = parse_cell(cell)
                if ids is None:
                    bad.append(f"{sec['id']} / {kind} / {col}: unparseable cell {cell[:80]!r}")
                    continue
                for pid in ids:
                    refs[pid] = refs.get(pid, 0) + 1
    return bad, refs


def expand(a: str, b: str) -> set[str]:
    pa, pb = a.split("."), b.split(".")
    if len(pa) == len(pb) == 2 and pa[0] == pb[0] and int(pa[1]) <= int(pb[1]) <= int(pa[1]) + 50:
        return {f"{pa[0]}.{n}" for n in range(int(pa[1]), int(pb[1]) + 1)}
    if len(pa) == len(pb) == 2 and pa[1] == pb[1] and int(pa[0]) <= int(pb[0]) <= int(pa[0]) + 50:
        return {f"{n}.{pa[1]}" for n in range(int(pa[0]), int(pb[0]) + 1)}
    return {a, b}


def shipped_ids(root: Path) -> set[str]:
    headings = []
    in_shipped = False
    for line in (root / ROADMAP_MD).read_text(encoding="utf-8").split("\n"):
        if line.startswith("## "):
            in_shipped = line.strip() == "## Shipped"
        elif in_shipped and line.startswith("### "):
            headings.append(line)
    for hist in sorted((root / "docs" / "history").glob("roadmap-shipped-*.md")):
        headings += [l for l in hist.read_text(encoding="utf-8").split("\n") if l.startswith("### ")]
    ids: set[str] = set()
    for h in (h for h in headings if "Pass" in h):
        ids |= set(PASS_TOKEN.findall(h))
        for a, b in PASS_RANGE.findall(h):
            ids |= expand(a, b)
    return ids


def stage_pending(root: Path, refs: dict[str, int]) -> list[str]:
    known = set(PASS_TOKEN.findall((root / ROADMAP_MD).read_text(encoding="utf-8")))
    shipped = shipped_ids(root)
    bad = []
    for pid in sorted(refs):
        if pid not in known:
            bad.append(f"pass {pid} ({refs[pid]} cell ref(s)) is not in {ROADMAP_MD}")
        elif pid in shipped:
            bad.append(f"pass {pid} ({refs[pid]} cell ref(s)) has shipped; drive the cell or re-point it")
    return bad


def features_rows(text: str) -> list[tuple[int, str, bool]]:
    """(1-based line, row text, object_bearing) for every data row."""
    out, h2, h3 = [], "", ""
    for n, line in enumerate(text.split("\n"), 1):
        if line.startswith("## "):
            h2, h3 = line[3:].strip(), ""
        elif line.startswith("### "):
            h3 = line[4:].strip()
        elif line.startswith("|") and not re.match(r"^\|\s*:?-", line) and not line.startswith("| core |"):
            bearing = h2 == PLANNED or (h2 == "Implemented" and h3 in OBJECT_SECTIONS)
            out.append((n, line, bearing))
    return out


def changed_lines(root: Path, base: str) -> set[int] | None:
    """Working-tree line numbers of FEATURES.md that differ from `base`."""
    try:
        diff = subprocess.run(
            ["git", "diff", "-U0", "--no-color", "--no-ext-diff", base, "--", FEATURES_MD],
            cwd=root, capture_output=True, text=True, encoding="utf-8", check=True,
            stdin=subprocess.DEVNULL,
        ).stdout
    except (subprocess.CalledProcessError, OSError):
        return None
    lines, cur = set(), 0
    for line in diff.split("\n"):
        if m := re.match(r"^@@ -\S+ \+(\d+)(?:,\d+)? @@", line):
            cur = int(m.group(1))
        elif line.startswith("+") and not line.startswith("+++"):
            lines.add(cur)
            cur += 1
    return lines


def stage_features(root: Path, base: str, kinds: list[str], baseline: set[str], counts: dict) -> list[str]:
    bad = []
    rows = features_rows((root / FEATURES_MD).read_text(encoding="utf-8"))
    changed = changed_lines(root, base)
    if changed is None:
        return [f"cannot diff {FEATURES_MD} against XCUT_BASE {base[:12]} (shallow clone? fetch-depth: 0)"]
    tagged: set[str] = set()
    c = {"object_rows": 0, "changed": 0, "tagged_rows": 0, "legacy_untagged": 0}
    for n, row, bearing in rows:
        tags = TAG.findall(row)
        c["object_rows"] += bearing
        c["tagged_rows"] += bool(tags)
        for tag in tags:
            if NONE_TAG.match(tag):
                continue
            if tag.startswith("none"):
                bad.append(f"{FEATURES_MD}:{n}: `xc: none` needs a quoted reason")
                continue
            ids = [t.strip() for t in tag.split(",")]
            unknown = [t for t in ids if t not in kinds]
            if unknown or not tag:
                bad.append(f"{FEATURES_MD}:{n}: xc tag names unknown kind(s) {unknown or [tag]}")
            tagged |= {t for t in ids if t in kinds}
        if bearing and n in changed:
            c["changed"] += 1
            if not tags:
                bad.append(f"{FEATURES_MD}:{n}: row changed since XCUT_BASE has no `<!-- xc: ... -->` tag: {row[:90]}")
        elif bearing and not tags:
            c["legacy_untagged"] += 1
    debt = {e.split(None, 1)[1] for e in baseline if e.startswith("untagged-kind ")}
    for k in kinds:
        if k not in tagged and k not in debt:
            bad.append(f"kind {k} is tagged on no FEATURES.md row and is not baselined")
    for k in sorted(debt):
        if k not in kinds:
            bad.append(f"STALE untagged-kind {k}: not a kind (delete the baseline line)")
        elif k in tagged:
            bad.append(f"STALE untagged-kind {k}: now tagged (delete the baseline line)")
    c["kinds_untagged"] = len([k for k in kinds if k not in tagged])
    counts.update(c)
    return bad


def stage_carry(root: Path, baseline: set[str], counts: dict) -> list[str]:
    found = set()
    crates = root / "crates"
    for path in sorted(crates.rglob("*.rs")) if crates.is_dir() else []:
        if "target" in path.relative_to(root).parts:
            continue
        for line in path.read_text(encoding="utf-8", errors="replace").split("\n"):
            m = CARRY_CONST.match(line) or CARRY_STRUCT.match(line)
            if m:
                found.add(f"{path.relative_to(root).as_posix()}::{m.group(1)}")
    known = {e.split(None, 1)[1] for e in baseline if e.startswith("carry ")}
    counts["carry_found"], counts["carry_baseline"] = len(found), len(known)
    bad = [f"NEW carry {e}: carry only via xcut::carry::clone_then_patch (decision 228)" for e in sorted(found - known)]
    bad += [f"STALE carry {e}: gone (delete the baseline line)" for e in sorted(known - found)]
    return bad


def read_baseline(root: Path) -> set[str]:
    p = root / BASELINE
    if not p.exists():
        return set()
    return {l.strip() for l in p.read_text(encoding="utf-8").split("\n") if l.strip() and not l.startswith("#")}


def run(root: Path, base: str = XCUT_BASE) -> tuple[dict[str, list[str]], dict]:
    kinds = parse_kinds((root / KIND_RS).read_text(encoding="utf-8"))
    ops = parse_ops((root / OP_RS).read_text(encoding="utf-8"))
    matrix = parse_matrix((root / MATRIX_MD).read_text(encoding="utf-8"))
    baseline = read_baseline(root)
    counts = {"kinds": len(kinds), "ops": sum(len(c) for _, c in ops),
              "cells": sum(len(s["rows"]) * len(s["cols"] or []) for s in matrix)}
    fails = {"freshness": stage_freshness(kinds, ops, matrix)}
    fails["grammar"], refs = stage_grammar(matrix)
    counts["pass_ids"], counts["pass_refs"] = len(refs), sum(refs.values())
    fails["pending"] = stage_pending(root, refs)
    fails["features"] = stage_features(root, base, kinds, baseline, counts)
    fails["carry"] = stage_carry(root, baseline, counts)
    return fails, counts


def report(fails: dict[str, list[str]], counts: dict) -> int:
    for stage in STAGES:
        for f in fails[stage]:
            print(f"FAIL [{stage}] {f}")
    print("cross-cutting: " + " ".join(f"{k}={v}" for k, v in counts.items()))
    red = [s for s in STAGES if fails[s]]
    if red:
        print(f"cross-cutting: FAIL -- {sum(len(fails[s]) for s in red)} finding(s) in {', '.join(red)}")
        return 1
    print(f"cross-cutting: clean -- {len(STAGES)} stages, XCUT_BASE {XCUT_BASE[:8]}")
    return 0


# --------------------------------------------------------------------------
# Self-test: a minimal good tree in a throwaway git repo, then one mutation
# per case; each mutation must turn exactly its listed stage(s) red.
# --------------------------------------------------------------------------

GOOD = {
    KIND_RS: 'macro_rules! object_kinds { ($v:ident => $id:literal,) => {} }\n'
             'object_kinds! {\n    Content {\n        Path => "content.path",\n    }\n'
             '    Annotation {\n        Stamp => "annot.stamp",\n    }\n}\n',
    OP_RS: 'ops! {\n    Clipboard => "clipboard" {\n        Copy => "copy",\n        Paste => "paste",\n    }\n}\n',
    MATRIX_MD: '# m\n\n## clipboard\n\n| kind | copy | paste |\n|---|---|---|\n'
               '| content.path | pass:900.1 | n/a:"§7 | why" |\n'
               '| annot.stamp | ok:translated(Rect),dropped(Popup->900.2),renumbered | refused:popup-travels |\n',
    ROADMAP_MD: '# r\n\n## Shipped\n\n### `Pass 899.0`-`899.2`: done\n\n## Next up\n\n'
                '- **`900.1`** one. **`900.2`** two. `899.1` cited.\n',
    FEATURES_MD: '# f\n\n## Implemented\n\n### Annotations & markup\n\n| core | cli | gui | Acrobat | Feature |\n'
                 '|:----:|:---:|:---:|:-------:|---------|\n| [x] | [x] | [ ] | [x] | Legacy row. |\n\n'
                 '### Shell & UX\n\n| core | cli | gui | Acrobat | Feature |\n|:-:|:-:|:-:|:-:|---|\n'
                 '| [x] | — | [ ] | — | Shell row. |\n\n## Planned, in predicted order\n\n'
                 '| core | cli | gui | Acrobat | Feature |\n|:-:|:-:|:-:|:-:|---|\n',
    BASELINE: '# debt\ncarry crates/x/src/a.rs::OldCarry\nuntagged-kind content.path\n',
    "crates/x/src/a.rs": 'pub(crate) struct OldCarry {\n    n: usize,\n}\nstruct Carried;\n',
}
NEW_ROW = '| [ ] | [ ] | [ ] | ? | New stamp row. <!-- xc: annot.stamp --> |\n'


def sub(path, old, new):
    return lambda files: files.__setitem__(path, files[path].replace(old, new, 1))


def add(path, text):
    return lambda files: files.__setitem__(path, files[path] + text)


CASES = [
    ("freshness+features", "kind added, matrix stale",sub(KIND_RS, '"annot.stamp",\n', '"annot.stamp",\n        Ink => "annot.ink",\n')),
    ("freshness", "op columns reordered", sub(MATRIX_MD, "| kind | copy | paste |", "| kind | paste | copy |")),
    ("freshness", "op group renamed", sub(OP_RS, '"clipboard"', '"clip"')),
    ("grammar", "unknown change", sub(MATRIX_MD, "renumbered |", "moved(Rect) |")),
    ("grammar", "n/a without reason", sub(MATRIX_MD, 'n/a:"§7 | why"', 'n/a:""')),
    ("grammar", "refused without cause", sub(MATRIX_MD, "refused:popup-travels", "refused:")),
    ("grammar", "blank cell", sub(MATRIX_MD, "pass:900.1", "")),
    ("pending", "pass id not in roadmap", sub(MATRIX_MD, "pass:900.1", "pass:999.9")),
    ("pending", "pass id shipped", sub(MATRIX_MD, "pass:900.1", "pass:899.0")),
    ("pending", "change gap shipped (range)", sub(MATRIX_MD, "Popup->900.2", "Popup->899.1")),
    ("features", "untagged new row", add(FEATURES_MD, '| [ ] | [ ] | [ ] | ? | Untagged new row. |\n')),
    ("features", "edited legacy row untagged", sub(FEATURES_MD, "Legacy row.", "Legacy row, edited.")),
    ("features", "tag names unknown kind", sub(FEATURES_MD + "#new", "annot.stamp", "annot.stamps")),
    ("features", "none without reason", sub(FEATURES_MD + "#new", "xc: annot.stamp", "xc: none")),
    ("features", "kind untagged, not baselined", sub(BASELINE, "untagged-kind content.path\n", "")),
    ("features", "stale untagged-kind", sub(FEATURES_MD + "#new", "xc: annot.stamp", "xc: annot.stamp, content.path")),
    ("features", "base unresolvable", "BADBASE"),
    ("carry", "new CARRIED_KEYS const", add("crates/x/src/a.rs", "const WIDGET_CARRIED_KEYS: &[&str] = &[];\n")),
    ("carry", "new CARRY_KEYS static", add("crates/x/src/a.rs", "pub static PAGE_CARRY_KEYS: [u8; 0] = [];\n")),
    ("carry", "new Carry struct", add("crates/x/src/a.rs", "pub struct LinkCarry;\n")),
    ("carry", "stale baseline entry", sub("crates/x/src/a.rs", "OldCarry", "OldReport")),
]


def git(cwd: Path, *args: str) -> None:
    subprocess.run(["git", "-c", "user.name=selftest", "-c", "user.email=selftest@invalid",
                    "-c", "core.autocrlf=false", *args],
                   cwd=cwd, check=True, capture_output=True, stdin=subprocess.DEVNULL)


def build(tmp: Path, mutate) -> str:
    """Commit GOOD as the base, apply the new row and `mutate` to the work tree."""
    for rel, text in GOOD.items():
        (tmp / rel).parent.mkdir(parents=True, exist_ok=True)
        (tmp / rel).write_bytes(text.encode())
    git(tmp, "init", "-q")
    git(tmp, "add", "-A")
    git(tmp, "commit", "-q", "-m", "base")
    base = subprocess.run(["git", "rev-parse", "HEAD"], cwd=tmp, capture_output=True, text=True,
                          check=True, stdin=subprocess.DEVNULL).stdout.strip()
    files = dict(GOOD)
    files[FEATURES_MD] += NEW_ROW
    if callable(mutate):
        files[FEATURES_MD + "#new"] = NEW_ROW
        mutate(files)
        files[FEATURES_MD] = files[FEATURES_MD].replace(NEW_ROW, files.pop(FEATURES_MD + "#new"))
    for rel, text in files.items():
        (tmp / rel).write_bytes(text.encode())
    return "0" * 40 if mutate == "BADBASE" else base


def counts_printed_on_failure() -> bool:
    """Stage 7: a red run still prints the counts line and exits 1."""
    buf = io.StringIO()
    with redirect_stdout(buf):
        code = report({s: (["x"] if s == "carry" else []) for s in STAGES}, {"kinds": 1})
    hit = code == 1 and "cross-cutting: kinds=1" in buf.getvalue()
    print(f"self-test {'caught' if hit else 'MISSED'}: [counts] counts line printed on a red run")
    return hit


def self_test() -> int:
    ok = True
    with tempfile.TemporaryDirectory(prefix="xcut-selftest-") as d:
        fails, _ = run(Path(d), build(Path(d), None))
        red = [s for s in STAGES if fails[s]]
        print(f"self-test control (good tree): {'clean' if not red else 'RED ' + str(fails)}")
        ok &= not red
    for stage, name, mutate in CASES:
        with tempfile.TemporaryDirectory(prefix="xcut-selftest-") as d:
            fails, _ = run(Path(d), build(Path(d), mutate))
        red = [s for s in STAGES if fails[s]]
        hit = red == stage.split("+")
        print(f"self-test {'caught' if hit else 'MISSED'}: [{stage}] {name}" + ("" if hit else f" (red: {red})"))
        ok &= hit
    ok &= counts_printed_on_failure()
    print(f"cross-cutting self-test: {'PASS' if ok else 'FAIL'} -- control + {len(CASES)} bad input(s) + counts-on-fail")
    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--root", default=str(Path(__file__).resolve().parent.parent))
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()
    return report(*run(Path(args.root)))


if __name__ == "__main__":
    sys.exit(main())
