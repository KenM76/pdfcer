//! Certificate revocation lists (RFC 5280 §5): read one, and decide what it
//! says about a certificate. The chain walk is in `revocation`.
//!
//! Offline only: the CRLs come from the document's `/DSS` or from a caller
//! that fetched them. Every doubt resolves to
//! [`CrlStatus::Unusable`], never to a false "not revoked":
//!
//! - A CRL must be signed by the certificate's issuer (same name, a key that
//!   verifies the CRL signature) whose `keyUsage`, if a v3 certificate,
//!   asserts `cRLSign` (RFC 10007 §6.3.3(f)).
//! - A delta CRL, an indirect CRL, a CRL scoped to other reasons or
//!   attribute certificates, or one carrying an unknown critical CRL or
//!   entry extension is not used (§5.2, §5.3).
//! - An `issuingDistributionPoint` naming distribution points is used only
//!   when one of its URIs is among the certificate's own
//!   `cRLDistributionPoints` URIs (§6.3.3(b)(2)(i)); the only-user and
//!   only-CA scopes are honoured.
//! - A CRL whose `nextUpdate` is before the reference time is not used.
//!   A missing `nextUpdate` (the standard is silent on its meaning) is
//!   accepted and reported by [`CertCoverage::next_update`] being `None`.

use crate::asn1::{self, Tlv};
use crate::cms::{self, AlgId, Certificate};
use crate::trust_chain::verify_signed;

/// The most entries one CRL may list before it is refused as unusable.
pub const MAX_CRL_ENTRIES: usize = 1_000_000;

mod ext {
    pub const CRL_NUMBER: &str = "2.5.29.20";
    pub const DELTA_CRL_INDICATOR: &str = "2.5.29.27";
    pub const ISSUING_DISTRIBUTION_POINT: &str = "2.5.29.28";
    pub const AUTHORITY_KEY_ID: &str = "2.5.29.35";
    pub const ISSUER_ALT_NAME: &str = "2.5.29.18";
    pub const FRESHEST_CRL: &str = "2.5.29.46";
    pub const REASON_CODE: &str = "2.5.29.21";
    pub const INVALIDITY_DATE: &str = "2.5.29.24";
    pub const CERTIFICATE_ISSUER: &str = "2.5.29.29";
}

/// `CRLReason` (RFC 5280 §5.3.1). Value 7 is unused by the standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrlReason {
    Unspecified,
    KeyCompromise,
    CaCompromise,
    AffiliationChanged,
    Superseded,
    CessationOfOperation,
    CertificateHold,
    RemoveFromCrl,
    PrivilegeWithdrawn,
    AaCompromise,
}

impl CrlReason {
    /// The reason a `CRLReason` ENUMERATED value names; `None` for 7 or
    /// anything above 10.
    pub(crate) fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => Self::Unspecified,
            1 => Self::KeyCompromise,
            2 => Self::CaCompromise,
            3 => Self::AffiliationChanged,
            4 => Self::Superseded,
            5 => Self::CessationOfOperation,
            6 => Self::CertificateHold,
            8 => Self::RemoveFromCrl,
            9 => Self::PrivilegeWithdrawn,
            10 => Self::AaCompromise,
            _ => return None,
        })
    }

    /// The RFC 5280 ASN.1 name (`keyCompromise`, …).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::KeyCompromise => "keyCompromise",
            Self::CaCompromise => "cACompromise",
            Self::AffiliationChanged => "affiliationChanged",
            Self::Superseded => "superseded",
            Self::CessationOfOperation => "cessationOfOperation",
            Self::CertificateHold => "certificateHold",
            Self::RemoveFromCrl => "removeFromCRL",
            Self::PrivilegeWithdrawn => "privilegeWithdrawn",
            Self::AaCompromise => "aACompromise",
        }
    }
}

/// One `revokedCertificates` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrlEntry<'a> {
    /// The serial number, as [`asn1::integer_bytes`] strips it.
    pub serial: &'a [u8],
    /// `revocationDate`, ISO-8601.
    pub revocation_date: Option<String>,
    /// The `reasonCode` entry extension, if present.
    pub reason: Option<CrlReason>,
}

/// A parsed `CertificateList`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crl<'a> {
    /// The issuer name, `CN=…` form.
    pub issuer: String,
    /// The issuer `Name`'s raw DER.
    pub issuer_der: &'a [u8],
    /// `thisUpdate`, ISO-8601.
    pub this_update: Option<String>,
    /// `nextUpdate`, ISO-8601; `None` when absent.
    pub next_update: Option<String>,
    /// Every revoked certificate listed.
    pub entries: Vec<CrlEntry<'a>>,
    /// The raw `tbsCertList` — what the issuer signed.
    pub tbs_der: &'a [u8],
    /// The outer `signatureAlgorithm`.
    pub sig_alg: Option<AlgId<'a>>,
    /// The `signatureValue` BIT STRING contents.
    pub sig_value: &'a [u8],
    /// Why this CRL cannot be used whatever its signature, if it cannot.
    pub defect: Option<String>,
    /// The `issuingDistributionPoint` scope, if the extension is present.
    pub scope: Option<CrlScope>,
}

/// What an `issuingDistributionPoint` (RFC 5280 §5.2.5) limits a CRL to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CrlScope {
    /// The `distributionPoint` `fullName` URIs; empty when absent.
    pub uris: Vec<String>,
    /// `true` when a distribution point name is present at all (URIs or not).
    pub has_point_name: bool,
    pub only_user_certs: bool,
    pub only_ca_certs: bool,
}

/// Parse a DER `CertificateList`. `None` when it is not one.
#[must_use]
pub fn parse_crl(der: &[u8]) -> Option<Crl<'_>> {
    let (outer, _) = asn1::expect(der, asn1::SEQUENCE)?;
    let kids = asn1::children(outer)?;
    let tbs = kids.first().filter(|t| t.tag == asn1::SEQUENCE)?;
    let sig_alg = kids.get(1).and_then(|t| cms::alg_id(*t));
    let sig_value = kids
        .get(2)
        .and_then(|t| asn1::bit_string_bytes(*t))
        .unwrap_or(&[]);
    let mut fields = asn1::children(*tbs)?.into_iter().peekable();
    // version INTEGER OPTIONAL — untagged, unlike a certificate's [0].
    let version = fields.next_if(|t| t.tag == asn1::INTEGER);
    let _inner_alg = fields.next().filter(|t| t.tag == asn1::SEQUENCE)?;
    let issuer = fields.next().filter(|t| t.tag == asn1::SEQUENCE)?;
    let this_update = asn1::time_value(fields.next()?);
    let next_update = fields
        .next_if(|t| matches!(t.tag, asn1::UTC_TIME | asn1::GENERALIZED_TIME))
        .and_then(asn1::time_value);
    let mut crl = Crl {
        issuer: cms::name_to_string(issuer),
        issuer_der: issuer.raw,
        this_update,
        next_update,
        entries: Vec::new(),
        tbs_der: tbs.raw,
        sig_alg,
        sig_value,
        defect: None,
        scope: None,
    };
    let is_v2 = version.is_some_and(|v| v.content == [1]);
    if let Some(list) = fields.next_if(|t| t.tag == asn1::SEQUENCE) {
        read_entries(list, &mut crl);
    }
    if let Some(exts) = fields.next_if(|t| t.tag == asn1::context(0)) {
        if !is_v2 {
            crl.defect
                .get_or_insert_with(|| "it carries extensions but is not a v2 CRL".to_owned());
        }
        let (seq, _) = asn1::expect(exts.content, asn1::SEQUENCE)?;
        for e in asn1::children(seq)? {
            read_crl_extension(e, &mut crl);
        }
    }
    if fields.next().is_some() {
        crl.defect
            .get_or_insert_with(|| "it has fields after crlExtensions".to_owned());
    }
    Some(crl)
}

/// An `Extension` as (OID, critical, extnValue contents).
fn extension(e: Tlv<'_>) -> Option<(String, bool, &[u8])> {
    let parts = asn1::children(e)?;
    let oid = asn1::oid_to_string(parts.first().filter(|t| t.tag == asn1::OID)?.content)?;
    let critical = parts
        .get(1)
        .filter(|t| t.tag == asn1::BOOLEAN)
        .is_some_and(|t| t.content == [0xFF]);
    let value = parts
        .last()
        .filter(|t| t.tag == asn1::OCTET_STRING)?
        .content;
    Some((oid, critical, value))
}

fn read_entries<'a>(list: Tlv<'a>, crl: &mut Crl<'a>) {
    let mut rest = list.content;
    while !rest.is_empty() {
        if crl.entries.len() >= MAX_CRL_ENTRIES {
            crl.defect = Some(format!("it lists more than {MAX_CRL_ENTRIES} entries"));
            return;
        }
        let Some((entry, r)) = asn1::expect(rest, asn1::SEQUENCE) else {
            crl.defect = Some("a revoked-certificate entry is malformed".to_owned());
            return;
        };
        rest = r;
        let parts = asn1::children(entry).unwrap_or_default();
        let Some(serial) = parts.first().and_then(|t| asn1::integer_bytes(*t)) else {
            crl.defect = Some("a revoked-certificate entry has no serial".to_owned());
            return;
        };
        let mut out = CrlEntry {
            serial,
            revocation_date: parts.get(1).and_then(|t| asn1::time_value(*t)),
            reason: None,
        };
        if let Some(exts) = parts.get(2).filter(|t| t.tag == asn1::SEQUENCE) {
            for e in asn1::children(*exts).unwrap_or_default() {
                let Some((oid, critical, value)) = extension(e) else {
                    crl.defect = Some("an entry extension is malformed".to_owned());
                    continue;
                };
                match oid.as_str() {
                    ext::REASON_CODE => {
                        // extnValue wraps ENUMERATED (tag 0x0A).
                        let code = asn1::expect(value, 0x0A)
                            .and_then(|(t, _)| (t.content.len() == 1).then(|| t.content.first()))
                            .flatten()
                            .and_then(|c| CrlReason::from_code(*c));
                        if code.is_none() {
                            crl.defect =
                                Some("an entry's reasonCode is not a known CRLReason".to_owned());
                        }
                        out.reason = code;
                    }
                    ext::INVALIDITY_DATE => {}
                    ext::CERTIFICATE_ISSUER => {
                        crl.defect = Some(
                            "it is an indirect CRL (a certificateIssuer entry extension); not supported"
                                .to_owned(),
                        );
                    }
                    _ if critical => {
                        crl.defect = Some(format!(
                            "an entry carries the unrecognised critical extension {oid}"
                        ));
                    }
                    _ => {}
                }
            }
        }
        if out.reason == Some(CrlReason::RemoveFromCrl) {
            crl.defect =
                Some("a complete CRL lists a removeFromCRL entry (delta CRLs only)".to_owned());
        }
        crl.entries.push(out);
    }
}

fn read_crl_extension(e: Tlv<'_>, crl: &mut Crl<'_>) {
    let Some((oid, critical, value)) = extension(e) else {
        crl.defect = Some("a CRL extension is malformed".to_owned());
        return;
    };
    match oid.as_str() {
        ext::CRL_NUMBER | ext::AUTHORITY_KEY_ID | ext::ISSUER_ALT_NAME | ext::FRESHEST_CRL => {}
        ext::DELTA_CRL_INDICATOR => {
            crl.defect = Some("it is a delta CRL; only complete CRLs are used".to_owned());
        }
        ext::ISSUING_DISTRIBUTION_POINT => match read_scope(value) {
            Ok(scope) => crl.scope = Some(scope),
            Err(why) => crl.defect = Some(why),
        },
        _ if critical => {
            crl.defect = Some(format!(
                "it carries the unrecognised critical extension {oid}"
            ));
        }
        _ => {}
    }
}

/// `IssuingDistributionPoint ::= SEQUENCE { distributionPoint [0],
/// onlyContainsUserCerts [1], onlyContainsCACerts [2], onlySomeReasons [3],
/// indirectCRL [4], onlyContainsAttributeCerts [5] }`, all IMPLICIT.
fn read_scope(value: &[u8]) -> Result<CrlScope, String> {
    let malformed = || "its issuingDistributionPoint is malformed".to_owned();
    let (seq, _) = asn1::expect(value, asn1::SEQUENCE).ok_or_else(malformed)?;
    let mut scope = CrlScope::default();
    let is_true = |t: &Tlv<'_>| t.content == [0xFF];
    for f in asn1::children(seq).ok_or_else(malformed)? {
        match f.tag {
            0xA0 => {
                scope.has_point_name = true;
                // [0] fullName GeneralNames; URIs are [6] IMPLICIT IA5String.
                if let Some((full, _)) = asn1::read(f.content)
                    && full.tag == 0xA0
                {
                    for name in asn1::children(full).unwrap_or_default() {
                        if name.tag == 0x86
                            && let Ok(uri) = std::str::from_utf8(name.content)
                        {
                            scope.uris.push(uri.to_owned());
                        }
                    }
                }
            }
            0x81 => scope.only_user_certs = is_true(&f),
            0x82 => scope.only_ca_certs = is_true(&f),
            0x83 => return Err("it covers only some revocation reasons".to_owned()),
            0x84 if is_true(&f) => {
                return Err("it is an indirect CRL; not supported".to_owned());
            }
            0x85 if is_true(&f) => {
                return Err("it covers only attribute certificates".to_owned());
            }
            0x84 | 0x85 => {}
            _ => return Err(malformed()),
        }
    }
    Ok(scope)
}

/// What one CRL says about one certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrlStatus {
    /// The CRL is usable and does not list the certificate.
    NotRevoked,
    /// The CRL is usable and lists the certificate.
    Revoked {
        date: Option<String>,
        reason: Option<CrlReason>,
    },
    /// The CRL cannot answer for this certificate, and why.
    Unusable { reason: String },
}

/// Check `cert` against `crl`, which `issuer` (the certificate that issued
/// `cert`) must have signed. `at` is the ISO-8601 reference time the CRL must
/// still be current at; `None` skips that test.
#[must_use]
pub fn check_crl(
    crl: &Crl<'_>,
    cert: &Certificate<'_>,
    issuer: &Certificate<'_>,
    at: Option<&str>,
) -> CrlStatus {
    let unusable = |reason: String| CrlStatus::Unusable { reason };
    if crl.issuer_der != cert.issuer_der || crl.issuer_der != issuer.subject_der {
        return unusable(format!(
            "it was issued by {}, not the certificate's issuer",
            crl.issuer
        ));
    }
    if let Some(defect) = &crl.defect {
        return unusable(defect.clone());
    }
    match issuer.key_usage_crl_sign {
        Some(true) => {}
        Some(false) => {
            return unusable(format!(
                "its issuer {} is not permitted to sign CRLs (keyUsage)",
                issuer.subject
            ));
        }
        None if issuer.version >= 3 => {
            return unusable(format!(
                "its issuer {} has no keyUsage asserting cRLSign (RFC 10007)",
                issuer.subject
            ));
        }
        None => {}
    }
    if !verify_signed(
        crl.tbs_der,
        crl.sig_alg.as_ref(),
        crl.sig_value,
        &issuer.key,
    ) {
        return unusable("its signature does not verify with the issuer's key".to_owned());
    }
    if let Some(scope) = &crl.scope {
        if scope.only_user_certs && cert.is_ca {
            return unusable("it covers only end-entity certificates".to_owned());
        }
        if scope.only_ca_certs && !cert.is_ca {
            return unusable("it covers only CA certificates".to_owned());
        }
        if scope.has_point_name
            && !scope
                .uris
                .iter()
                .any(|u| cert.revocation_uris.crl.contains(u))
        {
            return unusable(
                "it is a partitioned CRL for a distribution point the certificate does not name"
                    .to_owned(),
            );
        }
    }
    if let (Some(at), Some(next)) = (at, crl.next_update.as_deref())
        && next < at
    {
        return unusable(format!("it expired (nextUpdate {next}) before {at}"));
    }
    match crl.entries.iter().find(|e| e.serial == cert.serial) {
        Some(e) => CrlStatus::Revoked {
            date: e.revocation_date.clone(),
            reason: e.reason,
        },
        None => CrlStatus::NotRevoked,
    }
}
