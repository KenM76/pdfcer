#!/usr/bin/env python3
"""Fail if a field an OUTCOME struct computes is surfaced by no shell.

WHAT THIS IS FOR
================

`pdfce-core` returns outcome structs — `MergeOutcome`, `AdoptOutcome`,
`VertexOutcome`, `FlattenOutcome` and their siblings — whose fields exist for
exactly one reason: **project rule 4's disclosure obligation.** They carry the
things pdfce decided, inferred, renamed, dropped or could not do, which the
operator has no other way to learn. A field on one of these is not data a
caller may use; it is a statement a caller OWES the operator.

**So a field no shell reads is a disclosure that does not happen.** It is
computed, returned, and dropped on the floor, and the effect is identical to
never having computed it. In `pdfce-cli` that is worse than in a GUI: the
invocation IS the commit (rule 11), so there is no later screen to find the
number on and no undo to reconsider with.

WHY IT IS A GATE, AND NOT VIGILANCE
===================================

On 2026-08-20, `Pass 106.1` added three fields to `MergeOutcome` —
`named_destinations_carried`, `named_destinations_renamed` and
`outline_items_carried` — and `pdfce-cli`'s `merge-document` never grew the
tokens. Three numbers computed, returned and discarded, one of them describing
a **cross-file breakage**: pdfce rewrites the bookmarks it carried to the new
keys but cannot rewrite a link it did not copy, so a `/GoToR` in a THIRD
document now silently resolves to the wrong destination.

**Nothing type-checks that.** A struct field with no reader is not a warning.
`#[must_use]` is about the struct, not its fields — reading ONE field consumes
the value and satisfies the compiler forever. And R151 is about a `pub fn` with
no caller, which is the adjacent case, not this one. It was found by the
librarian while filing a different Pass, entirely by luck, and fixed as
`Pass 106.2`.

The same morning, in an unrelated subsystem, `Pass 108.0` fixed a *setting*
that was parsed, validated, written and read by nothing. Two instances of "a
value that exists and is never consumed", in one session, in two directions.
`tools/check-settings-consumed.py` already covers the inbound direction —
structs a CALLER fills and hands to an API. This covers the outbound one:
structs an API fills and hands to a CALLER.

WHAT COUNTS AS A CONSUMER
=========================

A read, in a SHELL — `crates/pdfce-cli/src`. (Until Pass 247.0 the in-repo
GUI crate was the second shell scanned; it is gone, and the separate
pdfcer-gui project is not on this disk's tree, so the CLI is the ONLY shell
this gate can see. A field the CLI does not surface is therefore a
disclosure this repository cannot prove happens anywhere.)
Deliberately not `crates/pdfce-core`: core's own tests read outcome fields
constantly (`assert_eq!(out.pages_merged, 2)`), and a round trip through a test
proves the field is computed, not that anybody is told. The obligation is to
the operator, so it is discharged in the layer that talks to one.

`D:\\dev\\pdfceGUI` is the live GUI and is OUT OF TREE, so this gate cannot see
it. That is a real limit and it is the reason the exemption table below exists
rather than being a purity failure — see `InsertOutcome`.

Reads are distinguished from writes exactly as `check-settings-consumed.py`
does it, and for the same reason its comments give at length: a naive `.field`
search matches an assignment and reports clean on a field nobody reads. The
pattern here is simpler because outcome structs are built with struct-literal
syntax (`MergeOutcome { pages_merged: n, .. }`), which contains `field:` and not
`.field` — so core's construction cannot satisfy this check by accident.

THE EXEMPTION TABLE, AND WHY IT IS NOT A BASELINE
=================================================

Some fields legitimately have no reader:

* **Handles.** `ImageAuthorOutcome::content_id` is an object id, like
  `image_id` and `soft_mask_id` beside it. A handle is offered so a caller CAN
  refer to the thing later; it makes no claim about the document and discloses
  nothing. `image_id` happens to be read and `content_id` is not, and neither
  fact means anything.
* **A struct whose only shell is out of tree.** `InsertOutcome` belongs to
  `EditSession::insert_pages`, which `pdfce-cli` deliberately does not expose —
  a one-shot invocation has no open session to insert into, and the CLI's
  `insert-pages` is the OTHER verb (`pageops::insert`). `docs/FEATURES.md`
  records that `cli` box as `—`, not a gap. `pdfceGUI` surfaces all five
  fields; this gate cannot see that repository.

Each exemption states its reason **in this file**, next to the name. That is
the difference between an exemption and a baseline: a baseline is a list of
things nobody has looked at, and it grows by default. This list requires
somebody to write down why, which is the whole cost that keeps it honest.

Adding a struct to `OUTCOME_STRUCTS` is how this gate grows. It does not
discover them, deliberately — auto-discovery by name (`*Outcome`) would silently
change what the gate checks when somebody renames a type, and a gate whose
scope moves without anybody deciding is the shape this project has been bitten
by twice (see `tools/check-string-gaps.sh`'s and
`tools/check-ledger-numbers.py`'s widening notes).

USAGE
=====

    python tools/check-outcome-disclosed.py [--self-test]

Exit 0 clean, 1 on a finding, 2 on a stale list (a struct or file that moved —
reported rather than skipped, because a gate that silently checks nothing is
the failure mode all of this exists to prevent).
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# The structs whose fields are disclosures. Grown by hand — see the module
# docstring on why this is not auto-discovered from the `*Outcome` suffix.
OUTCOME_STRUCTS: list[tuple[str, str]] = [
    ("crates/pdfce-core/src/edit.rs", "FieldAuthorOutcome"),
    ("crates/pdfce-core/src/edit.rs", "InsertOutcome"),
    ("crates/pdfce-core/src/edit.rs", "MergeOutcome"),
    ("crates/pdfce-core/src/edit.rs", "VertexOutcome"),
    ("crates/pdfce-core/src/edit.rs", "AdoptOutcome"),
    ("crates/pdfce-core/src/edit.rs", "DeleteOutcome"),
    ("crates/pdfce-core/src/edit.rs", "ResetOutcome"),
    ("crates/pdfce-core/src/edit.rs", "FillOutcome"),
    ("crates/pdfce-core/src/edit.rs", "RegenOutcome"),
    ("crates/pdfce-core/src/edit.rs", "ImportOutcome"),
    ("crates/pdfce-core/src/edit.rs", "FlattenOutcome"),
    ("crates/pdfce-core/src/edit.rs", "ImageAuthorOutcome"),
    # `Pass 119.0`. Not in `edit.rs`, and that is the point: the gate's list was
    # written from ONE file, so every report type living in a submodule was
    # outside it while the summary line read "clean". `EditReport` gained three
    # fields that day -- including `form_invocations`, whose whole purpose is to
    # stop a shell changing six drawing sheets while showing one -- and the gate
    # would have stayed green if all three had been dropped on the floor.
    ("crates/pdfce-core/src/text_edit/edit.rs", "EditReport"),
    ("crates/pdfce-core/src/text_edit/format.rs", "FormatReport"),
    # `Pass 113.0` / `Pass 120.0`. Added when each landed rather than
    # afterwards -- the `EditReport` lesson was that a struct outside this list
    # is a struct whose fields can be dropped on the floor while the gate
    # reports clean.
    ("crates/pdfce-core/src/edit.rs", "TransformOutcome"),
    ("crates/pdfce-core/src/edit.rs", "PasteOutcome"),
    # `Pass 183.0`. Not named `*Outcome`, and it is the strongest case on the
    # list: every field of `SubmitDisclosure` describes something a
    # `/SubmitForm` button would send that the operator CANNOT SEE by any other
    # means -- hidden field values, a password field, a local file carried off
    # the machine, the document's own path. A field dropped on the floor here
    # is not an unstated number, it is an undisclosed exfiltration.
    ("crates/pdfce-core/src/edit.rs", "SubmitDisclosure"),
    # `Pass 183.1`. Smaller than its sibling and on the list for the same
    # reason: every field is the gap between what the operator NAMED and what
    # will actually move -- widgets on pages they were not looking at, and
    # named fields that own no widget at all, for which the button they just
    # made does nothing.
    ("crates/pdfce-core/src/edit.rs", "HideDisclosure"),
    # `Pass 184.0` criterion A. `FieldRename` has carried a rule-4 disclosure
    # since it was written -- `descendants_renamed`, the count that tells an
    # operator a one-field request renamed six -- and was never on this list,
    # which is the `EditReport` lesson again: a struct outside the list is a
    # struct whose fields can be dropped on the floor while the gate reports
    # clean. Added when the second disclosure landed rather than afterwards.
    ("crates/pdfce-core/src/edit.rs", "FieldRename"),
]

# `Struct::field` -> why no shell reads it. A reason is mandatory: see the
# docstring on why this is an exemption table and not a baseline.
EXEMPT: dict[str, str] = {
    "ImageAuthorOutcome::content_id": (
        "a HANDLE, not a disclosure — the object id of the created content "
        "stream, offered so a caller can refer to it later. It states nothing "
        "about the document. `image_id` beside it happens to be read and this "
        "one is not; neither fact means anything."
    ),
    # All five of InsertOutcome's disclosure fields, one reason.
    "InsertOutcome::pages_inserted": (
        "`EditSession::insert_pages` has no `pdfce-cli` verb BY DECISION — a "
        "one-shot invocation has no open session to insert into, and the "
        "CLI's `insert-pages` is the other verb (`pageops::insert`). "
        "`docs/FEATURES.md` records that box as `—`, not a gap. `pdfceGUI` "
        "surfaces all five fields and is out of tree, so this gate cannot see "
        "it. ★ REMOVE THIS EXEMPTION the moment a session verb reaches the "
        "CLI: at that point the disclosure obligation lands here too."
    ),
    "InsertOutcome::orphaned_widgets": "see InsertOutcome::pages_inserted",
    "InsertOutcome::orphaned_widgets_unrecoverable": (
        "see InsertOutcome::pages_inserted"
    ),
    "InsertOutcome::source_outline_dropped": "see InsertOutcome::pages_inserted",
    "InsertOutcome::source_page_labels_dropped": (
        "see InsertOutcome::pages_inserted"
    ),
}

CONSUMER_ROOTS = [
    ROOT / "crates" / "pdfce-cli" / "src",
]


def named_struct_fields(source: str, name: str) -> list[str]:
    """Every `pub` field of a named struct, in declaration order.

    Anchored on `pub struct NAME {` … the first line that is exactly `}` at
    column 0, which is how the sibling gate parses the same shapes. Doc
    comments and attributes are skipped by requiring `pub ` at the start of
    the field line.
    """
    match = re.search(rf"pub struct {re.escape(name)} \{{(.*?)\n\}}", source, re.DOTALL)
    if not match:
        return []
    return re.findall(r"^\s*pub ([a-z_][a-z0-9_]*):", match.group(1), re.MULTILINE)


def shell_text(roots: list[pathlib.Path]) -> str:
    """Every `.rs` byte under the shell crates, concatenated."""
    parts: list[str] = []
    for root in roots:
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*.rs")):
            try:
                parts.append(path.read_text(encoding="utf-8"))
            except OSError:
                continue
    return "\n".join(parts)


def is_read(field: str, text: str) -> bool:
    """Whether `text` READS `.field` — an assignment does not count.

    `[^=]` after `=` keeps `==` a read.

    ★ THE WHITESPACE GOES INSIDE THE LOOKAHEAD, AND THAT IS NOT A STYLE
    CHOICE. The obvious spelling — `\\b\\s*(?!=[^=])` — is **broken**, and the
    self-test's decoy case caught it on the first run. `\\s*` outside the
    lookahead can BACKTRACK TO ZERO: the engine matches `.renamed`, lets
    `\\s*` consume nothing, and then asks whether the very next character is
    `=`. For `o.renamed = 3` the next character is a SPACE, so the negative
    lookahead succeeds and an assignment is counted as a read. The filter that
    is the entire point of this function silently does nothing on the one
    spelling that matters.

    Writing it as `\\b(?!\\s*=[^=])` makes the whitespace part of the thing
    being forbidden, which is what was meant all along.

    ★ `tools/check-settings-consumed.py` CARRIES THE BROKEN SPELLING TODAY.
    Its comment says it was sabotage-verified — and it was, against a defect
    whose write form was a `&mut` BORROW, which its separate borrow filter
    catches. The `=` filter beside it was never the thing under test. Filed;
    do not copy that line, and if you are fixing it, this is the shape.

    A `&mut` borrow is a write there and is not filtered here, deliberately:
    nothing hands an outcome field to a widget by mutable reference, because
    an outcome is a report the caller RECEIVED rather than a control it owns.
    If that ever changes, copy the sibling's borrow filter across rather than
    reasoning about it again.
    """
    return re.search(rf"\.\s*{re.escape(field)}\b(?!\s*=[^=])", text) is not None


def check(roots: list[pathlib.Path], structs: list[tuple[str, str]]) -> list[str]:
    problems: list[str] = []
    text = shell_text(roots)
    for rel, name in structs:
        path = ROOT / rel
        if not path.is_file():
            problems.append(f"`{name}`: {rel} not found — this gate's list is stale")
            continue
        fields = named_struct_fields(path.read_text(encoding="utf-8"), name)
        if not fields:
            problems.append(
                f"`{name}`: no `pub` fields found — either the struct moved or "
                f"this gate's parser is stale. Refusing to report clean."
            )
            continue
        for field in fields:
            key = f"{name}::{field}"
            if key in EXEMPT:
                continue
            if not is_read(field, text):
                problems.append(
                    f"`{key}` is computed by pdfce-core and READ BY NO SHELL. "
                    f"Rule 4: a disclosure nobody emits is a disclosure that "
                    f"does not happen. Print it, surface it, or add it to "
                    f"EXEMPT with the reason it needs no reader."
                )
    return problems


def self_test() -> None:
    """A dirty case and a clean case, both synthetic.

    The dirty case is the shape `Pass 106.1` actually shipped: a struct that
    gains a field while its consumer keeps printing the old ones. The clean
    case pins the two things that must NOT trip it — a struct-literal
    construction (which contains `field:`, not `.field`) and an assignment.
    """
    import tempfile

    fail = 0
    with tempfile.TemporaryDirectory() as tmp:
        base = pathlib.Path(tmp)
        core = base / "core"
        core.mkdir()
        (core / "lib.rs").write_text(
            "pub struct DemoOutcome {\n"
            "    /// read by the shell\n"
            "    pub carried: usize,\n"
            "    /// nobody reads this one\n"
            "    pub renamed: usize,\n"
            "}\n",
            encoding="utf-8",
        )

        dirty = base / "dirty"
        dirty.mkdir()
        (dirty / "main.rs").write_text(
            'fn go(o: DemoOutcome) { println!("carried={}", o.carried); }\n',
            encoding="utf-8",
        )
        clean = base / "clean"
        clean.mkdir()
        (clean / "main.rs").write_text(
            'fn go(o: DemoOutcome) {\n'
            '    println!("carried={} renamed={}", o.carried, o.renamed);\n'
            "}\n",
            encoding="utf-8",
        )
        # Neither of these is a read, and both must leave the gate red in the
        # dirty tree: a struct literal names `renamed:` and an assignment
        # writes `.renamed =`.
        decoy = base / "decoy"
        decoy.mkdir()
        (decoy / "main.rs").write_text(
            'fn build() -> DemoOutcome { DemoOutcome { carried: 1, renamed: 2 } }\n'
            "fn set(o: &mut DemoOutcome) { o.renamed = 3; }\n"
            'fn also(o: DemoOutcome) { println!("carried={}", o.carried); }\n',
            encoding="utf-8",
        )

        global ROOT
        saved_root = ROOT
        ROOT = base
        try:
            structs = [("core/lib.rs", "DemoOutcome")]
            if not check([dirty], structs):
                print("SELF-TEST FAILED: an unread outcome field was not detected")
                fail = 1
            if check([clean], structs):
                print("SELF-TEST FAILED: a read field was reported as unread")
                fail = 1
            if not check([decoy], structs):
                print(
                    "SELF-TEST FAILED: a struct literal or an assignment was "
                    "counted as a read — that is exactly how the defect stays "
                    "invisible"
                )
                fail = 1
            # A stale list must be reported, never silently skipped.
            if not check([clean], [("core/missing.rs", "DemoOutcome")]):
                print("SELF-TEST FAILED: a missing file was not reported")
                fail = 1
        finally:
            ROOT = saved_root

    if fail:
        sys.exit(1)
    print("check-outcome-disclosed self-test: PASS")
    sys.exit(0)


def main() -> None:
    if "--self-test" in sys.argv:
        self_test()
    problems = check(CONSUMER_ROOTS, OUTCOME_STRUCTS)
    if not problems:
        total = 0
        for rel, name in OUTCOME_STRUCTS:
            path = ROOT / rel
            if path.is_file():
                total += len(named_struct_fields(path.read_text(encoding="utf-8"), name))
        print(
            f"outcome-disclosed: clean — {total} field(s) across "
            f"{len(OUTCOME_STRUCTS)} outcome struct(s); "
            f"{len(EXEMPT)} exempt with a stated reason."
        )
        sys.exit(0)
    print(f"outcome-disclosed: {len(problems)} problem(s):", file=sys.stderr)
    for problem in problems:
        print(f"  - {problem}", file=sys.stderr)
    print(
        "\nAn outcome field exists to be TOLD to the operator. Computing one "
        "and not emitting it has the same effect as never computing it, and "
        "in pdfce-cli the invocation is the commit — there is no later screen.",
        file=sys.stderr,
    )
    sys.exit(1)


if __name__ == "__main__":
    main()
