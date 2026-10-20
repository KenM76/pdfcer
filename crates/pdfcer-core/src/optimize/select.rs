//! Rule selectors: which objects a [`Rule`](super::Rule) covers.

use std::fmt;

use crate::object::ObjId;
use crate::space_audit::{ObjectSpace, SpaceCategory};

/// The facts a selector tests about one object.
pub(crate) struct ObjectFacts<'a> {
    /// The object's space-audit row; `None` for an object created in this
    /// session, which only id and size constraints can match.
    pub(crate) audit: Option<&'a ObjectSpace>,
    /// Stored stream bytes.
    pub(crate) stored_bytes: u64,
    /// The filter chain, in order.
    pub(crate) filters: &'a [String],
}

/// 1-based inclusive page ranges, such as `1-3,7`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PageRanges(pub Vec<(usize, usize)>);

impl PageRanges {
    /// Parse `1-3,7,10-` (an open end runs to the last page).
    ///
    /// # Errors
    ///
    /// A message naming the bad piece.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut out = Vec::new();
        for piece in text.split(',').map(str::trim) {
            let bad = || format!("bad page range {piece:?}");
            let (lo, hi) = match piece.split_once('-') {
                Some((lo, "")) => (lo.trim().parse().map_err(|_| bad())?, usize::MAX),
                Some((lo, hi)) => (
                    lo.trim().parse().map_err(|_| bad())?,
                    hi.trim().parse().map_err(|_| bad())?,
                ),
                None => {
                    let n = piece.parse().map_err(|_| bad())?;
                    (n, n)
                }
            };
            if lo == 0 || hi < lo {
                return Err(bad());
            }
            out.push((lo, hi));
        }
        Ok(Self(out))
    }

    /// Whether `page` (1-based) is in a range.
    #[must_use]
    pub fn contains(&self, page: usize) -> bool {
        self.0.iter().any(|&(lo, hi)| (lo..=hi).contains(&page))
    }
}

impl fmt::Display for PageRanges {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, &(lo, hi)) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            match hi {
                usize::MAX => write!(f, "{lo}-")?,
                _ if hi == lo => write!(f, "{lo}")?,
                _ => write!(f, "{lo}-{hi}")?,
            }
        }
        Ok(())
    }
}

/// Which objects a rule covers. Every constraint that is set must hold; an
/// empty list or `None` places no constraint, so the default matches all.
///
/// A constraint on something an object does not have fails: a `pages`
/// constraint fails on an object no page uses, and a ppi or colour-space
/// constraint fails on anything that is not an image.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Selector {
    /// Space-audit categories.
    pub categories: Vec<SpaceCategory>,
    /// Pages, matched when any page that uses the object is in range.
    pub pages: Option<PageRanges>,
    /// Smallest stored stream size, in bytes.
    pub min_bytes: Option<u64>,
    /// Largest stored stream size, in bytes.
    pub max_bytes: Option<u64>,
    /// Filter names such as `FlateDecode`; `none` matches an unfiltered
    /// stream. Matched when any filter in the chain is listed.
    pub filters: Vec<String>,
    /// Image colour-space names, such as `DeviceRGB` or `ICCBased`.
    pub color_spaces: Vec<String>,
    /// Lowest maximum effective image resolution, in ppi.
    pub min_ppi: Option<f64>,
    /// Highest maximum effective image resolution, in ppi.
    pub max_ppi: Option<f64>,
    /// Object ids.
    pub ids: Vec<ObjId>,
}

impl Selector {
    /// A selector that matches everything.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the selector covers the object.
    pub(crate) fn matches(&self, id: ObjId, facts: &ObjectFacts<'_>) -> bool {
        let audit = facts.audit;
        let image = audit.and_then(|a| a.image.as_ref());
        (self.ids.is_empty() || self.ids.contains(&id))
            && self.min_bytes.is_none_or(|m| facts.stored_bytes >= m)
            && self.max_bytes.is_none_or(|m| facts.stored_bytes <= m)
            && (self.categories.is_empty()
                || audit.is_some_and(|a| self.categories.contains(&a.category)))
            && self
                .pages
                .as_ref()
                .is_none_or(|r| audit.is_some_and(|a| a.pages.iter().any(|p| r.contains(*p))))
            && (self.filters.is_empty() || self.filter_matches(facts.filters))
            && (self.color_spaces.is_empty()
                || image
                    .and_then(|i| i.color_space.as_deref())
                    .is_some_and(|cs| self.color_spaces.iter().any(|c| c == cs)))
            && self.ppi_matches(image.and_then(crate::space_audit::ImageSpace::max_effective_ppi))
    }

    fn filter_matches(&self, chain: &[String]) -> bool {
        if chain.is_empty() {
            return self.filters.iter().any(|f| f == "none");
        }
        chain.iter().any(|c| self.filters.contains(c))
    }

    fn ppi_matches(&self, ppi: Option<f64>) -> bool {
        if self.min_ppi.is_none() && self.max_ppi.is_none() {
            return true;
        }
        ppi.is_some_and(|p| {
            self.min_ppi.is_none_or(|m| p >= m) && self.max_ppi.is_none_or(|m| p <= m)
        })
    }
}
