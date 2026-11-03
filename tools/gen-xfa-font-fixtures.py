"""Synthetic fonts for the XFA real-font tests (fixtures/synthetic/xfa/fonts/):
one box glyph for every printable ASCII character, a chosen advance,
family/style per variant. Run from the repo root; needs fontTools.
Head timestamps are pinned so a rerun reproduces the same bytes."""
import os
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.pens.t2CharStringPen import T2CharStringPen

OUT = 'fixtures/synthetic/xfa/fonts'
os.makedirs(OUT, exist_ok=True)

def box(pen, adv):
    pen.moveTo((50, 0)); pen.lineTo((50, 700)); pen.lineTo((adv - 50, 700)); pen.lineTo((adv - 50, 0)); pen.closePath()

def build(fname, family, style, ps, adv, weight=400, width=5, fs_type=0, italic=False, cff=False):
    order = ['.notdef', 'space', 'box']
    fb = FontBuilder(1000, isTTF=not cff)
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap({0x20: 'space', **{c: 'box' for c in range(0x21, 0x7F)}})
    metrics = {'.notdef': (500, 0), 'space': (adv // 2, 0), 'box': (adv, 50)}
    if cff:
        cs = {}
        for g in order:
            p = T2CharStringPen(metrics[g][0], None)
            if g != 'space':
                box(p, metrics[g][0])
            cs[g] = p.getCharString()
        fb.setupCFF(ps, {'FullName': f'{family} {style}', 'FamilyName': family}, cs, {})
    else:
        glyphs = {}
        for g in order:
            p = TTGlyphPen(None)
            if g != 'space':
                box(p, metrics[g][0])
            glyphs[g] = p.glyph()
        fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({'familyName': family, 'styleName': style, 'psName': ps,
                       'uniqueFontIdentifier': ps, 'fullName': f'{family} {style}'})
    fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200,
                usWeightClass=weight, usWidthClass=width, fsType=fs_type, sCapHeight=700,
                fsSelection=(0x01 if italic else 0) | (0x20 if weight >= 700 else 0) | (0x40 if not italic and weight < 700 else 0))
    fb.setupPost(isFixedPitch=0, italicAngle=-12 if italic else 0)
    fb.updateHead(created=0, modified=0, macStyle=(1 if weight >= 700 else 0) | (2 if italic else 0))
    fb.save(f'{OUT}/{fname}')

build('XfaTestSans-Regular.ttf', 'Xfa Test Sans', 'Regular', 'XfaTestSans-Regular', 800)
build('XfaTestSans-Bold.ttf', 'Xfa Test Sans', 'Bold', 'XfaTestSans-Bold', 900, weight=700)
build('XfaTestSans-Cond.ttf', 'Xfa Test Sans', 'Condensed', 'XfaTestSans-Cond', 400, width=3)
build('XfaTestSerif-Regular.otf', 'Xfa Test Serif', 'Regular', 'XfaTestSerif-Regular', 700, cff=True)
build('XfaTestLocked-Regular.ttf', 'Xfa Test Locked', 'Regular', 'XfaTestLocked-Regular', 600, fs_type=2)
print(sorted(os.listdir(OUT)), [os.path.getsize(f'{OUT}/{f}') for f in sorted(os.listdir(OUT))])
