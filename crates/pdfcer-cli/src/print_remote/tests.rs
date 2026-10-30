// Test code: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::io::{Read, Write};
use std::net::TcpListener;

use pdfcer_print::ipp::{IppAttribute, IppGroup, IppValue, group, tag};

use super::*;

/// A PDF of `n` empty Letter pages.
fn pages_pdf(n: usize) -> Vec<u8> {
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", i + 3)).collect();
    let mut bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
    ];
    bodies.extend(
        (0..n).map(|_| "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_owned()),
    );
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = buf.len();
    buf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", bodies.len() + 1).as_bytes(),
    );
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    let size = bodies.len() + 1;
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    buf
}

fn job_for<'a>(printer: Option<&'a str>, output: &'a PrintOutput) -> RemoteJob<'a> {
    RemoteJob {
        printer,
        requested: Requested::default(),
        ignore_presets: false,
        send: false,
        uncollated: false,
        subset: SubsetArg::All,
        orientation: OrientationArg::Auto,
        output,
        layout_flags: Vec::new(),
    }
}

fn prepared(copies: u16, ranges: Vec<(u32, u32)>) -> Prepared {
    let mut job = crate::print_presets::merge(
        Requested::default(),
        &pdfcer_core::print_presets::PrintPresets::default(),
        1,
        true,
    );
    job.copies = copies;
    job.duplex = DuplexArg::LongEdge;
    Prepared {
        pdf: b"%PDF".to_vec(),
        name: "a.pdf".to_owned(),
        job,
        ranges,
    }
}

#[test]
fn only_uris_route_here_on_windows() {
    assert!(is_network_printer(Some("ipp://p.local/ipp/print")));
    assert!(is_network_printer(Some("ipps://p/ipp/print")));
    assert!(!is_network_printer(Some("HP LaserJet")));
    assert!(!is_network_printer(None));
    assert_eq!(routes_here(Some("HP LaserJet")), cfg!(not(windows)));
}

#[test]
fn page_ranges_collapse_and_every_page_is_none() {
    assert_eq!(page_ranges(&[0, 1, 2], SubsetArg::All, 3), None);
    assert_eq!(
        page_ranges(&[0, 1, 2, 4, 6, 7], SubsetArg::All, 8),
        Some(vec![(1, 3), (5, 5), (7, 8)])
    );
    // Odd keeps document pages 1, 3, 5 (0-based 0, 2, 4).
    assert_eq!(
        page_ranges(&[0, 1, 2, 3, 4], SubsetArg::Odd, 5),
        Some(vec![(1, 1), (3, 3), (5, 5)])
    );
    assert_eq!(page_ranges(&[0], SubsetArg::Even, 1), Some(vec![]));
}

#[test]
fn set_flags_keeps_only_the_given_ones() {
    assert_eq!(
        set_flags(&[("--a", true), ("--b", false), ("--c", true)]),
        ["--a", "--c"]
    );
}

#[test]
fn lp_arguments_carry_the_job() {
    let output = PrintOutput {
        color: ColorArg::Grayscale,
        ..PrintOutput::default()
    };
    let mut job = job_for(Some("office"), &output);
    job.orientation = OrientationArg::Landscape;
    job.uncollated = true;
    let args = lp_args(&job, &prepared(3, vec![(1, 2), (4, 4)])).join(" ");
    assert_eq!(
        args,
        "-d office -n 3 -t a.pdf -o sides=two-sided-long-edge -o print-scaling=fit \
         -o page-ranges=1-2,4-4 -o print-color-mode=monochrome \
         -o orientation-requested=4 \
         -o multiple-document-handling=separate-documents-uncollated-copies"
    );
}

#[test]
fn layout_flags_are_refused_before_anything_is_opened() {
    let output = PrintOutput::default();
    let mut job = job_for(Some("ipp://127.0.0.1:1/ipp"), &output);
    job.layout_flags = vec!["--n-up"];
    assert_eq!(print_remote(Path::new("missing.pdf"), &job), USAGE);
}

/// Answer each IPP request on `listener` with `reply(request)`, `count`
/// times, and return the requests' bodies.
fn serve(
    listener: TcpListener,
    count: usize,
    reply: fn(&[u8]) -> Vec<u8>,
) -> std::thread::JoinHandle<Vec<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut seen = Vec::new();
        for _ in 0..count {
            let (mut s, _) = listener.accept().unwrap();
            let mut raw = Vec::new();
            let mut buf = [0u8; 8192];
            let body = loop {
                let n = s.read(&mut buf).unwrap();
                raw.extend_from_slice(&buf[..n]);
                let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
                    continue;
                };
                let head = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
                let len: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                if raw.len() >= end + 4 + len {
                    break raw[end + 4..end + 4 + len].to_vec();
                }
            };
            let answer = reply(&body);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\n\r\n",
                answer.len()
            );
            s.write_all(head.as_bytes()).unwrap();
            s.write_all(&answer).unwrap();
            seen.push(body);
        }
        seen
    })
}

/// A printer that takes PDF and accepts every job as job 7.
fn pdf_printer(request: &[u8]) -> Vec<u8> {
    let op = pdfcer_print::ipp::operation_group("ipp://p/", "u");
    let groups = if request[3] == 0x0B {
        vec![
            op,
            IppGroup::new(
                group::PRINTER,
                vec![IppAttribute::many(
                    "document-format-supported",
                    vec![IppValue::string(tag::MIME, "application/pdf")],
                )],
            ),
        ]
    } else {
        vec![
            op,
            IppGroup::new(
                group::JOB,
                vec![IppAttribute::new("job-id", IppValue::Integer(7))],
            ),
        ]
    };
    let mut bytes = pdfcer_print::ipp::encode_request(0, 1, &groups, b"");
    bytes[0] = 2;
    bytes
}

fn temp_pdf(name: &str, pages: usize) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("pdfcer-{name}-{}.pdf", std::process::id()));
    std::fs::write(&path, pages_pdf(pages)).unwrap();
    path
}

#[test]
fn a_job_is_sent_to_a_network_printer() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let uri = format!(
        "ipp://127.0.0.1:{}/ipp/print",
        listener.local_addr().unwrap().port()
    );
    let server = serve(listener, 2, pdf_printer);
    let input = temp_pdf("ipp-send", 3);
    let output = PrintOutput::default();
    let mut job = job_for(Some(&uri), &output);
    job.send = true;
    job.requested.pages = Some("2-3".to_owned());
    job.requested.copies = Some(2);
    assert_eq!(print_remote(&input, &job), exit::SUCCESS);
    let seen = server.join().unwrap();
    std::fs::remove_file(&input).unwrap();
    assert_eq!(&seen[0][2..4], &[0, 0x0B], "queried first");
    let sent = pdfcer_print::ipp::parse_response(&seen[1]).unwrap();
    let ranges = sent.attribute(group::JOB, "page-ranges").unwrap();
    assert_eq!(ranges.values, [IppValue::Range(2, 3)]);
    let copies = sent.attribute(group::JOB, "copies").unwrap();
    assert_eq!(copies.values, [IppValue::Integer(2)]);
    // The unmodified file is sent byte-for-byte after the end tag.
    assert!(seen[1].ends_with(&pages_pdf(3)));
}

#[test]
fn a_dry_run_queries_and_sends_nothing() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let uri = format!(
        "ipp://127.0.0.1:{}/ipp/print",
        listener.local_addr().unwrap().port()
    );
    let server = serve(listener, 1, pdf_printer);
    let input = temp_pdf("ipp-dry", 1);
    let output = PrintOutput::default();
    assert_eq!(
        print_remote(&input, &job_for(Some(&uri), &output)),
        exit::SUCCESS
    );
    let seen = server.join().unwrap();
    std::fs::remove_file(&input).unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(&seen[0][2..4], &[0, 0x0B]);
}

#[test]
fn an_unreachable_printer_has_its_own_exit_code() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let uri = format!("ipp://127.0.0.1:{port}/ipp/print");
    let input = temp_pdf("ipp-down", 1);
    let output = PrintOutput::default();
    let code = print_remote(&input, &job_for(Some(&uri), &output));
    std::fs::remove_file(&input).unwrap();
    assert_eq!(code, exit::PRINTER_UNREACHABLE);
}

#[cfg(not(feature = "download"))]
#[test]
fn ipps_without_tls_is_refused_by_name() {
    use pdfcer_print::ipp::IppTransport;
    let e = transport()
        .post("https://p:631/ipp/print", b"x")
        .unwrap_err();
    let e = pdfcer_print::ipp::IppSendError::Transport(e);
    assert_eq!(report(&e), exit::EDIT_REFUSED);
}
