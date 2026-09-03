#!/usr/bin/env python3
"""gen-form-hierarchy-fixtures.py — the field-tree shapes the corpus does not contain.

WHY THIS SCRIPT EXISTS
======================
`docs/ROADMAP.md`'s Pass 7.0 entry records the census result that motivates
every fixture here:

    corpus max = 63 fields/file ... 1x on depth but **no corpus file nests
    fields at all**

pdfcer's field-hierarchy handling has therefore, in effect, **never been
exercised against real data**. Every non-terminal code path in
`forms::walk_field` — inheritance down `/Kids`, fully-qualified-name
composition, widget-kid classification — has only ever run on flat files
where it does nothing.

That was survivable while pdfcer only READ forms: a reader that mishandles a
shape no input file contains mishandles nothing. Decision 020 (form-field
authoring) ends that, because authoring's entire purpose is to start
GENERATING precisely those shapes — a merge attaches a second widget to an
existing field, and Shape A->B promotion turns a merged field into a
`/Kids` parent. The shapes stop being hypothetical the moment F1 ships.

So these five fixtures are built FIRST, the existing verbs are run over
them, and what breaks is fixed before any authoring code depends on it.
That ordering is decision 020 §3.3.3's F0, and it follows decision 019's
Pass 19.0 precedent ("CORRECTNESS ONLY, no new operator surface").

THE FIVE FIXTURES (decision 020 §6, F0)
=======================================
| File | Shape | What it exists to catch |
|---|---|---|
| `multi-widget-form.pdf` | (a) one terminal field, 3 kid widgets, across 2 pages | Fan-out: one fill must paint three widgets on two different pages; flatten must burn each onto its OWN page |
| `nested-form.pdf` | (b) 2-level non-terminal hierarchy | FQN composition (`Personal.Address.Zip`) and attribute inheritance down two levels |
| `radio-group-form.pdf` | (c) radio group, 3 kids, distinct on-states | Button state selection across a `/Kids` group with a shared `/V` |
| `mixed-kids-form.pdf` | (d) node with BOTH field-kids and widget-kids | The `walk_field` early-return that silently DROPS widget-kids |
| `xfa-hybrid-form.pdf` | (e) static-XFA hybrid | That authoring refuses (§3.2.2) while reading still works |

Fixture (d) is the sharpest of the five. §12.7.3.1's merge rule is stated
in terms of what a kid IS, not what its siblings are, so a node may legally
hold a child field AND a bare widget of its own. `walk_field` recurses into
the child fields and RETURNS — the widget-kids never reach `out`. No corpus
file has the shape, so nothing has ever caught it.

FIXTURE (e) IS NOT AN XFA IMPLEMENTATION
========================================
The `/XFA` array here holds a minimal, well-formed XDP packet skeleton. It
exists so `detect_xfa` reports `PacketArray`, so the authoring refusal is
reachable, and so the AcroForm half can be read normally — which is the
whole point of a HYBRID. pdfcer parses none of the XML and must not start.

BYTE-AUTHORED, NO PDF LIBRARY (LEGAL.md §5, project rule 7)
===========================================================
Every byte is emitted by this script. No library sits between the intent
and the file, so a fixture cannot inherit a bug — or a silent
normalization — from the very code it is meant to test. This is `LEGAL.md`
§5 category (a): wholly synthetic, no third-party source, no attribution
owed or claimed.

Each file uses a classic §7.5.4 cross-reference TABLE (not a stream), no
encryption, no object streams, and the bare §9.6.2.1 four-key standard-14
font form, so a parser defect in any of those layers cannot masquerade as
a field-tree defect.

USAGE
=====
    python tools/gen-form-hierarchy-fixtures.py

Writes into `fixtures/synthetic/forms/`, overwriting. Run from the repo
root. Deterministic: identical bytes on every run, so a regenerated
fixture that differs means the script changed.
"""

from __future__ import annotations

from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "forms"


def stream_obj(dict_prefix: bytes, content: bytes) -> bytes:
    """A stream object body: the dict, its computed `/Length`, and the data.

    `/Length` is computed rather than written by hand because a wrong
    `/Length` is the single most common hand-authored-PDF defect, and it
    fails in a way that looks like a lexer bug.
    """
    return dict_prefix + b" /Length %d >>\nstream\n" % len(content) + content + b"\nendstream"


def assemble(objs: dict[int, bytes]) -> bytes:
    """Serialize numbered objects into a §7.5.4 cross-reference-table file.

    Offsets are recorded as each object is appended, so the table cannot
    drift from the body. Object numbers must be contiguous from 1: the
    single-subsection `xref` header written here (`0 <size>`) says they are,
    and a gap would make every later offset point at the wrong object.
    """
    assert sorted(objs) == list(range(1, max(objs) + 1)), "object numbers must be contiguous from 1"
    buf = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"
    off: dict[int, int] = {}
    for n in sorted(objs):
        off[n] = len(buf)
        buf += b"%d 0 obj\n" % n + objs[n] + b"\nendobj\n"
    xref_at = len(buf)
    size = max(objs) + 1
    buf += b"xref\n0 %d\n0000000000 65535 f \n" % size
    for n in range(1, size):
        buf += b"%010d 00000 n \n" % off[n]
    buf += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (size, xref_at)
    return buf


# The `/DR` every fixture shares: one standard-14 face in the bare
# §9.6.2.1 four-key form, which is what the `/DA` strings resolve `/Helv`
# against. Written inline rather than as an indirect object so the AcroForm
# dict is readable as one unit in a hex dump.
DR = (
    b"/DR << /Font << /Helv << /Type /Font /Subtype /Type1 "
    b"/BaseFont /Helvetica /Encoding /WinAnsiEncoding >> >> >>"
)


def write(name: str, data: bytes) -> None:
    (OUT / name).write_bytes(data)
    print(f"wrote {name}: {len(data)} bytes")


# ---------------------------------------------------------------------------
# (a) One terminal field, three kid widgets, spread across TWO pages.
# ---------------------------------------------------------------------------
def fixture_multi_widget() -> bytes:
    """A single field `Reference` with three widgets on two pages.

    This is the shape a MERGE produces, which is why it is fixture (a): F1's
    whole purpose is to generate it, so every existing verb must already
    handle it before F1 may run.

    Three widgets, not two, and across two pages, not one. Two widgets on
    one page would let an off-by-one (`widgets[0]` only, or
    `widgets[..len-1]`) pass, and a single page would let a flatten that
    burns every widget onto the FIRST page pass. Both are the errors this
    shape actually invites.

    Note what the field dict does NOT carry: no `/Subtype`, no `/Rect`, no
    `/AP`. It is Shape B (`/Kids` widgets), so the field is not itself an
    annotation, and the pages' `/Annots` reference the WIDGETS. A file that
    got this wrong would point `/Annots` at a dict with no `/Subtype
    /Widget` — exactly the state decision 020 §3.1.5 step 4 warns the
    promotion path must not leave behind.
    """
    objs: dict[int, bytes] = {}
    objs[1] = (
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [6 0 R] "
        b"/DA (/Helv 0 Tf 0 g) " + DR + b" >> >>"
    )
    objs[2] = b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>"
    # Page 1 holds two of the three widgets; page 2 holds the third.
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << >> "
        b"/Annots [7 0 R 8 0 R] >>"
    )
    objs[4] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << >> "
        b"/Annots [9 0 R] >>"
    )
    # Object 5 is deliberately a plain content stream, so the numbering has
    # a non-form object in the middle and nothing may assume "every object
    # is a field".
    objs[5] = stream_obj(b"<<", b"BT ET")
    # THE FIELD: value + type + name here, appearance nowhere.
    objs[6] = (
        b"<< /FT /Tx /T (Reference) /TU (Reference number, repeated on every page) "
        b"/DA (/Helv 9 Tf 0 g) /V (R-1000) /Kids [7 0 R 8 0 R 9 0 R] >>"
    )
    # THE THREE WIDGETS: /T-less, /Parent back to the field, /P to their page.
    objs[7] = (
        b"<< /Type /Annot /Subtype /Widget /Parent 6 0 R /Rect [20 150 160 172] "
        b"/P 3 0 R /F 4 /MK << /BC [0 0 0] >> >>"
    )
    objs[8] = (
        b"<< /Type /Annot /Subtype /Widget /Parent 6 0 R /Rect [20 40 160 62] "
        b"/P 3 0 R /F 4 /MK << /BC [0 0 0] >> >>"
    )
    objs[9] = (
        b"<< /Type /Annot /Subtype /Widget /Parent 6 0 R /Rect [20 150 160 172] "
        b"/P 4 0 R /F 4 /MK << /BC [0 0 0] >> >>"
    )
    return assemble(objs)


# ---------------------------------------------------------------------------
# (b) A two-level non-terminal hierarchy: `Personal.Address.Zip`.
# ---------------------------------------------------------------------------
def fixture_nested() -> bytes:
    """Two levels of grouping above two terminal fields.

    Tree:

        Personal                (non-terminal, no /FT of its own)
          +- Address            (non-terminal, declares /DA for its subtree)
          |    +- Zip           (terminal /Tx)  -> `Personal.Address.Zip`
          |    +- City          (terminal /Tx)  -> `Personal.Address.City`
          +- Name               (terminal /Tx)  -> `Personal.Name`

    Three things are being tested at once, and each is placed deliberately:

    1. **FQN composition across two levels.** `Personal.Address.Zip` cannot
       be produced by a reader that concatenates only one level, and cannot
       be produced at all by a flat reader.
    2. **Inheritance from a NON-TERMINAL.** `/FT /Tx` is declared on
       `Personal` and on neither terminal; `/DA` is declared on `Address`
       and on neither of its children. A reader that only reads a terminal
       node's own keys reports `field_type: None` and no `/DA` — and then
       refuses to fill a perfectly fillable field.
    3. **A sibling at a DIFFERENT depth.** `Personal.Name` sits one level
       up from `Personal.Address.Zip`, so a reader that tracks depth with a
       single counter rather than per-branch state gets the wrong prefix for
       whichever it visits second.

    `Personal` is the node F1's `NameIsGroupingNode` refusal fires on: it
    bears a name, it is not a terminal, and Table 220 gives it no type of
    its own.
    """
    objs: dict[int, bytes] = {}
    objs[1] = (
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] "
        b"/DA (/Helv 0 Tf 0 g) " + DR + b" >> >>"
    )
    objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 260] /Resources << >> "
        b"/Annots [6 0 R 7 0 R 8 0 R] >>"
    )
    # `Personal`: the type lives here and is inherited by all three terminals.
    objs[4] = b"<< /T (Personal) /FT /Tx /Kids [5 0 R 8 0 R] >>"
    # `Address`: declares the subtree's /DA, inherited by Zip and City.
    objs[5] = b"<< /T (Address) /DA (/Helv 8 Tf 0 0 1 rg) /Kids [6 0 R 7 0 R] /Parent 4 0 R >>"
    # The terminals are MERGED (Shape A): field keys and widget keys in one
    # dict, which is the common real-world shape and the one `dict_is_widget`
    # must recognise even on a node that also has a `/Parent`.
    objs[6] = (
        b"<< /T (Zip) /Parent 5 0 R /V (K7L 1A1) /Type /Annot /Subtype /Widget "
        b"/Rect [20 200 140 222] /P 3 0 R /F 4 /MK << /BC [0 0 0] >> >>"
    )
    objs[7] = (
        b"<< /T (City) /Parent 5 0 R /V (Kingston) /Type /Annot /Subtype /Widget "
        b"/Rect [20 160 140 182] /P 3 0 R /F 4 /MK << /BC [0 0 0] >> >>"
    )
    objs[8] = (
        b"<< /T (Name) /Parent 4 0 R /V (A. Operator) /Type /Annot /Subtype /Widget "
        b"/Rect [20 120 240 142] /P 3 0 R /F 4 /MK << /BC [0 0 0] >> >>"
    )
    return assemble(objs)


# ---------------------------------------------------------------------------
# (c) A radio group: three kids with distinct on-states.
# ---------------------------------------------------------------------------
def fixture_radio_group() -> bytes:
    """A `/Btn` `Radio` field whose three widget kids have distinct on-states.

    Distinct from `radio-choice-form.pdf`, which exists for the GUI panel:
    this one is deliberately minimal and exists to prove the CORE path over
    a `/Kids` group — `/V` on the parent, `/AS` on each kid, exclusivity
    when one is selected.

    Each widget's `/AP /N` carries TWO keys: its own on-state and `/Off`.
    That is §12.7.4.2.3's requirement and the reason a radio group cannot be
    modelled as "one appearance per widget": selecting `Green` must set
    widget 2's `/AS` to `/Green` and widgets 1 and 3's to `/Off`, so every
    widget needs an `/Off` stream to switch TO.

    The on-state names are `/Red`, `/Green`, `/Blue` — not `/1`, `/2`, `/3`.
    Positional names are legal (Table 227) but decision 020 §8.3 records
    that pdfcer cannot resolve them to export values, so a fixture using them
    would be testing a shape pdfcer has explicitly not committed to.
    """
    objs: dict[int, bytes] = {}
    objs[1] = (
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] "
        b"/DA (/Helv 0 Tf 0 g) " + DR + b" >> >>"
    )
    objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << >> "
        b"/Annots [5 0 R 6 0 R 7 0 R] >>"
    )
    # /Ff 32768 = bit 16 (Radio). /V names the currently-on state.
    objs[4] = b"<< /FT /Btn /T (Priority) /Ff 32768 /V /Green /Kids [5 0 R 6 0 R 7 0 R] >>"
    for num, (state, x) in enumerate(
        [(b"Red", 20), (b"Green", 70), (b"Blue", 120)], start=5
    ):
        on = b"/AS /%s" % state if state == b"Green" else b"/AS /Off"
        objs[num] = (
            b"<< /Type /Annot /Subtype /Widget /Parent 4 0 R /Rect [%d 100 %d 114] "
            b"/P 3 0 R /F 4 %s /AP << /N << /%s 8 0 R /Off 9 0 R >> >> >>"
            % (x, x + 14, on, state)
        )
    objs[8] = stream_obj(b"<< /Type /XObject /Subtype /Form /BBox [0 0 14 14]", b"0 0 14 14 re f")
    objs[9] = stream_obj(b"<< /Type /XObject /Subtype /Form /BBox [0 0 14 14]", b"0 0 14 14 re S")
    return assemble(objs)


# ---------------------------------------------------------------------------
# (d) A node with BOTH field-kids and widget-kids. The sharpest fixture.
# ---------------------------------------------------------------------------
def fixture_mixed_kids() -> bytes:
    """A node whose `/Kids` holds a child FIELD and a bare WIDGET of its own.

    Tree:

        Order                   /FT /Tx, /V (ORD-77), /Kids [ widget, Qty ]
          +- (widget)           /T-less  -> a widget OF `Order`
          +- Qty                /T (Qty) -> terminal `Order.Qty`

    §12.7.3.1's merge rule classifies each kid INDIVIDUALLY — a kid with its
    own `/T` is a child field, a `/T`-less widget kid is one of the parent's
    appearances. Nothing in the spec says a node must pick one KIND of kid.

    `walk_field` does pick one. It partitions `/Kids`, and if ANY kid is a
    field it recurses into those and returns — so `Order`'s own widget is
    never modelled, `Order` never reaches `out`, and the field with a real
    value and a real rectangle on the page vanishes from `list-fields`
    entirely. The page's `/Annots` still references the widget, so the
    document renders it: the field is on screen and absent from the form.

    No corpus file has this shape (Pass 7.0's census: no file nests fields
    at all), which is why the defect has survived. It is fixture (d) because
    F1 can generate it: merging a widget onto a node that already has a
    child field produces exactly this.

    `Order` deliberately carries a `/V` and a `/Rect` of its own so the loss
    is measurable rather than cosmetic — a dropped field with no value would
    only shrink a count.
    """
    objs: dict[int, bytes] = {}
    objs[1] = (
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] "
        b"/DA (/Helv 0 Tf 0 g) " + DR + b" >> >>"
    )
    objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << >> "
        b"/Annots [5 0 R 6 0 R] >>"
    )
    # `Order` is a TERMINAL field (it has /FT and /V) that ALSO has a child
    # field. Its own presence is the /T-less widget kid, object 5.
    objs[4] = b"<< /FT /Tx /T (Order) /V (ORD-77) /Kids [5 0 R 6 0 R] >>"
    objs[5] = (
        b"<< /Type /Annot /Subtype /Widget /Parent 4 0 R /Rect [20 150 200 172] "
        b"/P 3 0 R /F 4 /MK << /BC [0 0 0] >> >>"
    )
    objs[6] = (
        b"<< /T (Qty) /Parent 4 0 R /V (3) /Type /Annot /Subtype /Widget "
        b"/Rect [20 100 80 122] /P 3 0 R /F 4 /MK << /BC [0 0 0] >> >>"
    )
    return assemble(objs)


# ---------------------------------------------------------------------------
# (e) A static-XFA hybrid.
# ---------------------------------------------------------------------------
def fixture_xfa_hybrid() -> bytes:
    """An AcroForm that ALSO carries an `/XFA` packet array (§12.7.8).

    "Hybrid" means both halves describe the same form: the AcroForm half is
    complete and fillable by any viewer, and the XFA half is what an
    XFA-aware viewer prefers. That is precisely why decision 020 §3.2.2
    refuses field CREATION here — pdfcer can write the AcroForm half and not
    the XFA half, so a one-sided add makes the two halves disagree about how
    many fields the document has, and which viewer you open it in decides
    what you see.

    Reading and filling stay allowed: the AcroForm half is a real form and
    refusing to read it would be refusing a capability pdfcer has.

    The XDP packets are a minimal well-formed skeleton. `detect_xfa` counts
    array PAIRS and parses no XML; a fuller packet would add bytes without
    adding coverage, and pdfcer must never start interpreting them.
    """
    objs: dict[int, bytes] = {}
    objs[1] = (
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] "
        b"/DA (/Helv 0 Tf 0 g) " + DR + b" /XFA [(preamble) 5 0 R (postamble) 6 0 R] >> >>"
    )
    objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << >> "
        b"/Annots [4 0 R] >>"
    )
    objs[4] = (
        b"<< /FT /Tx /T (Applicant) /V (J. Doe) /Type /Annot /Subtype /Widget "
        b"/Rect [20 150 240 172] /P 3 0 R /F 4 /MK << /BC [0 0 0] >> >>"
    )
    objs[5] = stream_obj(
        b"<<",
        b'<?xml version="1.0" encoding="UTF-8"?>\n'
        b'<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/">',
    )
    objs[6] = stream_obj(b"<<", b"</xdp:xdp>")
    return assemble(objs)



# ---------------------------------------------------------------------------
# (f) The decision-009 JavaScript carriers, for the byte-verbatim test.
# ---------------------------------------------------------------------------
def fixture_js_carriers() -> bytes:
    """An AcroForm carrying every JavaScript hook pdfcer promises not to touch.

    Pass 7.0 guarantees that the three JS carriers — `/AcroForm /CO`, a field
    `/AA`, and the document `/Names /JavaScript` tree — are re-emitted BYTE
    VERBATIM. Decision 009 forbids executing embedded PDF JavaScript
    permanently, so recognising and preserving them is the whole of pdfcer's
    contract with them.

    That guarantee held STRUCTURALLY, not by assertion: filling a field never
    writes the `/AcroForm` dictionary at all, so there was nothing that could
    disturb `/CO`. Field CREATION must write `/AcroForm /Fields`. The
    guarantee therefore stops being structural the moment authoring ships —
    and because it was never asserted, **no existing test goes red**. That is
    the exact shape of a promise that quietly stops holding, which is why
    decision 020 §7.2 made a byte-grep test mandatory in this slice.

    Every carrier is placed where authoring will actually pass close to it:

    * `/CO` sits in the `/AcroForm` dict that field registration rewrites;
    * `/AA` sits on a field, alongside the `/Fields` array that grows;
    * `/Names /JavaScript` sits in the CATALOG, which the `/AcroForm`-absent
      creation path writes.

    The JavaScript itself is inert and deliberately trivial. pdfcer must never
    parse it, so its content buys no coverage — its BYTES are the whole test.
    """
    objs: dict[int, bytes] = {}
    objs[1] = (
        b"<< /Type /Catalog /Pages 2 0 R /Names << /JavaScript 7 0 R >> "
        b"/AcroForm << /Fields [4 0 R 5 0 R] /CO [5 0 R 4 0 R] "
        b"/DA (/Helv 0 Tf 0 g) " + DR + b" >> >>"
    )
    objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << >> "
        b"/Annots [4 0 R 5 0 R] >>"
    )
    objs[4] = (
        b"<< /FT /Tx /T (Price) /V (10.00) /Type /Annot /Subtype /Widget "
        b"/Rect [20 150 140 172] /P 3 0 R /F 4 >>"
    )
    # `/AA` with a `/C` calculate hook and a `/F` format hook - the two an
    # ordinary calculated field carries.
    objs[5] = (
        b"<< /FT /Tx /T (Total) /V (20.00) /Type /Annot /Subtype /Widget "
        b"/Rect [20 110 140 132] /P 3 0 R /F 4 "
        b"/AA << /C << /S /JavaScript /JS 8 0 R >> "
        b"/F << /S /JavaScript /JS (AFNumber_Format\(2, 0, 0, 0, $, true\);) >> >> >>"
    )
    objs[6] = stream_obj(b"<<", b"BT ET")
    # The document-level name tree.
    objs[7] = b"<< /Names [(docInit) 9 0 R] >>"
    objs[8] = stream_obj(b"<<", b"event.value = 2 * this.getField('Price').value;")
    objs[9] = b"<< /S /JavaScript /JS (console.println\('init'\);) >>"
    return assemble(objs)


# ---------------------------------------------------------------------------
# (g) A TAGGED document with structure tab order, for the two disclosures.
# ---------------------------------------------------------------------------
def fixture_tagged_struct_tabs() -> bytes:
    """A tagged document whose page declares `/Tabs /S` (decision 020 §3.4.3).

    Two disclosures need this fixture, and neither is reachable without it:

    * **The document is tagged** (`/StructTreeRoot`) and a pdfcer-authored
      field is not in its structure tree. pdfcer has no structure-tree writer,
      and §3.5.3 deliberately ships the disclosure rather than a partial
      writer — a half-written tag tree claims a completeness the document
      does not have.

    * **The page uses structure tab order** (`/Tabs /S`, Table 30). §14.7
      derives tab order from the TAG TREE, so an untagged field on such a
      page has no tab position **at all** — not "last", *undefined*, with
      different viewers doing different things. That is a functional defect
      in the form, not only an accessibility gap.

    `/Tabs` sits on the PAGE here rather than on the page tree node, because
    that is the shape a real producer writes and it is the one the inheriting
    lookup must get right in its base case. The structure tree is a minimal
    well-formed skeleton: pdfcer reads no further than "is `/StructTreeRoot`
    present", so a fuller tree would add bytes without adding coverage.

    Without this fixture both disclosures would be unreachable code — a rule
    that is correct, wired, and never fires, which is exactly the shape R96
    exists to forbid.
    """
    objs: dict[int, bytes] = {}
    objs[1] = (
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
    )
    objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << >> "
        b"/Contents 4 0 R /Tabs /S >>"
    )
    objs[4] = stream_obj(b"<<", b"BT ET")
    objs[5] = b"<< /Type /StructTreeRoot /K [] >>"
    return assemble(objs)

def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    write("multi-widget-form.pdf", fixture_multi_widget())
    write("nested-form.pdf", fixture_nested())
    write("radio-group-form.pdf", fixture_radio_group())
    write("mixed-kids-form.pdf", fixture_mixed_kids())
    write("xfa-hybrid-form.pdf", fixture_xfa_hybrid())
    write("js-carriers-form.pdf", fixture_js_carriers())
    write("tagged-struct-tabs.pdf", fixture_tagged_struct_tabs())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
