//! The OCR layer's content stream: words fitted to their boxes and written
//! invisible (ISO 32000-1 §9.3.6, Table 106, mode 3), grouped by
//! [`super::structure`] into text objects in reading order.

use crate::fontdata::{self, BaseEncoding, Std14};
use crate::object::{Name, ObjId, Object};
use crate::vartext::encode_winansi;
use crate::writer::content::{emit_literal_string, emit_number};

use super::OcrPage;
use super::layer::{
    HELVETICA_ASCENT_FRAC, HELVETICA_DESCENT_FRAC, MAX_TZ, MIN_TZ, OcrLayerOptions,
};
use super::layer_report::OcrLayerReport;
use super::structure::{ResolvedBlock, resolve};

/// One word, resolved to the numbers the content stream actually needs.
///
/// Split out from [`build_layer_content`] so the geometry is testable without
/// parsing emitted bytes — a test that asserts on `size`/`tz`/`baseline`
/// pinpoints a fit regression, where one that greps the stream for a substring
/// only says "something changed".
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PlacedWord {
    codes: Vec<u8>,
    size: f64,
    tz: f64,
    x: f64,
    baseline_y: f64,
    substituted: bool,
    clamped: bool,
}

/// Measure WinAnsi `bytes` in `font` at `size`, in text-space points.
///
/// §9.4.4: advance = Σ(width/1000) × size, before `Tc`/`Tw`/`Th`. This
/// deliberately measures at `Th = 1.0` because the whole point is to then
/// *solve* for the `Th` that makes the result equal the target width.
pub(super) fn natural_width(font: Std14, size: f64, bytes: &[u8]) -> f64 {
    let units: u32 = bytes
        .iter()
        .map(|&c| {
            u32::from(
                fontdata::encoding_glyph_name(BaseEncoding::WinAnsi, c)
                    .and_then(|name| fontdata::std14_width(font, name))
                    .unwrap_or(0),
            )
        })
        .sum();
    f64::from(units) / 1000.0 * size
}

/// Fit one recognised word to its box, or reject it as unplaceable.
///
/// Returns `None` for a word that cannot be positioned at all: empty text, a
/// non-finite or non-positive box, or text whose glyphs all have zero advance
/// (which would make the horizontal fit a division by zero). Each of those is
/// counted as a skip by the caller rather than being silently dropped.
pub(super) fn place_word(word: &super::RecognizedWord, font: Std14) -> Option<PlacedWord> {
    if word.text.is_empty() {
        return None;
    }
    let r = word.rect;
    let (w, h) = (r.urx - r.llx, r.ury - r.lly);
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 || !r.llx.is_finite() {
        return None;
    }

    let (codes, missing) = encode_winansi(&word.text);
    if codes.is_empty() {
        return None;
    }

    // Vertical fit: solve size so the glyph box (ascent+descent) equals the
    // reported box height, then sit the baseline a descender above its bottom.
    let size = h / (HELVETICA_ASCENT_FRAC + HELVETICA_DESCENT_FRAC);
    let baseline_y = HELVETICA_DESCENT_FRAC.mul_add(size, r.lly);

    // Horizontal fit: solve Tz so the natural advance equals the box width.
    let natural = natural_width(font, size, &codes);
    if !natural.is_finite() || natural <= 0.0 {
        return None;
    }
    let raw_tz = 100.0 * w / natural;
    let tz = raw_tz.clamp(MIN_TZ, MAX_TZ);

    Some(PlacedWord {
        codes,
        size,
        tz,
        x: r.llx,
        baseline_y,
        substituted: missing > 0,
        clamped: (tz - raw_tz).abs() > f64::EPSILON,
    })
}

/// Build the invisible text-layer content stream for one page.
///
/// Pure: no document, no allocation of object numbers, no I/O. Given the words
/// and the resource name the font will be filed under, it returns the exact
/// bytes and the counts that become the report. Kept pure so the emitted
/// stream can be asserted on directly, which is the only way to catch a
/// regression in something whose entire visible effect is *nothing*.
///
/// `font_name` is the `/Resources → /Font` key (without the leading slash),
/// chosen by the caller against the page's existing names so it cannot collide.
/// [`OcrLayerOptions::optional_content`] needs a `/Properties` name the page
/// binds, so this builder ignores it; the writers apply it.
#[must_use]
pub fn build_layer_content(
    page: &OcrPage,
    font_name: &[u8],
    opts: &OcrLayerOptions,
) -> (Vec<u8>, OcrLayerReport) {
    layer_content(page, font_name, None, opts)
}

/// [`build_layer_content`], with the text inside `/OC /oc_name BDC … EMC`
/// when `oc_name` is given.
pub(super) fn layer_content(
    page: &OcrPage,
    font_name: &[u8],
    oc_name: Option<&Name>,
    opts: &OcrLayerOptions,
) -> (Vec<u8>, OcrLayerReport) {
    let mut out: Vec<u8> = Vec::new();
    let (blocks, structure) = resolve(page);
    let mut report = OcrLayerReport {
        words_written: 0,
        words_skipped: 0,
        words_substituted: 0,
        words_scale_clamped: 0,
        lines_written: 0,
        blocks_written: 0,
        structure,
        mean_confidence: page.mean_confidence(),
        confidence_available: page.confidence_available,
        content_object: 0,
        font_object: 0,
        layers_replaced: 0,
    };

    let placed: Vec<Option<PlacedWord>> = page
        .words
        .iter()
        .map(|w| place_word(w, opts.font))
        .collect();
    report.words_skipped = placed.iter().filter(|p| p.is_none()).count();
    if report.words_skipped == placed.len() {
        return (out, report);
    }

    open_layer(&mut out, opts, oc_name);
    for block in &blocks {
        write_block(&mut out, block, &placed, font_name, &mut report);
    }
    out.extend_from_slice(b"Q\n");
    if oc_name.is_some() {
        out.extend_from_slice(b"EMC\n");
    }
    out.extend_from_slice(b"EMC\n");
    (out, report)
}

/// One block as one text object (`BT … ET`), its lines and their words in
/// order; nothing when none of its words could be placed.
fn write_block(
    out: &mut Vec<u8>,
    block: &ResolvedBlock,
    placed: &[Option<PlacedWord>],
    font_name: &[u8],
    report: &mut OcrLayerReport,
) {
    let mut opened = false;
    for line in &block.lines {
        let words: Vec<&PlacedWord> = line
            .iter()
            .filter_map(|&w| placed.get(w).and_then(Option::as_ref))
            .collect();
        if words.is_empty() {
            continue;
        }
        if !opened {
            out.extend_from_slice(b"BT\n");
            opened = true;
        }
        for p in words {
            write_word(out, p, font_name);
            report.words_written += 1;
            report.words_substituted += usize::from(p.substituted);
            report.words_scale_clamped += usize::from(p.clamped);
        }
        report.lines_written += 1;
    }
    if opened {
        out.extend_from_slice(b"ET\n");
        report.blocks_written += 1;
    }
}

/// `Tf`, `Tz`, `Tm` and `Tj` for one word. Emitted per word: two adjacent
/// words almost never share a size, and the layer is machine output.
fn write_word(out: &mut Vec<u8>, p: &PlacedWord, font_name: &[u8]) {
    out.push(b'/');
    out.extend_from_slice(font_name);
    out.push(b' ');
    emit_number(out, p.size);
    out.extend_from_slice(b" Tf\n");
    emit_number(out, p.tz);
    out.extend_from_slice(b" Tz\n");
    out.extend_from_slice(b"1 0 0 1 ");
    emit_number(out, p.x);
    out.push(b' ');
    emit_number(out, p.baseline_y);
    out.extend_from_slice(b" Tm\n");
    emit_literal_string(out, &p.codes);
    out.extend_from_slice(b" Tj\n");
}

/// The marker, the optional group section, then `q 3 Tr`.
///
/// `q` first: Tf/Tr/Tz are graphics state and must not leak into the
/// streams that follow this one in the /Contents array (§8.4.2); `Tr` is a
/// text-state operator, legal outside a text object and kept across each
/// block's `BT … ET` (§9.3.1). The marker (`super::marker`) is outermost, so
/// the whole stream is one marked-content sequence: BDC > (OC BDC >) q > BT,
/// properly nested (§14.6.1).
fn open_layer(out: &mut Vec<u8>, opts: &OcrLayerOptions, oc_name: Option<&Name>) {
    out.push(b'\n');
    super::marker::open_marker(out, opts.engine.as_deref());
    if let Some(name) = oc_name {
        out.extend_from_slice(b"/OC ");
        crate::writer::serialize::write_object(
            out,
            &Object::Name(name.clone()),
            ObjId::new(0, 0),
            &[],
            &crate::writer::IdentityEncoder,
        );
        out.extend_from_slice(b" BDC\n");
    }
    out.extend_from_slice(b"q\n3 Tr\n");
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests {
    use super::*;
    use crate::ocr::RecognizedWord;
    use crate::ocr::{OcrBlock, OcrBlockKind, OcrLine, OcrStructureSource};
    use crate::page_tree::Rect;

    fn word(text: &str, llx: f64, lly: f64, urx: f64, ury: f64) -> RecognizedWord {
        RecognizedWord {
            text: text.to_owned(),
            rect: Rect::from_corners(llx, lly, urx, ury),
            confidence: Some(0.9),
        }
    }

    fn page_of(words: Vec<RecognizedWord>) -> OcrPage {
        OcrPage {
            words,
            confidence_available: true,
            ..OcrPage::default()
        }
    }

    /// The one thing this module exists to guarantee: the text is INVISIBLE.
    ///
    /// `3 Tr` must be present, and must come before any `Tj`. Without it the
    /// layer renders as visible garbage across the scan — the exact failure the
    /// spec corpus warns renderers about, produced here at the writing end
    /// instead. Asserted on the emitted bytes because there is no visual
    /// symptom to catch it any other way.
    #[test]
    fn the_layer_sets_invisible_rendering_mode_before_showing_any_text() {
        let (bytes, _) = page_of(vec![word("HELLO", 10.0, 10.0, 60.0, 22.0)])
            .pipe_build(b"OCR0", &OcrLayerOptions::new());
        let s = String::from_utf8_lossy(&bytes).to_string();
        let tr = s.find("3 Tr").expect("mode 3 must be set");
        let tj = s.find(" Tj").expect("a word must be shown");
        assert!(tr < tj, "3 Tr must precede the first Tj, got {tr} vs {tj}");
    }

    /// The stream is balanced and isolated.
    ///
    /// `Tf`/`Tr`/`Tz` are graphics state, and a `/Contents` array concatenates.
    /// An unwrapped layer would leave `3 Tr` set and make every later stream's
    /// text invisible — a defect that would look like "the OCR broke my
    /// document's existing text", which is a much harder bug to trace back
    /// here than it is to prevent.
    #[test]
    fn the_stream_is_wrapped_so_invisible_mode_cannot_leak() {
        let (bytes, _) = page_of(vec![word("x", 0.0, 0.0, 10.0, 10.0)])
            .pipe_build(b"OCR0", &OcrLayerOptions::new());
        let s = String::from_utf8_lossy(&bytes).to_string();
        assert!(s.contains("q\n3 Tr\nBT\n"), "must open q, 3 Tr, then BT");
        assert!(
            s.trim_end().ends_with("ET\nQ\nEMC"),
            "must close ET, Q, then the marker's EMC: {s}"
        );
        assert_eq!(s.matches('q').count(), 1, "exactly one q");
        assert_eq!(s.matches('Q').count(), 1, "exactly one Q");
    }

    /// The vertical fit puts the glyph box top at the reported box top.
    ///
    /// Solved rather than approximated, so it is asserted exactly: baseline +
    /// ascent must equal the box's `ury`. A regression here shifts every
    /// selection highlight off the ink by a constant, which reads as "OCR is
    /// slightly wrong" and is very hard to attribute.
    #[test]
    fn the_glyph_box_top_lands_on_the_reported_box_top() {
        let p =
            place_word(&word("Ag", 10.0, 100.0, 60.0, 120.0), Std14::Helvetica).expect("placeable");
        let top = HELVETICA_ASCENT_FRAC.mul_add(p.size, p.baseline_y);
        assert!((top - 120.0).abs() < 1e-9, "glyph top {top} should be 120");
        assert!(
            p.baseline_y > 100.0,
            "the baseline sits a descender ABOVE the box bottom, got {}",
            p.baseline_y
        );
    }

    /// The horizontal fit makes the advance equal the reported box width.
    ///
    /// `Tz` is a PERCENTAGE (§9.3.4): `Th = Tz/100`. The corpus flags treating
    /// the operand as the ratio as a 100× error, so the check multiplies the
    /// natural width by `tz/100` and expects the box width back — a test that
    /// would fail loudly if the percentage/ratio confusion were ever
    /// introduced here.
    #[test]
    fn the_advance_is_scaled_to_the_reported_box_width() {
        let w = word("Invoice", 10.0, 100.0, 90.0, 112.0);
        let p = place_word(&w, Std14::Helvetica).expect("placeable");
        let natural = natural_width(Std14::Helvetica, p.size, &p.codes);
        let fitted = natural * p.tz / 100.0;
        assert!(
            (fitted - 80.0).abs() < 1e-6,
            "fitted advance {fitted} should equal the box width 80"
        );
    }

    /// A degenerate box is skipped and COUNTED, never silently dropped.
    ///
    /// Note what is NOT tested here: an inverted box built through
    /// [`Rect::from_corners`], because that constructor **normalises** its
    /// corners, so `(40,0)→(0,12)` arrives as a perfectly ordinary 40-wide
    /// rect. The first draft of this test asserted on exactly that case and
    /// failed — the test was wrong, not the guard. The genuinely inverted case
    /// needs a struct literal and gets its own test below.
    #[test]
    fn unplaceable_words_are_counted_as_skips() {
        let p = page_of(vec![
            word("good", 0.0, 0.0, 40.0, 12.0),
            word("", 0.0, 0.0, 40.0, 12.0),
            word("zero-height", 0.0, 0.0, 40.0, 0.0),
            word("zero-width", 40.0, 0.0, 40.0, 12.0),
        ]);
        let (_, report) = p.pipe_build(b"OCR0", &OcrLayerOptions::new());
        assert_eq!(report.words_written, 1);
        assert_eq!(report.words_skipped, 3, "each unplaceable word is counted");
    }

    /// An inverted or non-finite box — reachable only by building [`Rect`]
    /// through its public fields, which an engine adapter may well do — is
    /// rejected rather than producing a negative size and a `NaN` scaling.
    ///
    /// The guard is defensive by design: nothing in the crate can currently
    /// hand it such a rect, and that is precisely the state in which a guard
    /// quietly stops working and nobody notices.
    #[test]
    fn a_hand_built_inverted_or_nonfinite_box_is_rejected() {
        let inverted = RecognizedWord {
            text: "backwards".to_owned(),
            rect: Rect {
                llx: 40.0,
                lly: 12.0,
                urx: 0.0,
                ury: 0.0,
            },
            confidence: None,
        };
        assert!(place_word(&inverted, Std14::Helvetica).is_none());

        let nan = RecognizedWord {
            text: "nan".to_owned(),
            rect: Rect {
                llx: f64::NAN,
                lly: 0.0,
                urx: 40.0,
                ury: 12.0,
            },
            confidence: None,
        };
        assert!(place_word(&nan, Std14::Helvetica).is_none());
    }

    /// A non-WinAnsi character is substituted and DISCLOSED, not refused.
    ///
    /// This is the deliberate divergence from `add_text`'s R71 refusal, and the
    /// test pins both halves: the layer is still written (so one stray glyph
    /// cannot cost a page its text layer), AND the substitution is counted (so
    /// it is not silent). Either half alone would be the wrong behaviour.
    #[test]
    fn a_non_winansi_word_is_substituted_and_reported_not_refused() {
        let (bytes, report) = page_of(vec![word("日本語", 0.0, 0.0, 40.0, 12.0)])
            .pipe_build(b"OCR0", &OcrLayerOptions::new());
        assert_eq!(report.words_written, 1, "the layer is still written");
        assert_eq!(report.words_substituted, 1, "and the loss is disclosed");
        assert!(!bytes.is_empty());
        let msgs = report.disclosures().join(" ");
        assert!(
            msgs.contains("no WinAnsi code"),
            "the disclosure must name the cause: {msgs}"
        );
    }

    /// An engine with no confidence says so, rather than looking clean.
    ///
    /// The failure this prevents: an engine that reports nothing produces no
    /// low-confidence warnings and therefore reads as MORE trustworthy than one
    /// that reports honestly. The disclosure must state the absence.
    #[test]
    fn an_engine_without_confidence_discloses_the_absence() {
        let page = OcrPage {
            words: vec![RecognizedWord {
                text: "word".to_owned(),
                rect: Rect::from_corners(0.0, 0.0, 40.0, 12.0),
                confidence: None,
            }],
            confidence_available: false,
            ..OcrPage::default()
        };
        let (_, report) = page.pipe_build(b"OCR0", &OcrLayerOptions::new());
        assert_eq!(report.mean_confidence, None, "None, never zero");
        let msgs = report.disclosures().join(" ");
        assert!(
            msgs.contains("NO per-word confidence"),
            "absence must be stated as its own fact: {msgs}"
        );
    }

    /// A collapsed box clamps the scaling and reports it.
    #[test]
    fn an_absurd_box_clamps_the_scaling_and_says_so() {
        let (_, report) = page_of(vec![word("wide", 0.0, 0.0, 100_000.0, 4.0)])
            .pipe_build(b"OCR0", &OcrLayerOptions::new());
        assert_eq!(report.words_scale_clamped, 1);
        assert!(
            report.disclosures().join(" ").contains("clamped"),
            "clamping is disclosed"
        );
    }

    /// An empty page emits no bytes at all, rather than an empty `q…Q`.
    ///
    /// The caller turns this into `NothingToWrite`; emitting a stream and a
    /// font for zero words would grow the file and change its bytes to
    /// accomplish nothing.
    #[test]
    fn a_page_with_no_placeable_words_emits_nothing() {
        let (bytes, report) = page_of(vec![]).pipe_build(b"OCR0", &OcrLayerOptions::new());
        assert!(bytes.is_empty());
        assert_eq!(report.words_written, 0);
    }

    /// The font resource name the caller chose is the one emitted.
    #[test]
    fn the_supplied_font_resource_name_is_used() {
        let (bytes, _) = page_of(vec![word("x", 0.0, 0.0, 10.0, 10.0)])
            .pipe_build(b"pdfceOcr7", &OcrLayerOptions::new());
        assert!(String::from_utf8_lossy(&bytes).contains("/pdfceOcr7 "));
    }

    fn shown(bytes: &[u8]) -> Vec<String> {
        String::from_utf8_lossy(bytes)
            .lines()
            .filter_map(|l| l.strip_suffix(") Tj")?.strip_prefix('('))
            .map(str::to_owned)
            .collect()
    }

    fn structured(lines: Vec<OcrLine>, blocks: Vec<OcrBlock>) -> OcrPage {
        let mut p = page_of(vec![
            word("a", 50.0, 700.0, 80.0, 712.0),
            word("b", 90.0, 700.0, 120.0, 712.0),
            word("c", 50.0, 680.0, 80.0, 692.0),
            word("d", 50.0, 600.0, 80.0, 612.0),
        ]);
        p.lines = lines;
        p.blocks = blocks;
        p
    }

    /// Each reported block is one text object, in the Vec's order.
    #[test]
    fn reported_blocks_are_one_text_object_each_in_vec_order() {
        let page = structured(
            vec![
                OcrLine::new(vec![0, 1]),
                OcrLine::new(vec![2]),
                OcrLine::new(vec![3]),
            ],
            vec![
                OcrBlock::new(OcrBlockKind::Caption, vec![2]),
                OcrBlock::new(OcrBlockKind::Paragraph, vec![0, 1]),
            ],
        );
        let (bytes, report) = page.pipe_build(b"OCR0", &OcrLayerOptions::new());
        assert_eq!(shown(&bytes), ["d", "a", "b", "c"]);
        let s = String::from_utf8_lossy(&bytes);
        assert_eq!(s.matches("BT\n").count(), 2);
        assert_eq!(s.matches("ET\n").count(), 2);
        assert_eq!(report.structure, OcrStructureSource::Reported);
        assert_eq!((report.lines_written, report.blocks_written), (3, 2));
        assert!(!report.disclosures().join(" ").contains("inferred"));
    }

    /// Lines without blocks: the lines stay whole, the blocks are inferred.
    #[test]
    fn reported_lines_keep_their_words_and_blocks_are_inferred() {
        let page = structured(
            vec![
                OcrLine::new(vec![1, 0]),
                OcrLine::new(vec![2]),
                OcrLine::new(vec![3]),
            ],
            vec![],
        );
        let (bytes, report) = page.pipe_build(b"OCR0", &OcrLayerOptions::new());
        let order = shown(&bytes);
        let b = order.iter().position(|w| w == "b").unwrap();
        assert_eq!(order.get(b + 1).map(String::as_str), Some("a"));
        assert_eq!(report.structure, OcrStructureSource::BlocksInferred);
        assert_eq!(report.lines_written, 3);
        assert!(
            report
                .disclosures()
                .join(" ")
                .contains("blocks were inferred")
        );
    }

    /// Bad indices are ignored and an unnamed word still reaches the layer.
    #[test]
    fn bad_and_repeated_indices_lose_no_word_and_write_none_twice() {
        let page = structured(
            vec![OcrLine::new(vec![0, 99, 0]), OcrLine::new(vec![1])],
            vec![OcrBlock::new(OcrBlockKind::Paragraph, vec![0, 7, 1, 1])],
        );
        let (bytes, report) = page.pipe_build(b"OCR0", &OcrLayerOptions::new());
        assert_eq!(shown(&bytes), ["a", "b", "c", "d"]);
        assert_eq!(report.words_written, 4);
        assert_eq!(report.blocks_written, 2, "the leftovers form one block");
    }

    /// Words only, two columns, engine order row-major across them: the
    /// layer reads the left column to its end before the right.
    #[test]
    fn inferred_structure_reads_a_two_column_page_column_by_column() {
        let mut words = Vec::new();
        for row in 0..12 {
            let y = 700.0 - f64::from(row) * 14.0;
            words.push(word(&format!("L{row}"), 72.0, y, 260.0, y + 10.0));
            words.push(word(&format!("R{row}"), 330.0, y, 520.0, y + 10.0));
        }
        let (bytes, report) = page_of(words).pipe_build(b"OCR0", &OcrLayerOptions::new());
        let order = shown(&bytes);
        let last_left = order.iter().rposition(|w| w.starts_with('L')).unwrap();
        let first_right = order.iter().position(|w| w.starts_with('R')).unwrap();
        assert!(last_left < first_right, "column by column: {order:?}");
        assert_eq!(report.structure, OcrStructureSource::Inferred);
        assert_eq!(report.words_written, 24);
        assert!(report.lines_written >= 2);
        assert!(
            report
                .disclosures()
                .join(" ")
                .contains("lines and blocks were inferred"),
            "{:?}",
            report.disclosures()
        );
    }

    /// Words only, as a recogniser boxes them tight on the ink: 11 pt
    /// text on 16 pt leading, x-height words beside ascender and descender
    /// words, two columns of two paragraphs a blank line apart. Each
    /// paragraph is one block, not one per line.
    #[test]
    fn inferred_structure_groups_tight_word_boxes_into_paragraphs() {
        const EM: f64 = 11.0;
        // (text, ink bottom, ink top) in ems from the baseline.
        let shapes = [
            ("an", 0.0, 0.52),
            ("the", 0.0, 0.72),
            ("apple", -0.21, 0.72),
            ("go", -0.21, 0.52),
            ("In", 0.0, 0.72),
        ];
        let mut words = Vec::new();
        for (col, x0) in [(0, 72.0), (1, 330.0)] {
            let mut baseline = 700.0;
            for para in 0..2 {
                for line in 0..4 {
                    for k in 0..shapes.len() {
                        let Some(&(t, lo, hi)) = shapes.get((k + line) % shapes.len()) else {
                            continue;
                        };
                        let x = x0 + k as f64 * 36.0;
                        words.push(word(
                            &format!("{t}{col}{para}{line}"),
                            x,
                            baseline + lo * EM,
                            x + 30.0,
                            baseline + hi * EM,
                        ));
                    }
                    baseline -= 16.0;
                }
                baseline -= 16.0;
            }
        }
        let (_, report) = page_of(words).pipe_build(b"OCR0", &OcrLayerOptions::new());
        assert_eq!(report.structure, OcrStructureSource::Inferred);
        assert_eq!(report.words_written, 80);
        assert_eq!((report.lines_written, report.blocks_written), (16, 4));
    }

    /// Test-only sugar so each case reads as one line of intent.
    trait PipeBuild {
        fn pipe_build(&self, name: &[u8], opts: &OcrLayerOptions) -> (Vec<u8>, OcrLayerReport);
    }
    impl PipeBuild for OcrPage {
        fn pipe_build(&self, name: &[u8], opts: &OcrLayerOptions) -> (Vec<u8>, OcrLayerReport) {
            build_layer_content(self, name, opts)
        }
    }
}
