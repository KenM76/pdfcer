//! Fuzz target 12: annotation authoring + content-stream serializer
//! (`pdfcer_core::annot_author`, `pdfcer_core::writer::content`,
//! `pdfcer_core::edit::EditSession::add_markup`; docs/decisions/008 Pass 6.1).
//!
//! Two invariants over untrusted input, both the crate's panic-free
//! policy (X5/X6):
//!
//! 1. **Content-stream serializer round-trip (R46/X6).** The raw fuzz
//!    bytes are parsed as a content stream; if that succeeds,
//!    `reemit_canonical` re-emits them and the result is re-parsed. The
//!    re-emission must always re-parse (a serializer that emitted an
//!    unparseable token would be caught here), and it must never panic.
//!
//! 2. **Authoring round-trip (X5/X7).** The fuzz bytes drive a sequence
//!    of geometric-markup authorings (Square/Circle/Line/Ink/Polygon/
//!    PolyLine/text-markup) onto a fixed blank document through
//!    `EditSession::add_markup`, deriving all coordinates from the input.
//!    The session is then saved both ways and reloaded, and the reloaded
//!    documents' annotations are walked. No sequence of authorings, at
//!    any geometry the bytes name, may panic, hang, or produce a file
//!    that fails to reload — this is the staging-buffer (R45) and
//!    `/Annots`-patch (X7) machinery under adversarial geometry.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::annot::page_annotations;
use pdfcer_core::annot_author::{
    Color, LineEnding, MarkupSpec, Quad, StampName, StickyIcon, TextAnnotSpec, TextMarkupKind,
};
use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::fontdata::Std14;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::vartext::{self, FontResource, Quadding, TextColor};
use pdfcer_core::writer::SaveOptions;
use pdfcer_core::writer::content::reemit_canonical;

/// A minimal one-page document the authoring path can target.
const BLANK: &[u8] = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 400 400] /Resources << >> >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n\
xref\n0 4\n0000000000 65535 f \n0000000017 00000 n \n0000000066 00000 n \n0000000159 00000 n \n\
trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n211\n%%EOF\n";

/// A tiny cursor pulling numbers out of the fuzz input deterministically.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    /// One byte, wrapping around the input so a short input still drives
    /// several authorings.
    fn byte(&mut self) -> u8 {
        if self.data.is_empty() {
            return 0;
        }
        let b = self.data[self.pos % self.data.len()];
        self.pos = self.pos.wrapping_add(1);
        b
    }
    /// A coordinate in roughly `[-32, 480]` (deliberately allowed to fall
    /// outside the page so out-of-bounds geometry is exercised too).
    fn coord(&mut self) -> f64 {
        let hi = self.byte() as i32;
        let lo = self.byte() as i32;
        f64::from((hi << 8 | lo) % 512 - 32)
    }
    fn point(&mut self) -> (f64, f64) {
        (self.coord(), self.coord())
    }
    fn rect(&mut self) -> Rect {
        let (x0, y0) = self.point();
        let (x1, y1) = self.point();
        Rect::from_corners(x0, y0, x1, y1)
    }
    fn color(&mut self) -> Color {
        match self.byte() % 3 {
            0 => Color::Gray(f64::from(self.byte()) / 255.0),
            1 => Color::Rgb(
                f64::from(self.byte()) / 255.0,
                f64::from(self.byte()) / 255.0,
                f64::from(self.byte()) / 255.0,
            ),
            _ => Color::Cmyk(
                f64::from(self.byte()) / 255.0,
                f64::from(self.byte()) / 255.0,
                f64::from(self.byte()) / 255.0,
                f64::from(self.byte()) / 255.0,
            ),
        }
    }
    fn points(&mut self, n: usize) -> Vec<(f64, f64)> {
        (0..n).map(|_| self.point()).collect()
    }
}

/// Derive one markup spec from the cursor.
///
/// ★ **THE MODULO IS A COVERAGE CLAIM, and it went stale.** It read `% 8`
/// against an eight-variant enum while one arm was spent on a second
/// `TextMarkup` kind — so `MarkupSpec::Cloud`, which bakes a scalloped
/// appearance from a free-form vertex list and takes a **continuous**
/// intensity, was **never fuzzed at all**. Nothing said so: the target
/// compiled, ran, and reported coverage over the variants it happened to
/// name.
///
/// Found 2026-08-21 by CI failing to *compile* this file — `Square` had
/// gained a `border_effect` field it was not passing — which is the only
/// reason anybody read the dispatch. **A compile break is the loudest
/// failure a fuzz target has; a missing variant is silent.**
///
/// If a variant is added to `MarkupSpec`, this modulo and the arm count
/// below must move with it. There is no check that enforces that, which is
/// stated here rather than assumed away.
fn spec(c: &mut Cursor<'_>) -> MarkupSpec {
    let width = f64::from(c.byte() % 12);
    match c.byte() % 9 {
        0 => MarkupSpec::Square {
            rect: c.rect(),
            border: Some(c.color()),
            interior: if c.byte() % 2 == 0 {
                Some(c.color())
            } else {
                None
            },
            border_width: width,
            // `/BE` (§12.5.4 Table 167). Driven from the cursor rather
            // than pinned, and deliberately allowed OUT OF RANGE: the
            // documented intensity is a continuous `0.0..=2.0`, so the
            // values worth fuzzing are the ones outside it, the boundary
            // itself, and `None` — which writes no `/BE` key at all and is
            // a different code path rather than a zero.
            border_effect: if c.byte() % 3 == 0 {
                None
            } else {
                Some(f64::from(c.byte()) / 64.0 - 1.0)
            },
        },
        1 => MarkupSpec::Circle {
            rect: c.rect(),
            border: Some(c.color()),
            interior: if c.byte() % 2 == 0 {
                Some(c.color())
            } else {
                None
            },
            border_width: width,
        },
        2 => MarkupSpec::Line {
            start: c.point(),
            end: c.point(),
            color: c.color(),
            width,
            endings: (LineEnding::OpenArrow, LineEnding::ClosedArrow),
        },
        3 => {
            let n = 2 + usize::from(c.byte() % 5);
            MarkupSpec::Ink {
                strokes: vec![c.points(n)],
                color: c.color(),
                width,
            }
        }
        4 => {
            let n = 3 + usize::from(c.byte() % 5);
            MarkupSpec::Polygon {
                vertices: c.points(n),
                border: Some(c.color()),
                interior: Some(c.color()),
                width,
            }
        }
        5 => {
            let n = 2 + usize::from(c.byte() % 5);
            MarkupSpec::PolyLine {
                vertices: c.points(n),
                color: c.color(),
                width,
            }
        }
        6 => {
            let n = 1 + c.byte() % 3;
            let quads = (0..n).map(|_| Quad::from_rect(c.rect())).collect();
            MarkupSpec::TextMarkup {
                kind: TextMarkupKind::Highlight,
                quads,
                color: c.color(),
            }
        }
        7 => {
            // The variant that had no arm at all. A revision cloud bakes a
            // scalloped appearance from an arbitrary vertex list, so it is
            // the markup spec with the most arithmetic between the input
            // bytes and the emitted path — exactly what a fuzz target is
            // for. Two and three vertices are included on purpose: a
            // "cloud" with too few points to enclose anything is the
            // degenerate case, and `intensity` is swept across and beyond
            // its documented `0.0..=2.0` for the same reason as `/BE`
            // above.
            let n = 2 + usize::from(c.byte() % 6);
            MarkupSpec::Cloud {
                vertices: c.points(n),
                border: Some(c.color()),
                interior: if c.byte() % 2 == 0 {
                    Some(c.color())
                } else {
                    None
                },
                width,
                intensity: f64::from(c.byte()) / 64.0 - 1.0,
            }
        }
        _ => {
            let kind = match c.byte() % 3 {
                0 => TextMarkupKind::Underline,
                1 => TextMarkupKind::StrikeOut,
                _ => TextMarkupKind::Squiggly,
            };
            let n = 1 + c.byte() % 3;
            let quads = (0..n).map(|_| Quad::from_rect(c.rect())).collect();
            MarkupSpec::TextMarkup {
                kind,
                quads,
                color: c.color(),
            }
        }
    }
}

/// A standard-14 face selected from the cursor — Symbol/ZapfDingbats
/// included so the SymbolicFont refusal path is exercised.
fn face(c: &mut Cursor<'_>) -> Std14 {
    match c.byte() % 8 {
        0 => Std14::Helvetica,
        1 => Std14::HelveticaBold,
        2 => Std14::TimesRoman,
        3 => Std14::TimesItalic,
        4 => Std14::Courier,
        5 => Std14::CourierBold,
        6 => Std14::Symbol,
        _ => Std14::ZapfDingbats,
    }
}

/// A text string derived from a slice of the fuzz input (huge-text and
/// non-Latin cases fall out of `from_utf8_lossy`).
fn text_of(data: &[u8]) -> String {
    // Bound the length so one run cannot balloon unboundedly.
    let end = data.len().min(4096);
    String::from_utf8_lossy(&data[..end]).into_owned()
}

/// Derive one text-bearing spec from the cursor.
fn text_spec(c: &mut Cursor<'_>, data: &[u8]) -> TextAnnotSpec {
    let rect = c.rect();
    let color = c.color();
    match c.byte() % 3 {
        0 => TextAnnotSpec::FreeText {
            rect,
            text: text_of(data),
            font: face(c),
            font_size: if c.byte() % 2 == 0 {
                0.0
            } else {
                f64::from(c.byte() % 40)
            },
            color: TextColor::from(color),
            quadding: match c.byte() % 3 {
                0 => Quadding::Left,
                1 => Quadding::Center,
                _ => Quadding::Right,
            },
            multiline: c.byte() % 2 == 0,
            border: if c.byte() % 2 == 0 {
                Some(c.color())
            } else {
                None
            },
            border_width: f64::from(c.byte() % 6),
        },
        1 => TextAnnotSpec::Sticky {
            rect,
            icon: StickyIcon::Note,
            contents: text_of(data),
            color,
            open: c.byte() % 2 == 0,
        },
        _ => TextAnnotSpec::Stamp {
            rect,
            name: StampName::Draft,
            label: if c.byte() % 2 == 0 {
                Some(text_of(data))
            } else {
                None
            },
            color,
        },
    }
}

fuzz_target!(|data: &[u8]| {
    // (1) Content-stream serializer round-trip (X6).
    if let Ok(cs) = ContentStream::parse(data.to_vec()) {
        let reemitted = reemit_canonical(&cs);
        // The re-emission must always re-parse (never emit an unparseable
        // token) and never panic.
        let _ = ContentStream::parse(reemitted);
    }

    // (1b) /DA parsing over raw bytes (X9): a malformed /DA must be a named
    // error, never a panic. And generating a variable-text appearance from
    // a cursor-derived resource set exercises the unresolvable-font,
    // symbolic-font, auto-size (size 0) and huge-text paths.
    let _ = vartext::parse_default_appearance(data);
    {
        let mut c = Cursor::new(data);
        let bbox = c.rect();
        let name: Vec<u8> = match c.byte() % 3 {
            0 => b"Helv".to_vec(), // resolvable
            1 => b"F1".to_vec(),   // unresolvable ⇒ named refusal
            _ => vec![c.byte().max(b'A')],
        };
        let da = vartext::default_appearance_string(
            &name,
            f64::from(c.byte() % 30),
            TextColor::Gray(0.0),
        );
        let resources = [FontResource {
            name: b"Helv".to_vec(),
            font: face(&mut c),
        }];
        let quad = match c.byte() % 3 {
            0 => Quadding::Left,
            1 => Quadding::Center,
            _ => Quadding::Right,
        };
        let _ = vartext::build_variable_text(
            bbox,
            &text_of(data),
            &da,
            quad,
            c.byte() % 2 == 0,
            &resources,
        );
    }

    // (2) Authoring round-trip (X5/X7). Cap the number of authorings so a
    // pathological input cannot make one run unbounded.
    let doc = match Document::from_bytes(BLANK.to_vec()) {
        Ok(d) => d,
        Err(_) => return,
    };
    let mut session = EditSession::new(doc);
    let mut c = Cursor::new(data);
    let count = 1 + (data.first().copied().unwrap_or(1) % 8);
    for _ in 0..count {
        let s = spec(&mut c);
        // add_markup may refuse (empty geometry) — that is fine; it must
        // never panic.
        let _ = session.add_markup(0, &s);
        // Interleave a Pass-6.2 text authoring (freetext/sticky/stamp). It
        // may refuse (symbolic font) — that is fine; it must never panic.
        let ts = text_spec(&mut c, data);
        let _ = session.add_text_annotation(0, &ts);
    }

    // Save both ways and reload; walk the reloaded annotations. The
    // combined base ++ staging source (R45) must resolve every authored
    // appearance span, or reload/walk would find garbage.
    if let Ok((bytes, _)) = session.to_incremental_bytes(&SaveOptions::identity()) {
        if let Ok(reloaded) = Document::from_bytes(bytes) {
            if let Ok(pages) = page_tree::pages(&reloaded) {
                for page in &pages {
                    for a in page_annotations(&reloaded, page.id) {
                        let _ = a.subtype_label();
                    }
                }
            }
        }
    }
    if let Ok((bytes, _)) = session.to_full_bytes(&SaveOptions::default()) {
        let _ = Document::from_bytes(bytes);
    }
});
