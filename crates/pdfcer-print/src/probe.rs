//! Reading every printer's details in the background, one thread per
//! printer, so a printer that takes a minute to answer delays only its own
//! entry.
//!
//! [`list_printer_names`] returns at once; the per-printer reads
//! ([`printer_detail`], [`device_features`], [`printer_forms`]) can each
//! wait 45–50 s on an installed but unreachable printer (see `listing`).
//! A [`PrinterProbe`] runs those reads for every printer concurrently and
//! hands each printer's answer over as it arrives. A shell starts one when
//! it opens and polls it from its UI loop; nothing in it blocks the poller.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use super::{
    DeviceFeatures, PaperForm, PrintError, Printer, PrinterName, device_features,
    list_printer_names, printer_detail, printer_forms,
};

/// Everything the per-printer reads say about one printer. Each read fails
/// on its own: a printer whose forms cannot be read can still report its
/// driver and features.
#[derive(Debug, Clone)]
pub struct PrinterReport {
    /// Driver and port ([`printer_detail`]).
    pub detail: Result<Printer, PrintError>,
    /// Duplex and copy limits ([`device_features`]).
    pub features: Result<DeviceFeatures, PrintError>,
    /// The driver's paper forms ([`printer_forms`]).
    pub forms: Result<Vec<PaperForm>, PrintError>,
}

impl PrinterReport {
    /// Run the three per-printer reads for `printer`, in order, on the
    /// calling thread. **Can block** for about a minute per read on an
    /// unreachable printer; [`PrinterProbe`] calls it off the caller's
    /// thread.
    #[must_use]
    pub fn read(printer: &str) -> Self {
        Self {
            detail: printer_detail(printer),
            features: device_features(printer),
            forms: printer_forms(printer),
        }
    }

    /// A report whose every read failed with `err`.
    fn failed(err: &PrintError) -> Self {
        Self {
            detail: Err(err.clone()),
            features: Err(err.clone()),
            forms: Err(err.clone()),
        }
    }
}

/// How a probe reads one printer: [`PrinterReport::read`] in production, a
/// fake in tests (a shell's own tests can inject a slow printer this way).
pub type ReportReader = Arc<dyn Fn(&str) -> PrinterReport + Send + Sync>;

/// The background reads of every printer's details, polled from a UI loop.
///
/// Each printer is read on its own thread, so answers arrive in whatever
/// order the printers give them and a slow printer holds back no other.
/// Dropping the probe abandons reads still in flight: their threads finish
/// on their own and their answers are discarded.
///
/// # Example
///
/// ```no_run
/// use pdfcer_print::PrinterProbe;
///
/// let mut probe = PrinterProbe::start()?;      // returns at once
/// // ... each frame:
/// if probe.poll() {
///     // something new arrived; request a repaint
/// }
/// for p in probe.printers() {
///     match probe.report(&p.name) {
///         Some(r) => println!("{}: duplex={:?}", p.name, r.features.as_ref().map(|f| f.supports_duplex)),
///         None => println!("{}: still asking", p.name),
///     }
/// }
/// # Ok::<(), pdfcer_print::PrintError>(())
/// ```
pub struct PrinterProbe {
    printers: Vec<PrinterName>,
    reports: HashMap<String, PrinterReport>,
    reader: ReportReader,
    tx: Sender<(String, PrinterReport)>,
    rx: Receiver<(String, PrinterReport)>,
}

impl std::fmt::Debug for PrinterProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrinterProbe")
            .field("printers", &self.printers)
            .field("reports", &self.reports)
            .finish_non_exhaustive()
    }
}

impl PrinterProbe {
    /// List the printers by name ([`list_printer_names`], which returns at
    /// once) and start reading every one of them in the background.
    ///
    /// # Errors
    ///
    /// Whatever [`list_printer_names`] returns: [`PrintError::Enumerate`]
    /// when the spooler cannot be queried, [`PrintError::Unsupported`] off
    /// Windows.
    pub fn start() -> Result<Self, PrintError> {
        Ok(Self::start_with(
            list_printer_names()?,
            Arc::new(PrinterReport::read),
        ))
    }

    /// Start reading `printers` with `reader`, one thread each.
    ///
    /// Where a thread cannot be started, that printer's report is every
    /// read failing with [`PrintError::Unsupported`], available at the next
    /// [`poll`](Self::poll).
    #[must_use]
    pub fn start_with(printers: Vec<PrinterName>, reader: ReportReader) -> Self {
        let (tx, rx) = channel();
        let probe = Self {
            printers,
            reports: HashMap::new(),
            reader,
            tx,
            rx,
        };
        for p in &probe.printers {
            probe.spawn(&p.name);
        }
        probe
    }

    /// Read `printer` again in the background (after the operator changes
    /// its driver settings, say). Its current report stays until the new
    /// one arrives.
    pub fn refresh(&self, printer: &str) {
        self.spawn(printer);
    }

    fn spawn(&self, printer: &str) {
        let name = printer.to_owned();
        let reader = Arc::clone(&self.reader);
        let tx = self.tx.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("printer-probe {name}"))
            .spawn({
                let name = name.clone();
                // A send fails only once the probe is dropped, when the
                // answer is meant to be discarded.
                move || drop(tx.send((name.clone(), reader(&name))))
            });
        if spawned.is_err() {
            drop(
                self.tx
                    .send((name, PrinterReport::failed(&PrintError::Unsupported))),
            );
        }
    }

    /// Take every answer that has arrived. Never blocks. Returns whether
    /// any arrived, so a UI loop knows to redraw.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok((name, report)) = self.rx.try_recv() {
            self.reports.insert(name, report);
            any = true;
        }
        any
    }

    /// The printers, in the spooler's order, with the default flagged.
    #[must_use]
    pub fn printers(&self) -> &[PrinterName] {
        &self.printers
    }

    /// `printer`'s report, once it has arrived (as of the last
    /// [`poll`](Self::poll)).
    #[must_use]
    pub fn report(&self, printer: &str) -> Option<&PrinterReport> {
        self.reports.get(printer)
    }

    /// Whether every listed printer has a report (as of the last poll).
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.printers
            .iter()
            .all(|p| self.reports.contains_key(&p.name))
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use std::sync::{Condvar, Mutex};
    use std::time::{Duration, Instant};

    fn names(list: &[&str]) -> Vec<PrinterName> {
        list.iter()
            .map(|n| PrinterName {
                name: (*n).to_owned(),
                is_default: *n == "slow",
            })
            .collect()
    }

    fn report_for(name: &str) -> PrinterReport {
        PrinterReport {
            detail: Ok(Printer {
                name: name.to_owned(),
                driver: format!("{name} driver"),
                port: String::new(),
                is_default: false,
            }),
            features: Ok(DeviceFeatures::default()),
            forms: Ok(Vec::new()),
        }
    }

    /// Poll until `done` holds or `limit` passes.
    fn poll_until(probe: &mut PrinterProbe, limit: Duration, done: impl Fn(&PrinterProbe) -> bool) {
        let end = Instant::now() + limit;
        while !done(probe) && Instant::now() < end {
            probe.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A printer held until released answers last, and the others answer
    /// while it is still held; `start_with` and `poll` never wait for it.
    #[test]
    fn a_slow_printer_holds_back_no_other() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let held = Arc::clone(&gate);
        let reader: ReportReader = Arc::new(move |name: &str| {
            if name == "slow" {
                let (lock, cv) = &*held;
                let mut open = lock.lock().unwrap();
                while !*open {
                    open = cv.wait(open).unwrap();
                }
            }
            report_for(name)
        });
        let begun = Instant::now();
        let mut probe = PrinterProbe::start_with(names(&["slow", "a", "b"]), reader);
        probe.poll();
        assert!(
            begun.elapsed() < Duration::from_secs(1),
            "start/poll waited"
        );
        poll_until(&mut probe, Duration::from_secs(10), |p| {
            p.report("a").is_some() && p.report("b").is_some()
        });
        assert!(probe.report("a").is_some() && probe.report("b").is_some());
        assert!(probe.report("slow").is_none() && !probe.is_complete());
        assert_eq!(probe.printers()[0].name, "slow");

        let (lock, cv) = &*gate;
        *lock.lock().unwrap() = true;
        cv.notify_all();
        poll_until(
            &mut probe,
            Duration::from_secs(10),
            PrinterProbe::is_complete,
        );
        let slow = probe.report("slow").expect("slow answered");
        assert_eq!(slow.detail.as_ref().unwrap().driver, "slow driver");
    }

    /// `refresh` re-reads one printer; the old report stays until the new
    /// one arrives.
    #[test]
    fn refresh_replaces_a_report_when_the_new_one_arrives() {
        let calls = Arc::new(Mutex::new(0u32));
        let counted = Arc::clone(&calls);
        let reader: ReportReader = Arc::new(move |name: &str| {
            let mut n = counted.lock().unwrap();
            *n += 1;
            let mut r = report_for(name);
            if let Ok(d) = &mut r.detail {
                d.port = format!("call {n}");
            }
            r
        });
        let mut probe = PrinterProbe::start_with(names(&["a"]), reader);
        poll_until(
            &mut probe,
            Duration::from_secs(10),
            PrinterProbe::is_complete,
        );
        let port = |p: &PrinterProbe| p.report("a").unwrap().detail.as_ref().unwrap().port.clone();
        assert_eq!(port(&probe), "call 1");
        probe.refresh("a");
        poll_until(&mut probe, Duration::from_secs(10), |p| port(p) == "call 2");
        assert_eq!(port(&probe), "call 2");
    }

    /// Dropping a probe with a read in flight neither blocks nor panics.
    #[test]
    fn dropping_a_probe_abandons_reads_in_flight() {
        let reader: ReportReader = Arc::new(|name: &str| {
            std::thread::sleep(Duration::from_millis(200));
            report_for(name)
        });
        let begun = Instant::now();
        drop(PrinterProbe::start_with(names(&["a"]), reader));
        assert!(begun.elapsed() < Duration::from_millis(150));
    }
}
