//! PAdES B-LT validation material: the certificates, CRLs and OCSP responses
//! [`crate::edit::EditSession::add_validation_material`] writes into the
//! catalog's `/DSS` (ETSI EN 319 142-1 §5.4.2.2; ISO 32000-2 §12.8.4.3).
//!
//! pdfcer-core fetches nothing: the caller supplies DER bytes, and every blob
//! is parsed before anything is written. No `/VRI` is written — ETSI EN
//! 319 142-1 §6.3 requirement v) says the B-LT/B-LTA VRI "should not be
//! used". Spec digest: `pades/pades__ref__dss_vri.md` in the spec RAG.

use pdfcer_pkix::asn1;

use super::der_out;

/// Caller-supplied validation material for a document's `/DSS`.
///
/// ```
/// use pdfcer_core::sign::ltv::ValidationMaterial;
/// # let (crl_der, ocsp_der) = (Vec::new(), Vec::new());
/// let material = ValidationMaterial::new()
///     .with_crl(crl_der)
///     .with_ocsp(ocsp_der);
/// assert!(material.includes_signature_certificates());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationMaterial {
    certs: Vec<Vec<u8>>,
    crls: Vec<Vec<u8>>,
    ocsps: Vec<Vec<u8>>,
    signature_certificates: bool,
    allow_under_p1: bool,
}

impl Default for ValidationMaterial {
    fn default() -> Self {
        Self {
            certs: Vec::new(),
            crls: Vec::new(),
            ocsps: Vec::new(),
            signature_certificates: true,
            allow_under_p1: false,
        }
    }
}

impl ValidationMaterial {
    /// Nothing supplied; the signatures' own certificates are included.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one DER X.509 certificate (RFC 5280 §4.1).
    #[must_use]
    pub fn with_cert(mut self, der: impl Into<Vec<u8>>) -> Self {
        self.certs.push(der.into());
        self
    }

    /// Add one DER `CertificateList` (RFC 5280 §5.1).
    #[must_use]
    pub fn with_crl(mut self, der: impl Into<Vec<u8>>) -> Self {
        self.crls.push(der.into());
        self
    }

    /// Add one DER `OCSPResponse` (RFC 6960 §4.2.1). A bare
    /// `BasicOCSPResponse` is accepted and wrapped into an `OCSPResponse`,
    /// the encoding ETSI EN 319 142-1 §5.4.2.2 requires in `/OCSPs`.
    #[must_use]
    pub fn with_ocsp(mut self, der: impl Into<Vec<u8>>) -> Self {
        self.ocsps.push(der.into());
        self
    }

    /// Whether every certificate in every signature's CMS also goes into
    /// `/Certs` (ISO 32000-2 §12.8.4.3: the array holds the whole chain).
    /// Default `true`.
    #[must_use]
    pub fn include_signature_certificates(mut self, include: bool) -> Self {
        self.signature_certificates = include;
        self
    }

    /// Whether the signatures' own certificates are included.
    #[must_use]
    pub fn includes_signature_certificates(&self) -> bool {
        self.signature_certificates
    }

    /// Permit the write under a certification that allows no changes
    /// (`/P 1`). The spec exempts a DSS increment from DocMDP (ETSI EN
    /// 319 142-1 §5.4.2.3; ISO 32000-2 Table 257), but Acrobat is reported
    /// to treat one as a change under `/P 1`, so the default refuses.
    #[must_use]
    pub fn allow_under_no_changes_certification(mut self, allow: bool) -> Self {
        self.allow_under_p1 = allow;
        self
    }

    /// Whether [`Self::allow_under_no_changes_certification`] was set.
    #[must_use]
    pub fn allows_under_no_changes_certification(&self) -> bool {
        self.allow_under_p1
    }

    /// The supplied blobs, by kind, in the order given.
    pub(crate) fn supplied(&self) -> [(MaterialKind, &[Vec<u8>]); 3] {
        [
            (MaterialKind::Certificate, &self.certs),
            (MaterialKind::Crl, &self.crls),
            (MaterialKind::Ocsp, &self.ocsps),
        ]
    }
}

/// Which `/DSS` array a blob belongs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MaterialKind {
    /// `/Certs`.
    Certificate,
    /// `/CRLs`.
    Crl,
    /// `/OCSPs`.
    Ocsp,
}

impl MaterialKind {
    /// `"certificate"`, `"CRL"` or `"OCSP response"`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Certificate => "certificate",
            Self::Crl => "CRL",
            Self::Ocsp => "OCSP response",
        }
    }

    /// The `/DSS` key: `Certs`, `CRLs` or `OCSPs`.
    #[must_use]
    pub fn dss_key(self) -> &'static [u8] {
        match self {
            Self::Certificate => b"Certs",
            Self::Crl => b"CRLs",
            Self::Ocsp => b"OCSPs",
        }
    }
}

/// What [`crate::edit::EditSession::add_validation_material`] wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct DssReport {
    /// New streams in `/Certs`.
    pub certs_added: usize,
    /// New streams in `/CRLs`.
    pub crls_added: usize,
    /// New streams in `/OCSPs`.
    pub ocsps_added: usize,
    /// Of `certs_added`, how many came from the signatures' own CMS.
    pub signature_certificates_added: usize,
    /// Blobs byte-identical to one already in `/DSS` or earlier in the input.
    pub duplicates_skipped: usize,
    /// Bare `BasicOCSPResponse`s wrapped into an `OCSPResponse`.
    pub ocsps_wrapped: usize,
    /// Entries carried forward from the previous `/DSS`, all kinds.
    pub carried_forward: usize,
}

impl DssReport {
    /// Whether nothing new was written.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.certs_added + self.crls_added + self.ocsps_added == 0
    }
}

/// `id-pkix-ocsp-basic` (RFC 6960 §4.2.1).
const OCSP_BASIC: &str = "1.3.6.1.5.5.7.48.1.1";

/// Check one supplied blob; return the bytes to store and whether it was
/// wrapped. `Err` carries the reason it cannot be stored.
pub(crate) fn check_blob(kind: MaterialKind, der: &[u8]) -> Result<(Vec<u8>, bool), String> {
    let whole = match asn1::expect(der, asn1::SEQUENCE) {
        Some((_, [])) => true,
        Some(_) => false,
        None => return Err("it is not a DER SEQUENCE".to_owned()),
    };
    if !whole {
        return Err("bytes follow the DER object".to_owned());
    }
    match kind {
        MaterialKind::Certificate => pdfcer_pkix::cms::parse_certificate(der)
            .map(|_| (der.to_vec(), false))
            .ok_or_else(|| "it does not parse as an X.509 certificate".to_owned()),
        MaterialKind::Crl => pdfcer_pkix::crl::parse_crl(der)
            .map(|_| (der.to_vec(), false))
            .ok_or_else(|| "it does not parse as a CRL".to_owned()),
        MaterialKind::Ocsp => {
            let parsed = pdfcer_pkix::ocsp::parse_ocsp(der)
                .ok_or_else(|| "it does not parse as an OCSP response".to_owned())?;
            // `parse_ocsp` records a non-`successful` responseStatus as a defect.
            if let Some(defect) = parsed.defect {
                return Err(defect);
            }
            let bare = asn1::expect(der, asn1::SEQUENCE)
                .and_then(|(outer, _)| asn1::read(outer.content))
                .is_some_and(|(first, _)| first.tag == asn1::SEQUENCE);
            if bare {
                Ok((wrap_basic(der), true))
            } else {
                Ok((der.to_vec(), false))
            }
        }
    }
}

/// `OCSPResponse { successful, responseBytes { id-pkix-ocsp-basic, basic } }`
/// (RFC 6960 §4.2.1).
fn wrap_basic(basic: &[u8]) -> Vec<u8> {
    let oid = der_out::oid(OCSP_BASIC).unwrap_or_default();
    let response_bytes = der_out::sequence(&[oid, der_out::octet_string(basic)]);
    der_out::sequence(&[
        der_out::tlv(0x0A, &[0]),
        der_out::context(0, &response_bytes),
    ])
}
