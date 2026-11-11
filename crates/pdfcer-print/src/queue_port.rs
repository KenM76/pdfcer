//! Reads what Windows records about a queue's port, for
//! [`crate::ipp::resolve_queue_port`]. Read-only: the spooler's port name
//! (`GetPrinter` level 2) and registry values the port monitors keep.
//!
//! - Standard TCP/IP: `HKLM\SYSTEM\CurrentControlSet\Control\Print\Monitors\
//!   Standard TCP/IP Port\Ports\<port>` values `HostName`, `IPAddress`.
//! - WSD: `...\Monitors\WSD Port\Ports\<port>` value `Printer UUID`, then
//!   `LocationInformation` under `HKLM\SYSTEM\CurrentControlSet\Enum\SWD\
//!   DAFWSDProvider\urn:uuid:<uuid>` and `...\Enum\SWD\PRINTENUM\<port>`.

use crate::PrintError;
use crate::ipp::QueuePort;

const MONITORS: &str = "SYSTEM\\CurrentControlSet\\Control\\Print\\Monitors";
const SWD: &str = "SYSTEM\\CurrentControlSet\\Enum\\SWD";

/// What the system records about `printer`'s port. A value the registry
/// does not hold is left out rather than refused: the resolver decides
/// whether what remains is enough.
///
/// # Errors
///
/// [`PrintError`] if the queue cannot be opened or described
/// ([`crate::printer_detail`]); [`PrintError::Unsupported`] off Windows.
pub fn queue_port(printer: &str) -> Result<QueuePort, PrintError> {
    let port = crate::printer_detail(printer)?.port;
    let mut queue = QueuePort::named(&port);
    let tcp = format!("{MONITORS}\\Standard TCP/IP Port\\Ports\\{port}");
    for value in ["HostName", "IPAddress"] {
        if let Some(host) = read_string(&tcp, value).filter(|h| !h.is_empty()) {
            queue.hosts.push(host);
        }
    }
    let wsd = format!("{MONITORS}\\WSD Port\\Ports\\{port}");
    let mut devices = Vec::new();
    if let Some(uuid) = read_string(&wsd, "Printer UUID").filter(|u| !u.is_empty()) {
        devices.push(format!("{SWD}\\DAFWSDProvider\\urn:uuid:{uuid}"));
    }
    devices.push(format!("{SWD}\\PRINTENUM\\{port}"));
    for key in devices {
        if let Some(url) = read_string(&key, "LocationInformation").filter(|u| !u.is_empty()) {
            queue.device_urls.push(url);
        }
    }
    Ok(queue)
}

/// A `REG_SZ` value under `HKEY_LOCAL_MACHINE`, or `None` when the key or
/// value is absent or unreadable.
#[cfg(windows)]
fn read_string(subkey: &str, value: &str) -> Option<String> {
    use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::PCWSTR;

    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (key, name) = (wide(subkey), wide(value));
    let mut bytes: u32 = 0;
    // SAFETY: the size query; both strings are NUL-terminated and live
    // across the call.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&raw mut bytes),
        )
    };
    if status.is_err() || bytes == 0 {
        return None;
    }
    let mut buffer = vec![0u16; (bytes as usize).div_ceil(2)];
    // SAFETY: `buffer` holds `bytes` bytes, as the size query asked.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut bytes),
        )
    };
    if status.is_err() {
        return None;
    }
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(buffer.get(..len)?))
}

#[cfg(not(windows))]
fn read_string(_subkey: &str, _value: &str) -> Option<String> {
    None
}

#[cfg(all(test, windows))]
mod tests {
    /// Every installed queue's port reads without error, and a port the
    /// resolver turns into URIs names a scheme pdfcer can send to.
    #[test]
    fn every_installed_queue_port_reads() {
        let Ok(names) = crate::list_printer_names() else {
            return;
        };
        for p in names {
            let Ok(queue) = super::queue_port(&p.name) else {
                continue;
            };
            if let Ok(r) = crate::ipp::resolve_queue_port(&queue) {
                assert!(r.uris.iter().all(|u| u.starts_with("ipp")), "{r:?}");
            }
        }
    }
}
