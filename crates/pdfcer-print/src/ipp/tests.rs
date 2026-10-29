// Test code: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;

/// A response as a printer would send it.
fn response(status: u16, groups: &[IppGroup]) -> Vec<u8> {
    let mut bytes = encode_request(status, 7, groups, b"");
    bytes[0] = 2; // IPP/2.0 reply
    bytes
}

#[test]
fn a_print_job_encodes_byte_for_byte() {
    let mut job = IppJob::new("ipp://p.local/ipp/print", "ken", "a.pdf");
    job.copies = 2;
    job.sides = Some(Sides::TwoSidedLongEdge);
    let bytes = job.print_job(1, b"%PDF");
    // version 1.1, Print-Job, request id 1, operation group.
    assert_eq!(&bytes[..9], &[1, 1, 0, 2, 0, 0, 0, 1, 0x01]);
    // The first attribute is attributes-charset = utf-8 (RFC 8011 §4.1.4).
    let first = [&[0x47, 0, 18][..], b"attributes-charset", &[0, 5], b"utf-8"].concat();
    assert_eq!(&bytes[9..9 + first.len()], &first[..]);
    // copies = 2 in the job group; the document follows the end tag.
    let copies = [&[0x21, 0, 6][..], b"copies", &[0, 4, 0, 0, 0, 2]].concat();
    assert!(bytes.windows(copies.len()).any(|w| w == copies));
    assert!(bytes.ends_with(&[0x03, b'%', b'P', b'D', b'F']));
}

#[test]
fn defaults_are_omitted_so_the_printer_default_applies() {
    let job = IppJob::new("ipp://p/", "u", "j");
    assert!(job.job_attributes().is_empty());
    let bytes = job.print_job(1, b"");
    let groups = parse_response(&bytes).unwrap().groups;
    assert_eq!(groups.len(), 1, "no job group when nothing is set");
    assert_eq!(groups[0].tag, group::OPERATION);
}

#[test]
fn a_response_round_trips_through_the_parser() {
    let groups = [
        operation_group("ipp://p/", "u"),
        IppGroup::new(
            group::PRINTER,
            vec![
                IppAttribute::many(
                    "document-format-supported",
                    vec![
                        IppValue::string(tag::MIME, "image/urf"),
                        IppValue::string(tag::MIME, "application/pdf"),
                    ],
                ),
                IppAttribute::new("copies-supported", IppValue::Range(1, 99)),
                IppAttribute::new("color-supported", IppValue::Boolean(true)),
            ],
        ),
        IppGroup::new(
            group::UNSUPPORTED,
            vec![IppAttribute::new("finishings", IppValue::OutOfBand(0x10))],
        ),
    ];
    let parsed = parse_response(&response(0x0001, &groups)).unwrap();
    assert_eq!(parsed.version, (2, 1));
    assert!(parsed.is_success());
    assert_eq!(parsed.request_id, 7);
    assert_eq!(parsed.groups, groups.to_vec());
    assert_eq!(
        parsed.printer_strings("document-format-supported"),
        ["image/urf", "application/pdf"]
    );
    assert_eq!(parsed.unsupported(), ["finishings"]);
    assert_eq!(
        status_name(parsed.status),
        "successful-ok-ignored-or-substituted-attributes"
    );
}

#[test]
fn a_job_id_and_status_message_are_read() {
    let mut op = operation_group("ipp://p/", "u");
    op.attributes.push(IppAttribute::new(
        "status-message",
        IppValue::string(tag::TEXT, "queued"),
    ));
    let job = IppGroup::new(
        group::JOB,
        vec![IppAttribute::new("job-id", IppValue::Integer(42))],
    );
    let parsed = parse_response(&response(0, &[op, job])).unwrap();
    assert_eq!(parsed.job_id(), Some(42));
    assert_eq!(parsed.status_message(), Some("queued"));
}

#[test]
fn malformed_responses_are_named_errors_not_panics() {
    let good = response(0, &[operation_group("ipp://p/", "u")]);
    for cut in 0..good.len() - 1 {
        let err = parse_response(&good[..cut]).unwrap_err();
        assert!(
            matches!(err, IppError::Truncated { .. } | IppError::NoEndTag),
            "{cut}: {err:?}"
        );
    }
    // A value length running past the end.
    let mut bad = good[..9].to_vec();
    bad.extend_from_slice(&[0x41, 0, 1, b'x', 0xFF, 0xFF]);
    assert!(matches!(
        parse_response(&bad),
        Err(IppError::Truncated { .. })
    ));
}

#[test]
fn an_attribute_flood_is_refused() {
    let mut bytes = vec![2, 0, 0, 0, 0, 0, 0, 1, group::PRINTER];
    for _ in 0..=MAX_ATTRIBUTES {
        bytes.extend_from_slice(&[tag::INTEGER, 0, 1, b'a', 0, 4, 0, 0, 0, 1]);
    }
    bytes.push(group::END);
    assert_eq!(parse_response(&bytes), Err(IppError::TooManyAttributes));
}

#[test]
fn a_wrong_length_value_is_kept_as_bytes() {
    let mut bytes = vec![2, 0, 0, 0, 0, 0, 0, 1, group::PRINTER];
    bytes.extend_from_slice(&[tag::INTEGER, 0, 1, b'n', 0, 2, 9, 9, group::END]);
    let parsed = parse_response(&bytes).unwrap();
    assert_eq!(
        parsed.groups[0].attributes[0].values,
        [IppValue::Other {
            tag: tag::INTEGER,
            bytes: vec![9, 9]
        }]
    );
}

#[test]
fn printer_uris_map_to_http_urls_on_port_631() {
    let cases = [
        (
            "ipp://printer.local/ipp/print",
            "http://printer.local:631/ipp/print",
        ),
        (
            "ipps://10.0.0.5/ipp/print",
            "https://10.0.0.5:631/ipp/print",
        ),
        ("ipp://host:8631/printers/q", "http://host:8631/printers/q"),
        ("ipp://[fe80::1]/ipp", "http://[fe80::1]:631/ipp"),
        ("ipp://[fe80::1]:9100/ipp", "http://[fe80::1]:9100/ipp"),
        ("IPP://h", "http://h:631"),
        ("http://h:631/ipp", "http://h:631/ipp"),
    ];
    for (uri, url) in cases {
        assert_eq!(http_url(uri).as_deref(), Some(url), "{uri}");
    }
    assert_eq!(http_url("ipp:///x"), None);
    assert_eq!(http_url("lpd://h/q"), None);
    assert_eq!(http_url("HP LaserJet"), None);
}

#[test]
fn a_get_printer_attributes_request_names_what_it_asks_for() {
    let bytes = get_printer_attributes("ipp://p/", "u", 3);
    assert_eq!(&bytes[2..4], &OP_GET_PRINTER_ATTRIBUTES.to_be_bytes());
    let parsed = parse_response(&bytes).unwrap();
    let asked = parsed
        .attribute(group::OPERATION, "requested-attributes")
        .unwrap();
    assert_eq!(asked.values.len(), QUERIED_ATTRIBUTES.len());
    assert_eq!(asked.values[4].as_str(), Some("document-format-supported"));
}
