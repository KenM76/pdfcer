//! What an IPP printer offers, read from its Get-Printer-Attributes reply
//! (RFC 8011 §5.4; `finishings` values per PWG 5100.1), and a job's
//! finishing checked against it.

use super::{IppResponse, Sides};
use crate::finishing::{Edge, Finishing, Fold, PUNCH_RUNS, Staple, finishing_name, ipp_finishings};

/// The `finishings` values that name a class, not a finish: `none`,
/// `staple`, `punch`, `fold`.
const CLASS_VALUES: [i32; 4] = [3, 4, 5, 10];

/// Hole punching a printer offers on one edge.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Punch {
    /// The edge.
    pub edge: Edge,
    /// Hole counts offered, ascending; `None` is `punch-multiple-*` (a
    /// count the printer does not state).
    pub holes: Vec<Option<u8>>,
}

/// What a printer says it offers. Every list is empty when the printer did
/// not list it; [`finishings`](Self::finishings) alone tells "not listed"
/// (`None`) from "lists none".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PrinterOffer {
    /// `printer-make-and-model`.
    pub make_and_model: Option<String>,
    /// `document-format-supported` (MIME types).
    pub formats: Vec<String>,
    /// `finishings-supported`, raw; `None` when not listed.
    pub finishings: Option<Vec<i32>>,
    /// Staple positions among `finishings`, in [`Staple::ALL`] order.
    pub staples: Vec<Staple>,
    /// Punch edges among `finishings`, in [`Edge::ALL`] order.
    pub punches: Vec<Punch>,
    /// Folds among `finishings`, in [`Fold::ALL`] order.
    pub folds: Vec<Fold>,
    /// Other `finishings` (registry keywords: `booklet-maker`, `trim`...)
    /// that a [`Finishing`] cannot ask for.
    pub other_finishings: Vec<&'static str>,
    /// `output-bin-supported`.
    pub output_bins: Vec<String>,
    /// `sides-supported`, the keywords pdfcer knows.
    pub sides: Vec<Sides>,
    /// `print-color-mode-supported` keywords (`monochrome`, `color`...).
    pub color_modes: Vec<String>,
}

impl PrinterOffer {
    /// Read the offer from a Get-Printer-Attributes reply.
    #[must_use]
    pub fn from_response(r: &IppResponse) -> Self {
        let strings = |name| -> Vec<String> {
            r.printer_strings(name)
                .into_iter()
                .map(str::to_owned)
                .collect()
        };
        let finishings = r.printer_ints("finishings-supported");
        let list = finishings.as_deref().unwrap_or_default();
        Self {
            make_and_model: strings("printer-make-and-model").into_iter().next(),
            formats: strings("document-format-supported"),
            staples: offered(&Staple::ALL, |s| list.contains(&s.ipp())),
            punches: Edge::ALL.iter().filter_map(|&e| punch(e, list)).collect(),
            folds: offered(&Fold::ALL, |f| list.contains(&f.ipp())),
            other_finishings: other_finishings(list),
            output_bins: strings("output-bin-supported"),
            sides: r
                .printer_strings("sides-supported")
                .into_iter()
                .filter_map(sides)
                .collect(),
            color_modes: strings("print-color-mode-supported"),
            finishings,
        }
    }

    /// Whether the printer takes PDF: it lists `application/pdf`, or lists
    /// no formats at all.
    #[must_use]
    pub fn accepts_pdf(&self) -> bool {
        self.formats.is_empty()
            || self
                .formats
                .iter()
                .any(|f| f.eq_ignore_ascii_case("application/pdf"))
    }
}

fn offered<T: Copy>(all: &[T], has: impl Fn(T) -> bool) -> Vec<T> {
    all.iter().copied().filter(|&t| has(t)).collect()
}

fn punch(edge: Edge, list: &[i32]) -> Option<Punch> {
    let holes: Vec<Option<u8>> = PUNCH_RUNS
        .iter()
        .filter(|(_, start)| list.contains(&(start + edge.ipp_offset())))
        .map(|&(n, _)| (n != 0).then_some(n))
        .collect();
    (!holes.is_empty()).then_some(Punch { edge, holes })
}

fn other_finishings(list: &[i32]) -> Vec<&'static str> {
    list.iter()
        .filter(|n| !CLASS_VALUES.contains(n))
        .filter_map(|&n| finishing_name(n))
        .filter(|n| !n.starts_with("staple") && !n.starts_with("punch") && !n.starts_with("fold"))
        .collect()
}

fn sides(keyword: &str) -> Option<Sides> {
    [
        Sides::OneSided,
        Sides::TwoSidedLongEdge,
        Sides::TwoSidedShortEdge,
    ]
    .into_iter()
    .find(|s| s.keyword() == keyword)
}

/// Something about a finishing request the printer could not confirm. The
/// request is sent as given.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OfferNote {
    /// Staple, punch or fold was asked for and the printer does not list
    /// which finishing it offers.
    FinishingNotListed,
    /// An output bin was asked for and the printer does not list its bins.
    BinsNotListed {
        /// The bin asked for.
        asked: String,
    },
}

impl OfferNote {
    /// The note as a sentence about printer `who`.
    #[must_use]
    pub fn describe(&self, who: &str) -> String {
        match self {
            Self::FinishingNotListed => {
                format!("{who} does not say which finishing it offers; asking as given.")
            }
            Self::BinsNotListed { asked } => {
                format!("{who} does not list its output bins; asking for {asked:?} as given.")
            }
        }
    }
}

/// A finishing request checked against an offer: what to put in the job,
/// what could not be confirmed and what the printer does not offer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CheckedFinishing {
    /// The `finishings` values.
    pub values: Vec<i32>,
    /// The `output-bin`, spelled as the printer lists it.
    pub output_bin: Option<String>,
    /// What could not be confirmed, in the order found.
    pub notes: Vec<OfferNote>,
    /// One line per request the printer lists and does not offer, naming
    /// it; the job must not be sent unless this is empty.
    pub refused: Vec<String>,
}

/// Check `f` against `offer` (`None` when the printer gave no reply).
/// Anything the printer lists and does not offer is refused by name; an
/// unlisted capability is asked for as given and noted. A bin is matched
/// without regard to case and sent as the printer spells it.
#[must_use]
pub fn check_finishing(f: &Finishing, offer: Option<&PrinterOffer>) -> CheckedFinishing {
    let supported = offer.and_then(|o| o.finishings.as_deref());
    let mut notes = Vec::new();
    if (f.staple.is_some() || f.punch.is_some() || f.fold.is_some()) && supported.is_none() {
        notes.push(OfferNote::FinishingNotListed);
    }
    let mut refused = ipp_finishings(f, supported).err().unwrap_or_default();
    let values = ipp_finishings(f, None).unwrap_or_default();
    let bins = offer.map_or(&[][..], |o| o.output_bins.as_slice());
    let output_bin = match &f.output_bin {
        None => None,
        Some(want) if bins.is_empty() => {
            notes.push(OfferNote::BinsNotListed {
                asked: want.clone(),
            });
            Some(want.clone())
        }
        Some(want) => match bins.iter().find(|b| b.eq_ignore_ascii_case(want)) {
            Some(b) => Some(b.clone()),
            None => {
                refused.push(format!(
                    "output bin {want:?} (it lists {})",
                    bins.join(", ")
                ));
                None
            }
        },
    };
    CheckedFinishing {
        values,
        output_bin,
        notes,
        refused,
    }
}
