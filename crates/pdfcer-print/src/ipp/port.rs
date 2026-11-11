//! Where an installed print queue's port sends its bytes, as IPP printer
//! URIs worth asking (`pdfcer print --pdf-direct`). Pure: the platform
//! reader (`queue_port`) gathers what the system records about the port,
//! and [`resolve_queue_port`] turns that into candidates, so every rule
//! here is tested on every target.
//!
//! A network printer that reads PDF almost always serves IPP at
//! `ipp://host/ipp/print` (PWG 5100.14 §4.1, IPP Everywhere), whatever
//! protocol its Windows port speaks (raw 9100 for a Standard TCP/IP port,
//! WS-Print for a WSD port); so a host is turned into that URI and the
//! printer's own Get-Printer-Attributes answer decides the rest.

use std::net::{Ipv4Addr, Ipv6Addr};

/// What the system records about one queue's port.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct QueuePort {
    /// The port's name (`PRINTER_INFO_2.pPortName`): `192.168.1.20`,
    /// `WSD-<guid>`, `LPT1:`, `http://host:631/printers/q`, ...
    pub port: String,
    /// Hosts a Standard TCP/IP port states (`HostName`, `IPAddress`).
    pub hosts: Vec<String>,
    /// Device addresses a WSD port's device states
    /// (`LocationInformation`, e.g. `http://192.168.0.90:80/wsd/mex`).
    pub device_urls: Vec<String>,
}

impl QueuePort {
    /// A port known only by its name.
    pub fn named(port: &str) -> Self {
        Self {
            port: port.to_owned(),
            ..Self::default()
        }
    }
}

/// Which record a resolution came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PortSource {
    /// The port's name is itself a printer URI.
    PortUri,
    /// A Standard TCP/IP port's stated host.
    TcpIpHost,
    /// A WSD port's device address.
    WsdDevice,
    /// The port's name is an IPv4 address (`10.0.0.5`, `IP_10.0.0.5`).
    PortName,
}

impl PortSource {
    /// Plain words, for a disclosure line.
    pub fn describe(self) -> &'static str {
        match self {
            Self::PortUri => "the port's printer address",
            Self::TcpIpHost => "the TCP/IP port's host",
            Self::WsdDevice => "the WSD port's device address",
            Self::PortName => "the port's name",
        }
    }
}

/// Printer URIs to ask, most likely first.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PortResolution {
    /// `ipp://` / `ipps://` URIs, deduplicated, IPv4 before names before
    /// IPv6.
    pub uris: Vec<String>,
    /// Where they came from.
    pub source: PortSource,
}

/// A port with no network address pdfcer can read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("port {port} {reason}")]
#[non_exhaustive]
pub struct NoNetworkAddress {
    /// The port's name.
    pub port: String,
    /// Why, in plain words, completing "port X ...".
    pub reason: &'static str,
}

/// The IPP URIs worth asking for `queue`'s printer.
///
/// In order: a port name that is a printer URI (`http://` and `https://`
/// become `ipp://` and `ipps://`, keeping the stated port); a TCP/IP port's
/// hosts; a WSD device's address hosts; a port name that is an IPv4
/// address. Each host becomes `ipp://host/ipp/print`.
///
/// # Errors
///
/// [`NoNetworkAddress`] for a local port (`LPT1:`, `USB001`, `FILE:`, a
/// `\\server\queue` share) or a port that states no host.
pub fn resolve_queue_port(queue: &QueuePort) -> Result<PortResolution, NoNetworkAddress> {
    let port = queue.port.trim();
    let refuse = |reason| NoNetworkAddress {
        port: port.to_owned(),
        reason,
    };
    if let Some(uri) = port_uri(port) {
        return Ok(PortResolution {
            uris: vec![uri],
            source: PortSource::PortUri,
        });
    }
    if port.starts_with("\\\\") {
        return Err(refuse(
            "is a shared queue on another computer, which pdfcer cannot reach the printer through",
        ));
    }
    if is_local(port) {
        return Err(refuse("is a local connection, not a network address"));
    }
    let candidates = [
        (
            queue.hosts.iter().map(String::as_str).collect::<Vec<_>>(),
            PortSource::TcpIpHost,
        ),
        (
            queue
                .device_urls
                .iter()
                .filter_map(|u| url_host(u))
                .collect(),
            PortSource::WsdDevice,
        ),
        (
            ipv4_port_name(port).into_iter().collect(),
            PortSource::PortName,
        ),
    ];
    for (hosts, source) in candidates {
        let uris = host_uris(&hosts);
        if !uris.is_empty() {
            return Ok(PortResolution { uris, source });
        }
    }
    Err(refuse("states no network address pdfcer can read"))
}

/// The port name as a printer URI, if it is one.
fn port_uri(port: &str) -> Option<String> {
    let lower = port.to_ascii_lowercase();
    if lower.starts_with("ipp://") || lower.starts_with("ipps://") {
        return Some(port.to_owned());
    }
    let (scheme, rest) = if lower.starts_with("https://") {
        ("ipps", port.get(8..)?)
    } else if lower.starts_with("http://") {
        ("ipp", port.get(7..)?)
    } else {
        return None;
    };
    let (authority, path) = rest.find('/').map_or((rest, ""), |i| rest.split_at(i));
    if authority.is_empty() {
        return None;
    }
    // `http://host/x` names port 80, which `ipp://host/x` would not.
    let host_end = authority.rfind(']').map_or(0, |i| i + 1);
    let has_port = authority.get(host_end..).is_some_and(|t| t.contains(':'));
    let default = if scheme == "ipps" { ":443" } else { ":80" };
    let port = if has_port { "" } else { default };
    Some(format!("{scheme}://{authority}{port}{path}"))
}

/// Ports that are a cable, a file or a prompt rather than an address.
fn is_local(port: &str) -> bool {
    let upper = port.to_ascii_uppercase();
    let numbered = |prefix: &str| {
        upper.strip_prefix(prefix).is_some_and(|n| {
            !n.is_empty() && n.trim_end_matches(':').bytes().all(|b| b.is_ascii_digit())
        })
    };
    port.is_empty()
        || upper.ends_with(':')
        || ["LPT", "COM", "USB", "DOT4_", "TS", "XPSPORT"]
            .iter()
            .any(|p| numbered(p))
        || ["FILE:", "NUL:", "PORTPROMPT:", "SHRFAX:", "XPSPORT:"].contains(&upper.as_str())
}

/// The host of an `http(s)://host[:port]/...` URL, brackets kept.
fn url_host(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit_once('@').map_or(authority, |(_, a)| a);
    let host = if authority.starts_with('[') {
        authority.get(..=authority.find(']')?)?
    } else {
        authority.split(':').next()?
    };
    (!host.is_empty()).then_some(host)
}

/// `10.0.0.5`, `IP_10.0.0.5` or `IP_10.0.0.5_2` (a second port to the same
/// address) as the address.
fn ipv4_port_name(port: &str) -> Option<&str> {
    let bare = port
        .get(..3)
        .filter(|p| p.eq_ignore_ascii_case("IP_"))
        .map_or(port, |_| &port[3..]);
    if bare.parse::<Ipv4Addr>().is_ok() {
        return Some(bare);
    }
    let (addr, n) = bare.rsplit_once('_')?;
    (n.bytes().all(|b| b.is_ascii_digit()) && addr.parse::<Ipv4Addr>().is_ok()).then_some(addr)
}

/// `ipp://host/ipp/print` for each usable host: IPv4 first, then names,
/// then IPv6 (bracketed), duplicates dropped.
fn host_uris(hosts: &[&str]) -> Vec<String> {
    let mut ranked: Vec<(u8, String)> = Vec::new();
    for host in hosts {
        let host = host.trim();
        let bare = host.trim_start_matches('[').trim_end_matches(']');
        let unzoned = bare.split('%').next().unwrap_or(bare);
        let (rank, authority) = if bare.parse::<Ipv4Addr>().is_ok() {
            (0, bare.to_owned())
        } else if unzoned.parse::<Ipv6Addr>().is_ok() {
            (2, format!("[{bare}]"))
        } else if is_host_name(bare) {
            (1, bare.to_owned())
        } else {
            continue;
        };
        let uri = format!("ipp://{authority}/ipp/print");
        if !ranked.iter().any(|(_, u)| u.eq_ignore_ascii_case(&uri)) {
            ranked.push((rank, uri));
        }
    }
    ranked.sort_by_key(|(rank, _)| *rank);
    ranked.into_iter().map(|(_, u)| u).collect()
}

/// A DNS name: letters, digits, `-` and `.`, not empty.
fn is_host_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

#[cfg(test)]
mod tests;
