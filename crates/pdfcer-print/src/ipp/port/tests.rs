// Test code: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;

fn uris(q: &QueuePort) -> (Vec<String>, PortSource) {
    let r = resolve_queue_port(q).unwrap();
    (r.uris, r.source)
}

fn port(name: &str, hosts: &[&str], urls: &[&str]) -> QueuePort {
    QueuePort {
        port: name.to_owned(),
        hosts: hosts.iter().map(|s| (*s).to_owned()).collect(),
        device_urls: urls.iter().map(|s| (*s).to_owned()).collect(),
    }
}

#[test]
fn an_ipp_port_name_is_used_as_given() {
    let (u, s) = uris(&QueuePort::named("ipps://p.local:443/ipp/print"));
    assert_eq!(u, ["ipps://p.local:443/ipp/print"]);
    assert_eq!(s, PortSource::PortUri);
}

#[test]
fn an_http_port_name_becomes_ipp_and_keeps_its_port() {
    let (u, _) = uris(&QueuePort::named("http://srv:631/printers/q"));
    assert_eq!(u, ["ipp://srv:631/printers/q"]);
    let (u, _) = uris(&QueuePort::named("http://srv/printers/q"));
    assert_eq!(u, ["ipp://srv:80/printers/q"], "http names port 80");
    let (u, _) = uris(&QueuePort::named("https://[fe80::1]/q"));
    assert_eq!(u, ["ipps://[fe80::1]:443/q"]);
}

#[test]
fn a_tcp_ip_port_asks_its_stated_host_ipv4_first() {
    let q = port(
        "printer-3",
        &["laser.office", "192.168.2.96", "fe80::5"],
        &[],
    );
    let (u, s) = uris(&q);
    assert_eq!(
        u,
        [
            "ipp://192.168.2.96/ipp/print",
            "ipp://laser.office/ipp/print",
            "ipp://[fe80::5]/ipp/print",
        ]
    );
    assert_eq!(s, PortSource::TcpIpHost);
}

#[test]
fn a_host_stated_twice_is_asked_once() {
    let q = port("192.168.2.96", &["192.168.2.96", "192.168.2.96"], &[]);
    assert_eq!(uris(&q).0, ["ipp://192.168.2.96/ipp/print"]);
}

#[test]
fn a_wsd_port_asks_its_device_hosts() {
    let q = port(
        "WSD-2ccdff04-f26a-4859-a400-0547238ea463",
        &[],
        &[
            "http://[fe80::decd:2fff:fe81:fdcb%14]:80/WSD/DEVICE",
            "http://192.168.0.90:80/wsd/mex",
        ],
    );
    let (u, s) = uris(&q);
    assert_eq!(
        u,
        [
            "ipp://192.168.0.90/ipp/print",
            "ipp://[fe80::decd:2fff:fe81:fdcb%14]/ipp/print",
        ],
        "the zone id stays: a link-local address means nothing without it"
    );
    assert_eq!(s, PortSource::WsdDevice);
}

#[test]
fn a_wsd_port_without_a_device_address_is_not_network() {
    let e = resolve_queue_port(&QueuePort::named("WSD-a8521be9")).unwrap_err();
    assert_eq!(
        e.to_string(),
        "port WSD-a8521be9 states no network address pdfcer can read"
    );
}

#[test]
fn an_ipv4_port_name_is_the_address_when_nothing_else_is_stated() {
    for name in ["10.0.0.5", "IP_10.0.0.5", "ip_10.0.0.5", "IP_10.0.0.5_2"] {
        let (u, s) = uris(&QueuePort::named(name));
        assert_eq!(u, ["ipp://10.0.0.5/ipp/print"], "{name}");
        assert_eq!(s, PortSource::PortName);
    }
}

#[test]
fn stated_hosts_outrank_the_port_name() {
    let q = port("IP_10.0.0.5", &["10.0.0.9"], &[]);
    assert_eq!(uris(&q).0, ["ipp://10.0.0.9/ipp/print"]);
}

#[test]
fn local_ports_are_not_network() {
    for name in [
        "LPT1:",
        "COM3:",
        "USB001",
        "FILE:",
        "nul:",
        "PORTPROMPT:",
        "SHRFAX:",
        "TS001",
        "CPW4:",
        "",
    ] {
        let e = resolve_queue_port(&QueuePort::named(name)).unwrap_err();
        assert_eq!(
            e.reason, "is a local connection, not a network address",
            "{name:?}"
        );
    }
}

#[test]
fn a_share_on_another_computer_is_not_network() {
    let e = resolve_queue_port(&QueuePort::named("\\\\server\\laser")).unwrap_err();
    assert!(e.reason.starts_with("is a shared queue"), "{e}");
}

#[test]
fn a_vendor_port_with_no_host_is_not_network() {
    let e = resolve_queue_port(&QueuePort::named("EP81FDCB:ET-16600 SERIES")).unwrap_err();
    assert_eq!(e.reason, "states no network address pdfcer can read");
}

#[test]
fn junk_hosts_are_skipped() {
    let q = port("x", &["", "has space", "a/b"], &["not a url"]);
    assert!(resolve_queue_port(&q).is_err());
}
