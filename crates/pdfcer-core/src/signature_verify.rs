//! Signature **verification** — the integrity stage of ISO 32000-1 §12.8,
//! with coverage alongside it and trust named as unchecked.
//!
//! Spec sources, PDF-spec RAG: `iso32000__s__12.8.md` (the byte-range
//! coverage model and the two-stage validation model this module's sibling
//! [`crate::signature`] already enacts), `iso32000__s__12.8.3.md` (Table 252
//! in full, §12.8.3.3's PKCS#7 subfilters, 2.0's CAdES clause, the RFC 5652
//! mechanics `SI-C1`–`SI-C4`) and the derived consolidator
//! `iso32000__ref__signature_verification.md`. Identifiers like `SI-W1`
//! below are that RAG's, so a reader can find the sentence a check enacts.
//!
//! # The three facts, kept apart
//!
//! [`SignatureVerdict`] carries three independent answers, because they
//! fail independently and an operator must know which one did:
//!
//! | fact | question | answered by |
//! |---|---|---|
//! | [`Integrity`] | are the signed bytes unaltered, and is the signature over them genuine? | this module: digest over `/ByteRange` vs `messageDigest`, then the CMS signature over the signed attributes against the signer's own embedded certificate |
//! | coverage | does the signed range reach the end of the file, or was something appended after signing? | [`crate::signature::ByteRangeCoverage`], computed here from the same `/ByteRange` |
//! | [`Trust`] | is the signer who they claim, and were they entitled? | **nobody, yet** — [`Trust::NotChecked`], said in those words |
//!
//! The first cut answers the first two. "The bytes under this signature
//! have not been altered, and nothing was appended after it" is a real
//! answer and needs no certificate store; the trust question is the one that
//! drags in a store, chain building, revocation and time, and the request
//! that drove this module (pdfcer-gui, 2026-09-03) asked in as many words that
//! it not block the integrity one. A shell **must not** render
//! `Integrity::Verified` as "valid" or "signed by X": the certificate's
//! subject is carried as a *claim* ([`SignatureVerdict::signer_subject`])
//! for exactly that reason.
//!
//! # What "integrity" checks, per subfilter (`SI-W1`, `SI-W2`, `SI-C3`)
//!
//! Let `D` be the bytes selected by `/ByteRange`, and `H` the signer's
//! `digestAlgorithm`.
//!
//! - **`adbe.pkcs7.detached`** and **`ETSI.CAdES.detached`**: `H(D)` must equal
//!   the `messageDigest` signed attribute; `eContent` must be absent.
//! - **`adbe.pkcs7.sha1`**: the `eContent` must equal `SHA1(D)` (twenty
//!   bytes — the inner hash is pinned to SHA-1 by the subfilter name, Table
//!   257 footnote b), and `H(eContent)` must equal `messageDigest`.
//! - **`adbe.x509.rsa_sha1`**, **`ETSI.RFC3161`**, anything else: reported as
//!   [`Integrity::Unverifiable`] by name. Nothing is guessed.
//!
//! Then, for every subfilter: the `content-type` attribute must be `id-data`
//! (RFC 5652 §5.6), and the signature value must verify over
//! `DER(SET OF signedAttrs)` — the `[0]` tag in the file rewritten to `0x31`
//! (`SI-C2`) — with the signer's certificate's key. RSA PKCS#1 v1.5 (the
//! hash is the one `digestAlgorithm` names, `SI-W13`), RSASSA-PSS (params
//! from the `AlgorithmIdentifier`, RFC 4055) and ECDSA on P-256/P-384 are
//! implemented in-crate ([`crate::crypto::rsa`], [`crate::crypto::ecdsa`]);
//! any other key or curve is *unverifiable, by name*.
//!
//! # The hole (`SI-W3`, `SI-W4`, `SI-A3`; geometry checks `G1`–`G10`)
//!
//! `/ByteRange [0 a b c]` excludes `[a, b)`, which "shall fit precisely" the
//! `/Contents` hex string including its `<` `>` delimiters. That is checked
//! against the FILE bytes, not the parsed object: `bytes[a] == '<'`,
//! `bytes[b-1] == '>'`, only hex digits and white space between. The DER is
//! decoded from those bytes and parsed by its own length; trailing padding
//! is ignored and disclosed when it is not zeros. More than two pairs is
//! conforming (`G7`): exactly one gap must be the `/Contents` token, and
//! every other gap is DISCLOSED with its extent, because it is bytes inside
//! the signed span the signature does not cover. A first pair not at 0
//! (`G8`) and an ETSI signature that does not reach EOF (`G10`) are
//! disclosed too — the first is a `should`, the second a `shall`, and the
//! notes say which.
//!
//! The verdict vocabulary here is pdfcer's own. ETSI EN 319 102-1 defines
//! `TOTAL-PASSED` / `TOTAL-FAILED` / `INDETERMINATE` for the *full*
//! validation including trust; when the trust stage lands, its names are
//! the ones to align to (spec RAG `SV-A5`), not these.
//!
//! # Which signatures
//!
//! Every `/FT /Sig` field with a `/V` — the same census
//! [`crate::signature::byte_range_coverage`] walks, so the two lists index
//! identically. A signature dictionary reachable only from `/Perms` (no
//! field) is not listed; the certification census covers it.

use crate::cms::{self, PublicKey, oid};
use crate::crypto::ecdsa::{Curve, EcPublicKey};
use crate::crypto::rsa::{Hash, RsaPublicKey};
use crate::crypto::{bignum::Uint, sha1::sha1};
use crate::graph::ObjectGraph;
use crate::object::Object;
use crate::signature::ByteRangeCoverage;

/// Whether the signed bytes are what the signer signed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Integrity {
    /// The digest over the byte range matches what the signer signed, and
    /// the signature over the signed attributes verifies with the signer's
    /// own certificate. **Not** "valid": see [`Trust`].
    Verified {
        /// The digest algorithm the signer used (`"SHA-256"`, …). Carried
        /// so a shell can disclose a SHA-1 signature as verified-with-a-weak
        /// digest rather than hide it.
        digest_algorithm: &'static str,
        /// The signature algorithm, for the same disclosure
        /// (`"RSA PKCS#1 v1.5, 2048-bit"`, `"ECDSA P-256"`, …).
        signature_algorithm: String,
    },
    /// The digest over the byte range does NOT match the signed
    /// `messageDigest`: the covered bytes were altered after signing.
    DigestMismatch,
    /// The covered bytes are what was signed, but the signature value does
    /// not verify with the signer's certificate — the signature itself, or
    /// the certificate, or the signed attributes were altered.
    SignatureInvalid,
    /// pdfcer could not reach a verdict, and says why: a subfilter or
    /// algorithm it does not implement, a malformed CMS, a missing
    /// certificate, a hole that does not fit the range. Never reported as
    /// either of the other three.
    Unverifiable {
        /// The reason, in operator terms.
        reason: String,
    },
}

/// Whether the signer is who the certificate claims and was entitled to sign.
///
/// [`NotChecked`](Trust::NotChecked) is the default: no trust anchors were
/// supplied. When anchors ARE supplied (`Pass 10.3`, an opt-in read of an
/// installed Acrobat's trust store), the other variants report whether the
/// signer chains, BY SIGNATURE, to one of them. ★ A [`Trusted`](Trust::Trusted)
/// verdict checks signature linkage, RFC 5280 CA/key-usage constraints, and —
/// when a signing-time clock is available — certificate validity dates
/// (`Pass 10.5`); it does NOT check revocation (CRL/OCSP), which needs the
/// network `pdfcer-core` never touches. The verdict's own note says exactly
/// what ran.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Trust {
    /// No trust store, no chain building, no revocation, no time check.
    /// The certificate's subject and validity dates are reported as claims.
    NotChecked,
    /// The signer chains, by valid signatures, to a trusted anchor, and RFC
    /// 5280 CA/key-usage constraints held. NOT a revocation verdict, and only a
    /// validity-date verdict when `validity_checked` (see the type docs and note).
    Trusted {
        /// The trusted anchor's subject the chain terminated at.
        anchor_subject: String,
        /// That anchor's `/Source` provenance (`AATL`/`EUTL`/`ADBE`).
        source: Vec<String>,
        /// Whether certificate validity dates were checked against the signing
        /// time. `false` means no clock was available, so expiry was NOT checked
        /// (revocation is never checked this build — pdfcer-core has no network).
        validity_checked: bool,
    },
    /// Trust was evaluated and the signer does NOT chain to a trusted anchor
    /// (a valid signature can still be *untrusted* — "valid but untrusted").
    Untrusted {
        /// Why (incomplete chain, untrusted root, bad link signature, …).
        reason: String,
    },
    /// Trust was requested but the signer's certificate could not be parsed,
    /// so trust could not even be attempted. Distinct from `Untrusted`.
    SignerUnknown,
}

/// The verdict on one signature field.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SignatureVerdict {
    /// The signature field's fully qualified name.
    pub field_name: Option<String>,
    /// `/SubFilter`, as written.
    pub sub_filter: Option<String>,
    /// The integrity verdict (module docs).
    pub integrity: Integrity,
    /// What the `/ByteRange` covers, against the file's real length.
    pub coverage: ByteRangeCoverage,
    /// Always [`Trust::NotChecked`] this build.
    pub trust: Trust,
    /// The signer certificate's subject (`CN=…, O=…`), a CLAIM.
    pub signer_subject: Option<String>,
    /// The signer certificate's issuer, a claim.
    pub signer_issuer: Option<String>,
    /// The certificate's validity window, ISO-8601, claims.
    pub cert_not_before: Option<String>,
    pub cert_not_after: Option<String>,
    /// The `signingTime` signed attribute (the signer's clock), ISO-8601.
    pub signing_time: Option<String>,
    /// The signature dictionary's `/Name`, `/M`, `/Reason`, `/Location` —
    /// what the signer WROTE, none of it verified (Table 252 `SI-T9`–`T12`).
    pub name: Option<String>,
    pub date: Option<String>,
    pub reason: Option<String>,
    pub location: Option<String>,
    /// Disclosures: a weak digest, non-zero padding, an odd CMS version,
    /// extra signers — everything the operator cannot see from the verdict.
    pub notes: Vec<String>,
}

/// Verify every signature field in `graph`, over the file `bytes` the
/// graph was loaded from. Order and count match
/// [`crate::signature::byte_range_coverage`].
#[must_use]
pub fn verify_all<G: ObjectGraph + ?Sized>(graph: &G, bytes: &[u8]) -> Vec<SignatureVerdict> {
    verify_all_with_trust(graph, bytes, None)
}

/// [`verify_all`] plus **trust evaluation** against `anchors` (`Pass 10.3`).
///
/// When `anchors` is `None`, trust is [`Trust::NotChecked`] (identical to
/// [`verify_all`]). When `Some`, each signature's signer is chained by
/// signature to the anchor pool (e.g. an installed Acrobat's AATL/EUTL store,
/// read opt-in by the shell) and the verdict's [`SignatureVerdict::trust`]
/// reports [`Trust::Trusted`]/[`Untrusted`](Trust::Untrusted)/[`SignerUnknown`](Trust::SignerUnknown).
/// Integrity is unaffected by trust — a signature can be
/// [`Integrity::Verified`] and [`Trust::Untrusted`] (valid but untrusted).
#[must_use]
pub fn verify_all_with_trust<G: ObjectGraph + ?Sized>(
    graph: &G,
    bytes: &[u8],
    anchors: Option<&crate::trust_store::TrustAnchorSet>,
) -> Vec<SignatureVerdict> {
    let mut out = Vec::new();
    let Some(form) = crate::forms::parse_acroform(graph) else {
        return out;
    };
    for field in &form.fields {
        if field.field_type != Some(crate::forms::FieldType::Signature) {
            continue;
        }
        let Some(dict) = graph
            .resolved(field.id)
            .as_dict()
            .and_then(|d| d.get(b"V"))
            .map(|o| graph.resolve(o))
            .and_then(Object::as_dict)
        else {
            continue;
        };
        let mut verdict = verify_dict(graph, bytes, dict, anchors);
        verdict.field_name = Some(field.fully_qualified_name.clone());
        out.push(verdict);
    }
    out
}

/// Verify the `index`-th signature field (in [`verify_all`]'s order).
#[must_use]
pub fn verify<G: ObjectGraph + ?Sized>(
    graph: &G,
    bytes: &[u8],
    index: usize,
) -> Option<SignatureVerdict> {
    verify_all(graph, bytes).into_iter().nth(index)
}

fn text<G: ObjectGraph + ?Sized>(
    graph: &G,
    dict: &crate::object::Dict,
    key: &[u8],
) -> Option<String> {
    match graph.resolve(dict.get(key)?) {
        Object::String(s) => Some(crate::textstring::decode_text_string(s).text),
        Object::Name(n) => Some(String::from_utf8_lossy(n.as_bytes()).into_owned()),
        _ => None,
    }
}

/// The verdict on one signature dictionary.
fn verify_dict<G: ObjectGraph + ?Sized>(
    graph: &G,
    bytes: &[u8],
    dict: &crate::object::Dict,
    anchors: Option<&crate::trust_store::TrustAnchorSet>,
) -> SignatureVerdict {
    let sub_filter = text(graph, dict, b"SubFilter");
    let mut notes = Vec::new();
    let file_len = bytes.len() as u64;

    // --- /ByteRange geometry (Table 252 SI-T6; SI-D6) ---
    let nums: Vec<i64> = dict
        .get(b"ByteRange")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)
        .map(|a| {
            a.iter()
                .map(|o| graph.resolve(o))
                .filter_map(Object::as_int)
                .collect()
        })
        .unwrap_or_default();
    let mut ranges: Vec<(u64, u64)> = Vec::new();
    let mut well_formed = nums.len() >= 2 && nums.len().is_multiple_of(2);
    for pair in nums.chunks_exact(2) {
        if let [off, len] = pair
            && *off >= 0
            && *len >= 0
        {
            ranges.push((off.unsigned_abs(), len.unsigned_abs()));
        } else {
            well_formed = false;
        }
    }
    let mut prev_end = 0u64;
    for (off, len) in &ranges {
        if *off < prev_end || off.saturating_add(*len) > file_len {
            well_formed = false;
        }
        prev_end = off.saturating_add(*len);
    }
    let end = ranges
        .iter()
        .map(|(o, l)| o.saturating_add(*l))
        .max()
        .unwrap_or(0);
    let coverage = ByteRangeCoverage {
        field_name: None,
        pair_count: ranges.len(),
        covered: ranges.iter().map(|(_, l)| *l).sum(),
        file_len,
        uncovered_tail: file_len.saturating_sub(end),
        ranges: ranges.clone(),
        ranges_well_formed: well_formed,
    };

    let mut verdict = SignatureVerdict {
        field_name: None,
        sub_filter: sub_filter.clone(),
        integrity: Integrity::Unverifiable {
            reason: String::new(),
        },
        coverage,
        trust: Trust::NotChecked,
        signer_subject: None,
        signer_issuer: None,
        cert_not_before: None,
        cert_not_after: None,
        signing_time: None,
        name: text(graph, dict, b"Name"),
        date: text(graph, dict, b"M"),
        reason: text(graph, dict, b"Reason"),
        location: text(graph, dict, b"Location"),
        notes: Vec::new(),
    };
    let unverifiable = |reason: &str| Integrity::Unverifiable {
        reason: reason.to_string(),
    };

    if !well_formed {
        verdict.integrity = unverifiable(
            "the /ByteRange is malformed (not pairs, negative, overlapping, or past the end of the file)",
        );
        return verdict;
    }
    let sf = sub_filter.as_deref().unwrap_or("");
    // G10: the ETSI subfilters make whole-file coverage a `shall`.
    if sf.starts_with("ETSI.") && verdict.coverage.uncovered_tail > 0 {
        notes.push(format!(
            "an {sf} signature shall cover the whole file (ISO 32000-2 Table 255, ETSI EN 319 142-1 §6.3 k) and this one leaves {} byte(s) after its range — a later revision was appended",
            verdict.coverage.uncovered_tail
        ));
    }
    match sf {
        "adbe.pkcs7.detached" | "ETSI.CAdES.detached" | "adbe.pkcs7.sha1" => {}
        "adbe.x509.rsa_sha1" => {
            verdict.integrity = unverifiable(
                "the PKCS#1 subfilter adbe.x509.rsa_sha1 (Table 252 /Cert, §12.8.3.2) is not implemented",
            );
            return verdict;
        }
        "ETSI.RFC3161" => {
            verdict.integrity = unverifiable(
                "a document timestamp (ETSI.RFC3161, ISO 32000-2 §12.8.5) is not verified this build",
            );
            return verdict;
        }
        other => {
            verdict.integrity = unverifiable(&format!(
                "the /SubFilter {other:?} is not one pdfcer verifies (adbe.pkcs7.detached, ETSI.CAdES.detached, adbe.pkcs7.sha1)"
            ));
            return verdict;
        }
    }

    // --- the gaps: exactly one is the /Contents token (G5–G8, SI-W3) ---
    //
    // Canonical is `[0 a b c]` with one gap. More pairs are conforming (G7)
    // but mean the digest skips something else — each extra gap is disclosed
    // with its extent, because the operator cannot see it and it changes
    // what "signed" means. A first pair not starting at 0 is a `should`
    // (G8) and is disclosed the same way.
    if let Some((first_off, _)) = ranges.first()
        && *first_off > 0
    {
        notes.push(format!(
            "the signed range starts at byte {first_off}, not 0 — the {first_off} byte(s) before it are not covered (ISO 32000-1 §12.8.1 recommends whole-file coverage)"
        ));
    }
    let mut contents_hole: Option<(usize, usize)> = None;
    for pair in ranges.windows(2) {
        let (Some((o1, l1)), Some((o2, _))) = (pair.first(), pair.get(1)) else {
            continue;
        };
        let (gs, ge) = ((o1 + l1) as usize, *o2 as usize);
        let looks_like_contents = bytes
            .get(gs..ge)
            .is_some_and(|g| g.first() == Some(&b'<') && g.last() == Some(&b'>'));
        if looks_like_contents && contents_hole.is_none() {
            contents_hole = Some((gs, ge));
        } else {
            notes.push(format!(
                "the /ByteRange skips bytes {gs}..{ge} in addition to the /Contents hole — {} byte(s) inside the signed span are NOT covered by the signature (Table 252 permits this; §12.8.1 does not recommend it)",
                ge.saturating_sub(gs)
            ));
        }
    }
    let Some((hs, he)) = contents_hole else {
        verdict.integrity = unverifiable(
            "no gap in the /ByteRange is the /Contents hex string (no < > delimiters at any gap's edges, SI-W3)",
        );
        verdict.notes = notes;
        return verdict;
    };
    let Some(hole_bytes) = bytes.get(hs..he) else {
        verdict.integrity = unverifiable("the /ByteRange hole is outside the file");
        verdict.notes = notes;
        return verdict;
    };
    let mut der = Vec::with_capacity(hole_bytes.len() / 2);
    let mut nibble: Option<u8> = None;
    let inner = hole_bytes
        .get(1..hole_bytes.len().saturating_sub(1))
        .unwrap_or(&[]);
    for &b in inner {
        let v = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            b' ' | b'\n' | b'\r' | b'\t' | b'\x0C' | b'\0' => continue,
            _ => {
                verdict.integrity = unverifiable(
                    "the /ByteRange hole holds something other than a hex string (SI-W3)",
                );
                return verdict;
            }
        };
        match nibble.take() {
            None => nibble = Some(v),
            Some(hi) => der.push((hi << 4) | v),
        }
    }
    if let Some(hi) = nibble {
        der.push(hi << 4); // §7.3.4.3: a final odd digit is padded (SI-W5)
        notes.push("the /Contents hex string has an odd digit count; §7.3.4.3 pads it".into());
    }

    // --- the CMS object, by its own DER length (SI-W4, SI-A3) ---
    let Some((outer, trailing)) = crate::asn1::read(&der) else {
        verdict.integrity = unverifiable("the /Contents is not a DER object");
        verdict.notes = notes;
        return verdict;
    };
    if trailing.iter().any(|&b| b != 0) {
        notes.push(
            "the /Contents padding after the CMS object is not all zeros (§12.8.3.3.2 says it shall be); tolerated"
                .into(),
        );
    }
    let Some(sd) = cms::parse_signed_data(outer.raw) else {
        verdict.integrity = unverifiable("the /Contents is not a CMS SignedData pdfcer can read");
        verdict.notes = notes;
        return verdict;
    };
    let Some(signer) = sd.signer.as_ref() else {
        verdict.integrity = unverifiable("the SignedData carries no SignerInfo");
        verdict.notes = notes;
        return verdict;
    };
    if sd.signer_count > 1 {
        notes.push(format!(
            "the SignedData carries {} signers; only the first is verified",
            sd.signer_count
        ));
    }
    if sd.version != 1 || signer.version != 1 {
        notes.push(format!(
            "CMS versions are SignedData {} / SignerInfo {} (RFC 5652 §5.1 expects 1/1 for this shape); not enforced",
            sd.version, signer.version
        ));
    }
    verdict.signing_time = signer.signing_time.clone();

    // --- the signer's certificate (a claim) ---
    let Some(cert) = sd.signer_certificate() else {
        verdict.integrity = unverifiable(
            "the SignedData does not carry the signer's certificate (§12.8.3.3.1: it shall)",
        );
        verdict.notes = notes;
        return verdict;
    };
    verdict.signer_subject = Some(cert.subject.clone());
    verdict.signer_issuer = Some(cert.issuer.clone());
    verdict.cert_not_before = cert.not_before.clone();
    verdict.cert_not_after = cert.not_after.clone();

    // --- the digest algorithm (SI-W13: digestAlgorithm is authoritative) ---
    let Some(hash) = hash_for(&signer.digest_alg.oid) else {
        verdict.integrity = unverifiable(&format!(
            "the digest algorithm {} is not one pdfcer computes (SHA-1/256/384/512)",
            signer.digest_alg.oid
        ));
        verdict.notes = notes;
        return verdict;
    };
    if hash == Hash::Sha1 {
        notes.push(
            "the signature digest is SHA-1, which is no longer considered collision-resistant; ISO 32000-1 permits it"
                .into(),
        );
    }

    // --- D and the message digest (SI-W1, SI-W2) ---
    let mut data = Vec::with_capacity(coverage_len(&ranges));
    for (off, len) in &ranges {
        if let Some(part) = bytes.get(*off as usize..(*off + *len) as usize) {
            data.extend_from_slice(part);
        }
    }
    let Some(md_claimed) = signer.message_digest else {
        verdict.integrity = unverifiable(
            "the SignerInfo has no messageDigest signed attribute (RFC 5652 §5.4 requires one when attributes are signed)",
        );
        verdict.notes = notes;
        return verdict;
    };
    let md_actual = if sf == "adbe.pkcs7.sha1" {
        let Some(inner) = sd.econtent else {
            verdict.integrity = unverifiable(
                "adbe.pkcs7.sha1 requires the SHA-1 of the byte range as encapsulated content, and none is present",
            );
            verdict.notes = notes;
            return verdict;
        };
        if inner != sha1(&data) {
            verdict.integrity = Integrity::DigestMismatch;
            verdict.notes = notes;
            return verdict;
        }
        hash.digest(inner)
    } else {
        if sd.econtent.is_some() {
            notes.push(
                "a detached subfilter carries encapsulated content, which §12.8.3.3.1 says it shall not; ignored"
                    .into(),
            );
        }
        hash.digest(&data)
    };
    if md_actual != md_claimed {
        verdict.integrity = Integrity::DigestMismatch;
        verdict.notes = notes;
        return verdict;
    }

    // --- content-type must be id-data (RFC 5652 §5.6) ---
    if signer.content_type.as_deref() != Some(oid::DATA) || sd.content_type != oid::DATA {
        verdict.integrity = Integrity::SignatureInvalid;
        notes.push(
            "the content-type signed attribute or eContentType is not id-data (RFC 5652 §5.6 makes that a validity failure)"
                .into(),
        );
        verdict.notes = notes;
        return verdict;
    }

    // --- the signature over DER(SET OF signedAttrs) (SI-C2) ---
    let Some(signed_attrs) = signer.signed_attrs_der.as_deref() else {
        verdict.integrity = unverifiable("the SignerInfo has no signed attributes");
        verdict.notes = notes;
        return verdict;
    };
    let attrs_digest = hash.digest(signed_attrs);
    let (ok, alg_name) = match check_signature(
        &cert.key,
        &signer.signature_alg,
        hash,
        &attrs_digest,
        signer.signature,
        &mut notes,
    ) {
        Ok(pair) => pair,
        Err(reason) => {
            verdict.integrity = Integrity::Unverifiable { reason };
            verdict.notes = notes;
            return verdict;
        }
    };
    verdict.integrity = if ok {
        Integrity::Verified {
            digest_algorithm: hash.name(),
            signature_algorithm: alg_name,
        }
    } else {
        Integrity::SignatureInvalid
    };
    // Trust (Pass 10.3): only when the caller supplied anchors. Integrity above
    // is independent -- a valid signature by an untrusted signer is Verified +
    // Untrusted.
    if let Some(anchors) = anchors {
        verdict.trust = match sd.signer_certificate_der() {
            None => {
                notes.push(
                    "trust: the signer certificate is not embedded, so trust could not be evaluated"
                        .to_owned(),
                );
                Trust::SignerUnknown
            }
            Some(signer_der) => {
                // The signing time is the RFC 5280 reference clock: was the
                // chain valid WHEN the document was signed (Pass 10.5)?
                let now = signer.signing_time.as_deref();
                match crate::trust_chain::evaluate(signer_der, &sd.certificates, anchors, now) {
                    crate::trust_chain::ChainVerdict::Trusted {
                        anchor_subject,
                        source,
                        checks,
                    } => {
                        let validity = if checks.validity_checked {
                            "validity dates were checked at the signing time"
                        } else {
                            "validity dates were NOT checked (no signing-time clock)"
                        };
                        notes.push(format!(
                            "trust: the signer chains by signature to a trusted anchor, and RFC 5280 CA/key-usage constraints held; {validity}. Certificate revocation (CRL/OCSP) is NOT checked -- pdfcer-core never touches the network (Pass 10.5)."
                        ));
                        Trust::Trusted {
                            anchor_subject,
                            source,
                            validity_checked: checks.validity_checked,
                        }
                    }
                    crate::trust_chain::ChainVerdict::Untrusted { reason } => {
                        Trust::Untrusted { reason }
                    }
                    crate::trust_chain::ChainVerdict::SignerUnparseable => Trust::SignerUnknown,
                }
            }
        };
    }
    verdict.notes = notes;
    verdict
}

fn coverage_len(ranges: &[(u64, u64)]) -> usize {
    ranges
        .iter()
        .map(|(_, l)| usize::try_from(*l).unwrap_or(0))
        .sum()
}

fn hash_for(oid_str: &str) -> Option<Hash> {
    match oid_str {
        oid::SHA1 => Some(Hash::Sha1),
        oid::SHA256 => Some(Hash::Sha256),
        oid::SHA384 => Some(Hash::Sha384),
        oid::SHA512 => Some(Hash::Sha512),
        _ => None,
    }
}

/// Verify `signature` over `attrs_digest` with `key` under `alg`.
/// `Ok((verified, algorithm name))`, or `Err(reason)` when the combination
/// is one pdfcer does not implement.
fn check_signature(
    key: &PublicKey<'_>,
    alg: &cms::AlgId<'_>,
    hash: Hash,
    attrs_digest: &[u8],
    signature: &[u8],
    notes: &mut Vec<String>,
) -> Result<(bool, String), String> {
    match (key, alg.oid.as_str()) {
        (
            PublicKey::Rsa { n, e },
            oid::RSA_ENCRYPTION
            | oid::SHA1_WITH_RSA
            | oid::SHA256_WITH_RSA
            | oid::SHA384_WITH_RSA
            | oid::SHA512_WITH_RSA,
        ) => {
            // SI-W13: a shaNWithRSA identifier that disagrees with
            // digestAlgorithm is disclosed; digestAlgorithm wins.
            let named = match alg.oid.as_str() {
                oid::SHA1_WITH_RSA => Some(Hash::Sha1),
                oid::SHA256_WITH_RSA => Some(Hash::Sha256),
                oid::SHA384_WITH_RSA => Some(Hash::Sha384),
                oid::SHA512_WITH_RSA => Some(Hash::Sha512),
                _ => None,
            };
            if let Some(nh) = named
                && nh != hash
            {
                notes.push(format!(
                    "signatureAlgorithm names {} but digestAlgorithm is {}; RFC 3370 §3.2 makes digestAlgorithm authoritative",
                    nh.name(),
                    hash.name()
                ));
            }
            let rsa = RsaPublicKey {
                n: Uint::from_be_bytes(n),
                e: Uint::from_be_bytes(e),
            };
            let bits = rsa.n.bits();
            Ok((
                rsa.verify_pkcs1v15(hash, attrs_digest, signature),
                format!("RSA PKCS#1 v1.5, {bits}-bit"),
            ))
        }
        (PublicKey::Rsa { n, e }, oid::RSASSA_PSS) => {
            let (pss_hash, mgf_hash, salt_len) = pss_params(alg)?;
            if pss_hash != hash {
                return Err(format!(
                    "RSASSA-PSS hashAlgorithm {} disagrees with digestAlgorithm {}",
                    pss_hash.name(),
                    hash.name()
                ));
            }
            let rsa = RsaPublicKey {
                n: Uint::from_be_bytes(n),
                e: Uint::from_be_bytes(e),
            };
            let bits = rsa.n.bits();
            Ok((
                rsa.verify_pss(hash, mgf_hash, salt_len, attrs_digest, signature),
                format!(
                    "RSASSA-PSS ({}, MGF1-{}, salt {salt_len}), {bits}-bit",
                    hash.name(),
                    mgf_hash.name()
                ),
            ))
        }
        (
            PublicKey::Ec { curve_oid, point },
            oid::ECDSA_SHA1 | oid::ECDSA_SHA256 | oid::ECDSA_SHA384 | oid::ECDSA_SHA512,
        ) => {
            let Some(curve) = Curve::from_oid(curve_oid) else {
                return Err(format!(
                    "the certificate's curve {curve_oid} is not one pdfcer verifies (P-256, P-384)"
                ));
            };
            let Some(key) = EcPublicKey::from_sec1(curve, point) else {
                return Err(
                    "the certificate's EC public key is not an uncompressed point on its curve"
                        .into(),
                );
            };
            // ECDSA-Sig-Value ::= SEQUENCE { r INTEGER, s INTEGER } (RFC 5480 §2.2)
            let (seq, _) = crate::asn1::expect(signature, crate::asn1::SEQUENCE)
                .ok_or_else(|| "the ECDSA signature value is not a DER SEQUENCE".to_string())?;
            let parts = crate::asn1::children(seq).unwrap_or_default();
            let (Some(r), Some(s)) = (
                parts.first().and_then(|t| crate::asn1::integer_bytes(*t)),
                parts.get(1).and_then(|t| crate::asn1::integer_bytes(*t)),
            ) else {
                return Err("the ECDSA signature value does not hold two INTEGERs".into());
            };
            Ok((
                key.verify(attrs_digest, r, s),
                format!("ECDSA {}", curve.name()),
            ))
        }
        (PublicKey::Other(k), _) => Err(format!(
            "the certificate's key algorithm {k} is not one pdfcer verifies (RSA, EC)"
        )),
        (_, other) => Err(format!(
            "the signature algorithm {other} is not one pdfcer verifies"
        )),
    }
}

/// `RSASSA-PSS-params` (RFC 4055 §3.1): `(hash, mgf1 hash, salt length)`,
/// with the RFC's defaults for absent fields.
pub(crate) fn pss_params(alg: &cms::AlgId<'_>) -> Result<(Hash, Hash, usize), String> {
    let mut hash = Hash::Sha1;
    let mut mgf = Hash::Sha1;
    let mut salt = 20usize;
    let Some(params) = alg.params.filter(|p| p.tag == crate::asn1::SEQUENCE) else {
        return Ok((hash, mgf, salt));
    };
    for field in crate::asn1::children(params).unwrap_or_default() {
        match field.tag {
            0xA0 => {
                let (h, _) = crate::asn1::read(field.content).ok_or("bad PSS hashAlgorithm")?;
                let oid_s = crate::asn1::children(h)
                    .and_then(|k| k.first().copied())
                    .and_then(|t| crate::asn1::oid_to_string(t.content))
                    .ok_or("bad PSS hashAlgorithm")?;
                hash = hash_for(&oid_s).ok_or_else(|| format!("PSS hash {oid_s} unsupported"))?;
            }
            0xA1 => {
                let (m, _) = crate::asn1::read(field.content).ok_or("bad PSS maskGenAlgorithm")?;
                let kids = crate::asn1::children(m).ok_or("bad PSS maskGenAlgorithm")?;
                let mgf_oid = kids
                    .first()
                    .and_then(|t| crate::asn1::oid_to_string(t.content))
                    .ok_or("bad PSS maskGenAlgorithm")?;
                if mgf_oid != oid::MGF1 {
                    return Err(format!(
                        "PSS mask generation function {mgf_oid} is not MGF1"
                    ));
                }
                let inner = kids.get(1).copied().ok_or("bad PSS MGF1 parameters")?;
                let oid_s = crate::asn1::children(inner)
                    .and_then(|k| k.first().copied())
                    .and_then(|t| crate::asn1::oid_to_string(t.content))
                    .ok_or("bad PSS MGF1 hash")?;
                mgf =
                    hash_for(&oid_s).ok_or_else(|| format!("PSS MGF1 hash {oid_s} unsupported"))?;
            }
            0xA2 => {
                let (i, _) = crate::asn1::read(field.content).ok_or("bad PSS saltLength")?;
                let b = crate::asn1::integer_bytes(i).ok_or("bad PSS saltLength")?;
                salt = b.iter().fold(0usize, |acc, &x| (acc << 8) | usize::from(x));
            }
            0xA3 => {
                let (i, _) = crate::asn1::read(field.content).ok_or("bad PSS trailerField")?;
                if crate::asn1::integer_bytes(i) != Some(&[1]) {
                    return Err("PSS trailerField is not 1 (0xBC)".into());
                }
            }
            _ => {}
        }
    }
    Ok((hash, mgf, salt))
}
