//! Reader for Tesseract's TSV output (`tesseract <image> stdout tsv`).
//!
//! Pure text parsing, no process spawning: running `tesseract.exe` belongs to
//! the shell, which keeps `pdfcer-core` free of subprocesses and compiling for
//! `wasm32`. A shell spawns Tesseract, captures stdout, and hands it here.
//!
//! # Format
//!
//! Tab-separated, one header row, then twelve columns per row:
//! `level page_num block_num par_num line_num word_num left top width height
//! conf text`. Levels 1–4 are page/block/paragraph/line rows; **level 5 is a
//! word**, and only those become [`RecognizedWord`]s. `left`/`top`/`width`/
//! `height` are image pixels, y-down. `conf` is `0..=100` for a word
//! (Tesseract writes `-1` on structural rows).
//!
//! Output order is Tesseract's reading order, which is preserved.

use crate::ocr::RecognizedWord;
use crate::page_tree::Rect;

/// Columns in a Tesseract TSV row.
const COLUMNS: usize = 12;

/// The TSV `level` of a word row.
const WORD_LEVEL: &str = "5";

/// Why a TSV document was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TsvError {
    /// The first line is not Tesseract's TSV header.
    #[error("not Tesseract TSV output: first line is not the `level\\tpage_num...` header")]
    MissingHeader,
    /// A word row is malformed.
    #[error("Tesseract TSV line {line}: {reason}")]
    BadRow {
        /// 1-based line number in the input.
        line: usize,
        /// What was wrong with it.
        reason: &'static str,
    },
}

/// Parse Tesseract TSV into words in image pixel coordinates, y-down.
///
/// Word rows whose text is empty or whitespace are skipped (Tesseract emits
/// them for detected-but-unread regions). Confidence is `conf / 100`, clamped
/// to `0.0..=1.0`; a negative `conf` on a word row becomes `None`, so a
/// missing score is never reported as a zero one.
///
/// Rows at levels 1–4 are only checked for having enough columns to read
/// their level; everything else about them is ignored.
///
/// # Errors
///
/// [`TsvError::MissingHeader`] when the input does not start with the TSV
/// header; [`TsvError::BadRow`] when a word row has too few columns or a
/// non-numeric geometry or confidence field.
///
/// # Examples
///
/// ```
/// use pdfcer_core::ocr::tesseract_tsv::parse_tsv;
///
/// let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
///            5\t1\t1\t1\t1\t1\t10\t20\t30\t12\t96.5\tHello\n";
/// let words = parse_tsv(tsv)?;
/// assert_eq!(words[0].text, "Hello");
/// assert_eq!(words[0].confidence, Some(0.965));
/// # Ok::<(), pdfcer_core::ocr::tesseract_tsv::TsvError>(())
/// ```
pub fn parse_tsv(tsv: &str) -> Result<Vec<RecognizedWord>, TsvError> {
    let mut lines = tsv.lines().enumerate();
    match lines.next() {
        Some((_, header)) if header.starts_with("level\tpage_num") => {}
        _ => return Err(TsvError::MissingHeader),
    }

    let mut words = Vec::new();
    for (index, raw) in lines {
        let line = index + 1;
        if raw.is_empty() {
            continue;
        }
        // `splitn` so a word's text may itself contain a tab without being cut.
        let fields: Vec<&str> = raw.splitn(COLUMNS, '\t').collect();
        if fields.first() != Some(&WORD_LEVEL) {
            continue;
        }
        let [_, _, _, _, _, _, left, top, width, height, conf, text] = fields.as_slice() else {
            return Err(TsvError::BadRow {
                line,
                reason: "word row has fewer than 12 columns",
            });
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let num = |field: &str| -> Result<f64, TsvError> {
            field
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or(TsvError::BadRow {
                    line,
                    reason: "non-numeric geometry or confidence field",
                })
        };
        let (left, top, width, height) = (num(left)?, num(top)?, num(width)?, num(height)?);
        if width < 0.0 || height < 0.0 {
            return Err(TsvError::BadRow {
                line,
                reason: "negative width or height",
            });
        }
        if !(left + width).is_finite() || !(top + height).is_finite() {
            return Err(TsvError::BadRow {
                line,
                reason: "geometry out of range",
            });
        }
        let conf = num(conf)?;
        #[allow(clippy::cast_possible_truncation)] // 0..=1 fits f32 exactly enough for a score
        let confidence = (conf >= 0.0).then(|| (conf / 100.0).clamp(0.0, 1.0) as f32);
        words.push(RecognizedWord {
            text: text.to_owned(),
            rect: Rect::from_corners(left, top, left + width, top + height),
            confidence,
        });
    }
    Ok(words)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    const HEADER: &str = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext";

    fn doc(rows: &[&str]) -> String {
        let mut s = String::from(HEADER);
        for r in rows {
            s.push('\n');
            s.push_str(r);
        }
        s.push('\n');
        s
    }

    #[test]
    fn words_only_in_reading_order_structural_rows_ignored() {
        let tsv = doc(&[
            "1\t1\t0\t0\t0\t0\t0\t0\t1000\t800\t-1\t",
            "4\t1\t1\t1\t1\t0\t10\t20\t200\t12\t-1\t",
            "5\t1\t1\t1\t1\t1\t10\t20\t50\t12\t91\tfirst",
            "5\t1\t1\t1\t1\t2\t70\t20\t60\t12\t88.25\tsecond",
        ]);
        let words = parse_tsv(&tsv).unwrap();
        assert_eq!(
            words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert_eq!(words[1].rect, Rect::from_corners(70.0, 20.0, 130.0, 32.0));
        assert_eq!(words[1].confidence, Some(0.8825));
    }

    #[test]
    fn blank_word_rows_are_skipped() {
        let tsv = doc(&[
            "5\t1\t1\t1\t1\t1\t0\t0\t5\t5\t95\t  ",
            "5\t1\t1\t1\t1\t2\t0\t0\t5\t5\t95\tx",
        ]);
        assert_eq!(parse_tsv(&tsv).unwrap().len(), 1);
    }

    #[test]
    fn negative_word_conf_is_none_not_zero() {
        let tsv = doc(&["5\t1\t1\t1\t1\t1\t0\t0\t5\t5\t-1\tx"]);
        assert_eq!(parse_tsv(&tsv).unwrap()[0].confidence, None);
    }

    #[test]
    fn out_of_range_conf_is_clamped() {
        let tsv = doc(&["5\t1\t1\t1\t1\t1\t0\t0\t5\t5\t140\tx"]);
        assert_eq!(parse_tsv(&tsv).unwrap()[0].confidence, Some(1.0));
    }

    #[test]
    fn tab_inside_text_is_kept() {
        let tsv = doc(&["5\t1\t1\t1\t1\t1\t0\t0\t5\t5\t90\ta\tb"]);
        assert_eq!(parse_tsv(&tsv).unwrap()[0].text, "a\tb");
    }

    #[test]
    fn crlf_line_endings_parse() {
        let tsv = doc(&["5\t1\t1\t1\t1\t1\t0\t0\t5\t5\t90\tword"]).replace('\n', "\r\n");
        assert_eq!(parse_tsv(&tsv).unwrap()[0].text, "word");
    }

    #[test]
    fn missing_header_is_refused() {
        assert_eq!(
            parse_tsv("5\t1\t1\t1\t1\t1\t0\t0\t5\t5\t90\tx"),
            Err(TsvError::MissingHeader)
        );
        assert_eq!(parse_tsv(""), Err(TsvError::MissingHeader));
    }

    #[test]
    fn malformed_word_rows_are_refused_with_line_number() {
        let short = doc(&["5\t1\t1"]);
        assert!(matches!(
            parse_tsv(&short),
            Err(TsvError::BadRow { line: 2, .. })
        ));
        let nan = doc(&["5\t1\t1\t1\t1\t1\tNaN\t0\t5\t5\t90\tx"]);
        assert!(matches!(
            parse_tsv(&nan),
            Err(TsvError::BadRow { line: 2, .. })
        ));
        let neg = doc(&["5\t1\t1\t1\t1\t1\t0\t0\t-5\t5\t90\tx"]);
        assert!(matches!(
            parse_tsv(&neg),
            Err(TsvError::BadRow { line: 2, .. })
        ));
    }
}
