//! `print`'s finishing flags (`--staple`, `--punch`, `--punch-holes`,
//! `--fold`, `--output-bin`) and `list-finishing`. A request the printer
//! does not list is refused by name, never dropped; when the printer does
//! not list what it offers, the request is sent as given and that is said.

use pdfcer_print::finishing::{self, Edge, Finishing, Fold, Staple};
use pdfcer_print::ipp::IppResponse;

use crate::exit;

/// `--staple` on `print`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum StapleArg {
    /// One staple, top-left.
    TopLeft,
    /// One staple, top-right.
    TopRight,
    /// One staple, bottom-left.
    BottomLeft,
    /// One staple, bottom-right.
    BottomRight,
    /// Two staples, left edge.
    DualLeft,
    /// Two staples, right edge.
    DualRight,
    /// Two staples, top edge.
    DualTop,
    /// Two staples, bottom edge.
    DualBottom,
    /// Saddle stitch, through the fold of a booklet.
    Saddle,
}

const STAPLES: [(StapleArg, Staple); 9] = [
    (StapleArg::TopLeft, Staple::TopLeft),
    (StapleArg::TopRight, Staple::TopRight),
    (StapleArg::BottomLeft, Staple::BottomLeft),
    (StapleArg::BottomRight, Staple::BottomRight),
    (StapleArg::DualLeft, Staple::DualLeft),
    (StapleArg::DualRight, Staple::DualRight),
    (StapleArg::DualTop, Staple::DualTop),
    (StapleArg::DualBottom, Staple::DualBottom),
    (StapleArg::Saddle, Staple::Saddle),
];

/// `--punch` on `print`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum EdgeArg {
    /// Left edge.
    Left,
    /// Top edge.
    Top,
    /// Right edge.
    Right,
    /// Bottom edge.
    Bottom,
}

const EDGES: [(EdgeArg, Edge); 4] = [
    (EdgeArg::Left, Edge::Left),
    (EdgeArg::Top, Edge::Top),
    (EdgeArg::Right, Edge::Right),
    (EdgeArg::Bottom, Edge::Bottom),
];

/// `--fold` on `print`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum FoldArg {
    /// In half.
    Half,
    /// Letter (tri-fold).
    Letter,
    /// Z-fold.
    Z,
    /// Half, then Z.
    HalfZ,
    /// Gate.
    Gate,
    /// Double gate.
    DoubleGate,
    /// Left gate.
    LeftGate,
    /// Right gate.
    RightGate,
    /// Accordion.
    Accordion,
    /// Parallel.
    Parallel,
    /// Poster.
    Poster,
    /// Engineering Z-fold, for large drawings.
    EngineeringZ,
}

const FOLDS: [(FoldArg, Fold); 12] = [
    (FoldArg::Half, Fold::Half),
    (FoldArg::Letter, Fold::Letter),
    (FoldArg::Z, Fold::Z),
    (FoldArg::HalfZ, Fold::HalfZ),
    (FoldArg::Gate, Fold::Gate),
    (FoldArg::DoubleGate, Fold::DoubleGate),
    (FoldArg::LeftGate, Fold::LeftGate),
    (FoldArg::RightGate, Fold::RightGate),
    (FoldArg::Accordion, Fold::Accordion),
    (FoldArg::Parallel, Fold::Parallel),
    (FoldArg::Poster, Fold::Poster),
    (FoldArg::EngineeringZ, Fold::EngineeringZ),
];

fn lookup<A: PartialEq + Copy, C: PartialEq + Copy>(table: &[(A, C)], a: A) -> C {
    table
        .iter()
        .find(|(x, _)| *x == a)
        .map(|(_, c)| *c)
        .expect("every arg is in its table")
}

fn flag_name<A: clap::ValueEnum>(a: &A) -> String {
    a.to_possible_value()
        .map_or_else(String::new, |v| v.get_name().to_owned())
}

/// The finishing `print`'s flags ask for.
pub(crate) fn finishing(
    staple: Option<StapleArg>,
    punch: Option<EdgeArg>,
    holes: Option<u8>,
    fold: Option<FoldArg>,
    output_bin: Option<String>,
) -> Finishing {
    let mut f = Finishing::default();
    f.staple = staple.map(|s| lookup(&STAPLES, s));
    f.punch = punch.map(|e| lookup(&EDGES, e));
    f.holes = holes;
    f.fold = fold.map(|x| lookup(&FOLDS, x));
    f.output_bin = output_bin;
    f
}

/// The `finishings` values and `output-bin` for an IPP job to `who`,
/// checked against what its Get-Printer-Attributes reply lists (`None`
/// when there was no reply). Refusals are reported; the error is the exit
/// code.
pub(crate) fn ipp_checked(
    f: &Finishing,
    attrs: Option<&IppResponse>,
    who: &str,
) -> Result<(Vec<i32>, Option<String>), u8> {
    let supported = attrs.and_then(|a| a.printer_ints("finishings-supported"));
    let asks_finishing = f.staple.is_some() || f.punch.is_some() || f.fold.is_some();
    if asks_finishing && supported.is_none() {
        eprintln!("pdfcer: {who} does not say which finishing it offers; asking as given.");
    }
    let mut refused = finishing::ipp_finishings(f, supported.as_deref())
        .err()
        .unwrap_or_default();
    let values = finishing::ipp_finishings(f, None).unwrap_or_default();
    let bins: Vec<&str> =
        attrs.map_or_else(Vec::new, |a| a.printer_strings("output-bin-supported"));
    let bin = match &f.output_bin {
        None => None,
        Some(want) if bins.is_empty() => {
            eprintln!("pdfcer: {who} does not list its output bins; asking for {want:?} as given.");
            Some(want.clone())
        }
        Some(want) => match bins.iter().find(|b| b.eq_ignore_ascii_case(want)) {
            Some(b) => Some((*b).to_owned()),
            None => {
                refused.push(format!(
                    "output bin {want:?} (it lists {})",
                    bins.join(", ")
                ));
                None
            }
        },
    };
    if !refused.is_empty() {
        eprintln!("pdfcer: {who} does not offer: {}", refused.join("; "));
        return Err(exit::EDIT_REFUSED);
    }
    Ok((values, bin))
}

/// The `list-finishing` lines for an IPP printer's reply.
pub(crate) fn describe_ipp(attrs: &IppResponse) -> Vec<String> {
    let supported = attrs.printer_ints("finishings-supported");
    let mut lines = Vec::new();
    match &supported {
        None => lines.push("finishing: not listed by the printer".to_owned()),
        Some(list) => {
            let staples = names(&STAPLES, |s| list.contains(&s.ipp()));
            let folds = names(&FOLDS, |x| list.contains(&x.ipp()));
            lines.push(format!("--staple: {}", or_none(&staples)));
            lines.push(format!("--punch: {}", or_none(&punches(list))));
            lines.push(format!("--fold: {}", or_none(&folds)));
            let other: Vec<&str> = list
                .iter()
                .filter(|n| ![3, 4, 5, 10].contains(*n))
                .filter_map(|n| finishing::finishing_name(*n))
                .filter(|n| {
                    !n.starts_with("staple") && !n.starts_with("punch") && !n.starts_with("fold")
                })
                .collect();
            if !other.is_empty() {
                lines.push(format!("other (not settable here): {}", other.join(", ")));
            }
        }
    }
    let bins = attrs.printer_strings("output-bin-supported");
    lines.push(format!(
        "--output-bin: {}",
        if bins.is_empty() {
            "not listed".to_owned()
        } else {
            bins.join(", ")
        }
    ));
    lines
}

fn names<A: clap::ValueEnum + Copy, C: Copy>(
    table: &[(A, C)],
    offered: impl Fn(C) -> bool,
) -> Vec<String> {
    table
        .iter()
        .filter(|(_, c)| offered(*c))
        .map(|(a, _)| flag_name(a))
        .collect()
}

/// `edge` per edge offered, with its hole counts (`--punch-holes`).
fn punches(list: &[i32]) -> Vec<String> {
    const RUNS: [(&str, i32); 4] = [("2", 74), ("3", 78), ("4", 82), ("multiple", 86)];
    EDGES
        .iter()
        .enumerate()
        .filter_map(|(i, (arg, _))| {
            let offset = i32::try_from(i).unwrap_or(0);
            let counts: Vec<&str> = RUNS
                .iter()
                .filter(|(_, s)| list.contains(&(s + offset)))
                .map(|(c, _)| *c)
                .collect();
            (!counts.is_empty()).then(|| format!("{} ({} holes)", flag_name(arg), counts.join("/")))
        })
        .collect()
}

fn or_none(v: &[String]) -> String {
    if v.is_empty() {
        "none".to_owned()
    } else {
        v.join(", ")
    }
}

/// The `lp -o` pairs for `f`: `finishings` as registry values (CUPS takes
/// them comma-separated) and `output-bin`.
pub(crate) fn lp_options(values: &[i32], bin: Option<&str>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if !values.is_empty() {
        let v: Vec<String> = values.iter().map(i32::to_string).collect();
        out.push(("finishings".to_owned(), v.join(",")));
    }
    if let Some(b) = bin {
        out.push(("output-bin".to_owned(), b.to_owned()));
    }
    out
}

/// The flags of `f` for a report line.
// Only the Windows driver route reports through it; tested on every target.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn describe(f: &Finishing) -> String {
    let mut parts = Vec::new();
    if let Some(s) = f.staple {
        parts.push(format!("staple {}", flag_name(&reverse(&STAPLES, s))));
    }
    if let Some(e) = f.punch {
        let holes = f.holes.map(|h| format!(", {h} holes")).unwrap_or_default();
        parts.push(format!("punch {}{holes}", flag_name(&reverse(&EDGES, e))));
    }
    if let Some(x) = f.fold {
        parts.push(format!("fold {}", flag_name(&reverse(&FOLDS, x))));
    }
    if let Some(b) = &f.output_bin {
        parts.push(format!("output bin {b}"));
    }
    parts.join(", ")
}

// Only the Windows driver route reports through it; tested on every target.
#[cfg_attr(not(windows), allow(dead_code))]
fn reverse<A: Copy, C: PartialEq + Copy>(table: &[(A, C)], c: C) -> A {
    table
        .iter()
        .find(|(_, x)| *x == c)
        .map(|(a, _)| *a)
        .expect("every value is in its table")
}

/// `list-finishing`: what a printer offers for each finishing flag. A URI
/// is asked over IPP; off Windows a name is a CUPS queue asked at
/// localhost; on Windows a name is asked through its driver.
pub(crate) fn cmd_list_finishing(printer: Option<&str>) -> u8 {
    if crate::print_remote::is_network_printer(printer) {
        let uri = printer.unwrap_or_default();
        return print_ipp_lines(uri);
    }
    list_local(printer)
}

fn print_ipp_lines(uri: &str) -> u8 {
    match crate::print_remote::query_printer(uri) {
        Ok(attrs) => {
            describe_ipp(&attrs).iter().for_each(|l| println!("{l}"));
            exit::SUCCESS
        }
        Err(code) => code,
    }
}

#[cfg(not(windows))]
fn list_local(printer: Option<&str>) -> u8 {
    let Some(name) = printer else {
        eprintln!("pdfcer: list-finishing needs --printer: a CUPS queue name or an ipp:// URI");
        return crate::print_remote::USAGE;
    };
    print_ipp_lines(&crate::print_remote::cups_uri(name))
}

#[cfg(windows)]
fn list_local(printer: Option<&str>) -> u8 {
    let name = match crate::print::print_target(printer) {
        Ok(name) => name,
        Err(code) => return code,
    };
    match finishing::printer_finishing(&name) {
        Ok(caps) => {
            describe_driver(&caps).iter().for_each(|l| println!("{l}"));
            exit::SUCCESS
        }
        Err(err) => {
            eprintln!("pdfcer: {err}");
            exit::EDIT_REFUSED
        }
    }
}

/// The `list-finishing` lines for a Windows driver's capabilities. Fold
/// types and hole counts are not in Windows print settings, so they are
/// listed as such rather than as "none".
// Only the Windows driver route reports through it; tested on every target.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn describe_driver(caps: &finishing::print_schema::Capabilities) -> Vec<String> {
    let staples = names(&STAPLES, |s| caps.staples.contains(&s));
    let punches = names(&EDGES, |e| caps.punch.contains(&e));
    let bins: Vec<String> = caps.bins.iter().map(|b| b.name.clone()).collect();
    vec![
        format!("--staple: {}", or_none(&staples)),
        format!("--punch: {}", or_none(&punches)),
        "--punch-holes: not settable through Windows print settings".to_owned(),
        "--fold: not settable through Windows print settings".to_owned(),
        format!("--output-bin: {}", or_none(&bins)),
    ]
}

/// The job's driver settings with the requested finishing merged in by the
/// driver; `config` unchanged when nothing was asked. Refusals are
/// reported; the error is the exit code.
#[cfg(windows)]
pub(crate) fn driver_config(
    name: &str,
    config: Option<pdfcer_print::PrinterConfiguration>,
    f: &Finishing,
) -> Result<Option<pdfcer_print::PrinterConfiguration>, u8> {
    if f.is_empty() {
        return Ok(config);
    }
    let base = match config {
        Some(c) => c,
        None => pdfcer_print::printer_configuration(name).map_err(|err| {
            eprintln!("pdfcer: {err}");
            exit::EDIT_REFUSED
        })?,
    };
    match finishing::apply_finishing(name, &base, f) {
        Ok(merged) => {
            eprintln!(
                "pdfcer: finishing set by {name:?}'s driver: {}",
                describe(f)
            );
            Ok(Some(merged))
        }
        Err(err) => {
            eprintln!("pdfcer: {err}");
            Err(exit::EDIT_REFUSED)
        }
    }
}

#[cfg(test)]
mod tests;
