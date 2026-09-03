//! # `gs` works when the ExtGState is an indirect reference
//!
//! ISO 32000-1 §7.3.10 lets **any** value in any dictionary be an
//! indirect reference, and real producers overwhelmingly write
//! `/ExtGState << /GS0 12 0 R >>` rather than inlining the dictionary.
//!
//! Until 2026-08-08 `interpret::apply_ext_gstate` resolved neither the
//! `/ExtGState` sub-dictionary nor the named entry inside it. `as_dict()`
//! on a `Reference` returns `None`, so the lookup collapsed to the
//! "tolerated" arm and **`gs` was a silent no-op on essentially every
//! real file** — no line width, no cap, no join, no dash, and no
//! diagnostic an operator would read as "the graphics state you asked
//! for was ignored".
//!
//! Every other resource lookup in that file already resolved (`/Font`,
//! `/XObject`), which is what makes this a slip rather than a policy —
//! and what makes a regression here easy to reintroduce.
//!
//! Found by the benign-bucket audit, not by a test: the defect lived
//! inside `render-parity`'s `benign-renderer-noise` bucket, on a page
//! whose divergence was below the threshold that bucket is defined by.
//! Both halves of that are the lesson — the bug was invisible to the
//! oracle, and the oracle's own name said the difference did not matter.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

/// Build an offset-consistent classic PDF from `(number, body)` pairs.
fn build(objects: &[(u32, &str)]) -> Vec<u8> {
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

/// A page that strokes one horizontal line, taking its width from a `gs`.
///
/// `resources` and `gs_object` are supplied by the caller so the same page
/// can be built with the ExtGState inline or behind a reference — the
/// whole point of the comparison.
fn page_with(resources: &str, gs_object: Option<(u32, &str)>) -> Vec<u8> {
    // `1 w` first, so a `gs` that does nothing leaves a HAIRLINE. The
    // assertion then distinguishes "the setting was applied" from "the
    // default happened to look similar".
    let content = "1 w /GS0 gs 0 g 10 50 m 90 50 l S\n";
    let mut objects: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] \
                 /Resources {resources} >>"
            ),
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_owned(),
        ),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            ),
        ),
    ];
    if let Some((num, body)) = gs_object {
        objects.push((num, body.to_owned()));
    }
    let borrowed: Vec<(u32, &str)> = objects.iter().map(|(n, b)| (*n, b.as_str())).collect();
    build(&borrowed)
}

fn render(bytes: Vec<u8>) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let page = page_tree::pages(&doc).expect("page tree").remove(0);
    render_page_with(&doc, &page, 1.0, &RenderOptions::default()).expect("render")
}

/// How many pixels of the stroked row are dark — a proxy for stroke width
/// that does not depend on exact anti-aliasing.
fn dark_pixels(rendered: &RenderedPage) -> usize {
    let pixmap = &rendered.pixmap;
    (0..pixmap.height())
        .flat_map(|y| (0..pixmap.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            pixmap
                .pixel(x, y)
                .is_some_and(|p| p.demultiply().red() < 128)
        })
        .count()
}

#[test]
fn an_indirect_ext_gstate_is_honoured_exactly_like_an_inline_one() {
    // THE REGRESSION TEST. These two documents differ in one way only:
    // whether the ExtGState dictionary is written inline or behind a
    // reference. §7.3.10 says that difference must not be observable, and
    // for two years of this project's life it was the difference between
    // an 8-unit stroke and a hairline.
    let inline = render(page_with("<< /ExtGState << /GS0 << /LW 8 >> >> >>", None));
    let indirect = render(page_with(
        "<< /ExtGState << /GS0 5 0 R >> >>",
        Some((5, "<< /LW 8 >>")),
    ));

    assert_eq!(
        dark_pixels(&inline),
        dark_pixels(&indirect),
        "an ExtGState behind an indirect reference must render identically \
         to the same dictionary written inline"
    );
}

/// And the whole `/ExtGState` sub-dictionary may itself be indirect —
/// the outer lookup was unresolved too, so this is a genuinely separate
/// failure path rather than a restatement of the test above.
#[test]
fn an_indirect_ext_gstate_subdictionary_is_also_honoured() {
    let inline = render(page_with("<< /ExtGState << /GS0 << /LW 8 >> >> >>", None));
    let indirect = render(page_with(
        "<< /ExtGState 5 0 R >>",
        Some((5, "<< /GS0 << /LW 8 >> >>")),
    ));

    assert_eq!(
        dark_pixels(&inline),
        dark_pixels(&indirect),
        "an indirect /ExtGState sub-dictionary must resolve too"
    );
}

/// The control that gives the two tests above their meaning.
///
/// Without this, both would pass on a renderer that ignored `gs`
/// completely — two identical no-ops compare equal. This pins that
/// `/LW 8` actually paints more than the `1 w` already in the content
/// stream, so "identical" above means "identically applied", not
/// "identically ignored".
#[test]
fn the_width_from_the_ext_gstate_really_does_something() {
    let without = render(page_with("<< >>", None));
    let with = render(page_with("<< /ExtGState << /GS0 << /LW 8 >> >> >>", None));
    assert!(
        dark_pixels(&with) > dark_pixels(&without) * 3,
        "an 8-unit stroke must be far heavier than the 1-unit default \
         ({} vs {})",
        dark_pixels(&with),
        dark_pixels(&without)
    );
}
