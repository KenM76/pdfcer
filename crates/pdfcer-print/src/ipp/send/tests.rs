// Test code: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::super::*;
use crate::finishing::{Edge, Finishing, Fold, Staple};

/// Replays canned replies and records what was posted where.
struct Fake {
    replies: Vec<Result<Vec<u8>, TransportError>>,
    posted: Vec<(String, Vec<u8>)>,
}

impl IppTransport for Fake {
    fn post(&mut self, url: &str, message: &[u8]) -> Result<Vec<u8>, TransportError> {
        self.posted.push((url.to_owned(), message.to_vec()));
        self.replies.remove(0)
    }
}

fn fake(replies: Vec<Result<Vec<u8>, TransportError>>) -> Fake {
    Fake {
        replies,
        posted: Vec::new(),
    }
}

fn reply(status: u16, groups: &[IppGroup]) -> Vec<u8> {
    let mut bytes = encode_request(status, 7, groups, b"");
    bytes[0] = 2;
    bytes
}

fn printer(attrs: Vec<IppAttribute>) -> Vec<u8> {
    reply(0, &[IppGroup::new(group::PRINTER, attrs)])
}

fn ints(name: &str, v: &[i32]) -> IppAttribute {
    IppAttribute::many(name, v.iter().map(|&n| IppValue::Enum(n)).collect())
}

fn strs(name: &str, t: u8, v: &[&str]) -> IppAttribute {
    IppAttribute::many(name, v.iter().map(|s| IppValue::string(t, s)).collect())
}

/// A printer offering: staple top-left (20), dual-left punch (74) and
/// triple-left (78), dual-top (75), half fold (93), trim (11), bins.
fn office_printer() -> Vec<u8> {
    printer(vec![
        strs("printer-make-and-model", tag::TEXT, &["Acme 9000"]),
        strs("document-format-supported", tag::MIME, &["application/pdf"]),
        ints("finishings-supported", &[3, 4, 20, 74, 78, 75, 93, 11]),
        strs("output-bin-supported", tag::KEYWORD, &["top", "Stacker-1"]),
        strs(
            "sides-supported",
            tag::KEYWORD,
            &["one-sided", "two-sided-long-edge", "bogus"],
        ),
        strs(
            "print-color-mode-supported",
            tag::KEYWORD,
            &["monochrome", "color"],
        ),
    ])
}

fn finishing(set: impl FnOnce(&mut Finishing)) -> Finishing {
    let mut f = Finishing::default();
    set(&mut f);
    f
}

#[test]
fn the_offer_is_read_from_get_printer_attributes() {
    let mut t = fake(vec![Ok(office_printer())]);
    let o = query_printer("ipp://p.local/ipp/print", "ken", &mut t).unwrap();
    assert_eq!(t.posted[0].0, "http://p.local:631/ipp/print");
    assert_eq!(&t.posted[0].1[2..8], &[0, 0x0B, 0, 0, 0, 1]);
    assert_eq!(o.make_and_model.as_deref(), Some("Acme 9000"));
    assert_eq!(o.staples, vec![Staple::TopLeft]);
    assert_eq!(o.punches.len(), 2);
    assert_eq!(
        (o.punches[0].edge, o.punches[0].holes.clone()),
        (Edge::Left, vec![Some(2), Some(3)])
    );
    assert_eq!(
        (o.punches[1].edge, o.punches[1].holes.clone()),
        (Edge::Top, vec![Some(2)])
    );
    assert_eq!(o.folds, vec![Fold::Half]);
    assert_eq!(o.other_finishings, vec!["trim"]);
    assert_eq!(o.output_bins, vec!["top", "Stacker-1"]);
    assert_eq!(o.sides, vec![Sides::OneSided, Sides::TwoSidedLongEdge]);
    assert_eq!(o.color_modes, vec!["monochrome", "color"]);
    assert!(o.accepts_pdf());
}

#[test]
fn a_printer_listing_nothing_offers_unlisted_not_none() {
    let o = PrinterOffer::from_response(&parse_response(&printer(vec![])).unwrap());
    assert_eq!(o.finishings, None);
    assert!(
        o.accepts_pdf(),
        "an unlisted format list does not refuse PDF"
    );
    let c = check_finishing(
        &finishing(|f| {
            f.staple = Some(Staple::TopLeft);
            f.output_bin = Some("top".to_owned());
        }),
        Some(&o),
    );
    assert!(c.refused.is_empty());
    assert_eq!(c.values, vec![20]);
    assert_eq!(c.output_bin.as_deref(), Some("top"));
    assert_eq!(
        c.notes,
        vec![
            OfferNote::FinishingNotListed,
            OfferNote::BinsNotListed {
                asked: "top".to_owned()
            }
        ]
    );
    assert_eq!(
        c.notes[1].describe("P"),
        "P does not list its output bins; asking for \"top\" as given."
    );
}

#[test]
fn a_listed_bin_is_matched_without_case_and_sent_as_spelled() {
    let o = PrinterOffer::from_response(&parse_response(&office_printer()).unwrap());
    let c = check_finishing(
        &finishing(|f| f.output_bin = Some("stacker-1".to_owned())),
        Some(&o),
    );
    assert_eq!(c.output_bin.as_deref(), Some("Stacker-1"));
}

#[test]
fn send_queries_checks_and_returns_the_receipt() {
    let accepted = reply(
        0,
        &[
            IppGroup::new(
                group::UNSUPPORTED,
                vec![strs("print-scaling", tag::KEYWORD, &["fit"])],
            ),
            IppGroup::new(
                group::JOB,
                vec![IppAttribute::new("job-id", IppValue::Integer(41))],
            ),
        ],
    );
    let mut t = fake(vec![Ok(office_printer()), Ok(accepted)]);
    let job = IppJob::new("ipp://p.local/ipp/print", "ken", "a.pdf");
    let f = finishing(|f| f.punch = Some(Edge::Left));
    let sent = send_ipp(job, &f, b"%PDF-1.7", &mut t).unwrap();
    assert_eq!(sent.receipt.job_id, Some(41));
    assert_eq!(sent.receipt.ignored, vec!["print-scaling"]);
    assert_eq!(sent.plan.job.finishings, vec![74]);
    assert!(t.posted[1].1.ends_with(b"%PDF-1.7"));
    assert_eq!(&t.posted[1].1[2..8], &[0, 2, 0, 0, 0, 2]);
}

#[test]
fn a_refused_request_is_never_sent_and_names_what_was_refused() {
    let mut t = fake(vec![Ok(office_printer())]);
    let job = IppJob::new("ipp://p/", "ken", "a.pdf");
    let f = finishing(|f| {
        f.fold = Some(Fold::Z);
        f.output_bin = Some("bottom".to_owned());
    });
    let e = send_ipp(job, &f, b"", &mut t).unwrap_err();
    assert_eq!(t.posted.len(), 1, "only the query was posted");
    assert_eq!(
        e.to_string(),
        "ipp://p/ does not offer: fold Z (fold-z); output bin \"bottom\" (it lists top, Stacker-1)"
    );
}

#[test]
fn a_printer_that_does_not_take_pdf_is_refused() {
    let p = printer(vec![strs(
        "document-format-supported",
        tag::MIME,
        &["application/postscript"],
    )]);
    let mut t = fake(vec![Ok(p)]);
    let e = send_ipp(
        IppJob::new("ipp://p/", "u", "j"),
        &Finishing::default(),
        b"",
        &mut t,
    )
    .unwrap_err();
    assert_eq!(
        e.to_string(),
        "ipp://p/ does not accept PDF (it takes application/postscript). Print through a driver that converts it."
    );
}

#[test]
fn an_error_status_is_refused_with_its_message() {
    let p = reply(
        0x0400,
        &[IppGroup::new(
            group::OPERATION,
            vec![strs("status-message", tag::TEXT, &["no"])],
        )],
    );
    let mut t = fake(vec![Ok(p)]);
    let e = query_printer("ipp://p/", "u", &mut t).unwrap_err();
    assert!(matches!(e, IppSendError::Refused { status: 0x0400, .. }));
    assert_eq!(
        e.to_string(),
        format!("ipp://p/ refused: {} (no)", status_name(0x0400))
    );
}

#[test]
fn the_http_transport_refuses_tls_and_a_transport_error_passes_through() {
    let e = HttpTransport::default()
        .post("https://p:631/", b"")
        .unwrap_err();
    assert_eq!(
        e,
        TransportError::NeedsTls {
            url: "https://p:631/".to_owned()
        }
    );
    let mut t = fake(vec![Err(TransportError::NeedsTls {
        url: "u".to_owned(),
    })]);
    let e = query_printer("ipps://p/", "u", &mut t).unwrap_err();
    assert_eq!(t.posted[0].0, "https://p:631/");
    assert!(matches!(
        e,
        IppSendError::Transport(TransportError::NeedsTls { .. })
    ));
    let e = query_printer("lpd://p/", "u", &mut fake(vec![])).unwrap_err();
    assert!(matches!(e, IppSendError::NotAPrinterUri { .. }));
}
