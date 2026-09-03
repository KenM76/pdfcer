//! # Printing — Windows print-system access for `pdfcer`
//!
//! The Reader-parity sweep's largest gap. Acrobat Reader's most-used
//! function after viewing is printing, and pdfcer had **no print code
//! anywhere in the workspace** before this module.
//!
//! ## Why this lives in the CLI crate and not in `pdfcer-core`
//!
//! Printing is the one genuinely platform-bound capability pdfcer needs.
//! `pdfcer-core` and `pdfcer-render` must never gain a platform or
//! windowing dependency — that invariant (project rule 2,
//! `ARCHITECTURE.md` §3) is what keeps the eventual web/WASM fork a
//! shell-crate swap rather than a rewrite, and a `windows` dependency in
//! core would end it as surely as an `egui` one.
//!
//! So the split is: **core rasterises, the shell spools.**
//! `pdfcer_render::render_page` produces an RGBA pixmap from a page on any
//! platform; this module is the Windows-only half that hands those pixels
//! to a printer. The GUI will call the same code for the same reason.
//!
//! The whole module is `#[cfg(windows)]` at its use site, and the
//! `windows` crate is declared under `[target.'cfg(windows)'.dependencies]`
//! so the Linux and macOS CI jobs still compile this crate — a compile
//! signal that the codebase stays platform-clean (R10), never a support
//! claim (R9).
//!
//! ## Not a new dependency
//!
//! The `windows` crate was ALREADY in the workspace tree at 0.62, pulled
//! transitively by eframe/winit, MIT-OR-Apache-2.0, already listed in the
//! generated `THIRD_PARTY_LICENSES.md`. Verified with `cargo tree` before
//! adding rather than assumed — project rule 13 makes classifying a
//! dependency a precondition, and "it was already there" is a claim that
//! has to be checked like any other.
//!
//! ## What this module does NOT do yet, stated plainly
//!
//! # Its own crate, and why not `pdfcer-core`
//!
//! This began as a module inside `pdfcer`. It moved when the GUI
//! needed it, and it moved OUT rather than DOWN.
//!
//! `pdfcer-core` and `pdfcer-render` must not gain a platform dependency:
//! that is the invariant (`ARCHITECTURE.md` §3) which keeps the eventual
//! web/WASM fork a shell-crate swap instead of a rewrite, and a print
//! spooler is about as platform-bound as code gets. Putting it in either
//! would trade a load-bearing property for the convenience of one fewer
//! manifest.
//!
//! The alternative — a copy in each shell — fails for the ordinary
//! reason: two copies of page-placement arithmetic drift, and the
//! symptom is a GUI print that lands differently from a CLI print of the
//! same document, which nobody would look for.
//!
//! So: one crate, two shells, and `windows` confined to the only place
//! in the workspace that talks to a spooler.
//!
//! # ★ Spooling is an irreversible outward-facing act
//!
//! Printing consumes paper, occupies a device other people may share,
//! and cannot be undone. Nothing in this crate starts a job as a side
//! effect of anything else: [`spool`] is the only function that reaches
//! `StartDoc`, and it is reached only from a control an operator
//! deliberately clicked.
//!
//! [`DryRun::Yes`] exists so that the whole path — device context,
//! `DEVMODE`, capability query, placement, rasterisation, the per-page
//! loop — can be exercised and verified without a sheet of paper moving.
//! That is not a testing convenience bolted on afterwards; it is how this
//! code was developed, because the machine it was written on has one
//! printer and its owner was sitting at it.
//!
//! ## The rendering approach, and how it differs from Reader
//!
//! Reader sends **vector and text natively to the print driver**, which
//! RIPs at print time; "Print as Image" is a separate, explicitly-invoked
//! fallback for driver bugs and damaged content
//! (`Acrobat_Features/printing__rendering_pipeline_and_resolution.md`).
//!
//! pdfcer's planned first slice rasterises — i.e. it makes Reader's
//! *fallback* the default. That is an honest limitation, not a hidden
//! one: a raster print of a vector CAD drawing at 300 DPI is visibly
//! coarser than the driver's own RIP would produce, and an operator
//! printing a drawing needs to be told that rather than discovering it on
//! paper. Emitting vector to a GDI device context means a second
//! rendering backend targeting GDI primitives, which is a substantial
//! piece of work and a later slice.
//!
//! Memory is the constraint that decides the default resolution: an A4
//! page at 600 DPI is 4960×7016 px, which at RGBA is ~139 MB for one
//! page. At 300 DPI it is ~35 MB. So a cap exists, and when it binds it
//! is disclosed — pdfcer chose a resolution the operator did not ask for,
//! which is exactly rule 4's territory.

// NOTE: this module is NOT wholly `cfg(windows)`. The page-placement
// math below is pure geometry with no platform dependency, and it is the
// part most worth unit-testing — so it compiles and its tests run on the
// Linux and macOS CI jobs too. Only the spooler-facing half is gated.

// Imposition — N-up, booklet and poster. Pure geometry, deliberately NOT
// `cfg`-gated, for the reason stated in the note above: it is the part
// most worth testing, and gating it would delete its coverage on two of
// three CI jobs without telling anyone.
pub mod imposition;

// The `DEVMODE` model — the driver's own settings structure, sourced
// from the driver rather than synthesised. Also deliberately NOT
// `cfg`-gated: the amend-a-DEVMODE logic is where the bugs are, it is
// pure byte arithmetic over a documented ABI, and gating it would delete
// its coverage on two of three CI jobs. Only ACQUIRING and USING a
// configuration is Windows-only. See the module's own docs for the
// three-defects-one-cause history that produced it.
mod devmode;

pub use devmode::{
    ConfigurationError, ConfigurationSummary, MAX_CUSTOM_SHEET_TENTHS_MM, PaperForm,
    PaperSelection, PrinterConfiguration,
};

// Un-gated: `PrintError`'s Display impl needs it on every platform. See
// that type's own note for why the error type is not Windows-only.
use std::fmt;

/// One printer the system knows about.
///
/// Not `cfg(windows)`: it holds four `String`/`bool` fields and no Win32
/// handle. Only the code that FILLS it is Windows-only. See [`PrintError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Printer {
    /// The printer's name, as the spooler reports it. This is the string
    /// a caller passes to `--printer`.
    pub name: String,
    /// The driver's name, for disambiguation. Two printers can share a
    /// human-readable name closely enough that an operator cannot tell
    /// which is which; the driver usually distinguishes them.
    pub driver: String,
    /// The port, for the same reason.
    pub port: String,
    /// Whether this is the system default.
    pub is_default: bool,
}

/// Why the print system could not be queried.
///
/// **Not `cfg(windows)`, and that is load-bearing rather than tidy.** Every
/// non-Windows stub in this file returns `Result<_, PrintError>` — that is
/// how they say "printing is a Windows capability in this release" in the
/// type system rather than by refusing to compile. Gating the enum meant
/// those stubs referenced a type that did not exist, so `pdfcer-print` did
/// not build for **any** non-Windows target, including the `wasm32` one the
/// future web shell depends on.
///
/// The variants carry only `u32` and `String`, never a Win32 handle, so
/// there is nothing platform-specific to gate in the first place. What is
/// Windows-only is the code that *constructs* them, and that is still gated
/// individually.
#[derive(Debug, Clone)]
pub enum PrintError {
    /// `EnumPrinters` failed. Carries the Win32 error code, because
    /// "could not list printers" without one is unactionable.
    Enumerate(u32),
    /// A printer name did not resolve to a device. Carries the name,
    /// because the overwhelmingly common cause is a typo and a generic
    /// failure leaves the operator nothing to compare against.
    OpenDevice(String),
    /// The driver reported a resolution of zero, which is malformed.
    /// Named rather than worked around: dividing by it would produce
    /// infinities that reach the placement math and emerge as a blank
    /// page with no explanation.
    NoResolution(String),
    /// `CreateDC` returned no device context for a printer that
    /// enumerated. Distinct from [`PrintError::OpenDevice`]: the name
    /// resolved and the DEVICE still refused, which usually means a
    /// driver problem rather than a typo, and sends the operator
    /// somewhere different.
    DeviceContext {
        /// The printer that refused.
        printer: String,
    },
    /// `StartDoc` failed. **No job exists**, so nothing is queued and
    /// nothing needs cancelling.
    JobStart {
        /// The printer that refused the job.
        printer: String,
    },
    /// `StartPage` failed part-way through a job. The job is aborted.
    PageStart,
    /// `ResetDC` failed part-way through a job — the driver refused a
    /// mid-job change of orientation or paper. The job is aborted.
    ///
    /// An error rather than a degradation, deliberately: continuing
    /// would print every remaining sheet in the previous setup, which is
    /// silently-wrong output, and silently-wrong output is the failure
    /// mode this crate's whole settings path was rebuilt to remove.
    SheetSetup,
    /// `EndPage` failed part-way through a job. The job is aborted.
    PageEnd,
    /// `EndDoc` failed. The job may or may not have reached the device —
    /// stated as the uncertainty it is, because claiming either would be
    /// a guess about a queue this process no longer controls.
    JobEnd,
    /// `StretchDIBits` drew nothing.
    Blit,
    /// A page's pixel dimensions exceed what GDI accepts.
    PageTooLarge,
    /// `DocumentProperties` would not report a device's own settings.
    ///
    /// Distinct from [`PrintError::OpenDevice`]: the printer resolved
    /// and the SPOOLER declined to describe it, which is what a
    /// disconnected network printer does. A job can still be sent — see
    /// [`SettingsSource::Synthesised`] for what is lost when it is.
    DriverSettings {
        /// The printer whose settings could not be read.
        printer: String,
    },
    /// A [`PrinterConfiguration`] was not usable. Carries the specific
    /// reason rather than a generic parse failure, because the reasons
    /// send an operator somewhere different — see [`ConfigurationError`].
    Configuration(ConfigurationError),
    /// Printing is not available on this platform.
    Unsupported,
}

impl From<ConfigurationError> for PrintError {
    fn from(err: ConfigurationError) -> Self {
        Self::Configuration(err)
    }
}

/// Not derived, because [`PrintError`] predates it and hand-rolls
/// [`fmt::Display`]; and not omitted, because a public error type that
/// does not implement [`std::error::Error`] cannot be boxed, cannot be a
/// `source()`, and does not compose with `?` in a caller that uses
/// `Box<dyn Error>` — Rust API Guidelines `C-GOOD-ERR`. It was missing
/// from this crate's only error type until 2026-08-18; `ImpositionError`
/// in the sibling module has had it all along via `thiserror`, so the
/// two halves of one crate disagreed about whether their errors were
/// errors.
impl std::error::Error for PrintError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Configuration(err) => Some(err),
            _ => None,
        }
    }
}

// Un-gated for the same reason as the enum: a non-Windows caller that gets
// `PrintError::Unsupported` back must be able to PRINT it, and an error type
// whose Display exists on only one platform is not a usable error type.
impl fmt::Display for PrintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Enumerate(code) => write!(
                f,
                "the Windows print spooler could not be queried (error {code}) — \
                 the Print Spooler service may be stopped"
            ),
            Self::OpenDevice(name) => write!(
                f,
                "no printer named {name:?} — run `pdfcer list-printers` to see \
                 the names this machine knows"
            ),
            Self::DeviceContext { printer } => write!(
                f,
                "the printer {printer:?} was found but its driver would not open a device; this is usually a driver problem rather than a wrong name"
            ),
            Self::JobStart { printer } => write!(
                f,
                "{printer:?} refused the print job. Nothing was queued, so there is nothing to cancel"
            ),
            Self::PageStart => write!(f, "the printer refused a page; the job was cancelled"),
            Self::SheetSetup => write!(
                f,
                "the printer refused to change orientation or paper part-way through the job; \
                 the job was cancelled rather than printing the rest the wrong way up"
            ),
            Self::PageEnd => write!(f, "a page failed to finish; the job was cancelled"),
            Self::JobEnd => write!(
                f,
                "the job did not close cleanly. Some pages may already have reached the printer — check the queue rather than reprinting blind"
            ),
            Self::Blit => write!(f, "the page image could not be drawn to the printer"),
            Self::Unsupported => write!(
                f,
                "printing is not available on this platform in this release"
            ),
            Self::PageTooLarge => write!(
                f,
                "the page is too large in pixels for the print system; try a lower resolution"
            ),
            Self::NoResolution(name) => write!(
                f,
                "the driver for {name:?} reports a resolution of zero dots per inch, \
                 which pdfcer cannot lay a page out against"
            ),
            Self::DriverSettings { printer } => write!(
                f,
                "the spooler would not report {printer:?}'s own settings, so pdfcer cannot \
                 start from them — a disconnected network printer does this. The job can \
                 still be sent, but only the settings pdfcer sets itself will apply"
            ),
            Self::Configuration(err) => write!(f, "{err}"),
        }
    }
}

/// List the printers this machine can reach.
///
/// # Why `PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS`
///
/// `LOCAL` alone misses network printers the user has connected to,
/// which on a workstation is usually *most* of them — an operator whose
/// office printer is absent from the list would reasonably conclude
/// pdfcer cannot see it. `CONNECTIONS` adds exactly those.
///
/// # The two-call pattern is required, not defensive
///
/// `EnumPrinters` is called twice by design: the first call fails with
/// `ERROR_INSUFFICIENT_BUFFER` and reports the byte count needed, the
/// second fills it. There is no way to ask for the size alone, and
/// guessing a buffer size would either truncate the list silently or
/// waste memory on every call.
///
/// # Errors
///
/// [`PrintError::Enumerate`] when the spooler cannot be queried at all.
/// An empty list is NOT an error — a machine with no printers installed
/// is a normal machine, and reporting that as a failure would send a
/// caller looking for a fault that does not exist.
#[cfg(windows)]
pub fn list_printers() -> Result<Vec<Printer>, PrintError> {
    use windows::Win32::Graphics::Printing::{
        EnumPrintersW, GetDefaultPrinterW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL,
        PRINTER_INFO_2W,
    };

    // SAFETY: the two-call pattern below is the documented contract for
    // `EnumPrintersW`. The first call is expected to fail; its purpose is
    // to write the required byte count into `needed`.
    let mut needed: u32 = 0;
    let mut returned: u32 = 0;
    unsafe {
        // Deliberately ignoring the result: this call is EXPECTED to fail
        // with ERROR_INSUFFICIENT_BUFFER, and treating that as an error
        // would make the happy path unreachable.
        let _ = EnumPrintersW(
            PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS,
            None,
            2,
            None,
            &mut needed,
            &mut returned,
        );
    }
    if needed == 0 {
        // No printers at all. Not an error — see this function's docs.
        return Ok(Vec::new());
    }

    let mut buffer = vec![0u8; needed as usize];
    // SAFETY: `buffer` is `needed` bytes, which is the size the call above
    // asked for.
    unsafe {
        EnumPrintersW(
            PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS,
            None,
            2,
            Some(&mut buffer),
            &mut needed,
            &mut returned,
        )
    }
    .map_err(|e| PrintError::Enumerate(e.code().0.unsigned_abs()))?;

    // The default printer's name, for flagging. A failure here is not
    // fatal: not knowing which is default is a smaller loss than
    // reporting no printers at all, so it degrades to "none flagged".
    let default_name = {
        let mut len: u32 = 0;
        // SAFETY: same two-call pattern; the first call reports the length.
        unsafe {
            let _ = GetDefaultPrinterW(None, &mut len);
        }
        if len == 0 {
            String::new()
        } else {
            let mut buf = vec![0u16; len as usize];
            // SAFETY: `buf` holds `len` UTF-16 units, as just requested.
            // Returns BOOL, not Result — unlike `EnumPrintersW` in the same
            // module, which does return Result. The `windows` crate maps
            // each API to whatever its own signature is, so the two sit
            // side by side with different shapes.
            let ok = unsafe {
                GetDefaultPrinterW(Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len)
            }
            .as_bool();
            if ok {
                utf16_to_string(&buf)
            } else {
                String::new()
            }
        }
    };

    let mut out = Vec::with_capacity(returned as usize);
    // SAFETY: the spooler wrote `returned` contiguous `PRINTER_INFO_2W`
    // records at the head of `buffer`; the pointers inside them point into
    // the same allocation, which outlives this loop.
    let infos = unsafe {
        std::slice::from_raw_parts(buffer.as_ptr().cast::<PRINTER_INFO_2W>(), returned as usize)
    };
    for info in infos {
        // SAFETY: these are NUL-terminated UTF-16 strings inside `buffer`.
        let name = unsafe { pwstr_to_string(info.pPrinterName) };
        let driver = unsafe { pwstr_to_string(info.pDriverName) };
        let port = unsafe { pwstr_to_string(info.pPortName) };
        if name.is_empty() {
            // A nameless printer cannot be targeted by `--printer`, so
            // listing it would offer something unusable (R83).
            continue;
        }
        out.push(Printer {
            is_default: !default_name.is_empty() && name == default_name,
            name,
            driver,
            port,
        });
    }
    Ok(out)
}

/// Decode a NUL-terminated wide string the spooler owns.
///
/// # Safety
///
/// `p` must be either null or a pointer to a NUL-terminated UTF-16 string
/// that remains valid for the duration of the call.
#[cfg(windows)]
unsafe fn pwstr_to_string(p: windows::core::PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: the caller guarantees NUL-termination and validity.
    unsafe { p.to_string() }.unwrap_or_default()
}

/// Decode a UTF-16 buffer that may carry a trailing NUL.
#[cfg(windows)]
fn utf16_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(buf.get(..end).unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Printer capabilities — Windows, read-only, starts no job
// ---------------------------------------------------------------------------

/// What a printer can physically do with a sheet.
///
/// Every measurement is in **points** (1/72 inch), converted from the
/// device's own pixels here so nothing downstream has to know the DPI.
/// That conversion is the one place a printing bug hides most easily:
/// mixing device pixels and points silently produces output that is right
/// on one printer and wrong on the next.
/// Not `cfg(windows)`, for the same reason as [`Printer`]: it is `u32` and
/// `f64` pairs, and `DeviceGeometry::from_caps` — which is pure geometry and
/// deliberately un-gated so it tests on every platform — takes one by
/// reference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrinterCaps {
    /// Horizontal resolution in dots per inch.
    pub dpi_x: u32,
    /// Vertical resolution.
    pub dpi_y: u32,
    /// The full sheet, in points.
    pub physical_pt: (f64, f64),
    /// The area the hardware can actually mark, in points.
    ///
    /// Always smaller than [`Self::physical_pt`]. Fitting a page to the
    /// PHYSICAL size instead of this one produces a page whose edges the
    /// hardware crops — which looks exactly like a pdfcer bug and is not
    /// one.
    pub printable_pt: (f64, f64),
    /// Where the printable area begins relative to the sheet corner, in
    /// points. Needed because GDI's drawing origin is the printable
    /// corner, not the paper corner.
    pub offset_pt: (f64, f64),
}

/// Query a printer's capabilities.
///
/// Opens an information device context, reads it, and closes it. **It
/// starts no print job** — `CreateDC` on a printer is a read of the
/// driver's configuration, not a spool operation, so this is safe to run
/// on a machine somebody is using.
///
/// # Errors
///
/// [`PrintError::OpenDevice`] when the printer name does not resolve.
/// The most common cause is a typo, so the error names the string that
/// failed rather than reporting a generic failure.
#[cfg(windows)]
pub fn printer_caps(name: &str) -> Result<PrinterCaps, PrintError> {
    printer_caps_for(name, None, PaperSelection::DeviceDefault)
}

/// A printer's capabilities **for the sheet a specific job will use**.
///
/// # ★ Why selecting paper without this would be a new instance of an
/// # old bug
///
/// [`printer_caps`] opens an information device context with the
/// device's DEFAULT `DEVMODE` and reports the geometry of whatever sheet
/// that names. Every placement pdfcer computes — [`place_page`],
/// [`plan_job`], every `imposition` layout, the GUI preview — is
/// computed against that geometry.
///
/// So a job that asks for A3 while the device's default is Letter would,
/// without this function, be PLANNED for Letter and PRINTED on A3: the
/// two halves describing different sheets, with no clip reported and
/// nothing to explain it. That is exactly the defect
/// [`DeviceGeometry::for_orientation`] exists to make unrepresentable,
/// in a new dimension — it is written out in full there, and the same
/// reasoning applies here without restating it.
///
/// A caller that changes the sheet must therefore read its geometry
/// through this function and plan against THAT. `config` covers the same
/// hazard for a configuration an operator edited in the driver's own
/// dialog, where the sheet may have been changed by hand.
///
/// # What is deliberately NOT applied
///
/// Orientation. This reports the sheet as the driver holds it, un-turned,
/// because [`DeviceGeometry::from_caps`] is the one place rotation is
/// written and a second rotation here would eventually disagree with it.
///
/// # Errors
///
/// The same as [`printer_caps`], plus [`PrintError::Configuration`] when
/// `config` belongs to a different device.
#[cfg(windows)]
pub fn printer_caps_for(
    name: &str,
    config: Option<&PrinterConfiguration>,
    paper: PaperSelection,
) -> Result<PrinterCaps, PrintError> {
    use windows::Win32::Graphics::Gdi::{
        CreateDCW, DEVMODEW, DeleteDC, GetDeviceCaps, HORZRES, LOGPIXELSX, LOGPIXELSY,
        PHYSICALHEIGHT, PHYSICALOFFSETX, PHYSICALOFFSETY, PHYSICALWIDTH, VERTRES,
    };
    use windows::core::HSTRING;

    // Nothing changes the sheet, so nothing needs a `DEVMODE` — the
    // cheap path, and the one every existing caller takes.
    let configured = if config.is_none() && paper == PaperSelection::DeviceDefault {
        None
    } else {
        let mut base = match config {
            Some(config) => {
                config.ensure_device(name)?;
                config.clone()
            }
            None => printer_configuration(name)?,
        };
        base.apply_paper(paper);
        Some(base)
    };
    // The buffer must outlive `CreateDC`; a pointer into a dropped
    // temporary is a dangling one.
    let words = configured
        .as_ref()
        .map(PrinterConfiguration::to_aligned_words);

    let wide = HSTRING::from(name);
    // SAFETY: `wide` outlives the call, as does `words` when present. A
    // null return is the documented failure signal, checked immediately
    // below.
    let hdc = unsafe {
        CreateDCW(
            None,
            &wide,
            None,
            words.as_ref().map(|w| w.as_ptr().cast::<DEVMODEW>()),
        )
    };
    if hdc.is_invalid() {
        return Err(PrintError::OpenDevice(name.to_owned()));
    }

    // SAFETY: `hdc` is a valid DC until `DeleteDC` below.
    let caps = unsafe {
        let dpi_x = GetDeviceCaps(Some(hdc), LOGPIXELSX);
        let dpi_y = GetDeviceCaps(Some(hdc), LOGPIXELSY);
        // Guard the divisors before any conversion. A driver reporting
        // zero DPI is malformed, and dividing by it would produce
        // infinities that reach the placement math and turn into a blank
        // page nobody can explain.
        if dpi_x <= 0 || dpi_y <= 0 {
            let _ = DeleteDC(hdc);
            return Err(PrintError::NoResolution(name.to_owned()));
        }
        let px_to_pt_x = |px: i32| f64::from(px) * 72.0 / f64::from(dpi_x);
        let px_to_pt_y = |px: i32| f64::from(px) * 72.0 / f64::from(dpi_y);
        let c = PrinterCaps {
            dpi_x: dpi_x.unsigned_abs(),
            dpi_y: dpi_y.unsigned_abs(),
            physical_pt: (
                px_to_pt_x(GetDeviceCaps(Some(hdc), PHYSICALWIDTH)),
                px_to_pt_y(GetDeviceCaps(Some(hdc), PHYSICALHEIGHT)),
            ),
            printable_pt: (
                px_to_pt_x(GetDeviceCaps(Some(hdc), HORZRES)),
                px_to_pt_y(GetDeviceCaps(Some(hdc), VERTRES)),
            ),
            offset_pt: (
                px_to_pt_x(GetDeviceCaps(Some(hdc), PHYSICALOFFSETX)),
                px_to_pt_y(GetDeviceCaps(Some(hdc), PHYSICALOFFSETY)),
            ),
        };
        let _ = DeleteDC(hdc);
        c
    };
    Ok(caps)
}

// ---------------------------------------------------------------------------
// Page placement — pure geometry, no platform dependency
// ---------------------------------------------------------------------------

/// How a page is sized onto the sheet.
///
/// The four modes Acrobat Reader offers, and they are genuinely four:
/// **Fit and ShrinkOversized are not the same operation**
/// (`Acrobat_Features/printing__scaling_modes.md`). Fit scales in both
/// directions — a small page is ENLARGED to fill the sheet.
/// ShrinkOversized only ever reduces. Treating them as one, which is the
/// natural simplification, silently blows a business card up to A4.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScaleMode {
    /// Scale to fill the printable area, up or down, preserving aspect.
    /// Reader's default.
    Fit,
    /// 1 PDF point = 1/72 inch on paper, whatever that costs.
    ActualSize,
    /// Like [`Self::ActualSize`], except a page too large for the sheet
    /// is reduced to fit. Never enlarges.
    ShrinkOversized,
    /// An explicit multiplier, where `1.0` is actual size. Reader accepts
    /// a free-form 1–1000%, not a set of presets.
    Custom(f64),
}

/// Where and how big a page lands on the sheet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Multiplier from PDF points to paper points.
    pub scale: f64,
    /// Offset within the printable area, in paper points, to centre the
    /// page.
    pub offset_x_pt: f64,
    /// Vertical offset, same units.
    pub offset_y_pt: f64,
    /// **The scaled page does not fit and will lose content off the
    /// edges.**
    ///
    /// Acrobat's documented behaviour here is to clip SILENTLY — a page
    /// wider than the paper simply loses its margins with no warning
    /// (`printing__scaling_modes.md`, recorded as a still-open Acrobat
    /// weakness). pdfcer reports it instead, which is the operator's
    /// standing ruling applied: parity is a floor, and losing content
    /// without saying so is not a behaviour worth matching.
    pub clipped: bool,
}

/// Compute where a page lands on a sheet.
///
/// All inputs and outputs are in **points** (1/72 inch), including the
/// paper measurements — the caller converts from device pixels using the
/// printer's own DPI, so this function never sees a device unit and
/// therefore cannot be wrong about one.
///
/// `printable` is the PRINTABLE area, not the physical sheet. Every
/// printer has an unprintable margin it cannot reach, and fitting to the
/// physical size instead produces a page whose edges are cropped by the
/// hardware — which looks exactly like a pdfcer bug and is not one.
#[must_use]
pub fn place_page(page: (f64, f64), printable: (f64, f64), mode: ScaleMode) -> Placement {
    let (pw, ph) = page;
    let (aw, ah) = printable;
    // A degenerate page or sheet has no meaningful placement. Returning
    // scale 1.0 rather than dividing by zero: the caller gets something
    // renderable, and `clipped` tells the truth about it.
    if pw <= 0.0 || ph <= 0.0 || aw <= 0.0 || ah <= 0.0 {
        return Placement {
            scale: 1.0,
            offset_x_pt: 0.0,
            offset_y_pt: 0.0,
            clipped: true,
        };
    }

    let fit = (aw / pw).min(ah / ph);
    let scale = match mode {
        ScaleMode::Fit => fit,
        ScaleMode::ActualSize => 1.0,
        // `min(1.0)` is the whole difference from Fit, and the reason
        // both modes exist.
        ScaleMode::ShrinkOversized => fit.min(1.0),
        // A non-finite or non-positive multiplier is a caller error that
        // must not become a non-finite scale downstream; fall back to
        // actual size rather than propagate a NaN into device
        // coordinates, where it would silently produce nothing on paper.
        ScaleMode::Custom(m) if m.is_finite() && m > 0.0 => m,
        ScaleMode::Custom(_) => 1.0,
    };

    let w = pw * scale;
    let h = ph * scale;
    // A hair of tolerance: floating-point `fit` can land a whisker over
    // the boundary and report a clip nobody could see on paper.
    const EPS: f64 = 0.5;
    Placement {
        scale,
        // Centred. Clamped at zero so an oversized page starts at the
        // edge of the printable area rather than at a negative offset,
        // which would push MORE of it off the sheet than necessary.
        offset_x_pt: ((aw - w) / 2.0).max(0.0),
        offset_y_pt: ((ah - h) / 2.0).max(0.0),
        clipped: w > aw + EPS || h > ah + EPS,
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
    use super::{
        Collate, DeviceGeometry, JobSpec, Orientation, PageSubset, Placement, ScaleMode,
        job_resolution, place_page, plan_job, resolve_orientation, sheet_orientation,
    };

    /// A4 in points.
    const A4: (f64, f64) = (595.0, 842.0);
    /// A Letter sheet's printable area with a typical 1/4-inch hardware
    /// margin all round.
    const LETTER_PRINTABLE: (f64, f64) = (612.0 - 36.0, 792.0 - 36.0);

    /// ★ A `/Rotate 90` page is planned LANDSCAPE, because that is what
    /// the renderer produces.
    ///
    /// The two halves of the program disagreed about this until
    /// 2026-08-18: `render-page` produced a 337x238 pixmap for an A4
    /// with `/Rotate 90` while `print-preview` reported
    /// `size_pt=595.0x842.0`. The placement was computed for the wrong
    /// aspect, `Auto` never turned the sheet, and the blit stretched a
    /// landscape image into a portrait rectangle.
    ///
    /// The assertions that matter are the NEGATIVE ones: 0 and 180 must
    /// NOT swap. A "rotate means transpose" implementation passes the
    /// 90 case and fails those, which is why they are here.
    #[test]
    fn a_rotated_page_is_planned_at_its_displayed_size() {
        assert_eq!(super::displayed_page_size(A4, 90), (842.0, 595.0));
        assert_eq!(super::displayed_page_size(A4, 270), (842.0, 595.0));
        assert_eq!(super::displayed_page_size(A4, 0), A4);
        assert_eq!(super::displayed_page_size(A4, 180), A4);
        // A rotated page's ORIENTATION follows, which is the whole point:
        // before this, `Auto` read the un-turned box and left the sheet
        // portrait for a page that displays landscape.
        assert_eq!(
            resolve_orientation(Orientation::Auto, super::displayed_page_size(A4, 90)),
            Orientation::Landscape
        );
        assert_eq!(
            resolve_orientation(Orientation::Auto, super::displayed_page_size(A4, 180)),
            Orientation::Portrait
        );
    }

    /// Angles a file may contain but a parser has not normalized.
    ///
    /// `Page::rotate` is normalized to {0, 90, 180, 270}, but this is a
    /// PUBLIC function and its caller may not be that parser. A negative
    /// angle is legal in the wild (`/Rotate -90`), and a value that is
    /// not a multiple of 90 cannot be honoured by an axis swap at all —
    /// so it is treated as no rotation rather than rounded to a guess.
    #[test]
    fn out_of_range_rotations_are_reduced_not_trusted() {
        assert_eq!(super::displayed_page_size(A4, 450), (842.0, 595.0));
        assert_eq!(super::displayed_page_size(A4, -90), (842.0, 595.0));
        assert_eq!(super::displayed_page_size(A4, -180), A4);
        assert_eq!(super::displayed_page_size(A4, 360), A4);
        assert_eq!(super::displayed_page_size(A4, 45), A4);
    }

    /// Fit ENLARGES a small page; ShrinkOversized refuses to.
    ///
    /// The single assertion that keeps the two modes distinct. Collapsing
    /// them is the natural simplification, and it silently blows a
    /// business card up to fill a Letter sheet.
    #[test]
    fn fit_enlarges_where_shrink_oversized_refuses_to() {
        let card = (252.0, 144.0);
        let fit = place_page(card, LETTER_PRINTABLE, ScaleMode::Fit);
        let shrink = place_page(card, LETTER_PRINTABLE, ScaleMode::ShrinkOversized);
        assert!(fit.scale > 1.0, "Fit must enlarge: {fit:?}");
        assert!(
            (shrink.scale - 1.0).abs() < f64::EPSILON,
            "ShrinkOversized must never enlarge: {shrink:?}"
        );
    }

    /// The two modes agree when the page is too big — which is the only
    /// case where Shrink has anything to do.
    #[test]
    fn the_two_modes_agree_on_an_oversized_page() {
        let big = (1190.0, 1684.0);
        let fit = place_page(big, LETTER_PRINTABLE, ScaleMode::Fit);
        let shrink = place_page(big, LETTER_PRINTABLE, ScaleMode::ShrinkOversized);
        assert!(fit.scale < 1.0);
        assert!((fit.scale - shrink.scale).abs() < 1e-12);
        assert!(!fit.clipped, "a fitted page must not clip: {fit:?}");
    }

    /// Actual size CLIPS an oversized page, and says so.
    ///
    /// Acrobat clips here silently. Reporting it is the deliberate
    /// divergence: an operator who is about to lose the right-hand column
    /// of a drawing should learn it before the paper comes out.
    #[test]
    fn actual_size_reports_the_clip_acrobat_stays_quiet_about() {
        let big = (1190.0, 1684.0);
        let p = place_page(big, LETTER_PRINTABLE, ScaleMode::ActualSize);
        assert!((p.scale - 1.0).abs() < f64::EPSILON);
        assert!(p.clipped, "content WILL be lost and must be reported");
        // Offsets clamp at zero: an oversized page starts at the printable
        // edge rather than at a negative offset, which would throw away
        // more of it than the paper requires.
        assert_eq!(p.offset_x_pt, 0.0);
        assert_eq!(p.offset_y_pt, 0.0);
    }

    /// A page that fits is centred in the printable area.
    #[test]
    fn a_fitting_page_is_centred() {
        let small = (288.0, 396.0);
        let p = place_page(small, LETTER_PRINTABLE, ScaleMode::ActualSize);
        assert!(!p.clipped);
        assert!((p.offset_x_pt - (LETTER_PRINTABLE.0 - 288.0) / 2.0).abs() < 1e-9);
        assert!((p.offset_y_pt - (LETTER_PRINTABLE.1 - 396.0) / 2.0).abs() < 1e-9);
    }

    /// A custom multiplier is honoured, and a nonsense one degrades to
    /// actual size rather than poisoning device coordinates with a NaN.
    #[test]
    fn a_custom_scale_is_honoured_and_a_nonsense_one_is_not_propagated() {
        let p = place_page(A4, LETTER_PRINTABLE, ScaleMode::Custom(0.5));
        assert!((p.scale - 0.5).abs() < f64::EPSILON);
        for bad in [f64::NAN, f64::INFINITY, 0.0, -2.0] {
            let q: Placement = place_page(A4, LETTER_PRINTABLE, ScaleMode::Custom(bad));
            assert!(q.scale.is_finite() && q.scale > 0.0, "bad={bad} gave {q:?}");
        }
    }

    /// A degenerate page or sheet yields something renderable rather than
    /// a division by zero — and admits it is not right.
    #[test]
    fn degenerate_input_does_not_produce_a_non_finite_scale() {
        for (page, sheet) in [
            ((0.0, 100.0), LETTER_PRINTABLE),
            ((100.0, 0.0), LETTER_PRINTABLE),
            (A4, (0.0, 100.0)),
            (A4, (100.0, -5.0)),
        ] {
            let p = place_page(page, sheet, ScaleMode::Fit);
            assert!(p.scale.is_finite() && p.scale > 0.0, "{p:?}");
            assert!(p.clipped, "a degenerate placement must not claim to fit");
        }
    }

    // ---- job planning ----

    /// Capabilities standing in for a 600-DPI Letter printer with a
    /// quarter-inch unprintable margin all round.
    fn letter_600() -> DeviceGeometry {
        DeviceGeometry {
            dpi: (600, 600),
            printable_pt: (576.0, 756.0),
            physical_pt: (612.0, 792.0),
            offset_pt: (18.0, 18.0),
        }
    }

    fn spec(pages: Vec<usize>, mode: ScaleMode, max_dpi: u32) -> JobSpec {
        JobSpec {
            pages,
            mode,
            max_dpi,
            subset: PageSubset::All,
            reverse: false,
            copies: 1,
            collate: Collate::Collated,
        }
    }

    /// **The render scale already carries the print scale.**
    ///
    /// This is the property that keeps a printed line as sharp as the
    /// same line on screen: the pixels handed to GDI are the size they
    /// will occupy on paper, so the blit is a copy rather than a
    /// resample. If this ever becomes plain `dpi / 72`, output softens
    /// everywhere and nothing else fails.
    #[test]
    fn the_render_scale_folds_in_the_placement_scale() {
        let caps = letter_600();
        // A Letter page shrunk to the printable area: 576/612 ≈ 0.941.
        let plans = plan_job(
            &caps,
            &[(612.0, 792.0)],
            &spec(vec![0], ScaleMode::Fit, 600),
        );
        let p = plans.first().expect("one page planned");
        assert!(p.placement.scale < 1.0, "Fit shrinks a full-bleed page");
        let expected = (600.0 / 72.0) * p.placement.scale;
        assert!(
            (p.render_scale - expected).abs() < 1e-9,
            "render_scale must be dpi/72 × placement.scale, not dpi/72"
        );
    }

    /// The cap binds, is reported, and changes the render scale with it.
    #[test]
    fn the_dpi_cap_binds_and_is_disclosed() {
        let caps = letter_600();
        let res = job_resolution(&caps, &spec(vec![0], ScaleMode::ActualSize, 300));
        assert_eq!(res.dpi, 300);
        assert_eq!(res.device_dpi, 600);
        assert!(res.capped, "300 < 600, so the operator is told");

        let uncapped = job_resolution(&caps, &spec(vec![0], ScaleMode::ActualSize, 1200));
        assert_eq!(uncapped.dpi, 600, "the cap never RAISES beyond the device");
        assert!(!uncapped.capped);
    }

    /// ★ **An asymmetric device renders at its SMALLER axis.**
    ///
    /// 600×300 is real on plotters. Rendering at 600 for a device that
    /// can only place 300 dots vertically makes the driver resample —
    /// which undoes the entire reason for rendering at device
    /// resolution, silently, and on exactly the machines whose output
    /// people care most about.
    #[test]
    fn an_asymmetric_device_renders_at_its_smaller_axis() {
        let caps = DeviceGeometry {
            dpi: (600, 300),
            ..letter_600()
        };
        assert_eq!(
            job_resolution(&caps, &spec(vec![0], ScaleMode::ActualSize, 2400)).dpi,
            300
        );
    }

    /// **A stale page index is skipped, not fatal.**
    ///
    /// A page range is operator input and can name a page a since-edited
    /// document no longer has. Refusing the whole job because one index
    /// is stale is worse than printing what exists and reporting the
    /// count — the operator wanted paper, and nine of ten pages is
    /// recoverable where zero is not.
    #[test]
    fn an_out_of_range_page_is_skipped_rather_than_failing_the_job() {
        let caps = letter_600();
        let sizes = [(612.0, 792.0), (612.0, 792.0)];
        let plans = plan_job(&caps, &sizes, &spec(vec![0, 7, 1], ScaleMode::Fit, 300));
        assert_eq!(plans.len(), 2, "two real pages survive");
        assert_eq!(plans[0].index, 0);
        assert_eq!(plans[1].index, 1, "and the order given is preserved");
    }

    /// The page ORDER in the spec is the print order, including
    /// duplicates and reversals — the shells build ranges, and reverse
    /// order is an option Acrobat offers.
    #[test]
    fn the_planned_order_is_the_requested_order() {
        let caps = letter_600();
        let sizes = [(612.0, 792.0); 3];
        let plans = plan_job(&caps, &sizes, &spec(vec![2, 0, 2], ScaleMode::Fit, 300));
        assert_eq!(
            plans.iter().map(|p| p.index).collect::<Vec<_>>(),
            vec![2, 0, 2]
        );
    }

    /// Mixed page sizes each get their own placement — a document with a
    /// landscape drawing among portrait pages must not scale them all to
    /// the first page's factor.
    #[test]
    fn each_page_is_placed_on_its_own_size() {
        let caps = letter_600();
        let sizes = [(612.0, 792.0), (792.0, 612.0)];
        let plans = plan_job(&caps, &sizes, &spec(vec![0, 1], ScaleMode::Fit, 300));
        assert!(
            (plans[0].placement.scale - plans[1].placement.scale).abs() > 1e-6,
            "a portrait and a landscape page cannot share a fit scale"
        );
    }

    // ---- page sequencing: subset, reverse, copies, collate ----

    fn seq(
        pages: Vec<usize>,
        subset: PageSubset,
        reverse: bool,
        copies: u16,
        collate: Collate,
    ) -> Vec<usize> {
        JobSpec {
            pages,
            mode: ScaleMode::Fit,
            max_dpi: 300,
            subset,
            reverse,
            copies,
            collate,
        }
        .sequence()
    }

    /// ★ **Odd/even is by DOCUMENT page number, not position in the
    /// range.**
    ///
    /// "Pages 2-9, odd" means the pages numbered 3, 5, 7, 9 — what is
    /// printed on the paper — not the first, third and fifth entries of
    /// the range, which would be 2, 4, 6.
    ///
    /// Both readings produce a plausible page count, and one produces
    /// entirely the wrong sheets. That is why this has a test rather
    /// than a comment.
    #[test]
    fn odd_and_even_are_by_document_page_number() {
        // Zero-based 1..=8 is document pages 2..=9.
        let range: Vec<usize> = (1..=8).collect();
        assert_eq!(
            seq(range.clone(), PageSubset::Odd, false, 1, Collate::Collated),
            vec![2, 4, 6, 8],
            "document pages 3,5,7,9"
        );
        assert_eq!(
            seq(range, PageSubset::Even, false, 1, Collate::Collated),
            vec![1, 3, 5, 7],
            "document pages 2,4,6,8"
        );
    }

    /// ★ **Subset is applied BEFORE reverse.**
    ///
    /// "Even pages, reversed" is the even pages in reverse order.
    /// Reversing first and then taking every other entry yields a
    /// different SET — on an even-length range it yields the odd pages.
    #[test]
    fn the_subset_is_taken_before_the_reverse() {
        let range: Vec<usize> = (0..4).collect(); // document pages 1..=4
        assert_eq!(
            seq(range, PageSubset::Even, true, 1, Collate::Collated),
            vec![3, 1],
            "document pages 4 then 2 — not pages 3 and 1"
        );
    }

    /// Collated repeats the whole sequence; uncollated repeats each page.
    #[test]
    fn collation_decides_where_the_copies_go() {
        let range = vec![0, 1, 2];
        assert_eq!(
            seq(range.clone(), PageSubset::All, false, 2, Collate::Collated),
            vec![0, 1, 2, 0, 1, 2]
        );
        assert_eq!(
            seq(range, PageSubset::All, false, 2, Collate::Uncollated),
            vec![0, 0, 1, 1, 2, 2]
        );
    }

    /// **Copies multiply the FINISHED sequence.**
    ///
    /// If copies were applied before the subset, the filter would run
    /// over duplicated pages and collation would have nothing left to
    /// mean. Pinned with all three options at once, because the order of
    /// operations is the only place a defect can hide in code this
    /// short.
    #[test]
    fn copies_apply_to_the_sequence_after_subset_and_reverse() {
        let range: Vec<usize> = (0..4).collect();
        assert_eq!(
            seq(range, PageSubset::Odd, true, 2, Collate::Collated),
            vec![2, 0, 2, 0],
            "odd document pages 1,3 -> reversed 3,1 -> twice"
        );
    }

    /// Zero copies prints once. A job of nothing is never what was
    /// meant, and erroring would be a dialog fault for a value no UI
    /// should have produced.
    #[test]
    fn zero_copies_is_treated_as_one() {
        assert_eq!(
            seq(vec![0, 1], PageSubset::All, false, 0, Collate::Collated),
            vec![0, 1]
        );
    }

    // ---- orientation and the sheet it turns --------------------------
    //
    // The device in these tests is `letter_600()`: a portrait-default
    // Letter printer reporting a 576 × 756 pt printable area inside a
    // 612 × 792 pt sheet. A landscape LETTER page is 792 × 612 pt.

    /// A landscape Letter page, as a document reports it.
    const LANDSCAPE_LETTER: (f64, f64) = (792.0, 612.0);
    /// A portrait Letter page.
    const PORTRAIT_LETTER: (f64, f64) = (612.0, 792.0);
    /// The scale a landscape page gets when the sheet is CORRECTLY
    /// turned: 576 / 612 — the turned sheet's SHORT side over the page's
    /// short side, which is the axis that binds once the long axis has
    /// room. (Not 756/792; the long axis stops being the constraint, which
    /// is the whole reason turning helps.)
    const SCALE_TURNED: f64 = 0.941_176_470_588_235_3;
    /// The scale it got before the sheet was turned: 576 / 792. About 77%
    /// of correct size, centred, with no clip to report it — which is why
    /// this read as a scaling preference rather than a defect.
    const SCALE_UNTURNED: f64 = 0.727_272_727_272_727_3;

    /// ★ **The regression: a landscape page is planned against the
    /// TURNED sheet.**
    ///
    /// `Auto` on a landscape page resolves to landscape, the driver turns
    /// the sheet, and planning must turn with it. Before the fix this
    /// planned 0.7273 against the un-turned 576 × 756 printable area
    /// while the driver printed on 756 × 576 — the page came out at
    /// 0.7273 / 0.9412 ≈ 77% of correct size with a wide empty margin.
    ///
    /// The exact scale is asserted, not merely "it changed": a fix that
    /// rotated by the wrong amount, or rotated the physical sheet and not
    /// the printable area, would also change it.
    #[test]
    fn a_landscape_page_is_planned_against_the_turned_sheet() {
        let device = letter_600().for_orientation(Orientation::Auto, LANDSCAPE_LETTER);
        let plans = plan_job(
            &device,
            &[LANDSCAPE_LETTER],
            &spec(vec![0], ScaleMode::Fit, 600),
        );
        let p = plans.first().expect("one page planned");
        // The SCALE is asserted before the geometry, deliberately: it is
        // the operator-visible symptom, so an ablation of the rotation
        // reports the number that was on paper (0.7273) rather than a
        // pair of sheet dimensions that has to be re-derived into one.
        assert!(
            (p.placement.scale - SCALE_TURNED).abs() < 1e-12,
            "expected {SCALE_TURNED} (576/612), got {}; \
             {SCALE_UNTURNED} means the sheet was not turned",
            p.placement.scale
        );
        assert_eq!(device.printable_pt, (756.0, 576.0), "the sheet turns");
    }

    /// **Portrait FORCED on a landscape page does not turn the sheet.**
    ///
    /// Without this, "always turn for a landscape page" would pass the
    /// test above and be wrong: the operator who picks Portrait is asking
    /// for a landscape drawing on an upright sheet, and gets the
    /// under-scaled placement on purpose.
    #[test]
    fn forcing_portrait_on_a_landscape_page_leaves_the_sheet_upright() {
        let device = letter_600().for_orientation(Orientation::Portrait, LANDSCAPE_LETTER);
        assert_eq!(device, letter_600(), "nothing about the sheet may move");
        let plans = plan_job(
            &device,
            &[LANDSCAPE_LETTER],
            &spec(vec![0], ScaleMode::Fit, 600),
        );
        let p = plans.first().expect("one page planned");
        assert!(
            (p.placement.scale - SCALE_UNTURNED).abs() < 1e-12,
            "expected {SCALE_UNTURNED} (576/792), got {}",
            p.placement.scale
        );
    }

    /// **The identity case: a portrait page on a portrait device is
    /// untouched.**
    ///
    /// Without this a helper that swapped unconditionally would pass
    /// every other test here — the two above only pin the cases where a
    /// turn is or is not wanted, not the case where the answer is "do
    /// nothing".
    #[test]
    fn a_portrait_page_on_a_portrait_device_changes_nothing() {
        for requested in [Orientation::Auto, Orientation::Portrait] {
            assert_eq!(
                letter_600().for_orientation(requested, PORTRAIT_LETTER),
                letter_600(),
                "{requested:?} on an upright sheet must be the identity"
            );
        }
    }

    /// ★ **Rotation is relative to the DEVICE's default, not to
    /// portrait.**
    ///
    /// Landscape-default devices are real — wide-format plotters and
    /// label printers ship that way, and any driver's properties page can
    /// set it. A helper that turned "whenever the job is landscape" would
    /// turn a sheet that was already turned, producing a PORTRAIT
    /// printable area for a landscape job: the original bug, with the
    /// sign flipped, on exactly the hardware this project's operator uses.
    #[test]
    fn a_landscape_default_device_does_not_turn_for_a_landscape_job() {
        let plotter = DeviceGeometry {
            dpi: (600, 600),
            printable_pt: (756.0, 576.0),
            physical_pt: (792.0, 612.0),
            offset_pt: (18.0, 18.0),
        };
        assert_eq!(plotter.default_orientation(), Orientation::Landscape);
        assert_eq!(
            plotter.for_orientation(Orientation::Landscape, LANDSCAPE_LETTER),
            plotter,
            "already landscape — nothing to turn"
        );
        assert_eq!(
            plotter.for_orientation(Orientation::Auto, LANDSCAPE_LETTER),
            plotter,
            "and Auto resolves to the same thing"
        );
        // The other direction still turns, and turns the other way.
        let upright = plotter.for_orientation(Orientation::Portrait, LANDSCAPE_LETTER);
        assert_eq!(upright.printable_pt, (576.0, 756.0));
        assert_eq!(upright.physical_pt, (612.0, 792.0));
    }

    /// ★ **An ASYMMETRIC margin turns with the sheet, and not by a plain
    /// swap.**
    ///
    /// The margins belong to the paper path, so they stay on the same
    /// physical edges while the coordinate system turns around them:
    /// left→top, top→right, right→bottom, bottom→left. For a 612 × 792
    /// sheet with a 576 × 700 printable area at offset (12, 36) the
    /// bottom margin is 792 − 36 − 700 = 56, so the turned offset is
    /// (56, 12).
    ///
    /// Asserted on an asymmetric offset because every wrong version of
    /// this — a plain component swap `(36, 12)`, or leaving the offset
    /// alone — is INVISIBLE on the symmetric margins every office printer
    /// reports. A test written on a square margin cannot fail.
    #[test]
    fn an_asymmetric_margin_turns_with_the_sheet() {
        let device = DeviceGeometry {
            dpi: (600, 600),
            printable_pt: (576.0, 700.0),
            physical_pt: (612.0, 792.0),
            offset_pt: (12.0, 36.0),
        };
        let turned = device.for_orientation(Orientation::Landscape, PORTRAIT_LETTER);
        assert_eq!(turned.physical_pt, (792.0, 612.0));
        assert_eq!(turned.printable_pt, (700.0, 576.0));
        assert_eq!(
            turned.offset_pt,
            (56.0, 12.0),
            "bottom margin becomes left, left margin becomes top — \
             a plain swap would give (36, 12)"
        );
        // The margin ring is intact: what was the top margin (36) is now
        // the right margin.
        let right = turned.physical_pt.0 - turned.offset_pt.0 - turned.printable_pt.0;
        assert!((right - 36.0).abs() < 1e-12, "top margin must become right");
        // And turning back is the identity, which is what makes this one
        // rotation rather than two independently-guessed ones.
        assert_eq!(
            turned.for_orientation(Orientation::Portrait, PORTRAIT_LETTER),
            device,
            "turning out and back must return the sheet it started on"
        );
    }

    /// The DPI does NOT turn with the sheet.
    ///
    /// `LOGPIXELSX`/`LOGPIXELSY` describe the engine's dot pitch, not the
    /// page. Swapping them on an asymmetric device (600 × 300 is real on
    /// plotters) would mis-size every rasterisation, and the only symptom
    /// would be a stretched print.
    #[test]
    fn the_resolution_does_not_turn_with_the_sheet() {
        let device = DeviceGeometry {
            dpi: (600, 300),
            ..letter_600()
        };
        assert_eq!(
            device
                .for_orientation(Orientation::Landscape, LANDSCAPE_LETTER)
                .dpi,
            (600, 300)
        );
    }

    /// A square sheet reads as portrait, matching `resolve_orientation`'s
    /// reading of a square page — so a square page on a square sheet asks
    /// for no turn.
    #[test]
    fn a_square_sheet_reads_as_portrait() {
        assert_eq!(sheet_orientation((612.0, 612.0)), Orientation::Portrait);
        assert_eq!(sheet_orientation((792.0, 612.0)), Orientation::Landscape);
        assert_eq!(sheet_orientation((612.0, 792.0)), Orientation::Portrait);
    }

    /// ★ **The orientation page is the first page SENT, not `pages[0]`.**
    ///
    /// A reversed job sends its last page first. If the geometry rotation
    /// resolved `Auto` from `pages[0]` while the `DEVMODE` resolved it
    /// from the first page sent, the two would disagree on exactly the
    /// jobs that mix page shapes — the driver turning a sheet pdfcer
    /// planned flat, which is this whole defect by a different route.
    #[test]
    fn the_orientation_page_is_the_first_page_sent() {
        let sizes = [PORTRAIT_LETTER, LANDSCAPE_LETTER];
        let mut s = spec(vec![0, 1], ScaleMode::Fit, 600);
        assert_eq!(s.first_page_pt(&sizes), PORTRAIT_LETTER);
        s.reverse = true;
        assert_eq!(
            s.first_page_pt(&sizes),
            LANDSCAPE_LETTER,
            "reversed, the landscape page is sent first and decides the sheet"
        );
        // An empty job falls back rather than forcing an Option on
        // callers for a case that cannot reach paper.
        assert_eq!(
            spec(vec![], ScaleMode::Fit, 600).first_page_pt(&sizes),
            super::US_LETTER_PORTRAIT_PT
        );
    }
}

// ---------------------------------------------------------------------------
// Job planning — the arithmetic both shells share
// ---------------------------------------------------------------------------

/// What to print, in the caller's terms.
///
/// # Why planning is separate from rendering
///
/// Both shells need the same answer to "at what scale, and where on the
/// sheet, does page N land, and what resolution should it be rendered
/// at" — and that arithmetic is the part that drifts when it is written
/// twice. The symptom of drift here is a GUI print landing differently
/// from a CLI print of the same document at the same settings, which
/// nobody would think to compare.
///
/// So the arithmetic lives here and the RENDERING stays in the shells.
/// That keeps this crate free of `pdfcer-render` — see the crate docs on
/// why a printing crate that also rendered would need the whole render
/// stack to be testable, when the failures worth testing here (a wrong
/// `DEVMODE`, an upside-down DIB, a job left open) have nothing to do
/// with PDF.
#[derive(Debug, Clone, PartialEq)]
pub struct JobSpec {
    /// Zero-based page indices, in the order they should print.
    pub pages: Vec<usize>,
    /// How each page is sized onto the sheet.
    pub mode: ScaleMode,
    /// Upper bound on rendering resolution, in DPI.
    ///
    /// A MEMORY bound, not a quality preference: an A4 page at 600 DPI is
    /// 4960×7016 px, about 139 MB at RGBA for one page. Whoever sets it
    /// is choosing a number the operator did not, so both shells disclose
    /// it when it binds (rule 4).
    pub max_dpi: u32,
    /// Odd/even filtering, applied over [`Self::pages`].
    pub subset: PageSubset,
    /// Print the sequence back to front.
    pub reverse: bool,
    /// How many copies. Zero is treated as one — a job of nothing is
    /// never what an operator meant, and refusing it would be a dialog
    /// error for a value no UI should have allowed.
    pub copies: u16,
    /// Copy ordering.
    pub collate: Collate,
}

/// Which of the selected pages actually print (Acrobat's odd/even
/// subset filter).
///
/// Applied AFTER the range, and composing with it rather than replacing
/// it — "pages 1-10, even only" is a thing an operator asks for, and a
/// design where the subset replaced the range would make that
/// unexpressible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageSubset {
    /// Every page in the range.
    #[default]
    All,
    /// Odd pages by their 1-based DOCUMENT number, not their position in
    /// the range.
    ///
    /// This distinction is the whole reason the field is documented: an
    /// operator printing "2-9, odd" means document pages 3, 5, 7, 9 —
    /// the numbers printed on the paper — not the first, third and fifth
    /// entries of the range. Getting it wrong produces a plausible page
    /// count and the wrong sheets, which is the hardest kind of wrong to
    /// notice.
    Odd,
    /// Even pages, by document number, same reasoning.
    Even,
}

/// How multiple copies are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Collate {
    /// 1,2,3, 1,2,3 — whole documents, in order.
    #[default]
    Collated,
    /// 1,1, 2,2, 3,3 — all copies of each page together.
    ///
    /// Faster on most hardware because the page is rasterised once per
    /// position rather than once per copy, and the order a stapler
    /// wants.
    Uncollated,
}

impl JobSpec {
    /// Expand `pages` into the actual print sequence: subset filtered,
    /// reversed if asked, then multiplied by copies in the chosen order.
    ///
    /// # Order of operations, and why it is this one
    ///
    /// Subset, then reverse, then copies. Each step is defined on the
    /// result of the previous, and the order is not arbitrary:
    ///
    /// - **Subset before reverse**, because "even pages, reversed" means
    ///   the even pages in reverse order. Reversing first and then taking
    ///   every other entry would yield a different set entirely — odd
    ///   pages, on an even-length range.
    /// - **Copies last**, because a copy is a copy of the finished
    ///   sequence. Multiplying first would let the subset filter run over
    ///   duplicated pages, and `Collate` would have nothing left to mean.
    ///
    /// Written down because all three steps are one-liners and the ORDER
    /// is the only place a defect can hide.
    #[must_use]
    pub fn sequence(&self) -> Vec<usize> {
        let mut seq: Vec<usize> = self
            .pages
            .iter()
            .copied()
            .filter(|&i| match self.subset {
                PageSubset::All => true,
                // `i` is zero-based; the operator's page number is `i+1`.
                PageSubset::Odd => (i + 1) % 2 == 1,
                PageSubset::Even => (i + 1) % 2 == 0,
            })
            .collect();
        if self.reverse {
            seq.reverse();
        }
        let copies = self.copies.max(1);
        match self.collate {
            Collate::Collated => seq
                .iter()
                .copied()
                .cycle()
                .take(seq.len() * copies as usize)
                .collect(),
            Collate::Uncollated => seq
                .iter()
                .flat_map(|&i| std::iter::repeat_n(i, copies as usize))
                .collect(),
        }
    }

    /// The page whose shape decides the whole job's orientation.
    ///
    /// # Why this is a method and not "just index page 0"
    ///
    /// A `DEVMODE` applies to the entire job, so exactly one page can
    /// decide [`Orientation::Auto`], and `build_devmode` has already
    /// chosen which: the first page SENT. That is not `pages[0]` — the
    /// sequence may be subset-filtered, reversed, or repeated for
    /// copies, so the first page sent is `sequence()[0]`.
    ///
    /// Both shells call this rather than reaching for an index of their
    /// own, because the geometry rotation ([`DeviceGeometry::for_orientation`])
    /// and the `DEVMODE` must resolve `Auto` from the SAME page. If they
    /// disagree — a reversed job of a portrait first page and a landscape
    /// last one is enough — the driver turns a sheet pdfcer planned flat,
    /// which is the defect the rotation was added to fix, reintroduced by
    /// a different route.
    ///
    /// `page_sizes` is in DOCUMENT order, indexed by the values in
    /// [`Self::pages`].
    ///
    /// Falls back to US Letter portrait for a job with no pages, matching
    /// `build_devmode`'s own fallback. Such a job spools nothing, so the
    /// value is never printed against; it exists so neither caller has to
    /// carry an `Option` for a case that cannot reach paper.
    ///
    /// ```
    /// use pdfcer_print::{Collate, JobSpec, PageSubset, ScaleMode};
    ///
    /// let sizes = [(612.0, 792.0), (792.0, 612.0)];
    /// let mut spec = JobSpec {
    ///     pages: vec![0, 1],
    ///     mode: ScaleMode::Fit,
    ///     max_dpi: 300,
    ///     subset: PageSubset::All,
    ///     reverse: false,
    ///     copies: 1,
    ///     collate: Collate::Collated,
    /// };
    /// assert_eq!(spec.first_page_pt(&sizes), (612.0, 792.0));
    ///
    /// // Reversed, the LAST page is sent first and decides the sheet.
    /// spec.reverse = true;
    /// assert_eq!(spec.first_page_pt(&sizes), (792.0, 612.0));
    /// ```
    #[must_use]
    pub fn first_page_pt(&self, page_sizes: &[(f64, f64)]) -> (f64, f64) {
        self.sequence()
            .first()
            .and_then(|&i| page_sizes.get(i).copied())
            .unwrap_or(US_LETTER_PORTRAIT_PT)
    }
}

/// The page size assumed when a job names none.
///
/// Shared by [`JobSpec::first_page_pt`] and `build_devmode` so the two
/// cannot fall back differently — a divergence that would only ever
/// appear on an empty job, i.e. never in a way anyone could observe, and
/// would then be inherited by whatever asked next.
pub const US_LETTER_PORTRAIT_PT: (f64, f64) = (612.0, 792.0);

/// Where one page lands, and how big to render it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PagePlan {
    /// The page this describes, as given in [`JobSpec::pages`].
    pub index: usize,
    /// Placement on the sheet.
    pub placement: Placement,
    /// The scale to rasterise at, in device pixels per PDF point.
    ///
    /// # It already carries the print scale, deliberately
    ///
    /// This is `dpi / 72 × placement.scale`, so the pixels handed to the
    /// spooler are already the size they will occupy on paper and the
    /// blit is a 1:1 copy.
    ///
    /// The alternative — render at device resolution and let
    /// `StretchDIBits` scale — resamples twice, once in the renderer's
    /// own transform and once in GDI's, and the visible result is a
    /// printed line softer than the same line on screen. On a CAD
    /// drawing, whose value is thin lines, that is the difference the
    /// operator would notice first.
    pub render_scale: f64,
}

/// The resolution a job will render at, and whether the cap bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobResolution {
    /// The DPI actually used.
    pub dpi: u32,
    /// The device's own resolution, before the cap.
    pub device_dpi: u32,
    /// Whether [`JobSpec::max_dpi`] reduced it — the case that must be
    /// disclosed, because pdfcer chose a number the operator did not.
    pub capped: bool,
}

impl JobResolution {
    /// Rough memory cost of ONE page at the DEVICE's resolution, in
    /// megabytes, for a US-Letter sheet at RGBA.
    ///
    /// Approximate on purpose, and the figure a disclosure quotes: an
    /// operator deciding whether to raise the cap needs an order of
    /// magnitude, not a precise number for a page size they may not be
    /// printing.
    #[must_use]
    pub const fn uncapped_page_mb(self) -> u64 {
        (self.device_dpi as u64 * self.device_dpi as u64 * 8 * 11 * 4) / 1_000_000
    }
}

/// The device geometry planning needs, with no platform type in it.
///
/// # Why not just take `PrinterCaps`
///
/// `PrinterCaps` is `cfg(windows)` — it is what a Win32 driver reported.
/// The planning arithmetic is pure geometry, and this module's own note
/// says that half stays un-gated so it compiles and TESTS on the Linux
/// and macOS CI jobs.
///
/// Taking `PrinterCaps` here would have quietly moved the most
/// test-worthy code in the crate behind a `cfg` that CI does not build —
/// the tests would still pass on Windows and simply stop existing
/// elsewhere, which is the kind of coverage loss nothing reports.
///
/// # ★ It carries the WHOLE sheet, not only the printable area
///
/// `physical_pt` and `offset_pt` are here even though `plan_job` reads
/// neither, and that is deliberate. They are what a preview needs to
/// draw the sheet and the margin inside it, and — more importantly —
/// they are what [`Self::for_orientation`] needs in order to turn the
/// sheet correctly. A type holding only `printable_pt` cannot rotate
/// itself: the unprintable margins are not recoverable from the
/// printable size alone, so the rotation would have to be written a
/// second time by whoever holds the rest, which is exactly the drift
/// this type exists to prevent.
///
/// # ★ Orientation is not optional here, and that is the whole point
///
/// This used to be reachable through an infallible
/// `From<&PrinterCaps>`, which copied `printable_pt` verbatim. That
/// conversion was a trap: [`printer_caps`] reads the device's DEFAULT
/// `DEVMODE`, so on a portrait-default printer it reports a PORTRAIT
/// printable area — and a job whose `DEVMODE` sets landscape prints on a
/// sheet that has been turned. Planning against the un-turned area
/// under-scales every page (a Letter page on a Letter sheet planned at
/// 0.727 instead of 0.941, about 77% of correct size, with a wide empty
/// margin and no clip to report it).
///
/// The `From` impl is gone rather than documented, because a wrong
/// answer that is one `.into()` away will be reached again. The only
/// route from a `PrinterCaps` to a `DeviceGeometry` is
/// [`Self::from_caps`], which cannot be called without stating the
/// job's orientation and its first page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviceGeometry {
    /// Resolution in dots per inch, horizontal and vertical.
    pub dpi: (u32, u32),
    /// The printable area in points — smaller than the sheet by the
    /// unprintable margins the driver reports.
    pub printable_pt: (f64, f64),
    /// The full sheet in points.
    pub physical_pt: (f64, f64),
    /// Where the printable area begins relative to the sheet corner, in
    /// points — the top-left unprintable margin.
    pub offset_pt: (f64, f64),
}

/// Resolve the rendering resolution for a job.
#[must_use]
pub fn job_resolution(device: &DeviceGeometry, spec: &JobSpec) -> JobResolution {
    // The SMALLER axis, not an average: a device with asymmetric
    // resolution (600×300 is real on some plotters) must not be rendered
    // at a resolution one axis cannot reproduce, because the driver then
    // resamples and undoes the point of rendering at device resolution.
    let smaller = device.dpi.0.min(device.dpi.1);
    let dpi = smaller.min(spec.max_dpi);
    JobResolution {
        dpi,
        device_dpi: smaller,
        capped: dpi < smaller,
    }
}

/// Plan every page of a job.
///
/// `page_sizes` is indexed by the document's page order, in PDF points.
/// Indices in [`JobSpec::pages`] that fall outside it are SKIPPED rather
/// than erroring: a page range is operator input, and a job that refuses
/// wholesale because one index is stale is worse than one that prints
/// what it can and reports the count.
#[must_use]
pub fn plan_job(
    device: &DeviceGeometry,
    page_sizes: &[(f64, f64)],
    spec: &JobSpec,
) -> Vec<PagePlan> {
    let resolution = job_resolution(device, spec);
    spec.sequence()
        .into_iter()
        .filter_map(|index| {
            let size = *page_sizes.get(index)?;
            let placement = place_page(size, device.printable_pt, spec.mode);
            Some(PagePlan {
                index,
                placement,
                render_scale: (f64::from(resolution.dpi) / 72.0) * placement.scale,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Driver settings (DEVMODE)
// ---------------------------------------------------------------------------

/// Which way up the sheet is fed.
///
/// # `Auto` is per-page — and for a while this said so and was not
///
/// Acrobat's default computes orientation **for each page** within one
/// job: a document mixing portrait text with a landscape drawing gets
/// both, from one command. That is the behaviour worth matching, and
/// **it is what [`spool_sheets`] does** — one `DEVMODE` per contiguous
/// run of same-orientation sheets, applied with `ResetDC` between pages.
///
/// ★ Until 2026-08-18 this heading made that claim and the code did not
/// honour it. [`resolve_orientation`] is per-page-capable and was called
/// twice, both times for the whole job, because a `DEVMODE` handed to
/// `CreateDC` applies until something changes it and nothing did. A CAD
/// export — an A4 portrait title sheet followed by A3 landscape
/// drawings — printed every sheet in whichever orientation page 1
/// resolved to. It was reported from outside, by the `pdfcer-gui` shell,
/// which noticed the divergence by reading this comment and then the
/// call sites.
///
/// Which entry point a caller uses decides which behaviour it gets, and
/// that is stated rather than left to be discovered:
///
/// - [`spool_sheets`] — per-sheet. The caller supplies each sheet's
///   resolved orientation, having planned its placement against that
///   sheet's own geometry.
/// - [`spool`] and [`spool_with_config`] — per-job, resolved from the
///   one page the caller nominates. Unchanged, because a caller that
///   imposes several source pages onto one sheet has exactly one
///   orientation by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    /// Choose per page from its own aspect ratio.
    #[default]
    Auto,
    /// Force portrait.
    Portrait,
    /// Force landscape.
    Landscape,
}

/// Two-sided printing.
///
/// # Driver-gated, never simulated
///
/// Acrobat does not software-simulate duplex, and neither does pdfcer. A
/// printer that cannot do it will not be made to by reordering pages and
/// asking the operator to reinsert the stack: that is a workflow with a
/// documented mis-assembly failure mode, and offering it as though it
/// were duplex would be claiming a capability the hardware does not
/// have.
///
/// [`Duplex::Simplex`] is not "the default" so much as "what a device
/// that cannot duplex does". `supports_duplex` on the capabilities is
/// what a shell must consult before offering the control at all (R83).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Duplex {
    /// One side only.
    #[default]
    Simplex,
    /// Two-sided, flipped on the long edge — the usual "book" binding.
    LongEdge,
    /// Two-sided, flipped on the short edge — "notepad" binding.
    ShortEdge,
}

/// Driver-level settings applied to the device before the job starts.
///
/// # Why these are separate from [`JobSpec`]
///
/// Everything in `JobSpec` is arithmetic pdfcer performs itself — which
/// pages, at what scale, in what order. Everything here is a request to
/// the DRIVER, which may refuse it. The two fail differently: a scale
/// pdfcer computes is exact, and a duplex setting a device declines is a
/// job that silently comes out single-sided.
///
/// Keeping them apart means the shells can report the second kind
/// honestly rather than presenting both as though pdfcer controlled them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeviceSettings {
    /// Sheet orientation.
    pub orientation: Orientation,
    /// Two-sided printing, if the device supports it.
    pub duplex: Duplex,
    /// Ask the driver to pick the input tray from each page's size
    /// rather than using its default tray.
    ///
    /// The operator-facing companion to a document's own
    /// `/PickTrayByPDFSize` viewer preference: this is the per-job
    /// override of the same idea.
    ///
    /// Reaches the driver as `dmDefaultSource = DMBIN_FORMSOURCE`,
    /// which is answered by the driver's own Form-to-Tray Assignment
    /// table. A device that does not list that bin cannot honour it —
    /// [`DeviceFeatures::supports_form_source_bin`] is what a shell
    /// consults before offering the control (R83), because a job that
    /// silently came out of the default tray looks identical to one
    /// where the request was never made.
    pub pick_tray_by_page_size: bool,
    /// Which sheet to feed.
    ///
    /// [`PaperSelection::DeviceDefault`] says nothing, and the device
    /// prints on whatever its own Windows preferences are set to — which
    /// was pdfcer's ONLY behaviour until 2026-08-18, and left an operator
    /// no route to a different sheet but to leave pdfcer, open Devices
    /// and Printers, change the default, and come back.
    ///
    /// [`crate::printer_forms`] enumerates what a device actually
    /// offers; nothing here validates a form id against that list,
    /// deliberately — the driver is the authority on its own forms and a
    /// second opinion in this crate would be one more thing to drift.
    pub paper: PaperSelection,
}

/// The size a page is DISPLAYED at, from its box and its `/Rotate`.
///
/// # ★ Why the print path may not use the media box directly
///
/// `/Rotate` (ISO 32000-1 Table 30) is a clockwise DISPLAY rotation: a
/// 595 x 842 portrait page with `/Rotate 90` is shown, and printed, as
/// an 842 x 595 landscape one. `pdfcer-render` honours it and produces a
/// pixmap with the axes swapped.
///
/// Every consumer of a rasterised page in this crate therefore has to
/// agree with the renderer about which way round the page is, and until
/// 2026-08-18 the shells did not: they took `page_sizes` straight off
/// `media_box` and ignored `/Rotate` entirely. Three things went wrong
/// at once on a rotated page, and only the third is visible:
///
/// 1. [`resolve_orientation`] read the UNROTATED box, so
///    [`Orientation::Auto`] never turned the sheet for a page that
///    displays landscape;
/// 2. [`place_page`] computed a scale and an offset for the wrong aspect
///    ratio, so `clipped` was decided against the wrong rectangle too;
/// 3. the blit stretched a landscape pixmap into a portrait rectangle,
///    which comes out of the printer visibly distorted.
///
/// Measured on a `/Rotate 90` A4: `pdfcer render-page` produced
/// 337 x 238 px while `print-preview` reported `size_pt=595.0x842.0`.
/// The two halves of one program disagreed about the shape of the page.
///
/// A 180-degree rotation swaps nothing, which is exactly why a
/// naive "rotate means transpose" is wrong and this takes the angle
/// rather than a boolean.
///
/// ```
/// use pdfcer_print::displayed_page_size;
///
/// let a4 = (595.0, 842.0);
/// assert_eq!(displayed_page_size(a4, 0), a4);
/// assert_eq!(displayed_page_size(a4, 90), (842.0, 595.0));
/// assert_eq!(displayed_page_size(a4, 180), a4);
/// assert_eq!(displayed_page_size(a4, 270), (842.0, 595.0));
/// // Out-of-range angles are reduced, not trusted: a file may say 450.
/// assert_eq!(displayed_page_size(a4, 450), (842.0, 595.0));
/// // And a value that is not a multiple of 90 cannot be honoured by an
/// // axis swap, so it is treated as no rotation rather than guessed at.
/// assert_eq!(displayed_page_size(a4, 45), a4);
/// ```
#[must_use]
pub fn displayed_page_size(media_box_pt: (f64, f64), rotate_degrees: i32) -> (f64, f64) {
    // `Page::rotate` is normalized to {0, 90, 180, 270} by the parser,
    // but this is a public function and a caller may hand over whatever
    // a file said. Reducing here rather than trusting the caller keeps
    // the two from disagreeing, and costs one modulo.
    let quarter_turns = rotate_degrees.rem_euclid(360) / 90;
    if rotate_degrees.rem_euclid(90) != 0 {
        return media_box_pt;
    }
    if quarter_turns % 2 == 1 {
        (media_box_pt.1, media_box_pt.0)
    } else {
        media_box_pt
    }
}

/// The orientation a page will actually print at.
///
/// Landscape when the page is wider than it is tall — the only sensible
/// reading of `Auto`, and the one that keeps a mixed document upright
/// throughout instead of rotating the drawing to match the text.
#[must_use]
pub fn resolve_orientation(requested: Orientation, page_pt: (f64, f64)) -> Orientation {
    match requested {
        Orientation::Auto => {
            if page_pt.0 > page_pt.1 {
                Orientation::Landscape
            } else {
                Orientation::Portrait
            }
        }
        explicit => explicit,
    }
}

/// The orientation a SHEET is already in, from its own proportions.
///
/// # Why the device's default is derived rather than assumed portrait
///
/// [`printer_caps`] reports whatever the driver's default `DEVMODE`
/// says, and that default is not always portrait: wide-format plotters
/// and label printers ship landscape-default, and an operator can set a
/// landscape default on any printer from the driver's own properties
/// page. Rotation is therefore RELATIVE — the sheet turns only when the
/// job's orientation differs from the one the reported geometry is
/// already in.
///
/// Hard-coding "portrait" here would be right on the common case and
/// silently wrong on the machines whose owners notice, and it would fail
/// in the worse direction: it would turn a sheet that was already turned.
///
/// A square sheet is reported [`Orientation::Portrait`], matching
/// [`resolve_orientation`]'s reading of a square page. The two must
/// agree, or a square page on a square sheet would ask for a rotation
/// that changes nothing but shifts every asymmetric margin.
///
/// ```
/// use pdfcer_print::{Orientation, sheet_orientation};
///
/// assert_eq!(sheet_orientation((612.0, 792.0)), Orientation::Portrait);
/// assert_eq!(sheet_orientation((792.0, 612.0)), Orientation::Landscape);
/// // A square sheet reads upright, matching `resolve_orientation`.
/// assert_eq!(sheet_orientation((612.0, 612.0)), Orientation::Portrait);
/// ```
#[must_use]
pub fn sheet_orientation(physical_pt: (f64, f64)) -> Orientation {
    if physical_pt.0 > physical_pt.1 {
        Orientation::Landscape
    } else {
        Orientation::Portrait
    }
}

impl DeviceGeometry {
    /// The orientation this geometry was reported in.
    ///
    /// See [`sheet_orientation`] for why this is read off the sheet
    /// rather than assumed.
    #[must_use]
    pub fn default_orientation(self) -> Orientation {
        sheet_orientation(self.physical_pt)
    }

    /// This geometry as the DRIVER will present it for a job that
    /// requests `requested` and whose first page is `first_page_pt`.
    ///
    /// # ★ The bug this exists to make unrepresentable
    ///
    /// Orientation reaches the device as `DEVMODE::dmOrientation`, which
    /// turns the SHEET. Everything pdfcer computes about where a page
    /// lands — [`place_page`], [`plan_job`], every `imposition` layout,
    /// and the GUI preview — is computed against a printable area that
    /// [`printer_caps`] read from the device's DEFAULT `DEVMODE`. If the
    /// job's orientation differs from that default and nobody turns the
    /// geometry, the two halves describe different sheets: the driver
    /// prints on 756 × 576 pt while pdfcer planned for 576 × 756 pt. A
    /// Letter page then fits at 0.727 rather than 0.941 — 77% of correct
    /// size, centred, with no clip and nothing to report. It looks like a
    /// scaling preference rather than a defect, which is why it survived.
    ///
    /// This is the ONE place that rotation is written. Every planning
    /// site and the preview take their geometry from here, because two
    /// independently-written rotations eventually disagree and the
    /// disagreement is invisible until it reaches paper.
    ///
    /// # The job has ONE orientation, so it takes ONE page
    ///
    /// A `DEVMODE` applies to the whole job — per-page orientation would
    /// need a `ResetDC` between pages and is not built (see
    /// `build_devmode`). `first_page_pt` is therefore the FIRST page of
    /// the job, the same page `build_devmode` resolves from. Passing a
    /// different one would put the preview and the driver back into
    /// disagreement, which is this bug again in a new place.
    ///
    /// # ★ Why `offset_pt` rotates too, and not by a plain swap
    ///
    /// `offset_pt` is the top-left unprintable margin. The unprintable
    /// margins belong to the PAPER PATH — the gripper edge, the leading
    /// edge — so they stay on the same physical edges of the sheet while
    /// the driver turns the coordinate system around them.
    ///
    /// Win32 landscape puts the drawing origin at what was the sheet's
    /// bottom-left corner, so a sheet point `(x, y)` is addressed as
    /// `(H − y, x)` where `H` is the portrait sheet height. Mapping the
    /// printable rectangle `[ox, ox+w] × [oy, oy+h]` through that gives
    ///
    /// ```text
    /// offset' = (H − oy − h,  ox)      printable' = (h, w)
    /// ```
    ///
    /// — the original BOTTOM margin becomes the new left margin, and the
    /// original LEFT margin becomes the new top margin. The whole ring of
    /// margins turns together (left→top, top→right, right→bottom,
    /// bottom→left), which is the property to check this against.
    ///
    /// A plain component swap `(oy, ox)` is the tempting wrong answer and
    /// is **invisible on a symmetric sheet** — every office printer has
    /// equal margins, so it would pass every test written on one and
    /// misplace every page by the margin difference on a plotter, which
    /// is precisely the hardware this project's operator prints on.
    ///
    /// The inverse mapping is applied when a landscape-default device is
    /// asked for portrait, so `for_orientation` is a true involution
    /// between the two states rather than a one-way turn.
    ///
    /// ```
    /// use pdfcer_print::{DeviceGeometry, Orientation, ScaleMode, place_page};
    ///
    /// // A portrait-default Letter printer, quarter-inch margins.
    /// let device = DeviceGeometry {
    ///     dpi: (600, 600),
    ///     printable_pt: (576.0, 756.0),
    ///     physical_pt: (612.0, 792.0),
    ///     offset_pt: (18.0, 18.0),
    /// };
    /// let landscape_page = (792.0, 612.0);
    ///
    /// // `Auto` on a landscape page turns the sheet. The page then fits
    /// // at 576/612 — the turned sheet's short side over the page's short
    /// // side, which is the binding axis — instead of 576/792, which is
    /// // what the un-turned sheet forced.
    /// let turned = device.for_orientation(Orientation::Auto, landscape_page);
    /// assert_eq!(turned.printable_pt, (756.0, 576.0));
    /// let fitted = place_page(landscape_page, turned.printable_pt, ScaleMode::Fit);
    /// assert!((fitted.scale - 576.0 / 612.0).abs() < 1e-12);
    /// let untouched = place_page(landscape_page, device.printable_pt, ScaleMode::Fit);
    /// assert!((untouched.scale - 576.0 / 792.0).abs() < 1e-12);
    ///
    /// // Forcing portrait leaves the sheet alone, deliberately.
    /// assert_eq!(device.for_orientation(Orientation::Portrait, landscape_page), device);
    /// ```
    #[must_use]
    pub fn for_orientation(self, requested: Orientation, first_page_pt: (f64, f64)) -> Self {
        let target = resolve_orientation(requested, first_page_pt);
        let current = self.default_orientation();
        if target == current {
            return self;
        }
        let (pw, ph) = self.physical_pt;
        let (aw, ah) = self.printable_pt;
        let (ox, oy) = self.offset_pt;
        // Which of the two quarter-turns depends on which way the sheet
        // is going. Both are derived in the doc comment above; each is
        // the other's inverse, so turning a sheet out and back returns
        // the geometry it started with.
        let offset_pt = match target {
            // Portrait-reported sheet, landscape job: origin moves to the
            // portrait bottom-left corner.
            Orientation::Landscape => (ph - oy - ah, ox),
            // Landscape-reported sheet, portrait job: the inverse turn.
            Orientation::Auto | Orientation::Portrait => (oy, pw - ox - aw),
        };
        Self {
            dpi: self.dpi,
            // DPI does NOT swap with the sheet. `LOGPIXELSX`/`LOGPIXELSY`
            // are the device's addressable dot pitch, which is a property
            // of the engine and not of the page's orientation. Swapping
            // them would mis-size every rasterisation on an asymmetric
            // device (600×300 is real on plotters) in a way that only
            // shows up as a stretched print.
            printable_pt: (ah, aw),
            physical_pt: (ph, pw),
            offset_pt,
        }
    }

    /// Build planning geometry from a Win32 device, turned for the job.
    ///
    /// The only route from [`PrinterCaps`] to a `DeviceGeometry`. It
    /// takes the orientation and the job's first page because it must:
    /// see the type docs for what the infallible conversion this replaced
    /// got wrong.
    #[must_use]
    pub fn from_caps(
        caps: &PrinterCaps,
        requested: Orientation,
        first_page_pt: (f64, f64),
    ) -> Self {
        Self {
            dpi: (caps.dpi_x, caps.dpi_y),
            printable_pt: caps.printable_pt,
            physical_pt: caps.physical_pt,
            offset_pt: caps.offset_pt,
        }
        .for_orientation(requested, first_page_pt)
    }
}

/// What the device says it can do, beyond geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeviceFeatures {
    /// The driver reports duplex support.
    ///
    /// A shell must check this before offering a duplex control: an
    /// affordance the hardware cannot honour is R83's failure, and a
    /// duplex checkbox that silently prints single-sided is worse than
    /// no checkbox, because the operator finds out from the paper.
    pub supports_duplex: bool,
    /// The number of copies the DRIVER can produce itself.
    ///
    /// Not the number pdfcer will offer. A device that can collate in
    /// hardware does it faster than pdfcer re-sending pages, but pdfcer
    /// sends its own sequence today, so this is reported rather than
    /// used — and reporting it is what lets a later decision be made on
    /// evidence instead of assumption.
    pub max_copies: u16,
    /// Whether the driver LISTS `DMBIN_FORMSOURCE` among its input bins.
    ///
    /// `DMBIN_FORMSOURCE` is the value that means "choose the tray from
    /// the sheet size", and it is what
    /// [`DeviceSettings::pick_tray_by_page_size`] sends. Read
    /// [`FormSourceSupport`] before using this to gate a control: unlike
    /// [`Self::supports_duplex`], "not listed" is **not** a refusal, and
    /// this was measured rather than assumed.
    pub form_source_bin: FormSourceSupport,
}

/// Whether a device offers `DMBIN_FORMSOURCE` — "choose the tray from
/// the sheet size".
///
/// # ★ Three states, because two would encode a claim that is false
///
/// The natural design here is a `bool` mirroring
/// [`DeviceFeatures::supports_duplex`], and it was written that way
/// first. Then it was measured against the four drivers on the
/// developer's machine on 2026-08-18, and the `bool` turned out to state
/// something untrue:
///
/// | device | what `DC_BINS` said | its OWN default `dmDefaultSource` |
/// |---|---|---|
/// | Microsoft Print to PDF | **would not answer at all** | **15** — `DMBIN_FORMSOURCE` |
/// | Microsoft XPS Document Writer | listed 15 | 15 |
/// | EPSON ET-16600 (network) | answered, no 15 | 7 (`DMBIN_AUTO`) |
/// | EPSON SC-F100 (network) | answered, no 15 | 258 (vendor-defined) |
///
/// The first row is the one that decides the shape, and it decides it
/// twice over. "Microsoft Print to PDF" — the commonest printer on any
/// Windows machine — returns NO bin list, and its own default
/// configuration is already `DMBIN_FORMSOURCE`. A `bool` would have
/// collapsed "the driver said nothing" into "no" and reported "this
/// device cannot pick a tray by size" about a device that does exactly
/// that by default. Windows' Form-to-Tray Assignment is a spooler-level
/// feature and a driver is under no obligation to advertise it as a
/// selectable bin.
///
/// So the three-way distinction is real and an `Option`-shaped or
/// `bool`-shaped answer hides the disagreeing case inside the negative
/// one.
///
/// # What a shell should do with each
///
/// - [`Self::Listed`] — offer the control plainly.
/// - [`Self::NotListed`] — **still offer it**, and disclose that the
///   driver does not advertise it, so the request may be ignored. Hiding
///   it here would remove a working capability from the operator on the
///   commonest Windows printer there is.
/// - [`Self::Unknown`] — same as `NotListed`, with the honest reason:
///   nothing was learned either way.
///
/// The R83 rule this looks like — "never offer an affordance the
/// hardware cannot honour" — is not in play, because `DC_BINS` does not
/// establish that it cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormSourceSupport {
    /// `DC_BINS` includes `DMBIN_FORMSOURCE`. The driver advertises it.
    Listed,
    /// `DC_BINS` answered and did not include it. **Not a refusal** —
    /// see the type's own table.
    NotListed,
    /// `DC_BINS` did not answer. Nothing is known either way, which is
    /// a different fact from `NotListed` and is kept distinct for that
    /// reason.
    #[default]
    Unknown,
}

#[cfg(windows)]
/// Read the non-geometric capabilities of a device.
///
/// # Errors
///
/// [`PrintError::OpenDevice`] if the printer name does not resolve.
pub fn device_features(printer: &str) -> Result<DeviceFeatures, PrintError> {
    use windows::Win32::Storage::Xps::{DC_BINS, DC_COPIES, DC_DUPLEX, DeviceCapabilitiesW};
    use windows::core::PCWSTR;

    let wide: Vec<u16> = printer.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is NUL-terminated and outlives both calls. A
    // negative return is the documented failure and is treated as
    // "the driver will not say", never as a capability.
    let (duplex, copies) = unsafe {
        (
            DeviceCapabilitiesW(PCWSTR(wide.as_ptr()), PCWSTR::null(), DC_DUPLEX, None, None),
            DeviceCapabilitiesW(PCWSTR(wide.as_ptr()), PCWSTR::null(), DC_COPIES, None, None),
        )
    };
    if duplex < 0 && copies < 0 {
        return Err(PrintError::OpenDevice(printer.to_owned()));
    }
    // `DC_BINS` fills an array of WORDs with the `DMBIN_*` values this
    // device offers. The two-call pattern is the same as everywhere else
    // in this crate: the first asks how many, the second fills. A driver
    // that will not answer leaves the list empty, which reads as "does
    // not support it" — the safe direction, stated in the field's docs.
    let bins = device_capability_words(&wide, DC_BINS);
    Ok(DeviceFeatures {
        form_source_bin: if bins.is_empty() {
            FormSourceSupport::Unknown
        } else if bins.contains(&DMBIN_FORMSOURCE_VALUE) {
            FormSourceSupport::Listed
        } else {
            FormSourceSupport::NotListed
        },
        // `DC_DUPLEX` returns 1 when the device supports it. A driver
        // that will not answer is treated as NOT supporting it — the
        // safe direction, because the cost of being wrong the other way
        // is a job the operator believes is two-sided and is not.
        supports_duplex: duplex == 1,
        max_copies: u16::try_from(copies.max(1)).unwrap_or(1),
    })
}

/// `DMBIN_FORMSOURCE` as a bare `u16`, for comparing against the
/// `DC_BINS` array.
///
/// Duplicated from the `devmode` module's `i16` copy rather than shared,
/// because `DC_BINS` reports unsigned words and `dmDefaultSource` is a
/// signed member. The `devmode` module's `#[cfg(windows)]` ABI guard
/// asserts its copy against the real constant; this one is asserted in
/// `dmbin_formsource_agrees_across_its_two_representations`.
#[cfg(windows)]
const DMBIN_FORMSOURCE_VALUE: u16 = 15;

/// Run a `DeviceCapabilities` query that returns an array of WORDs.
///
/// # Why the two-call pattern is required rather than defensive
///
/// `DeviceCapabilitiesW` with a null output buffer returns the ENTRY
/// COUNT — not a byte count, which is the trap: `DC_PAPERSIZE` returns
/// the same count for entries that are eight bytes each. Guessing a
/// buffer size instead would either truncate a plotter's form list
/// silently or over-allocate on every call.
///
/// An empty `Vec` means "the driver would not say", which every caller
/// treats as an absent capability rather than an error, for the reason
/// [`DeviceFeatures::supports_form_source_bin`] states.
#[cfg(windows)]
fn device_capability_words(
    printer_wide: &[u16],
    capability: windows::Win32::Storage::Xps::PRINTER_DEVICE_CAPABILITIES,
) -> Vec<u16> {
    use windows::Win32::Storage::Xps::DeviceCapabilitiesW;
    use windows::core::{PCWSTR, PWSTR};

    // SAFETY: `printer_wide` is NUL-terminated and outlives the call; a
    // null output buffer is the documented "how many?" form.
    let count = unsafe {
        DeviceCapabilitiesW(
            PCWSTR(printer_wide.as_ptr()),
            PCWSTR::null(),
            capability,
            None,
            None,
        )
    };
    let Ok(count) = usize::try_from(count) else {
        return Vec::new();
    };
    if count == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0u16; count];
    // SAFETY: `buffer` holds exactly the `count` words the call above
    // asked for. The parameter is typed `PWSTR` by the `windows` crate
    // because the Win32 signature reuses one pointer for every
    // capability's payload; for `DC_BINS` and `DC_PAPERS` that payload
    // is an array of WORDs, not a string.
    let written = unsafe {
        DeviceCapabilitiesW(
            PCWSTR(printer_wide.as_ptr()),
            PCWSTR::null(),
            capability,
            Some(PWSTR(buffer.as_mut_ptr())),
            None,
        )
    };
    match usize::try_from(written) {
        Ok(n) => {
            buffer.truncate(n.min(count));
            buffer
        }
        // A driver that answered the count and then failed the fill is
        // reporting nothing, not reporting `count` zeroes.
        Err(_) => Vec::new(),
    }
}

/// The paper sizes a device offers.
///
/// # ★ Three parallel arrays related only by index
///
/// Win32 answers this in three separate calls — `DC_PAPERS` for the
/// `dmPaperSize` ids, `DC_PAPERNAMES` for 64-character names, and
/// `DC_PAPERSIZE` for the dimensions — and nothing but the INDEX relates
/// them. A driver that returns different counts for the three (which
/// happens: a `DC_PAPERNAMES` implementation can be missing while
/// `DC_PAPERS` works) would, zipped naively, produce forms whose name
/// belongs to a different sheet. So this zips to the SHORTEST array and
/// fills what is missing rather than pairing across a gap:
///
/// - no id → the form is dropped entirely, because an id is what
///   [`PaperSelection::Form`] needs and a form that cannot be selected
///   is an affordance for something that cannot happen (R83);
/// - no name → the id is used as the name (`"form 9"`), which is ugly
///   and true;
/// - no size → `(0.0, 0.0)`, and a shell showing sizes must treat that
///   as "the driver would not say" rather than as a zero-area sheet.
///
/// # Errors
///
/// [`PrintError::OpenDevice`] when the printer name does not resolve —
/// i.e. when even `DC_PAPERS` refuses. An empty list from a device that
/// DID answer is not an error, for the same reason
/// [`list_printers`] treats an empty machine as normal.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), pdfcer_print::PrintError> {
/// for form in pdfcer_print::printer_forms("Microsoft Print to PDF")? {
///     println!("{}: {} ({:.0}x{:.0} pt)", form.id, form.name, form.size_pt.0, form.size_pt.1);
/// }
/// # Ok(())
/// # }
/// ```
#[cfg(windows)]
pub fn printer_forms(printer: &str) -> Result<Vec<PaperForm>, PrintError> {
    use windows::Win32::Storage::Xps::{
        DC_PAPERNAMES, DC_PAPERS, DC_PAPERSIZE, DeviceCapabilitiesW,
    };
    use windows::core::{PCWSTR, PWSTR};

    let wide: Vec<u16> = printer.encode_utf16().chain(std::iter::once(0)).collect();
    let ids = device_capability_words(&wide, DC_PAPERS);
    if ids.is_empty() {
        // Distinguish "no such printer" from "this printer offers no
        // forms": ask something every device answers.
        // SAFETY: `wide` is NUL-terminated and outlives the call.
        let probe = unsafe {
            DeviceCapabilitiesW(
                PCWSTR(wide.as_ptr()),
                PCWSTR::null(),
                DC_PAPERNAMES,
                None,
                None,
            )
        };
        if probe < 0 {
            return Err(PrintError::OpenDevice(printer.to_owned()));
        }
        return Ok(Vec::new());
    }

    // `DC_PAPERNAMES` writes fixed 64-WCHAR records, NUL-padded, NOT
    // NUL-terminated when the name fills the field — which is why the
    // decode below takes exactly 64 units per record rather than reading
    // to a terminator.
    const NAME_UNITS: usize = 64;
    let mut names: Vec<String> = Vec::new();
    {
        let mut buffer = vec![0u16; ids.len() * NAME_UNITS];
        // SAFETY: `buffer` holds `ids.len()` records of `NAME_UNITS`
        // words, which is the layout `DC_PAPERNAMES` documents for a
        // list of that length.
        let written = unsafe {
            DeviceCapabilitiesW(
                PCWSTR(wide.as_ptr()),
                PCWSTR::null(),
                DC_PAPERNAMES,
                Some(PWSTR(buffer.as_mut_ptr())),
                None,
            )
        };
        if let Ok(n) = usize::try_from(written) {
            for record in buffer.chunks_exact(NAME_UNITS).take(n) {
                let end = record.iter().position(|&c| c == 0).unwrap_or(NAME_UNITS);
                names.push(String::from_utf16_lossy(
                    record.get(..end).unwrap_or_default(),
                ));
            }
        }
    }

    // `DC_PAPERSIZE` writes an array of `POINT` — two `i32` — in TENTHS
    // OF A MILLIMETRE. Not points, despite the name of the structure;
    // the collision between Win32's "POINT" and PDF's "point" is exactly
    // the kind of unit confusion this crate converts at the boundary so
    // nothing downstream has to know.
    let mut sizes: Vec<(f64, f64)> = Vec::new();
    {
        let mut buffer = vec![0i32; ids.len() * 2];
        // SAFETY: `buffer` holds `ids.len()` POINT records. The `PWSTR`
        // parameter is Win32's one-pointer-for-every-payload convention,
        // as in `device_capability_words`.
        let written = unsafe {
            DeviceCapabilitiesW(
                PCWSTR(wide.as_ptr()),
                PCWSTR::null(),
                DC_PAPERSIZE,
                Some(PWSTR(buffer.as_mut_ptr().cast::<u16>())),
                None,
            )
        };
        if let Ok(n) = usize::try_from(written) {
            for point in buffer.chunks_exact(2).take(n) {
                let tenths_mm_to_pt = |v: i32| {
                    if v > 0 {
                        f64::from(v) * 72.0 / 254.0
                    } else {
                        0.0
                    }
                };
                sizes.push((
                    tenths_mm_to_pt(*point.first().unwrap_or(&0)),
                    tenths_mm_to_pt(*point.get(1).unwrap_or(&0)),
                ));
            }
        }
    }

    Ok(ids
        .into_iter()
        .enumerate()
        .map(|(i, id)| PaperForm {
            id,
            name: names
                .get(i)
                .filter(|n| !n.is_empty())
                .cloned()
                .unwrap_or_else(|| format!("form {id}")),
            size_pt: sizes.get(i).copied().unwrap_or((0.0, 0.0)),
        })
        .collect())
}

/// Open a printer handle, run `f`, and close it on every path.
///
/// `DocumentProperties` needs a spooler handle, not a device context,
/// and a leaked printer handle holds a spooler object open. The closure
/// shape is the same one [`spool`] uses for its device context, and for
/// the same reason: Rust has no `finally`.
#[cfg(windows)]
fn with_printer_handle<T>(
    printer: &str,
    f: impl FnOnce(windows::Win32::Graphics::Printing::PRINTER_HANDLE) -> T,
) -> Result<T, PrintError> {
    use windows::Win32::Graphics::Printing::{ClosePrinter, OpenPrinterW, PRINTER_HANDLE};
    use windows::core::PCWSTR;

    let wide: Vec<u16> = printer.encode_utf16().chain(std::iter::once(0)).collect();
    let mut handle = PRINTER_HANDLE::default();
    // SAFETY: `wide` is NUL-terminated and outlives the call; `handle`
    // is a live local the spooler writes into.
    unsafe { OpenPrinterW(PCWSTR(wide.as_ptr()), &raw mut handle, None) }
        .map_err(|_| PrintError::OpenDevice(printer.to_owned()))?;
    let out = f(handle);
    // SAFETY: `handle` was opened above and is closed exactly once.
    unsafe {
        let _ = ClosePrinter(handle);
    }
    Ok(out)
}

/// Ask a driver for its current settings.
///
/// This is the base every job now starts from: `DocumentProperties` with
/// `DM_OUT_BUFFER` returns a FULLY-POPULATED `DEVMODE` — every field the
/// driver knows about, including the private tail Win32 has no names
/// for — where pdfcer previously synthesised one from zero. See the
/// `devmode` module's own docs for what that cost.
///
/// # Errors
///
/// [`PrintError::OpenDevice`] if the name does not resolve;
/// [`PrintError::DriverSettings`] if the spooler will not describe the
/// device, which a disconnected network printer does;
/// [`PrintError::Configuration`] if what the driver wrote is not a
/// `DEVMODE` pdfcer can amend — re-validated rather than trusted, because
/// a driver's output is untrusted input in exactly the way a file is.
#[cfg(windows)]
pub fn printer_configuration(printer: &str) -> Result<PrinterConfiguration, PrintError> {
    document_properties(printer, Prompt::No, None, None)?.ok_or_else(|| {
        PrintError::DriverSettings {
            printer: printer.to_owned(),
        }
    })
}

/// Let the operator edit a device's settings in the DRIVER's own dialog.
///
/// # ★ Why a UI call lives in a crate with no UI dependency
///
/// It is arguable, so here is the reasoning rather than an assertion.
///
/// **Against:** `DocumentProperties` with `DM_IN_PROMPT` opens a modal
/// window, and windowing code in a non-windowing crate is the wrong
/// direction. This crate exists partly BECAUSE `pdfcer-core` and
/// `pdfcer-render` must stay platform-free.
///
/// **For, and it decides it:** the dialog's OUTPUT is a `DEVMODE`, and a
/// `DEVMODE` is meaningless to anything but [`spool_with_config`]. A
/// shell that opened this dialog itself and could not hand the result
/// anywhere would let an operator configure settings that are then
/// discarded — which is exactly the defect
/// [`DeviceSettings::pick_tray_by_page_size`] was, rebuilt deliberately.
/// And `pdfcer` prints too: a properties dialog living only in the
/// GUI would be a capability the CLI could not reach, which is the same
/// boundary error in the other direction.
///
/// No windowing dependency is added — `parent` is a raw window handle
/// the caller already owns, passed as an integer, and this crate never
/// creates a window.
///
/// # The handle argument
///
/// `parent` is an `HWND` as `isize`. `None` passes a null owner, which
/// Windows accepts: the dialog is then unowned and can fall behind the
/// application's window. A GUI should always pass its own handle; a CLI
/// has none to pass and `None` is correct there.
///
/// # Returns
///
/// `Ok(None)` when the operator pressed **Cancel**. That is not an
/// error and must not be reported as one — it is the operator declining,
/// and a shell that showed an error for it would be scolding them for
/// using the dialog correctly.
///
/// # Errors
///
/// The same three as [`printer_configuration`].
#[cfg(windows)]
pub fn edit_printer_configuration(
    printer: &str,
    parent: Option<isize>,
    start_from: Option<&PrinterConfiguration>,
) -> Result<Option<PrinterConfiguration>, PrintError> {
    document_properties(printer, Prompt::Yes, parent, start_from)
}

/// Whether [`document_properties`] shows the driver's dialog.
///
/// An enum rather than a `bool` because the two callers are the whole
/// difference between "read the device's settings" and "open a modal
/// window on the operator's screen", and a bare `true` at a call site
/// does not say which of those it is asking for.
#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prompt {
    /// Read the driver's settings; show nothing.
    No,
    /// Open the driver's own properties dialog.
    Yes,
}

/// The one `DocumentProperties` call site, in both its modes.
///
/// # The three-step protocol, and why each step is required
///
/// 1. `fMode = 0` returns the BYTE SIZE of this driver's `DEVMODE`,
///    which is `dmSize + dmDriverExtra` and is driver-specific. There is
///    no way to ask for it any other way, and a fixed-size buffer would
///    truncate the private tail of every vendor driver.
/// 2. A buffer of that size is allocated **aligned**, via
///    `PrinterConfiguration::to_aligned_words`: `DEVMODEW` contains
///    `u32` members and a `Vec<u8>` does not guarantee 4-byte alignment.
/// 3. The real call fills it. With `DM_IN_PROMPT` the return is a dialog
///    result (`IDOK` = 1, `IDCANCEL` = 2); without it, `IDOK` on success.
///    A negative return is failure.
///
/// # ★ `DM_IN_PROMPT` and `DM_PAPERLENGTH` are the same number
///
/// Win32 has two unrelated families of `DM_*` constants — the `fMode`
/// flags for this function, and the `dmFields` flags inside the
/// structure — and the `windows` crate gives BOTH the type
/// `DEVMODE_FIELD_FLAGS`. `DM_IN_PROMPT` is 4 and so is
/// `DM_PAPERLENGTH`; `DM_OUT_BUFFER` is 2 and so is `DM_PAPERSIZE`.
/// Passing the wrong one compiles, type-checks, and silently means
/// something else. They are spelled out at this one call site for that
/// reason, and never mixed with the `devmode` module's `dmFields` bits.
#[cfg(windows)]
fn document_properties(
    printer: &str,
    prompt: Prompt,
    parent: Option<isize>,
    start_from: Option<&PrinterConfiguration>,
) -> Result<Option<PrinterConfiguration>, PrintError> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{DEVMODEW, DM_IN_BUFFER, DM_IN_PROMPT, DM_OUT_BUFFER};
    use windows::Win32::Graphics::Printing::DocumentPropertiesW;
    use windows::core::PCWSTR;

    /// `IDOK` — the dialog was accepted, or the buffer-only call
    /// succeeded.
    const IDOK: i32 = 1;

    if let Some(config) = start_from {
        config.ensure_device(printer)?;
    }
    let name: Vec<u16> = printer.encode_utf16().chain(std::iter::once(0)).collect();
    let hwnd = parent.map(|h| HWND(h as *mut core::ffi::c_void));

    with_printer_handle(printer, |handle| {
        // Step 1 — how big is this driver's DEVMODE?
        // SAFETY: `name` is NUL-terminated and outlives the call; null
        // buffers with `fMode = 0` are the documented size query.
        let needed =
            unsafe { DocumentPropertiesW(None, handle, PCWSTR(name.as_ptr()), None, None, 0) };
        let Ok(needed) = usize::try_from(needed) else {
            return Err(PrintError::DriverSettings {
                printer: printer.to_owned(),
            });
        };
        if needed == 0 {
            return Err(PrintError::DriverSettings {
                printer: printer.to_owned(),
            });
        }

        // Step 2 — an ALIGNED buffer of exactly that size, never
        // smaller than the public structure this Windows defines so
        // that the pointer handed over is fully in bounds either way.
        let words = needed.max(devmode::DEVMODE_PUBLIC_BYTES).div_ceil(4);
        let mut out = vec![0u32; words];
        let input = start_from.map(PrinterConfiguration::to_aligned_words);

        // `DM_IN_BUFFER` means "start from the DEVMODE I am supplying";
        // `DM_IN_PROMPT` means "show the dialog". They compose, and the
        // combination is how a shell reopens the dialog on the settings
        // the operator chose last time rather than resetting them.
        let mut mode = DM_OUT_BUFFER.0;
        if start_from.is_some() {
            mode |= DM_IN_BUFFER.0;
        }
        if prompt == Prompt::Yes {
            mode |= DM_IN_PROMPT.0;
        }

        // SAFETY: `out` holds `needed` bytes as just requested; `input`
        // when present is a validated DEVMODE of its own declared
        // length; both outlive the call.
        let result = unsafe {
            DocumentPropertiesW(
                hwnd,
                handle,
                PCWSTR(name.as_ptr()),
                Some(out.as_mut_ptr().cast::<DEVMODEW>()),
                input.as_ref().map(|w| w.as_ptr().cast::<DEVMODEW>()),
                mode,
            )
        };
        if result < 0 {
            return Err(PrintError::DriverSettings {
                printer: printer.to_owned(),
            });
        }
        if result != IDOK {
            // `IDCANCEL`. The operator declined; not an error.
            return Ok(None);
        }
        PrinterConfiguration::from_aligned_words(&out, needed)
            .map(Some)
            .map_err(PrintError::Configuration)
    })?
}

#[cfg(not(windows))]
/// Non-Windows stub. Printing is a Windows capability in this release.
///
/// # Errors
///
/// Always [`PrintError::Unsupported`].
pub fn device_features(_printer: &str) -> Result<DeviceFeatures, PrintError> {
    Err(PrintError::Unsupported)
}

#[cfg(not(windows))]
/// Non-Windows stub. Printing is a Windows capability in this release.
///
/// # Errors
///
/// Always [`PrintError::Unsupported`].
pub fn printer_forms(_printer: &str) -> Result<Vec<PaperForm>, PrintError> {
    Err(PrintError::Unsupported)
}

#[cfg(not(windows))]
/// Non-Windows stub. Printing is a Windows capability in this release.
///
/// # Errors
///
/// Always [`PrintError::Unsupported`].
pub fn printer_configuration(_printer: &str) -> Result<PrinterConfiguration, PrintError> {
    Err(PrintError::Unsupported)
}

#[cfg(not(windows))]
/// Non-Windows stub. Printing is a Windows capability in this release.
///
/// # Errors
///
/// Always [`PrintError::Unsupported`].
pub fn edit_printer_configuration(
    _printer: &str,
    _parent: Option<isize>,
    _start_from: Option<&PrinterConfiguration>,
) -> Result<Option<PrinterConfiguration>, PrintError> {
    Err(PrintError::Unsupported)
}

#[cfg(not(windows))]
/// Non-Windows stub. Printing is a Windows capability in this release.
///
/// Returns an error rather than an empty list, deliberately: §`list_printers`
/// on Windows documents that an empty `Vec` means "this machine has no
/// printers installed", which is a normal machine. Reporting the same value
/// for "this platform cannot enumerate printers at all" would collapse two
/// different facts into one and send a caller looking for hardware.
///
/// # Errors
///
/// Always [`PrintError::Unsupported`].
pub fn list_printers() -> Result<Vec<Printer>, PrintError> {
    Err(PrintError::Unsupported)
}

#[cfg(not(windows))]
/// Non-Windows stub. Printing is a Windows capability in this release.
///
/// # Errors
///
/// Always [`PrintError::Unsupported`].
pub fn printer_caps(_name: &str) -> Result<PrinterCaps, PrintError> {
    Err(PrintError::Unsupported)
}

#[cfg(not(windows))]
/// Non-Windows stub. Printing is a Windows capability in this release.
///
/// # Errors
///
/// Always [`PrintError::Unsupported`].
pub fn printer_caps_for(
    _name: &str,
    _config: Option<&PrinterConfiguration>,
    _paper: PaperSelection,
) -> Result<PrinterCaps, PrintError> {
    Err(PrintError::Unsupported)
}

// ---------------------------------------------------------------------------
// Spooling (§ the irreversible half)
// ---------------------------------------------------------------------------

/// One page's pixels, ready to place on a sheet.
///
/// The caller rasterises. This crate does device setup, placement and
/// blitting, and knows nothing about PDF — which is why it does not
/// depend on `pdfcer-render`.
///
/// That split is deliberate rather than incidental: a printing crate
/// that also rendered would need the whole render stack to be testable,
/// and the interesting failures here (a wrong `DEVMODE`, an upside-down
/// DIB, a job left open on an error path) have nothing to do with PDF.
#[derive(Debug, Clone)]
pub struct PageBitmap {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGBA8, row-major, top row first — the layout `tiny_skia::Pixmap`
    /// produces, so the caller hands over `pixmap.data().to_vec()`
    /// unchanged.
    pub rgba: Vec<u8>,
    /// Where this page lands on the sheet, from [`place_page`].
    pub placement: Placement,
    /// The page's size in PDF points, for the placement arithmetic.
    pub page_pt: (f64, f64),
}

/// How ONE sheet is fed: which way up, and on what paper.
///
/// # Why these two and not the whole of [`DeviceSettings`]
///
/// They are the two that describe the SHEET's shape, and they are the
/// two a mixed document genuinely varies. Duplex and tray describe how
/// the job is FED — nothing has asked to vary them mid-job, and a
/// `DMDUP_SIMPLEX` asserted part-way would silently cancel a driver's
/// own duplex default, which is a defect this crate has already had
/// once. They stay job-wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SheetSetup {
    /// Which way up. [`Orientation::Auto`] is resolved by
    /// [`spool_sheets`] against the sheet's own `page_pt` — which is
    /// right for a plain page and WRONG for an imposed one, where
    /// `page_pt` is the printable area rather than a source page, so an
    /// n-up or booklet caller resolves it itself.
    pub orientation: Orientation,
    /// Which sheet to feed.
    pub paper: PaperSelection,
}

/// One sheet of a job: its pixels, and the setup they need.
///
/// # Why the bitmap is borrowed
///
/// A rasterised page is megabytes — an A4 sheet at 300 DPI is about
/// 35 MB of RGBA — and [`spool`] builds one of these per page from a
/// slice it does not own. Owning the bitmap here would make the
/// compatibility path copy the whole job.
///
/// No `PartialEq`: [`PageBitmap`] has none, and deriving one that
/// compared several megabytes of pixels per call would be a trap wearing
/// a common trait's name (`C-COMMON-TRAITS` wants the traits that make
/// sense, not all of them).
#[derive(Debug, Clone, Copy)]
pub struct Sheet<'a> {
    /// The page's pixels and its placement on the sheet, already
    /// computed by the caller against THIS sheet's geometry.
    pub bitmap: &'a PageBitmap,
    /// How this sheet is fed.
    pub setup: SheetSetup,
}

/// Whether [`spool`] actually starts a print job.
///
/// # ★ Not a testing convenience — the development mode
///
/// [`DryRun::Yes`] performs every step except the four that reach the
/// spooler (`StartDoc`, `StartPage`, `EndPage`, `EndDoc`) and the blit.
/// It opens the device context, reads the real device's resolution and
/// printable area, computes placement for every page, and walks the
/// whole loop.
///
/// So the things that actually go wrong — a printer name that does not
/// resolve, a device that reports a printable area smaller than the
/// caller assumed, a page whose scaled size clips, an arithmetic slip in
/// the DIB header — all surface without a sheet of paper moving.
///
/// This exists because the machine this was written on has one printer
/// and its owner was sitting at it. That constraint produced a better
/// design than unlimited paper would have: the expensive, irreversible
/// step is isolated behind one flag rather than woven through the
/// function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DryRun {
    /// Do everything except start a job. Nothing prints.
    Yes,
    /// Start a real job on a real device. **Consumes paper.**
    No,
}

/// What a spool attempt did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolReport {
    /// Pages sent, or that would have been sent under [`DryRun::Yes`].
    pub pages: usize,
    /// Whether a job was actually started.
    pub printed: bool,
    /// The device's reported resolution.
    pub dpi: (i32, i32),
    /// Pages whose placement reported [`Placement::clipped`].
    ///
    /// Reported rather than refused: an operator may legitimately want a
    /// page cropped to the sheet, and Acrobat clips silently. pdfcer
    /// clips and SAYS so — the operator's standing ruling that parity is
    /// a floor.
    pub clipped_pages: usize,
    /// The job's spooler ID, when one was started.
    pub job_id: Option<u32>,
    /// How many DISTINCT sheet setups the job used.
    ///
    /// `1` for an ordinary job. More means the device was reconfigured
    /// mid-job — pages that print a different way up, or on different
    /// paper — which is worth reporting because it is not visible in a
    /// page count and it is the operator's evidence that
    /// [`Orientation::Auto`] did the per-page thing its documentation
    /// promises.
    pub sheet_setups: usize,
    /// Where the `DEVMODE` this job was sent with came from.
    ///
    /// The disclosure that a shell owes the operator under project
    /// rule 4: pdfcer may have had to fall back to a synthesised
    /// configuration, and that changes what a driver-level setting can
    /// mean. Silence about it is exactly the shape the `pick_tray`
    /// defect had — a job that succeeds either way.
    pub settings_source: SettingsSource,
}

/// Where the `DEVMODE` a job was sent with came from.
///
/// # Why this is reported rather than assumed
///
/// pdfcer writes at most four members of a `DEVMODE`. Everything else a
/// device does — media type, quality, stapling, output bin, the whole
/// vendor-private half — lives in the driver's own configuration, which
/// pdfcer carries through untouched *when it has one*. Whether it had one
/// is therefore a fact about what the job could possibly honour, and it
/// is not visible from the printed page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsSource {
    /// No `DEVMODE` was sent at all: nothing pdfcer controls differs from
    /// what the device is already set to, so the device's own defaults
    /// apply in full. The cheapest and most conservative case, and the
    /// common one.
    #[default]
    DeviceDefault,
    /// The driver's own current settings were fetched and amended. The
    /// normal case for a job that changes anything.
    DriverSupplied,
    /// A configuration the CALLER supplied — from
    /// [`edit_printer_configuration`] or a stored file — was amended.
    CallerSupplied,
    /// ★ The driver would not report its settings, so pdfcer sent a
    /// SYNTHESISED `DEVMODE` carrying only what it sets itself.
    ///
    /// The job prints. What is lost is everything the driver holds that
    /// pdfcer does not model, because a synthesised structure has no
    /// driver-private tail to carry it in. A shell must say so: this is
    /// pdfcer having chosen something the operator did not ask for.
    Synthesised,
}

/// Send pages to a printer — **the only function in pdfcer that starts a
/// print job**.
///
/// # Errors
///
/// [`PrintError`] if the printer cannot be resolved, the device context
/// cannot be created, or the spooler rejects the job. A job that fails
/// part-way is ABORTED rather than left open (see the guard below), so a
/// half-finished document does not sit in the queue holding a device.
///
/// # Safety of the irreversible step
///
/// `StartDoc` is reached on exactly one code path, guarded by
/// [`DryRun::No`], and this function is called from exactly one place in
/// each shell — a control the operator clicked. Nothing here runs as a
/// side effect of rendering, previewing, saving or opening.
#[cfg(windows)]
pub fn spool(
    printer: &str,
    pages: &[PageBitmap],
    dry_run: DryRun,
    output: Option<&std::path::Path>,
    settings: DeviceSettings,
    first_page_pt: (f64, f64),
) -> Result<SpoolReport, PrintError> {
    spool_with_config(
        printer,
        pages,
        dry_run,
        output,
        settings,
        first_page_pt,
        None,
    )
}

/// [`spool`], starting from a `DEVMODE` the caller supplies.
///
/// # Why this is a second function rather than a seventh parameter
///
/// [`spool`] is called from both shells and its signature is the one
/// they compile against. A configuration is an addition that most
/// callers never need — a caller that has not opened the driver's
/// properties dialog has nothing to pass — so the common call keeps its
/// shape and the capability gets its own name. The two share one
/// implementation below; there is no second copy of the job loop.
///
/// `config` is amended, not replaced: the [`DeviceSettings`] still win
/// for the members they name, exactly as they do over a driver-supplied
/// base. Everything else in the operator's configuration survives, which
/// is the entire point of carrying it.
///
/// # Errors
///
/// The same as [`spool`], plus [`PrintError::Configuration`] when
/// `config` belongs to a different device — a `DEVMODE`'s private tail
/// is one driver's private format, so handing it to another is not a
/// degraded result but an undefined one.
#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
pub fn spool_with_config(
    printer: &str,
    pages: &[PageBitmap],
    dry_run: DryRun,
    output: Option<&std::path::Path>,
    settings: DeviceSettings,
    first_page_pt: (f64, f64),
    config: Option<&PrinterConfiguration>,
) -> Result<SpoolReport, PrintError> {
    // ONE setup for the whole job, resolved from the one page the caller
    // nominated — which is exactly what a single `DEVMODE` has always
    // meant here, and is what [`spool`]'s callers still get.
    let setup = SheetSetup {
        orientation: resolve_orientation(settings.orientation, first_page_pt),
        paper: settings.paper,
    };
    let sheets: Vec<Sheet<'_>> = pages.iter().map(|bitmap| Sheet { bitmap, setup }).collect();
    spool_sheets(printer, &sheets, dry_run, output, settings, config)
}

/// Send sheets that do not all print the same way up, or on the same
/// paper.
///
/// # ★ What this exists to fix: `Auto` was documented per-page and was
/// # per-job
///
/// [`Orientation`]'s own documentation said, under a heading that made
/// the claim the point:
///
/// > *"Acrobat's default computes orientation **for each page** within
/// > one job — a document mixing portrait text with a landscape drawing
/// > gets both, from one command."*
///
/// It did not. [`resolve_orientation`] was called twice, both times for
/// the whole job, because a `DEVMODE` is handed to `CreateDC` once and
/// applies until something changes it. A document mixing an A4 portrait
/// title sheet with A3 landscape drawings — which is what a CAD export
/// IS — printed every sheet in whichever orientation page 1 resolved to.
///
/// The doc comment was the defect, not merely a description of one: it
/// is a claim that reads as true, and this project's costliest failures
/// are all that shape.
///
/// # The Win32 mechanism, and the trap the old comment already named
///
/// `ResetDC` is the documented way to change a device's `DEVMODE`
/// mid-job, and it must be called BETWEEN pages — after `EndPage` and
/// before the next `StartPage`. That is what the loop below does.
///
/// The trap the previous implementation's comment named is real: a reset
/// changes the printable area, and everything pdfcer computed about where
/// a page lands was computed against the area read BEFORE. So this
/// function does not attempt to re-place anything. The caller places
/// each sheet against ITS OWN geometry — [`DeviceGeometry::from_caps`]
/// turned for that sheet — and hands the result over already placed, in
/// [`Sheet::bitmap`]. Placement stays in one place, this function stays
/// responsible only for telling the driver, and the two cannot come to
/// disagree because neither does the other's job.
///
/// # What does NOT vary per sheet, and why
///
/// Duplex and tray. Both are properties of how the job is FED rather
/// than of a sheet's shape, changing them mid-job is not something any
/// caller has asked for, and a `DMDUP_SIMPLEX` asserted mid-job would
/// silently cancel a driver's own duplex default — the defect recorded
/// in [`job_configuration`]'s notes. They come from `settings` and apply
/// to the whole job.
///
/// # Cost
///
/// One `ResetDC` per CHANGE, not per page: identical consecutive setups
/// share one, and a job whose sheets all agree issues none at all and is
/// byte-for-byte the same sequence of Win32 calls as before this
/// function existed. The driver's `DEVMODE` is fetched ONCE and amended
/// per distinct setup, so a hundred alternating pages cost two
/// structures, not a hundred.
///
/// # Errors
///
/// The same as [`spool`], plus [`PrintError::SheetSetup`] when the
/// driver refuses a mid-job change. That is an error rather than a
/// degradation: continuing would print the remaining sheets the wrong
/// way up, which is the silent-wrong-output case this whole change set
/// exists to remove.
#[cfg(windows)]
pub fn spool_sheets(
    printer: &str,
    sheets: &[Sheet<'_>],
    dry_run: DryRun,
    output: Option<&std::path::Path>,
    settings: DeviceSettings,
    config: Option<&PrinterConfiguration>,
) -> Result<SpoolReport, PrintError> {
    use windows::Win32::Graphics::Gdi::{CreateDCW, DEVMODEW, DeleteDC, ResetDCW};
    use windows::Win32::Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};
    use windows::core::PCWSTR;

    let caps = printer_caps(printer)?;
    let wide: Vec<u16> = printer.encode_utf16().chain(std::iter::once(0)).collect();

    // ★ `caps` is the UN-TURNED geometry, and that is exactly what is
    // wanted here. The decision below needs to know which orientation
    // the device is in BY DEFAULT so it can tell whether a sheet needs
    // it turned; an already-rotated view would make every sheet look
    // like it needed no turn. The rotated view is the CALLER's concern —
    // it is what the pages were planned against, and it reaches this
    // function baked into `PageBitmap::placement`.
    let device_default = sheet_orientation(caps.physical_pt);

    let resolved = resolve_sheet_setups(sheets);

    // The cheap path survives: if no sheet needs anything said to the
    // driver, none is fetched, none is built, and no reset is issued.
    let needs_devmode = config.is_some()
        || resolved
            .iter()
            .any(|setup| setup_needs_devmode(settings, *setup, device_default));
    let (base, settings_source) = if needs_devmode {
        let (base, source) = job_base(printer, config)?;
        (Some(base), source)
    } else {
        (None, SettingsSource::DeviceDefault)
    };

    // One structure per DISTINCT setup. A mixed CAD set has two.
    let mut distinct: Vec<(SheetSetup, Vec<u32>)> = Vec::new();
    if let Some(base) = base.as_ref() {
        for setup in &resolved {
            if distinct.iter().any(|(known, _)| known == setup) {
                continue;
            }
            let mut config = base.clone();
            config.apply(
                setup.orientation,
                setup.paper,
                settings.pick_tray_by_page_size,
                setup_is_explicit(settings, *setup).then_some(settings.duplex),
            );
            distinct.push((*setup, config.to_aligned_words()));
        }
    }
    // The buffer handed to `CreateDC` must outlive the call; a pointer
    // into a dropped temporary is a dangling one and nothing in the type
    // system catches it here.
    let first_words = resolved
        .first()
        .and_then(|setup| distinct.iter().find(|(known, _)| known == setup))
        .map(|(_, words)| words);

    // SAFETY: `wide` is NUL-terminated and outlives the call, and
    // `first_words` (when present) is an ALIGNED buffer holding a
    // validated DEVMODE owned by `distinct`, which outlives it too. A
    // null DC is the documented failure and is checked rather than
    // assumed.
    let hdc = unsafe {
        CreateDCW(
            PCWSTR::null(),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            first_words.map(|w| w.as_ptr().cast::<DEVMODEW>()),
        )
    };
    if hdc.is_invalid() {
        return Err(PrintError::DeviceContext {
            printer: printer.to_owned(),
        });
    }

    // Every early return past this point must delete the DC, and a job
    // opened must be ended. Rust has no `finally`, so the work happens in
    // a closure whose result is inspected AFTER the cleanup — which is
    // the shape that makes "the error path leaked a device context" and
    // "the error path left a job in the queue" both unrepresentable
    // rather than merely avoided.
    let mut report = SpoolReport {
        pages: 0,
        printed: false,
        dpi: (caps.dpi_x as i32, caps.dpi_y as i32),
        clipped_pages: sheets.iter().filter(|s| s.bitmap.placement.clipped).count(),
        job_id: None,
        settings_source,
        sheet_setups: distinct.len().max(1),
    };

    let outcome: Result<(), PrintError> = (|| {
        if dry_run == DryRun::Yes {
            // The dry run stops HERE, after the device has been opened
            // and interrogated for real. Everything above this line is
            // the part that fails in practice.
            report.pages = sheets.len();
            return Ok(());
        }

        let doc_name: Vec<u16> = "pdfcer document\0".encode_utf16().collect();
        // `lpszOutput` redirects the job to a FILE instead of the port.
        //
        // This is what makes a `PORTPROMPT:` driver — "Microsoft Print to
        // PDF" and most PDF writers — usable without a Save dialog
        // appearing. It is both a real capability ("print to file") and
        // the only way this code path can be verified by anything other
        // than a person watching a printer.
        //
        // The buffer is bound rather than built inline because the
        // `PCWSTR` must outlive the `DOCINFOW`: a pointer into a dropped
        // temporary is a dangling one, and nothing in the type system
        // catches it here.
        let out_wide: Option<Vec<u16>> = output.map(|p| {
            p.as_os_str()
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect()
        });
        let info = DOCINFOW {
            cbSize: i32::try_from(std::mem::size_of::<DOCINFOW>()).unwrap_or(0),
            lpszDocName: PCWSTR(doc_name.as_ptr()),
            lpszOutput: out_wide
                .as_ref()
                .map_or_else(PCWSTR::null, |w| PCWSTR(w.as_ptr())),
            ..Default::default()
        };
        // SAFETY: `hdc` is valid (checked above) and `info` outlives the
        // call. A non-positive return is the documented failure.
        let job = unsafe { StartDocW(hdc, &info) };
        if job <= 0 {
            return Err(PrintError::JobStart {
                printer: printer.to_owned(),
            });
        }
        report.printed = true;
        report.job_id = u32::try_from(job).ok();

        // The setup the device is currently in. The first sheet's is
        // already in force: `CreateDC` above was given its `DEVMODE`.
        let mut current = resolved.first().copied();
        for (sheet, setup) in sheets.iter().zip(resolved.iter()) {
            if current != Some(*setup) {
                // ★ BETWEEN pages, never inside one. `ResetDC` is the
                // documented mechanism for changing a device's settings
                // mid-job, and the previous page's `EndPage` has already
                // run, so no page is open here.
                let words = distinct
                    .iter()
                    .find(|(known, _)| known == setup)
                    .map(|(_, words)| words);
                if let Some(words) = words {
                    // SAFETY: `hdc` is valid with a job open and no page
                    // open; `words` is an ALIGNED, validated DEVMODE
                    // owned by `distinct`, which outlives this call.
                    //
                    // The return is documented to be a handle to the
                    // ORIGINAL device context, so `hdc` stays the handle
                    // this function owns and deletes; a null return is
                    // the documented failure.
                    let reset = unsafe { ResetDCW(hdc, words.as_ptr().cast::<DEVMODEW>()) };
                    if reset.is_invalid() {
                        return Err(PrintError::SheetSetup);
                    }
                }
                current = Some(*setup);
            }
            // SAFETY: valid DC, and the page loop always pairs
            // StartPage with EndPage — see the abort path below for the
            // case where it cannot.
            if unsafe { StartPage(hdc) } <= 0 {
                return Err(PrintError::PageStart);
            }
            blit_page(hdc, sheet.bitmap, (caps.dpi_x as i32, caps.dpi_y as i32))?;
            if unsafe { EndPage(hdc) } <= 0 {
                return Err(PrintError::PageEnd);
            }
            report.pages += 1;
        }

        // SAFETY: valid DC with a job open.
        if unsafe { EndDoc(hdc) } <= 0 {
            return Err(PrintError::JobEnd);
        }
        Ok(())
    })();

    // A job that errored part-way is ABORTED, not left open. Windows
    // holds the device for an unfinished job, so a leaked one blocks
    // every other user of a shared printer until it times out — the
    // failure mode most likely to affect somebody who is not the
    // operator.
    if outcome.is_err() && report.printed {
        // SAFETY: valid DC with a job open. `AbortDoc` is the
        // documented cancel, and its result is deliberately ignored —
        // the error already being returned is the one that matters, and
        // a failure to abort cleanly changes nothing the caller can act
        // on.
        unsafe {
            let _ = AbortDoc(hdc);
        }
    }
    // SAFETY: valid DC, deleted exactly once on every path.
    unsafe {
        let _ = DeleteDC(hdc);
    }
    outcome.map(|()| report)
}

/// Resolve every sheet's setup, deciding [`Orientation::Auto`] against
/// that sheet's own page size.
///
/// # Why this is a separate function from the job loop
///
/// It is the whole of the per-page-orientation decision, it is pure, and
/// it is the property the `pdfcer-gui` shell reported as missing — so it
/// is tested directly rather than only through a spooler that needs
/// hardware to run. The substitution is stated rather than implied: a
/// test on this function proves that a mixed document produces two
/// different setups in the right order; that the DRIVER then honours the
/// `ResetDC` is verified end-to-end against a real device instead,
/// because no unit test can establish it.
///
/// A caller that imposed several source pages onto one sheet must
/// resolve `Auto` itself before calling: `page_pt` is then the printable
/// area rather than a source page, and this function would resolve from
/// the wrong input. [`spool_with_config`] does exactly that, which is
/// why the imposition paths keep one orientation for the whole job.
#[cfg(windows)]
fn resolve_sheet_setups(sheets: &[Sheet<'_>]) -> Vec<SheetSetup> {
    sheets
        .iter()
        .map(|sheet| SheetSetup {
            orientation: resolve_orientation(sheet.setup.orientation, sheet.bitmap.page_pt),
            paper: sheet.setup.paper,
        })
        .collect()
}

/// Does this sheet need anything said to the driver at all?
///
/// # ★ Why the test is not "did the operator change something"
///
/// It used to be, and that had a consequence nobody had traced.
/// [`Orientation::Auto`] is the DEFAULT, so at default settings a
/// landscape page never turned the sheet at all — pdfcer's headline
/// auto-orientation behaviour did nothing unless the operator happened
/// to also change the duplex or tray control, at which point it switched
/// on as a side effect of an unrelated setting. Written up as
/// `D:/dev/rag/rust/a_disturb_nothing_by_default_guard_can_silently_disable_the_default_behaviour_it_is_guarding.md`.
///
/// So the test is "will the device be in the state this sheet needs":
/// `device_default` is what [`sheet_orientation`] read off the un-turned
/// [`printer_caps`], and a `DEVMODE` is built whenever the resolved
/// orientation differs from it — or whenever any setting differs from
/// its default.
#[cfg(windows)]
fn setup_needs_devmode(
    settings: DeviceSettings,
    setup: SheetSetup,
    device_default: Orientation,
) -> bool {
    setup_is_explicit(settings, setup) || setup.orientation != device_default
}

/// Did the caller ask for anything beyond the defaults?
///
/// Gates `DM_DUPLEX` and nothing else, deliberately. A `DEVMODE` naming
/// `DMDUP_SIMPLEX` OVERRIDES a driver whose own default is duplex, so an
/// orientation-only turn — which `Auto` performs at otherwise-default
/// settings — must not quietly cancel a duplex default the operator
/// never asked pdfcer to touch.
///
/// The per-sheet paper is folded in rather than read off `settings`,
/// because [`spool_sheets`] lets a sheet carry its own and a job whose
/// only non-default request is that sheet's paper is still explicit.
#[cfg(windows)]
fn setup_is_explicit(settings: DeviceSettings, setup: SheetSetup) -> bool {
    DeviceSettings {
        paper: setup.paper,
        ..settings
    } != DeviceSettings::default()
}

/// Fetch the `DEVMODE` a job's sheets will be amended from, once.
///
/// # ★ It starts from the DRIVER's own configuration — and until
/// # 2026-08-18 the doc comment here said so while the code did not
///
/// The function this replaced carried the heading *"Why it starts from
/// the driver's own default rather than zeroed"* and the sentence *"the
/// driver's current default is fetched first and only the requested
/// fields are overwritten"*. Nothing was fetched. It built a
/// `DEVMODEW::default()` — a zeroed structure — set `dmOrientation` and
/// `dmDuplex`, and handed that to `CreateDC`. The parameter that would
/// have been needed to fetch anything was present and unused, named
/// `_printer_wide`.
///
/// A doc comment describing behaviour the code does not have is worse
/// than an undocumented function: it is a claim that reads as true, and
/// it survived review because reviewing the claim meant reading past it
/// to the eight lines underneath. It is recorded here rather than
/// quietly corrected, because the shape is the point.
///
/// What the synthesised structure cost is set out in the `devmode`
/// module's own docs; the short version is that a `DEVMODE` carries a
/// driver-private tail of `dmDriverExtra` bytes holding everything Win32
/// has no field for — 5208 bytes on Microsoft Print to PDF, 7972 on the
/// EPSON drivers, measured — a synthesised one has none of it, and there
/// was nowhere to put a paper size, a tray, or a configuration returned
/// by the driver's properties dialog.
///
/// # Errors
///
/// [`PrintError::Configuration`] when a caller-supplied configuration
/// belongs to a different device. A driver that will NOT report its own
/// settings is NOT an error here: the job falls back to a synthesised
/// base and says so through [`SettingsSource::Synthesised`], because
/// refusing to print at all would be a regression against the behaviour
/// that shipped for months.
#[cfg(windows)]
fn job_base(
    printer: &str,
    supplied: Option<&PrinterConfiguration>,
) -> Result<(PrinterConfiguration, SettingsSource), PrintError> {
    match supplied {
        Some(config) => {
            config.ensure_device(printer)?;
            Ok((config.clone(), SettingsSource::CallerSupplied))
        }
        None => match printer_configuration(printer) {
            Ok(config) => Ok((config, SettingsSource::DriverSupplied)),
            // A driver that will not describe itself — a disconnected
            // network printer is the usual cause — degrades to exactly
            // what this code did before it learned to ask: a structure
            // carrying only what pdfcer sets. The job still prints; the
            // caller is told, and tells the operator.
            Err(_) => Ok((
                PrinterConfiguration::blank(printer),
                SettingsSource::Synthesised,
            )),
        },
    }
}

/// Blit one page's pixels onto the current page of `hdc`.
///
/// # The two conversions that are easy to get wrong
///
/// **Orientation.** A `BITMAPINFOHEADER` with a POSITIVE height is
/// bottom-up: Windows reads the first row in memory as the BOTTOM of the
/// image. The caller's buffer is top-down (that is what `tiny_skia`
/// produces), so the height is negated. Get this wrong and every page
/// prints upside down — which is obvious on paper and invisible in every
/// test that does not print.
///
/// **Channel order.** `BI_RGB` at 32bpp is B, G, R, X in memory, and the
/// caller's buffer is R, G, B, A. The swap happens here rather than
/// being asked of the caller, because the caller's layout is the
/// renderer's and this crate is the one that knows what GDI wants.
///
/// Alpha is DISCARDED, not composited: a printed page has no
/// transparency, and the renderer has already composited onto white.
#[cfg(windows)]
fn blit_page(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    page: &PageBitmap,
    dpi: (i32, i32),
) -> Result<(), PrintError> {
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, SRCCOPY, StretchDIBits,
    };

    let w = i32::try_from(page.width).map_err(|_| PrintError::PageTooLarge)?;
    let h = i32::try_from(page.height).map_err(|_| PrintError::PageTooLarge)?;

    // RGBA (caller) -> BGRX (GDI).
    let mut bgra = Vec::with_capacity(page.rgba.len());
    for px in page.rgba.chunks_exact(4) {
        bgra.extend_from_slice(&[px[2], px[1], px[0], 0]);
    }

    let header = BITMAPINFOHEADER {
        biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>()).unwrap_or(40),
        biWidth: w,
        // NEGATIVE: top-down. See the fn docs.
        biHeight: -h,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let info = BITMAPINFO {
        bmiHeader: header,
        ..Default::default()
    };

    // Points -> device pixels, at the device's own resolution. 72 points
    // to the inch is the PDF unit's definition, not a convention.
    let px_x = |pt: f64| (pt * f64::from(dpi.0) / 72.0).round() as i32;
    let px_y = |pt: f64| (pt * f64::from(dpi.1) / 72.0).round() as i32;

    let dest_w = px_x(page.page_pt.0 * page.placement.scale);
    let dest_h = px_y(page.page_pt.1 * page.placement.scale);
    let dest_x = px_x(page.placement.offset_x_pt);
    let dest_y = px_y(page.placement.offset_y_pt);

    // SAFETY: `hdc` is valid with a page open; `info` and `bgra` outlive
    // the call; the dimensions are derived from the buffer itself.
    let sent = unsafe {
        StretchDIBits(
            hdc,
            dest_x,
            dest_y,
            dest_w,
            dest_h,
            0,
            0,
            w,
            h,
            Some(bgra.as_ptr().cast()),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        )
    };
    if sent == 0 {
        return Err(PrintError::Blit);
    }
    Ok(())
}

/// A stub so callers compile on non-Windows without `cfg` at every call
/// site. Printing is a Windows capability in this release.
#[cfg(not(windows))]
pub fn spool(
    _printer: &str,
    _pages: &[PageBitmap],
    _dry_run: DryRun,
    _output: Option<&std::path::Path>,
    _settings: DeviceSettings,
    _first_page_pt: (f64, f64),
) -> Result<SpoolReport, PrintError> {
    Err(PrintError::Unsupported)
}

/// Non-Windows twin of [`spool_with_config`].
///
/// # Why this exists, and why its absence was a real defect rather than a
/// tidiness complaint
///
/// **Reported by the `pdfcer-gui` session, 2026-08-18**, as a heads-up rather
/// than a blocker — and it was right that it is not a blocker and right that
/// it mattered. Every *query* added by the 2026-08-18 print filing shipped
/// with a `cfg(not(windows))` twin (`printer_forms`, `printer_configuration`,
/// `edit_printer_configuration`, `printer_caps_for`), and [`spool`] has had
/// one since it was written. **The two new spool entry points did not**, so
/// the property held everywhere except the two functions a shell actually
/// calls to print.
///
/// The consequence is specific: `pdfcer-gui` deliberately moved its single
/// spool call from [`spool`] to [`spool_with_config`] — one call site rather
/// than two branches, because *"a shell that chose between two spool
/// functions would have two paths to the one irreversible operation in the
/// application and the rarer one would be the one nobody drove."* That is a
/// good reason, and it silently made their only print path Windows-only.
///
/// This is the same shape as commit `ea5159e` earlier the same day, where
/// `cmd_print` called four `#[cfg(windows)]` callees while itself ungated:
/// **Windows stayed green while the platform CI actually builds stopped
/// compiling.** A stub is how this crate has always prevented that.
#[cfg(not(windows))]
pub fn spool_with_config(
    _printer: &str,
    _pages: &[PageBitmap],
    _dry_run: DryRun,
    _output: Option<&std::path::Path>,
    _settings: DeviceSettings,
    _first_page_pt: (f64, f64),
    _config: Option<&PrinterConfiguration>,
) -> Result<SpoolReport, PrintError> {
    Err(PrintError::Unsupported)
}

/// Non-Windows twin of [`spool_sheets`].
///
/// Same rationale as [`spool_with_config`] above — see that doc comment for
/// why the gap mattered. This is the per-sheet entry point, so a shell that
/// prints a mixed-size job reaches it rather than [`spool_with_config`], and
/// it needs the twin for the same reason.
#[cfg(not(windows))]
pub fn spool_sheets(
    _printer: &str,
    _sheets: &[Sheet<'_>],
    _dry_run: DryRun,
    _output: Option<&std::path::Path>,
    _settings: DeviceSettings,
    _config: Option<&PrinterConfiguration>,
) -> Result<SpoolReport, PrintError> {
    Err(PrintError::Unsupported)
}

/// The Windows-only half of the settings path, tested WITHOUT a device.
///
/// # Why these tests are worth having even though a printer is not
///
/// Everything that decides WHAT to send is reachable without opening a
/// device: resolving each sheet's setup, deciding whether a `DEVMODE` is
/// needed at all, and amending one are pure. Only the fetch and the
/// `ResetDC` need hardware, and those are verified end-to-end against a
/// real device instead — the substitution is stated here rather than
/// left implicit, because a reader is entitled to know which half a
/// green suite covers.
///
/// The alternative — a test that names a printer and skips when it is
/// absent — reports success while testing nothing, which is the failure
/// mode this project has caught repeatedly.
#[cfg(all(test, windows))]
mod windows_settings_tests {
    use super::{
        DMBIN_FORMSOURCE_VALUE, DeviceSettings, Duplex, Orientation, PageBitmap, PaperSelection,
        Placement, PrintError, PrinterConfiguration, SettingsSource, Sheet, SheetSetup, job_base,
        resolve_sheet_setups, setup_is_explicit, setup_needs_devmode,
    };

    /// A4 portrait, in points.
    const A4: (f64, f64) = (595.0, 842.0);
    /// A3 landscape — the drawing sheet behind the title page.
    const A3_LANDSCAPE: (f64, f64) = (1190.0, 842.0);

    /// A one-pixel stand-in. The pixels are irrelevant to every decision
    /// under test; the `page_pt` is not, and that is the point.
    fn bitmap(page_pt: (f64, f64)) -> PageBitmap {
        PageBitmap {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
            placement: Placement {
                scale: 1.0,
                offset_x_pt: 0.0,
                offset_y_pt: 0.0,
                clipped: false,
            },
            page_pt,
        }
    }

    /// ★ The property the `pdfcer-gui` shell reported as missing.
    ///
    /// A CAD export — an A4 portrait title sheet followed by A3
    /// landscape drawings — must resolve to BOTH orientations, in order.
    /// Before 2026-08-18 every sheet took page 1's answer.
    #[test]
    fn auto_resolves_orientation_per_sheet_not_once_for_the_job() {
        let title = bitmap(A4);
        let drawing = bitmap(A3_LANDSCAPE);
        let setup = SheetSetup::default();
        let sheets = [
            Sheet {
                bitmap: &title,
                setup,
            },
            Sheet {
                bitmap: &drawing,
                setup,
            },
            Sheet {
                bitmap: &drawing,
                setup,
            },
        ];
        let resolved = resolve_sheet_setups(&sheets);
        assert_eq!(
            resolved
                .iter()
                .map(|s| s.orientation)
                .collect::<Vec<Orientation>>(),
            vec![
                Orientation::Portrait,
                Orientation::Landscape,
                Orientation::Landscape
            ],
            "Auto must read each sheet's own shape"
        );
        // And the run is collapsed: two distinct setups, not three.
        let mut distinct: Vec<SheetSetup> = Vec::new();
        for setup in &resolved {
            if !distinct.contains(setup) {
                distinct.push(*setup);
            }
        }
        assert_eq!(distinct.len(), 2, "one ResetDC, not two");
    }

    /// An explicit orientation is honoured verbatim on every sheet — a
    /// landscape page does NOT force a turn when the operator said
    /// portrait.
    #[test]
    fn an_explicit_orientation_is_not_second_guessed_per_sheet() {
        let drawing = bitmap(A3_LANDSCAPE);
        let sheets = [Sheet {
            bitmap: &drawing,
            setup: SheetSetup {
                orientation: Orientation::Portrait,
                paper: PaperSelection::DeviceDefault,
            },
        }];
        assert_eq!(
            resolve_sheet_setups(&sheets)[0].orientation,
            Orientation::Portrait
        );
    }

    /// ★ The regression guard for the defect written up as
    /// `a_disturb_nothing_by_default_guard_can_silently_disable_the_default_behaviour_it_is_guarding.md`:
    /// at DEFAULT settings, a landscape sheet on a portrait-default
    /// device still needs a `DEVMODE`, because `Auto` resolving to
    /// landscape IS a change even though nothing was "set".
    #[test]
    fn a_landscape_sheet_needs_a_devmode_even_at_default_settings() {
        let landscape = SheetSetup {
            orientation: Orientation::Landscape,
            paper: PaperSelection::DeviceDefault,
        };
        let portrait = SheetSetup::default();
        assert!(setup_needs_devmode(
            DeviceSettings::default(),
            landscape,
            Orientation::Portrait
        ));
        // …and the genuinely-nothing-to-do case still says nothing.
        assert!(!setup_needs_devmode(
            DeviceSettings::default(),
            SheetSetup {
                orientation: Orientation::Portrait,
                ..portrait
            },
            Orientation::Portrait
        ));
        // A landscape-DEFAULT device is the mirror image, and gets the
        // opposite answers. A test written only on a portrait device
        // would pass with the comparison hard-coded to portrait.
        assert!(!setup_needs_devmode(
            DeviceSettings::default(),
            landscape,
            Orientation::Landscape
        ));
        assert!(setup_needs_devmode(
            DeviceSettings::default(),
            portrait,
            Orientation::Landscape
        ));
    }

    /// `DM_DUPLEX` is gated on something actually differing, so an
    /// `Auto` turn cannot cancel a driver's own duplex default.
    #[test]
    fn an_orientation_only_turn_is_not_explicit() {
        assert!(!setup_is_explicit(
            DeviceSettings::default(),
            SheetSetup {
                orientation: Orientation::Landscape,
                paper: PaperSelection::DeviceDefault,
            }
        ));
        // A per-SHEET paper selection makes it explicit even though
        // `settings` is untouched — the case a whole-struct comparison
        // against `settings` alone would miss.
        assert!(setup_is_explicit(
            DeviceSettings::default(),
            SheetSetup {
                orientation: Orientation::Portrait,
                paper: PaperSelection::Form(9),
            }
        ));
        assert!(setup_is_explicit(
            DeviceSettings {
                duplex: Duplex::LongEdge,
                ..DeviceSettings::default()
            },
            SheetSetup::default()
        ));
    }

    /// A caller-supplied configuration is used as the base and reported
    /// as the caller's. Reached with a printer name that does not exist,
    /// which PROVES no driver call happened on this path.
    #[test]
    fn a_caller_supplied_configuration_is_the_base_and_is_reported_as_such() {
        let supplied = PrinterConfiguration::blank("phantom");
        let (base, source) =
            job_base("phantom", Some(&supplied)).expect("a supplied configuration needs no device");
        assert_eq!(source, SettingsSource::CallerSupplied);
        assert_eq!(base, supplied);
    }

    /// A configuration from a different device is refused rather than
    /// sent: its private tail is another driver's private format.
    #[test]
    fn a_configuration_from_another_device_is_refused() {
        let foreign = PrinterConfiguration::blank("some other printer");
        let err = job_base("phantom", Some(&foreign)).expect_err("a foreign one must be refused");
        assert!(matches!(err, PrintError::Configuration(_)));
    }

    /// The amend a sheet actually gets: orientation and paper from the
    /// SHEET, tray from the job.
    #[test]
    fn a_sheet_configuration_carries_the_sheets_own_orientation_and_paper() {
        let settings = DeviceSettings {
            pick_tray_by_page_size: true,
            ..DeviceSettings::default()
        };
        let setup = SheetSetup {
            orientation: Orientation::Landscape,
            paper: PaperSelection::Form(8),
        };
        let mut config = PrinterConfiguration::blank("phantom");
        config.apply(
            setup.orientation,
            setup.paper,
            settings.pick_tray_by_page_size,
            setup_is_explicit(settings, setup).then_some(settings.duplex),
        );
        let summary = config.summary();
        assert_eq!(summary.orientation, Some(Orientation::Landscape));
        assert_eq!(summary.paper_form_id, Some(8));
        assert!(summary.picks_tray_by_size);
    }

    /// `DMBIN_FORMSOURCE` is written into a signed member and compared
    /// against an unsigned capability array, so it exists twice in this
    /// crate. The `devmode` module asserts its copy against the real
    /// constant; this asserts the other one against the same source.
    #[test]
    fn dmbin_formsource_agrees_across_its_two_representations() {
        use windows::Win32::Graphics::Gdi::DMBIN_FORMSOURCE;
        assert_eq!(u32::from(DMBIN_FORMSOURCE_VALUE), DMBIN_FORMSOURCE);
    }
}
