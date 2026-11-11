// Test code: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::net::TcpListener;

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_print::ipp::{IppAttribute, IppGroup, IppValue, group, tag};

use super::*;
use crate::print_compose::tests::placements;
use crate::print_presets::Requested;
use crate::print_remote::tests::{pdf_printer, serve, temp_pdf};

/// A printer that answers Get-Printer-Attributes with PostScript only.
fn postscript_printer(_request: &[u8]) -> Vec<u8> {
    let groups = vec![
        ipp::operation_group("ipp://p/", "u"),
        IppGroup::new(
            group::PRINTER,
            vec![IppAttribute::many(
                "document-format-supported",
                vec![IppValue::string(tag::MIME, "application/postscript")],
            )],
        ),
    ];
    let mut bytes = ipp::encode_request(0, 1, &groups, b"");
    bytes[0] = 2;
    bytes
}

fn job<'a>(mode: PdfDirectArg, shape: Shape, output: &'a PrintOutput) -> DirectJob<'a> {
    DirectJob {
        mode,
        remote: RemoteJob {
            printer: Some("Office"),
            requested: Requested::default(),
            ignore_presets: false,
            send: true,
            uncollated: false,
            subset: SubsetArg::All,
            orientation: OrientationArg::Auto,
            output,
            layout_flags: Vec::new(),
        },
        reverse: false,
        shape,
        resize: false,
        ignored: Vec::new(),
    }
}

/// Letter, printable to every edge.
fn letter() -> Result<Paper, u8> {
    Ok(Paper::portrait((612.0, 792.0), (612.0, 792.0), (0.0, 0.0)))
}

/// Run `job` against a queue whose port names the mock on `listener`; the
/// exit code and the `route` the driver was called with, once per call.
fn run(job: &DirectJob<'_>, input: &Path, port: Option<u16>) -> (u8, Vec<Option<String>>) {
    let queue = move || match port {
        Some(p) => Ok(QueuePort::named(&format!("http://127.0.0.1:{p}/ipp/print"))),
        None => Err("the queue was read".to_owned()),
    };
    let mut calls = Vec::new();
    let mut driver = |_: &Path, output: &PrintOutput| {
        calls.push(output.route.clone());
        exit::SUCCESS
    };
    let code = direct_to(input, job, "Office", &queue, &letter, &mut driver);
    (code, calls)
}

fn mock(reply: fn(&[u8]) -> Vec<u8>, count: usize) -> (u16, std::thread::JoinHandle<Vec<Vec<u8>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    (port, serve(listener, count, reply))
}

#[test]
fn a_pdf_printer_receives_the_two_up_sheets() {
    let (port, server) = mock(pdf_printer, 2);
    let input = temp_pdf("direct-2up", 3);
    let output = PrintOutput::default();
    let shape = shape(Some((2, false)), None, None);
    let (code, calls) = run(&job(PdfDirectArg::On, shape, &output), &input, Some(port));
    let seen = server.join().unwrap();
    std::fs::remove_file(&input).unwrap();
    assert_eq!(code, exit::SUCCESS);
    assert!(calls.is_empty(), "the driver was not used");
    assert_eq!(&seen[0][2..4], &[0, 0x0B], "asked first");
    assert_eq!(&seen[1][2..4], &[0, 0x02], "then Print-Job");
    let sent = ipp::parse_response(&seen[1]).unwrap();
    let scaling = sent.attribute(group::JOB, "print-scaling").unwrap();
    assert_eq!(scaling.values, [IppValue::string(tag::KEYWORD, "none")]);
    let at = seen[1].windows(5).position(|w| w == b"%PDF-").unwrap();
    let out = EditSession::new(Document::from_bytes(seen[1][at..].to_vec()).unwrap());
    let pages = out.pages().unwrap();
    assert_eq!(pages.len(), 2, "three pages two to a sheet");
    for p in &pages {
        assert_eq!(p.rotate, 90);
        assert_eq!((p.media_box.width(), p.media_box.height()), (792.0, 612.0));
    }
    let first = placements(&out, 0);
    assert_eq!(first.len(), 2);
    assert_eq!(placements(&out, 1).len(), 1);
    // Each Letter page fits a 396x612 half: scale 396/612.
    for [s, d, e, _] in &first {
        assert!(
            (s - 396.0 / 612.0).abs() < 1e-3 && (s - d).abs() < 1e-9,
            "{s}"
        );
        assert!(*e >= -1e-6 && e + 612.0 * s <= 792.0 + 1e-6, "{e}");
    }
    assert!(first[0][2] + 396.0 - 1e-3 <= first[1][2], "{first:?}");
}

#[test]
fn auto_prints_through_the_driver_when_the_printer_lacks_pdf() {
    let (port, server) = mock(postscript_printer, 1);
    let input = temp_pdf("direct-auto", 1);
    let output = PrintOutput::default();
    let (code, calls) = run(
        &job(PdfDirectArg::Auto, Shape::Pages, &output),
        &input,
        Some(port),
    );
    let seen = server.join().unwrap();
    std::fs::remove_file(&input).unwrap();
    assert_eq!(code, exit::SUCCESS);
    assert_eq!(seen.len(), 1, "only asked");
    assert_eq!(calls, [Some("route=driver fallback=no-pdf".to_owned())]);
}

#[test]
fn on_refuses_when_the_printer_lacks_pdf() {
    let (port, server) = mock(postscript_printer, 1);
    let input = temp_pdf("direct-on", 1);
    let output = PrintOutput::default();
    let (code, calls) = run(
        &job(PdfDirectArg::On, Shape::Pages, &output),
        &input,
        Some(port),
    );
    let seen = server.join().unwrap();
    std::fs::remove_file(&input).unwrap();
    assert_eq!(code, exit::EDIT_REFUSED);
    assert_eq!(seen.len(), 1);
    assert!(calls.is_empty());
}

#[test]
fn off_is_the_driver_with_the_output_unchanged() {
    let output = PrintOutput::default();
    let job = job(PdfDirectArg::Off, Shape::Pages, &output);
    let mut calls = Vec::new();
    let mut driver = |input: &Path, output: &PrintOutput| {
        calls.push((input.to_path_buf(), output.route.clone()));
        exit::SUCCESS
    };
    // No file, no queue and no printer: off reaches none of them.
    let code = print_direct(Path::new("missing.pdf"), &job, None, None, &mut driver);
    assert_eq!(code, exit::SUCCESS);
    assert_eq!(calls, [(Path::new("missing.pdf").to_path_buf(), None)]);
}

#[test]
fn a_driver_only_flag_falls_back_before_the_network() {
    let output = PrintOutput::default();
    let mut job = job(PdfDirectArg::Auto, Shape::Pages, &output);
    job.remote.layout_flags = vec!["--separations"];
    let (code, calls) = run(&job, Path::new("missing.pdf"), None);
    assert_eq!(code, exit::SUCCESS);
    assert_eq!(
        calls,
        [Some("route=driver fallback=driver-only-option".to_owned())]
    );
    job.mode = PdfDirectArg::On;
    assert_eq!(
        run(&job, Path::new("missing.pdf"), None).0,
        exit::EDIT_REFUSED
    );
}

#[test]
fn a_port_with_no_address_falls_back() {
    let output = PrintOutput::default();
    let job = job(PdfDirectArg::Auto, Shape::Pages, &output);
    let queue = || Ok(QueuePort::named("USB001"));
    let mut calls = Vec::new();
    let mut driver = |_: &Path, output: &PrintOutput| {
        calls.push(output.route.clone());
        exit::SUCCESS
    };
    let code = direct_to(
        Path::new("missing.pdf"),
        &job,
        "Office",
        &queue,
        &letter,
        &mut driver,
    );
    assert_eq!(code, exit::SUCCESS);
    assert_eq!(
        calls,
        [Some("route=driver fallback=no-network-address".to_owned())]
    );
}

#[test]
fn the_sequence_applies_subset_then_reverse() {
    assert_eq!(sequence(&[0, 1, 2, 3, 4], SubsetArg::Odd, false), [0, 2, 4]);
    assert_eq!(sequence(&[0, 1, 2, 3, 4], SubsetArg::Even, true), [3, 1]);
    assert_eq!(sequence(&[1, 2], SubsetArg::All, true), [2, 1]);
}

#[test]
fn reorder_keeps_only_the_sequence_in_its_order() {
    let mut s =
        EditSession::new(Document::from_bytes(crate::print_remote::tests::pages_pdf(4)).unwrap());
    let before: Vec<_> = s.pages().unwrap().iter().map(|p| p.id).collect();
    reorder(&mut s, &[3, 1], 4).unwrap();
    let after: Vec<_> = s.pages().unwrap().iter().map(|p| p.id).collect();
    assert_eq!(after, [before[3], before[1]]);
}
