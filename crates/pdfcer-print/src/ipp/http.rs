//! IPP over plain HTTP/1.1 (RFC 8010 §4) on the standard library's TCP, so a
//! build with no network crate can still reach an `ipp://` printer when the
//! operator sends a job to one. TLS (`ipps://`) is not handled here.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// The largest HTTP response accepted, in bytes, headers included (4 MiB).
pub const MAX_HTTP_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

/// Why an IPP exchange over HTTP failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TransportError {
    /// The URL is not `http://host[:port]/path`.
    #[error("{url} is not an http:// printer URL")]
    BadUrl {
        /// The URL given.
        url: String,
    },
    /// The printer could not be reached or the connection failed.
    #[error("cannot reach {url}: {reason}")]
    Unreachable {
        /// The printer URL.
        url: String,
        /// The operating system's reason.
        reason: String,
    },
    /// The printer answered with an HTTP status other than 200.
    #[error("{url} answered HTTP {status}")]
    HttpStatus {
        /// The printer URL.
        url: String,
        /// The HTTP status code.
        status: u16,
    },
    /// The response exceeded [`MAX_HTTP_RESPONSE_BYTES`].
    #[error("{url} sent more than {MAX_HTTP_RESPONSE_BYTES} bytes")]
    TooLarge {
        /// The printer URL.
        url: String,
    },
    /// The response was not well-formed HTTP.
    #[error("{url} sent a malformed HTTP response: {reason}")]
    Malformed {
        /// The printer URL.
        url: String,
        /// What was wrong.
        reason: &'static str,
    },
}

/// `(host, port, path)` of an `http://` URL; the port defaults to 80.
fn split_url(url: &str) -> Option<(&str, u16, &str)> {
    let rest = url
        .get(..7)?
        .eq_ignore_ascii_case("http://")
        .then(|| url.get(7..))??;
    let (authority, path) = rest.find('/').map_or((rest, "/"), |i| rest.split_at(i));
    let host_end = authority.rfind(']').map_or(0, |i| i + 1);
    let (host, port) = match authority.rfind(':').filter(|&i| i >= host_end) {
        Some(i) => (authority.get(..i)?, authority.get(i + 1..)?.parse().ok()?),
        None => (authority, 80),
    };
    (!host.is_empty()).then_some((host, port, path))
}

/// POST `body` as `application/ipp` to `url` (`http://` only) and return
/// the response body. `timeout` bounds the connect and each read or write.
///
/// # Errors
///
/// [`TransportError`].
pub fn post_ipp_http(url: &str, body: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
    let bad = || TransportError::BadUrl {
        url: url.to_owned(),
    };
    let (host, port, path) = split_url(url).ok_or_else(bad)?;
    let unreachable = |e: std::io::Error| TransportError::Unreachable {
        url: url.to_owned(),
        reason: e.to_string(),
    };
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let addrs: Vec<_> = (bare, port)
        .to_socket_addrs()
        .map_err(unreachable)?
        .collect();
    let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, "no address");
    let mut stream = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last = e,
        }
    }
    let mut stream = stream.ok_or_else(|| unreachable(last))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(unreachable)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(unreachable)?;
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/ipp\r\n\
         Content-Length: {}\r\nConnection: close\r\nUser-Agent: pdfcer\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).map_err(unreachable)?;
    stream.write_all(body).map_err(unreachable)?;
    stream.flush().map_err(unreachable)?;
    let mut raw = Vec::new();
    (&mut stream)
        .take(MAX_HTTP_RESPONSE_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(unreachable)?;
    if raw.len() as u64 > MAX_HTTP_RESPONSE_BYTES {
        return Err(TransportError::TooLarge {
            url: url.to_owned(),
        });
    }
    parse_http_response(url, &raw)
}

/// The body of the final (non-1xx) HTTP/1.x response in `raw`.
pub(crate) fn parse_http_response(url: &str, raw: &[u8]) -> Result<Vec<u8>, TransportError> {
    let malformed = |reason| TransportError::Malformed {
        url: url.to_owned(),
        reason,
    };
    let mut rest = raw;
    loop {
        let end = find(rest, b"\r\n\r\n").ok_or_else(|| malformed("no end of headers"))?;
        let (head, after) = rest.split_at(end);
        let body = after.get(4..).unwrap_or_default();
        let head = std::str::from_utf8(head).map_err(|_| malformed("headers are not text"))?;
        let mut lines = head.split("\r\n");
        let status = lines
            .next()
            .filter(|l| l.starts_with("HTTP/1."))
            .and_then(|l| l.split(' ').nth(1))
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or_else(|| malformed("no status line"))?;
        if (100..200).contains(&status) {
            rest = body;
            continue;
        }
        if status != 200 {
            return Err(TransportError::HttpStatus {
                url: url.to_owned(),
                status,
            });
        }
        let header = |name: &str| {
            lines.clone().find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
            })
        };
        if header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
            return dechunk(body).ok_or_else(|| malformed("bad chunked encoding"));
        }
        return match header("content-length").map(str::parse::<usize>) {
            Some(Ok(n)) => body
                .get(..n)
                .map(<[u8]>::to_vec)
                .ok_or_else(|| malformed("body shorter than Content-Length")),
            Some(Err(_)) => Err(malformed("bad Content-Length")),
            None => Ok(body.to_vec()),
        };
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Decode a chunked body (RFC 9112 §7.1); trailers are ignored.
fn dechunk(mut body: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line_end = find(body, b"\r\n")?;
        let size_line = std::str::from_utf8(body.get(..line_end)?).ok()?;
        let hex = size_line.split(';').next()?.trim();
        let size = usize::from_str_radix(hex, 16).ok()?;
        let start = line_end + 2;
        if size == 0 {
            return Some(out);
        }
        let end = start.checked_add(size)?;
        out.extend_from_slice(body.get(start..end)?);
        body = body.get(end.checked_add(2)?..)?;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    #[test]
    fn urls_split_into_host_port_and_path() {
        assert_eq!(
            split_url("http://h:631/ipp/print"),
            Some(("h", 631, "/ipp/print"))
        );
        assert_eq!(split_url("HTTP://h"), Some(("h", 80, "/")));
        assert_eq!(split_url("http://[::1]:631/x"), Some(("[::1]", 631, "/x")));
        assert_eq!(split_url("http://[::1]/x"), Some(("[::1]", 80, "/x")));
        assert_eq!(split_url("https://h/"), None);
        assert_eq!(split_url("http://:631/"), None);
        assert_eq!(split_url("http://h:port/"), None);
    }

    #[test]
    fn a_content_length_body_is_returned_after_an_interim_response() {
        let raw =
            b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabcdef";
        assert_eq!(parse_http_response("u", raw).unwrap(), b"abc");
    }

    #[test]
    fn a_chunked_body_is_decoded() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;x=y\r\nde\r\n0\r\n\r\n";
        assert_eq!(parse_http_response("u", raw).unwrap(), b"abcde");
    }

    #[test]
    fn errors_are_named() {
        let not_found = b"HTTP/1.1 404 Not Found\r\n\r\n";
        assert!(matches!(
            parse_http_response("u", not_found),
            Err(TransportError::HttpStatus { status: 404, .. })
        ));
        for raw in [
            &b"garbage"[..],
            b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nabc",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nffffffffffffffff\r\n",
        ] {
            assert!(matches!(
                parse_http_response("u", raw),
                Err(TransportError::Malformed { .. })
            ));
        }
    }

    #[test]
    fn an_unreachable_printer_is_a_named_error() {
        // Bind then drop, so the port is closed.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let url = format!("http://127.0.0.1:{port}/ipp");
        let err = post_ipp_http(&url, b"x", Duration::from_secs(2)).unwrap_err();
        assert!(matches!(err, TransportError::Unreachable { .. }), "{err:?}");
    }
}
