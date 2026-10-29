//! `print` to a network printer by URI (`ipp://`, `ipps://`; RFC 8010 and
//! RFC 8011) and, off Windows, to a CUPS queue through `lp`.
//!
//! Both routes send the PDF itself and let the printer lay it out, so the
//! options that need pdfcer to compose the sheets (n-up, booklet, poster,
//! a paper or scale override, separations, image printing) are refused by
//! name rather than dropped. Header, footer and printer's marks are page
//! edits, so they are applied to the copy that is sent. Without `--send`
//! the printer is queried and the job described, and nothing prints.

use std::path::Path;
use std::time::Duration;

use pdfcer_print::ipp::{self, IppJob, Sides};

use super::*;
use crate::print_output::{ColorArg, PrintOutput};
use crate::print_presets::{Effective, Requested};

/// How long to wait for a network printer's connect, and each read or write.
const IPP_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for the local CUPS server to describe a queue.
const CUPS_QUERY_TIMEOUT: Duration = Duration::from_secs(5);

/// Exit code for an option this route cannot honour (clap's usage code).
pub(crate) const USAGE: u8 = 2;

/// Whether `printer` names a network printer by URI rather than a queue.
pub(crate) fn is_network_printer(printer: Option<&str>) -> bool {
    printer.is_some_and(|p| ipp::http_url(p).is_some())
}

/// Whether `print` takes this module's route: a network URI anywhere, and
/// every printer off Windows, where queues are CUPS's.
pub(crate) fn routes_here(printer: Option<&str>) -> bool {
    is_network_printer(printer) || cfg!(not(windows))
}

/// The settings a URI or CUPS job carries.
pub(crate) struct RemoteJob<'a> {
    pub printer: Option<&'a str>,
    pub requested: Requested,
    pub ignore_presets: bool,
    pub send: bool,
    pub uncollated: bool,
    pub subset: SubsetArg,
    pub orientation: OrientationArg,
    pub output: &'a PrintOutput,
    /// The flags given that need pdfcer to compose the sheets.
    pub layout_flags: Vec<&'static str>,
}

/// The flags in `given` (name, set) that are set.
pub(crate) fn set_flags(given: &[(&'static str, bool)]) -> Vec<&'static str> {
    given
        .iter()
        .filter(|(_, set)| *set)
        .map(|(n, _)| *n)
        .collect()
}

/// The document prepared for sending: its bytes and what to ask for.
struct Prepared {
    pdf: Vec<u8>,
    name: String,
    job: Effective,
    /// 1-based inclusive page ranges; empty for every page.
    ranges: Vec<(u32, u32)>,
}

/// Print `input` by URI or through CUPS. The exit code.
pub(crate) fn print_remote(input: &Path, job: &RemoteJob<'_>) -> u8 {
    let where_ = if is_network_printer(job.printer) {
        "a network printer"
    } else {
        "a CUPS queue"
    };
    if !job.layout_flags.is_empty() {
        eprintln!(
            "pdfcer: {} need pdfcer to lay out the sheets, and {where_} receives \
             the PDF and lays it out itself. Print without them, or through a \
             Windows printer.",
            job.layout_flags.join(", ")
        );
        return USAGE;
    }
    eprintln!(
        "pdfcer: {where_} decides which annotations print (normally those the \
         PDF marks printable), not --comments."
    );
    let prepared = match prepare(input, job) {
        Ok(p) => p,
        Err(code) => return code,
    };
    match job.printer.filter(|p| is_network_printer(Some(p))) {
        Some(uri) => {
            let finishing = &job.output.finishing;
            print_ipp(
                uri,
                ipp_job(uri, job, &prepared),
                finishing,
                &prepared,
                job.send,
            )
        }
        None => {
            let (values, bin) = match cups_finishing(job.printer, &job.output.finishing) {
                Ok(v) => v,
                Err(code) => return code,
            };
            let mut args = lp_args(job, &prepared);
            for (k, v) in crate::print_finishing::lp_options(&values, bin.as_deref()) {
                args.extend(["-o".to_owned(), format!("{k}={v}")]);
            }
            print_cups(job.printer, &args, &prepared, job.send)
        }
    }
}

/// Open, stamp and select: the bytes that are sent, and the page ranges.
fn prepare(input: &Path, job: &RemoteJob<'_>) -> Result<Prepared, u8> {
    let fail = |err: &dyn std::fmt::Display, code| {
        eprintln!("pdfcer: {}: {err}", input.display());
        code
    };
    let doc = open_document(input).map_err(|e| fail(&e, exit_code_for_doc(&e)))?;
    let presets = pdfcer_core::print_presets::print_presets(&doc);
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    let name = input
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let notes = job
        .output
        .stamp(&mut session, &name)
        .map_err(|e| fail(&e, exit::EDIT_REFUSED))?;
    notes.iter().for_each(|n| eprintln!("pdfcer: {n}"));
    let count = session
        .pages()
        .map_err(|e| fail(&e, exit::RUNTIME_ERROR))?
        .len();
    let merged =
        crate::print_presets::merge(job.requested.clone(), &presets, count, job.ignore_presets);
    merged.notes.iter().for_each(|n| eprintln!("pdfcer: {n}"));
    let selected = parse_pages(&merged.pages, count).map_err(|m| fail(&m, exit::RUNTIME_ERROR))?;
    let ranges = page_ranges(&selected, job.subset, count);
    if ranges.as_ref().is_some_and(Vec::is_empty) {
        eprintln!("pdfcer: {}: no pages left to print", input.display());
        return Err(exit::RUNTIME_ERROR);
    }
    let pdf = if session.is_modified() {
        let options = pdfcer_core::writer::SaveOptions::default();
        session
            .to_incremental_bytes(&options)
            .map_err(|e| fail(&e, exit::SAVE_REFUSED))?
            .0
    } else {
        std::fs::read(input).map_err(|e| fail(&e, exit::IO_ERROR))?
    };
    Ok(Prepared {
        pdf,
        name,
        job: merged,
        ranges: ranges.unwrap_or_default(),
    })
}

/// `selected` (0-based) filtered by `subset` (document page numbers) as
/// 1-based inclusive ranges; `None` when that is every one of `count` pages.
fn page_ranges(selected: &[usize], subset: SubsetArg, count: usize) -> Option<Vec<(u32, u32)>> {
    let keep = |i: &usize| match subset {
        SubsetArg::All => true,
        SubsetArg::Odd => i.is_multiple_of(2),
        SubsetArg::Even => !i.is_multiple_of(2),
    };
    let pages: Vec<usize> = selected.iter().copied().filter(keep).collect();
    if pages.len() == count && pages.iter().enumerate().all(|(n, &p)| n == p) {
        return None;
    }
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for p in pages {
        let n = u32::try_from(p + 1).unwrap_or(u32::MAX);
        match ranges.last_mut() {
            Some(last) if last.1.checked_add(1) == Some(n) => last.1 = n,
            _ => ranges.push((n, n)),
        }
    }
    Some(ranges)
}

fn sides(duplex: DuplexArg) -> Sides {
    match duplex {
        DuplexArg::Simplex => Sides::OneSided,
        DuplexArg::LongEdge => Sides::TwoSidedLongEdge,
        DuplexArg::ShortEdge => Sides::TwoSidedShortEdge,
    }
}

/// The `print-scaling` keyword (PWG 5100.13) for pdfcer's `--scale`.
fn scaling(scale: PrintScaleArg) -> &'static str {
    match scale {
        PrintScaleArg::Fit => "fit",
        PrintScaleArg::Actual => "none",
        PrintScaleArg::Shrink => "auto-fit",
    }
}

/// `orientation-requested` (RFC 8011 §5.2.10): 3 portrait, 4 landscape.
fn orientation(o: OrientationArg) -> Option<i32> {
    match o {
        OrientationArg::Auto => None,
        OrientationArg::Portrait => Some(3),
        OrientationArg::Landscape => Some(4),
    }
}

/// The requesting user: `USERNAME` or `USER`, else `pdfcer`.
fn user_name() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "pdfcer".to_owned())
}

fn ipp_job(uri: &str, job: &RemoteJob<'_>, p: &Prepared) -> IppJob {
    let mut out = IppJob::new(uri, &user_name(), &p.name);
    out.copies = p.job.copies;
    out.sides = Some(sides(p.job.duplex));
    out.page_ranges.clone_from(&p.ranges);
    out.monochrome = job.output.color == ColorArg::Grayscale;
    out.orientation = orientation(job.orientation);
    out.uncollated = job.uncollated && p.job.copies > 1;
    out.print_scaling = Some(scaling(p.job.scale).to_owned());
    out
}

/// The `lp` arguments (CUPS `lp(1)`, options per RFC 8011 names).
fn lp_args(job: &RemoteJob<'_>, p: &Prepared) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut opt = |k: &str, v: &str| args.extend(["-o".to_owned(), format!("{k}={v}")]);
    opt("sides", sides(p.job.duplex).keyword());
    opt("print-scaling", scaling(p.job.scale));
    if !p.ranges.is_empty() {
        let r: Vec<String> = p.ranges.iter().map(|(a, b)| format!("{a}-{b}")).collect();
        opt("page-ranges", &r.join(","));
    }
    if job.output.color == ColorArg::Grayscale {
        opt("print-color-mode", "monochrome");
    }
    if let Some(o) = orientation(job.orientation) {
        opt("orientation-requested", &o.to_string());
    }
    if job.uncollated && p.job.copies > 1 {
        opt(
            "multiple-document-handling",
            "separate-documents-uncollated-copies",
        );
    }
    let mut head = Vec::new();
    if let Some(dest) = job.printer {
        head.extend(["-d".to_owned(), dest.to_owned()]);
    }
    if p.job.copies > 1 {
        head.extend(["-n".to_owned(), p.job.copies.to_string()]);
    }
    head.extend(["-t".to_owned(), p.name.clone()]);
    head.extend(args);
    head
}

/// POST an IPP message to `uri` over the transport its scheme needs.
fn post(uri: &str, message: &[u8]) -> Result<Vec<u8>, (String, u8)> {
    let url = ipp::http_url(uri).unwrap_or_default();
    if url.to_ascii_lowercase().starts_with("https://") {
        return post_tls(&url, message);
    }
    ipp::post_ipp_http(&url, message, IPP_TIMEOUT).map_err(|e| {
        let code = match e {
            ipp::TransportError::Unreachable { .. } => exit::PRINTER_UNREACHABLE,
            _ => exit::RUNTIME_ERROR,
        };
        (e.to_string(), code)
    })
}

#[cfg(feature = "download")]
fn post_tls(url: &str, message: &[u8]) -> Result<Vec<u8>, (String, u8)> {
    // The whole exchange, the document upload included.
    pdfcer_fetch::post_ipp(url, message, IPP_TIMEOUT * 5).map_err(|e| {
        let code = match e {
            pdfcer_fetch::FetchError::Transport { .. }
            | pdfcer_fetch::FetchError::TimedOut { .. } => exit::PRINTER_UNREACHABLE,
            _ => exit::RUNTIME_ERROR,
        };
        (e.to_string(), code)
    })
}

#[cfg(not(feature = "download"))]
fn post_tls(url: &str, _message: &[u8]) -> Result<Vec<u8>, (String, u8)> {
    Err((
        format!(
            "{url} needs TLS, and this build has no TLS client (the `download` \
             feature is off). Use the printer's ipp:// address, or a build with \
             `--features download`."
        ),
        exit::EDIT_REFUSED,
    ))
}

/// The IPP address CUPS serves queue `name` on, on this machine.
pub(crate) fn cups_uri(name: &str) -> String {
    format!("ipp://localhost:631/printers/{name}")
}

/// Ask a printer for its attributes. The exit code on failure, reported.
pub(crate) fn query_printer(uri: &str) -> Result<ipp::IppResponse, u8> {
    match exchange(uri, &ipp::get_printer_attributes(uri, &user_name(), 1))? {
        r if r.is_success() => Ok(r),
        r => Err(refused(uri, &r)),
    }
}

/// The finishing for an `lp` job, checked against what CUPS lists for the
/// queue when it answers at localhost; otherwise sent as given and said so.
fn cups_finishing(
    dest: Option<&str>,
    f: &pdfcer_print::finishing::Finishing,
) -> Result<(Vec<i32>, Option<String>), u8> {
    if f.is_empty() {
        return Ok((Vec::new(), None));
    }
    let who = dest.unwrap_or("the default CUPS queue");
    let attrs = dest.and_then(|d| {
        let uri = cups_uri(d);
        let message = ipp::get_printer_attributes(&uri, &user_name(), 1);
        let raw = ipp::post_ipp_http(&ipp::http_url(&uri)?, &message, CUPS_QUERY_TIMEOUT).ok()?;
        ipp::parse_response(&raw)
            .ok()
            .filter(ipp::IppResponse::is_success)
    });
    if attrs.is_none() {
        eprintln!("pdfcer: CUPS did not describe {who}; finishing not confirmed.");
    }
    crate::print_finishing::ipp_checked(f, attrs.as_ref(), who)
}

/// Send one request to `uri` and decode the reply.
fn exchange(uri: &str, message: &[u8]) -> Result<ipp::IppResponse, u8> {
    let raw = post(uri, message).map_err(|(msg, code)| {
        eprintln!("pdfcer: {msg}");
        code
    })?;
    ipp::parse_response(&raw).map_err(|e| {
        eprintln!("pdfcer: {uri}: {e}");
        exit::RUNTIME_ERROR
    })
}

/// Query the printer, then (with `send`) print. The exit code.
fn print_ipp(
    uri: &str,
    mut job: IppJob,
    finishing: &pdfcer_print::finishing::Finishing,
    p: &Prepared,
    send: bool,
) -> u8 {
    let attrs = match query_printer(uri) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let model = attrs.printer_strings("printer-make-and-model");
    eprintln!(
        "pdfcer: printer {uri}{}",
        model.first().map(|m| format!(" ({m})")).unwrap_or_default()
    );
    let formats = attrs.printer_strings("document-format-supported");
    if !formats.is_empty() && !formats.contains(&"application/pdf") {
        eprintln!(
            "pdfcer: {uri} does not accept PDF (it takes {}). Print through a \
             driver that converts it.",
            formats.join(", ")
        );
        return exit::EDIT_REFUSED;
    }
    match crate::print_finishing::ipp_checked(finishing, Some(&attrs), uri) {
        Ok((values, bin)) => (job.finishings, job.output_bin) = (values, bin),
        Err(code) => return code,
    }
    let asked: Vec<String> = job.job_attributes().iter().map(describe).collect();
    let summary = format!("{} bytes, {}", p.pdf.len(), asked.join(", "));
    if !send {
        eprintln!(
            "pdfcer: dry run: would send {} ({summary}). Pass --send to print.",
            p.name
        );
        return exit::SUCCESS;
    }
    let reply = match exchange(uri, &job.print_job(2, &p.pdf)) {
        Ok(r) if r.is_success() => r,
        Ok(r) => return refused(uri, &r),
        Err(code) => return code,
    };
    let id = reply
        .job_id()
        .map(|n| format!(" as job {n}"))
        .unwrap_or_default();
    println!("sent {} to {uri}{id} ({summary})", p.name);
    let ignored = reply.unsupported();
    if !ignored.is_empty() {
        eprintln!(
            "pdfcer: the printer ignored or substituted: {}",
            ignored.join(", ")
        );
    }
    exit::SUCCESS
}

/// Report an IPP error status. The exit code.
fn refused(uri: &str, r: &ipp::IppResponse) -> u8 {
    eprintln!(
        "pdfcer: {uri} refused: {}{}",
        ipp::status_name(r.status),
        r.status_message()
            .map(|m| format!(" ({m})"))
            .unwrap_or_default()
    );
    exit::RUNTIME_ERROR
}

/// `name=value` for a report line.
fn describe(a: &ipp::IppAttribute) -> String {
    let values: Vec<String> = a
        .values
        .iter()
        .map(|v| match v {
            ipp::IppValue::Range(lo, hi) => format!("{lo}-{hi}"),
            v => v
                .as_str()
                .map(str::to_owned)
                .or_else(|| v.as_i32().map(|n| n.to_string()))
                .unwrap_or_default(),
        })
        .collect();
    format!("{}={}", a.name, values.join(","))
}

/// Print through CUPS's `lp`, the document passed in a temporary file.
fn print_cups(dest: Option<&str>, args: &[String], p: &Prepared, send: bool) -> u8 {
    let queue = dest.unwrap_or("the default CUPS queue");
    if !send {
        eprintln!(
            "pdfcer: dry run: would run lp {} <{} bytes> on {queue}. Pass --send to print.",
            args.join(" "),
            p.pdf.len()
        );
        return exit::SUCCESS;
    }
    let path = std::env::temp_dir().join(format!("pdfcer-print-{}.pdf", std::process::id()));
    if let Err(e) = std::fs::write(&path, &p.pdf) {
        eprintln!("pdfcer: {}: {e}", path.display());
        return exit::IO_ERROR;
    }
    let run = std::process::Command::new("lp")
        .args(args)
        .arg("--")
        .arg(&path)
        .output();
    let _ = std::fs::remove_file(&path);
    match run {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("pdfcer: `lp` was not found; install CUPS, or print by ipp:// URI.");
            exit::PRINTER_UNREACHABLE
        }
        Err(e) => {
            eprintln!("pdfcer: lp: {e}");
            exit::RUNTIME_ERROR
        }
        Ok(out) if out.status.success() => {
            let said = String::from_utf8_lossy(&out.stdout);
            println!("sent {} to {queue}: {}", p.name, said.trim());
            exit::SUCCESS
        }
        Ok(out) => {
            let said = String::from_utf8_lossy(&out.stderr);
            eprintln!("pdfcer: lp failed on {queue}: {}", said.trim());
            exit::RUNTIME_ERROR
        }
    }
}

#[cfg(test)]
mod tests;
