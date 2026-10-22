use super::super::tests::{pdf, stream};
use super::super::{RebuildOptions, rebuild};
use crate::document::Document;
use crate::object::{Dict, ObjId, Object};
use crate::writer::{SaveOptions, WriteError};

/// Catalog, a page tree whose root carries the inherited `/Resources` and
/// `/MediaBox`, three pages, a font every page uses, a font only pages 2
/// and 3 use, private content streams, a two-item outline and `/Info`.
fn sample(page_mode: &str) -> Vec<u8> {
    let c1 = stream("", "BT /F1 12 Tf (one) Tj ET");
    let c2 = stream("", "BT /F2 12 Tf (two) Tj ET");
    let c3 = stream("", "BT /F2 12 Tf (three) Tj ET");
    let catalog = format!("<< /Type /Catalog /Pages 2 0 R /Outlines 9 0 R {page_mode} >>");
    let bytes = pdf(&[
        &catalog,
        "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 \
         /Resources << /Font << /F1 6 0 R >> >> /MediaBox [0 0 612 792] >>",
        "<< /Type /Page /Parent 2 0 R /Contents 12 0 R >>",
        "<< /Type /Page /Parent 2 0 R /Contents 13 0 R \
         /Resources << /Font << /F1 6 0 R /F2 7 0 R >> >> >>",
        "<< /Type /Page /Parent 2 0 R /Contents 14 0 R /Rotate 90 \
         /Resources << /Font << /F1 6 0 R /F2 7 0 R >> >> >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>",
        "<< /Producer (pdfcer test) >>",
        "<< /Type /Outlines /First 10 0 R /Last 11 0 R /Count 2 >>",
        "<< /Title (One) /Parent 9 0 R /Next 11 0 R /Dest [3 0 R /Fit] >>",
        "<< /Title (Two) /Parent 9 0 R /Prev 10 0 R /Dest [4 0 R /Fit] >>",
        &c1,
        &c2,
        &c3,
    ]);
    with_info(bytes, 8)
}

/// Add `/Info n 0 R` to the trailer `pdf` wrote.
fn with_info(bytes: Vec<u8>, info: u32) -> Vec<u8> {
    let text = String::from_utf8(bytes).unwrap();
    let text = text.replacen("/Root 1 0 R", &format!("/Root 1 0 R /Info {info} 0 R"), 1);
    // The table offsets precede the trailer, so they are unaffected.
    text.into_bytes()
}

fn linearize(bytes: Vec<u8>) -> Result<(Vec<u8>, super::RebuildReport), WriteError> {
    let doc = Document::from_bytes(bytes).unwrap();
    let options = RebuildOptions {
        linearize: true,
        ..RebuildOptions::all()
    };
    rebuild(&doc, &options, &SaveOptions::default())
}

/// When `PDFCER_LIN_DUMP` names a folder, write the output there for an
/// external check (`qpdf --check-linearization`).
fn dump(name: &str, bytes: &[u8]) {
    if let Some(dir) = std::env::var_os("PDFCER_LIN_DUMP") {
        std::fs::write(std::path::Path::new(&dir).join(name), bytes).unwrap();
    }
}

fn dict(doc: &Document, id: ObjId) -> Dict {
    match &doc.get(id).unwrap().value {
        Object::Dict(d) => d.clone(),
        Object::Stream(s) => s.dict.clone(),
        other => panic!("{id} is {other:?}"),
    }
}

fn int(d: &Dict, key: &[u8]) -> i64 {
    match d.get(key) {
        Some(Object::Integer(v)) => *v,
        other => panic!("{:?} is {other:?}", String::from_utf8_lossy(key)),
    }
}

fn reference(value: Option<&Object>) -> ObjId {
    match value {
        Some(Object::Reference(r)) => *r,
        other => panic!("not a reference: {other:?}"),
    }
}

/// The parameter dictionary, which must be the file's first object.
fn parameters(bytes: &[u8], doc: &Document) -> Dict {
    let text = String::from_utf8_lossy(bytes.get(..200).unwrap()).into_owned();
    let line = text.lines().nth(2).unwrap();
    let num: u32 = line.split(' ').next().unwrap().parse().unwrap();
    let d = dict(doc, ObjId::new(num, 0));
    assert_eq!(int(&d, b"Linearized"), 1);
    d
}

fn kids(doc: &Document) -> Vec<ObjId> {
    let pages = reference(doc.catalog().unwrap().get(b"Pages"));
    let node = dict(doc, pages);
    assert!(!node.contains_key(b"Resources") && !node.contains_key(b"MediaBox"));
    node.get(b"Kids")
        .and_then(Object::as_array)
        .unwrap()
        .iter()
        .map(|k| reference(Some(k)))
        .collect()
}

#[test]
fn parameters_match_the_file() {
    let (bytes, report) = linearize(sample("")).unwrap();
    dump("lin-outlines.pdf", &bytes);
    assert!(report.linearized && report.object_streams == 0);
    let doc = Document::from_bytes(bytes.clone()).unwrap();
    assert!(
        doc.load_anomalies().is_empty(),
        "{:?}",
        doc.load_anomalies()
    );
    let lin = parameters(&bytes, &doc);
    assert_eq!(int(&lin, b"L"), bytes.len() as i64);
    assert_eq!(int(&lin, b"N"), 3);
    let pages = kids(&doc);
    assert_eq!(int(&lin, b"O"), i64::from(pages[0].num));
    assert_eq!(report.objects_out, doc.object_count());
}

#[test]
fn every_page_carries_its_inherited_attributes() {
    let (bytes, _) = linearize(sample("")).unwrap();
    let doc = Document::from_bytes(bytes).unwrap();
    let pages: Vec<Dict> = kids(&doc).into_iter().map(|p| dict(&doc, p)).collect();
    for page in &pages {
        assert!(page.contains_key(b"MediaBox") && page.contains_key(b"Resources"));
    }
    assert_eq!(int(&pages[2], b"Rotate"), 90);
    assert!(!pages[0].contains_key(b"Rotate"));
}

#[test]
fn page_content_survives_in_order() {
    let (bytes, _) = linearize(sample("")).unwrap();
    let doc = Document::from_bytes(bytes).unwrap();
    for (page, text) in kids(&doc).into_iter().zip(["one", "two", "three"]) {
        let contents = reference(dict(&doc, page).get(b"Contents"));
        let Object::Stream(s) = &doc.get(contents).unwrap().value else {
            panic!("contents not a stream")
        };
        let data = s.data_span.slice(doc.bytes()).unwrap();
        assert!(String::from_utf8_lossy(data).contains(&format!("({text})")));
    }
}

#[test]
fn outlines_open_with_the_first_page_when_the_document_opens_on_them() {
    let (bytes, _) = linearize(sample("/PageMode /UseOutlines")).unwrap();
    dump("lin-use-outlines.pdf", &bytes);
    let doc = Document::from_bytes(bytes.clone()).unwrap();
    let lin = parameters(&bytes, &doc);
    let outlines = reference(doc.catalog().unwrap().get(b"Outlines"));
    let at = |id: ObjId| {
        let needle = format!("\n{} 0 obj\n", id.num);
        bytes
            .windows(needle.len())
            .position(|w| w == needle.as_bytes())
            .unwrap() as i64
    };
    assert!(
        at(outlines) < int(&lin, b"E"),
        "outlines are in the first-page section"
    );
}

#[test]
fn a_single_page_old_file_is_raised_to_pdf_1_2() {
    let content = stream("", "0 0 m 1 1 l S");
    let mut bytes = pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> /Contents 4 0 R >>",
        &content,
    ]);
    bytes[7] = b'1';
    let (out, report) = linearize(bytes).unwrap();
    dump("lin-single.pdf", &out);
    assert!(out.starts_with(b"%PDF-1.2\n"));
    assert!(report.version_raised.is_some());
    let doc = Document::from_bytes(out.clone()).unwrap();
    assert_eq!(int(&parameters(&out, &doc), b"N"), 1);
}

#[test]
fn a_document_without_pages_is_refused() {
    let bytes = pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [] /Count 0 >>",
    ]);
    assert!(matches!(
        linearize(bytes),
        Err(WriteError::NothingToLinearize)
    ));
}

/// Byte offset of `id`'s object header in `bytes`.
fn offset(bytes: &[u8], id: ObjId) -> usize {
    let needle = format!("\n{} 0 obj\n", id.num);
    bytes
        .windows(needle.len())
        .position(|w| w == needle.as_bytes())
        .unwrap()
}

#[test]
fn a_page_claims_what_it_shares_with_the_document() {
    let c = stream("", "0 0 m 1 1 l S");
    let bytes = pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 7 0 R /Outlines 8 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /Contents 10 0 R >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /Contents 10 0 R /Annots [6 0 R] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /Contents 10 0 R >>",
        "<< /Type /Annot /Subtype /Link /Rect [0 0 1 1] /Border [0 0 0] >>",
        "<< /Type /StructTreeRoot /K [ << /Type /OBJR /Obj 6 0 R >> ] >>",
        "<< /Type /Outlines /First 9 0 R /Last 9 0 R /Count 1 >>",
        "<< /Title (x) /Parent 8 0 R /A << /S /GoTo /D [4 0 R /Fit] >> /SE 6 0 R >>",
        &c,
    ]);
    let (out, _) = linearize(bytes).unwrap();
    dump("lin-claimed.pdf", &out);
    let doc = Document::from_bytes(out.clone()).unwrap();
    let pages = kids(&doc);
    let annot = reference(
        dict(&doc, pages[1])
            .get(b"Annots")
            .and_then(Object::as_array)
            .and_then(|a| a.first()),
    );
    let at = offset(&out, annot);
    assert!(
        offset(&out, pages[1]) < at && at < offset(&out, pages[2]),
        "the annotation is in page 2's part"
    );
}

#[test]
fn the_page_tree_stays_out_of_page_one_when_a_page_reaches_it() {
    let c = stream("", "0 0 m 1 1 l S");
    let bytes = pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /Outlines 5 0 R /PageMode /UseOutlines >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /Contents 7 0 R /Annots [2 0 R] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /Contents 7 0 R >>",
        "<< /Type /Outlines /First 6 0 R /Last 6 0 R /Count 1 >>",
        "<< /Title (x) /Parent 5 0 R /First 2 0 R /Last 2 0 R /Count 1 >>",
        &c,
    ]);
    let (out, _) = linearize(bytes).unwrap();
    dump("lin-tree-reached.pdf", &out);
    let doc = Document::from_bytes(out.clone()).unwrap();
    let lin = parameters(&out, &doc);
    let tree = reference(doc.catalog().unwrap().get(b"Pages"));
    assert!(
        offset(&out, tree) as i64 > int(&lin, b"E"),
        "page tree in part 9"
    );
}

#[test]
fn a_null_object_page_one_reaches_is_not_counted_in_it() {
    let c = stream("", "BT /F1 9 Tf (x) Tj ET");
    let bytes = pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /Contents 5 0 R \
         /Resources << /Font << /F1 4 0 R >> >> >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FontFile3 6 0 R >>",
        &c,
        "null",
    ]);
    let (out, _) = linearize(bytes).unwrap();
    dump("lin-dangling.pdf", &out);
    let doc = Document::from_bytes(out.clone()).unwrap();
    let lin = parameters(&out, &doc);
    let null = b" 0 obj\nnull\nendobj";
    let at = out.windows(null.len()).position(|w| w == null).unwrap();
    assert!(at as i64 > int(&lin, b"E"), "the null object is in part 9");
}

/// MSB-first reader over a hint stream (F.3.4: bits run high to low).
struct BitReader<'a> {
    data: &'a [u8],
    bit: usize,
}

impl BitReader<'_> {
    fn read(&mut self, width: u64) -> u64 {
        let mut v = 0;
        for _ in 0..width {
            let byte = self.data[self.bit / 8];
            v = (v << 1) | u64::from((byte >> (7 - self.bit % 8)) & 1);
            self.bit += 1;
        }
        v
    }

    /// Columns start on a byte boundary (qpdf's reading of Table F.4).
    fn align(&mut self) {
        self.bit = self.bit.div_ceil(8) * 8;
    }
}

#[test]
fn the_page_offset_table_locates_every_page_and_counts_its_objects() {
    let (out, _) = linearize(sample("")).unwrap();
    let doc = Document::from_bytes(out.clone()).unwrap();
    let lin = parameters(&out, &doc);
    let Some(Object::Array(h)) = lin.get(b"H") else {
        panic!("no /H")
    };
    let [Object::Integer(at), Object::Integer(len), ..] = h.as_slice() else {
        panic!("bad /H")
    };
    let (at, hint_len) = (
        usize::try_from(*at).unwrap(),
        usize::try_from(*len).unwrap(),
    );
    let head = String::from_utf8_lossy(&out[at..at + 20]).into_owned();
    let num: u32 = head.split(' ').next().unwrap().parse().unwrap();
    let Object::Stream(s) = &doc.get(ObjId::new(num, 0)).unwrap().value else {
        panic!("hint stream not a stream")
    };
    let data =
        crate::filters::decode_stream(&s.dict, s.data_span.slice(doc.bytes()).unwrap()).unwrap();
    let mut r = BitReader {
        data: &data,
        bit: 0,
    };
    let min_n = r.read(32);
    let first = r.read(32) as usize;
    let bits_n = r.read(16);
    let min_len = r.read(32);
    let bits_len = r.read(16);
    for width in [32, 16, 32, 16, 16, 16, 16, 16] {
        r.read(width);
    }
    let pages = kids(&doc);
    let counts: Vec<u64> = pages.iter().map(|_| min_n + r.read(bits_n)).collect();
    r.align();
    let lengths: Vec<usize> = pages
        .iter()
        .map(|_| (min_len + r.read(bits_len)) as usize)
        .collect();
    // F.4: hint offsets are counted as if the hint stream were absent, and
    // every page follows it.
    let mut start = first + hint_len;
    for (i, page) in pages.iter().enumerate() {
        assert_eq!(start, offset(&out, *page) + 1, "page {i} offset");
        let part = &out[start..start + lengths[i]];
        let objects = part.windows(7).filter(|w| w == b" 0 obj\n").count();
        assert_eq!(objects as u64, counts[i], "page {i} object count");
        start += lengths[i];
    }
}
