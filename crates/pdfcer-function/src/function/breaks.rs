//! Where a one-input function stops being affine.

use super::{Exponential, Kind, PdfFunction, Sampled, Stitching, clip};

impl PdfFunction {
    /// The inputs between which every output is affine in the one input `t`,
    /// sorted, with the `/Domain` ends first and last; at most `cap` of them.
    ///
    /// A type 0 breaks at each sample position (its `/Encode` mapped back
    /// onto `/Domain`), a type 2 with `/N` 0 or 1 only at the domain ends, a
    /// type 3 at its `/Bounds` and at each sub-function's breaks mapped
    /// through that sub-function's `/Encode`. Wherever a `/Range` clips an
    /// output, the point it starts or stops clipping is a break too. A
    /// function can jump at a break (a type 3 bound); outside the domain it
    /// is constant.
    ///
    /// `None` when no such list exists or it is longer than `cap`: more than
    /// one input, a type 2 with any other `/N`, a type 4, or a type 3 with
    /// such a sub-function. A caller that needs an exact answer takes `None`
    /// as "sample it some other way".
    #[must_use]
    pub fn affine_breaks(&self, cap: usize) -> Option<Vec<f64>> {
        let &[[d0, d1]] = self.domain.as_slice() else {
            return None;
        };
        let mut breaks = match &self.kind {
            Kind::Sampled(s) => s.breaks(d0, d1, cap)?,
            Kind::Exponential(e) => e.breaks()?,
            Kind::Stitching(s) => s.breaks(d0, d1, cap)?,
            Kind::PostScript(_) => return None,
        };
        breaks.extend([d0, d1]);
        breaks.retain(|t| t.is_finite() && (d0..=d1).contains(t));
        sort_dedup(&mut breaks);
        if let Some(range) = self.range.as_deref() {
            let crossings = self.range_crossings(&breaks, range)?;
            breaks.extend(crossings);
            sort_dedup(&mut breaks);
        }
        (breaks.len() <= cap).then_some(breaks)
    }

    /// Where an output, affine between neighbouring `breaks` before the
    /// `/Range` clip, crosses a `/Range` end.
    fn range_crossings(&self, breaks: &[f64], range: &[[f64; 2]]) -> Option<Vec<f64>> {
        let mut out = Vec::new();
        for (&a, &b) in breaks.iter().zip(breaks.iter().skip(1)) {
            let (fa, fb) = (self.unclipped(a, b)?, self.unclipped(b, a)?);
            for ((&ya, &yb), &[lo, hi]) in fa.iter().zip(&fb).zip(range) {
                for edge in [lo, hi] {
                    if (ya - edge) * (yb - edge) < 0.0 {
                        out.push(a + (b - a) * (edge - ya) / (yb - ya));
                    }
                }
            }
        }
        Some(out)
    }

    /// The outputs at `t` before the `/Range` clip, taken from the side of
    /// `t` facing `toward` (a type 3 can jump at `t`).
    fn unclipped(&self, t: f64, toward: f64) -> Option<Vec<f64>> {
        let [d0, d1] = *self.domain.first()?;
        let x = clip(
            if toward < t {
                t.next_down().max(toward)
            } else {
                t
            },
            d0,
            d1,
        );
        let mut out = Vec::new();
        match &self.kind {
            Kind::Sampled(s) => s.eval(&self.domain, &[x], &mut out).ok()?,
            Kind::Exponential(e) => e.eval(&[x], &mut out),
            Kind::Stitching(s) => s.eval(&self.domain, &[x], &mut out).ok()?,
            Kind::PostScript(_) => return None,
        }
        Some(out)
    }
}

impl Sampled {
    /// Each sample position mapped back from `/Encode` onto `[d0, d1]`.
    fn breaks(&self, d0: f64, d1: f64, cap: usize) -> Option<Vec<f64>> {
        let (&[size], &[[e0, e1]]) = (self.size.as_slice(), self.encode.as_slice()) else {
            return None;
        };
        if size > cap {
            return None;
        }
        if e1 == e0 || d1 <= d0 {
            return Some(Vec::new());
        }
        #[allow(clippy::cast_precision_loss)] // size <= cap, a table length
        let at = |i: usize| d0 + (i as f64 - e0) * (d1 - d0) / (e1 - e0);
        Some((0..size).map(at).collect())
    }
}

impl Exponential {
    /// Affine only for `/N` 0 or 1.
    fn breaks(&self) -> Option<Vec<f64>> {
        (self.n == 0.0 || self.n == 1.0).then(Vec::new)
    }
}

impl Stitching {
    /// `/Bounds`, plus each sub-function's breaks mapped onto its
    /// sub-domain.
    fn breaks(&self, d0: f64, d1: f64, cap: usize) -> Option<Vec<f64>> {
        let starts = std::iter::once(d0).chain(self.bounds.iter().copied());
        let ends = self.bounds.iter().copied().chain(std::iter::once(d1));
        let mut out: Vec<f64> = self.bounds.clone();
        for (((lo, hi), &[a, b]), f) in starts.zip(ends).zip(&self.encode).zip(&self.functions) {
            out.extend([lo, hi]);
            if b == a || hi <= lo {
                continue;
            }
            for c in f.affine_breaks(cap)? {
                out.push(lo + (c - a) * (hi - lo) / (b - a));
            }
            if out.len() > cap.saturating_mul(4) {
                return None;
            }
        }
        Some(out)
    }
}

fn sort_dedup(v: &mut Vec<f64>) {
    v.sort_by(f64::total_cmp);
    v.dedup();
}
