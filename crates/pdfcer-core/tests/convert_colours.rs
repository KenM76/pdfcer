//! `EditSession::convert_colours` over synthetic fixtures with a mock
//! converter (the real one is pdfcer-render's, tested there). ISO 32000-2
//! §8.6.8, §8.9.5, §8.6.6.3, §8.7.4.5.3, §14.11.5.

use pdfcer_core::color::RenderingIntent;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::prepress::{
    ColourConversion, ColourConverter, ColourDestination, ColourSource, ColourTransform,
    ConvertFailure, ConvertScope, ConvertedColour, OutputIntentChange, SourceFamily,
};
use pdfcer_core::writer::SaveOptions;

/// RGB → CMYK as `1 - x` with no black; grey → K; Lab → K from L*. Pure
/// red is "out of gamut".
struct Mock;

struct MockTransform(Kind);

#[derive(Clone, Copy)]
enum Kind {
    Rgb,
    Grey,
    Lab,
}

impl ColourConverter for Mock {
    fn transform(
        &self,
        source: &ColourSource,
        destination: &[u8],
        _intent: RenderingIntent,
        _bpc: bool,
    ) -> Result<Box<dyn ColourTransform>, ConvertFailure> {
        assert_eq!(&destination[16..20], b"CMYK");
        let kind = match source {
            ColourSource::DeviceRgb | ColourSource::CalRgb { .. } => Kind::Rgb,
            ColourSource::DeviceGray | ColourSource::CalGray { .. } => Kind::Grey,
            ColourSource::Lab { .. } => Kind::Lab,
            _ => return Err(ConvertFailure::new("mock: unsupported")),
        };
        Ok(Box::new(MockTransform(kind)))
    }
}

impl ColourTransform for MockTransform {
    fn convert(&self, v: &[f64]) -> Option<ConvertedColour> {
        Some(match self.0 {
            Kind::Rgb => ConvertedColour::new(
                vec![1.0 - v[0], 1.0 - v[1], 1.0 - v[2], 0.0],
                v == [1.0, 0.0, 0.0],
            ),
            Kind::Grey => ConvertedColour::new(vec![0.0, 0.0, 0.0, 1.0 - v[0]], false),
            Kind::Lab => ConvertedColour::new(vec![0.0, 0.0, 0.0, 1.0 - v[0] / 100.0], false),
        })
    }

    fn note(&self) -> Option<String> {
        Some("mock conversion".into())
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
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
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

fn stream(dict: &str, data: &[u8]) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{}\nendstream",
        data.len(),
        String::from_utf8_lossy(data)
    )
}

const CONTENT: &str = "1 0 0 rg 0 0 10 10 re f 0 0 1 RG 0 0 5 5 re S 0.5 g \
                       /L0 cs 50 0 0 sc 0 0 2 2 re f /Im Do /Ix Do /Sh sh";

/// One page. `/L0` Lab; `/Im` a 2x1 DeviceRGB image (red, blue); `/Ix` a
/// 2x1 image in an Indexed space over DeviceRGB (object 11); `/Sh` an axial
/// DeviceRGB shading (object 12). Object 9 is untouched by every edit.
fn fixture(catalog_extra: &str) -> Vec<u8> {
    let resources = "<< /ColorSpace << /L0 [/Lab << /WhitePoint [0.9642 1 0.8249] >>] >> \
                     /XObject << /Im 10 0 R /Ix 13 0 R >> /Shading << /Sh 12 0 R >> >>";
    build(&[
        (
            1,
            format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>"),
        ),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".into(),
        ),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources {resources} /Extra 9 0 R >>"
            ),
        ),
        (4, stream("", CONTENT.as_bytes())),
        (
            5,
            "<< /FunctionType 2 /Domain [0 1] /C0 [1 1 1] /C1 [0 0 0] /N 1 >>".into(),
        ),
        (9, "<< /Note (untouched) >>".into()),
        (
            10,
            stream(
                "/Type /XObject /Subtype /Image /Width 2 /Height 1 /BitsPerComponent 8 \
                 /ColorSpace /DeviceRGB /Filter /ASCIIHexDecode",
                b"FF00000000FF>",
            ),
        ),
        (11, "[/Indexed /DeviceRGB 1 <FFFFFF000000>]".into()),
        (
            12,
            "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 10 0] /Function 5 0 R >>".into(),
        ),
        (
            13,
            stream(
                "/Type /XObject /Subtype /Image /Width 2 /Height 1 /BitsPerComponent 8 \
                 /ColorSpace 11 0 R /Filter /ASCIIHexDecode",
                b"0001>",
            ),
        ),
    ])
}

fn session(catalog_extra: &str) -> EditSession {
    EditSession::new(Document::from_bytes(fixture(catalog_extra)).expect("fixture loads"))
}

fn conversion() -> ColourConversion {
    ColourConversion::new(ColourDestination::Profile(cmyk_profile()))
}

fn value(session: &EditSession, num: u32) -> Object {
    session
        .view()
        .graph()
        .value(ObjId::new(num, 0))
        .cloned()
        .expect("object exists")
}

fn decoded(session: &EditSession, num: u32) -> Vec<u8> {
    let view = session.view();
    let Some(Object::Stream(s)) = view.graph().value(ObjId::new(num, 0)) else {
        panic!("object {num} is not a stream");
    };
    let raw = view.slice(s.data_span).expect("stream data");
    pdfcer_core::filters::decode_stream(&s.dict, raw).expect("decodes")
}

fn dict_get(obj: &Object, key: &[u8]) -> Option<Object> {
    match obj {
        Object::Dict(d) => d.get(key).cloned(),
        Object::Stream(s) => s.dict.get(key).cloned(),
        _ => None,
    }
}

#[test]
fn converts_content_images_palettes_and_shadings() {
    let mut s = session("");
    let report = s
        .convert_colours(&ConvertScope::All, &conversion(), &Mock)
        .expect("converts");
    let content = String::from_utf8(decoded(&s, 4)).expect("ascii");
    assert!(
        !content.contains(" rg") && !content.contains(" RG"),
        "{content}"
    );
    assert!(content.contains("0 1 1 0 k"), "red is 0 1 1 0: {content}");
    assert!(content.contains("1 1 0 0 K"), "blue stroke: {content}");
    assert!(content.contains("0.5 g"), "grey is not selected: {content}");
    assert!(content.contains("0.5 k"), "Lab L*=50 is K 0.5: {content}");
    assert_eq!(decoded(&s, 10), [0, 255, 255, 0, 255, 255, 0, 0]);
    assert_eq!(
        dict_get(&value(&s, 10), b"ColorSpace"),
        Some(Object::Name(b"DeviceCMYK".into()))
    );
    let Object::Array(indexed) = value(&s, 11) else {
        panic!("indexed space")
    };
    assert_eq!(indexed[1], Object::Name(b"DeviceCMYK".into()));
    assert_eq!(
        indexed[3],
        Object::String(vec![0, 0, 0, 0, 0, 0, 0, 255]),
        "white, then black kept as K only"
    );
    assert_eq!(
        dict_get(&value(&s, 12), b"ColorSpace"),
        Some(Object::Name(b"DeviceCMYK".into()))
    );
    let rgb = report
        .by_source
        .iter()
        .find(|c| c.source == SourceFamily::DeviceRgb)
        .expect("DeviceRGB counted");
    assert_eq!(
        (rgb.images, rgb.lookups, rgb.shadings),
        (1, 1, 1),
        "{report:?}"
    );
    assert!(rgb.paints >= 2, "{report:?}");
    assert!(rgb.out_of_gamut >= 1, "pure red: {report:?}");
    assert!(
        report
            .by_source
            .iter()
            .any(|c| c.source == SourceFamily::Lab && c.paints == 1),
        "{report:?}"
    );
    assert_eq!(report.images.len(), 1, "{report:?}");
    assert_eq!(report.images[0].object, ObjId::new(10, 0));
    assert_eq!(
        report.images[0].filter_before.as_deref(),
        Some("ASCIIHexDecode")
    );
    assert!(
        report
            .disclosures
            .iter()
            .any(|d| d.contains("mock conversion"))
    );
    assert!(!report.changed.contains(&ObjId::new(9, 0)));
    assert_eq!(s.undo(), Some(CommandKind::ConvertColours));
    assert_eq!(value(&s, 11), value(&session(""), 11), "undo restores");
    assert!(!s.can_undo(), "one undo step");
}

#[test]
fn save_leaves_untouched_objects_alone() {
    let source = fixture("");
    let mut s = EditSession::new(Document::from_bytes(source.clone()).expect("loads"));
    s.convert_colours(&ConvertScope::All, &conversion(), &Mock)
        .expect("converts");
    let (inc, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("saves");
    assert!(inc.starts_with(&source), "incremental save appends only");
    let appended = &inc[source.len()..];
    for num in [2, 3, 5, 9] {
        let head = format!("\n{num} 0 obj");
        assert!(
            !contains(appended, head.as_bytes()),
            "untouched object {num} was re-emitted"
        );
    }
    let (full, _) = s.to_full_bytes(&SaveOptions::identity()).expect("saves");
    for num in [2, 3, 5, 9] {
        assert!(
            contains(&full, object_bytes(&source, num)),
            "object {num} differs in the full rewrite"
        );
    }
}

#[test]
fn keep_pure_black_writes_the_destination_black() {
    let mut s = EditSession::new(
        Document::from_bytes(build(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 9 9] >>".into(),
            ),
            (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
            (4, stream("", b"0 0 0 rg 0 0 1 1 re f")),
        ]))
        .expect("loads"),
    );
    s.convert_colours(&ConvertScope::All, &conversion(), &Mock)
        .expect("converts");
    let content = String::from_utf8(decoded(&s, 4)).expect("ascii");
    assert!(content.contains("0 0 0 1 k"), "K only: {content}");
}

#[test]
fn embeds_the_destination_as_an_output_intent_and_replaces_one() {
    let mut c = conversion();
    c.embed_output_intent = Some("GTS_PDFX".into());
    let mut s = session("");
    let report = s
        .convert_colours(&ConvertScope::All, &c, &Mock)
        .expect("converts");
    assert_eq!(report.output_intent, OutputIntentChange::Added);
    let intents = dict_get(&value(&s, 1), b"OutputIntents").expect("intents written");
    let Object::Array(list) = intents else {
        panic!("an array")
    };
    assert_eq!(list.len(), 1);

    let existing = "/OutputIntents [<< /Type /OutputIntent /S /GTS_PDFX \
                    /OutputConditionIdentifier (Old press) >>]";
    let mut s = session(existing);
    let report = s
        .convert_colours(&ConvertScope::All, &c, &Mock)
        .expect("converts");
    assert_eq!(
        report.output_intent,
        OutputIntentChange::Replaced {
            previous: "Old press".into()
        }
    );
    assert!(
        report.disclosures.iter().any(|d| d.contains("Old press")),
        "{report:?}"
    );
}

#[test]
fn refusals_commit_nothing() {
    let mut s = session("");
    let intent = ColourConversion::new(ColourDestination::OutputIntent);
    assert!(matches!(
        s.convert_colours(&ConvertScope::All, &intent, &Mock),
        Err(EditError::ColourConversionRefused { .. })
    ));
    let mut none = conversion();
    none.device_rgb = false;
    none.icc_rgb = false;
    none.lab = false;
    none.cal_rgb = false;
    assert!(matches!(
        s.convert_colours(&ConvertScope::All, &none, &Mock),
        Err(EditError::ColourConversionRefused { .. })
    ));
    let bad = ColourConversion::new(ColourDestination::Profile(vec![0; 64]));
    assert!(matches!(
        s.convert_colours(&ConvertScope::All, &bad, &Mock),
        Err(EditError::ColourConversionRefused { .. })
    ));
    assert!(matches!(
        s.convert_colours(&ConvertScope::Pages(vec![4]), &conversion(), &Mock),
        Err(EditError::PageOutOfRange { .. })
    ));
    assert!(!s.can_undo());
}

#[test]
fn page_scope_converts_only_that_page() {
    let mut s = session("");
    let report = s
        .convert_colours(&ConvertScope::Pages(vec![0]), &conversion(), &Mock)
        .expect("converts");
    assert!(report.changed.contains(&ObjId::new(4, 0)), "{report:?}");
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// `N 0 obj ... endobj` as written in `pdf`.
fn object_bytes(pdf: &[u8], num: u32) -> &[u8] {
    let head = format!("{num} 0 obj\n");
    let start = pdf
        .windows(head.len())
        .position(|w| w == head.as_bytes())
        .expect("object present");
    let end = start
        + pdf[start..]
            .windows(6)
            .position(|w| w == b"endobj")
            .expect("endobj")
        + 6;
    &pdf[start..end]
}
