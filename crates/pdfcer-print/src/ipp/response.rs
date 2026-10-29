//! Decoding a printer's IPP response (RFC 8010 §3), and the `ipp://` to
//! HTTP URL mapping (RFC 3510 §4, RFC 7472 §4).

use super::{IppAttribute, IppGroup, IppValue, MAX_ATTRIBUTES, group, tag};

/// Why a response could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum IppError {
    /// The response ended inside a field.
    #[error("the printer's IPP response is truncated at byte {offset}")]
    Truncated {
        /// Where the missing bytes should have been.
        offset: usize,
    },
    /// The response had no end-of-attributes tag.
    #[error("the printer's IPP response has no end-of-attributes tag")]
    NoEndTag,
    /// The response carried more than [`MAX_ATTRIBUTES`] attributes.
    #[error("the printer's IPP response has more than {MAX_ATTRIBUTES} attributes")]
    TooManyAttributes,
}

/// A decoded response.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct IppResponse {
    /// The protocol version the printer answered with.
    pub version: (u8, u8),
    /// The status code (RFC 8011 §4.1.6); see [`status_name`].
    pub status: u16,
    /// The request id echoed back.
    pub request_id: u32,
    /// The attribute groups, in wire order.
    pub groups: Vec<IppGroup>,
}

impl IppResponse {
    /// `successful-ok` and its variants (`0x0000..=0x00FF`).
    pub fn is_success(&self) -> bool {
        self.status <= 0x00FF
    }

    /// The first attribute named `name` in a group tagged `group_tag`.
    pub fn attribute(&self, group_tag: u8, name: &str) -> Option<&IppAttribute> {
        self.groups
            .iter()
            .filter(|g| g.tag == group_tag)
            .flat_map(|g| &g.attributes)
            .find(|a| a.name == name)
    }

    /// The string values of printer attribute `name`, empty when absent.
    pub fn printer_strings(&self, name: &str) -> Vec<&str> {
        self.attribute(group::PRINTER, name)
            .map(|a| a.values.iter().filter_map(IppValue::as_str).collect())
            .unwrap_or_default()
    }

    /// The names of the attributes the printer reported it ignored or
    /// substituted (the unsupported-attributes group).
    pub fn unsupported(&self) -> Vec<&str> {
        self.groups
            .iter()
            .filter(|g| g.tag == group::UNSUPPORTED)
            .flat_map(|g| &g.attributes)
            .map(|a| a.name.as_str())
            .collect()
    }

    /// The operation group's `status-message`, if any.
    pub fn status_message(&self) -> Option<&str> {
        self.attribute(group::OPERATION, "status-message")?
            .values
            .first()?
            .as_str()
    }

    /// The `job-id` of a created job.
    pub fn job_id(&self) -> Option<i32> {
        self.attribute(group::JOB, "job-id")?
            .values
            .first()?
            .as_i32()
    }
}

/// The RFC 8011 §B name of a status code, or `"unknown-status"`.
pub fn status_name(status: u16) -> &'static str {
    match status {
        0x0000 => "successful-ok",
        0x0001 => "successful-ok-ignored-or-substituted-attributes",
        0x0002 => "successful-ok-conflicting-attributes",
        0x0400 => "client-error-bad-request",
        0x0401 => "client-error-forbidden",
        0x0402 => "client-error-not-authenticated",
        0x0403 => "client-error-not-authorized",
        0x0404 => "client-error-not-possible",
        0x0405 => "client-error-timeout",
        0x0406 => "client-error-not-found",
        0x0408 => "client-error-request-entity-too-large",
        0x040A => "client-error-document-format-not-supported",
        0x040B => "client-error-attributes-or-values-not-supported",
        0x040D => "client-error-conflicting-attributes",
        0x0500 => "server-error-internal-error",
        0x0501 => "server-error-operation-not-supported",
        0x0503 => "server-error-version-not-supported",
        0x0506 => "server-error-not-accepting-jobs",
        0x0507 => "server-error-busy",
        _ => "unknown-status",
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], IppError> {
        let end = self.at.checked_add(n);
        let slice = end.and_then(|end| self.bytes.get(self.at..end));
        let slice = slice.ok_or(IppError::Truncated { offset: self.at })?;
        self.at += n;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, IppError> {
        Ok(self.take(1)?.first().copied().unwrap_or_default())
    }

    fn u16(&mut self) -> Result<u16, IppError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes(
            [b.first(), b.get(1)].map(|x| x.copied().unwrap_or(0)),
        ))
    }

    fn u32(&mut self) -> Result<u32, IppError> {
        Ok(u32::from(self.u16()?) << 16 | u32::from(self.u16()?))
    }
}

/// Decode `bytes` as an IPP response. Data after the end tag is ignored.
///
/// # Errors
///
/// [`IppError`] for a truncated message, a missing end tag, or more than
/// [`MAX_ATTRIBUTES`] attributes.
pub fn parse_response(bytes: &[u8]) -> Result<IppResponse, IppError> {
    let mut c = Cursor { bytes, at: 0 };
    let version = (c.u8()?, c.u8()?);
    let status = c.u16()?;
    let request_id = c.u32()?;
    let mut groups: Vec<IppGroup> = Vec::new();
    let mut count = 0usize;
    loop {
        let t = c.u8().map_err(|_| IppError::NoEndTag)?;
        if t == group::END {
            break;
        }
        if t <= 0x0F {
            groups.push(IppGroup::new(t, Vec::new()));
            continue;
        }
        let name = String::from_utf8_lossy(take_field(&mut c)?).into_owned();
        let value = decode_value(t, take_field(&mut c)?);
        // A value before any group header, or a lone additional value before
        // any named attribute, has nowhere to go; it is dropped.
        let Some(g) = groups.last_mut() else { continue };
        if name.is_empty() {
            if let Some(last) = g.attributes.last_mut() {
                last.values.push(value);
            }
            continue;
        }
        count += 1;
        if count > MAX_ATTRIBUTES {
            return Err(IppError::TooManyAttributes);
        }
        g.attributes.push(IppAttribute::new(&name, value));
    }
    Ok(IppResponse {
        version,
        status,
        request_id,
        groups,
    })
}

fn take_field<'a>(c: &mut Cursor<'a>) -> Result<&'a [u8], IppError> {
    let n = c.u16()?;
    c.take(usize::from(n))
}

fn decode_value(t: u8, b: &[u8]) -> IppValue {
    let int = |b: &[u8]| <[u8; 4]>::try_from(b).ok().map(i32::from_be_bytes);
    match t {
        tag::INTEGER => int(b).map(IppValue::Integer),
        tag::ENUM => int(b).map(IppValue::Enum),
        tag::BOOLEAN if b.len() == 1 => b.first().map(|v| IppValue::Boolean(*v != 0)),
        tag::RANGE if b.len() == 8 => {
            let (lo, hi) = b.split_at(4);
            int(lo).zip(int(hi)).map(|(lo, hi)| IppValue::Range(lo, hi))
        }
        0x41..=0x49 => Some(IppValue::String {
            tag: t,
            value: String::from_utf8_lossy(b).into_owned(),
        }),
        0x10..=0x1F => Some(IppValue::OutOfBand(t)),
        _ => None,
    }
    .unwrap_or_else(|| IppValue::Other {
        tag: t,
        bytes: b.to_vec(),
    })
}

/// The HTTP URL an IPP printer URI is reached at: `ipp://` becomes
/// `http://` and `ipps://` becomes `https://`, on port 631 when the URI
/// names none; `http://` and `https://` pass through. `None` for any other
/// scheme or an empty host.
pub fn http_url(printer_uri: &str) -> Option<String> {
    let lower = printer_uri.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(printer_uri.to_owned());
    }
    let (scheme, rest) = if lower.starts_with("ipps://") {
        ("https", printer_uri.get(7..)?)
    } else if lower.starts_with("ipp://") {
        ("http", printer_uri.get(6..)?)
    } else {
        return None;
    };
    let (authority, path) = rest.find('/').map_or((rest, ""), |i| rest.split_at(i));
    if authority.is_empty() {
        return None;
    }
    // A port follows the last ':' after any bracketed IPv6 literal.
    let host_end = authority.rfind(']').map_or(0, |i| i + 1);
    let has_port = authority.get(host_end..).is_some_and(|t| t.contains(':'));
    let port = if has_port { "" } else { ":631" };
    Some(format!("{scheme}://{authority}{port}{path}"))
}
