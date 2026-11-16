//! Why a job that could go straight to a printer that reads PDF goes
//! through the driver instead, named the same way by both shells.

/// Why a job is not sent to the printer as a PDF.
///
/// # Examples
///
/// ```
/// use pdfcer_print::pdf_direct::FallbackReason;
///
/// assert_eq!(FallbackReason::NoPdf.keyword(), "no-pdf");
/// assert_eq!(FallbackReason::NoPdf.to_string(), "no-pdf");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FallbackReason {
    /// The job asks for something only the driver can do.
    DriverOnlyOption,
    /// The queue's port names no network address to reach the printer at.
    NoNetworkAddress,
    /// The printer answered, and PDF is not among the formats it reads.
    NoPdf,
    /// No printer answered at the queue's address.
    Unreachable,
    /// The job's sheets could not be composed into a PDF
    /// (`compose::ComposeError`, with the `compose` feature).
    CannotCompose,
}

impl FallbackReason {
    /// The stable keyword for a report line or receipt (`fallback=<keyword>`
    /// on the CLI's `print` line).
    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::DriverOnlyOption => "driver-only-option",
            Self::NoNetworkAddress => "no-network-address",
            Self::NoPdf => "no-pdf",
            Self::Unreachable => "unreachable",
            Self::CannotCompose => "cannot-compose",
        }
    }
}

impl std::fmt::Display for FallbackReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.keyword())
    }
}
