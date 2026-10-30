//! Sending a PDF to an IPP printer: query its attributes, check the job
//! against what it offers, send Print-Job, read the receipt (RFC 8011
//! §4.2.1, §4.2.5). The transport is injected, so this crate carries no TLS
//! client; [`HttpTransport`] covers `ipp://` and `http://`.

use std::time::Duration;

use super::{
    IppError, IppJob, IppResponse, OfferNote, PrinterOffer, TransportError, check_finishing,
    get_printer_attributes, http_url, parse_response, post_ipp_http, status_name,
};
use crate::finishing::Finishing;

/// Carries one IPP message to a printer and returns the reply body.
pub trait IppTransport {
    /// POST `message` as `application/ipp` to `url` (the HTTP URL from
    /// [`http_url`]: `http://` or `https://`).
    ///
    /// # Errors
    ///
    /// [`TransportError`]; a transport that cannot do TLS returns
    /// [`TransportError::NeedsTls`] for `https://`.
    fn post(&mut self, url: &str, message: &[u8]) -> Result<Vec<u8>, TransportError>;
}

/// Plain HTTP/1.1 on the standard library's TCP ([`post_ipp_http`]).
/// `https://` is refused with [`TransportError::NeedsTls`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpTransport {
    /// Bounds the connect and each read or write.
    pub timeout: Duration,
}

impl Default for HttpTransport {
    /// A 60-second timeout.
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(60),
        }
    }
}

impl IppTransport for HttpTransport {
    fn post(&mut self, url: &str, message: &[u8]) -> Result<Vec<u8>, TransportError> {
        if url
            .get(..8)
            .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
        {
            return Err(TransportError::NeedsTls {
                url: url.to_owned(),
            });
        }
        post_ipp_http(url, message, self.timeout)
    }
}

/// Why a printer query or job failed. The `Display` text is a complete
/// sentence naming the printer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum IppSendError {
    /// `uri` is not an `ipp://`, `ipps://`, `http://` or `https://` URI.
    #[error("{uri} is not a printer URI (ipp://, ipps://, http:// or https://)")]
    NotAPrinterUri {
        /// The URI given.
        uri: String,
    },
    /// The message could not be carried.
    #[error(transparent)]
    Transport(#[from] TransportError),
    /// The reply was not a well-formed IPP response.
    #[error("{uri}: {source}")]
    Malformed {
        /// The printer URI.
        uri: String,
        /// What was wrong.
        source: IppError,
    },
    /// The printer answered with an error status.
    #[error("{uri} refused: {}{}", status_name(*status), message.as_ref().map(|m| format!(" ({m})")).unwrap_or_default())]
    Refused {
        /// The printer URI.
        uri: String,
        /// The status code (RFC 8011 §4.1.6).
        status: u16,
        /// `status-message`, when sent.
        message: Option<String>,
    },
    /// The printer lists its document formats and PDF is not one.
    #[error("{uri} does not accept PDF (it takes {}). Print through a driver that converts it.", formats.join(", "))]
    NotPdf {
        /// The printer URI.
        uri: String,
        /// `document-format-supported`.
        formats: Vec<String>,
    },
    /// The printer lists its finishing or bins and does not offer what was
    /// asked.
    #[error("{uri} does not offer: {}", refused.join("; "))]
    NotOffered {
        /// The printer URI.
        uri: String,
        /// One line per request refused, naming it.
        refused: Vec<String>,
        /// What could not be confirmed, reported before the refusal.
        notes: Vec<OfferNote>,
    },
}

/// A job checked against a printer and ready to send.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct IppPlan {
    /// The job, with `finishings` and `output-bin` set from the check.
    pub job: IppJob,
    /// What the printer could not confirm; the job asks for it as given.
    pub notes: Vec<OfferNote>,
}

/// What the printer said when it accepted a job.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct IppReceipt {
    /// `job-id`, when sent.
    pub job_id: Option<i32>,
    /// Attribute names the printer ignored or substituted.
    pub ignored: Vec<String>,
}

/// The outcome of [`send_ipp`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct IppSent {
    /// What the printer offers.
    pub offer: PrinterOffer,
    /// The job as sent, and what could not be confirmed.
    pub plan: IppPlan,
    /// The printer's receipt.
    pub receipt: IppReceipt,
}

/// Send one request to `uri` over `t` and decode the reply; an error
/// status is [`IppSendError::Refused`].
fn exchange(
    uri: &str,
    message: &[u8],
    t: &mut dyn IppTransport,
) -> Result<IppResponse, IppSendError> {
    let url = http_url(uri).ok_or_else(|| IppSendError::NotAPrinterUri {
        uri: uri.to_owned(),
    })?;
    let raw = t.post(&url, message)?;
    let r = parse_response(&raw).map_err(|source| IppSendError::Malformed {
        uri: uri.to_owned(),
        source,
    })?;
    if r.is_success() {
        Ok(r)
    } else {
        Err(IppSendError::Refused {
            uri: uri.to_owned(),
            status: r.status,
            message: r.status_message().map(str::to_owned),
        })
    }
}

/// Ask printer `uri` what it offers (Get-Printer-Attributes, request id 1).
///
/// # Errors
///
/// [`IppSendError`]: the URI, the transport, the reply or its status.
pub fn query_printer(
    uri: &str,
    user: &str,
    t: &mut dyn IppTransport,
) -> Result<PrinterOffer, IppSendError> {
    let r = exchange(uri, &get_printer_attributes(uri, user, 1), t)?;
    Ok(PrinterOffer::from_response(&r))
}

/// Check `job` and `finishing` against `offer`: PDF must be accepted, and
/// listed finishing and bins must include what was asked. Nothing is sent.
///
/// # Errors
///
/// [`IppSendError::NotPdf`] or [`IppSendError::NotOffered`].
pub fn plan_ipp_job(
    mut job: IppJob,
    finishing: &Finishing,
    offer: &PrinterOffer,
) -> Result<IppPlan, IppSendError> {
    let uri = job.printer_uri.clone();
    if !offer.accepts_pdf() {
        return Err(IppSendError::NotPdf {
            uri,
            formats: offer.formats.clone(),
        });
    }
    let checked = check_finishing(finishing, Some(offer));
    if !checked.refused.is_empty() {
        return Err(IppSendError::NotOffered {
            uri,
            refused: checked.refused,
            notes: checked.notes,
        });
    }
    job.finishings = checked.values;
    job.output_bin = checked.output_bin;
    Ok(IppPlan {
        job,
        notes: checked.notes,
    })
}

/// Send `pdf` as `job` (Print-Job, request id 2) and read the receipt.
///
/// # Errors
///
/// [`IppSendError`]: the URI, the transport, the reply or its status.
pub fn send_ipp_job(
    job: &IppJob,
    pdf: &[u8],
    t: &mut dyn IppTransport,
) -> Result<IppReceipt, IppSendError> {
    let r = exchange(&job.printer_uri, &job.print_job(2, pdf), t)?;
    Ok(IppReceipt {
        job_id: r.job_id(),
        ignored: r.unsupported().into_iter().map(str::to_owned).collect(),
    })
}

/// Query, check and send in one call: what `pdfcer print --to <uri>
/// --send` does after preparing the document.
///
/// # Errors
///
/// As [`query_printer`], [`plan_ipp_job`] and [`send_ipp_job`].
///
/// # Example
///
/// ```no_run
/// use pdfcer_print::finishing::Finishing;
/// use pdfcer_print::ipp::{HttpTransport, IppJob, send_ipp};
///
/// let job = IppJob::new("ipp://printer.local/ipp/print", "ken", "a.pdf");
/// let pdf = std::fs::read("a.pdf")?;
/// let sent = send_ipp(job, &Finishing::default(), &pdf, &mut HttpTransport::default())?;
/// println!("job {:?}", sent.receipt.job_id);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn send_ipp(
    job: IppJob,
    finishing: &Finishing,
    pdf: &[u8],
    t: &mut dyn IppTransport,
) -> Result<IppSent, IppSendError> {
    let offer = query_printer(&job.printer_uri, &job.user, t)?;
    let plan = plan_ipp_job(job, finishing, &offer)?;
    let receipt = send_ipp_job(&plan.job, pdf, t)?;
    Ok(IppSent {
        offer,
        plan,
        receipt,
    })
}

#[cfg(test)]
mod tests;
