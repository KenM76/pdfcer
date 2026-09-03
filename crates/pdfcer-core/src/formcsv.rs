//! Form data as CSV — the format an office actually passes around.
//!
//! # Why this exists beside FDF and XFDF
//!
//! [`crate::fdf`] already round-trips a form's data in the two formats the
//! PDF world defines. Neither opens in a spreadsheet, and a spreadsheet is
//! where form data goes: to be sorted, checked, merged, and sent to somebody
//! who does not have a PDF tool. A format nobody outside the PDF world reads
//! is not an export path, it is an interchange format between PDF programs.
//!
//! # Shape
//!
//! Two columns, header row `name,value`, one row per field, in the document's
//! own field order:
//!
//! ```text
//! name,value
//! Item.1,100
//! Item.2,32.50
//! Total,132.50
//! ```
//!
//! Deliberately **not** one column per field with a single data row. That
//! wide shape is the right one for filling many copies of a form from a
//! spreadsheet, and it is a different feature with a different unit of work
//! (a batch across documents, not a document). Picking the tall shape here
//! keeps this a peer of FDF — one document's data — rather than half of an
//! unbuilt batch feature.
//!
//! # ★ The hazard: a CSV cell is not inert
//!
//! A spreadsheet treats a cell beginning `=`, `+`, `-` or `@` as a
//! **formula**. Form values come from a PDF that pdfcer did not write and
//! cannot vouch for, so a field holding `=1+1` becomes live arithmetic, and
//! the well-known hostile forms (`=cmd|…`, `=HYPERLINK(…)`, `=WEBSERVICE(…)`)
//! reach out of the spreadsheet entirely — one of them to the network, which
//! is a capability pdfcer refuses itself (R12) and would here be handing to
//! another program on the operator's behalf.
//!
//! pdfcer **neutralises** such a value by prefixing a single apostrophe, the
//! convention every spreadsheet honours as "this is text", and **counts and
//! discloses** every one it touched. Three properties matter and each is
//! tested:
//!
//! - The **PDF is unchanged.** Neutralising is a property of the CSV, not of
//!   the document; nothing is written back.
//! - The change is **visible**, not silent. An operator comparing the
//!   spreadsheet against the form would otherwise find a value that gained a
//!   character with no explanation.
//! - It is **reversible on import**: [`parse_csv`] strips a leading
//!   apostrophe it recognises as pdfcer's own, so a round trip through a
//!   spreadsheet returns the original value rather than accumulating
//!   apostrophes.
//!
//! Refusing to export instead would be the wrong trade: the operator would
//! lose a legitimate export because one field held an unlucky character, and
//! the value that triggered it is very often just a negative number.
//!
//! # RFC 4180
//!
//! Quoting follows RFC 4180: a field containing a comma, a double quote or a
//! line break is wrapped in double quotes and its own quotes are doubled.
//! Line endings are `\r\n`, which the RFC specifies and which is also what
//! Excel expects on Windows.

use crate::fdf::{FieldData, FormData};

/// The characters a spreadsheet reads as beginning a formula.
///
/// `-` is included even though a negative number is the commonest reason a
/// value starts with it. Excel resolves `-5` as a number and `-cmd|…` as a
/// formula, and the difference is not knowable from the first character —
/// which is precisely why the conservative rule is the right one, and why
/// the change is disclosed rather than silent.
pub const FORMULA_LEADS: [char; 4] = ['=', '+', '-', '@'];

/// The prefix that marks a value as literal text to a spreadsheet.
pub const TEXT_PREFIX: char = '\'';

/// Separator between the values of a multi-select field in one CSV cell.
///
/// The same character `pdfcer fill-field` uses (`--set Colours=Red|Blue`),
/// so the two surfaces spell a multi-selection identically instead of each
/// choosing its own.
pub const MULTI_SEPARATOR: &str = "|";

/// A CSV export, with what pdfcer had to change to make it safe to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvExport {
    /// The CSV bytes.
    pub csv: Vec<u8>,
    /// How many values were prefixed to stop a spreadsheet evaluating them.
    ///
    /// Non-zero means the CSV and the PDF differ by exactly those prefixes,
    /// which is a fact the operator needs before they compare the two and
    /// conclude pdfcer corrupted something.
    pub neutralised: usize,
    /// The names of the fields that were neutralised, so a disclosure can
    /// point at them rather than only count them.
    ///
    /// Capped at [`MAX_NAMED`]; the count above is always complete.
    pub neutralised_fields: Vec<String>,
}

/// How many neutralised field names a report lists before relying on the
/// count alone.
pub const MAX_NAMED: usize = 10;

/// Render form data as RFC 4180 CSV.
#[must_use]
pub fn to_csv(data: &FormData) -> CsvExport {
    let mut out = String::from("name,value\r\n");
    let mut neutralised = 0usize;
    let mut names = Vec::new();
    for field in &data.fields {
        // A multi-select carries several values and a CSV cell is one
        // string. `|` matches `fill-field --set NAME=Red|Blue`, so the two
        // surfaces spell a multi-selection the same way rather than each
        // inventing a separator.
        let value = field.values.join(MULTI_SEPARATOR);
        let (safe, changed) = neutralise(&value);
        if changed {
            neutralised += 1;
            if names.len() < MAX_NAMED {
                names.push(field.name.clone());
            }
        }
        out.push_str(&quote(&field.name));
        out.push(',');
        out.push_str(&quote(&safe));
        out.push_str("\r\n");
    }
    CsvExport {
        csv: out.into_bytes(),
        neutralised,
        neutralised_fields: names,
    }
}

/// Prefix a value a spreadsheet would evaluate, reporting whether it did.
fn neutralise(value: &str) -> (String, bool) {
    match value.chars().next() {
        Some(c) if FORMULA_LEADS.contains(&c) => {
            let mut s = String::with_capacity(value.len() + 1);
            s.push(TEXT_PREFIX);
            s.push_str(value);
            (s, true)
        }
        _ => (value.to_owned(), false),
    }
}

/// RFC 4180 quoting: wrap when the value contains a delimiter, a quote or a
/// line break, and double any embedded quote.
fn quote(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

/// Why a CSV could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CsvError {
    /// The file is empty, or has only a header.
    #[error("the CSV has no data rows")]
    NoRows,
    /// A row did not have the two columns the header promises.
    #[error("row {row}: expected 2 columns (name,value), found {found}")]
    WrongColumnCount {
        /// 1-based row number, counting the header as row 1.
        row: usize,
        /// How many columns the row actually had.
        found: usize,
    },
    /// A quoted field ran to the end of the file without closing.
    #[error("row {row}: a quoted value is never closed")]
    UnterminatedQuote {
        /// 1-based row number.
        row: usize,
    },
}

/// Parse `name,value` CSV back into form data.
///
/// A header row is accepted and skipped when its first cell is exactly
/// `name` (case-insensitively). A file without one is read as all data —
/// exporting and re-importing pdfcer's own output round-trips, and so does a
/// hand-written two-column file from someone who did not think to add a
/// header.
///
/// A leading apostrophe is stripped, reversing [`to_csv`]'s neutralisation.
/// That is why a round trip through a spreadsheet does not accumulate
/// apostrophes — and it is also why a value that genuinely begins with one
/// cannot survive the round trip. That trade is deliberate: a form value
/// starting with an apostrophe is rare, and silently importing `'=1+1` as a
/// literal would re-arm the hazard on the next export.
///
/// # Errors
///
/// [`CsvError`] for an empty file, a malformed quote, or a row without
/// exactly two columns. A row count mismatch is an error rather than a
/// best-effort skip: a spreadsheet that lost a column would otherwise import
/// as names with no values, silently blanking a form.
pub fn parse_csv(bytes: &[u8]) -> Result<FormData, CsvError> {
    let text = String::from_utf8_lossy(bytes);
    // A UTF-8 BOM is what Excel writes; strip it rather than making it part
    // of the first field name, which would make every such file import zero
    // matching fields and look like pdfcer could not read it.
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);

    let rows = split_rows(text)?;
    let mut fields = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        // Destructured rather than indexed: the length check below already
        // guarantees two cells, but a total accessor keeps the parser
        // panic-free by inspection, and a CSV is untrusted input.
        let [name, raw] = row.as_slice() else {
            // A wholly blank trailing line is not a row; every spreadsheet
            // writes one and it is not an error.
            if row.iter().all(|c| c.trim().is_empty()) {
                continue;
            }
            return Err(CsvError::WrongColumnCount {
                row: index + 1,
                found: row.len(),
            });
        };
        if index == 0 && name.eq_ignore_ascii_case("name") {
            continue;
        }
        let value = raw.strip_prefix(TEXT_PREFIX).unwrap_or(raw);
        fields.push(FieldData {
            name: name.clone(),
            values: value.split(MULTI_SEPARATOR).map(str::to_owned).collect(),
            rich_value: None,
        });
    }
    if fields.is_empty() {
        return Err(CsvError::NoRows);
    }
    Ok(FormData { fields })
}

/// Split RFC 4180 text into rows of cells.
fn split_rows(text: &str) -> Result<Vec<Vec<String>>, CsvError> {
    let mut rows = Vec::new();
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                // A doubled quote inside a quoted field is one quote.
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => quoted = true,
            ',' => cells.push(std::mem::take(&mut cell)),
            '\r' => {
                // `\r\n` is one terminator; a lone `\r` is treated as one too,
                // because a file that reached pdfcer through an old tool is
                // still a file the operator wants read.
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                cells.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut cells));
            }
            '\n' => {
                cells.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut cells));
            }
            _ => cell.push(c),
        }
    }
    if quoted {
        return Err(CsvError::UnterminatedQuote {
            row: rows.len() + 1,
        });
    }
    if !cell.is_empty() || !cells.is_empty() {
        cells.push(cell);
        rows.push(cells);
    }
    if rows.is_empty() {
        return Err(CsvError::NoRows);
    }
    Ok(rows)
}

impl CsvExport {
    /// The operator-facing note, or `None` when nothing was changed.
    #[must_use]
    pub fn message(&self) -> Option<String> {
        if self.neutralised == 0 {
            return None;
        }
        let named = if self.neutralised_fields.is_empty() {
            String::new()
        } else {
            format!(" ({})", self.neutralised_fields.join(", "))
        };
        let more = if self.neutralised > self.neutralised_fields.len() {
            format!(
                " and {} more",
                self.neutralised - self.neutralised_fields.len()
            )
        } else {
            String::new()
        };
        Some(format!(
            "{} value(s){named}{more} begin with a character a spreadsheet reads as a \
             FORMULA, so pdfcer prefixed each with an apostrophe to keep it text. The PDF \
             is unchanged, and importing this CSV back removes the prefix.",
            self.neutralised
        ))
    }
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

    fn data(pairs: &[(&str, &str)]) -> FormData {
        FormData {
            fields: pairs
                .iter()
                .map(|(n, v)| FieldData {
                    name: (*n).to_owned(),
                    values: vec![(*v).to_owned()],
                    rich_value: None,
                })
                .collect(),
        }
    }

    fn text_of(export: &CsvExport) -> String {
        String::from_utf8(export.csv.clone()).expect("utf-8")
    }

    /// The ordinary case: header, one row per field, document order kept.
    #[test]
    fn ordinary_data_round_trips_through_csv() {
        let original = data(&[("Item.1", "100"), ("Item.2", "32.50"), ("Total", "132.50")]);
        let export = to_csv(&original);
        assert_eq!(
            text_of(&export),
            "name,value\r\nItem.1,100\r\nItem.2,32.50\r\nTotal,132.50\r\n"
        );
        assert_eq!(export.neutralised, 0);
        assert_eq!(export.message(), None, "nothing to say");

        let back = parse_csv(&export.csv).expect("parses");
        assert_eq!(back.fields.len(), 3);
        assert_eq!(back.fields[0].name, "Item.1");
        assert_eq!(back.fields[2].values.join("|"), "132.50");
    }

    /// ★ **A value a spreadsheet would evaluate is neutralised, counted and
    /// named — and the round trip removes the prefix again.**
    ///
    /// The hostile forms reach outside the spreadsheet: `=cmd|…` launches,
    /// `=WEBSERVICE(…)` fetches. pdfcer refuses network access itself (R12);
    /// exporting a cell that hands it to another program on the operator's
    /// behalf would be the same capability by a longer route.
    #[test]
    fn a_formula_looking_value_is_neutralised_and_disclosed() {
        let export = to_csv(&data(&[
            ("Safe", "hello"),
            ("Formula", "=1+1"),
            ("Hostile", "=cmd|'/c calc'!A1"),
            ("Negative", "-5"),
        ]));
        assert_eq!(export.neutralised, 3, "= = and - all lead a formula");
        assert!(text_of(&export).contains("'=1+1"));
        assert!(text_of(&export).contains("'-5"));

        let m = export.message().expect("discloses");
        assert!(m.contains("FORMULA"), "{m}");
        assert!(m.contains("Formula"), "and names the fields: {m}");
        assert!(m.contains("PDF is unchanged"), "{m}");

        // Reversible: importing strips the prefix rather than keeping it.
        let back = parse_csv(&export.csv).expect("parses");
        assert_eq!(back.fields[1].values.join("|"), "=1+1");
        assert_eq!(back.fields[3].values.join("|"), "-5");
    }

    /// A second round trip does not accumulate apostrophes.
    #[test]
    fn a_second_round_trip_does_not_stack_prefixes() {
        let once = to_csv(&data(&[("F", "=1+1")]));
        let back = parse_csv(&once.csv).expect("parses");
        let twice = to_csv(&back);
        assert_eq!(text_of(&once), text_of(&twice), "stable under round trip");
    }

    /// RFC 4180 quoting for the three characters that need it.
    #[test]
    fn commas_quotes_and_newlines_are_quoted_per_rfc_4180() {
        let export = to_csv(&data(&[
            ("Comma", "a,b"),
            ("Quote", "say \"hi\""),
            ("Newline", "one\ntwo"),
        ]));
        let t = text_of(&export);
        assert!(t.contains("Comma,\"a,b\""), "{t}");
        assert!(t.contains("Quote,\"say \"\"hi\"\"\""), "{t}");
        assert!(t.contains("Newline,\"one\ntwo\""), "{t}");

        let back = parse_csv(&export.csv).expect("parses");
        assert_eq!(back.fields[0].values.join("|"), "a,b");
        assert_eq!(back.fields[1].values.join("|"), "say \"hi\"");
        assert_eq!(back.fields[2].values.join("|"), "one\ntwo");
    }

    /// Excel's BOM is stripped rather than becoming part of the first name.
    #[test]
    fn a_utf8_bom_does_not_become_part_of_the_first_field_name() {
        let mut bytes = "\u{feff}".as_bytes().to_vec();
        bytes.extend_from_slice(b"name,value\r\nA,1\r\n");
        let parsed = parse_csv(&bytes).expect("parses");
        assert_eq!(parsed.fields[0].name, "A", "not \\u{{feff}}A");
    }

    /// A header is optional — a hand-written two-column file imports too.
    #[test]
    fn a_file_without_a_header_is_read_as_data() {
        let parsed = parse_csv(b"A,1\r\nB,2\r\n").expect("parses");
        assert_eq!(parsed.fields.len(), 2);
        assert_eq!(parsed.fields[0].name, "A");
    }

    /// ★ **A row missing a column is an ERROR, not a best-effort skip.**
    ///
    /// A spreadsheet that lost its value column would otherwise import as
    /// names with empty values and silently blank the whole form — a
    /// destructive outcome from a file that merely looked wrong.
    #[test]
    fn a_row_with_the_wrong_column_count_refuses() {
        let err = parse_csv(b"name,value\r\nA,1\r\nB\r\n").expect_err("must refuse");
        assert_eq!(err, CsvError::WrongColumnCount { row: 3, found: 1 });
        assert!(err.to_string().contains("row 3"), "{err}");
    }

    /// An unterminated quote refuses rather than silently swallowing the
    /// rest of the file into one cell.
    #[test]
    fn an_unterminated_quote_refuses() {
        let err = parse_csv(b"name,value\r\nA,\"oops\r\n").expect_err("must refuse");
        assert!(matches!(err, CsvError::UnterminatedQuote { .. }));
    }

    /// An empty file, and a header with no rows, both refuse — importing
    /// nothing over a filled form would clear it.
    #[test]
    fn an_empty_import_refuses_rather_than_clearing_the_form() {
        assert!(parse_csv(b"").is_err());
        assert_eq!(parse_csv(b"name,value\r\n"), Err(CsvError::NoRows));
    }

    /// A blank trailing line — which every spreadsheet writes — is not a row.
    #[test]
    fn a_trailing_blank_line_is_not_a_row() {
        let parsed = parse_csv(b"name,value\r\nA,1\r\n\r\n").expect("parses");
        assert_eq!(parsed.fields.len(), 1);
    }

    /// Many neutralised fields are counted in full and named up to a cap.
    #[test]
    fn many_neutralised_fields_are_capped_but_counted_in_full() {
        let pairs: Vec<(String, String)> = (0..15)
            .map(|i| (format!("F{i}"), "=1".to_owned()))
            .collect();
        let borrowed: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(n, v)| (n.as_str(), v.as_str()))
            .collect();
        let export = to_csv(&data(&borrowed));
        assert_eq!(export.neutralised, 15);
        assert_eq!(export.neutralised_fields.len(), MAX_NAMED);
        assert!(
            export.message().expect("discloses").contains("and 5 more"),
            "the tail is summarised, not dropped"
        );
    }
}
