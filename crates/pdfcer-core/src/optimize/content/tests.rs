#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::rewrite::compact;
use super::*;
use crate::content::ContentStream;
use crate::optimize::tests::{build, id, session};
use crate::optimize::{OptimizePlan, OptimizeSettings, OptimizedFile, RebuildOptions};
use crate::space_audit::tests::stream;
use crate::writer::SaveOptions;

fn settings(whitespace: Whitespace, numbers: Numbers, redundant: bool) -> ResolvedContent {
    ResolvedContent {
        whitespace,
        numbers,
        redundant,
        round: None,
        start: Scale::new(1.0),
        encode: StreamAction::Recompress { level: 9 },
    }
}

/// `src` compacted with the defaults; the text and (removed, shortened).
fn squeeze(src: &[u8]) -> (String, u64, u64) {
    squeeze_with(src, settings(Whitespace::Minimal, Numbers::Shortest, true))
}

fn squeeze_with(src: &[u8], s: ResolvedContent) -> (String, u64, u64) {
    let out = compact(src, s).unwrap();
    (
        String::from_utf8(out.bytes).unwrap(),
        out.removed,
        out.shortened,
    )
}

#[test]
fn patches_parse_and_print_their_spelling() {
    let p = ContentPatch::parse("content(whitespace=keep, redundant=off,reset)").unwrap();
    assert!(p.reset);
    assert_eq!(p.whitespace, Some(Whitespace::Keep));
    assert_eq!(p.redundant, Some(false));
    assert_eq!(p.numbers, None);
    assert_eq!(
        p.to_string(),
        "content(redundant=off,reset,whitespace=keep)"
    );
    assert_eq!(ContentPatch::parse(&p.to_string()).unwrap(), p);
    assert_eq!(ContentPatch::new().to_string(), "content()");
    for bad in [
        "content(whitespace=tight)",
        "content(numbers=keep,numbers=keep)",
        "content(colour=on)",
        "content(redundant)",
        "content(",
    ] {
        assert!(ContentPatch::parse(bad).is_err(), "{bad}");
    }
    let action = StreamAction::parse("content(numbers=keep)").unwrap();
    assert!(!action.is_lossy());
    assert_eq!(action.to_profile_string(), "content(numbers=keep)");
    assert!(
        StreamAction::parse("contents()")
            .unwrap_err()
            .contains("content(...)")
    );
}

#[test]
fn patches_merge_and_a_whole_state_action_clears_them() {
    let p = |t| {
        (
            StreamAction::parse(t).unwrap(),
            crate::optimize::SettingSource::Streams,
        )
    };
    let r = crate::optimize::image::fold([
        p("brotli-5"),
        p("content(whitespace=keep)"),
        p("content(numbers=keep)"),
    ]);
    let c = r.content.unwrap();
    assert_eq!(
        (c.whitespace, c.numbers, c.redundant),
        (Whitespace::Keep, Numbers::Keep, true)
    );
    assert_eq!(c.encode, StreamAction::Brotli { quality: 5 });
    let r = crate::optimize::image::fold([p("content(whitespace=keep)"), p("content(reset)")]);
    assert_eq!(r.content.unwrap().whitespace, Whitespace::Minimal);
    assert_eq!(
        r.content.unwrap().encode,
        StreamAction::Recompress { level: 9 }
    );
    let r = crate::optimize::image::fold([p("content()"), p("recompress-3")]);
    assert_eq!(r.content, None);
    assert_eq!(r.base, StreamAction::Recompress { level: 3 });
}

#[test]
fn minimal_whitespace_keeps_only_the_separators_a_reader_needs() {
    let (out, removed, shortened) = squeeze(b"  0.500 0.50 m\n% note\n 100.0 +7 l S\n");
    assert_eq!(out, "\n.5 .5 m\n100 7 l\nS\n");
    assert_eq!((removed, shortened), (0, 4));
    let (out, ..) = squeeze(b"/F1 12 Tf ( a  b ) Tj [ 1 2 ] 0 d <</MCID 0>> BDC EMC");
    assert_eq!(out, "/F1 12 Tf( a  b )Tj[1 2]0 d<</MCID 0>>BDC\nEMC");
}

#[test]
fn numbers_and_separators_inside_arrays_and_dictionaries_are_respelled() {
    let src =
        b"[ (a  b) -120.0 <41> 0.50 [ 3.0 ] ] TJ [ 2.000 1.0 ] 0.0 d /P << /MCID 007 /S /X >> BDC";
    let (out, _, shortened) = squeeze(src);
    assert_eq!(out, "[(a  b)-120<41>.5[3]]TJ[2 1]0 d/P<</MCID 7/S/X>>BDC");
    assert_eq!(shortened, 7);
    let keep = settings(Whitespace::Keep, Numbers::Shortest, true);
    let (out, _, shortened) = squeeze_with(b"[ 2.000  1.0 ] 0 d", keep);
    assert_eq!(out, "[ 2  1 ] 0 d");
    assert_eq!(shortened, 2);
    let spaces = settings(Whitespace::Minimal, Numbers::Keep, true);
    assert_eq!(squeeze_with(b"[ 2.0 1.0 ] 0 d", spaces).0, "[2.0 1.0]0 d");
}

#[test]
fn numbers_take_their_shortest_spelling_of_the_same_value() {
    let (out, ..) = squeeze(b"-0.0 1.0 -0.25 0.0001 0.0000001 007 1.5 m");
    assert_eq!(out, "0 1 -.25 .0001 0.0000001 7 1.5 m");
    let keep = settings(Whitespace::Minimal, Numbers::Keep, true);
    assert_eq!(squeeze_with(b"1.0 0.50 m", keep).0, "1.0 0.50 m");
}

#[test]
fn keep_whitespace_drops_only_the_gaps_of_removed_operators() {
    let s = settings(Whitespace::Keep, Numbers::Keep, true);
    let (out, removed, _) = squeeze_with(b"q Q 0 g  0 g\n5 w % kept\n", s);
    assert_eq!(out, " 0 g\n5 w % kept\n");
    assert_eq!(removed, 3);
}

#[test]
fn redundant_operators_go_and_the_ones_that_change_state_stay() {
    // The inner setter repeats the saved state, which empties the q/Q pair,
    // and the state after Q is the one saved.
    assert_eq!(squeeze(b"0 g q 0 g Q 0 g").0, "0 g");
    assert_eq!(squeeze(b"0.5 g .50 g").0, ".5 g");
    assert_eq!(
        squeeze(b"1 0 0 1 0 0 cm 2 0 0 2 0 0 cm").0,
        "2 0 0 2 0 0 cm"
    );
    for kept in [
        "0 g 1 g 0 g",
        "1 w /GS0 gs 1 w",
        "0 Tc 1 2 (a) \" 0 Tc",
        "BT 0 0 Td ET",
        "/CS0 cs 1 scn /CS0 cs 1 scn",
    ] {
        let (out, removed, _) = squeeze(kept.as_bytes());
        assert_eq!(removed, 0, "{kept}: {out}");
    }
    // A repeat inside q/Q is a repeat of the saved state; drawing keeps the
    // pair.
    assert_eq!(squeeze(b"1 w q 1 w 0 0 m 1 w Q").0, "1 w\nq\n0 0 m\nQ");
    // A `q` with operands still saves a level, so `Q` restores `1 g`.
    let (out, ..) = squeeze(b"0.5 g q 1 g 7 q 0 g Q 0.5 g");
    assert_eq!(out.matches(".5 g").count(), 2, "{out}");
    // Text state persists across BT/ET.
    assert_eq!(
        squeeze(b"BT /F1 9 Tf ET BT /F1 9 Tf ET").0,
        "BT/F1 9 Tf\nET\nBT\nET"
    );
}

#[test]
fn an_inline_image_is_copied_and_followed_by_white_space() {
    let src = b"q BI /W 1 /H 1 /BPC 8 /CS /G ID \x07 EI Q";
    let out = compact(src, settings(Whitespace::Minimal, Numbers::Shortest, true)).unwrap();
    let image = &src[2..src.len() - 2];
    let at = out.bytes.windows(image.len()).position(|w| w == image);
    assert!(at.is_some(), "{:?}", String::from_utf8_lossy(&out.bytes));
    assert!(out.bytes.ends_with(b"EI\nQ"));
}

#[test]
fn the_check_refuses_output_that_reads_differently() {
    let original = ContentStream::parse(b"0 g 1 w".to_vec()).unwrap();
    let keep = vec![true; original.tokens.len()];
    let none = vec![None; original.tokens.len()];
    let all = (&keep[..], &none[..]);
    assert!(super::check::same(&original, all, b"0 g 1 w"));
    assert!(super::check::same(&original, all, b"0.0 g 1.0 w"));
    assert!(!super::check::same(&original, all, b"0 G 1 w"));
    assert!(!super::check::same(&original, all, b"0 g 1 w n"));
    assert!(!super::check::same(&original, all, b"0 g"));
    assert!(!super::check::same(&original, all, b"0 g 2 w"));
    assert!(!super::check::same(&original, all, b"0 g (1) w"));
}

fn page_doc(page_content: &[u8]) -> Vec<u8> {
    let other = noise_text();
    build(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R \
           /Resources << /XObject << /Fm1 5 0 R >> /Pattern << /P1 6 0 R >> >> >>"
            .to_vec(),
        stream("", page_content),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 10 10]",
            // string-gap-exempt: separators for the rewrite to remove.
            &b"0.000 0.000 m    10.000 10.000 l   S\n".repeat(40),
        ),
        stream(
            "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 4 4] /XStep 4 /YStep 4",
            &b"1.0 0.0 0.0 rg   0 0 4 4 re f\n".repeat(40),
        ),
        stream("", &other),
    ])
}

/// A compressible stream that is not content.
fn noise_text() -> Vec<u8> {
    b"0.000 0.000 m    10.000 10.000 l   S\n".repeat(40) // string-gap-exempt: compressible filler
}

fn page_content() -> Vec<u8> {
    b"q 1.0 0 0 1.0 0 0 cm   0.500 g  0.500 g\n0.000 0.000 m 100.000 100.000 l S Q\n".repeat(50)
}

fn plan(bytes: Vec<u8>, settings: &OptimizeSettings) -> (crate::edit::EditSession, OptimizePlan) {
    let mut s = session(bytes);
    let plan = s.plan_optimize(settings).unwrap();
    s.apply_optimize(&plan).unwrap();
    (s, plan)
}

fn decoded_after(s: &crate::edit::EditSession, plan: &OptimizePlan, n: u32) -> Vec<u8> {
    let (file, _) = s
        .save_optimized(plan, &SaveOptions::default(), &RebuildOptions::none())
        .unwrap();
    let (OptimizedFile::Smaller(bytes) | OptimizedFile::NotSmaller { bytes, .. }) = file else {
        panic!("expected a written file");
    };
    let doc = crate::document::Document::from_bytes(bytes).unwrap();
    let Some(crate::object::Object::Stream(st)) = doc.get(id(n)).map(|o| o.value.clone()) else {
        panic!("object {n} is not a stream");
    };
    let raw = doc.bytes().get(st.data_span.range()).unwrap().to_vec();
    crate::filters::decode_stream(&st.dict, &raw).unwrap()
}

#[test]
fn a_plan_compacts_page_form_and_pattern_content_and_nothing_else() {
    let mut settings = OptimizeSettings::new();
    settings.content = Some(ContentPatch::new());
    let (s, plan) = plan(page_doc(&page_content()), &settings);
    let ids: Vec<u32> = plan.changes.iter().map(|c| c.id.num).collect();
    assert_eq!(ids, [4, 5, 6]);
    let page = plan.changes[0].content.unwrap();
    assert_eq!(page.operators_removed, 50 * 2);
    assert!(page.decoded_after < page.decoded_before);
    assert!(!plan.changes[0].lossy);
    assert!(matches!(plan.changes[0].action, StreamAction::Content(_)));
    let after = String::from_utf8(decoded_after(&s, &plan, 4)).unwrap();
    assert_eq!(after, "q\n.5 g\n0 0 m\n100 100 l\nS\nQ\n".repeat(50));
    assert_eq!(
        decoded_after(&s, &plan, 5),
        b"0 0 m\n10 10 l\nS\n".repeat(40)
    );
}

#[test]
fn an_override_compacts_one_stream_and_leave_after_content_keeps_it() {
    let mut settings = OptimizeSettings::new();
    settings.objects.insert(
        id(5),
        StreamAction::parse("content(whitespace=keep)").unwrap(),
    );
    let (_, p) = plan(page_doc(&page_content()), &settings);
    let ids: Vec<u32> = p.changes.iter().map(|c| c.id.num).collect();
    assert_eq!(ids, [5]);
    assert_eq!(p.changes[0].content.unwrap().numbers_shortened, 40 * 4);

    let mut settings = OptimizeSettings::new();
    settings.content = Some(ContentPatch::new());
    settings.objects.insert(id(4), StreamAction::Leave);
    let (_, p) = plan(page_doc(&page_content()), &settings);
    assert!(p.changes.iter().all(|c| c.id.num != 4));
}

#[test]
fn compaction_that_saves_nothing_is_kept_as_stored() {
    let mut settings = OptimizeSettings::new();
    settings.content = Some(ContentPatch::new());
    let (_, p) = plan(page_doc(b"0 g"), &settings);
    assert!(p.changes.iter().all(|c| c.id.num != 4));
    assert!(p.skipped.iter().any(
        |k| k.id == id(4) && matches!(k.reason, crate::optimize::SkipReason::NotSmaller { .. })
    ));
}
