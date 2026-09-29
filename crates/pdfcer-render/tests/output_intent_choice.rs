//! The blending space and the destination profile come from the SAME output
//! intent: the first whose `/DestOutputProfile` stream has an `/N` and
//! decodes (§14.11.5; `OI-A1` leaves the choice to the reader). An intent
//! with a profile that will not decode is skipped for both.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{BlendSpaceFrom, RenderOptions, page_composites_in_ink};

/// A profile stream with `/N n`: `broken` gives it a `/FlateDecode` filter
/// over bytes that are not a zlib stream.
fn profile(n: u8, broken: bool) -> String {
    let data = "not-a-real-icc-profile";
    let filter = if broken { "/Filter /FlateDecode " } else { "" };
    format!(
        "<< /N {n} {filter}/Length {} >>\nstream\n{data}\nendstream",
        data.len()
    )
}

/// One page with no `/Group`, and two output intents over objects 5 and 6.
fn doc(first: &str, second: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R /OutputIntents [\
         << /Type /OutputIntent /S /GTS_PDFX /OutputConditionIdentifier (A) /DestOutputProfile 5 0 R >> \
         << /Type /OutputIntent /S /GTS_PDFX /OutputConditionIdentifier (B) /DestOutputProfile 6 0 R >>] >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>".to_owned(),
        "<< /Length 0 >>\nstream\n\nendstream".to_owned(),
        first.to_owned(),
        second.to_owned(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    let n = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

fn in_ink(first: &str, second: &str) -> (bool, BlendSpaceFrom) {
    let d = Document::from_bytes(doc(first, second)).unwrap();
    let pages = page_tree::pages(&d).unwrap();
    let ink = page_composites_in_ink(&d.view(), &pages[0], &RenderOptions::default());
    (ink.composites_in_ink, ink.source)
}

#[test]
fn a_cmyk_intent_whose_profile_will_not_decode_does_not_make_the_page_ink() {
    let (ink, _) = in_ink(&profile(4, true), &profile(3, false));
    assert!(!ink, "the chosen intent is the second, RGB one");
}

#[test]
fn a_broken_rgb_intent_ahead_of_a_cmyk_one_leaves_the_cmyk_one_in_charge() {
    let (ink, from) = in_ink(&profile(3, true), &profile(4, false));
    assert!(ink);
    assert_eq!(from, BlendSpaceFrom::OutputIntent);
}

#[test]
fn the_first_usable_intent_wins_when_both_decode() {
    assert!(in_ink(&profile(4, false), &profile(3, false)).0);
    assert!(!in_ink(&profile(3, false), &profile(4, false)).0);
}
