"""Mark each standing rule in ROADMAP's index with the script that enforces it.

Mechanical and honest: a rule is marked `[gate: X]` when its own full text
names a script in `tools/`. Nothing else is marked, because "this rule is a
judgment call" is a claim about the rule that only a reader can make, and
guessing it for 182 rules would be exactly the confident-but-unverified prose
this whole trim is against.
"""
import pathlib, re, sys, io

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
ROOT = pathlib.Path(r"D:\Dev\pdfcer")

full = (ROOT / "docs" / "history" / "standing-rules-full.md").read_text(
    encoding="utf-8", errors="surrogateescape"
)
items, cur = [], None
for l in full.split("\n"):
    if l.startswith("- **"):
        if cur:
            items.append(cur)
        cur = [l]
    elif cur is not None:
        cur.append(l)
if cur:
    items.append(cur)

tools = {p.name for p in (ROOT / "tools").glob("*.py")} | {
    p.name for p in (ROOT / "tools").glob("*.sh")
}

gate_of = {}
for it in items:
    body = " ".join(it)
    m = re.match(r"- \*\*(R\d+)", it[0])
    if not m:
        continue
    hits = sorted({t for t in tools if t in body})
    if hits:
        gate_of[m.group(1)] = hits[:2]

p = ROOT / "docs" / "ROADMAP.md"
lines = p.read_text(encoding="utf-8", errors="surrogateescape").split("\n")
s = next(i for i, l in enumerate(lines) if l.strip() == "## Standing rules")
e = next(i for i, l in enumerate(lines) if l.strip() == "## Update protocol")

marked = 0
for i in range(s, e):
    m = re.match(r"^- `(R\d+)`", lines[i])
    if m and m.group(1) in gate_of and "[gate:" not in lines[i]:
        lines[i] = lines[i].rstrip() + "  **[gate: " + ", ".join(gate_of[m.group(1)]) + "]**"
        marked += 1

total = sum(1 for i in range(s, e) if re.match(r"^- `R\d+`", lines[i]))
note = [
    "",
    f"★★ **{marked} of {total} rules name a script that enforces them"
    f" (`[gate: …]` below). The other {total - marked} are followed by"
    " eyeball.**",
    "",
    "That second number is a **backlog, not a verdict** — a rule with no gate"
    " may be a genuine judgment call, and saying which is which is a reading"
    " nobody has done. What is certain is the failure mode: an unenforced rule"
    " is approximated, drifts, gets caught, and produces another paragraph"
    " about the drift. Operator, 2026-09-10: **script it or bin it.**",
    "",
    "The marks are derived, not maintained: a rule is marked when its own full"
    " text names a file in `tools/`. Re-run"
    " `tools/annotate-rule-gates.py` after adding a rule or a gate.",
]
# insert the note after the section's existing preamble (before the first rule)
first_rule = next(i for i in range(s, e) if re.match(r"^- `R\d+`", lines[i]))
lines = lines[:first_rule] + note + lines[first_rule:]
p.write_text("\n".join(lines), encoding="utf-8", errors="surrogateescape")
print(f"marked {marked} of {total} rules with an enforcing script")
