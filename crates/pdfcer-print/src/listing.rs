//! Enumerating printers: the full listing, the name-only listing that never
//! reaches a device, and the per-printer detail read.
//!
//! # Which call can block
//!
//! `EnumPrinters` level 4 (`PRINTER_INFO_4`: name, server, attributes) is
//! answered from the spooler's registry and never contacts a device, so
//! [`list_printer_names`] returns promptly even when an installed printer is
//! unreachable. Level 2 (`PRINTER_INFO_2`) asks for per-device detail, and
//! for an unreachable WSD-port printer that Windows still reports as ready
//! the spooler waits about 45–50 s per device before answering. So
//! [`list_printers`] and [`printer_detail`] can block, as can
//! `device_features`, `printer_forms` and `printer_caps`; a GUI calls them off
//! its UI thread.

use super::{PrintError, Printer};

/// A printer's name and whether it is the system default, as
/// [`list_printer_names`] reports it.
///
/// Not `cfg(windows)`, for the same reason as [`Printer`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PrinterName {
    /// The printer's name, as the spooler reports it; what `--printer`
    /// and every per-printer read take.
    pub name: String,
    /// Whether this is the system default.
    pub is_default: bool,
}

/// List the printers this machine can reach, with driver and port.
///
/// `PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS`: `LOCAL` alone misses the
/// network printers the user has connected to, which on a workstation are
/// usually most of them.
///
/// **Can block** for tens of seconds per unreachable printer (see the module
/// docs). An interactive caller wants [`list_printer_names`] and
/// [`printer_detail`] on a worker thread instead.
///
/// # Errors
///
/// [`PrintError::Enumerate`] when the spooler cannot be queried at all. An
/// empty list is not an error: a machine with no printers is a normal machine.
#[cfg(windows)]
pub fn list_printers() -> Result<Vec<Printer>, PrintError> {
    use windows::Win32::Graphics::Printing::PRINTER_INFO_2W;

    let (buffer, returned) = enum_printers(2)?;
    let default_name = default_printer_name();
    // SAFETY: the spooler wrote `returned` contiguous `PRINTER_INFO_2W`
    // records at the head of `buffer` (8-byte aligned); their string
    // pointers point into the same allocation, which outlives this loop.
    let infos =
        unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<PRINTER_INFO_2W>(), returned) };
    Ok(infos
        .iter()
        .filter_map(|info| {
            // SAFETY: NUL-terminated UTF-16 strings inside `buffer`.
            let name = unsafe { super::pwstr_to_string(info.pPrinterName) };
            // A nameless printer cannot be targeted, so listing it would
            // offer something unusable (R83).
            (!name.is_empty()).then(|| Printer {
                is_default: !default_name.is_empty() && name == default_name,
                driver: unsafe { super::pwstr_to_string(info.pDriverName) },
                port: unsafe { super::pwstr_to_string(info.pPortName) },
                name,
            })
        })
        .collect())
}

/// List the printers this machine can reach, by name only, without
/// contacting any device (`EnumPrinters` level 4; see the module docs).
///
/// Same set and order as [`list_printers`]; the default is flagged from
/// `GetDefaultPrinter`. Driver and port come from [`printer_detail`].
///
/// # Errors
///
/// [`PrintError::Enumerate`] when the spooler cannot be queried at all. An
/// empty list is not an error.
#[cfg(windows)]
pub fn list_printer_names() -> Result<Vec<PrinterName>, PrintError> {
    use windows::Win32::Graphics::Printing::PRINTER_INFO_4W;

    let (buffer, returned) = enum_printers(4)?;
    let default_name = default_printer_name();
    // SAFETY: as in `list_printers`, for `PRINTER_INFO_4W` records.
    let infos =
        unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<PRINTER_INFO_4W>(), returned) };
    Ok(infos
        .iter()
        .filter_map(|info| {
            // SAFETY: a NUL-terminated UTF-16 string inside `buffer`.
            let name = unsafe { super::pwstr_to_string(info.pPrinterName) };
            (!name.is_empty()).then(|| PrinterName {
                is_default: !default_name.is_empty() && name == default_name,
                name,
            })
        })
        .collect())
}

/// One printer's driver and port (`GetPrinter` level 2), as a [`Printer`].
///
/// **Can block** for tens of seconds when the printer is unreachable (see
/// the module docs); call it off a UI thread.
///
/// # Errors
///
/// [`PrintError::OpenDevice`] if the name does not resolve;
/// [`PrintError::Enumerate`] if the spooler will not describe it.
#[cfg(windows)]
pub fn printer_detail(printer: &str) -> Result<Printer, PrintError> {
    use windows::Win32::Graphics::Printing::{GetPrinterW, PRINTER_INFO_2W};

    let default_name = default_printer_name();
    super::with_printer_handle(printer, |handle| {
        let mut needed: u32 = 0;
        // SAFETY: the size query of the documented two-call pattern; it is
        // expected to fail and write the byte count into `needed`.
        let _ = unsafe { GetPrinterW(handle, 2, None, &raw mut needed) };
        if needed == 0 {
            return Err(PrintError::Enumerate(0));
        }
        let mut buffer = aligned_buffer(needed);
        // SAFETY: `buffer` holds at least `needed` bytes.
        unsafe {
            GetPrinterW(
                handle,
                2,
                Some(as_bytes(&mut buffer, needed)),
                &raw mut needed,
            )
        }
        .map_err(|e| PrintError::Enumerate(e.code().0.unsigned_abs()))?;
        // SAFETY: the spooler wrote one `PRINTER_INFO_2W` at the head of
        // the 8-byte-aligned `buffer`; its strings point into it.
        let info = unsafe { &*buffer.as_ptr().cast::<PRINTER_INFO_2W>() };
        // SAFETY: NUL-terminated UTF-16 strings inside `buffer`.
        let name = unsafe { super::pwstr_to_string(info.pPrinterName) };
        let name = if name.is_empty() {
            printer.to_owned()
        } else {
            name
        };
        Ok(Printer {
            is_default: !default_name.is_empty() && name == default_name,
            driver: unsafe { super::pwstr_to_string(info.pDriverName) },
            port: unsafe { super::pwstr_to_string(info.pPortName) },
            name,
        })
    })?
}

/// Run `EnumPrinters` at `level` and return the filled buffer and the
/// record count. The first call is the size query of the documented
/// two-call pattern and is expected to fail.
#[cfg(windows)]
fn enum_printers(level: u32) -> Result<(Vec<u64>, usize), PrintError> {
    use windows::Win32::Graphics::Printing::{
        EnumPrintersW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL,
    };

    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let mut needed: u32 = 0;
    let mut returned: u32 = 0;
    // SAFETY: a size query with no buffer; it writes only the two counts.
    let _ = unsafe { EnumPrintersW(flags, None, level, None, &mut needed, &mut returned) };
    if needed == 0 {
        return Ok((Vec::new(), 0));
    }
    let mut buffer = aligned_buffer(needed);
    // SAFETY: `buffer` holds at least `needed` bytes, the size just reported.
    unsafe {
        EnumPrintersW(
            flags,
            None,
            level,
            Some(as_bytes(&mut buffer, needed)),
            &mut needed,
            &mut returned,
        )
    }
    .map_err(|e| PrintError::Enumerate(e.code().0.unsigned_abs()))?;
    Ok((buffer, returned as usize))
}

/// A zeroed buffer of at least `bytes` bytes, aligned for the pointer-bearing
/// `PRINTER_INFO_*` records the spooler writes at its head.
#[cfg(windows)]
fn aligned_buffer(bytes: u32) -> Vec<u64> {
    vec![0u64; (bytes as usize).div_ceil(8)]
}

/// The first `bytes` bytes of `buffer`, for the spooler to write into.
#[cfg(windows)]
fn as_bytes(buffer: &mut [u64], bytes: u32) -> &mut [u8] {
    let len = (bytes as usize).min(buffer.len() * 8);
    // SAFETY: `len` bytes lie within `buffer`'s allocation, and any byte
    // pattern is a valid `u8` and a valid `u64`.
    unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<u8>(), len) }
}

/// The default printer's name, or empty when there is none or it cannot be
/// read: not knowing the default is a smaller loss than failing the listing.
#[cfg(windows)]
fn default_printer_name() -> String {
    use windows::Win32::Graphics::Printing::GetDefaultPrinterW;

    let mut len: u32 = 0;
    // SAFETY: the size query; it writes only `len`.
    unsafe {
        let _ = GetDefaultPrinterW(None, &mut len);
    }
    if len == 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize];
    // SAFETY: `buf` holds `len` UTF-16 units, as just requested.
    let ok = unsafe { GetDefaultPrinterW(Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len) }
        .as_bool();
    if ok {
        super::utf16_to_string(&buf)
    } else {
        String::new()
    }
}

/// Non-Windows stub; see the Windows [`list_printers`] for why this is an
/// error rather than an empty list.
///
/// # Errors
///
/// Always [`PrintError::Unsupported`].
#[cfg(not(windows))]
pub fn list_printers() -> Result<Vec<Printer>, PrintError> {
    Err(PrintError::Unsupported)
}

/// Non-Windows stub.
///
/// # Errors
///
/// Always [`PrintError::Unsupported`].
#[cfg(not(windows))]
pub fn list_printer_names() -> Result<Vec<PrinterName>, PrintError> {
    Err(PrintError::Unsupported)
}

/// Non-Windows stub.
///
/// # Errors
///
/// Always [`PrintError::Unsupported`].
#[cfg(not(windows))]
pub fn printer_detail(_printer: &str) -> Result<Printer, PrintError> {
    Err(PrintError::Unsupported)
}

#[cfg(all(test, windows))]
// Panicking on a spooler failure is the test failing.
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    /// The name-only listing names the same printers, in the same order,
    /// with the same default, as the full one; and each one's detail agrees
    /// with its full-listing row. Holds on a machine with no printers too.
    #[test]
    fn name_listing_and_detail_agree_with_the_full_listing() {
        let full = list_printers().expect("spooler");
        let names = list_printer_names().expect("spooler");
        let expected: Vec<PrinterName> = full
            .iter()
            .map(|p| PrinterName {
                name: p.name.clone(),
                is_default: p.is_default,
            })
            .collect();
        assert_eq!(names, expected);
        // A detail read of a network or WSD printer can block for a minute.
        let local = |p: &&Printer| !p.name.starts_with(r"\\") && !p.port.starts_with("WSD");
        for p in full.iter().filter(local).take(3) {
            assert_eq!(&printer_detail(&p.name).expect("detail"), p);
        }
    }

    #[test]
    fn detail_of_an_unknown_printer_is_open_device() {
        let name = "pdfcer no such printer 7f3a";
        assert!(matches!(printer_detail(name), Err(PrintError::OpenDevice(n)) if n == name));
    }
}
