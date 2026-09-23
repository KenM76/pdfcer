//! SVG export with text kept as text (G033).
//!
//! The oracle is the outline export of the same page. With text kept, each
//! glyph is drawn by an SVG consumer from the embedded web font at the
//! `<text>` element's transform and per-character `x`; these tests redo that
//! placement from the embedded font's own outlines and require it to land
//! where the outline export put the same glyph. They also require the
//! embedded font to map each character, through its new `cmap`, to the
//! same outline the source program had.
//!
//! Fixtures: the synthetic donor face from `tools/gen-subset-font-fixtures.py`
//! embedded by pdfcer's own add-text, `hello.pdf` (Standard-14 text drawn
//! from a bundled bare-CFF face) and `textedit/embedded_full.pdf` (an
//! embedded `/Type1C` program) (`docs/LEGAL.md` §5).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::addtext::{self, AddTextRequest};
use pdfcer_render::RenderOptions;
use pdfcer_render::font::FontData;
use pdfcer_render::font::subset::plan_subset;
use pdfcer_render::svg::{SvgExport, SvgOptions, SvgText, export_svg};
use skrifa::outline::DrawSettings;
use skrifa::outline::pen::OutlinePen;
use skrifa::prelude::{LocationRef, Size};
use skrifa::raw::TableProvider as _;
use skrifa::raw::ps::type1::Type1Font;
use skrifa::{FontRef, MetadataProvider};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../fixtures/synthetic/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("missing fixture {path}: {e}"))
}

/// A page showing `text` in the donor face, embedded as a Type0 font.
fn page_with(text: &str, render_mode: u8) -> Document {
    let plan = plan_subset(
        &fixture("text/subset-donor.ttf"),
        0,
        &['A', 'B', 'C'],
        "pdfceSubsetDemo",
        "ABCDEF",
    )
    .unwrap();
    let doc = Document::from_bytes(fixture("hello.pdf")).unwrap();
    let req = AddTextRequest::new(0, (72.0, 500.0), text)
        .with_embedded_face(plan)
        .with_size(36.0)
        .with_render_mode(render_mode);
    Document::from_bytes(addtext::add_text(&doc, &req).unwrap().bytes).unwrap()
}

fn export(doc: &Document, text: SvgText) -> SvgExport {
    export_with(doc, text, &RenderOptions::default())
}

fn export_with(doc: &Document, text: SvgText, options: &RenderOptions) -> SvgExport {
    let page = page_tree::pages(doc).unwrap().remove(0);
    export_svg(
        doc,
        &page,
        options,
        &SvgOptions::default().with_raster_dpi(144.0).with_text(text),
    )
    .unwrap()
}

/// The value of `name="…"` in `element`.
fn attr<'a>(element: &'a str, name: &str) -> &'a str {
    let key = format!(" {name}=\"");
    let at = element
        .find(&key)
        .unwrap_or_else(|| panic!("no {name} in {element}"))
        + key.len();
    let len = element[at..].find('"').unwrap();
    &element[at..at + len]
}

fn floats(s: &str) -> Vec<f32> {
    s.split([' ', ',', '(', ')'])
        .filter_map(|t| t.parse().ok())
        .collect()
}

fn base64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => panic!("bad base64 {c}"),
    };
    let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            acc |= u32::from(val(c)) << (18 - 6 * i);
        }
        for i in 0..chunk.len() - 1 {
            out.push((acc >> (16 - 8 * i)) as u8);
        }
    }
    out
}

/// The `<text>` element whose content is `text`.
fn text_element<'a>(svg: &'a str, text: &str) -> Option<&'a str> {
    let mut rest = svg;
    while let Some(start) = rest.find("<text ") {
        let end = start + rest[start..].find("</text>").unwrap();
        let element = &rest[start..end];
        if element.ends_with(&format!(">{text}")) {
            return Some(element);
        }
        rest = &rest[end..];
    }
    None
}

/// The web font an `@font-face` rule embeds for the family `element`
/// names first, and the rule's `format()`.
fn font_of<'a>(svg: &'a str, element: &str) -> (Vec<u8>, &'a str) {
    let family = attr(element, "font-family").split(',').next().unwrap();
    let rule = format!("@font-face{{font-family:'{family}';");
    let face = svg.find(&rule).unwrap_or_else(|| panic!("no {rule}"));
    let fmt_at = svg[face..].find("format('").unwrap() + face + "format('".len();
    let fmt = &svg[fmt_at..fmt_at + svg[fmt_at..].find('\'').unwrap()];
    let b64_at = svg[face..].find("base64,").unwrap() + face + "base64,".len();
    let b64_len = svg[b64_at..].find(')').unwrap();
    (base64_decode(&svg[b64_at..b64_at + b64_len]), fmt)
}

/// Every on- and off-curve point of a glyph, font units.
#[derive(Default)]
struct Points(Vec<(f32, f32)>);

impl OutlinePen for Points {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push((x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push((x, y));
    }
    fn quad_to(&mut self, a: f32, b: f32, x: f32, y: f32) {
        self.0.extend([(a, b), (x, y)]);
    }
    fn curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, x: f32, y: f32) {
        self.0.extend([(a, b), (c, d), (x, y)]);
    }
    fn close(&mut self) {}
}

fn outline(font: &FontRef<'_>, c: char) -> Vec<(f32, f32)> {
    let gid = font
        .charmap()
        .map(c)
        .unwrap_or_else(|| panic!("{c} unmapped"));
    let glyph = font.outline_glyphs().get(gid).unwrap();
    let mut pen = Points::default();
    glyph
        .draw(
            DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
            &mut pen,
        )
        .unwrap();
    pen.0
}

fn bbox(points: impl IntoIterator<Item = (f32, f32)>) -> [f32; 4] {
    points.into_iter().fold(
        [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
        |[x0, y0, x1, y1], (x, y)| [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
    )
}

/// The device-space bounding box of each `<path>` an export wrote, in
/// document order.
fn outline_boxes(svg: &str) -> Vec<[f32; 4]> {
    svg.split("<path ")
        .skip(1)
        .map(|p| {
            let d: Vec<f32> = attr(&format!(" {p}"), "d")
                .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
                .filter_map(|t| t.parse().ok())
                .collect();
            bbox(d.chunks(2).map(|c| (c[0], c[1])))
        })
        .collect()
}

#[test]
fn kept_text_is_one_text_element_with_an_embedded_font_that_maps_each_character() {
    let doc = page_with("ABCA", 0);
    let out = export(&doc, SvgText::KeepText);
    let t = out.outcome.text;
    // hello.pdf's own two Helvetica lines are kept too, in a second font.
    assert_eq!(t.runs_as_text, 3, "{t:?}");
    assert_eq!(t.fonts_embedded, 2, "{t:?}");

    let svg = &out.svg;
    let element = text_element(svg, "ABCA").expect("a <text> element");
    assert_eq!(attr(element, "xml:space"), "preserve");
    assert!(attr(element, "font-family").starts_with("pdfcer-f"));

    let (web, format) = font_of(svg, element);
    assert_eq!(format, "truetype");
    let web = FontRef::new(&web).expect("the embedded font parses");
    let donor_bytes = fixture("text/subset-donor.ttf");
    let donor = FontRef::new(&donor_bytes).unwrap();
    for c in ['A', 'B', 'C'] {
        assert_eq!(outline(&web, c), outline(&donor, c), "glyph for {c}");
    }
}

#[test]
fn each_kept_glyph_lands_where_the_outline_export_drew_it() {
    let doc = page_with("ABCA", 0);
    let outlines = export(&doc, SvgText::Outlines);
    let kept = export(&doc, SvgText::KeepText);
    assert!(!outlines.svg.contains("<text "));
    assert_eq!(outlines.outcome.text, Default::default());

    // The donor text is drawn last: its four glyphs are the outline
    // export's last four paths.
    let all = outline_boxes(&outlines.svg);
    let glyph_boxes = &all[all.len() - 4..];

    let svg = &kept.svg;
    let element = text_element(svg, "ABCA").unwrap();
    let m = floats(attr(element, "transform"));
    let xs = floats(attr(element, "x"));
    let upem: f32 = attr(element, "font-size").parse().unwrap();
    assert_eq!(xs.len(), 4);

    let donor_bytes = fixture("text/subset-donor.ttf");
    let donor = FontRef::new(&donor_bytes).unwrap();
    assert_eq!(
        upem,
        f32::from(donor.head().unwrap().units_per_em()),
        "font-size is one em in font units"
    );
    for ((c, x), want) in "ABCA".chars().zip(&xs).zip(glyph_boxes) {
        // SVG draws glyph point (px, py) at text-space (x + px, -py), then
        // applies the transform.
        let placed = outline(&donor, c).into_iter().map(|(px, py)| {
            let (u, v) = (x + px, -py);
            (m[0] * u + m[2] * v + m[4], m[1] * u + m[3] * v + m[5])
        });
        let got = bbox(placed);
        for (g, w) in got.iter().zip(want) {
            assert!(
                (g - w).abs() < 0.05,
                "{c}: kept {got:?} vs outlines {want:?}"
            );
        }
    }
}

#[test]
fn stroked_text_stays_outlines_and_is_counted() {
    let doc = page_with("AB", 1);
    let out = export(&doc, SvgText::KeepText);
    assert!(text_element(&out.svg, "AB").is_none());
    // Only hello.pdf's own Helvetica lines are kept.
    assert_eq!(out.outcome.text.runs_as_text, 2);
    assert_eq!(out.outcome.text.fallback_paint, 1, "{:?}", out.outcome.text);
    assert_eq!(out.outcome.text.fonts_embedded, 1);
}

/// Each kept run's characters placed from its own embedded font's
/// outlines by the run's `transform` and `x`, one box per inked character,
/// in document order.
fn placed_boxes(svg: &str) -> Vec<[f32; 4]> {
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(start) = rest.find("<text ") {
        let end = start + rest[start..].find("</text>").unwrap();
        let element = &rest[start..end];
        let text = &element[element.rfind('>').unwrap() + 1..];
        let (web, _) = font_of(svg, element);
        let web = FontRef::new(&web).unwrap();
        let m = floats(attr(element, "transform"));
        let xs = floats(attr(element, "x"));
        for (c, x) in text.chars().zip(&xs) {
            let points = outline(&web, c);
            if points.is_empty() {
                continue;
            }
            out.push(bbox(points.into_iter().map(|(px, py)| {
                let (u, v) = (x + px, -py);
                (m[0] * u + m[2] * v + m[4], m[1] * u + m[3] * v + m[5])
            })));
        }
        rest = &rest[end..];
    }
    out
}

/// A bare-CFF program must be kept: framed as OpenType, embedded with its
/// own charstrings, and every glyph drawn where the outline export drew it.
fn assert_cff_page_kept(doc: &Document) {
    let web = assert_page_kept(doc, &RenderOptions::default(), "opentype");
    assert_eq!(&web[..4], b"OTTO");
    let web = FontRef::new(&web).unwrap();
    assert_eq!(web.head().unwrap().units_per_em(), 1000);
}

/// Every run on the page is kept, in one embedded font of `format`, and
/// every glyph is drawn where the outline export drew it. Returns the
/// embedded font.
fn assert_page_kept(doc: &Document, options: &RenderOptions, format: &str) -> Vec<u8> {
    let kept = export_with(doc, SvgText::KeepText, options);
    let t = kept.outcome.text;
    assert_eq!(t.runs_as_outlines(), 0, "{t:?}");
    assert!(t.runs_as_text >= 1, "{t:?}");
    assert_eq!(t.fonts_embedded, 1, "{t:?}");

    let element = &kept.svg[kept.svg.find("<text ").unwrap()..];
    let (web, got_format) = font_of(&kept.svg, element);
    assert_eq!(got_format, format);
    FontRef::new(&web).expect("the embedded font parses");

    // The kept export's paths are the page's non-text paths; the outline
    // export draws those same paths first, then one per inked glyph.
    let outlines = export_with(doc, SvgText::Outlines, options);
    let all = outline_boxes(&outlines.svg);
    let want = &all[outline_boxes(&kept.svg).len()..];
    let got = placed_boxes(&kept.svg);
    assert_eq!(got.len(), want.len(), "one box per inked glyph");
    for (g, w) in got.iter().zip(want) {
        for (a, b) in g.iter().zip(w) {
            assert!((a - b).abs() < 0.05, "kept {g:?} vs outlines {w:?}");
        }
    }
    web
}

#[test]
fn bundled_standard_14_cff_text_is_kept() {
    // `hello.pdf` names Helvetica without embedding it: its glyphs come from
    // the bundled Standard-14 face, a bare CFF program.
    assert_cff_page_kept(&Document::from_bytes(fixture("hello.pdf")).unwrap());
}

#[test]
fn an_embedded_type1c_program_is_kept() {
    assert_cff_page_kept(&Document::from_bytes(fixture("textedit/embedded_full.pdf")).unwrap());
}

/// A one-page PDF showing `ABCA` in a non-embedded TrueType font named
/// `Donor`, which only a supplied face can draw.
fn page_naming_donor() -> Document {
    page_showing_abca(
        "<< /Type /Font /Subtype /TrueType /BaseFont /Donor \
         /FirstChar 65 /LastChar 67 /Widths [600 600 600] \
         /Encoding /WinAnsiEncoding >>",
        &[],
    )
}

/// A one-page PDF showing `ABCA` in the font dictionary `font` (object 4);
/// `extra` are objects 6 onward.
fn page_showing_abca(font: &str, extra: &[Vec<u8>]) -> Document {
    let content = "BT /F1 36 Tf 72 500 Td (ABCA) Tj ET";
    let mut objects: Vec<Vec<u8>> = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
         /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
            .to_owned(),
        font.to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ]
    .map(String::into_bytes)
    .to_vec();
    objects.extend_from_slice(extra);
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    let size = objects.len() + 1;
    pdf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for o in offsets {
        pdf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(pdf).unwrap()
}

/// A collection of `faces`: the `ttcf` header, then each face with its
/// table offsets moved to count from the start of the collection.
fn collection(faces: &[&[u8]]) -> Vec<u8> {
    let be32 = |d: &[u8], at: usize| u32::from_be_bytes(d[at..at + 4].try_into().unwrap());
    let mut out = b"ttcf".to_vec();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&u32::try_from(faces.len()).unwrap().to_be_bytes());
    let mut at = 12 + 4 * faces.len();
    for face in faces {
        out.extend_from_slice(&u32::try_from(at).unwrap().to_be_bytes());
        at += face.len().next_multiple_of(4);
    }
    for face in faces {
        let base = u32::try_from(out.len()).unwrap();
        let mut face = face.to_vec();
        let count = usize::from(u16::from_be_bytes([face[4], face[5]]));
        for i in 0..count {
            let rec = 12 + i * 16 + 8;
            let moved = be32(&face, rec) + base;
            face[rec..rec + 4].copy_from_slice(&moved.to_be_bytes());
        }
        out.extend_from_slice(&face);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    out
}

#[test]
fn a_supplied_font_collection_is_kept_from_its_first_face() {
    let donor = fixture("text/subset-donor.ttf");
    let mut options = RenderOptions::default();
    options.fonts.insert_named(
        "Donor",
        FontData::new(collection(&[&donor, &fixture("text/subset-donor.ttf")])),
    );
    let web = assert_page_kept(&page_naming_donor(), &options, "truetype");
    let web = FontRef::new(&web).unwrap();
    let donor = FontRef::new(&donor).unwrap();
    for c in ['A', 'B', 'C'] {
        assert_eq!(outline(&web, c), outline(&donor, c), "glyph for {c}");
    }
}

/// A Type 1 charstring number (Adobe Type 1 Font Format §6.2).
fn t1_number(out: &mut Vec<u8>, v: i32) {
    match v {
        -107..=107 => out.push(u8::try_from(v + 139).unwrap()),
        108..=1131 => {
            let n = v - 108;
            out.extend_from_slice(&[u8::try_from(n / 256 + 247).unwrap(), (n % 256) as u8]);
        }
        -1131..=-108 => {
            let n = -v - 108;
            out.extend_from_slice(&[u8::try_from(n / 256 + 251).unwrap(), (n % 256) as u8]);
        }
        _ => {
            out.push(255);
            out.extend_from_slice(&v.to_be_bytes());
        }
    }
}

/// Type 1 encryption (§7.1) with seed `r`, after `prefix` zero bytes.
fn t1_encrypt(plain: &[u8], mut r: u16, prefix: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(plain.len() + prefix);
    for &p in std::iter::repeat_n(&0u8, prefix).chain(plain) {
        let c = p ^ (r >> 8) as u8;
        r = (u16::from(c).wrapping_add(r))
            .wrapping_mul(52845)
            .wrapping_add(22719);
        out.push(c);
    }
    out
}

/// A synthetic Type 1 font named `Demo` with `.notdef`, a rectangle `A`, a
/// triangle `B` and a curved `C`, each 600 units wide, in the standard
/// encoding, with the given `FontMatrix`. Returns the cleartext part and the
/// binary eexec part, which together are a `/FontFile` program.
fn type1_parts_with_matrix(matrix: &str) -> (Vec<u8>, Vec<u8>) {
    // (name, operands-and-operators); an operator is OP plus its code.
    const OP: i32 = 10_000;
    const HSBW: i32 = OP + 13;
    const RLINETO: i32 = OP + 5;
    const RRCURVETO: i32 = OP + 8;
    const CLOSEPATH: i32 = OP + 9;
    const ENDCHAR: i32 = OP + 14;
    const RMOVETO: i32 = OP + 21;
    let glyphs: [(&str, &[i32]); 4] = [
        (".notdef", &[0, 500, HSBW, ENDCHAR]),
        (
            "A",
            &[
                0, 600, HSBW, 50, 0, RMOVETO, 500, 0, RLINETO, 0, 700, RLINETO, -500, 0, RLINETO,
                CLOSEPATH, ENDCHAR,
            ],
        ),
        (
            "B",
            &[
                0, 600, HSBW, 100, 0, RMOVETO, 400, 0, RLINETO, -200, 600, RLINETO, CLOSEPATH,
                ENDCHAR,
            ],
        ),
        (
            "C",
            &[
                0, 600, HSBW, 100, 100, RMOVETO, 0, 300, 200, 200, 200, 0, RRCURVETO, -200, -500,
                RLINETO, CLOSEPATH, ENDCHAR,
            ],
        ),
    ];
    let clear = format!(
        "%!PS-AdobeFont-1.0: Demo 001\n\
        11 dict begin\n\
        /FontName /Demo def\n\
        /FontType 1 def\n\
        /PaintType 0 def\n\
        /FontMatrix [{matrix}] readonly def\n\
        /FontBBox {{0 0 600 700}} readonly def\n\
        /Encoding StandardEncoding def\n\
        currentdict end\n\
        currentfile eexec\n"
    )
    .into_bytes();
    let mut private = b"dup /Private 8 dict dup begin\n\
        /RD {string currentfile exch readstring pop} executeonly def\n\
        /ND {noaccess def} executeonly def\n\
        /NP {noaccess put} executeonly def\n\
        /lenIV 4 def\n\
        /BlueValues [] def\n\
        /password 5839 def\n\
        /MinFeature {16 16} def\n\
        2 index /CharStrings 4 dict dup begin\n"
        .to_vec();
    for (name, program) in glyphs {
        let mut plain = Vec::new();
        for &v in program {
            if v >= OP {
                plain.push(u8::try_from(v - OP).unwrap());
            } else {
                t1_number(&mut plain, v);
            }
        }
        let encrypted = t1_encrypt(&plain, 4330, 4);
        private.extend_from_slice(format!("/{name} {} RD ", encrypted.len()).as_bytes());
        private.extend_from_slice(&encrypted);
        private.extend_from_slice(b" ND\n");
    }
    private.extend_from_slice(
        b"end\nend\nreadonly put\nnoaccess put\n\
        dup /FontName get exch definefont pop\n\
        mark currentfile closefile\n",
    );
    (clear, t1_encrypt(&private, 55665, 4))
}

/// The Type 1 test font with the standard 1000-unit em.
fn type1_parts() -> (Vec<u8>, Vec<u8>) {
    type1_parts_with_matrix("0.001 0 0 0.001 0 0")
}

/// The trailer every Type 1 program ends with: 512 zeros and `cleartomark`.
fn type1_trailer() -> Vec<u8> {
    let mut t = Vec::new();
    for _ in 0..8 {
        t.extend_from_slice(&[b'0'; 64]);
        t.push(b'\n');
    }
    t.extend_from_slice(b"cleartomark\n");
    t
}

/// The font as a `.pfb` file: text, binary and trailer segments, then EOF.
fn type1_pfb() -> Vec<u8> {
    pfb(type1_parts())
}

/// `parts` framed as a `.pfb` file.
fn pfb((clear, binary): (Vec<u8>, Vec<u8>)) -> Vec<u8> {
    let mut out = Vec::new();
    for (tag, data) in [(1u8, clear), (2, binary), (1, type1_trailer())] {
        out.extend_from_slice(&[0x80, tag]);
        out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&data);
    }
    out.extend_from_slice(&[0x80, 3]);
    out
}

/// What the Type 1 test font draws for `c`, point by point.
fn type1_outline(c: char) -> Vec<(f32, f32)> {
    let pfb = type1_pfb();
    let font = Type1Font::new(&pfb).unwrap();
    let name = c.to_string();
    let (gid, _) = font.glyph_names().find(|(_, n)| *n == name).unwrap();
    let mut pen = Points::default();
    font.draw(gid, None, &mut pen).unwrap();
    pen.0
}

/// A kept Type 1 page embeds an OpenType font whose glyphs are the Type 1
/// glyphs.
fn assert_type1_page_kept(doc: &Document, options: &RenderOptions) {
    let web = assert_page_kept(doc, options, "opentype");
    assert_eq!(&web[..4], b"OTTO");
    let web = FontRef::new(&web).unwrap();
    let metrics = web.glyph_metrics(Size::unscaled(), LocationRef::default());
    for c in ['A', 'B', 'C'] {
        assert_eq!(outline(&web, c), type1_outline(c), "glyph for {c}");
        let gid = web.charmap().map(c).unwrap();
        assert_eq!(metrics.advance_width(gid), Some(600.0), "advance of {c}");
    }
}

#[test]
fn an_embedded_type1_program_is_kept() {
    let (clear, binary) = type1_parts();
    let trailer = type1_trailer();
    let mut program = clear.clone();
    program.extend_from_slice(&binary);
    program.extend_from_slice(&trailer);
    let mut file = format!(
        "<< /Length {} /Length1 {} /Length2 {} /Length3 {} >>\nstream\n",
        program.len(),
        clear.len(),
        binary.len(),
        trailer.len()
    )
    .into_bytes();
    file.extend_from_slice(&program);
    file.extend_from_slice(b"\nendstream");
    let doc = page_showing_abca(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Demo \
         /FirstChar 65 /LastChar 67 /Widths [600 600 600] \
         /Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>",
        &[
            b"<< /Type /FontDescriptor /FontName /Demo /Flags 32 \
              /FontBBox [0 0 600 700] /ItalicAngle 0 /Ascent 700 /Descent 0 \
              /CapHeight 700 /StemV 80 /FontFile 7 0 R >>"
                .to_vec(),
            file,
        ],
    );
    assert_type1_page_kept(&doc, &RenderOptions::default());
}

/// `ABCA` in a non-embedded Type 1 font named `Demo`.
fn page_naming_demo() -> Document {
    page_showing_abca(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Demo \
         /FirstChar 65 /LastChar 67 /Widths [600 600 600] \
         /Encoding /WinAnsiEncoding >>",
        &[],
    )
}

#[test]
fn a_supplied_pfb_font_is_kept() {
    let mut options = RenderOptions::default();
    options
        .fonts
        .insert_named("Demo", FontData::new(type1_pfb()));
    assert_type1_page_kept(&page_naming_demo(), &options);
}

#[test]
fn a_type1_font_whose_em_is_not_1000_units_stays_outlines_and_is_counted() {
    let mut options = RenderOptions::default();
    let font = pfb(type1_parts_with_matrix("0.002 0 0 0.002 0 0"));
    options.fonts.insert_named("Demo", FontData::new(font));
    let t = export_with(&page_naming_demo(), SvgText::KeepText, &options)
        .outcome
        .text;
    assert_eq!(t.runs_as_text, 0, "{t:?}");
    assert_eq!(t.fallback_font_build, 1, "{t:?}");
    assert_eq!(t.runs_as_outlines(), 1, "{t:?}");
}
