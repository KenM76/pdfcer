#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;
use crate::document::Document;
use crate::edit::EditSession;
use crate::object::Object;
use crate::space_audit::tests::stream;
use crate::writer::SaveOptions;

/// A classic-xref file whose object `i + 1` is `bodies[i]`.
fn build(bodies: &[Vec<u8>]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fn content() -> Vec<u8> {
    b"0 0 m 100 100 l S\n".repeat(200)
}

fn hex(data: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = data
        .iter()
        .flat_map(|b| format!("{b:02X}").into_bytes())
        .collect();
    out.push(b'>');
    out
}

/// 4: plain content; 5: ASCIIHex form; 6: tiny content; 7: XMP; 8: JPEG.
fn sample(catalog_extra: &str, extra: &[Vec<u8>]) -> Vec<u8> {
    let mut bodies = vec![
        format!("<< /Type /Catalog /Pages 2 0 R /Metadata 7 0 R {catalog_extra} >>").into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents [4 0 R 6 0 R] \
           /Resources << /XObject << /Fm1 5 0 R /Im1 8 0 R >> >> >>"
            .to_vec(),
        stream("", &content()),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Filter /ASCIIHexDecode",
            &hex(&content()),
        ),
        stream("", b"q Q"),
        stream("/Type /Metadata /Subtype /XML", &b"<x:xmpmeta/>".repeat(50)),
        stream(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray \
             /BitsPerComponent 8 /Filter /DCTDecode",
            &[0xFF; 200],
        ),
    ];
    bodies.extend_from_slice(extra);
    build(&bodies)
}

fn session(bytes: Vec<u8>) -> EditSession {
    EditSession::new(Document::from_bytes(bytes).unwrap())
}

fn id(n: u32) -> ObjId {
    ObjId::new(n, 0)
}

fn decoded(doc: &Document, n: u32) -> Vec<u8> {
    let Some(Object::Stream(s)) = doc.get(id(n)).map(|o| o.value.clone()) else {
        panic!("object {n} is not a stream");
    };
    crate::filters::decode_stream(&s.dict, &raw(doc, n)).unwrap()
}

#[test]
fn a_plan_recompresses_eligible_streams_and_explains_every_skip() {
    let plan = session(sample("", &[]))
        .plan_optimize(&Preset::Standard.settings())
        .unwrap();
    let changed: Vec<u32> = plan.changes.iter().map(|c| c.id.num).collect();
    assert_eq!(changed, [4, 5]);
    assert!(
        plan.changes
            .iter()
            .all(|c| c.after_bytes < c.before_bytes && !c.lossy)
    );
    assert_eq!(plan.changes[1].filters_before, ["ASCIIHexDecode"]);
    let reason = |n: u32| {
        plan.skipped
            .iter()
            .find(|s| s.id.num == n)
            .map(|s| s.reason.clone())
    };
    assert!(matches!(reason(6), Some(SkipReason::NotSmaller { .. })));
    assert_eq!(reason(7), Some(SkipReason::Protected("XMP metadata")));
    assert_eq!(
        reason(8),
        Some(SkipReason::UnsupportedFilter("DCTDecode".into()))
    );
    assert!(plan.stream_saving() > 0);
}

#[test]
fn apply_and_save_keep_decoded_content_and_untouched_bytes() {
    let original = sample("", &[]);
    let before = Document::from_bytes(original.clone()).unwrap();
    let mut s = session(original);
    let plan = s.plan_optimize(&Preset::Lossless.settings()).unwrap();
    assert_eq!(s.apply_optimize(&plan).unwrap(), 2);
    let OptimizedFile::Smaller(bytes) = s
        .save_optimized(
            &SaveOptions::default(),
            &crate::optimize::RebuildOptions::none(),
        )
        .unwrap()
        .0
    else {
        panic!("expected a smaller file");
    };
    let after = Document::from_bytes(bytes).unwrap();
    for n in [4, 5, 7] {
        assert_eq!(decoded(&after, n), decoded(&before, n), "object {n}");
    }
    let Some(Object::Stream(s5)) = after.get(id(5)).map(|o| o.value.clone()) else {
        panic!()
    };
    assert_eq!(
        s5.dict.get(b"Filter"),
        Some(&Object::Name(crate::object::Name::from(b"FlateDecode")))
    );
    assert_eq!(raw(&after, 8), raw(&before, 8));
}

fn raw(doc: &Document, n: u32) -> Vec<u8> {
    let Some(Object::Stream(s)) = doc.get(id(n)).map(|o| o.value.clone()) else {
        panic!("object {n} is not a stream");
    };
    doc.bytes()[s.data_span.start..s.data_span.start + s.data_span.len].to_vec()
}

#[test]
fn the_plan_is_one_undo_entry() {
    let mut s = session(sample("", &[]));
    let plan = s.plan_optimize(&Preset::Standard.settings()).unwrap();
    s.apply_optimize(&plan).unwrap();
    assert!(s.is_modified());
    assert_eq!(
        s.undo(),
        Some(crate::edit::CommandKind::Optimize { count: 2 })
    );
    assert!(!s.is_modified());
}

#[test]
fn an_override_beats_a_rule_and_a_rule_beats_the_default() {
    let mut settings = Preset::Standard.settings();
    settings.rules.push(Rule {
        select: Selector {
            ids: vec![id(4), id(5)],
            ..Selector::new()
        },
        action: StreamAction::Leave,
    });
    settings
        .objects
        .insert(id(5), StreamAction::Recompress { level: 1 });
    let plan = session(sample("", &[])).plan_optimize(&settings).unwrap();
    let changed: Vec<(u32, StreamAction)> =
        plan.changes.iter().map(|c| (c.id.num, c.action)).collect();
    assert_eq!(changed, [(5, StreamAction::Recompress { level: 1 })]);
}

#[test]
fn selectors_match_category_pages_size_and_filter() {
    let facts_plan = |select: Selector| {
        let mut settings = OptimizeSettings::new();
        settings.rules.push(Rule {
            select,
            action: StreamAction::Recompress { level: 9 },
        });
        session(sample("", &[]))
            .plan_optimize(&settings)
            .unwrap()
            .changes
            .iter()
            .map(|c| c.id.num)
            .collect::<Vec<_>>()
    };
    let cat = |c| Selector {
        categories: vec![c],
        ..Selector::new()
    };
    assert_eq!(facts_plan(cat(SpaceCategory::FormXObjects)), [5]);
    assert_eq!(facts_plan(cat(SpaceCategory::ContentStreams)), [4]);
    let pages = |p: &str| Selector {
        pages: Some(PageRanges::parse(p).unwrap()),
        ..Selector::new()
    };
    assert_eq!(facts_plan(pages("1")), [4, 5]);
    assert!(facts_plan(pages("2-")).is_empty());
    let filter = |f: &str| Selector {
        filters: vec![f.to_owned()],
        ..Selector::new()
    };
    assert_eq!(facts_plan(filter("none")), [4]);
    assert_eq!(facts_plan(filter("ASCIIHexDecode")), [5]);
    let big = Selector {
        min_bytes: Some(content().len() as u64 + 1),
        ..Selector::new()
    };
    assert_eq!(facts_plan(big), [5]);
}

#[test]
fn a_whole_file_that_is_not_smaller_is_not_written() {
    assert_eq!(
        keep_if_smaller(vec![0; 10], 10),
        OptimizedFile::KeptOriginal {
            original: 10,
            rewritten: 10
        }
    );
    assert_eq!(
        keep_if_smaller(vec![0; 9], 10),
        OptimizedFile::Smaller(vec![0; 9])
    );
}

#[test]
fn a_signed_document_is_refused_unless_allowed() {
    let sig =
        b"<< /Type /Sig /Filter /Adobe.PPKLite /ByteRange [0 0 0 0] /Contents <00> >>".to_vec();
    let field = b"<< /FT /Sig /T (s) /V 9 0 R /Rect [0 0 0 0] /Subtype /Widget >>".to_vec();
    let bytes = sample("/AcroForm << /Fields [10 0 R] >>", &[sig, field]);
    let err = session(bytes.clone())
        .plan_optimize(&Preset::Standard.settings())
        .unwrap_err();
    assert!(matches!(
        &err,
        OptimizeError::Refused(r) if matches!(r.as_slice(), [Refusal::Signed { signatures: 1, .. }])
    ));
    let mut settings = Preset::Standard.settings();
    settings.allow_signed = true;
    let plan = session(bytes).plan_optimize(&settings).unwrap();
    assert!(plan.notes.iter().any(|n| n.contains("invalidated")));
}

#[test]
fn an_encrypted_document_is_refused() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/encryption/enc-emptyuser.pdf"
    );
    let doc = Document::from_bytes(std::fs::read(path).unwrap()).unwrap();
    let err = EditSession::new(doc)
        .plan_optimize(&Preset::Standard.settings())
        .unwrap_err();
    assert_eq!(err, OptimizeError::Refused(vec![Refusal::Encrypted]));
}

#[test]
fn a_plan_made_before_another_edit_is_stale() {
    let mut s = session(sample("", &[]));
    let plan = s.plan_optimize(&Preset::Standard.settings()).unwrap();
    s.apply_optimize(&plan).unwrap();
    assert!(matches!(
        s.apply_optimize(&plan),
        Err(OptimizeError::StalePlan { .. })
    ));
}

#[test]
fn actions_and_presets_round_trip_their_names() {
    for p in Preset::ALL {
        assert_eq!(Preset::from_name(p.as_str()), Some(p));
    }
    assert_eq!(Preset::from_name("mobile"), Some(Preset::Screen));
    for a in [StreamAction::Leave, StreamAction::Recompress { level: 3 }] {
        assert_eq!(StreamAction::parse(&a.to_profile_string()), Ok(a));
    }
    assert!(StreamAction::parse("recompress-10").is_err());
    assert_eq!(
        PageRanges::parse("1-3,7,10-").unwrap().to_string(),
        "1-3,7,10-"
    );
    assert!(PageRanges::parse("0").is_err());
    assert!(PageRanges::parse("3-1").is_err());
}

#[test]
fn a_preset_save_drops_an_orphan_and_packs_the_rest() {
    let orphan = stream("", &content());
    let original = sample("", &[orphan]);
    let before = Document::from_bytes(original.clone()).unwrap();
    let mut s = session(original);
    let settings = Preset::Standard.settings();
    let plan = s.plan_optimize(&settings).unwrap();
    s.apply_optimize(&plan).unwrap();
    let (file, report) = s
        .save_optimized(&SaveOptions::default(), &settings.structure)
        .unwrap();
    let report = report.expect("presets rebuild");
    assert_eq!(report.unreferenced, vec![id(9)]);
    assert!(report.object_streams >= 1);
    let OptimizedFile::Smaller(bytes) = file else {
        panic!("expected a smaller file");
    };
    let after = Document::from_bytes(bytes).unwrap();
    // Objects 1-8 keep their numbers: only the last one was dropped.
    for n in [4, 5, 6, 7] {
        assert_eq!(decoded(&after, n), decoded(&before, n), "object {n}");
    }
    assert_eq!(
        raw(&after, 8),
        raw(&before, 8),
        "the JPEG is copied as stored"
    );
    // Number 9 is free for the first object stream.
    let Some(Object::Stream(objstm)) = after.get(id(9)).map(|o| o.value.clone()) else {
        panic!("object 9 is the object stream");
    };
    assert_eq!(
        objstm.dict.get(b"Type"),
        Some(&Object::Name(b"ObjStm".into()))
    );
}
