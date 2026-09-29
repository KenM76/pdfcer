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
//! a stream — is counted in `PasswordValueScan::unreadable_revisions` rather
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

    // ---- Pass 387.1: purge_password_values + the decomposing full save ----

    use crate::edit::EditSession;
    use crate::writer::SaveOptions;

    fn session(bytes: Vec<u8>) -> EditSession {
        EditSession::new(Document::from_bytes(bytes).unwrap())
    }

    fn full(s: &EditSession) -> Vec<u8> {
        s.to_full_bytes_decomposing_containers(&SaveOptions::identity())
            .unwrap()
            .0
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    /// `base`, with the widget's `/AP /N` (obj 5) drawing the value.
    fn base_with_value_appearance() -> Vec<u8> {
        let ap = "<< /Type /XObject /Subtype /Form /BBox [0 0 90 20] /Length 25 >>\n\
                  stream\nBT 2 5 Td (hunter2) Tj ET\nendstream";
        let text = String::from_utf8(base(Some("hunter2"))).unwrap();
        let text = text.replace("/V (hunter2) >>", "/V (hunter2) /AP << /N 5 0 R >> >>");
        let cut = text.find("xref\n").unwrap();
        let mut out = text.as_bytes()[..cut].to_vec();
        let off5 = out.len();
        out.extend_from_slice(format!("5 0 obj\n{ap}\nendobj\n").as_bytes());
        let doc_offsets: Vec<usize> = (1..=4)
            .map(|n| text.find(&format!("\n{n} 0 obj\n")).unwrap() + 1)
            .collect();
        let xref_at = out.len();
        out.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
        for off in doc_offsets.iter().chain(std::iter::once(&off5)) {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
        );
        out
    }

    fn be(v: u64, width: usize) -> Vec<u8> {
        v.to_be_bytes().get(8 - width..).unwrap_or(&[]).to_vec()
    }

    /// The field dict (obj 4) compressed in object stream 5, reached by an
    /// xref stream (obj 6); a §7.5.7 layout.
    fn base_in_object_stream() -> Vec<u8> {
        let field = "<< /Type /Annot /Subtype /Widget /FT /Tx /Ff 8192 /T (pin) \
                     /Rect [10 10 100 30] /P 3 0 R /V (hunter2) >>";
        let header = "4 0 ";
        let data = format!("{header}{field} ");
        let objstm = format!(
            "<< /Type /ObjStm /N 1 /First {} /Length {} >>\nstream\n{data}\nendstream",
            header.len(),
            data.len()
        );
        let file_objs: Vec<(u32, String)> = vec![
            (
                1,
                "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>".to_owned(),
            ),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>"
                    .to_owned(),
            ),
            (5, objstm),
        ];
        let mut buf = b"%PDF-1.5\n".to_vec();
        let mut offsets: Vec<(u32, usize)> = Vec::new();
        for (num, body) in &file_objs {
            offsets.push((*num, buf.len()));
            buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
        }
        let xref_at = buf.len();
        offsets.push((6, xref_at));
        let mut data = Vec::new();
        for num in 0..7u32 {
            let (t, f2, f3): (u64, u64, u64) = if num == 0 {
                (0, 0, 65535)
            } else if num == 4 {
                (2, 5, 0)
            } else {
                let off = offsets.iter().find(|(n, _)| *n == num).unwrap().1;
                (1, off as u64, 0)
            };
            data.extend(be(t, 1));
            data.extend(be(f2, 4));
            data.extend(be(f3, 2));
        }
        let dict = format!(
            "<< /Type /XRef /Size 7 /W [1 4 2] /Root 1 0 R /Length {} >>",
            data.len()
        );
        buf.extend_from_slice(format!("6 0 obj\n{dict}\nstream\n").as_bytes());
        buf.extend_from_slice(&data);
        buf.extend_from_slice(b"\nendstream\nendobj\n");
        buf.extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
        buf
    }

    #[test]
    fn purging_then_saving_full_leaves_no_value_in_the_bytes() {
        let mut s = session(base(Some("hunter2")));
        let out = s.purge_password_values().unwrap();
        assert_eq!(out.fields_purged, vec!["pin".to_owned()]);
        assert!(out.read_only_purged.is_empty());
        let bytes = full(&s);
        assert!(!contains(&bytes, b"hunter2"));
        let scan = scan_stored_password_values(&bytes, Document::from_bytes);
        assert_eq!(scan.revisions, 1);
        assert!(scan.stored.is_empty());
    }

    #[test]
    fn a_superseded_appearance_drawing_the_value_is_removed() {
        let mut s = session(base_with_value_appearance());
        let out = s.purge_password_values().unwrap();
        assert_eq!(out.appearance_objects_removed, 1);
        assert!(!contains(&full(&s), b"hunter2"));
    }

    #[test]
    fn a_value_compressed_in_an_object_stream_needs_the_decomposing_save() {
        let mut s = session(base_in_object_stream());
        s.purge_password_values().unwrap();
        let plain = s.to_full_bytes(&SaveOptions::identity()).unwrap().0;
        assert!(
            contains(&plain, b"hunter2"),
            "fixture premise: the stale container survives"
        );
        let (bytes, _, dec) = s
            .to_full_bytes_decomposing_containers(&SaveOptions::identity())
            .unwrap();
        assert_eq!(dec.containers, 1);
        assert!(!contains(&bytes, b"hunter2"));
        assert!(Document::from_bytes(bytes).is_ok());
    }

    #[test]
    fn a_full_rewrite_drops_the_value_an_earlier_revision_stored() {
        let file = append_update(base(Some("hunter2")), None);
        let s = session(file);
        let bytes = full(&s);
        let scan = scan_stored_password_values(&bytes, Document::from_bytes);
        assert!(scan.stored.is_empty());
        assert!(!contains(&bytes, b"hunter2"));
    }

    #[test]
    fn undo_restores_the_purged_value() {
        let mut s = session(base(Some("hunter2")));
        s.purge_password_values().unwrap();
        assert!(s.undo().is_some());
        assert!(contains(&full(&s), b"hunter2"));
    }

    #[test]
    fn a_read_only_password_field_is_purged_and_named() {
        let text = String::from_utf8(base(Some("hunter2")))
            .unwrap()
            .replace("/Ff 8192", "/Ff 8193");
        let mut s = session(text.into_bytes());
        let out = s.purge_password_values().unwrap();
        assert_eq!(out.fields_purged, vec!["pin".to_owned()]);
        assert_eq!(out.read_only_purged, vec!["pin".to_owned()]);
        assert!(!contains(&full(&s), b"hunter2"));
    }

    #[test]
    fn nothing_to_purge_commits_nothing() {
        let mut s = session(base(None));
        let out = s.purge_password_values().unwrap();
        assert!(!out.changed());
        assert!(s.undo().is_none());
    }

    #[test]
    fn revision_ends_take_the_line_ending_and_the_tail() {
        assert_eq!(revision_ends(b"a%%EOF\r\nb%%EOF\nzz"), vec![8, 17]);
        assert_eq!(revision_ends(b"no marker"), vec![9]);
        assert_eq!(revision_ends(b"x%%EOF"), vec![6]);
    }
}
