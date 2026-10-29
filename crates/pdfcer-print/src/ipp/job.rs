//! A Print-Job request (RFC 8011 §4.2.1) and its job template attributes
//! (RFC 8011 §5.2).

use super::{IppAttribute, IppGroup, IppValue, group, tag};

/// `sides` (RFC 8011 §5.2.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Sides {
    /// `one-sided`.
    OneSided,
    /// `two-sided-long-edge`.
    TwoSidedLongEdge,
    /// `two-sided-short-edge`.
    TwoSidedShortEdge,
}

impl Sides {
    /// The IPP keyword.
    pub fn keyword(self) -> &'static str {
        match self {
            Self::OneSided => "one-sided",
            Self::TwoSidedLongEdge => "two-sided-long-edge",
            Self::TwoSidedShortEdge => "two-sided-short-edge",
        }
    }
}

/// What to print and how. Every job template field left at its default is
/// omitted from the request, so the printer's own default applies.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct IppJob {
    /// The printer's `ipp://` / `ipps://` URI, as given by the operator.
    pub printer_uri: String,
    /// `requesting-user-name`.
    pub user: String,
    /// `job-name`, shown in the printer's queue.
    pub job_name: String,
    /// `copies`; 1 is omitted.
    pub copies: u16,
    /// `sides`; `None` leaves the printer's default.
    pub sides: Option<Sides>,
    /// `page-ranges`, 1-based inclusive; empty prints every page.
    pub page_ranges: Vec<(u32, u32)>,
    /// `print-color-mode = monochrome`.
    pub monochrome: bool,
    /// `orientation-requested` (3 portrait, 4 landscape); `None` omits it.
    pub orientation: Option<i32>,
    /// `finishings` enum values (RFC 8011 §5.2.6); empty omits it.
    pub finishings: Vec<i32>,
    /// `output-bin` keyword (PWG 5100.2); `None` omits it.
    pub output_bin: Option<String>,
    /// `multiple-document-handling = separate-documents-uncollated-copies`
    /// (each page's copies together); `false` omits it.
    pub uncollated: bool,
    /// `print-scaling` keyword (PWG 5100.13 §6.2.10: `auto`, `auto-fit`,
    /// `fill`, `fit`, `none`); `None` omits it.
    pub print_scaling: Option<String>,
}

impl IppJob {
    /// A one-copy job of `job_name` for `printer_uri`, every other template
    /// attribute at the printer's default.
    pub fn new(printer_uri: &str, user: &str, job_name: &str) -> Self {
        Self {
            printer_uri: printer_uri.to_owned(),
            user: user.to_owned(),
            job_name: job_name.to_owned(),
            copies: 1,
            sides: None,
            page_ranges: Vec::new(),
            monochrome: false,
            orientation: None,
            finishings: Vec::new(),
            output_bin: None,
            uncollated: false,
            print_scaling: None,
        }
    }

    /// The job template attributes this job sets, in wire order.
    pub fn job_attributes(&self) -> Vec<IppAttribute> {
        let mut out = Vec::new();
        if self.copies > 1 {
            out.push(IppAttribute::new(
                "copies",
                IppValue::Integer(i32::from(self.copies)),
            ));
        }
        if let Some(sides) = self.sides {
            out.push(IppAttribute::new(
                "sides",
                IppValue::keyword(sides.keyword()),
            ));
        }
        if !self.page_ranges.is_empty() {
            let clamp = |n: u32| i32::try_from(n).unwrap_or(i32::MAX);
            let ranges = self.page_ranges.iter();
            out.push(IppAttribute::many(
                "page-ranges",
                ranges
                    .map(|&(a, b)| IppValue::Range(clamp(a), clamp(b)))
                    .collect(),
            ));
        }
        if self.monochrome {
            out.push(IppAttribute::new(
                "print-color-mode",
                IppValue::keyword("monochrome"),
            ));
        }
        if let Some(o) = self.orientation {
            out.push(IppAttribute::new(
                "orientation-requested",
                IppValue::Enum(o),
            ));
        }
        if !self.finishings.is_empty() {
            let values = self.finishings.iter().map(|&f| IppValue::Enum(f));
            out.push(IppAttribute::many("finishings", values.collect()));
        }
        if let Some(bin) = &self.output_bin {
            out.push(IppAttribute::new("output-bin", IppValue::keyword(bin)));
        }
        if self.uncollated {
            out.push(IppAttribute::new(
                "multiple-document-handling",
                IppValue::keyword("separate-documents-uncollated-copies"),
            ));
        }
        if let Some(scaling) = &self.print_scaling {
            out.push(IppAttribute::new(
                "print-scaling",
                IppValue::keyword(scaling),
            ));
        }
        out
    }

    /// The encoded Print-Job request carrying `pdf` as `application/pdf`.
    pub fn print_job(&self, request_id: u32, pdf: &[u8]) -> Vec<u8> {
        let mut op = super::operation_group(&self.printer_uri, &self.user);
        op.attributes.push(IppAttribute::new(
            "job-name",
            IppValue::string(tag::NAME, &self.job_name),
        ));
        op.attributes.push(IppAttribute::new(
            "document-format",
            IppValue::string(tag::MIME, "application/pdf"),
        ));
        let mut groups = vec![op];
        let job = self.job_attributes();
        if !job.is_empty() {
            groups.push(IppGroup::new(group::JOB, job));
        }
        super::encode_request(super::OP_PRINT_JOB, request_id, &groups, pdf)
    }
}
