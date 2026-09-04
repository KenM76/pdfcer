//! Reading an installed Acrobat/Reader trust store as a **trust-anchor source**
//! (`Pass 10.2`, decision 133).
//!
//! # What this is, and why it exists
//!
//! pdfcer's signature verification (`Pass 10.1`) proves a signed byte range is
//! intact but reports [`Trust::NotChecked`](crate::signature_verify::Trust) —
//! it has no set of trusted anchors to chain a signer to. The Adobe Approved
//! Trust List (AATL) and the EU Trusted Lists (EUTL) are the anchor sets a
//! signature reader needs, and AATL in particular is a **superset of the
//! Microsoft-root ∪ EUTL programs by construction** (independent Adobe audit),
//! with **no public machine-consumable bundle** — so the only 1:1 source is an
//! Acrobat/Reader install that has already downloaded it.
//!
//! That download lands in `addressbook.acrodata`, which — the useful surprise —
//! is a **COS file** (`%PPKLITE-2.1` header over ISO 32000-1 §7.5 object/xref
//! grammar). This module therefore adds **no new parser**: it loads the file
//! through [`Document::from_cos_bytes`] (the same tokenizer + classic-xref path
//! as a PDF, only the header sniff differs) and decodes each entry's embedded
//! certificate through the EXISTING X.509 decoder ([`crate::cms`], `Pass 10.1`).
//!
//! Format reference: `PDF_Spec/security/security__ppklite_addressbook.md`
//! (measured from a real specimen; Adobe publishes no grammar for this file).
//!
//! # Fuzzy-never-sneaky (rule 4)
//!
//! The per-entry `/Trust` value is an integer bitfield whose numeric constants
//! Adobe does NOT publish; the bit→category mapping is a documented hypothesis,
//! not a fact. So this reader exposes the **raw** `/Trust` integer and the
//! `/Source` tags and does **not** silently interpret the high-privilege bits.
//! A consumer that wants "is this trusted for signing" should treat any
//! non-zero `/Trust` conservatively and surface `/Source` + the raw integer,
//! never grant certify/JavaScript/system-operation trust off a guessed bit.
//!
//! # Boundaries
//!
//! - **Read-only.** Nothing is written; the operator's file is never modified.
//! - **No network.** A local file read only (rule: `pdfcer-core` never fetches).
//! - **Anchors, not verdicts.** This produces the anchor POOL; chain-building,
//!   revocation (CRL/OCSP from each cert's own DER extensions) and a clock are
//!   `Pass 10.3` and beyond.
//! - **Locating the file is the shell's job.** This module takes bytes or a
//!   path; finding an installed Acrobat's `Security` directory is
//!   platform-specific and lives in the CLI/GUI, off by default (decision 133).

use std::path::Path;

use crate::cms;
use crate::document::{DocError, Document};
use crate::object::Object;

/// The accepted header markers for a trust-store COS file. `%PPKLITE-` is the
/// address book; `%FDF-` is the sibling `directories.acrodata` (directory
/// config, no certs — accepted so a mis-pointed path fails with a clean
/// "no address book" rather than a header error).
const COS_MARKERS: &[&[u8]] = &[b"%PPKLITE-", b"%FDF-"];

/// A hard ceiling on address-book entries walked, so a hostile file cannot make
/// the reader run unbounded (ARCHITECTURE §10.1). The real specimen has ~2,500
/// entries; a million is far above any real store and still bounds the work.
const MAX_ENTRIES: usize = 1_000_000;

/// One trusted identity read from the address book — a single X.509 certificate
/// plus the trust metadata the file states about it.
///
/// The certificate is kept as owned DER ([`Self::der`]) so the anchor outlives
/// the [`Document`] it was read from and can feed a chain builder later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustAnchor {
    /// The certificate subject (RFC 4514-ish string from the X.509 decoder).
    pub subject: String,
    /// The certificate issuer.
    pub issuer: String,
    /// The serial number, lowercase hex.
    pub serial_hex: String,
    /// `notBefore`, as the certificate states it (ASN.1 time string).
    pub not_before: Option<String>,
    /// `notAfter`.
    pub not_after: Option<String>,
    /// The `/Source` provenance tags, verbatim: `AATL`, `EUTL`, `ADBE`, …. A
    /// certificate can belong to several lists, so this is a list; an empty
    /// list means the entry stated no source.
    pub sources: Vec<String>,
    /// The RAW `/Trust` bitfield. **Its bit meanings are Adobe-unpublished and
    /// pdfcer's interpretation is provisional** — surface this integer, do not
    /// silently act on a specific bit (module docs, rule 4).
    pub trust_bits: u32,
    /// `/PolicyOID` certificate-policy constraints (e.g. an eIDAS
    /// qualified-signer policy OID), verbatim strings; empty when none.
    pub policy_oids: Vec<String>,
    /// The whole certificate as DER, for chain building (`Pass 10.3`).
    pub der: Vec<u8>,
    /// The address book's own `/ID` for this entry (a namespace distinct from
    /// object numbers), when present.
    pub id: Option<i64>,
}

impl TrustAnchor {
    /// Whether this anchor carries `tag` in its `/Source` list (case-sensitive,
    /// matching Acrobat's own `AATL`/`EUTL`/`ADBE` spelling).
    #[must_use]
    pub fn has_source(&self, tag: &str) -> bool {
        self.sources.iter().any(|s| s == tag)
    }
}

/// Which `/Source` lists to keep when filtering a [`TrustAnchorSet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFilter {
    /// Only Adobe Approved Trust List anchors.
    Aatl,
    /// Only EU Trusted Lists anchors.
    Eutl,
    /// Only the Adobe built-in root(s).
    Adbe,
    /// Everything the store carries, including user-added identities.
    All,
}

/// A count of anchors by provenance, for the disclosure a shell shows before an
/// operator enables the store (rule 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SourceCounts {
    /// Anchors tagged `AATL` (may also be tagged `EUTL`).
    pub aatl: usize,
    /// Anchors tagged `EUTL`.
    pub eutl: usize,
    /// Anchors tagged `ADBE` (Adobe built-in root).
    pub adbe: usize,
    /// Anchors with no recognised source tag.
    pub other: usize,
    /// Total anchors.
    pub total: usize,
}

/// The full set of trusted anchors read from one address book.
#[derive(Debug, Clone, Default)]
pub struct TrustAnchorSet {
    /// Every certificate entry (`/ABEType 1`) the store carries. Identity
    /// groupings (`/ABEType 2`, mostly timestamp-authority contacts) are not
    /// anchors and are skipped.
    pub anchors: Vec<TrustAnchor>,
    /// Entries whose `/Cert` DER could not be decoded — reported, not hidden,
    /// so a shell can disclose that N entries were unreadable rather than
    /// pretend the store was fully understood.
    pub undecodable: usize,
}

impl TrustAnchorSet {
    /// The number of anchors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.anchors.len()
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.anchors.is_empty()
    }

    /// Count anchors by `/Source`.
    #[must_use]
    pub fn counts(&self) -> SourceCounts {
        let mut c = SourceCounts {
            total: self.anchors.len(),
            ..SourceCounts::default()
        };
        for a in &self.anchors {
            let aatl = a.has_source("AATL");
            let eutl = a.has_source("EUTL");
            let adbe = a.has_source("ADBE");
            if aatl {
                c.aatl += 1;
            }
            if eutl {
                c.eutl += 1;
            }
            if adbe {
                c.adbe += 1;
            }
            if !aatl && !eutl && !adbe {
                c.other += 1;
            }
        }
        c
    }

    /// The anchors matching `filter`, in store order.
    #[must_use]
    pub fn filter(&self, filter: SourceFilter) -> Vec<&TrustAnchor> {
        self.anchors
            .iter()
            .filter(|a| match filter {
                SourceFilter::All => true,
                SourceFilter::Aatl => a.has_source("AATL"),
                SourceFilter::Eutl => a.has_source("EUTL"),
                SourceFilter::Adbe => a.has_source("ADBE"),
            })
            .collect()
    }

    /// Read the anchor set from a loaded trust-store [`Document`].
    ///
    /// Walks `/Root → /PPK → /AddressBook → /Entries`, keeps `/ABEType 1`
    /// entries, and decodes each `/Cert`. Type-2 identity groupings are skipped
    /// (they reference type-1 entries by `/ID`, not object number, and carry no
    /// anchor of their own).
    ///
    /// # Errors
    ///
    /// [`TrustStoreError::NotAnAddressBook`] if the document is a COS file but
    /// not a PPKLITE address book (no `/PPK → /AddressBook → /Entries`).
    pub fn from_document(doc: &Document) -> Result<Self, TrustStoreError> {
        let root = doc
            .trailer()
            .get(b"Root")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
            .ok_or(TrustStoreError::NotAnAddressBook)?;
        let ppk = root
            .get(b"PPK")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
            .ok_or(TrustStoreError::NotAnAddressBook)?;
        let address_book = ppk
            .get(b"AddressBook")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
            .ok_or(TrustStoreError::NotAnAddressBook)?;
        let entries = address_book
            .get(b"Entries")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_array)
            .ok_or(TrustStoreError::NotAnAddressBook)?;

        let mut anchors = Vec::new();
        let mut undecodable = 0usize;
        for entry_ref in entries.iter().take(MAX_ENTRIES) {
            let Some(entry) = Object::as_dict(doc.resolve(entry_ref)) else {
                continue;
            };
            // Only certificate entries (/ABEType 1) are anchors.
            if entry
                .get(b"ABEType")
                .map(|o| doc.resolve(o))
                .and_then(Object::as_int)
                != Some(1)
            {
                continue;
            }
            let Some(der) = string_bytes(doc, entry.get(b"Cert")) else {
                continue;
            };
            let Some(cert) = cms::parse_certificate(&der) else {
                undecodable += 1;
                continue;
            };
            anchors.push(TrustAnchor {
                subject: cert.subject,
                issuer: cert.issuer,
                serial_hex: to_hex(cert.serial),
                not_before: cert.not_before,
                not_after: cert.not_after,
                sources: string_list(doc, entry.get(b"Source")),
                trust_bits: entry
                    .get(b"Trust")
                    .map(|o| doc.resolve(o))
                    .and_then(Object::as_int)
                    .and_then(|i| u32::try_from(i).ok())
                    .unwrap_or(0),
                policy_oids: string_list(doc, entry.get(b"PolicyOID")),
                der,
                id: entry
                    .get(b"ID")
                    .map(|o| doc.resolve(o))
                    .and_then(Object::as_int),
            });
        }
        Ok(Self {
            anchors,
            undecodable,
        })
    }
}

/// Read a trust store from raw bytes (`%PPKLITE-` address book).
///
/// # Errors
///
/// [`TrustStoreError::Load`] if the bytes are not a loadable COS file;
/// [`TrustStoreError::NotAnAddressBook`] if they load but are not a PPKLITE
/// address book.
pub fn load_from_bytes(bytes: Vec<u8>) -> Result<TrustAnchorSet, TrustStoreError> {
    let doc = Document::from_cos_bytes(bytes, COS_MARKERS).map_err(TrustStoreError::Load)?;
    TrustAnchorSet::from_document(&doc)
}

/// Read a trust store from a file path.
///
/// # Errors
///
/// [`TrustStoreError::Io`] if the file cannot be read; otherwise as
/// [`load_from_bytes`].
pub fn load_from_path(path: &Path) -> Result<TrustAnchorSet, TrustStoreError> {
    let bytes = std::fs::read(path).map_err(TrustStoreError::Io)?;
    load_from_bytes(bytes)
}

/// Why a trust store could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TrustStoreError {
    /// The file could not be read from disk.
    #[error("reading the trust store: {0}")]
    Io(#[source] std::io::Error),
    /// The bytes are not a loadable COS file (bad header, unparseable xref).
    #[error("the trust store is not a readable COS file: {0}")]
    Load(#[source] DocError),
    /// The file loaded as COS but is not a PPKLITE address book — it has no
    /// `/PPK → /AddressBook → /Entries`. `directories.acrodata` (directory
    /// config) reaches here, for example.
    #[error(
        "the file is not a PPKLITE address book (no /PPK /AddressBook /Entries); \
         it may be directories.acrodata or security-policy.acrodata, which carry no anchors"
    )]
    NotAnAddressBook,
}

/// The raw bytes of a `/Cert`-style literal string, resolved through the graph.
fn string_bytes(doc: &Document, obj: Option<&Object>) -> Option<Vec<u8>> {
    match doc.resolve(obj?) {
        Object::String(bytes) => Some(bytes.clone()),
        _ => None,
    }
}

/// A `/Source`- or `/PolicyOID`-style value: a single string OR an array of
/// strings, flattened to owned `String`s (empty strings and non-strings
/// dropped). Provenance tags are ASCII; a lossy decode is faithful for those.
fn string_list(doc: &Document, obj: Option<&Object>) -> Vec<String> {
    let one = |o: &Object| match o {
        Object::String(b) if !b.is_empty() => Some(String::from_utf8_lossy(b).into_owned()),
        _ => None,
    };
    match obj.map(|o| doc.resolve(o)) {
        Some(Object::String(b)) if !b.is_empty() => {
            vec![String::from_utf8_lossy(b).into_owned()]
        }
        Some(Object::Array(a)) => a.iter().map(|e| doc.resolve(e)).filter_map(one).collect(),
        _ => Vec::new(),
    }
}

/// Lowercase hex of a byte slice (serial numbers).
fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    /// A tiny synthetic `%PPKLITE-` address book with two certificate entries
    /// (one AATL, one EUTL) built from real DER, proving the walk, the two
    /// source tags, the raw `/Trust`, and the type-2 skip — without shipping
    /// any real trust file.
    #[test]
    fn reads_a_synthetic_addressbook() {
        let der = self_signed_der();
        let hex: String = der.iter().map(|b| format!("\\{b:03o}")).collect();
        // Build a classic-xref PPKLITE file. Object 1 = catalog, 2/3 = cert
        // entries, 4 = a type-2 grouping (must be skipped).
        let bytes = build_ppklite(&[
            // obj 2: AATL cert
            format!("<</ABEType 1/Cert({hex})/Source[(AATL)]/Trust 96/ID 1001>>"),
            // obj 3: EUTL cert
            format!("<</ABEType 1/Cert({hex})/Source[(EUTL)]/Trust 98/ID 1002>>"),
            // obj 4: type-2 grouping -> skipped
            "<</ABEType 2/Certs[1001 1002]/Name(A TSA)/ID 1003>>".to_owned(),
        ]);
        let set = load_from_bytes(bytes).expect("loads the synthetic address book");
        assert_eq!(set.len(), 2, "two certificate anchors, the type-2 skipped");
        assert_eq!(set.undecodable, 0);
        let c = set.counts();
        assert_eq!((c.aatl, c.eutl, c.total), (1, 1, 2));
        assert_eq!(set.filter(SourceFilter::Aatl).len(), 1);
        assert_eq!(set.anchors[0].trust_bits, 96);
        assert!(set.anchors[0].has_source("AATL"));
        assert!(!set.anchors[0].serial_hex.is_empty());
    }

    /// A COS file that is not a PPKLITE address book is refused by name.
    #[test]
    fn a_non_addressbook_cos_file_is_named() {
        // A minimal FDF-shaped catalog with no /PPK.
        let bytes = build_ppklite_with_root("<</Type/Catalog>>", &[]);
        match load_from_bytes(bytes) {
            Err(TrustStoreError::NotAnAddressBook) => {}
            other => panic!("expected NotAnAddressBook, got {other:?}"),
        }
    }

    // --- test helpers -----------------------------------------------------

    /// A synthetic self-signed ECDSA P-256 certificate (`CN=pdfcer trust-store
    /// test, O=pdfcer synthetic`), generated with OpenSSL for this test and
    /// embedded so the address-book test needs no external file. Rights-clear
    /// (pdfcer authored it); the X.509 decoder ([`crate::cms`]) accepts it.
    const TEST_CERT_DER: &[u8] = &[
        0x30, 0x82, 0x01, 0xcf, 0x30, 0x82, 0x01, 0x75, 0xa0, 0x03, 0x02, 0x01, 0x02, 0x02, 0x14,
        0x31, 0x96, 0xa7, 0xb2, 0x8f, 0xce, 0x7c, 0xae, 0x35, 0x6a, 0x93, 0x08, 0x0d, 0xca, 0x2c,
        0x75, 0x03, 0x58, 0xf3, 0x74, 0x30, 0x0a, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04,
        0x03, 0x02, 0x30, 0x3d, 0x31, 0x20, 0x30, 0x1e, 0x06, 0x03, 0x55, 0x04, 0x03, 0x0c, 0x17,
        0x70, 0x64, 0x66, 0x63, 0x65, 0x72, 0x20, 0x74, 0x72, 0x75, 0x73, 0x74, 0x2d, 0x73, 0x74,
        0x6f, 0x72, 0x65, 0x20, 0x74, 0x65, 0x73, 0x74, 0x31, 0x19, 0x30, 0x17, 0x06, 0x03, 0x55,
        0x04, 0x0a, 0x0c, 0x10, 0x70, 0x64, 0x66, 0x63, 0x65, 0x72, 0x20, 0x73, 0x79, 0x6e, 0x74,
        0x68, 0x65, 0x74, 0x69, 0x63, 0x30, 0x1e, 0x17, 0x0d, 0x32, 0x36, 0x30, 0x39, 0x30, 0x34,
        0x31, 0x35, 0x30, 0x39, 0x33, 0x32, 0x5a, 0x17, 0x0d, 0x33, 0x36, 0x30, 0x39, 0x30, 0x31,
        0x31, 0x35, 0x30, 0x39, 0x33, 0x32, 0x5a, 0x30, 0x3d, 0x31, 0x20, 0x30, 0x1e, 0x06, 0x03,
        0x55, 0x04, 0x03, 0x0c, 0x17, 0x70, 0x64, 0x66, 0x63, 0x65, 0x72, 0x20, 0x74, 0x72, 0x75,
        0x73, 0x74, 0x2d, 0x73, 0x74, 0x6f, 0x72, 0x65, 0x20, 0x74, 0x65, 0x73, 0x74, 0x31, 0x19,
        0x30, 0x17, 0x06, 0x03, 0x55, 0x04, 0x0a, 0x0c, 0x10, 0x70, 0x64, 0x66, 0x63, 0x65, 0x72,
        0x20, 0x73, 0x79, 0x6e, 0x74, 0x68, 0x65, 0x74, 0x69, 0x63, 0x30, 0x59, 0x30, 0x13, 0x06,
        0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d,
        0x03, 0x01, 0x07, 0x03, 0x42, 0x00, 0x04, 0x3c, 0xde, 0xe7, 0x0c, 0x07, 0x3b, 0xa4, 0x17,
        0x4f, 0xc2, 0x1c, 0x2b, 0x43, 0x2e, 0x43, 0xbe, 0x88, 0xeb, 0x22, 0x18, 0xc3, 0x27, 0xf1,
        0x66, 0x66, 0xfa, 0x4b, 0xc2, 0x7d, 0xdd, 0x8f, 0x6a, 0xc3, 0x3f, 0x90, 0x58, 0x56, 0x38,
        0x66, 0xcd, 0xf3, 0xe4, 0x49, 0xac, 0xc2, 0x7c, 0xbe, 0x16, 0x21, 0x6f, 0x10, 0xa6, 0xd3,
        0xd0, 0x2b, 0xb9, 0xa5, 0x90, 0xab, 0xd3, 0x65, 0x02, 0x0a, 0xaa, 0xa3, 0x53, 0x30, 0x51,
        0x30, 0x1d, 0x06, 0x03, 0x55, 0x1d, 0x0e, 0x04, 0x16, 0x04, 0x14, 0x06, 0xd3, 0x2a, 0x0e,
        0x65, 0x4b, 0x1f, 0xd2, 0x51, 0x13, 0xf7, 0xfb, 0xb7, 0x05, 0x68, 0x7d, 0x75, 0x96, 0xb7,
        0x05, 0x30, 0x1f, 0x06, 0x03, 0x55, 0x1d, 0x23, 0x04, 0x18, 0x30, 0x16, 0x80, 0x14, 0x06,
        0xd3, 0x2a, 0x0e, 0x65, 0x4b, 0x1f, 0xd2, 0x51, 0x13, 0xf7, 0xfb, 0xb7, 0x05, 0x68, 0x7d,
        0x75, 0x96, 0xb7, 0x05, 0x30, 0x0f, 0x06, 0x03, 0x55, 0x1d, 0x13, 0x01, 0x01, 0xff, 0x04,
        0x05, 0x30, 0x03, 0x01, 0x01, 0xff, 0x30, 0x0a, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d,
        0x04, 0x03, 0x02, 0x03, 0x48, 0x00, 0x30, 0x45, 0x02, 0x21, 0x00, 0xba, 0x4d, 0xf7, 0x8f,
        0xd3, 0xdf, 0xbb, 0x0c, 0x3d, 0x5a, 0x72, 0xc9, 0xb0, 0x22, 0xf8, 0xff, 0x3a, 0x27, 0x17,
        0x32, 0x05, 0x5a, 0xe4, 0xe8, 0x4e, 0x75, 0x17, 0x4b, 0x26, 0xf4, 0x1a, 0xcc, 0x02, 0x20,
        0x37, 0x99, 0xc3, 0xa7, 0x34, 0x0f, 0x8d, 0x1b, 0x38, 0xa5, 0xf9, 0xa4, 0x1b, 0x74, 0x65,
        0xcc, 0x53, 0xa5, 0xe4, 0xc2, 0xd7, 0x86, 0x79, 0xb8, 0x3b, 0xf9, 0x36, 0x0b, 0x81, 0x56,
        0x9f, 0x4c,
    ];

    fn self_signed_der() -> Vec<u8> {
        TEST_CERT_DER.to_vec()
    }

    fn build_ppklite(entries: &[String]) -> Vec<u8> {
        // Root references entry objects 2..N by object ref.
        let refs: String = (2..2 + entries.len())
            .map(|n| format!("{n} 0 R "))
            .collect();
        let root = format!(
            "<</PPK<</AddressBook<</Entries[{refs}]/NextID 9999/Type/AddressBook>>/Type/PPK>>/Type/Catalog>>"
        );
        build_ppklite_with_root(&root, entries)
    }

    fn build_ppklite_with_root(root_body: &str, entries: &[String]) -> Vec<u8> {
        let mut objs: Vec<String> = vec![root_body.to_owned()];
        objs.extend(entries.iter().cloned());
        let mut out: Vec<u8> = b"%PPKLITE-2.1\r%\xE2\xE3\xCF\xD3\r\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\r\n{}\r\nendobj\r\n", i + 1, body).as_bytes());
        }
        let xref_off = out.len();
        let n = objs.len() + 1;
        out.extend_from_slice(format!("xref\r\n0 {n}\r\n").as_bytes());
        out.extend_from_slice(b"0000000000 65535 f\r\n");
        for off in &offsets {
            out.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\r\n<</Size {n}/Root 1 0 R>>\r\nstartxref\r\n{xref_off}\r\n%%EOF\r\n")
                .as_bytes(),
        );
        out
    }
}
