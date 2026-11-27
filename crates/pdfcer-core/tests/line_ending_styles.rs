//! Every `/LE` line-ending style of ISO 32000-2 Table 179 (32000-1 Table
//! 176) painted on every caller of the shared ending builder: `/Line`
//! (§12.5.6.7), `/PolyLine` (§12.5.6.9) and the `/FreeText` callout
//! (§12.5.6.6). `/IC` fills the closed shapes of a Line or PolyLine; a
//! FreeText has no ending-interior key (FT-A1), so its shapes stay unfilled.
//! Also the derived (no-`/AP`) look of a callout.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot_author::{FreeTextC, derive_appearance};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::object::{Dict, ObjId, Object};

/// The ten Table 179 names.
const STYLES: [&str; 10] = [
    "Square",
    "Circle",
    "Diamond",
    "OpenArrow",
    "ClosedArrow",
    "None",
    "Butt",
    "ROpenArrow",
    "RClosedArrow",
    "Slash",
];

/// Width 2 ⇒ pdfcer's arrow length max(4·2, 8) = 8; the symmetric shapes
/// span half of it either side of the endpoint.
const A: f64 = 8.0;
const HALF_BARB: f64 = A * 0.414_213_562_373_095;
const H: f64 = A / 2.0;

fn session(annot: &str) -> EditSession {
    let bodies: Vec<String> = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Annots [4 0 R] >>",
        annot,
    ]
    .iter()
    .map(|b| (*b).to_owned())
    .collect();
    EditSession::new(Document::from_bytes(crate::block_layout::build_pdf(&bodies)).unwrap())
}

fn derived(annot: &str) -> Result<(Vec<u8>, Vec<String>), String> {
    let s = session(annot);
    let dict: Dict = match &s.document().get(ObjId::new(4, 0)).unwrap().value {
        Object::Dict(d) => d.clone(),
        other => panic!("not a dict: {other:?}"),
    };
    derive_appearance(&s, &dict, FreeTextC::Border)
        .map(|d| (d.ap_content, d.notes))
        .map_err(|e| e.reason)
}

fn tokens(content: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(content)
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// The tokens `with` has that `base` lacks: the common prefix and suffix
/// removed. An ending is drawn after its shaft and before anything else.
fn added(base: &[String], with: &[String]) -> Vec<String> {
    let pre = base.iter().zip(with).take_while(|(a, b)| a == b).count();
    let post = base[pre..]
        .iter()
        .rev()
        .zip(with[pre..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    with[pre..with.len() - post].to_vec()
}

/// Every `x y m` / `x y l` point, and every `c` end point.
fn points(t: &[String]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for (i, op) in t.iter().enumerate() {
        if matches!(op.as_str(), "m" | "l" | "c")
            && i >= 2
            && let (Ok(x), Ok(y)) = (t[i - 2].parse(), t[i - 1].parse())
        {
            out.push((x, y));
        }
    }
    out
}

fn paints(t: &[String]) -> Vec<&str> {
    t.iter()
        .map(String::as_str)
        .filter(|op| matches!(*op, "S" | "s" | "f" | "f*" | "B" | "b" | "B*" | "b*"))
        .collect()
}

fn has(points: &[(f64, f64)], want: (f64, f64)) -> bool {
    points
        .iter()
        .any(|p| (p.0 - want.0).abs() < 1e-2 && (p.1 - want.1).abs() < 1e-2)
}

const fn closed(style: &str) -> bool {
    matches!(
        style.as_bytes(),
        b"Square" | b"Circle" | b"Diamond" | b"ClosedArrow" | b"RClosedArrow"
    )
}

/// The points a style must pass through, its tip at `t` with the shaft
/// running toward +x.
fn expected_points(style: &str, t: (f64, f64)) -> Vec<(f64, f64)> {
    let (x, y) = t;
    let s30 = 0.5 * H;
    let c30 = 0.866_025_403_784_438_6 * H;
    match style {
        "Square" => vec![
            (x - H, y - H),
            (x + H, y - H),
            (x + H, y + H),
            (x - H, y + H),
        ],
        "Circle" => vec![(x + H, y), (x, y + H), (x - H, y), (x, y - H)],
        "Diamond" => vec![(x + H, y), (x, y + H), (x - H, y), (x, y - H)],
        "OpenArrow" | "ClosedArrow" => {
            vec![(x, y), (x + A, y + HALF_BARB), (x + A, y - HALF_BARB)]
        }
        "ROpenArrow" | "RClosedArrow" => {
            vec![(x, y), (x - A, y + HALF_BARB), (x - A, y - HALF_BARB)]
        }
        "Butt" => vec![(x, y + H), (x, y - H)],
        // 30° clockwise from the perpendicular: a `/` across a horizontal line.
        "Slash" => vec![(x - s30, y - c30), (x + s30, y + c30)],
        _ => Vec::new(),
    }
}

/// The ending's own tokens, each asserted to be the shape, fill and stroke
/// `style` names.
fn check(style: &str, ending: &[String], tip: (f64, f64), filled: bool, on: &str) {
    if style == "None" {
        assert!(ending.is_empty(), "{on} /None drew {ending:?}");
        return;
    }
    let pts = points(ending);
    for want in expected_points(style, tip) {
        assert!(
            has(&pts, want),
            "{on} /{style}: no point {want:?} in {ending:?}"
        );
    }
    let want = if closed(style) && filled { "B" } else { "S" };
    assert_eq!(paints(ending), vec![want], "{on} /{style}: {ending:?}");
    let curves = ending.iter().filter(|t| *t == "c").count();
    assert_eq!(
        curves,
        if style == "Circle" { 4 } else { 0 },
        "{on} /{style}"
    );
}

fn line(le: &str, ic: &str) -> String {
    format!(
        "<< /Type /Annot /Subtype /Line /Rect [80 80 220 120] /L [100 100 200 100] \
         /C [0 0 0] /BS << /W 2 >> /LE [/{le} /None] {ic} >>"
    )
}

fn polyline(le: &str, ic: &str) -> String {
    format!(
        "<< /Type /Annot /Subtype /PolyLine /Rect [80 80 220 220] \
         /Vertices [100 100 200 100 200 200] /C [0 0 0] /BS << /W 2 >> \
         /LE [/{le} /None] {ic} >>"
    )
}

/// The callout's `/Rect` corner is (50, 50), so its appearance draws the
/// start (100, 100) at (50, 50). `/IC` is pdfcer's box fill, never the
/// ending's interior.
fn callout(le: &str) -> String {
    format!(
        "<< /Type /Annot /Subtype /FreeText /Rect [50 50 300 200] /RD [150 0 0 0] \
         /IT /FreeTextCallout /CL [100 100 150 100 200 150] /LE /{le} /IC [0 0 1] \
         /C [1 0 0] /BS << /W 2 >> /DA (/Helv 10 Tf 0 g) /Contents (Note) >>"
    )
}

fn each_style(on: &str, build: impl Fn(&str) -> String, tip: (f64, f64), filled: bool) {
    let base = tokens(&derived(&build("None")).unwrap().0);
    for style in STYLES {
        let with = tokens(&derived(&build(style)).unwrap().0);
        check(style, &added(&base, &with), tip, filled, on);
    }
}

#[test]
fn every_style_paints_on_a_line_filled_by_its_interior_colour() {
    each_style(
        "Line+IC",
        |le| line(le, "/IC [1 0 0]"),
        (100.0, 100.0),
        true,
    );
}

#[test]
fn every_style_paints_on_a_line_stroked_only_without_ic() {
    each_style("Line", |le| line(le, ""), (100.0, 100.0), false);
}

#[test]
fn every_style_paints_on_a_polyline_filled_by_its_interior_colour() {
    each_style(
        "PolyLine+IC",
        |le| polyline(le, "/IC [1 0 0]"),
        (100.0, 100.0),
        true,
    );
}

/// A FreeText has no ending-interior key (FT-A1): its closed shapes are
/// stroked only, even when the dictionary carries `/IC` (the box fill).
#[test]
fn every_style_paints_on_a_derived_callout_never_filled() {
    each_style("Callout", callout, (50.0, 50.0), false);
}

/// Drawn, not declined, and the derived look says so.
#[test]
fn a_callout_without_ap_is_derived_with_its_line() {
    let (content, notes) = derived(&callout("OpenArrow")).expect("a callout is derived");
    let t = tokens(&content);
    let pts = points(&t);
    for knee in [(50.0, 50.0), (100.0, 50.0), (150.0, 100.0)] {
        assert!(has(&pts, knee), "callout line misses {knee:?}: {t:?}");
    }
    assert!(
        notes.iter().any(|n| n.contains("callout")),
        "the derived callout is disclosed: {notes:?}"
    );
    let (_, plain) = derived(&callout("None")).unwrap();
    assert!(
        plain.contains(&"callout line (/CL) drawn".to_owned()),
        "an ending-less callout is disclosed too: {plain:?}"
    );
}

/// `/CL` without `/IT /FreeTextCallout` is not a callout (Table 177): the
/// box is drawn and the unread `/CL` is named.
#[test]
fn cl_without_the_callout_intent_draws_the_box_and_names_cl() {
    let annot = "<< /Type /Annot /Subtype /FreeText /Rect [50 50 300 200] \
                 /CL [100 100 200 150] /C [1 0 0] /BS << /W 2 >> \
                 /DA (/Helv 10 Tf 0 g) /Contents (Note) >>";
    let (_, notes) = derived(annot).expect("drawn as a plain box");
    assert!(notes.iter().any(|n| n.contains("/CL")), "{notes:?}");
}

/// An unreadable `/CL` on a callout is geometry pdfcer would have to guess.
#[test]
fn a_callout_with_malformed_cl_is_declined() {
    let annot = "<< /Type /Annot /Subtype /FreeText /Rect [50 50 300 200] \
                 /IT /FreeTextCallout /CL [100 100 200] /C [1 0 0] \
                 /DA (/Helv 10 Tf 0 g) /Contents (Note) >>";
    let why = derived(annot).expect_err("declined");
    assert!(why.contains("/CL"), "{why}");
}

/// A name outside Table 179 draws nothing, and the derived callout says the
/// ending was not drawn.
#[test]
fn an_unknown_callout_ending_draws_nothing_and_is_noted() {
    let base = tokens(&derived(&callout("None")).unwrap().0);
    let (content, notes) = derived(&callout("Bogus")).unwrap();
    assert!(added(&base, &tokens(&content)).is_empty());
    assert!(
        notes
            .iter()
            .any(|n| n == "callout ending (/LE) not drawn: not a Table 179 style"),
        "{notes:?}"
    );
}

/// A reversed arrow's barbs reach past the line's end, so the look's
/// rectangle grows to hold them and their stroke.
#[test]
fn a_reversed_arrow_fits_inside_its_rect() {
    for style in ["ROpenArrow", "RClosedArrow"] {
        let s = session(&line(style, ""));
        let dict: Dict = match &s.document().get(ObjId::new(4, 0)).unwrap().value {
            Object::Dict(d) => d.clone(),
            other => panic!("not a dict: {other:?}"),
        };
        let r = derive_appearance(&s, &dict, FreeTextC::Border)
            .unwrap()
            .rect;
        // The barb tip plus half the width-2 stroke, miter included (sec 22.5°).
        let reach = A + 1.0 / 0.923_879_532_511_286_7;
        assert!(r.llx <= 100.0 - reach, "/{style}: {r:?}");
        assert!(r.lly <= 100.0 - HALF_BARB - 1.0, "/{style}: {r:?}");
    }
}
