//! Finishing through a Windows driver: its PrintCapabilities say what it
//! offers, and a delta PrintTicket merged onto the job's `DEVMODE` asks for
//! it (`prntvpt`, Print Schema). Off Windows both calls are
//! [`PrintError::Unsupported`].

use super::Finishing;
use super::print_schema::Capabilities;
use crate::{PrintError, PrinterConfiguration};

/// The staple, punch and output-bin options `printer`'s driver offers.
///
/// # Errors
///
/// [`PrintError::PrintTicket`] when the driver's PrintTicket provider will
/// not open or answer, or answers with XML that does not parse.
pub fn printer_finishing(printer: &str) -> Result<Capabilities, PrintError> {
    imp::printer_finishing(printer)
}

/// `base` with `f` merged in by `printer`'s driver.
///
/// The merged ticket is re-read: a driver that substituted a requested
/// option is reported as refusing it, never trusted to have applied it.
///
/// # Errors
///
/// [`PrintError::FinishingRefused`] naming every request the driver does
/// not offer or the Print Schema cannot express (fold types, hole counts);
/// [`PrintError::PrintTicket`] when the provider fails;
/// [`PrintError::Configuration`] when the driver returns an unusable
/// `DEVMODE`.
pub fn apply_finishing(
    printer: &str,
    base: &PrinterConfiguration,
    f: &Finishing,
) -> Result<PrinterConfiguration, PrintError> {
    imp::apply_finishing(printer, base, f)
}

#[cfg(not(windows))]
mod imp {
    use super::{Capabilities, Finishing, PrintError, PrinterConfiguration};

    /// See [`super::printer_finishing`].
    pub(super) fn printer_finishing(_printer: &str) -> Result<Capabilities, PrintError> {
        Err(PrintError::Unsupported)
    }

    /// See [`super::apply_finishing`].
    pub(super) fn apply_finishing(
        _printer: &str,
        _base: &PrinterConfiguration,
        _f: &Finishing,
    ) -> Result<PrinterConfiguration, PrintError> {
        Err(PrintError::Unsupported)
    }
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::Graphics::Gdi::DEVMODEA;
    use windows::Win32::Graphics::Printing::PrintTicket::{
        HPTPROVIDER, PTCloseProvider, PTConvertDevModeToPrintTicket, PTConvertPrintTicketToDevMode,
        PTGetPrintCapabilities, PTMergeAndValidatePrintTicket, PTOpenProvider, PTReleaseMemory,
        kPTJobScope, kUserDefaultDevmode,
    };
    use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize, IStream, STATFLAG_NONAME,
        STATSTG, STREAM_SEEK_SET,
    };
    use windows::core::{BSTR, HSTRING};

    use super::super::print_schema::{delta_ticket, parse_capabilities};
    use super::{Capabilities, Finishing, PrintError, PrinterConfiguration};

    /// A capabilities document or ticket larger than this is refused: the
    /// driver's output is untrusted input (ARCHITECTURE.md §10).
    const XML_CEILING: u64 = 16 * 1024 * 1024;

    /// COM for this thread, released on drop when this call initialised it.
    struct Com(bool);

    impl Com {
        fn init() -> Self {
            // SAFETY: no reserved pointer; a thread already in another
            // apartment returns an error, which leaves it as it was.
            Self(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
        }
    }

    impl Drop for Com {
        fn drop(&mut self) {
            if self.0 {
                // SAFETY: balances the successful CoInitializeEx above.
                unsafe { CoUninitialize() };
            }
        }
    }

    /// An open provider. Thread-bound, so it never leaves the call that
    /// opened it.
    struct Provider<'a> {
        handle: HPTPROVIDER,
        printer: &'a str,
    }

    impl<'a> Provider<'a> {
        fn open(printer: &'a str) -> Result<Self, PrintError> {
            let name = HSTRING::from(printer);
            // SAFETY: `name` is NUL-terminated and outlives the call.
            let handle = unsafe { PTOpenProvider(&name, 1) }.map_err(|e| fail(printer, &e))?;
            Ok(Self { handle, printer })
        }

        fn err(&self, e: &windows::core::Error, msg: &BSTR) -> PrintError {
            let detail = if msg.is_empty() {
                e.message()
            } else {
                msg.to_string()
            };
            PrintError::PrintTicket {
                printer: self.printer.to_owned(),
                detail,
            }
        }

        fn capabilities(&self) -> Result<String, PrintError> {
            let caps = new_stream().map_err(|e| fail(self.printer, &e))?;
            let mut msg = BSTR::new();
            // SAFETY: the handle is open on this thread; `caps` is a live
            // stream the provider writes into.
            unsafe { PTGetPrintCapabilities(self.handle, None::<&IStream>, &caps, Some(&mut msg)) }
                .map_err(|e| self.err(&e, &msg))?;
            read_text(&caps).map_err(|e| fail(self.printer, &e))
        }

        fn ticket_from(&self, base: &PrinterConfiguration) -> Result<IStream, PrintError> {
            let words = base.to_aligned_words();
            let len = u32::try_from(base.as_bytes().len()).unwrap_or(u32::MAX);
            let ticket = new_stream().map_err(|e| fail(self.printer, &e))?;
            // SAFETY: `words` holds a validated DEVMODEW of `len` bytes,
            // aligned; the binding names it DEVMODEA, the API reads DEVMODEW.
            unsafe {
                PTConvertDevModeToPrintTicket(
                    self.handle,
                    len,
                    words.as_ptr().cast::<DEVMODEA>(),
                    kPTJobScope,
                    &ticket,
                )
            }
            .map_err(|e| fail(self.printer, &e))?;
            rewind(&ticket).map_err(|e| fail(self.printer, &e))?;
            Ok(ticket)
        }

        fn merge(&self, base: &IStream, delta: &str) -> Result<IStream, PrintError> {
            let delta = stream_from(delta.as_bytes()).map_err(|e| fail(self.printer, &e))?;
            let merged = new_stream().map_err(|e| fail(self.printer, &e))?;
            let mut msg = BSTR::new();
            // SAFETY: all three streams are live and rewound; the handle is
            // open on this thread.
            unsafe {
                PTMergeAndValidatePrintTicket(
                    self.handle,
                    base,
                    &delta,
                    kPTJobScope,
                    &merged,
                    Some(&mut msg),
                )
            }
            .map_err(|e| self.err(&e, &msg))?;
            rewind(&merged).map_err(|e| fail(self.printer, &e))?;
            Ok(merged)
        }

        fn devmode(&self, ticket: &IStream) -> Result<PrinterConfiguration, PrintError> {
            let mut len = 0u32;
            let mut ptr: *mut DEVMODEA = core::ptr::null_mut();
            let mut msg = BSTR::new();
            // SAFETY: the ticket stream is rewound; on success the provider
            // hands back `len` bytes it owns, released below.
            unsafe {
                PTConvertPrintTicketToDevMode(
                    self.handle,
                    ticket,
                    kUserDefaultDevmode,
                    kPTJobScope,
                    &mut len,
                    &mut ptr,
                    Some(&mut msg),
                )
            }
            .map_err(|e| self.err(&e, &msg))?;
            if ptr.is_null() {
                return Err(PrintError::DriverSettings {
                    printer: self.printer.to_owned(),
                });
            }
            let n = usize::try_from(len).unwrap_or(0);
            // SAFETY: the provider returned `len` readable bytes at `ptr`
            // (a DEVMODEW plus its private tail); copied before release.
            let bytes = unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), n) }.to_vec();
            // SAFETY: `ptr` came from PTConvertPrintTicketToDevMode.
            let _ = unsafe { PTReleaseMemory(ptr.cast::<core::ffi::c_void>()) };
            Ok(PrinterConfiguration::from_bytes(&bytes)?)
        }
    }

    impl Drop for Provider<'_> {
        fn drop(&mut self) {
            // SAFETY: the handle was opened by PTOpenProvider on this thread.
            let _ = unsafe { PTCloseProvider(self.handle) };
        }
    }

    fn fail(printer: &str, e: &windows::core::Error) -> PrintError {
        PrintError::PrintTicket {
            printer: printer.to_owned(),
            detail: e.message(),
        }
    }

    fn new_stream() -> windows::core::Result<IStream> {
        // SAFETY: a null HGLOBAL asks the call to allocate; it frees on release.
        unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) }
    }

    fn rewind(s: &IStream) -> windows::core::Result<()> {
        // SAFETY: a live stream; seeking to its start.
        unsafe { s.Seek(0, STREAM_SEEK_SET, None) }
    }

    fn stream_from(bytes: &[u8]) -> windows::core::Result<IStream> {
        let s = new_stream()?;
        let len = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
        // SAFETY: `bytes` is readable for `len` bytes and outlives the call.
        unsafe { s.Write(bytes.as_ptr().cast(), len, None) }.ok()?;
        rewind(&s)?;
        Ok(s)
    }

    fn read_text(s: &IStream) -> windows::core::Result<String> {
        let mut stat = STATSTG::default();
        // SAFETY: `stat` is a valid out-pointer; NONAME allocates no name.
        unsafe { s.Stat(&mut stat, STATFLAG_NONAME) }?;
        let size = stat.cbSize.min(XML_CEILING);
        let mut buf = vec![0u8; usize::try_from(size).unwrap_or(0)];
        rewind(s)?;
        let mut read = 0u32;
        let want = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        // SAFETY: `buf` is writable for `want` bytes.
        unsafe { s.Read(buf.as_mut_ptr().cast(), want, Some(&mut read)) }.ok()?;
        buf.truncate(usize::try_from(read).unwrap_or(0));
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn parsed(printer: &str, xml: &str) -> Result<Capabilities, PrintError> {
        parse_capabilities(xml).map_err(|detail| PrintError::PrintTicket {
            printer: printer.to_owned(),
            detail,
        })
    }

    /// See [`super::printer_finishing`].
    pub(super) fn printer_finishing(printer: &str) -> Result<Capabilities, PrintError> {
        let _com = Com::init();
        let provider = Provider::open(printer)?;
        parsed(printer, &provider.capabilities()?)
    }

    /// See [`super::apply_finishing`].
    pub(super) fn apply_finishing(
        printer: &str,
        base: &PrinterConfiguration,
        f: &Finishing,
    ) -> Result<PrinterConfiguration, PrintError> {
        let _com = Com::init();
        let provider = Provider::open(printer)?;
        let caps = parsed(printer, &provider.capabilities()?)?;
        let refused = |refused| PrintError::FinishingRefused {
            printer: printer.to_owned(),
            refused,
        };
        let Some(delta) = delta_ticket(f, &caps).map_err(refused)? else {
            return Ok(base.clone());
        };
        let merged = provider.merge(&provider.ticket_from(base)?, &delta)?;
        let kept = parsed(printer, &read_text(&merged).map_err(|e| fail(printer, &e))?)?;
        let dropped = dropped(f, &kept);
        if !dropped.is_empty() {
            return Err(refused(dropped));
        }
        rewind(&merged).map_err(|e| fail(printer, &e))?;
        provider.devmode(&merged)
    }

    /// The requests the merged ticket no longer carries: a driver resolves a
    /// conflict by substituting, and reports success when it does.
    fn dropped(f: &Finishing, kept: &Capabilities) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(s) = f.staple.filter(|s| !kept.staples.contains(s)) {
            out.push(format!(
                "staple {s:?}: the driver substituted another setting"
            ));
        }
        if let Some(e) = f.punch.filter(|e| !kept.punch.contains(e)) {
            out.push(format!(
                "punch {e:?}: the driver substituted another setting"
            ));
        }
        if let Some(b) = &f.output_bin
            && !kept.bins.iter().any(|k| k.name.eq_ignore_ascii_case(b))
        {
            out.push(format!(
                "output bin {b:?}: the driver substituted another bin"
            ));
        }
        out
    }
}
