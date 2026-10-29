//! An isolated transparency group blends in its own declared `/CS` and
//! converts only its result into the parent's space (ISO 32000-2 §11.3.4,
//! §11.4.7 Table 147); a non-isolated one ignores `/CS` (§11.6.6).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::rgb_into_a_cmyk_group::build;
use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

/// Magenta under `Difference` black: green when blended in ink.
const INK_DIFF: &str = "0 1 0 0 k 50 50 100 100 re f /D gs 0 0 0 1 k 50 50 100 100 re f";
/// Blue under `Difference` yellow: white when blended on screen.
const RGB_DIFF: &str = "0 0 1 rg 50 50 100 100 re f /D gs 1 1 0 rg 50 50 100 100 re f";

fn stream(body: &str, dict: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{body}\nendstream",
        body.len()
    )
}

fn form(body: &str, group: &str, inner: &str) -> String {
    stream(
        body,
        &format!(
            "/Type /XObject /Subtype /Form /BBox [0 0 200 200] {group} \
             /Resources << /ExtGState << /D << /BM /Difference >> >> {inner} >>"
        ),
    )
}

/// A one-page document: `page_group` on the page, `content` its stream,
/// `/X` and `/Y` its forms.
fn doc(page_group: &str, content: &str, x: &str, y: Option<&str>) -> Vec<u8> {
    let content = stream(content, "");
    let mut objs = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] >>".to_owned(),
        ),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /Contents 4 0 R {page_group} \
                 /Resources << /XObject << /X 5 0 R >> \
                 /ExtGState << /D << /BM /Difference >> /H << /ca 0.5 >> >> >> >>"
            ),
        ),
        (4, content),
        (5, x.to_owned()),
    ];
    if let Some(y) = y {
        objs.push((6, y.to_owned()));
    }
    let objs: Vec<(u32, &str)> = objs.iter().map(|(n, s)| (*n, s.as_str())).collect();
    build(&objs)
}

fn render(bytes: Vec<u8>) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    render_page_with(&doc, &p, 1.0, &RenderOptions::default()).expect("render")
}

fn px(page: &RenderedPage, x: u32, y: u32) -> [i32; 3] {
    let p = page.pixmap.pixel(x, y).expect("in bounds").demultiply();
    [p.red().into(), p.green().into(), p.blue().into()]
}

fn near(got: [i32; 3], want: [i32; 3], tol: i32, label: &str) {
    assert!(
        got.iter().zip(want).all(|(g, w)| (g - w).abs() <= tol),
        "{label}: got {got:?}, want {want:?}"
    );
}

const CMYK_PAGE: &str = "/Group << /S /Transparency /CS /DeviceCMYK >>";
const ISO_CMYK: &str = "/Group << /S /Transparency /I true /CS /DeviceCMYK >>";
const ISO_RGB: &str = "/Group << /S /Transparency /I true /CS /DeviceRGB >>";

/// The same ink content painted directly on a page whose group is CMYK:
/// the colour an ink-blended group must reproduce.
fn ink_reference() -> [i32; 3] {
    let plain = form(INK_DIFF, "", "");
    px(&render(doc(CMYK_PAGE, "/X Do", &plain, None)), 100, 100)
}

#[test]
fn a_cmyk_group_on_an_rgb_page_blends_in_ink() {
    let want = ink_reference();
    assert!(
        want[1] > 120 && want[0] < 60,
        "reference is green: {want:?}"
    );
    let x = form(INK_DIFF, ISO_CMYK, "");
    for page_group in ["", "/Group << /S /Transparency /CS /DeviceRGB >>"] {
        let page = render(doc(page_group, "/X Do", &x, None));
        near(px(&page, 100, 100), want, 2, page_group);
        assert_eq!(page.diagnostics.blends_in_wrong_space, 0, "{page_group}");
    }
}

#[test]
fn an_rgb_group_on_a_cmyk_page_blends_on_screen() {
    let x = form(RGB_DIFF, ISO_RGB, "");
    let page = render(doc(CMYK_PAGE, "/X Do", &x, None));
    near(px(&page, 100, 100), [255, 255, 255], 2, "white");
}

#[test]
fn a_cmyk_group_nested_in_an_rgb_group_blends_in_ink() {
    let y = form(INK_DIFF, ISO_CMYK, "");
    let x = form("/Y Do", ISO_RGB, "/XObject << /Y 6 0 R >>");
    let page = render(doc("", "/X Do", &x, Some(&y)));
    near(px(&page, 100, 100), ink_reference(), 2, "nested");
}

#[test]
fn a_non_isolated_group_ignores_its_cs() {
    let plain = form(INK_DIFF, "", "");
    let want = px(&render(doc("", "/X Do", &plain, None)), 100, 100);
    let x = form(
        INK_DIFF,
        "/Group << /S /Transparency /CS /DeviceCMYK >>",
        "",
    );
    let page = render(doc("", "/X Do", &x, None));
    near(px(&page, 100, 100), want, 0, "inherits the page's space");
}

#[test]
fn a_page_already_in_ink_is_unchanged_by_restating_it() {
    let plain = form(INK_DIFF, "/Group << /S /Transparency /I true >>", "");
    let want = px(&render(doc(CMYK_PAGE, "/X Do", &plain, None)), 100, 100);
    let x = form(INK_DIFF, ISO_CMYK, "");
    let got = px(&render(doc(CMYK_PAGE, "/X Do", &x, None)), 100, 100);
    near(got, want, 0, "restated");
}

/// A knockout group holding a CMYK form group and then a half-opaque cyan
/// square: where they overlap only the cyan shows over the white page.
#[test]
fn a_knockout_group_still_knocks_out_an_ink_group() {
    let y = form(INK_DIFF, ISO_CMYK, "");
    let x = form(
        "/Y Do /H gs 0 1 1 rg 100 0 100 200 re f",
        "/Group << /S /Transparency /I true /K true >>",
        "/XObject << /Y 6 0 R >> /ExtGState << /H << /ca 0.5 >> >>",
    );
    let page = render(doc("", "/X Do", &x, Some(&y)));
    let cyan_alone = px(&page, 170, 100);
    near(
        px(&page, 120, 100),
        cyan_alone,
        1,
        "overlap shows only cyan",
    );
    near(
        px(&page, 70, 100),
        ink_reference(),
        2,
        "ink group outside overlap",
    );
}
