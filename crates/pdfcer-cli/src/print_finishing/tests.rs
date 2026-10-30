use pdfcer_print::ipp::{self, IppAttribute, IppGroup, IppValue, group};

use super::*;

fn reply(finishings: Option<&[i32]>, bins: &[&str]) -> PrinterOffer {
    let mut attrs = Vec::new();
    if let Some(list) = finishings {
        let values = list.iter().map(|&n| IppValue::Enum(n)).collect();
        attrs.push(IppAttribute::many("finishings-supported", values));
    }
    if !bins.is_empty() {
        let values = bins.iter().map(|b| IppValue::keyword(b)).collect();
        attrs.push(IppAttribute::many("output-bin-supported", values));
    }
    let bytes = ipp::encode_request(0, 1, &[IppGroup::new(group::PRINTER, attrs)], b"");
    PrinterOffer::from_response(&ipp::parse_response(&bytes).unwrap())
}

fn ask(
    staple: Option<StapleArg>,
    punch: Option<EdgeArg>,
    fold: Option<FoldArg>,
    bin: Option<&str>,
) -> Finishing {
    finishing(staple, punch, None, fold, bin.map(str::to_owned))
}

#[test]
fn offered_finishing_is_sent_with_the_listed_bin_spelling() {
    let attrs = reply(Some(&[3, 20, 74, 93]), &["face-down", "stacker-1"]);
    let f = ask(
        Some(StapleArg::TopLeft),
        Some(EdgeArg::Left),
        Some(FoldArg::Half),
        Some("Face-Down"),
    );
    let (values, bin) = ipp_checked(&f, Some(&attrs), "p").unwrap();
    assert_eq!(values, [20, 74, 93]);
    assert_eq!(bin.as_deref(), Some("face-down"));
}

#[test]
fn unoffered_finishing_or_bin_is_refused() {
    let attrs = reply(Some(&[3, 20]), &["face-down"]);
    let f = ask(Some(StapleArg::DualLeft), None, None, None);
    assert_eq!(ipp_checked(&f, Some(&attrs), "p"), Err(exit::EDIT_REFUSED));
    let f = ask(None, None, None, Some("tray-9"));
    assert_eq!(ipp_checked(&f, Some(&attrs), "p"), Err(exit::EDIT_REFUSED));
}

#[test]
fn unlisted_support_sends_as_given() {
    let f = ask(Some(StapleArg::Saddle), None, None, Some("top"));
    let (values, bin) = ipp_checked(&f, None, "p").unwrap();
    assert_eq!(values, [8]);
    assert_eq!(bin.as_deref(), Some("top"));
    let (values, _) = ipp_checked(&f, Some(&reply(None, &[])), "p").unwrap();
    assert_eq!(values, [8]);
}

#[test]
fn list_groups_by_flag() {
    let attrs = reply(Some(&[3, 4, 20, 31, 74, 78, 83, 93, 101, 13]), &["face-up"]);
    let lines = describe_ipp(&attrs);
    assert_eq!(lines[0], "--staple: top-left, dual-bottom");
    assert_eq!(lines[1], "--punch: left (2/3 holes), top (4 holes)");
    assert_eq!(lines[2], "--fold: half, engineering-z");
    assert_eq!(lines[3], "other (not settable here): booklet-maker");
    assert_eq!(lines[4], "--output-bin: face-up");
}

#[test]
fn lp_and_report_spellings() {
    assert_eq!(
        lp_options(&[20, 74], Some("face-down")),
        [
            ("finishings".to_owned(), "20,74".to_owned()),
            ("output-bin".to_owned(), "face-down".to_owned())
        ]
    );
    let mut f = ask(
        Some(StapleArg::TopLeft),
        Some(EdgeArg::Top),
        Some(FoldArg::Z),
        Some("tray-1"),
    );
    f.holes = Some(3);
    assert_eq!(
        describe(&f),
        "staple top-left, punch top, 3 holes, fold z, output bin tray-1"
    );
}

#[test]
fn driver_list_says_what_windows_cannot_set() {
    let xml = format!(
        "<psf:PrintCapabilities xmlns:psf=\"{}\" xmlns:psk=\"{}\" xmlns:b=\"urn:b\">         <psf:Feature name=\"psk:JobStapleAllDocuments\"><psf:Option name=\"psk:StapleTopLeft\"/></psf:Feature>         <psf:Feature name=\"psk:JobOutputBin\"><psf:Option name=\"b:FaceDown\"/></psf:Feature>         </psf:PrintCapabilities>",
        finishing::print_schema::PSF,
        finishing::print_schema::PSK
    );
    let caps = finishing::print_schema::parse_capabilities(&xml).unwrap();
    let lines = describe_driver(&caps);
    assert_eq!(lines[0], "--staple: top-left");
    assert_eq!(lines[1], "--punch: none");
    assert!(lines[3].contains("not settable"));
    assert_eq!(lines[4], "--output-bin: FaceDown");
}
