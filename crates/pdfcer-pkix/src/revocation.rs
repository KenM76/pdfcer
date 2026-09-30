//! What a set of CRLs (RFC 5280 §5) and OCSP responses (RFC 6960) says about
//! a whole certificate chain.
//!
//! Each certificate from the signer up to, not including, a self-issued root
//! or a caller-named stop certificate must be covered by a usable CRL or OCSP
//! response saying it is not revoked. Any usable answer saying it IS revoked
//! wins over every "good" answer. Every doubt resolves to
//! [`ChainRevocation::Undetermined`].

use crate::cms::{self, Certificate};
use crate::crl::{self, Crl, CrlReason, CrlStatus};
use crate::ocsp::{self, OcspResponse, OcspStatus};
use crate::trust_chain::verify_cert_signature;

/// The deepest certificate chain walked.
const MAX_DEPTH: usize = 16;

/// Which piece of the caller's evidence answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    /// An index into the caller's CRLs.
    Crl(usize),
    /// An index into the caller's OCSP responses.
    Ocsp(usize),
}

/// One chain certificate the evidence showed was not revoked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertCoverage {
    /// The certificate's subject.
    pub subject: String,
    /// Which CRL or OCSP response answered — the newest usable one.
    pub evidence: Evidence,
    /// Its `thisUpdate`.
    pub this_update: Option<String>,
    /// Its `nextUpdate`; `None` when it states none.
    pub next_update: Option<String>,
}

/// What the supplied evidence says about a whole chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainRevocation {
    /// Every certificate below the root was covered by a usable CRL or OCSP
    /// response that does not show it revoked; signer first.
    NotRevoked(Vec<CertCoverage>),
    /// A usable CRL or OCSP response shows a chain certificate revoked.
    Revoked {
        subject: String,
        evidence: Evidence,
        date: Option<String>,
        reason: Option<CrlReason>,
        /// Whether `date` is at or before `at`; `None` when either is unknown.
        before: Option<bool>,
    },
    /// The chain could not be fully checked, and why.
    Undetermined { reason: String },
}

/// One usable answer for one certificate.
enum Answer {
    Good {
        this_update: Option<String>,
        next_update: Option<String>,
    },
    Revoked {
        date: Option<String>,
        reason: Option<CrlReason>,
    },
    Unusable(String),
}

/// Check every certificate from `signer_der` up to (not including) a
/// self-issued root or a certificate in `stop_at`, against `crls` and
/// `ocsps` (DER `OCSPResponse`s or bare `BasicOCSPResponse`s).
///
/// Issuers are found in `pool` by name and verified by signature. `at` is
/// the reference time (for a signature, its signing time): evidence must be
/// current at it, and a revocation is reported as before or after it.
#[must_use]
pub fn chain_status(
    signer_der: &[u8],
    pool: &[&[u8]],
    stop_at: &[&[u8]],
    crls: &[&[u8]],
    ocsps: &[&[u8]],
    at: Option<&str>,
) -> ChainRevocation {
    let undetermined = |reason: String| ChainRevocation::Undetermined { reason };
    let Some(mut current) = cms::parse_certificate(signer_der) else {
        return undetermined("the signer's certificate could not be read".to_owned());
    };
    let mut current_der = signer_der;
    let pool: Vec<(&[u8], Certificate<'_>)> = pool
        .iter()
        .filter_map(|d| cms::parse_certificate(d).map(|c| (*d, c)))
        .collect();
    let parsed_crls: Vec<Option<Crl<'_>>> = crls.iter().map(|d| crl::parse_crl(d)).collect();
    let usable_crls: Vec<Crl<'_>> = parsed_crls.iter().flatten().cloned().collect();
    let parsed_ocsps: Vec<Option<OcspResponse<'_>>> =
        ocsps.iter().map(|d| ocsp::parse_ocsp(d)).collect();
    let mut covered = Vec::new();
    for _ in 0..MAX_DEPTH {
        if stop_at.contains(&current_der)
            || (current.subject_der == current.issuer_der
                && verify_cert_signature(&current, &current.key))
        {
            return ChainRevocation::NotRevoked(covered);
        }
        let issuers: Vec<&(&[u8], Certificate<'_>)> = pool
            .iter()
            .filter(|(_, c)| {
                c.subject_der == current.issuer_der && verify_cert_signature(&current, &c.key)
            })
            .collect();
        let Some((issuer_der, issuer)) = issuers.first().map(|(d, c)| (*d, c)) else {
            return undetermined(format!(
                "the issuer of {} is not available, so its revocation cannot be checked",
                current.subject
            ));
        };
        let mut answers: Vec<(Evidence, Answer)> = Vec::new();
        for (i, c) in parsed_crls.iter().enumerate() {
            let Some(c) = c else { continue };
            if c.issuer_der != current.issuer_der {
                continue;
            }
            for (_, candidate) in &issuers {
                let a = match crl::check_crl(c, &current, candidate, at) {
                    CrlStatus::NotRevoked => Answer::Good {
                        this_update: c.this_update.clone(),
                        next_update: c.next_update.clone(),
                    },
                    CrlStatus::Revoked { date, reason } => Answer::Revoked { date, reason },
                    CrlStatus::Unusable { reason } => Answer::Unusable(format!(
                        "the CRL for {} cannot be used: {reason}",
                        current.subject
                    )),
                };
                answers.push((Evidence::Crl(i), a));
            }
        }
        for (i, r) in parsed_ocsps.iter().enumerate() {
            let Some(r) = r else { continue };
            for (_, candidate) in &issuers {
                let a = match ocsp::check_ocsp(r, &current, candidate, at, &usable_crls) {
                    OcspStatus::NotRevoked {
                        this_update,
                        next_update,
                    } => Answer::Good {
                        this_update,
                        next_update,
                    },
                    OcspStatus::Revoked { date, reason } => Answer::Revoked { date, reason },
                    OcspStatus::Unusable { reason } => Answer::Unusable(format!(
                        "the OCSP response for {} cannot be used: {reason}",
                        current.subject
                    )),
                };
                answers.push((Evidence::Ocsp(i), a));
            }
        }
        let mut good: Option<CertCoverage> = None;
        let mut why_not: Option<String> = None;
        for (evidence, a) in answers {
            match a {
                Answer::Revoked { date, reason } => {
                    let before = match (date.as_deref(), at) {
                        (Some(d), Some(a)) => Some(d <= a),
                        _ => None,
                    };
                    return ChainRevocation::Revoked {
                        subject: current.subject.clone(),
                        evidence,
                        date,
                        reason,
                        before,
                    };
                }
                Answer::Good {
                    this_update,
                    next_update,
                } => {
                    if good.as_ref().is_none_or(|g| this_update > g.this_update) {
                        good = Some(CertCoverage {
                            subject: current.subject.clone(),
                            evidence,
                            this_update,
                            next_update,
                        });
                    }
                }
                Answer::Unusable(reason) => {
                    // An OCSP response that simply is not about this
                    // certificate says nothing worth reporting over a CRL's
                    // reason; keep the first specific one.
                    let vague = |r: &str| r.contains("has no answer for");
                    if why_not
                        .as_deref()
                        .is_none_or(|w| vague(w) && !vague(&reason))
                    {
                        why_not = Some(reason);
                    }
                }
            }
        }
        match good {
            Some(g) => covered.push(g),
            None => {
                return undetermined(why_not.unwrap_or_else(|| {
                    format!(
                        "no CRL or OCSP response from {} was available",
                        current.issuer
                    )
                }));
            }
        }
        current_der = issuer_der;
        current = issuer.clone();
    }
    undetermined(format!("the chain is deeper than {MAX_DEPTH} certificates"))
}
