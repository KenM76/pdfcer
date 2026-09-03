#!/usr/bin/env python3
"""Generate the embedded-SUBSET simple-font fixture (Pass 21.x / FF-C).

WHY THIS FIXTURE EXISTS
=======================
Two shipped refusals are reachable only with an embedded **subset** font:

  * **R-INV-1**, the embedded-subset floor — "this run's font is a subset
    and does not carry the character you just typed."
  * **`FormatError::CoverageFailure`** raised by `format.rs`'s subset branch,
    which refuses when a re-encoded code is not already carried on the page.

`fixtures/synthetic/` had three files and none of them could fire either.
That gap was not noticed until 2026-08-03, when two operator-facing hints for
these exact refusals turned out to have been telling the operator to do
something that could not work (`0893191`) — and the fix could not be observed
in the running app, because nothing in the corpus could reach the message.

A refusal no fixture can trigger is a refusal whose wording nobody has ever
read on screen. This closes that.

It is also a prerequisite for Pass 21.0 (decision 021): FF-C's whole purpose
is to turn these refusals into an actionable remedy, and "the refusal still
fires when it should" is half of that Pass's acceptance criteria.

WHY A *SIMPLE* FONT AND NOT A CIDFont
=====================================
`tools/gen-cidfont-nocmap-fixtures.py` already emits an embedded subset
`CIDFontType2`. It cannot reach these refusals, because a composite run is
refused *earlier* by **R-INV-4** (`/Type0` runs are not character-editable at
all) — so the subset branch is never consulted. The refusals being targeted
live on the **simple**-font path, so the fixture has to be a simple font.

That distinction is the entire reason this file exists as a sibling rather
than a flag on the existing generator.

WHAT MAKES IT A SUBSET, MECHANICALLY
====================================
`pdfcer-core`'s `is_subset_tag` (`text_edit/edit.rs`) decides subset-ness
purely from the `/BaseFont` name: exactly six ASCII **uppercase** letters
followed by `+`. That is ISO 32000-1 §9.6.4's subset prefix. The font is
*also* genuinely reduced — it carries outlines for only the characters shown
— so the fixture is not merely lying in its name. Both halves matter: the tag
is what the code branches on, the real reduction is what makes the fixture
honest if the detection rule ever changes.

**Verify-don't-assume (R22):** the builder asserts the sfnt magic, the
presence of `glyf`, and that the shipped `/BaseFont` actually satisfies the
same six-uppercase-plus-`+` predicate `is_subset_tag` applies. A fixture that
silently stops being a subset would make every test built on it pass for the
wrong reason.

LICENSING
=========
Fully SYNTHETIC (`docs/LEGAL.md` §5). Every outline is drawn here by
`fontTools`; no byte is copied from any real-world font or document. This
matters more than usual for a *font* fixture — embedding someone else's face
is exactly the redistribution question decision 021's R109 is about, and a
test corpus is not the place to have it.

Usage:
    python tools/gen-subset-font-fixtures.py [OUT_DIR]
        OUT_DIR defaults to fixtures/synthetic/text/.
"""

from __future__ import annotations

import io
import struct
import sys
from pathlib import Path

PAGE_WIDTH = 612
PAGE_HEIGHT = 792

OUT = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "text"

# The subset prefix. Six ASCII uppercase letters + '+', per ISO 32000-1
# §9.6.4 and exactly what `is_subset_tag` tests for.
SUBSET_TAG = "SUBSET"
FAMILY = "pdfceSubsetDemo"
BASE_FONT = f"{SUBSET_TAG}+{FAMILY}"

# The characters the subset CARRIES. Deliberately a small, contiguous,
# obviously-incomplete set: an operator looking at the page can see at a
# glance which letters exist, which makes a refusal about any other letter
# self-explanatory rather than mysterious.
CARRIED = "ABC"
# Where the first absent character sits, for whoever writes the test:
# anything outside CARRIED. 'Z' is used in the doc comments because it is
# unambiguous and adjacent in nobody's mental model to A/B/C.

UPEM = 1000
ADVANCE = 600


def build_subset_truetype() -> bytes:
    """A minimal TrueType carrying outlines for `CARRIED` and nothing else.

    Each carried character gets a distinguishable outline — a bar whose
    height varies by index — so a rendering test can tell the glyphs apart
    without OCR, and so a wrong-glyph bug looks different from a
    missing-glyph bug.
    """
    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.ttGlyphPen import TTGlyphPen

    glyph_names = [".notdef"] + [f"g{ch}" for ch in CARRIED]

    fb = FontBuilder(UPEM, isTTF=True)
    fb.setupGlyphOrder(glyph_names)

    glyphs = {}
    notdef_pen = TTGlyphPen(None)
    glyphs[".notdef"] = notdef_pen.glyph()  # empty outline

    for i, ch in enumerate(CARRIED):
        pen = TTGlyphPen(None)
        # Height rises with index: A short, B taller, C tallest. Distinct
        # on sight, and trivially assertable from a rasterized bitmap.
        top = 300 + 200 * i
        pen.moveTo((80, 0))
        pen.lineTo((ADVANCE - 80, 0))
        pen.lineTo((ADVANCE - 80, top))
        pen.lineTo((80, top))
        pen.closePath()
        glyphs[f"g{ch}"] = pen.glyph()

    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics(
        {name: (ADVANCE, 80) for name in glyph_names}
    )
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": FAMILY, "styleName": "Regular"})
    # A cmap covering ONLY the carried characters. This is what makes the
    # font a real subset rather than one that merely claims to be: a
    # consumer asking for 'Z' finds nothing, which is the truth the
    # /BaseFont tag is asserting.
    fb.setupCharacterMap({ord(ch): f"g{ch}" for ch in CARRIED})
    fb.setupOS2(sTypoAscender=800, sTypoDescender=-200)
    fb.setupPost()

    buf = io.BytesIO()
    fb.font.save(buf)
    data = buf.getvalue()

    # Verify-don't-assume (R22). A fixture that quietly stopped carrying a
    # glyf table, or quietly grew coverage for the absent characters, would
    # make every test built on it pass for the wrong reason.
    assert data[:4] == b"\x00\x01\x00\x00", f"sfnt magic was {data[:4]!r}"
    directory = data[: 12 + 16 * int.from_bytes(data[4:6], "big")]
    assert b"glyf" in directory, "no glyf table in the built font"
    cmap = fb.font["cmap"].getBestCmap()
    assert set(cmap) == {ord(c) for c in CARRIED}, (
        f"cmap coverage drifted: {sorted(cmap)} != {sorted(ord(c) for c in CARRIED)}"
    )
    return data


def build_cycle_truetype() -> bytes:
    """A TrueType whose composite glyphs form CYCLES.

    Two shapes, both of which a naive recursive `glyf` walk would follow
    forever:

      * `gSelf` is a composite whose only component is `gSelf` — a
        one-glyph loop.
      * `gPing` and `gPong` are composites referencing each other — a
        two-glyph loop, which a depth counter reset per glyph would miss.

    WHY THIS FIXTURE EXISTS INSTEAD OF A GUARD
    ------------------------------------------
    Composite-glyph recursion is the obvious unbounded-recursion risk in any
    font subsetter, and `ARCHITECTURE.md` §10 would normally demand a depth
    cap. Decision 021 §3.5 deliberately does NOT add one: `subsetter`'s
    `closure()` is an iterative worklist that enqueues a component only when
    the remapper has not already seen it, so the visited set grows
    monotonically and is bounded by `numGlyphs`. It terminates structurally,
    upstream. A pdfcer-side cap would sit behind a filter its guarded case
    cannot pass — a guard that reads as protection and executes never (R96).

    So the property is asserted rather than defended. This fixture is what
    makes that assertion possible, and — the part a redundant guard could
    never do — it will FAIL if upstream ever rewrites that walk recursively.

    Fully synthetic (`docs/LEGAL.md` §5); no byte comes from a real font.
    """
    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.ttGlyphPen import TTGlyphPen
    from fontTools.ttLib.tables import _g_l_y_f as glyf_mod

    glyph_names = [".notdef", "gBase", "gSelf", "gPing", "gPong"]

    fb = FontBuilder(UPEM, isTTF=True)
    fb.setupGlyphOrder(glyph_names)

    # One real outline so the font is not degenerate — a subsetter that
    # bailed out early on a contentless font would pass the cycle test
    # without ever walking anything.
    pen = TTGlyphPen(None)
    pen.moveTo((50, 0))
    pen.lineTo((550, 0))
    pen.lineTo((550, 500))
    pen.lineTo((50, 500))
    pen.closePath()
    base = pen.glyph()

    def composite(component_names):
        g = glyf_mod.Glyph()
        g.numberOfContours = -1  # composite
        g.components = []
        for name in component_names:
            c = glyf_mod.GlyphComponent()
            c.glyphName = name
            c.x, c.y = 0, 0
            c.flags = 0
            g.components.append(c)
        return g

    # Built ACYCLIC first — every composite points at `gBase`.
    #
    # fontTools cannot WRITE a cyclic composite: `Glyph.compile` recalculates
    # bounds by walking components, and that walk is recursive, so building
    # the cycle directly dies with a Python RecursionError before a byte is
    # emitted. Which is itself the point being made — a recursive composite
    # walk is the natural implementation, and it is exactly the one that
    # cannot survive this input.
    #
    # So the cycles are introduced afterwards, by rewriting the two-byte
    # component glyph indices in the compiled `glyf` table (see
    # `_make_components_cyclic`). The resulting file is well-formed sfnt that
    # no normal font tool would produce and every consumer must survive.
    glyphs = {
        ".notdef": TTGlyphPen(None).glyph(),
        "gBase": base,
        "gSelf": composite(["gBase"]),
        "gPing": composite(["gBase"]),
        "gPong": composite(["gBase"]),
    }

    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics({name: (ADVANCE, 50) for name in glyph_names})
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": "pdfceCycleDemo", "styleName": "Regular"})
    fb.setupCharacterMap(
        {ord("A"): "gBase", ord("S"): "gSelf", ord("P"): "gPing", ord("Q"): "gPong"}
    )
    fb.setupOS2(sTypoAscender=800, sTypoDescender=-200)
    fb.setupPost()

    buf = io.BytesIO()
    fb.font.save(buf)
    data = _make_components_cyclic(buf.getvalue(), glyph_names)

    # Verify-don't-assume (R22): read the FINISHED BYTES back and confirm the
    # cycles are really in them. If the patch ever silently misses — a table
    # layout change, a different component encoding — this fixture stops
    # testing the thing it is named for, and every test built on it passes
    # for the wrong reason.
    idx = {n: i for i, n in enumerate(glyph_names)}
    comps = _component_indices(data, glyph_names)
    assert comps["gSelf"] == [idx["gSelf"]], f"gSelf must reference itself: {comps}"
    assert comps["gPing"] == [idx["gPong"]], f"gPing must reference gPong: {comps}"
    assert comps["gPong"] == [idx["gPing"]], f"gPong must reference gPing: {comps}"
    assert data[:4] == b"\x00\x01\x00\x00", "sfnt magic"
    return data


def build_fstype_truetype(fs_type: int, os2_version: int = 4) -> bytes:
    """A donor whose `OS/2` `fsType` carries `fs_type` (R109 fixtures).

    Each embedding-permission refusal pdfcer can raise needs a font that
    actually triggers it. Without these the refusals are unreachable, and an
    unreachable refusal is one nobody has ever seen fire — the exact
    situation that let two operator-facing hints ship wrong for several
    releases (`0893191`).

    `os2_version` is a parameter because bits 8 and 9 (`No subsetting`,
    `Bitmap embedding only`) **must be ignored** on `OS/2` versions 0 and 1,
    where they had no assigned meaning. A fixture pair at v1 and v4 with the
    same bits set is what proves pdfcer honours that gating rather than
    reading the bytes unconditionally.

    Fully synthetic (`docs/LEGAL.md` §5).
    """
    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.ttGlyphPen import TTGlyphPen

    fb = FontBuilder(UPEM, isTTF=True)
    fb.setupGlyphOrder([".notdef", "gA"])

    pen = TTGlyphPen(None)
    pen.moveTo((80, 0))
    pen.lineTo((520, 0))
    pen.lineTo((520, 600))
    pen.lineTo((80, 600))
    pen.closePath()

    fb.setupGlyf({".notdef": TTGlyphPen(None).glyph(), "gA": pen.glyph()})
    fb.setupHorizontalMetrics({".notdef": (ADVANCE, 80), "gA": (ADVANCE, 80)})
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": "pdfceFsTypeDemo", "styleName": "Regular"})
    fb.setupCharacterMap({ord("A"): "gA"})
    fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, version=os2_version)
    fb.font["OS/2"].fsType = fs_type
    fb.setupPost()

    buf = io.BytesIO()
    fb.font.save(buf)
    data = buf.getvalue()

    # Verify-don't-assume (R22): read the finished bytes back. fontTools is
    # entitled to normalise an OS/2 field, and a fixture whose fsType quietly
    # reverted to 0 would make every permission test pass by permitting.
    reread = _reread(data)
    got = reread["OS/2"].fsType
    assert got == fs_type, f"fsType did not survive the write: wanted {fs_type:#06x}, got {got:#06x}"
    assert reread["OS/2"].version == os2_version, "OS/2 version drifted"
    return data


def _sfnt_tables(data: bytes) -> dict[str, tuple[int, int]]:
    """`tag -> (offset, length)` from the sfnt table directory."""
    num = int.from_bytes(data[4:6], "big")
    out = {}
    for i in range(num):
        rec = 12 + 16 * i
        tag = data[rec : rec + 4].decode("latin-1")
        off = int.from_bytes(data[rec + 8 : rec + 12], "big")
        ln = int.from_bytes(data[rec + 12 : rec + 16], "big")
        out[tag] = (off, ln)
    return out


def _glyph_offsets(data: bytes, count: int) -> list[int]:
    """Absolute `glyf` offsets for glyphs 0..count, from `loca` + `head`."""
    tabs = _sfnt_tables(data)
    head_off = tabs["head"][0]
    # indexToLocFormat is the last field of head: 0 = short (offset/2).
    fmt = int.from_bytes(data[head_off + 50 : head_off + 52], "big", signed=True)
    loca_off = tabs["loca"][0]
    glyf_off = tabs["glyf"][0]
    out = []
    for i in range(count + 1):
        if fmt == 0:
            v = int.from_bytes(data[loca_off + 2 * i : loca_off + 2 * i + 2], "big") * 2
        else:
            v = int.from_bytes(data[loca_off + 4 * i : loca_off + 4 * i + 4], "big")
        out.append(glyf_off + v)
    return out


def _component_indices(data: bytes, names: list[str]) -> dict[str, list[int]]:
    """Glyph indices each composite glyph references, read from the bytes."""
    offs = _glyph_offsets(data, len(names))
    found: dict[str, list[int]] = {}
    for i, name in enumerate(names):
        start, end = offs[i], offs[i + 1]
        if end - start < 10:
            continue  # empty glyph
        ncont = int.from_bytes(data[start : start + 2], "big", signed=True)
        if ncont >= 0:
            continue  # simple glyph
        refs, p = [], start + 10
        while True:
            flags = int.from_bytes(data[p : p + 2], "big")
            refs.append(int.from_bytes(data[p + 2 : p + 4], "big"))
            p += 4
            p += 4 if (flags & 0x0001) else 2          # ARG_1_AND_2_ARE_WORDS
            if flags & 0x0008:
                p += 2                                  # WE_HAVE_A_SCALE
            elif flags & 0x0040:
                p += 4                                  # X_AND_Y_SCALE
            elif flags & 0x0080:
                p += 8                                  # TWO_BY_TWO
            if not (flags & 0x0020):                    # MORE_COMPONENTS
                break
        found[name] = refs
    return found


def _make_components_cyclic(data: bytes, names: list[str]) -> bytes:
    """Rewrite composite component indices in place to form the cycles.

    Patches the two-byte glyph index inside each composite's first component
    record: `gSelf -> gSelf`, `gPing -> gPong`, `gPong -> gPing`.
    """
    idx = {n: i for i, n in enumerate(names)}
    offs = _glyph_offsets(data, len(names))
    out = bytearray(data)
    targets = {"gSelf": idx["gSelf"], "gPing": idx["gPong"], "gPong": idx["gPing"]}
    for name, new_index in targets.items():
        i = idx[name]
        start = offs[i]
        ncont = int.from_bytes(data[start : start + 2], "big", signed=True)
        assert ncont < 0, f"{name} is not a composite glyph"
        # Header is 10 bytes; the component record starts with flags then the
        # glyph index.
        pos = start + 10 + 2
        out[pos : pos + 2] = struct.pack(">H", new_index)
    return bytes(out)


def _reread(data: bytes):
    """Parse built bytes back, so assertions test the FILE, not the builder."""
    from fontTools.ttLib import TTFont

    return TTFont(io.BytesIO(data))


def serialize(objects: dict[int, bytes]) -> bytes:
    """Classic xref layout, exactly-20-byte entries (§7.5.4). Identical
    discipline to the sibling generators."""
    out = bytearray(b"%PDF-1.7\n")
    out += b"%\xe2\xe3\xcf\xd3\n"
    highest = max(objects)
    offsets: dict[int, int] = {}
    for num in range(1, highest + 1):
        body = objects.get(num)
        if body is None:
            continue
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode("ascii")
        out += body
        out += b"\nendobj\n"
    xref_at = len(out)
    out += f"xref\n0 {highest + 1}\n".encode("ascii")
    out += b"0000000000 65535 f \n"
    for num in range(1, highest + 1):
        if num in offsets:
            out += f"{offsets[num]:010d} 00000 n \n".encode("ascii")
        else:
            out += b"0000000000 65535 f \n"
    out += (
        f"trailer\n<< /Size {highest + 1} /Root 1 0 R >>\n"
        f"startxref\n{xref_at}\n%%EOF\n"
    ).encode("ascii")
    return bytes(out)


def raw_stream(body: bytes, extra: str) -> bytes:
    """Uncompressed stream with a correct `/Length` (plus caller-supplied
    keys such as `/Length1`). Uncompressed so a failure can never be blamed
    on the filter."""
    return (
        f"<< /Length {len(body)}{extra} >>\nstream\n".encode("ascii")
        + body
        + b"\nendstream"
    )


def subset_simple_embedded() -> bytes:
    """`/TrueType` simple font, embedded SUBSET program, carrying only `CARRIED`.

    `/FirstChar`..`/LastChar` and `/Widths` span exactly the carried
    characters, so the width array agrees with the program's real coverage.
    Disagreement there is its own class of real-world bug and is NOT what
    this fixture is for — a fixture that tests two things at once tells you
    nothing when it fails.
    """
    ttf = build_subset_truetype()

    first = ord(CARRIED[0])
    last = ord(CARRIED[-1])
    widths = " ".join(str(ADVANCE) for _ in CARRIED)

    content = (
        b"BT\n"
        b"/F0 48 Tf\n"
        b"72 600 Td\n"
        b"(" + CARRIED.encode("ascii") + b") Tj\n"
        b"ET\n"
    )

    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            f"/MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] "
            f"/Resources << /Font << /F0 5 0 R >> >> >>"
        ).encode("ascii"),
        3: b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>",
        4: raw_stream(content, ""),
        5: (
            f"<< /Type /Font /Subtype /TrueType /BaseFont /{BASE_FONT} "
            f"/FirstChar {first} /LastChar {last} /Widths [{widths}] "
            f"/Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>"
        ).encode("ascii"),
        6: (
            f"<< /Type /FontDescriptor /FontName /{BASE_FONT} "
            f"/Flags 32 /FontBBox [0 -200 {ADVANCE} 800] /ItalicAngle 0 "
            f"/Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 "
            f"/FontFile2 7 0 R >>"
        ).encode("ascii"),
        7: raw_stream(ttf, f" /Length1 {len(ttf)}"),
    }
    return serialize(objects)


def composite_editable() -> bytes:
    """A `/Type0` + `Identity-H` page whose run is genuinely EDITABLE.

    Pass 21.1 needs a composite fixture with MORE THAN ONE glyph. The
    sibling `cidfonttype2-with-tounicode.pdf` carries a single CID, so
    there is no second character to edit *to* — it can prove a refusal and
    nothing else.

    This one embeds the same three-glyph donor the simple fixtures use, so
    CID 1/2/3 are A/B/C (glyph order `.notdef, gA, gB, gC`, and
    `/CIDToGIDMap /Identity` makes CID == GID). The `/ToUnicode` is
    one-to-one, so it passes R110's injectivity check and an edit like
    ABC -> CBA exercises the real multi-byte encode and splice.

    Distinguishable glyph heights (A short, B taller, C tallest) mean a
    reordering edit is visible in a raster without OCR — a wrong-CID bug
    looks different from a missing-glyph bug.
    """
    ttf = build_subset_truetype()

    tounicode = (
        b"begincmap\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n"
        b"3 beginbfchar\n<0001> <0041>\n<0002> <0042>\n<0003> <0043>\nendbfchar\n"
        b"endcmap\n"
    )

    content = b"BT\n/F0 48 Tf\n72 600 Td\n<000100020003> Tj\nET\n"

    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            f"/MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] "
            f"/Resources << /Font << /F0 5 0 R >> >> >>"
        ).encode("ascii"),
        3: b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>",
        4: raw_stream(content, ""),
        5: (
            b"<< /Type /Font /Subtype /Type0 /BaseFont /CMPOSE+pdfceSubsetDemo "
            b"/Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 10 0 R >>"
        ),
        6: (
            b"<< /Type /Font /Subtype /CIDFontType2 "
            b"/BaseFont /CMPOSE+pdfceSubsetDemo "
            b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
            b"/FontDescriptor 7 0 R /CIDToGIDMap /Identity /DW 1000 "
            b"/W [1 [600 600 600]] >>"
        ),
        7: (
            b"<< /Type /FontDescriptor /FontName /CMPOSE+pdfceSubsetDemo "
            b"/Flags 4 /FontBBox [0 -200 600 800] /ItalicAngle 0 "
            b"/Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 "
            b"/FontFile2 9 0 R >>"
        ),
        9: raw_stream(ttf, f" /Length1 {len(ttf)}"),
        10: raw_stream(tounicode, ""),
    }
    return serialize(objects)


def main() -> int:
    out_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else OUT
    out_dir.mkdir(parents=True, exist_ok=True)

    # The shipped /BaseFont must satisfy the SAME predicate pdfcer-core
    # applies (`is_subset_tag`), or the fixture is not testing the branch
    # it names. Reproduced here rather than assumed, because the fixture
    # and the code that classifies it live in different languages and
    # nothing else would notice them drifting apart.
    tag, _, rest = BASE_FONT.partition("+")
    assert len(tag) == 6 and tag.isascii() and tag.isupper() and rest, (
        f"/BaseFont {BASE_FONT!r} does not satisfy is_subset_tag's rule "
        "(exactly six ASCII uppercase letters, then '+', then a name)"
    )

    path = out_dir / "subset-simple-embedded.pdf"
    path.write_bytes(subset_simple_embedded())
    print(f"wrote {path} ({path.stat().st_size} bytes)")

    # The SAME program, standalone, as a DONOR fixture for FF-C (Pass 21.0).
    #
    # `plan_subset` takes a font file from the operator's font folder, not a
    # program lifted out of a PDF — ISO 32000-1 §9.9 forbids the latter
    # ("a licensed copy of the font program, not a copy extracted from the
    # PDF file"). So the round-trip test needs a real .ttf on disk, and
    # without one the only tests possible are error-path ones: every refusal
    # would be covered and the path that actually does the work would not.
    #
    # Emitting it here rather than in a second generator keeps the two
    # artefacts provably identical — the donor IS the embedded program — so a
    # test can subset the donor and compare against what the PDF carries.
    donor = out_dir / "subset-donor.ttf"
    donor.write_bytes(build_subset_truetype())
    print(f"wrote {donor} ({donor.stat().st_size} bytes)")

    ce = out_dir / "composite-editable.pdf"
    ce.write_bytes(composite_editable())
    print(f"wrote {ce.name} ({ce.stat().st_size} bytes)  [Type0, 3 CIDs, injective ToUnicode]")

    cyc = out_dir / "subset-cycle-donor.ttf"
    cyc.write_bytes(build_cycle_truetype())
    print(f"wrote {cyc} ({cyc.stat().st_size} bytes)  [composite-glyph cycles]")

    # One donor per embedding-permission outcome (R109). Named for the bits
    # rather than the verdict, so renaming a refusal does not orphan a file.
    for label, bits, ver in [
        ("installable", 0x0000, 4),   # value 0 — most permissive
        ("restricted", 0x0002, 4),    # value 2 — may not be embedded
        ("preview-print", 0x0004, 4), # value 4 — embeddable, doc read-only
        ("editable", 0x0008, 4),      # value 8 — embeddable and editable
        ("nosubset", 0x0108, 4),      # bit 8 — editable BUT no subsetting
        ("bitmaponly", 0x0208, 4),    # bit 9 — outlines forbidden
        ("nosubset-v1", 0x0108, 1),   # same bits, v1 -> bits 8/9 IGNORED
    ]:
        f = out_dir / f"subset-fstype-{label}.ttf"
        f.write_bytes(build_fstype_truetype(bits, ver))
        print(f"wrote {f.name} (fsType={bits:#06x}, OS/2 v{ver})")
    print(f"  /BaseFont  {BASE_FONT}   (subset tag: {tag})")
    print(f"  carries    {CARRIED!r}")
    print(f"  absent     any other character — 'Z' is the canonical probe")
    return 0


if __name__ == "__main__":
    sys.exit(main())
