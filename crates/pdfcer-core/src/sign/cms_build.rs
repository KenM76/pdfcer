//! Building the CMS **`SignedData`** that goes into a signature's `/Contents`
//! — RFC 5652 §5 in the SIGN direction, with the CAdES attribute that makes
//! it PAdES (RFC 5035 §5.4.1, ETSI EN 319 142-1 requirement e/f).
//!
//! The parse/verify direction already lives in [`crate::cms`]; this module
//! is written against it as its oracle — every `SignedData` built here is
//! parsed back by `cms::parse_signed_data` and the signed-attributes digest
//! recomputed by `signature_verify`, in the tests.
//!
//! # The shape (RFC 5652 §5.1–§5.3), and the choices pdfcer makes
//!
//! ```text
//! ContentInfo { id-signedData, [0] SignedData {
//!   version 1,                                    -- CB-1: id-data + issuerAndSerialNumber ⇒ v1
//!   digestAlgorithms { sha256 },
//!   encapContentInfo { id-data },                 -- CB-2: DETACHED, no eContent
//!   certificates [0] { leaf, issuers… },          -- PAdES a): the chain, leaf first
//!   signerInfos { SignerInfo {
//!     version 1, sid issuerAndSerialNumber(leaf),
//!     digestAlgorithm sha256,
//!     signedAttrs [0] { content-type id-data, message-digest, signing-certificate-v2 },
//!     signatureAlgorithm, signature } } } }
//! ```
//!
//! - **No `signing-time` attribute.** PAdES Table 1 says it *shall not be
//!   present* at every level (`PC-3`); the claimed time is the PDF `/M`
//!   entry. For the plain `adbe.pkcs7.detached` SubFilter it is optional and
//!   pdfcer still omits it — one attribute set, one code path, and `/M` is
//!   where every reader shows the time anyway.
//! - **`signing-certificate-v2` always**, even for `adbe.pkcs7.detached`.
//!   Mandatory for CAdES (`CB-6`); harmless and protective (binds the signer
//!   certificate into the signed data) for PKCS#7. `hashAlgorithm` is the
//!   DEFAULT SHA-256 and is therefore **omitted** under DER; `issuerSerial`
//!   is written (optional, but Acrobat displays it). `CB-7`: RFC 5035's prose
//!   says SHA-1 for `certHash` — that is a copy/paste residue; the ASN.1
//!   DEFAULT is SHA-256 and that is what is hashed here.
//! - **The `0x31` retag (`CB-4`).** The signature is over the DER of the
//!   attributes with a universal `SET OF` tag (`0x31`); the wire
//!   `SignerInfo` carries the same content under `[0] IMPLICIT` (`0xA0`).
//!   [`build`] encodes the attribute set once with [`der_out::set_of`]
//!   (sorted, `0x31`), signs those bytes, then re-tags the same content
//!   octets as `0xA0` for the wire — two tags, one content, by construction.
//! - `crls` omitted (`CB-1` note): revocation material belongs in the PDF
//!   `/DSS` at B-LT, not in the CMS.
//! - `unsignedAttrs` omitted at B-B; a B-T timestamp token is appended there
//!   later without touching the signed bytes.

use super::der_out;
use super::{SignError, SignatureAlgorithm, Signer};

/// Which `/SubFilter` the CMS is being built for. The bytes are identical
/// today (see the module docs on `signing-certificate-v2`); the variant
/// exists so the choice is explicit at the call site and the PDF half can
/// write the matching name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SubFilter {
    /// `ETSI.CAdES.detached` (ISO 32000-2 §12.8.3.4) — PAdES. **The default.**
    #[default]
    EtsiCadesDetached,
    /// `adbe.pkcs7.detached` (ISO 32000-1 §12.8.3.3) — the widest legacy
    /// reader support; still CMS, still detached.
    AdbePkcs7Detached,
}

impl SubFilter {
    /// The `/SubFilter` name bytes.
    #[must_use]
    pub const fn name(self) -> &'static [u8] {
        match self {
            Self::EtsiCadesDetached => b"ETSI.CAdES.detached",
            Self::AdbePkcs7Detached => b"adbe.pkcs7.detached",
        }
    }
}

/// Why a `SignedData` could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CmsBuildError {
    /// The signer offered no certificate at all — there is nothing to name
    /// as `sid` and nothing to bind in `signing-certificate-v2`.
    #[error(
        "the signer has no certificate chain; a CMS signature needs at least the signer's own certificate"
    )]
    NoCertificate,
    /// The leaf certificate did not parse as X.509 — its issuer and serial
    /// could not be read for `IssuerAndSerialNumber`.
    #[error("the signer's certificate is not a parseable X.509 certificate")]
    LeafUnparseable,
    /// The key operation refused; see [`SignError`].
    #[error(transparent)]
    Sign(#[from] SignError),
}

/// A built `SignedData`, plus the facts the PDF half discloses.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BuiltCms {
    /// The complete DER `ContentInfo` — the bytes for the `/Contents` hole.
    pub der: Vec<u8>,
    /// The algorithm actually used.
    pub algorithm: SignatureAlgorithm,
    /// The leaf certificate's subject, as `cms.rs` renders it (`CN=…`).
    pub signer_subject: String,
    /// The leaf certificate's serial number, upper-case hex.
    pub signer_serial_hex: String,
    /// How many certificates were embedded.
    pub certificates: usize,
}

/// Build the detached `SignedData` over `message_digest` — the digest of
/// the PDF `/ByteRange` spans under `algorithm`'s hash (`SC-2` step 5) —
/// signing with `signer`.
///
/// `message_digest` is placed in the `message-digest` attribute verbatim;
/// the caller is responsible for having hashed the right bytes with the
/// right hash (`algorithm.digest(spans)`), which `apply` does.
///
/// # Errors
///
/// [`CmsBuildError::NoCertificate`], [`CmsBuildError::LeafUnparseable`], or
/// the signer's own [`SignError`].
pub fn build(
    signer: &dyn Signer,
    algorithm: SignatureAlgorithm,
    message_digest: &[u8],
) -> Result<BuiltCms, CmsBuildError> {
    let chain = signer.certificate_chain();
    let leaf_der = chain.first().ok_or(CmsBuildError::NoCertificate)?;
    let leaf = crate::cms::parse_certificate(leaf_der).ok_or(CmsBuildError::LeafUnparseable)?;

    let o = |s: &str| der_out::oid(s).unwrap_or_default();
    let digest_alg = der_out::algorithm_identifier(algorithm.digest_oid(), Some(der_out::null()))
        .unwrap_or_default();

    // --- signed attributes (RFC 5652 §5.3, §11.1, §11.2; RFC 5035 §5.4.1) ---
    let attribute =
        |oid: &str, value: Vec<u8>| der_out::sequence(&[o(oid), der_out::set_of(vec![value])]);
    let content_type = attribute(crate::cms::oid::CONTENT_TYPE, o(crate::cms::oid::DATA));
    let message_digest_attr = attribute(
        crate::cms::oid::MESSAGE_DIGEST,
        der_out::octet_string(message_digest),
    );
    // ESSCertIDv2 { hashAlgorithm DEFAULT sha256 (omitted), certHash, issuerSerial }
    // issuerSerial ::= SEQUENCE { issuer GeneralNames, serialNumber INTEGER }
    // GeneralNames ::= SEQUENCE OF GeneralName; directoryName is [4] EXPLICIT Name.
    let cert_hash = der_out::octet_string(&SignatureAlgorithm::RsaPkcs1v15Sha256.digest(leaf_der));
    let issuer_serial = der_out::sequence(&[
        der_out::sequence(&[der_out::context(4, leaf.issuer_der)]),
        der_out::integer(leaf.serial),
    ]);
    let ess_cert_id = der_out::sequence(&[cert_hash, issuer_serial]);
    let signing_certificate_v2 = attribute(
        "1.2.840.113549.1.9.16.2.47",
        der_out::sequence(&[der_out::sequence(&[ess_cert_id])]),
    );

    // The SET OF, DER-sorted, tagged 0x31 — THIS is what gets signed (CB-4).
    let signed_attrs_set = der_out::set_of(vec![
        content_type,
        message_digest_attr,
        signing_certificate_v2,
    ]);
    let signature = signer.sign(algorithm, &signed_attrs_set)?;

    // The same content octets re-tagged [0] IMPLICIT for the wire.
    let (set_tlv, _) = crate::asn1::read(&signed_attrs_set).unwrap_or((
        crate::asn1::Tlv {
            tag: crate::asn1::SET,
            content: &[],
            raw: &[],
        },
        &[],
    ));
    let signed_attrs_wire = der_out::context(0, set_tlv.content);

    // --- SignerInfo (RFC 5652 §5.3) ---
    let sid = der_out::sequence(&[leaf.issuer_der.to_vec(), der_out::integer(leaf.serial)]);
    let signer_info = der_out::sequence(&[
        der_out::integer_u64(1),
        sid,
        digest_alg.clone(),
        signed_attrs_wire,
        algorithm.signature_algorithm_der(),
        der_out::octet_string(&signature),
    ]);

    // --- SignedData (RFC 5652 §5.1) ---
    let certificates = der_out::context(0, &chain.concat());
    let signed_data = der_out::sequence(&[
        der_out::integer_u64(1),
        der_out::set_of(vec![digest_alg]),
        der_out::sequence(&[o(crate::cms::oid::DATA)]),
        certificates,
        der_out::set_of(vec![signer_info]),
    ]);
    let content_info = der_out::sequence(&[
        o(crate::cms::oid::SIGNED_DATA),
        der_out::context(0, &signed_data),
    ]);

    Ok(BuiltCms {
        der: content_info,
        algorithm,
        signer_subject: leaf.subject.clone(),
        signer_serial_hex: leaf.serial.iter().map(|b| format!("{b:02X}")).collect(),
        certificates: chain.len(),
    })
}
