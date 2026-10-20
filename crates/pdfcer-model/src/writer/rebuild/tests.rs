use super::*;
use crate::object::{Dict, Object};

/// A classic-xref PDF 1.4 from object bodies numbered 1..=n.
fn pdf(bodies: &[&str]) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", bodies.len() + 1).as_bytes(),
    );
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            bodies.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn stream(dict: &str, data: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{data}\nendstream",
        data.len()
    )
}

/// Two pages with identical content streams and identical images, plus an
/// orphan, two fonts that become identical only once their descriptors
/// merge, and one dangling reference.
fn sample() -> Document {
    let content = stream("", "q /Im Do Q");
    let image = stream(
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray",
        "A",
    );
    let page = |c: u32, im: u32, f: u32| {
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents {c} 0 R \
             /Resources << /XObject << /Im {im} 0 R >> /Font << /F {f} 0 R >> /X 99 0 R >> >>"
        )
    };
    let font = |d: u32| {
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FontDescriptor {d} 0 R >>")
    };
    let descriptor = "<< /Type /FontDescriptor /FontName /Helvetica /Flags 32 >>";
    let bodies: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".into(),
        page(5, 7, 10),
        page(6, 8, 11),
        content.clone(),
        content,
        image.clone(),
        image,
        "<< /Orphan true >>".into(),
        font(12),
        font(13),
        descriptor.into(),
        descriptor.into(),
    ];
    let refs: Vec<&str> = bodies.iter().map(String::as_str).collect();
    Document::from_bytes(pdf(&refs)).expect("sample parses")
}

fn id(n: u32) -> ObjId {
    ObjId::new(n, 0)
}

fn run(options: RebuildOptions) -> (Document, RebuildReport) {
    let (bytes, report) = rebuild(&sample(), &options, &SaveOptions::default()).expect("rebuild");
    (Document::from_bytes(bytes).expect("output reloads"), report)
}

fn page_dicts(doc: &Document) -> Vec<Dict> {
    let pages = doc
        .catalog()
        .expect("catalog")
        .get(b"Pages")
        .cloned()
        .expect("/Pages");
    let pages = doc.resolve(&pages).as_dict().expect("pages dict").clone();
    let kids = pages
        .get(b"Kids")
        .and_then(Object::as_array)
        .expect("kids")
        .to_vec();
    kids.iter()
        .map(|k| doc.resolve(k).as_dict().expect("page").clone())
        .collect()
}

fn resource(doc: &Document, page: &Dict, category: &[u8], name: &[u8]) -> Object {
    let res = page
        .get(b"Resources")
        .and_then(Object::as_dict)
        .expect("resources");
    let cat = res
        .get(category)
        .and_then(Object::as_dict)
        .expect("category");
    cat.get(name).cloned().map_or(Object::Null, |o| match o {
        Object::Reference(_) => o,
        other => doc.resolve(&other).clone(),
    })
}

fn content_bytes(doc: &Document, page: &Dict) -> Vec<u8> {
    let Object::Stream(s) = doc.resolve(page.get(b"Contents").expect("contents")) else {
        panic!("contents is a stream")
    };
    s.data_span.slice(doc.bytes()).expect("span").to_vec()
}

#[test]
fn drops_the_orphan_and_merges_what_is_identical() {
    let (_, report) = run(RebuildOptions::all());
    assert_eq!(report.unreferenced, vec![id(9)]);
    assert_eq!(
        report.merged,
        vec![(id(8), id(7)), (id(11), id(10)), (id(13), id(12))]
    );
    assert_eq!(report.dangling, 2, "/X 99 0 R on each page");
}

#[test]
fn page_content_streams_keep_their_identity() {
    let (doc, _) = run(RebuildOptions::all());
    let pages = page_dicts(&doc);
    assert_ne!(pages[0].get(b"Contents"), pages[1].get(b"Contents"));
    assert_eq!(content_bytes(&doc, &pages[0]), b"q /Im Do Q");
    assert_eq!(content_bytes(&doc, &pages[1]), b"q /Im Do Q");
}

#[test]
fn merged_resources_name_one_object() {
    let (doc, _) = run(RebuildOptions::all());
    let pages = page_dicts(&doc);
    for (cat, name) in [(b"XObject".as_slice(), b"Im".as_slice()), (b"Font", b"F")] {
        let a = resource(&doc, &pages[0], cat, name);
        assert!(matches!(a, Object::Reference(_)));
        assert_eq!(a, resource(&doc, &pages[1], cat, name));
    }
}

#[test]
fn packing_writes_object_streams_and_raises_the_version() {
    let (doc, report) = run(RebuildOptions::all());
    assert!(report.object_streams >= 1);
    assert_eq!(
        report.version_raised,
        Some(PdfVersion { major: 1, minor: 5 })
    );
    assert_eq!(doc.version(), PdfVersion { major: 1, minor: 5 });
    let compressed = doc
        .objects()
        .filter(|o| matches!(o.provenance, crate::object::Provenance::ObjectStream { .. }))
        .count();
    assert!(compressed > 0, "objects reload from the object stream");
    assert!(
        doc.load_anomalies().is_empty(),
        "{:?}",
        doc.load_anomalies()
    );
}

#[test]
fn classic_output_keeps_the_version_and_has_no_object_streams() {
    let options = RebuildOptions {
        object_streams: false,
        ..RebuildOptions::all()
    };
    let (doc, report) = run(options);
    assert_eq!(report.object_streams, 0);
    assert_eq!(report.version_raised, None);
    assert_eq!(doc.version(), PdfVersion { major: 1, minor: 4 });
    // 13 input objects, one orphan, three merged.
    assert_eq!(report.objects_out, 9);
    assert_eq!(doc.object_count(), 9);
    assert!(
        doc.load_anomalies().is_empty(),
        "{:?}",
        doc.load_anomalies()
    );
}

#[test]
fn everything_off_only_renumbers() {
    let options = RebuildOptions {
        remove_unreferenced: false,
        merge_duplicates: false,
        object_streams: false,
        level: 9,
    };
    let (doc, report) = run(options);
    assert!(report.unreferenced.is_empty() && report.merged.is_empty());
    assert_eq!(doc.object_count(), 13);
}

#[test]
fn rebuilding_the_output_again_changes_nothing() {
    let (bytes, _) =
        rebuild(&sample(), &RebuildOptions::all(), &SaveOptions::default()).expect("first");
    let doc = Document::from_bytes(bytes.clone()).expect("reload");
    let (again, report) =
        rebuild(&doc, &RebuildOptions::all(), &SaveOptions::default()).expect("second");
    assert!(report.unreferenced.is_empty() && report.merged.is_empty());
    assert_eq!(report.structural_dropped.len(), report.object_streams + 1);
    assert_eq!(again, bytes, "a rebuild is a fixpoint");
}
