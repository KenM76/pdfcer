//! `tac`: each page's total area coverage against a limit, measured by
//! `pdfcer_render::TacMeter` through a one-rule preflight run so the
//! preflight disclosures (RGB paint estimated, content not measured) come
//! with it.

use std::cell::{Cell, RefCell};

use pdfcer_core::preflight::{
    Check, CoverageMeter, FindingKind, Profile, ProfileRule, RunOptions, Severity, TacFailure,
    TacMeasurement, TacRequest,
};
use pdfcer_core::view::DocumentView;

use super::*;

/// Measures the selected pages and records each measurement; the rest are
/// reported clean so the run says nothing about them.
struct Recording<'a> {
    meter: pdfcer_render::TacMeter,
    selected: &'a [usize],
    max_regions: usize,
    next: Cell<usize>,
    measured: RefCell<Vec<(usize, TacMeasurement)>>,
}

impl CoverageMeter for Recording<'_> {
    fn measure_tac(
        &self,
        view: &DocumentView<'_>,
        page: &pdfcer_core::page_tree::Page,
        request: &TacRequest,
    ) -> Result<TacMeasurement, TacFailure> {
        let index = self.next.replace(self.next.get() + 1);
        if !self.selected.contains(&index) {
            return Ok(TacMeasurement::default());
        }
        let request = request.with_max_regions(self.max_regions);
        let m = self.meter.measure_tac(view, page, &request)?;
        self.measured.borrow_mut().push((index + 1, m.clone()));
        Ok(m)
    }
}

/// Print each selected page's coverage, regions and disclosures; exit 18
/// when a page is over or not fully measured.
pub(crate) fn cmd_tac(input: &Path, pages: &str, limit: f64, ppi: f64, max_regions: usize) -> u8 {
    if !(limit > 0.0 && (TacRequest::MIN_PPI..=TacRequest::MAX_PPI).contains(&ppi)) {
        eprintln!(
            "pdfcer: {}: --limit must be above 0 and --ppi {}-{}",
            input.display(),
            TacRequest::MIN_PPI,
            TacRequest::MAX_PPI
        );
        return exit::EDIT_REFUSED;
    }
    let (doc, _, indices, options) = match open_selected(input, pages) {
        Ok(opened) => opened,
        Err(code) => return code,
    };
    let profile = Profile {
        name: "tac".into(),
        description: String::new(),
        rules: vec![ProfileRule::new(
            "total-area-coverage",
            Check::TotalAreaCoverage {
                limit_percent: limit,
                ppi,
            },
            Severity::Error,
        )],
    };
    let recording = Recording {
        meter: pdfcer_render::TacMeter::with_options(options),
        selected: &indices,
        max_regions,
        next: Cell::new(0),
        measured: RefCell::new(Vec::new()),
    };
    let report = pdfcer_core::preflight::run_with_meter(
        &doc.view(),
        &profile,
        &RunOptions::default(),
        Some(&recording),
    );
    let measured = recording.measured.into_inner();
    let mut failed = false;
    for &index in &indices {
        let number = index + 1;
        match measured.iter().find(|(n, _)| *n == number) {
            Some((_, m)) => {
                failed |= m.over_pixels > 0;
                print_page(number, limit, m);
            }
            None => println!("tac page={number} measured=no"),
        }
        for f in report.findings.iter().filter(|f| f.page == Some(number)) {
            match f.kind {
                FindingKind::NeedsPerson => println!("tac-caveat page={number} \"{}\"", f.message),
                FindingKind::Incomplete => {
                    failed = true;
                    println!("tac-not-measured page={number} \"{}\"", f.message);
                }
                _ => {}
            }
        }
    }
    if failed {
        exit::PREFLIGHT_FAILED
    } else {
        exit::SUCCESS
    }
}

fn print_page(number: usize, limit: f64, m: &TacMeasurement) {
    println!(
        "tac page={number} max={:.1} over={} of={} area={:.4} ppi={} limit={limit}",
        m.max_percent,
        m.over_pixels,
        m.pixels,
        m.over_area_sq_in(),
        m.ppi
    );
    for r in &m.regions {
        println!(
            "tac-region page={number} rect={:.2},{:.2},{:.2},{:.2} max={:.1} pixels={} merged={}",
            r.rect.llx, r.rect.lly, r.rect.urx, r.rect.ury, r.max_percent, r.over_pixels, r.merged
        );
    }
}
