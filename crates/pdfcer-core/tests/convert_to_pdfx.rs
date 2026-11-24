//! `EditSession::convert_to_pdfx` over synthetic fixtures, with a mock
//! colour converter and a fake flattener (the real ones are tested in
//! pdfcer-render and pdfcer-emf). ISO 15930-1/-4/-6/-7.

use std::cell::Cell;

use pdfcer_core::color::RenderingIntent;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditSession};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::preflight::{PdfxLevel, PdfxVerdict, Severity};
use pdfcer_core::prepress::{
    ColourConverter, ColourSource, ColourTransform, ConvertFailure, ConvertedColour,
    PdfxConversion, PdfxConversionReport, PdfxOutputIntent,
};
use pdfcer_core::transparency::{
    FlattenFailure, FlattenMode, FlattenOptions, FlattenedPage, RasterPatch, TransparencyFlattener,
};
use pdfcer_core::view::DocumentView;
use pdfcer_core::writer::SaveOptions;

/// RGB and grey to CMYK as `1 - x`.
struct Mock;

struct Invert(bool);

impl ColourConverter for Mock {
    fn transform(
        &self,
        source: &ColourSource,
        _destination: &[u8],
        _intent: RenderingIntent,
        _bpc: bool,
    ) -> Result<Box<dyn ColourTransform>, ConvertFailure> {
        match source {
            ColourSource::DeviceRgb | ColourSource::CalRgb { .. } => Ok(Box::new(Invert(true))),
            ColourSource::DeviceGray | ColourSource::CalGray { .. } => Ok(Box::new(Invert(false))),
            _ => Err(ConvertFailure::new("mock: unsupported")),
        }
    }
}

impl ColourTransform for Invert {
    fn convert(&self, v: &[f64]) -> Option<ConvertedColour> {
        Some(if self.0 {
            ConvertedColour::new(vec![1.0 - v[0], 1.0 - v[1], 1.0 - v[2], 0.0], false)
        } else {
            ConvertedColour::new(vec![0.0, 0.0, 0.0, 1.0 - v[0]], false)
        })
    }

    fn note(&self) -> Option<String> {
        None
    }
}

/// Paints the page opaque, with a 1x1 RGB patch, counting its calls.
#[derive(Default)]
struct Fake {
    calls: Cell<usize>,
}

impl TransparencyFlattener for Fake {
    fn flatten_page(
        &self,
        _view: &DocumentView<'_>,
        _page: usize,
        _options: &FlattenOptions,
    ) -> Result<FlattenedPage, FlattenFailure> {
        self.calls.set(self.calls.get() + 1);
        let mut page = FlattenedPage::new(FlattenMode::Rebuilt);
        page.content = b"0.5 0.5 1 rg 10 10 50 50 re f q 1 0 0 1 0 0 cm /TrRaster1 Do Q".to_vec();
        page.images
            .push(RasterPatch::new("TrRaster1".into(), 1, 1, vec![255, 0, 0]).expect("patch"));
        page.vector_regions = 1;
        Ok(page)
    }
}

/// A 128-byte header-only CMYK output profile (ICC.1:2022 §7.2).
fn cmyk_profile() -> Vec<u8> {
    let mut p = vec![0u8; 128];
    p[0..4].copy_from_slice(&128u32.to_be_bytes());
    p[12..16].copy_from_slice(b"prtr");
    p[16..20].copy_from_slice(b"CMYK");
    p[20..24].copy_from_slice(b"Lab ");
    p[36..40].copy_from_slice(b"acsp");
    p
}

fn build(objects: &[(u32, String)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.6\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
    buf.extend_from_slice(format!("xref\n0 {}\n", max_num + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..=max_num {
        match offsets.iter().find(|(n, _)| *n == num) {
            Some((_, off)) => buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => buf.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /ID [<0102> <0304>] >>\nstartxref\n{xref_at}\n%%EOF\n",
            max_num + 1
        )
        .as_bytes(),
    );
    buf
}

fn stream(data: &str) -> String {
    format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len())
}

/// One page drawing RGB vector content. `resources` is the page's
/// `/Resources`; object 9 is untouched by every edit.
fn fixture(content: &str, resources: &str, extra: &[(u32, String)]) -> Vec<u8> {
    let mut objects = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] >>".into(),
        ),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources {resources} /Extra 9 0 R >>"
            ),
        ),
        (4, stream(content)),
        (9, "<< /Note (untouched) >>".into()),
    ];
    objects.extend_from_slice(extra);
    build(&objects)
}

fn rgb_doc() -> Vec<u8> {
    fixture(
        "1 0 0 rg 0 0 10 10 re f 0 0 1 RG 0 0 5 5 re S",
        "<< >>",
        &[],
    )
}

fn conversion() -> PdfxConversion {
    PdfxConversion::new(PdfxOutputIntent::new(cmyk_profile()).with_condition("FOGRA39"))
        .with_title("Test")
        .with_time(1_700_000_000)
}

fn convert(bytes: Vec<u8>, level: PdfxLevel) -> (EditSession, PdfxConversionReport, usize) {
    let mut s = EditSession::new(Document::from_bytes(bytes).expect("loads"));
    let fake = Fake::default();
    let report = s
        .convert_to_pdfx(level, &conversion(), &fake, &Mock)
        .expect("converts");
    (s, report, fake.calls.get())
}

fn value(s: &EditSession, id: ObjId) -> Option<Object> {
    s.view().graph().value(id).cloned()
}

fn info_string(s: &EditSession, key: &[u8]) -> Option<String> {
    let id = s.view().graph().trailer_entry(b"Info")?.as_reference()?;
    let Some(Object::Dict(info)) = value(s, id) else {
        return None;
    };
    match info.get(key)? {
        Object::String(b) => Some(String::from_utf8_lossy(b).into_owned()),
        Object::Name(n) => Some(String::from_utf8_lossy(n.as_bytes()).into_owned()),
        _ => None,
    }
}

fn failures(report: &PdfxConversionReport) -> Vec<String> {
    report
        .validation
        .preflight
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .map(|f| format!("{}: {}", f.rule_id, f.message))
        .collect()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn an_rgb_document_converts_to_x4_and_is_claimed() {
    let (mut s, report, calls) = convert(rgb_doc(), PdfxLevel::X4);
    assert!(report.claimed, "{:?}\n{report:#?}", failures(&report));
    assert_ne!(report.after, PdfxVerdict::Fails);
    assert_eq!(report.before, PdfxVerdict::Fails);
    assert_eq!(calls, 0, "X-4 keeps transparency");
    assert!(report.flatten.is_none());
    assert_eq!(report.trim_boxes, [0]);
    assert_eq!(
        info_string(&s, b"GTS_PDFXVersion").as_deref(),
        Some("PDF/X-4")
    );
    assert_eq!(info_string(&s, b"Trapped").as_deref(), Some("False"));
    assert!(
        report.disclosures.iter().any(|d| d.contains("Trapped")),
        "the Trapped default is disclosed: {:?}",
        report.disclosures
    );
    let catalog = value(&s, ObjId::new(1, 0)).expect("catalog");
    let Object::Dict(catalog) = catalog else {
        panic!("catalog");
    };
    let meta = catalog
        .get(b"Metadata")
        .and_then(Object::as_reference)
        .expect("X-4 creates an XMP packet");
    let Some(Object::Stream(packet)) = value(&s, meta) else {
        panic!("packet");
    };
    let raw = s.view().slice(packet.data_span).expect("data").to_vec();
    let xmp = String::from_utf8_lossy(&raw);
    assert!(xmp.contains("pdfxid:GTS_PDFXVersion"), "{xmp}");
    assert!(xmp.contains("PDF/X-4"), "{xmp}");
    let content = value(&s, ObjId::new(4, 0));
    assert!(content.is_some());
    assert_eq!(s.undo(), Some(CommandKind::ConvertToPdfx));
    assert!(!s.can_undo(), "one undo step");
    assert!(
        info_string(&s, b"GTS_PDFXVersion").is_none(),
        "undo removes it"
    );
}

#[test]
fn a_transparent_document_is_flattened_for_x1a() {
    let content = "/G gs 1 0 0 rg 10 10 50 50 re f";
    let bytes = fixture(
        content,
        "<< /ExtGState << /G 5 0 R >> >>",
        &[(5, "<< /Type /ExtGState /ca 0.5 >>".into())],
    );
    let (s, report, calls) = convert(bytes, PdfxLevel::X1a2001);
    assert_eq!(calls, 1, "the page was flattened");
    let flatten = report.flatten.as_ref().expect("a flatten report");
    assert_eq!(flatten.pages.len(), 1, "{flatten:?}");
    assert!(
        report.colours.is_some(),
        "the flattened RGB is converted afterwards"
    );
    assert!(report.claimed, "{:?}\n{report:#?}", failures(&report));
    assert_eq!(
        info_string(&s, b"GTS_PDFXVersion").as_deref(),
        Some("PDF/X-1:2001")
    );
    assert_eq!(
        info_string(&s, b"GTS_PDFXConformance").as_deref(),
        Some("PDF/X-1a:2001")
    );
}

#[test]
fn an_unembedded_font_refuses_the_claim_and_changes_nothing() {
    let content = "BT /F1 12 Tf 10 10 Td (Hi) Tj ET";
    let bytes = fixture(
        content,
        "<< /Font << /F1 6 0 R >> >>",
        &[(
            6,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        )],
    );
    let (s, report, _) = convert(bytes.clone(), PdfxLevel::X4);
    assert!(!report.claimed);
    assert_eq!(report.after, PdfxVerdict::Fails);
    assert!(!failures(&report).is_empty(), "what remains is reported");
    assert!(
        report.disclosures.iter().any(|d| d.contains("NOT written")),
        "{:?}",
        report.disclosures
    );
    assert!(!s.can_undo(), "a refused conversion changes nothing");
    assert!(info_string(&s, b"GTS_PDFXVersion").is_none());
}

#[test]
fn keep_partial_keeps_the_steps_without_the_claim() {
    let content = "BT /F1 12 Tf 10 10 Td (Hi) Tj ET 1 0 0 rg 0 0 5 5 re f";
    let bytes = fixture(
        content,
        "<< /Font << /F1 6 0 R >> >>",
        &[(
            6,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        )],
    );
    let mut s = EditSession::new(Document::from_bytes(bytes).expect("loads"));
    let report = s
        .convert_to_pdfx(
            PdfxLevel::X4,
            &conversion().with_keep_partial(true),
            &Fake::default(),
            &Mock,
        )
        .expect("converts");
    assert!(!report.claimed && report.kept_partial);
    assert!(info_string(&s, b"GTS_PDFXVersion").is_none(), "no claim");
    assert_eq!(s.undo(), Some(CommandKind::ConvertToPdfx));
    assert!(!s.can_undo(), "one undo step");
}

#[test]
fn save_leaves_untouched_objects_alone() {
    let source = rgb_doc();
    let (s, report, _) = convert(source.clone(), PdfxLevel::X4);
    assert!(report.claimed);
    let (inc, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("saves");
    assert!(inc.starts_with(&source), "incremental save appends only");
    let appended = &inc[source.len()..];
    assert!(
        !contains(appended, b"\n9 0 obj"),
        "untouched object 9 was re-emitted"
    );
    let reopened = Document::from_bytes(inc).expect("reopens");
    let check = pdfcer_core::preflight::validate_pdfx(&reopened.view(), PdfxLevel::X4);
    assert_ne!(check.verdict(), PdfxVerdict::Fails, "the saved file holds");
}
