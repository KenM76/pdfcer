//! `print --pdf-direct`: send the PDF itself to an installed queue's
//! printer when the printer reads PDF, instead of rasterising through the
//! driver.
//!
//! The queue's port is resolved to IPP printer URIs
//! ([`pdfcer_print::ipp::resolve_queue_port`]); each is asked
//! Get-Printer-Attributes (RFC 8011 §4.2.5) and the first that lists
//! `application/pdf` in `document-format-supported` gets a Print-Job
//! (§4.2.1). A job shape the printer cannot be asked for (n-up, booklet,
//! poster, a paper or percentage scale) is composed into a new PDF first
//! ([`crate::print_compose`]). Nothing touches the network unless the
//! operator ran `print` with `auto` or `on`.

use std::path::Path;

use pdfcer_print::ipp::{self, PortSource, QueuePort};

use super::*;
use crate::print_compose::{Composed, Paper, Request, Shape};
use crate::print_output::PrintOutput;
use crate::print_remote::{Prepared, RemoteJob, Staged};

/// `--pdf-direct` on `print`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum PdfDirectArg {
    /// Print through the driver.
    #[default]
    Off,
    /// Send the PDF when the printer reads it; otherwise use the driver.
    Auto,
    /// Send the PDF, or refuse.
    On,
}

/// A `print` job that may go straight to the printer.
pub(crate) struct DirectJob<'a> {
    pub mode: PdfDirectArg,
    /// The job; `layout_flags` holds the flags given that only the driver
    /// route can honour.
    pub remote: RemoteJob<'a>,
    pub reverse: bool,
    pub shape: Shape,
    /// `--paper` or `--paper-size` was given, so the sheets are composed
    /// for that paper.
    pub resize: bool,
    /// Flags given that have no effect on this route.
    pub ignored: Vec<&'static str>,
}

/// The job's shape from `print`'s flags, with the planner settings the
/// driver route uses; [`Shape::Pages`] when none is given.
pub(crate) fn shape(
    n_up: Option<(u32, bool)>,
    booklet: Option<(BindingArg, BookletSubsetArg)>,
    poster: Option<pdfcer_print::imposition::PosterSpec>,
) -> Shape {
    use pdfcer_print::imposition::{BookletSpec, NUpGrid, NUpSpec, PageOrder};
    if let Some((count, border)) = n_up {
        return Shape::NUp(NUpSpec {
            grid: NUpGrid::Count(count),
            order: PageOrder::Horizontal,
            border,
            auto_rotate: true,
        });
    }
    if let Some((binding, subset)) = booklet {
        return Shape::Booklet(BookletSpec {
            binding: binding.to_binding(),
            subset: subset.to_subset(),
            sheets: None,
            auto_rotate: true,
        });
    }
    poster.map_or(Shape::Pages, Shape::Poster)
}

/// Why the job is not sent straight to the printer.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Fallback {
    /// The `fallback=` keyword on the report line.
    pub keyword: &'static str,
    /// A plain sentence.
    pub why: String,
    /// The exit code under `--pdf-direct on`.
    pub code: u8,
}

impl Fallback {
    fn new(keyword: &'static str, why: String, code: u8) -> Self {
        Self { keyword, why, code }
    }
}

/// Print `input` per `job`: through `driver` for `off`, else straight to
/// the printer when it reads PDF. The exit code.
pub(crate) fn print_direct(
    input: &Path,
    job: &DirectJob<'_>,
    paper: Option<&str>,
    paper_size: Option<&str>,
    driver: &mut dyn FnMut(&Path, &PrintOutput) -> u8,
) -> u8 {
    if job.mode == PdfDirectArg::Off {
        return driver(input, job.remote.output);
    }
    #[cfg(windows)]
    {
        let name = match crate::print::print_target(job.remote.printer) {
            Ok(n) => n,
            Err(code) => return code,
        };
        let queue = || pdfcer_print::queue_port(&name).map_err(|e| e.to_string());
        let sheet = || driver_paper(&name, paper, paper_size);
        direct_to(input, job, &name, &queue, &sheet, driver)
    }
    #[cfg(not(windows))]
    {
        let _ = (paper, paper_size);
        let name = job.remote.printer.unwrap_or_default();
        let queue = || Err("printer queues here are CUPS's, which already receive the PDF".into());
        let sheet = || Err(exit::EDIT_REFUSED);
        direct_to(input, job, name, &queue, &sheet, driver)
    }
}

/// The paper the driver would print this job on, for composing.
#[cfg(windows)]
fn driver_paper(name: &str, paper: Option<&str>, paper_size: Option<&str>) -> Result<Paper, u8> {
    let (selection, _) = crate::print::resolve_paper(name, paper, paper_size)?;
    let caps = pdfcer_print::printer_caps_for(name, None, selection).map_err(|e| {
        eprintln!("pdfcer: {e}");
        exit::EDIT_REFUSED
    })?;
    Ok(Paper::portrait(
        caps.physical_pt,
        caps.printable_pt,
        caps.offset_pt,
    ))
}

/// The direct route for queue `name`, whose port `queue` reads and whose
/// paper `paper` measures; `driver` when it falls back. The exit code.
pub(crate) fn direct_to(
    input: &Path,
    job: &DirectJob<'_>,
    name: &str,
    queue: &dyn Fn() -> Result<QueuePort, String>,
    paper: &dyn Fn() -> Result<Paper, u8>,
    driver: &mut dyn FnMut(&Path, &PrintOutput) -> u8,
) -> u8 {
    if let Some(flags) =
        (!job.remote.layout_flags.is_empty()).then(|| job.remote.layout_flags.join(", "))
    {
        let why = format!("{flags} need the driver");
        return fall_back(
            input,
            job,
            Fallback::new("driver-only-option", why, exit::EDIT_REFUSED),
            driver,
        );
    }
    let (uri, offer, source) = match find_pdf_printer(name, queue) {
        Ok(found) => found,
        Err(fb) => return fall_back(input, job, fb, driver),
    };
    eprintln!(
        "pdfcer: {name:?} reads PDF at {uri} (found from {}), so the PDF is sent to it directly.",
        source.describe()
    );
    for flag in &job.ignored {
        eprintln!("pdfcer: {flag} has no effect when the PDF is sent directly.");
    }
    let staged = match crate::print_remote::stage(input, &job.remote) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let (prepared, pages, composed) = match prepare(input, job, staged, paper) {
        Ok(p) => p,
        Err(Stop::Fallback(fb)) => return fall_back(input, job, fb, driver),
        Err(Stop::Exit(code)) => return code,
    };
    let mut ipp_job = crate::print_remote::ipp_job(&uri, &job.remote, &prepared);
    if composed {
        // The sheets are already laid out on the paper, in its orientation.
        ipp_job.print_scaling = Some("none".to_owned());
        ipp_job.orientation = None;
    }
    let finishing = &job.remote.output.finishing;
    let sent = match crate::print_remote::send_prepared(
        &uri,
        &offer,
        ipp_job,
        finishing,
        &prepared,
        job.remote.send,
    ) {
        Ok(s) => s,
        Err(code) => return code,
    };
    println!(
        "print {} printer={name:?} route=pdf-direct uri={uri} pages={pages} composed={} printed={} job={}",
        input.display(),
        u8::from(composed),
        u8::from(sent.is_some()),
        sent.and_then(|s| s.job_id)
            .map_or_else(|| "-".to_owned(), |j| j.to_string()),
    );
    exit::SUCCESS
}

/// The first of the queue's printer URIs that answers and reads PDF.
fn find_pdf_printer(
    name: &str,
    queue: &dyn Fn() -> Result<QueuePort, String>,
) -> Result<(String, ipp::PrinterOffer, PortSource), Fallback> {
    let no_address = |why: String| Fallback::new("no-network-address", why, exit::EDIT_REFUSED);
    let port =
        queue().map_err(|e| no_address(format!("{name:?}'s port could not be read: {e}")))?;
    let found =
        ipp::resolve_queue_port(&port).map_err(|e| no_address(format!("{name:?}'s {e}")))?;
    let mut answered = None;
    let mut failures = Vec::new();
    for uri in &found.uris {
        match ipp::query_printer(
            uri,
            &crate::print_remote::user_name(),
            &mut crate::print_remote::transport(),
        ) {
            Ok(offer) if offer.accepts_pdf() => return Ok((uri.clone(), offer, found.source)),
            Ok(offer) => answered = Some((uri.clone(), offer)),
            Err(e) => failures.push(e.to_string()),
        }
    }
    if let Some((uri, offer)) = answered {
        let model = offer
            .make_and_model
            .map(|m| format!(" ({m})"))
            .unwrap_or_default();
        let why =
            format!("the printer at {uri}{model} does not list PDF among the formats it reads");
        return Err(Fallback::new("no-pdf", why, exit::EDIT_REFUSED));
    }
    let why = format!(
        "no printer answered at {}: {}",
        found.uris.join(", "),
        failures.join("; ")
    );
    Err(Fallback::new("unreachable", why, exit::PRINTER_UNREACHABLE))
}

/// Refuse under `on`; print through the driver under `auto`, saying why.
fn fall_back(
    input: &Path,
    job: &DirectJob<'_>,
    fb: Fallback,
    driver: &mut dyn FnMut(&Path, &PrintOutput) -> u8,
) -> u8 {
    if job.mode == PdfDirectArg::On {
        eprintln!(
            "pdfcer: --pdf-direct on: the PDF cannot be sent directly: {}.",
            fb.why
        );
        return fb.code;
    }
    eprintln!("pdfcer: printing through the driver: {}.", fb.why);
    let mut output = job.remote.output.clone();
    output.route = Some(format!("route=driver fallback={}", fb.keyword));
    driver(input, &output)
}

/// How preparing stopped.
enum Stop {
    Fallback(Fallback),
    Exit(u8),
}

/// The pages to print in order: the selection, the subset, then reversed.
pub(crate) fn sequence(selected: &[usize], subset: SubsetArg, reverse: bool) -> Vec<usize> {
    let mut pages: Vec<usize> = selected
        .iter()
        .copied()
        .filter(|i| match subset {
            SubsetArg::All => true,
            SubsetArg::Odd => i.is_multiple_of(2),
            SubsetArg::Even => !i.is_multiple_of(2),
        })
        .collect();
    if reverse {
        pages.reverse();
    }
    pages
}

/// The bytes to send, the page count, and whether they were composed.
fn prepare(
    input: &Path,
    job: &DirectJob<'_>,
    mut staged: Staged,
    paper: &dyn Fn() -> Result<Paper, u8>,
) -> Result<(Prepared, usize, bool), Stop> {
    let order = sequence(&staged.selected, job.remote.subset, job.reverse);
    if order.is_empty() {
        eprintln!("pdfcer: {}: no pages left to print", input.display());
        return Err(Stop::Exit(exit::RUNTIME_ERROR));
    }
    let compose = job.shape != Shape::Pages || job.resize || staged.job.scale_percent.is_some();
    if compose {
        let composed = compose_sheets(&mut staged, &order, job, paper)?;
        let prepared = Prepared {
            pdf: composed.pdf,
            name: staged.name,
            job: staged.job,
            ranges: Vec::new(),
        };
        return Ok((prepared, composed.pages, true));
    }
    let ranges = if job.reverse {
        reorder(&mut staged.session, &order, staged.count).map_err(|e| {
            eprintln!("pdfcer: {}: {e}", input.display());
            Stop::Exit(exit::EDIT_REFUSED)
        })?;
        Vec::new()
    } else {
        crate::print_remote::page_ranges(&staged.selected, job.remote.subset, staged.count)
            .unwrap_or_default()
    };
    let pdf = crate::print_remote::session_bytes(input, &staged.session).map_err(Stop::Exit)?;
    let prepared = Prepared {
        pdf,
        name: staged.name,
        job: staged.job,
        ranges,
    };
    Ok((prepared, order.len(), false))
}

/// Keep only `order`'s pages, in that order (`order` holds distinct pages).
fn reorder(
    session: &mut pdfcer_core::edit::EditSession,
    order: &[usize],
    count: usize,
) -> Result<(), pdfcer_core::edit::EditError> {
    let drop: Vec<usize> = (0..count).filter(|i| !order.contains(i)).collect();
    if !drop.is_empty() {
        session.delete_pages(&drop)?;
    }
    // After the deletion the kept pages sit in document order; `order`
    // names them by their old indices.
    let mut kept: Vec<usize> = order.to_vec();
    kept.sort_unstable();
    let new_order: Vec<usize> = order
        .iter()
        .filter_map(|p| kept.binary_search(p).ok())
        .collect();
    if new_order.iter().enumerate().any(|(i, &p)| i != p) {
        session.reorder_pages(&new_order)?;
    }
    Ok(())
}

/// Compose `order` on the job's paper, reporting what it decided.
fn compose_sheets(
    staged: &mut Staged,
    order: &[usize],
    job: &DirectJob<'_>,
    paper: &dyn Fn() -> Result<Paper, u8>,
) -> Result<Composed, Stop> {
    let paper = paper().map_err(Stop::Exit)?;
    let mode = staged.job.scale_percent.map_or_else(
        || staged.job.scale.to_mode(),
        |pct| pdfcer_print::ScaleMode::Custom(f64::from(pct) / 100.0),
    );
    let req = Request {
        paper,
        shape: job.shape,
        mode,
        orientation: job.remote.orientation.to_orientation(),
        label: &staged.name,
    };
    let composed = crate::print_compose::compose(&mut staged.session, order, &req)
        .map_err(|why| Stop::Fallback(Fallback::new("cannot-compose", why, exit::EDIT_REFUSED)))?;
    composed.notes.iter().for_each(|n| eprintln!("pdfcer: {n}"));
    let (w, h) = paper.physical_pt;
    eprintln!(
        "pdfcer: composed {} sheet side(s) of {w:.0}x{h:.0} pt for this job; the printer \
         prints them at actual size, so load that paper.",
        composed.pages
    );
    Ok(composed)
}

#[cfg(test)]
mod tests;
