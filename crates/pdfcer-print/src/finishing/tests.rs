// Test code: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::print_schema::{Capabilities, PSF, PSK, delta_ticket, dropped, parse_capabilities};
use super::*;

fn ask(
    staple: Option<Staple>,
    punch: Option<Edge>,
    holes: Option<u8>,
    fold: Option<Fold>,
) -> Finishing {
    Finishing {
        staple,
        punch,
        holes,
        fold,
        output_bin: None,
    }
}

#[test]
fn registry_values() {
    assert_eq!(Staple::TopLeft.ipp(), 20);
    assert_eq!(Staple::TopRight.ipp(), 22);
    assert_eq!(Staple::DualBottom.ipp(), 31);
    assert_eq!(Staple::Saddle.ipp(), 8);
    assert_eq!(Fold::Half.ipp(), 93);
    assert_eq!(Fold::EngineeringZ.ipp(), 101);
    assert_eq!(finishing_name(75), Some("punch-dual-top"));
    assert_eq!(finishing_name(89), Some("punch-multiple-bottom"));
    assert_eq!(finishing_name(20), Some("staple-top-left"));
    assert_eq!(finishing_name(64), None);
}

#[test]
fn unknown_support_sends_exact_values() {
    let f = ask(Some(Staple::TopLeft), Some(Edge::Left), None, Some(Fold::Z));
    assert_eq!(ipp_finishings(&f, None), Ok(vec![20, 74, 100]));
    let f = ask(None, Some(Edge::Top), Some(3), None);
    assert_eq!(ipp_finishings(&f, None), Ok(vec![79]));
}

#[test]
fn punch_without_count_takes_the_offered_run() {
    let f = ask(None, Some(Edge::Right), None, None);
    assert_eq!(ipp_finishings(&f, Some(&[3, 84])), Ok(vec![84]));
}

#[test]
fn unoffered_requests_are_refused_by_name() {
    let f = ask(Some(Staple::TopLeft), None, None, Some(Fold::Half));
    let err = ipp_finishings(&f, Some(&[3, 4, 20])).unwrap_err();
    assert_eq!(err, vec!["fold Half (fold-half)".to_owned()]);
}

fn capabilities_xml() -> String {
    format!(
        "<psf:PrintCapabilities xmlns:psf=\"{PSF}\" xmlns:psk=\"{PSK}\" xmlns:ns0000=\"http://example.com/drv\">\
         <psf:Feature name=\"psk:JobStapleAllDocuments\">\
         <psf:Option name=\"psk:StapleTopLeft\"/><psf:Option name=\"psk:None\"/></psf:Feature>\
         <psf:Feature name=\"psk:JobHolePunch\"><psf:Option name=\"psk:LeftEdge\"/></psf:Feature>\
         <psf:Feature name=\"psk:JobOutputBin\"><psf:Option name=\"ns0000:Tray1\"/></psf:Feature>\
         <psf:Feature name=\"psk:PageMediaSize\"><psf:Option name=\"psk:ISOA4\"/></psf:Feature>\
         </psf:PrintCapabilities>"
    )
}

#[test]
fn capabilities_read_staple_punch_and_bins() {
    let caps = parse_capabilities(&capabilities_xml()).unwrap();
    assert_eq!(caps.staples, vec![Staple::TopLeft]);
    assert_eq!(caps.punch, vec![Edge::Left]);
    assert_eq!(caps.bins.len(), 1);
    assert_eq!(caps.bins[0].name, "Tray1");
    assert_eq!(caps.bins[0].namespace, "http://example.com/drv");
}

#[test]
fn ticket_carries_offered_options_and_parses() {
    let caps = parse_capabilities(&capabilities_xml()).unwrap();
    let mut f = ask(Some(Staple::TopLeft), Some(Edge::Left), None, None);
    f.output_bin = Some("tray1".into());
    let ticket = delta_ticket(&f, &caps).unwrap().unwrap();
    let doc = roxmltree::Document::parse(&ticket).unwrap();
    let opts: Vec<_> = doc
        .descendants()
        .filter(|n| n.tag_name().name() == "Option")
        .map(|n| n.attribute("name").unwrap().to_owned())
        .collect();
    assert_eq!(opts, ["psk:StapleTopLeft", "psk:LeftEdge", "bin:Tray1"]);
    let bin = doc
        .descendants()
        .find(|n| n.attribute("name") == Some("bin:Tray1"))
        .unwrap();
    assert_eq!(
        bin.lookup_namespace_uri(Some("bin")),
        Some("http://example.com/drv")
    );
}

#[test]
fn ticket_refuses_what_the_schema_or_printer_lacks() {
    let caps = parse_capabilities(&capabilities_xml()).unwrap();
    let f = ask(
        Some(Staple::Saddle),
        Some(Edge::Left),
        Some(3),
        Some(Fold::Z),
    );
    let err = delta_ticket(&f, &caps).unwrap_err();
    assert_eq!(err.len(), 3, "{err:?}");
    assert!(err[0].starts_with("staple Saddle"));
    assert!(err[1].starts_with("punch holes"));
    assert!(err[2].starts_with("fold Z"));
}

#[test]
fn empty_request_needs_no_ticket() {
    assert_eq!(
        delta_ticket(&Finishing::default(), &Capabilities::default()),
        Ok(None)
    );
    assert!(Finishing::default().is_empty());
}

#[test]
fn malformed_capabilities_are_an_error() {
    assert!(parse_capabilities("<psf:PrintCapabilities").is_err());
}

/// A capabilities document (or ticket) listing one psk feature with one
/// option.
fn only(feature: &str, option: &str) -> String {
    format!(
        "<psf:PrintCapabilities xmlns:psf=\"{PSF}\" xmlns:psk=\"{PSK}\">\
         <psf:Feature name=\"psk:{feature}\"><psf:Option name=\"psk:{option}\"/></psf:Feature>\
         </psf:PrintCapabilities>"
    )
}

/// The feature names a delta ticket carries.
fn ticket_features(ticket: &str) -> Vec<String> {
    let doc = roxmltree::Document::parse(ticket).unwrap();
    doc.descendants()
        .filter(|n| n.tag_name().name() == "Feature")
        .map(|n| n.attribute("name").unwrap().to_owned())
        .collect()
}

#[test]
fn a_document_wide_staple_is_offered_and_asked_for_by_its_own_name() {
    let caps = parse_capabilities(&only("DocumentStaple", "SaddleStitch")).unwrap();
    assert_eq!(caps.staples, vec![Staple::Saddle]);
    let f = ask(Some(Staple::Saddle), None, None, None);
    let ticket = delta_ticket(&f, &caps).unwrap().unwrap();
    assert_eq!(ticket_features(&ticket), ["psk:DocumentStaple"]);
}

#[test]
fn a_document_wide_punch_is_offered() {
    let caps = parse_capabilities(&only("DocumentHolePunch", "TopEdge")).unwrap();
    assert_eq!(caps.punch, vec![Edge::Top]);
    let f = ask(None, Some(Edge::Top), None, None);
    let ticket = delta_ticket(&f, &caps).unwrap().unwrap();
    assert_eq!(ticket_features(&ticket), ["psk:DocumentHolePunch"]);
}

#[test]
fn a_page_wide_bin_is_offered_by_name_without_case() {
    let caps = parse_capabilities(&only("PageOutputBin", "AutoSelect")).unwrap();
    assert_eq!(caps.bins.len(), 1);
    assert_eq!(caps.bins[0].feature(), "PageOutputBin");
    let f = Finishing {
        output_bin: Some("autoselect".into()),
        ..Finishing::default()
    };
    let ticket = delta_ticket(&f, &caps).unwrap().unwrap();
    assert_eq!(ticket_features(&ticket), ["psk:PageOutputBin"]);
}

#[test]
fn a_job_wide_feature_wins_over_a_document_wide_one() {
    let xml = format!(
        "<psf:PrintCapabilities xmlns:psf=\"{PSF}\" xmlns:psk=\"{PSK}\">\
         <psf:Feature name=\"psk:DocumentStaple\"><psf:Option name=\"psk:StapleTopLeft\"/></psf:Feature>\
         <psf:Feature name=\"psk:JobStapleAllDocuments\"><psf:Option name=\"psk:StapleTopLeft\"/></psf:Feature>\
         </psf:PrintCapabilities>"
    );
    let caps = parse_capabilities(&xml).unwrap();
    assert_eq!(
        caps.staple_feature(Staple::TopLeft),
        Some("JobStapleAllDocuments")
    );
}

#[test]
fn a_merge_that_drops_the_document_wide_option_is_refused() {
    let caps = parse_capabilities(&only("DocumentStaple", "SaddleStitch")).unwrap();
    let f = ask(Some(Staple::Saddle), None, None, None);
    let kept = parse_capabilities(&only("DocumentStaple", "SaddleStitch")).unwrap();
    assert!(dropped(&f, &caps, &kept).is_empty());
    let substituted = parse_capabilities(&only("DocumentStaple", "StapleTopLeft")).unwrap();
    assert_eq!(dropped(&f, &caps, &substituted).len(), 1);
    let moved = parse_capabilities(&only("JobStapleAllDocuments", "SaddleStitch")).unwrap();
    assert_eq!(
        dropped(&f, &caps, &moved).len(),
        1,
        "kept under another feature"
    );
}

#[test]
fn a_merge_that_drops_a_page_wide_bin_is_refused() {
    let caps = parse_capabilities(&only("PageOutputBin", "AutoSelect")).unwrap();
    let f = Finishing {
        output_bin: Some("autoselect".into()),
        ..Finishing::default()
    };
    let kept = parse_capabilities(&only("PageOutputBin", "AutoSelect")).unwrap();
    assert!(dropped(&f, &caps, &kept).is_empty());
    let other = parse_capabilities(&only("PageOutputBin", "Upper")).unwrap();
    assert_eq!(dropped(&f, &caps, &other).len(), 1);
}
