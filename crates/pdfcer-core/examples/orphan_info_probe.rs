//! Reproduction probe: does an orphaned `/Info`-shaped object survive a redaction?
//!
//! Build a PDF whose trailer `/Info` points at object 5, while object 6 is a
//! SECOND document-information-shaped dictionary the trailer does not name but
//! the cross-reference table does. Both carry the word being redacted.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::writer::SaveOptions;

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn build() -> Vec<u8> {
    let objects: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_string()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R \
             /Resources << /Font << /F1 7 0 R >> >> >>"
                .to_string(),
        ),
        (4, {
            let stream = "BT /F1 24 Tf 40 200 Td (CONFIDENTIAL) Tj ET";
            format!(
                "<< /Length {} >>\nstream\n{stream}\nendstream",
                stream.len()
            )
        }),
        // The trailer's /Info -- the one carrier_info scrubs.
        (
            5,
            "<< /Title (Live info) /Keywords (CONFIDENTIAL live) >>".to_string(),
        ),
        // ★ THE ORPHAN: same shape, listed in the xref, named by nothing.
        (
            6,
            "<< /Title (Superseded info) /Keywords (CONFIDENTIAL orphan) >>".to_string(),
        ),
        (
            7,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ),
        // ORPHAN 2: an unreferenced CONTENT STREAM carrying the same text.
        (8, {
            let stream = "BT /F1 24 Tf 40 100 Td (CONFIDENTIAL stream orphan) Tj ET";
            format!(
                "<< /Length {} >>
stream
{stream}
endstream",
                stream.len()
            )
        }),
        // ORPHAN 3: an unreferenced XMP-shaped metadata stream.
        (9, {
            let stream = "<?xpacket begin='' ?><x:xmpmeta><dc:title>CONFIDENTIAL xmp orphan</dc:title></x:xmpmeta><?xpacket end='w'?>";
            format!(
                "<< /Type /Metadata /Subtype /XML /Length {} >>
stream
{stream}
endstream",
                stream.len()
            )
        }),
    ];

    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in &objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let high = objects.len() as u32 + 1;
    buf.extend_from_slice(format!("xref\n0 {high}\n").as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for n in 1..high {
        let off = offsets
            .iter()
            .find(|(num, _)| *num == n)
            .map(|(_, o)| *o)
            .unwrap();
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {high} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref_at}\n%%EOF\n"
        )
        .as_bytes(),
    );
    buf
}

fn main() {
    let doc = Document::from_bytes(build()).expect("the fixture loads");
    println!("loaded; anomalies = {}", doc.load_anomalies().len());

    let mut session = EditSession::new(doc);
    let marked = session
        .mark_redactions_by_search("CONFIDENTIAL", false)
        .expect("mark");
    println!("marked {} region(s)", marked.len());

    let report = session.apply_redactions().expect("apply");
    println!("redacted_text = {:?}", report.redacted_text);
    println!("info_strings_scrubbed = {}", report.info_strings_scrubbed);
    println!(
        "residual_sweep: entries={} objects={}",
        report.residual_sweep_entries_scrubbed, report.residual_sweep_objects_scrubbed
    );
    for n in &report.notes {
        println!("  note: {n}");
    }
    for c in &report.carriers {
        println!(
            "  carrier {} present={} action={}",
            c.carrier,
            c.present,
            c.action.as_str()
        );
    }

    let (bytes, _) = session
        .to_full_bytes(&SaveOptions::default())
        .expect("save the redacted document");

    println!("--- saved {} bytes", bytes.len());
    println!(
        "contains CONFIDENTIAL      : {}",
        contains(&bytes, b"CONFIDENTIAL")
    );
    println!(
        "contains 'Superseded info' : {}",
        contains(&bytes, b"Superseded info")
    );
    println!(
        "contains 'Live info'       : {}",
        contains(&bytes, b"Live info")
    );
    println!(
        "contains 'stream orphan'   : {}",
        contains(&bytes, b"stream orphan")
    );
    println!(
        "contains 'xmp orphan'      : {}",
        contains(&bytes, b"xmp orphan")
    );
    println!(
        "contains 'CONFIDENTIAL orphan' (info)  : {}",
        contains(&bytes, b"CONFIDENTIAL orphan")
    );
    println!(
        "contains 'CONFIDENTIAL xmp'    (xmp)   : {}",
        contains(&bytes, b"CONFIDENTIAL xmp")
    );
    println!(
        "contains 'CONFIDENTIAL stream' (stream): {}",
        contains(&bytes, b"CONFIDENTIAL stream")
    );
}
