//! Password-field values left in a file's revisions.
//!
//! ISO 32000-1 §12.7.4.3 Table 228 bit 14 (`Password`): a reader "should never
//! store the value of the text field in the PDF file". pdfcer withholds `/V`
//! when it fills such a field, but an incremental update (§7.5.6) appends and
//! never removes, so a value stored by an earlier revision stays in the bytes.
//! This scan finds those values; it names the field and the revision, never the
//! value.
//!
//! A revision is the file up to and including one `%%EOF` marker (§7.5.6: each
//! update ends with its own trailer and `%%EOF`). A prefix that does not open
//! as a document — a linearized file's first-page section, or a marker inside
//! a stream — is counted in [`PasswordValueScan::unreadable_revisions`] rather
//! than guessed at.

use crate::document::{DocError, Document};
use crate::forms::{self, FieldFlags, FieldType, FieldValue};

/// One password field holding a stored `/V` in one revision.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StoredPasswordValue {
    /// The field's fully qualified name in that revision.
    pub field: String,
    /// Zero-based revision index; the last revision is the file as a reader
    /// opens it.
    pub revision: usize,
}

/// The result of [`scan_stored_password_values`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PasswordValueScan {
    /// How many revisions the file has (`%%EOF`-delimited, at least 1).
    pub revisions: usize,
    /// Revisions whose prefix did not open; their fields are unknown.
    pub unreadable_revisions: usize,
    /// Every stored value found, in revision order.
    pub stored: Vec<StoredPasswordValue>,
}

impl PasswordValueScan {
    /// Values stored in the revision a reader opens. A full rewrite that
    /// removes them is needed for the file to hold no value at all.
    pub fn in_latest(&self) -> impl Iterator<Item = &StoredPasswordValue> {
        let last = self.revisions.saturating_sub(1);
        self.stored.iter().filter(move |s| s.revision == last)
    }

    /// Values in superseded revisions: invisible to a reader, still in the
    /// bytes, and removable only by a full rewrite.
    pub fn in_superseded(&self) -> impl Iterator<Item = &StoredPasswordValue> {
        let last = self.revisions.saturating_sub(1);
        self.stored.iter().filter(move |s| s.revision < last)
    }
}

/// The end offset of each revision: one past each `%%EOF` marker and its
/// line ending. The last entry is always `bytes.len()`, so trailing bytes
/// after the final marker belong to the last revision.
#[must_use]
pub fn revision_ends(bytes: &[u8]) -> Vec<usize> {
    const MARKER: &[u8] = b"%%EOF";
    let mut ends = Vec::new();
    let mut at = 0;
    while let Some(rest) = bytes.get(at..) {
        let Some(pos) = rest.windows(MARKER.len()).position(|w| w == MARKER) else {
            break;
        };
        let mut end = at + pos + MARKER.len();
        match (bytes.get(end), bytes.get(end + 1)) {
            (Some(b'\r'), Some(b'\n')) => end += 2,
            (Some(b'\r' | b'\n'), _) => end += 1,
            _ => {}
        }
        ends.push(end);
        at = end;
    }
    match ends.last_mut() {
        Some(last) => *last = bytes.len(),
        None => ends.push(bytes.len()),
    }
    ends
}

/// List every Password text field that stores a `/V` value, in every revision.
///
/// `open` parses one revision's bytes; pass the same password and load
/// options used to open the whole file, so an encrypted file's revisions
/// decrypt. The scan is read-only and never reports the value itself.
///
/// # Example
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::password_history::scan_stored_password_values;
///
/// let bytes = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n\
///     trailer\n<< /Root 1 0 R >>\n%%EOF\n";
/// let scan = scan_stored_password_values(bytes, Document::from_bytes);
/// assert_eq!(scan.revisions, 1);
/// assert!(scan.stored.is_empty());
/// ```
pub fn scan_stored_password_values<F>(bytes: &[u8], mut open: F) -> PasswordValueScan
where
    F: FnMut(Vec<u8>) -> Result<Document, DocError>,
{
    let ends = revision_ends(bytes);
    let mut scan = PasswordValueScan {
        revisions: ends.len(),
        ..PasswordValueScan::default()
    };
    for (revision, &end) in ends.iter().enumerate() {
        let Some(prefix) = bytes.get(..end) else {
            scan.unreadable_revisions += 1;
            continue;
        };
        let Ok(doc) = open(prefix.to_vec()) else {
            scan.unreadable_revisions += 1;
            continue;
        };
        let Some(form) = forms::parse_acroform(&doc) else {
            continue;
        };
        for field in &form.fields {
            let stored = field.field_type == Some(FieldType::Text)
                && field.flags.has(FieldFlags::PASSWORD)
                && matches!(&field.value, FieldValue::Text(t) if !t.is_empty());
            if stored {
                scan.stored.push(StoredPasswordValue {
                    field: field.fully_qualified_name.clone(),
                    revision,
                });
            }
        }
    }
    scan
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

    /// A one-page form with a Password text field whose `/V` is `value`
    /// (`None` = no `/V`).
    fn base(value: Option<&str>) -> Vec<u8> {
        let v = value.map_or(String::new(), |v| format!(" /V ({v})"));
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>".to_owned(),
            format!(
                "<< /Type /Annot /Subtype /Widget /FT /Tx /Ff 8192 /T (pin) \
                 /Rect [10 10 100 30] /P 3 0 R{v} >>"
            ),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offs = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offs.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref_at = out.len();
        out.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for off in offs {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
        );
        out
    }

    /// Append an incremental update redefining object 4 with `/V` = `value`.
    fn append_update(mut file: Vec<u8>, value: Option<&str>) -> Vec<u8> {
        let doc = Document::from_bytes(file.clone()).unwrap();
        let prev = doc.base_startxref();
        let v = value.map_or(String::new(), |v| format!(" /V ({v})"));
        let off = file.len();
        file.extend_from_slice(
            format!(
                "4 0 obj\n<< /Type /Annot /Subtype /Widget /FT /Tx /Ff 8192 /T (pin) \
                 /Rect [10 10 100 30] /P 3 0 R{v} >>\nendobj\n"
            )
            .as_bytes(),
        );
        let xref_at = file.len();
        file.extend_from_slice(
            format!(
                "xref\n0 1\n0000000000 65535 f \n4 1\n{off:010} 00000 n \n\
                 trailer\n<< /Size 5 /Root 1 0 R /Prev {prev} >>\nstartxref\n{xref_at}\n%%EOF\n"
            )
            .as_bytes(),
        );
        file
    }

    #[test]
    fn a_value_stored_by_an_earlier_revision_is_found_after_it_is_withheld() {
        let file = append_update(base(Some("hunter2")), None);
        let scan = scan_stored_password_values(&file, Document::from_bytes);
        assert_eq!(scan.revisions, 2);
        assert_eq!(scan.unreadable_revisions, 0);
        assert_eq!(
            scan.stored,
            vec![StoredPasswordValue {
                field: "pin".to_owned(),
                revision: 0
            }]
        );
        assert_eq!(scan.in_superseded().count(), 1);
        assert_eq!(scan.in_latest().count(), 0);
    }

    #[test]
    fn a_value_in_the_latest_revision_is_reported_as_latest() {
        let scan = scan_stored_password_values(&base(Some("hunter2")), Document::from_bytes);
        assert_eq!(scan.revisions, 1);
        assert_eq!(scan.in_latest().count(), 1);
        assert_eq!(scan.in_superseded().count(), 0);
    }

    #[test]
    fn a_clean_file_reports_nothing() {
        let file = append_update(base(None), None);
        let scan = scan_stored_password_values(&file, Document::from_bytes);
        assert_eq!(scan.revisions, 2);
        assert!(scan.stored.is_empty());
    }

    #[test]
    fn a_plain_text_field_value_is_not_a_password_value() {
        let text = String::from_utf8(base(Some("hunter2")))
            .unwrap()
            .replace("/Ff 8192", "/Ff 0");
        let scan = scan_stored_password_values(text.as_bytes(), Document::from_bytes);
        assert!(scan.stored.is_empty());
    }

    #[test]
    fn a_revision_that_does_not_open_is_counted_not_guessed() {
        let mut file = b"%PDF-1.4\n%%EOF\n".to_vec();
        file.extend_from_slice(&base(Some("hunter2"))[9..]);
        let scan = scan_stored_password_values(&file, Document::from_bytes);
        assert_eq!(scan.revisions, 2);
        assert_eq!(scan.unreadable_revisions, 1);
    }

    #[test]
    fn revision_ends_take_the_line_ending_and_the_tail() {
        assert_eq!(revision_ends(b"a%%EOF\r\nb%%EOF\nzz"), vec![8, 17]);
        assert_eq!(revision_ends(b"no marker"), vec![9]);
        assert_eq!(revision_ends(b"x%%EOF"), vec![6]);
    }
}
