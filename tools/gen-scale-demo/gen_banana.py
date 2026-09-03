#!/usr/bin/env python3
"""Generate a letter-size PDF: a banana at life size, and two of its cells
at the SAME scale, as a deep-zoom exercise for pdfcer.

WHY THIS FILE EXISTS
====================
Everything on the page is drawn in one coordinate system at true scale.
Nothing is enlarged "for clarity". That is the whole point: the cells are
genuinely 300 micrometres and 60 micrometres across, so at a zoom where
the page fits a screen they are a fraction of one pixel, and the only way
to read their labels is to magnify by about four orders of magnitude.

UNITS
=====
PDF user space is 1/72 inch per unit.

    1 pt = 25.4/72 mm = 352.7778 um
    1 um = 0.00283465 pt

So a 300 um cell is 0.85 pt across, and a 30 um label is 0.085 pt tall.

THE BIOLOGY, and its confidence
===============================
Representative values, not measurements of a particular fruit:

  * banana fruit PULP is parenchyma, and its cells are unusually large for
    plant tissue -- a few hundred micrometres is typical. 300 um is used
    here.
  * banana PEEL epidermis is a thin brick-like layer; a few tens of
    micrometres. 60 x 35 um is used here.

Both figures are printed on the page next to the thing they describe, so
the drawing states its own assumptions rather than relying on this file.

WHAT IS PLACED WHERE, and the three rules the request imposed
=============================================================
1. the label above the cells is at a size readable when the whole page is
   on screen (11 pt), while the labels UNDER the cells are 1/10 the height
   of the largest cell -- 300/10 = 30 um = 0.085 pt;
2. the arrow ends TWO CELL LENGTHS above the cells -- 2 x 300 um = 600 um
   = 1.7 pt above the top of the pulp cell;
3. the arrow therefore has to be a TAPERED DART rather than a
   shaft-and-head. A conventional arrowhead sized to be visible at page
   scale would be about 8 pt across, which is nine times the width of the
   thing it points at -- it would cover the cells completely at the zoom
   where they matter. A dart that narrows to a mathematical point reads as
   an arrow from a distance and vanishes to nothing at its tip.
"""

import zlib

import cells_detail
import mitochondrion
import molecules

# Interned once, at import, because `build_content()` needs the box's
# XObject name while it writes the page and `build_pdf()` needs the same
# registry afterwards to lay the objects out.
MOL_BOX_NAME = molecules.register()

PT_PER_UM = 25.4 / 72.0 / 1000.0  # = 1/352.7778
UM = 1.0 / (25.4 / 72.0 * 1000.0)  # points per micrometre


def um(v):
    """Micrometres to points."""
    return v * UM


def cm(v):
    """Centimetres to points."""
    return v * 72.0 / 2.54


# ---------------------------------------------------------------------------
# Geometry
# ---------------------------------------------------------------------------

PAGE_W, PAGE_H = 612.0, 792.0

# --- the banana, as two cubic edges meeting at the tips -------------------
# Outer (upper) edge, left tip -> right tip; inner (lower) edge comes back.
TIP_L = (85.0, 540.0)
TIP_R = (462.0, 512.0)
OUTER_C1, OUTER_C2 = (148.0, 700.0), (414.0, 706.0)
INNER_C1, INNER_C2 = (402.0, 566.0), (142.0, 578.0)


def bezier_pt(p0, p1, p2, p3, t):
    mt = 1.0 - t
    x = mt**3 * p0[0] + 3 * mt * mt * t * p1[0] + 3 * mt * t * t * p2[0] + t**3 * p3[0]
    y = mt**3 * p0[1] + 3 * mt * mt * t * p1[1] + 3 * mt * t * t * p2[1] + t**3 * p3[1]
    return (x, y)


def centreline_length():
    """Arc length of the banana's centreline, so the label can state the
    drawn size instead of asserting one."""
    mid = lambda a, b: ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0)
    c1 = mid(OUTER_C1, INNER_C2)
    c2 = mid(OUTER_C2, INNER_C1)
    total, prev = 0.0, TIP_L
    for i in range(1, 2001):
        p = bezier_pt(TIP_L, c1, c2, TIP_R, i / 2000.0)
        total += ((p[0] - prev[0]) ** 2 + (p[1] - prev[1]) ** 2) ** 0.5
        prev = p
    return total


# --- the cells ------------------------------------------------------------
PULP_W = PULP_H = 300.0        # micrometres
SKIN_W, SKIN_H = 60.0, 35.0    # micrometres
GAP = 250.0                    # micrometres between the two cells
LABEL_UM = PULP_H / 10.0       # 1/10 the height of the largest cell

PULP_CX, PULP_CY = 540.0, 560.0
pulp_l = PULP_CX - um(PULP_W) / 2
pulp_r = PULP_CX + um(PULP_W) / 2
pulp_b = PULP_CY - um(PULP_H) / 2
pulp_t = PULP_CY + um(PULP_H) / 2

skin_l = pulp_r + um(GAP)
skin_r = skin_l + um(SKIN_W)
skin_b = PULP_CY - um(SKIN_H) / 2
skin_t = PULP_CY + um(SKIN_H) / 2

GROUP_CX = (pulp_l + skin_r) / 2.0
ARROW_TIP = (GROUP_CX, pulp_t + um(2 * PULP_H))   # two cell lengths above


def esc(s):
    return s.replace("\\", r"\\").replace("(", r"\(").replace(")", r"\)")


def build_content():
    o = []
    A = o.append
    arc_cm = centreline_length() / 72.0 * 2.54

    # ---- title ----------------------------------------------------------
    A("0.12 0.12 0.14 rg")
    A("BT /F2 17 Tf 56 726 Td (Banana, life size) Tj ET")
    A("0.30 0.30 0.34 rg")
    A(
        "BT /F1 10.5 Tf 56 707 Td "
        f"({esc('Everything on this page is drawn at true scale. Nothing is enlarged.')}) Tj ET"
    )

    # ---- banana ---------------------------------------------------------
    A("0.96 0.82 0.24 rg 0.60 0.46 0.09 RG 1.2 w")
    A(f"{TIP_L[0]} {TIP_L[1]} m")
    A(f"{OUTER_C1[0]} {OUTER_C1[1]} {OUTER_C2[0]} {OUTER_C2[1]} {TIP_R[0]} {TIP_R[1]} c")
    A(f"{INNER_C1[0]} {INNER_C1[1]} {INNER_C2[0]} {INNER_C2[1]} {TIP_L[0]} {TIP_L[1]} c")
    A("h B")

    # a longitudinal ridge, so it reads as a banana rather than a crescent
    A("0.86 0.70 0.16 RG 0.9 w")
    A("118 556 m 190 646 380 650 452 524 c S")

    # stem at the left tip, blossom scar at the right
    A("0.42 0.31 0.11 rg")
    A("85 540 m 74 556 l 62 552 l 70 534 l h f")
    A("0.30 0.22 0.10 rg")
    A(f"{TIP_R[0]} {TIP_R[1]} m {TIP_R[0]+7} {TIP_R[1]-6} l {TIP_R[0]+2} {TIP_R[1]-9} l h f")

    # ---- 1 cm scale bar, to substantiate "life size" --------------------
    bar_x, bar_y = 85.0, 470.0
    A("0.20 0.20 0.24 rg")
    A(f"{bar_x} {bar_y} {cm(1)} 3 re f")
    A(f"{bar_x} {bar_y-4} 1 11 re f")
    A(f"{bar_x+cm(1)-1} {bar_y-4} 1 11 re f")
    A("0.30 0.30 0.34 rg")
    A(f"BT /F1 9 Tf {bar_x} {bar_y-16} Td (1 cm) Tj ET")
    A(
        f"BT /F1 9 Tf {bar_x + cm(1) + 12} {bar_y-16} Td "
        f"({esc('banana drawn %.1f cm along its curve' % arc_cm)}) Tj ET"
    )

    # ---- the label above the cells (readable at page-fit zoom) ----------
    A("0.12 0.12 0.14 rg")
    A("BT /F2 11 Tf 404 690 Td (Two banana cells,) Tj ET")
    A("BT /F2 11 Tf 404 677 Td (at the same scale) Tj ET")
    A("0.35 0.35 0.40 rg")
    A(
        "BT /F1 8.5 Tf 404 662 Td "
        f"({esc('they are really down there. Zoom to about')}) Tj ET"
    )
    A("BT /F1 8.5 Tf 404 652 Td (12 000 % to read their labels) Tj ET")

    # ---- the dart: tail near the label, point 600 um above the cells ----
    tx, ty = ARROW_TIP
    A("0.20 0.35 0.62 rg")
    A("468 646 m")
    A(f"499 620 521 592 {tx:.6f} {ty:.6f} c")
    A(f"{tx + um(120):.6f} {ty + um(900):.6f} 515 597 472 651 c")
    A("h f")
    # A real arrowhead, 250 um long, at the dart's point. At page scale it
    # is 1/1400 of an inch and simply is not there; at the zoom where the
    # cells are legible it is the thing that says "these ones".
    ah = um(250)
    A(f"{tx:.6f} {ty:.6f} m")
    A(f"{tx - ah*0.45:.6f} {ty + ah:.6f} l")
    A(f"{tx + ah*0.45:.6f} {ty + ah:.6f} l")
    A("h f")

    # ---- the cells, drawn in micrometre space by `cells_detail` -------
    A(cells_detail.draw_pulp(pulp_l, pulp_b))
    A(cells_detail.draw_skin(skin_l, skin_b, SKIN_W, SKIN_H))

    # ---- the labels UNDER the cells, at 1/10 the largest cell's height --
    fs = um(LABEL_UM)
    A("0.15 0.15 0.18 rg")
    lab_y1 = pulp_b - um(70)
    lab_y2 = lab_y1 - um(45)
    A(f"BT /F1 {fs:.6f} Tf {pulp_l:.6f} {lab_y1:.6f} Td (inner cell \\(pulp parenchyma\\)) Tj ET")
    A(f"BT /F1 {fs:.6f} Tf {pulp_l:.6f} {lab_y2:.6f} Td (300 \\265m across) Tj ET")
    A(f"BT /F1 {fs:.6f} Tf {skin_l:.6f} {lab_y1:.6f} Td (skin cell \\(epidermis\\)) Tj ET")
    A(f"BT /F1 {fs:.6f} Tf {skin_l:.6f} {lab_y2:.6f} Td (60 \\265m across) Tj ET")

    # ---- the molecule box, BELOW the cells, also at 1:1 ------------------
    #
    # 46 nm wide. That is 0.00013 pt -- one seven-thousandth of a point,
    # and about a two-millionth of the pulp cell sitting above it. It
    # cannot be seen at any zoom where the cells are visible, so it gets
    # the same treatment the cells themselves get from the dart: a caption
    # at a size that IS readable, and a tapered pointer that narrows to
    # nothing as it approaches the thing it points at.
    #
    # The caption is 6 um -- between the 30 um cell labels and the 8 um
    # organelle labels -- so the ladder down the page reads in order:
    # 11 pt title, 30 um cell labels, 8 um organelle labels, 6 um this
    # caption, then 1.45 nm inside the box. Five type sizes spanning
    # 7 500 : 1, all set on the same page in the same units.
    mol_cx = PULP_CX
    mol_cy = pulp_b - um(255)
    cap = um(6.0)
    A("0.16 0.16 0.19 rg")
    A(
        f"BT /F2 {cap:.7f} Tf {mol_cx - um(150):.6f} {pulp_b - um(190):.6f} Td "
        f"(and below THIS, the ten molecules those cells are made of) Tj ET"
    )
    A("0.40 0.40 0.45 rg")
    A(
        f"BT /F1 {cap*0.8:.7f} Tf {mol_cx - um(150):.6f} {pulp_b - um(202):.6f} Td "
        f"(also 1:1. a water molecule is 0.37 nm, so zoom to about 1 300 000 000 %) Tj ET"
    )
    # the pointer: a dart again, for the reason the first one exists --
    # an arrowhead big enough to see here would be 1000x the box.
    A("0.20 0.35 0.62 rg")
    A(f"{mol_cx - um(30):.6f} {pulp_b - um(212):.6f} m")
    A(
        f"{mol_cx - um(12):.6f} {pulp_b - um(228):.6f} "
        f"{mol_cx - um(2):.6f} {pulp_b - um(240):.6f} "
        f"{mol_cx:.6f} {mol_cy + um(0.4):.6f} c"
    )
    A(
        f"{mol_cx + um(3):.6f} {pulp_b - um(238):.6f} "
        f"{mol_cx + um(16):.6f} {pulp_b - um(226):.6f} "
        f"{mol_cx + um(30):.6f} {pulp_b - um(212):.6f} l"
    )
    A("h f")
    A("q")
    A(molecules.instance_matrix(mol_cx, mol_cy))
    A(f"/{MOL_BOX_NAME} Do")
    A("Q")

    # a 100 um scale bar beside the cells, at true scale
    A("0.15 0.15 0.18 rg")
    sb_x, sb_y = pulp_l, pulp_t + um(60)
    A(f"{sb_x:.6f} {sb_y:.6f} {um(100):.6f} {um(8):.6f} re f")
    A(f"BT /F1 {fs:.6f} Tf {sb_x:.6f} {sb_y + um(20):.6f} Td (100 \\265m) Tj ET")

    # ---- the scale chain, in the empty lower half -----------------------
    A("0.12 0.12 0.14 rg")
    A("BT /F2 12 Tf 56 392 Td (What you are looking at) Tj ET")
    A("0.30 0.30 0.34 rg")
    lines = [
        "The banana above is drawn at life size: 1 cm on the page is 1 cm of fruit.",
        "The two cells beside it are drawn at that SAME scale, so they are as small",
        "on the page as they are in the fruit.",
        "",
        "1 point = 352.8 micrometres, so the inner cell is 0.85 pt across and the",
        "skin cell is 0.17 pt. Their labels are 30 micrometres tall: 0.085 pt.",
        "",
        "At a zoom where this whole page fits a screen, the pair of cells covers",
        "roughly one pixel. Reading the labels under them takes about 12 000 %.",
        "The arrow stops 600 micrometres short of them, which is two cell widths,",
        "and is why its point disappears before it reaches anything.",
    ]
    y = 372.0
    for ln in lines:
        if ln:
            A(f"BT /F1 10 Tf 56 {y:.1f} Td ({esc(ln)}) Tj ET")
        y -= 14.5
    A("0.55 0.55 0.60 rg")
    A("BT /F1 8 Tf 56 96 Td (Drawn as vector geometry -- no raster images. Cell sizes are) Tj ET")
    A("BT /F1 8 Tf 56 86 Td (representative, not measurements of a particular fruit.) Tj ET")

    return "\n".join(o).encode("latin-1")


def build_pdf():
    """Assemble the file.

    Object numbering is fixed for the first six objects and then runs on
    into one object per mitochondrion Form XObject. The forms are
    registered as a SIDE EFFECT of `build_content()` -- every
    `mitochondrion.place()` call interns the shape it needs -- so the
    content stream must be built before the object list is laid out. That
    ordering is the only sequencing constraint in this function, and
    getting it backwards yields a page whose `/XObject` dictionary is empty
    and whose `Do` operators therefore paint nothing at all, silently.
    """
    mitochondrion.reset_library()
    # The molecule box quotes the banana's length in water molecules. Hand
    # it the measured centreline rather than letting it hold a copy.
    molecules.BANANA_PT = centreline_length()
    content = build_content()
    packed = zlib.compress(content)

    # Two form libraries, laid out end to end from object 7. Mitochondria
    # first (flat: no form references another), then the molecules, whose
    # last entry is the BOX and is the one nested form in the file -- it
    # invokes the ten molecule forms that precede it, so their object
    # numbers have to be known before it can be serialised, which is why
    # it is emitted last rather than first.
    mito = [(n, b, bb, None) for n, b, bb in mitochondrion.forms()]
    mols = molecules.forms()
    forms = mito + mols

    first_form_obj = 7
    obj_of = {name: first_form_obj + i for i, (name, _b, _bb, _r) in enumerate(forms)}
    xobj_entries = b" ".join(f"/{n} {obj_of[n]} 0 R".encode() for n in obj_of)
    resources = (
        b"<< /Font << /F1 5 0 R /F2 6 0 R >> /XObject << " + xobj_entries + b" >> >>"
    )

    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources " + resources + b" /Contents 4 0 R >>"
        ),
        b"<< /Length "
        + str(len(packed)).encode()
        + b" /Filter /FlateDecode >>\nstream\n"
        + packed
        + b"\nendstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>",
    ]

    # One Form XObject per shape, objects 7..n.
    #
    # `/Resources` is written EXPLICITLY on every form, empty where the
    # form needs nothing. A form that inherits the page's resources is
    # legal but reader-dependent (§7.8.3 case 3, which §8.10 calls
    # obsolete), so declaring the truth costs four bytes and removes a
    # class of "renders here, not there". The molecule box is the one form
    # with a non-empty dictionary: it invokes the ten molecules and sets
    # type, so it needs both fonts and ten XObject entries.
    for name, body, bbox, refs in forms:
        if refs:
            inner = b" ".join(f"/{r} {obj_of[r]} 0 R".encode() for r in refs)
            res = (
                b"<< /Font << /F1 5 0 R /F2 6 0 R >> /XObject << " + inner + b" >> >>"
            )
        else:
            res = b"<< >>"
        fp = zlib.compress(body.encode("latin-1"))
        bb = " ".join(f"{v:.2f}" for v in bbox)
        objs.append(
            b"<< /Type /XObject /Subtype /Form /FormType 1 /BBox ["
            + bb.encode()
            + b"] /Resources "
            + res
            + b" /Length "
            + str(len(fp)).encode()
            + b" /Filter /FlateDecode >>\nstream\n"
            + fp
            + b"\nendstream"
        )

    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for i, body in enumerate(objs, start=1):
        offsets.append(len(out))
        out += str(i).encode() + b" 0 obj\n" + body + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 " + str(len(objs) + 1).encode() + b"\n0000000000 65535 f \n"
    for off in offsets:
        out += f"{off:010d} 00000 n \n".encode()
    out += (
        b"trailer\n<< /Size "
        + str(len(objs) + 1).encode()
        + b" /Root 1 0 R >>\nstartxref\n"
        + str(xref).encode()
        + b"\n%%EOF\n"
    )
    return bytes(out)


if __name__ == "__main__":
    import sys

    path = sys.argv[1] if len(sys.argv) > 1 else "banana.pdf"
    open(path, "wb").write(build_pdf())
    print(f"wrote {path}")
    print(f"  banana centreline      {centreline_length()/72*2.54:.2f} cm")
    print(f"  pulp cell              {PULP_W:.0f} um = {um(PULP_W):.6f} pt")
    print(f"  skin cell              {SKIN_W:.0f} x {SKIN_H:.0f} um = {um(SKIN_W):.6f} pt wide")
    print(f"  label size             {LABEL_UM:.0f} um = {um(LABEL_UM):.6f} pt")
    print(f"  arrow tip              {2*PULP_H:.0f} um above the pulp cell, at y={ARROW_TIP[1]:.6f}")
    print(f"  zoom to read a label   ~{10.0/um(LABEL_UM)*100:,.0f} %")
    n_forms, n_f1 = mitochondrion.library_stats()
    n_mito = len(cells_detail.MITOS) + 3 + cells_detail.draw_pulp.mito_count
    f1_pt = mitochondrion.F1_R * 2 * mitochondrion.PT_PER_NM
    print(f"  mitochondria placed    {n_mito} instances from {n_forms} shared forms")
    print(f"  ATP synthase heads     {n_f1} across the form library")
    print(f"  smallest feature       {mitochondrion.F1_R*2:.0f} nm = {f1_pt:.3e} pt")
    print(f"  zoom to see one        ~{10.0/f1_pt*100:,.0f} %")
    wat = 370.0 * molecules.PT_PER_PM
    print(f"  molecule box           {molecules.BOX_W/1000:.0f} x {molecules.BOX_H/1000:.0f} nm"
          f" = {molecules.BOX_W*molecules.PT_PER_PM:.6f} pt wide, {len(molecules.TEN)} molecules")
    print(f"  smallest molecule      water, 0.37 nm = {wat:.3e} pt")
    print(f"  banana : water         {centreline_length()/wat:,.0f} : 1")
