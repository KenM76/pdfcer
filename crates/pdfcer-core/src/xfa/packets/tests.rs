#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // a panic here is the test failing

use super::*;
use crate::pageops::tests_support::build_pdf;

fn stream(body: &str) -> String {
    format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len())
}

fn doc_with(xfa: &str, extra: &[(u32, String)]) -> crate::document::Document {
    let catalog =
        format!("<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [] /XFA {xfa} >> >>");
    let mut objects: Vec<(u32, String)> = vec![
        (1, catalog),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>".to_owned(),
        ),
    ];
    objects.extend_from_slice(extra);
    let refs: Vec<(u32, &str)> = objects.iter().map(|(n, b)| (*n, b.as_str())).collect();
    build_pdf(&refs)
}

const XDP: &str = r#"<?xml version="1.0"?><xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform/></template><xfa:datasets xmlns:xfa="http://www.xfa.org/schema/xfa-data/1.0/"/></xdp:xdp>"#;

#[test]
fn a_single_stream_splits_into_its_packets() {
    let doc = doc_with("4 0 R", &[(4, stream(XDP))]);
    let xdp = read_xdp(&doc.view()).unwrap().unwrap();
    let names: Vec<&str> = xdp.packets().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["template", "datasets"]);
    assert!(
        xdp.packets()[0]
            .namespace
            .as_deref()
            .unwrap()
            .starts_with("http://www.xfa.org/schema/xfa-template/")
    );
    assert_eq!(
        xdp.packet_text("template"),
        Some(
            r#"<template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform/></template>"#
        )
    );
}

#[test]
fn an_array_concatenates_and_trusts_the_element_over_the_label() {
    let (a, rest) = XDP.split_at(XDP.find("<template").unwrap());
    let (b, c) = rest.split_at(rest.find("<xfa:datasets").unwrap());
    let doc = doc_with(
        "[(preamble) 4 0 R (config) 5 0 R (postamble) 6 0 R]",
        &[(4, stream(a)), (5, stream(b)), (6, stream(c))],
    );
    let xdp = read_xdp(&doc.view()).unwrap().unwrap();
    assert!(xdp.packet_text("template").is_some());
    assert!(xdp.packet_text("config").is_none());
}

#[test]
fn no_xfa_layer_reads_as_none() {
    let doc = build_pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
    ]);
    assert_eq!(read_xdp(&doc.view()), Ok(None));
}

#[test]
fn a_non_stream_packet_is_malformed() {
    let doc = doc_with("[(template) 42]", &[]);
    assert!(matches!(read_xdp(&doc.view()), Err(XfaError::Malformed(_))));
}

#[test]
fn a_dtd_is_refused() {
    let text = r#"<!DOCTYPE x [<!ENTITY a "aaaa">]><xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/">&a;</xdp:xdp>"#;
    assert!(matches!(parse_xdp(text.to_owned()), Err(XfaError::Xml(_))));
}

#[test]
fn deep_nesting_is_refused_before_parsing() {
    let n = MAX_XML_DEPTH + 1;
    let text = format!("{}{}", "<a>".repeat(n), "</a>".repeat(n));
    assert_eq!(parse_xdp(text), Err(XfaError::TooDeep));
}

#[test]
fn a_root_that_is_not_xdp_is_one_packet() {
    let xdp = parse_xdp("<template><subform/></template>".to_owned()).unwrap();
    assert_eq!(xdp.packets().len(), 1);
    assert_eq!(xdp.packets()[0].name, "template");
}

#[test]
fn utf16_with_a_bom_decodes() {
    let mut bytes = vec![0xFF, 0xFE];
    for u in "<template/>".encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    assert_eq!(text_of(&bytes).unwrap(), "<template/>");
    assert_eq!(text_of(&[0xC0, 0x80]), Err(XfaError::NotText));
}

#[test]
fn depth_skips_comments_quotes_and_self_closing_tags() {
    assert_eq!(nesting_depth("<a><b/><c x='>'><!-- <d><e> --></c></a>"), 2);
    assert_eq!(nesting_depth("<a><![CDATA[<b><c>]]><?pi <d>?></a>"), 1);
    assert_eq!(nesting_depth(""), 0);
}
