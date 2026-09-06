//! `Pass 10.14` — signing hardening owed from the `10.7`–`10.9` cut.
//!
//! What this file proves, and what the `sign_document.rs` round trips do
//! not:
//!
//! 1. **The verifiers DISCRIMINATE.** A round trip shows a correct build is
//!    accepted; it does not show a broken one is rejected. Each sabotage
//!    below builds a `SignedData` with exactly one defect through the
//!    doc-hidden [`build_with`] hook, splices it into a real signed file,
//!    and asserts that BOTH pdfcer's verifier and OpenSSL refuse it — with
//!    a control (`Sabotage::None` spliced the same way) that both accept,
//!    so a refusal cannot be an artefact of the splice.
//! 2. **Key/leaf pairing without `localKeyId`** works by public-key
//!    identity (the fallback `Pkcs12Signer::from_der` documents), on a
//!    fixture that is proven to carry no `localKeyId` at all.
//! 3. **A foreign signature survives pdfcer signing on top of it**, and
//!    pdfcer's signature survives a foreign countersignature (pyHanko).
//! 4. **The base revision is byte-identical** under every signing write
//!    (the append-only half of the round-trip invariant, measured).
//! 5. **The composed appearance** carries the signer, date, reason and
//!    location, and a rectangle too small for it is refused by name.
//! 6. **ECDSA P-384** end to end, through both verifiers.

#![cfg(feature = "signing")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::sign::apply::{SignApplyError, SignRequest};
use pdfcer_core::sign::cms_build::{Sabotage, build_with};
use pdfcer_core::sign::pkcs12::Pkcs12Signer;
use pdfcer_core::sign::{SignatureAlgorithm, Signer};
use pdfcer_core::signature_verify::{Integrity, SignatureVerdict, verify_all};
use pdfcer_core::writer::SaveOptions;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(rel)).unwrap()
}

fn pfx(name: &str) -> Pkcs12Signer {
    Pkcs12Signer::from_der(&read(&format!("signing/{name}")), "pdfcer").unwrap()
}

const T0: &str = "D:20260906031844Z";

fn sign_bytes(base: &[u8], signer: &dyn Signer, req: &SignRequest) -> Vec<u8> {
    let mut s = EditSession::new(Document::from_bytes(base.to_vec()).unwrap());
    let (out, _) = s.sign(signer, req, &SaveOptions::identity()).expect("sign");
    // Criterion 4, measured on every write: the base revision is untouched.
    assert_eq!(
        &out[..base.len()],
        base,
        "the base revision must be byte-identical"
    );
    out
}

fn verdicts(bytes: &[u8]) -> Vec<SignatureVerdict> {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    verify_all(&doc.view(), bytes)
}

fn verdict<'a>(v: &'a [SignatureVerdict], field: &str) -> &'a SignatureVerdict {
    v.iter()
        .find(|v| v.field_name.as_deref() == Some(field))
        .unwrap_or_else(|| panic!("no verdict for {field}: {v:?}"))
}

fn is_verified(v: &SignatureVerdict) -> bool {
    matches!(v.integrity, Integrity::Verified { .. })
}

/// The `/ByteRange` spans of `field`, concatenated — the CMS content.
fn signed_content(bytes: &[u8], field: &str) -> Vec<u8> {
    let all = verdicts(bytes);
    let v = verdict(&all, field);
    let mut content = Vec::new();
    for (start, len) in &v.coverage.ranges {
        content.extend_from_slice(&bytes[*start as usize..(*start + *len) as usize]);
    }
    content
}

/// The DER `/Contents` of `field` with the hex-zero padding removed.
fn cms_of(bytes: &[u8], field: &str) -> Vec<u8> {
    use pdfcer_core::graph::ObjectGraph as _;
    use pdfcer_core::object::Object;
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let form = pdfcer_core::forms::parse_acroform(&doc.view()).unwrap();
    let f = form
        .fields
        .iter()
        .find(|f| f.fully_qualified_name == field)
        .unwrap();
    let sig = doc
        .view()
        .resolve(&Object::Reference(f.id))
        .as_dict()
        .and_then(|d| d.get(b"V").cloned())
        .map(|v| doc.view().resolve(&v).clone())
        .unwrap();
    let contents = match sig.as_dict().and_then(|d| d.get(b"Contents")) {
        Some(Object::String(s)) => s.clone(),
        other => panic!("no /Contents: {other:?}"),
    };
    let (len, hdr) = der_outer_len(&contents);
    contents[..hdr + len].to_vec()
}

fn der_outer_len(der: &[u8]) -> (usize, usize) {
    assert_eq!(der[0], 0x30);
    let first = der[1] as usize;
    if first < 0x80 {
        (first, 2)
    } else {
        let n = first & 0x7F;
        let mut len = 0usize;
        for i in 0..n {
            len = (len << 8) | der[2 + i] as usize;
        }
        (len, 2 + n)
    }
}

/// Overwrite the LAST `/Contents <…>` hex hole with `der` (zero-padded to
/// the hole's length) — the same in-place patch the writer performs, done
/// from the outside so a differently-built CMS can be tested against the
/// file's real `/ByteRange`.
fn splice_cms(bytes: &mut [u8], der: &[u8]) {
    let needle = b"/Contents <";
    let start = bytes
        .windows(needle.len())
        .rposition(|w| w == needle)
        .expect("a /Contents hex string")
        + needle.len();
    let end = start + bytes[start..].iter().position(|&b| b == b'>').unwrap();
    let hole = &mut bytes[start..end];
    assert!(
        der.len() * 2 <= hole.len(),
        "the sabotaged CMS must fit the hole"
    );
    let hex: Vec<u8> = der
        .iter()
        .flat_map(|b| format!("{b:02X}").into_bytes())
        .collect();
    hole[..hex.len()].copy_from_slice(&hex);
    hole[hex.len()..].fill(b'0');
}

fn openssl_verifies(cms_der: &[u8], content: &[u8]) -> Result<(), String> {
    static CALL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = CALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "pdfcer-hardening-oracle-{}-{n}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let sig = dir.join("sig.der");
    let data = dir.join("content.bin");
    std::fs::write(&sig, cms_der).unwrap();
    std::fs::write(&data, content).unwrap();
    let out = std::process::Command::new("openssl")
        .args(["cms", "-verify", "-noverify", "-binary", "-inform", "DER"])
        .arg("-in")
        .arg(&sig)
        .arg("-content")
        .arg(&data)
        .arg("-out")
        .arg(dir.join("out.bin"))
        .output()
        .map_err(|e| format!("openssl not runnable: {e} — the oracle needs OpenSSL on PATH"))?;
    let _ = std::fs::remove_dir_all(&dir);
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

// ---------------------------------------------------------------------------
// 1. The verifiers discriminate (criterion 1)
// ---------------------------------------------------------------------------

/// Sign `hello.pdf`, then replace the CMS with one built under `sabotage`
/// over the file's REAL byte-range digest. Returns (pdfcer verified?,
/// OpenSSL verified?).
fn sabotaged(sabotage: Sabotage) -> (bool, bool) {
    let rsa = pfx("rsa2048-modern.pfx");
    let base = read("hello.pdf");
    let mut bytes = sign_bytes(&base, &rsa, &SignRequest::at(T0));
    let content = signed_content(&bytes, "Signature1");
    let alg = SignatureAlgorithm::RsaPkcs1v15Sha256;
    let digest = alg.digest(&content);
    let cms = build_with(&rsa, alg, &digest, sabotage).expect("build");
    splice_cms(&mut bytes, &cms.der);
    // The splice must not have moved the byte ranges.
    assert_eq!(signed_content(&bytes, "Signature1"), content);
    let ours = is_verified(verdict(&verdicts(&bytes), "Signature1"));
    let theirs = openssl_verifies(&cms_of(&bytes, "Signature1"), &content).is_ok();
    (ours, theirs)
}

#[test]
fn the_control_splice_is_accepted_by_both_verifiers() {
    // Without this the three refusals below could be the splice's doing.
    assert_eq!(sabotaged(Sabotage::None), (true, true));
}

#[test]
fn attributes_digested_under_the_context_tag_are_rejected_by_both() {
    // CB-4: the signature must be over the SET OF (0x31) encoding, not the
    // [0] IMPLICIT (0xA0) wire form.
    assert_eq!(sabotaged(Sabotage::DigestUnderContextTag), (false, false));
}

#[test]
fn attributes_reordered_after_signing_are_rejected_by_both() {
    // The tamper that matters: the signature was made over the DER-sorted
    // set and the wire order was changed afterwards. Both verifiers hash
    // the attributes AS RECEIVED, so the digest no longer matches.
    assert_eq!(sabotaged(Sabotage::ReorderedAfterSigning), (false, false));
}

#[test]
fn an_unsorted_set_signed_consistently_is_accepted_by_both_as_received() {
    // MEASURED 2026-09-06, and it amends criterion 1(b) of Pass 10.14,
    // which expected "both reject": a SET OF emitted out of X.690 §11.6
    // order but signed over exactly those bytes verifies under pdfcer AND
    // OpenSSL 1.1.1 — neither re-encodes the set canonically before
    // hashing. pdfcer keeps the oracle's rule on purpose: a stricter
    // verifier would refuse signatures every other reader accepts, and the
    // reordering that is a real attack (above) is caught by this rule. The
    // assertion pins agreement with the oracle, so a future divergence
    // either way is noticed.
    assert_eq!(sabotaged(Sabotage::UnsortedAttributes), (true, true));
}

#[test]
fn a_wrong_message_digest_is_rejected_by_both() {
    assert_eq!(sabotaged(Sabotage::WrongMessageDigest), (false, false));
}

// ---------------------------------------------------------------------------
// 2. Pairing without localKeyId (criterion 2)
// ---------------------------------------------------------------------------

#[test]
fn a_store_without_local_key_id_pairs_key_and_leaf_by_public_key() {
    let der = read("signing/rsa2048-nolocalkeyid.pfx");
    // The fixture is what it claims: no localKeyId OID anywhere in it
    // (1.2.840.113549.1.9.21 = 06 09 2A 86 48 86 F7 0D 01 09 15).
    let oid = [
        0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x15,
    ];
    assert!(
        !der.windows(oid.len()).any(|w| w == oid),
        "the fixture must carry no localKeyId"
    );
    let signer = Pkcs12Signer::from_der(&der, "pdfcer").expect("pairs by public key");
    assert_eq!(
        signer.report().mac.as_deref(),
        Some("SHA-256"),
        "the recomputed MAC verified"
    );
    assert_eq!(
        signer.certificate_chain()[0],
        read("signing/rsa2048.cer"),
        "the leaf paired is the key's own certificate"
    );
    let bytes = sign_bytes(&read("hello.pdf"), &signer, &SignRequest::at(T0));
    assert!(is_verified(verdict(&verdicts(&bytes), "Signature1")));
}

// ---------------------------------------------------------------------------
// 3. Foreign signatures (criterion 3)
// ---------------------------------------------------------------------------

#[test]
fn pdfcer_signs_on_top_of_a_pyhanko_signature_and_both_verify() {
    let base = read("signing/foreign-pyhanko-first.pdf");
    let before = verdicts(&base);
    assert_eq!(before.len(), 1);
    let foreign_before = verdict(&before, "ForeignSig");
    assert!(
        is_verified(foreign_before),
        "{:?}",
        foreign_before.integrity
    );
    let foreign_ranges = foreign_before.coverage.ranges.clone();

    let bytes = sign_bytes(&base, &pfx("ecp256-modern.pfx"), &SignRequest::at(T0));
    let after = verdicts(&bytes);
    assert_eq!(after.len(), 2, "{after:?}");
    let foreign_after = verdict(&after, "ForeignSig");
    assert!(is_verified(foreign_after), "{:?}", foreign_after.integrity);
    assert_eq!(
        foreign_after.coverage.ranges, foreign_ranges,
        "the foreign /ByteRange is intact"
    );
    assert!(is_verified(verdict(&after, "Signature1")));
    // And OpenSSL agrees about pdfcer's signature over the foreign base.
    openssl_verifies(
        &cms_of(&bytes, "Signature1"),
        &signed_content(&bytes, "Signature1"),
    )
    .expect("openssl verifies pdfcer's signature on the foreign-signed base");
}

#[test]
fn a_pyhanko_countersignature_over_pdfcer_output_leaves_both_verifiable() {
    // pdfcer signed first (Signature1), pyHanko countersigned (ForeignSig):
    // the generator's second direction, MEASURED because pyHanko was
    // available when the fixture was minted.
    let bytes = read("signing/pdfcer-then-pyhanko.pdf");
    let all = verdicts(&bytes);
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(
        is_verified(verdict(&all, "Signature1")),
        "{:?}",
        verdict(&all, "Signature1").integrity
    );
    assert!(
        is_verified(verdict(&all, "ForeignSig")),
        "{:?}",
        verdict(&all, "ForeignSig").integrity
    );
    openssl_verifies(
        &cms_of(&bytes, "Signature1"),
        &signed_content(&bytes, "Signature1"),
    )
    .expect("pdfcer's first signature still verifies under OpenSSL");
}

// ---------------------------------------------------------------------------
// 5. The composed appearance (criterion 5)
// ---------------------------------------------------------------------------

fn visible(w: f64, h: f64) -> SignRequest {
    let mut req = SignRequest::at(T0);
    req.visible = Some((
        0,
        pdfcer_core::page_tree::Rect {
            llx: 72.0,
            lly: 600.0,
            urx: 72.0 + w,
            ury: 600.0 + h,
        },
    ));
    req.reason = Some("Approved for release".to_owned());
    req.location = Some("Toronto".to_owned());
    req
}

#[test]
fn the_appearance_carries_signer_date_reason_and_location() {
    let rsa = pfx("rsa2048-modern.pfx");
    let mut s = EditSession::new(Document::from_bytes(read("hello.pdf")).unwrap());
    let (bytes, report) = s
        .sign(&rsa, &visible(230.0, 60.0), &SaveOptions::identity())
        .unwrap();
    assert_eq!(
        report.appearance_lines,
        vec![
            "Digitally signed by pdfcer synthetic RSA signer (test fixture, trust nothing)"
                .to_owned(),
            "Date: 2026.09.06 03:18:44 Z".to_owned(),
            "Reason: Approved for release".to_owned(),
            "Location: Toronto".to_owned(),
        ]
    );
    // The lines are IN the file (the /AP /N stream is plain), set in /Helv,
    // and the frame of the first cut is still drawn.
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains(
            "(Digitally signed by pdfcer synthetic RSA signer \\(test fixture, trust nothing\\)) Tj"
        ),
        "{text}"
    );
    assert!(text.contains("(Reason: Approved for release) Tj"));
    assert!(text.contains("(Location: Toronto) Tj"));
    assert!(text.contains("/Helv "));
    assert!(text.contains("/BaseFont /Helvetica"));
    assert!(text.contains(" re S"));
    assert!(is_verified(verdict(&verdicts(&bytes), "Signature1")));
    // Invisible: nothing composed, nothing claimed.
    let (_, invisible) = EditSession::new(Document::from_bytes(read("hello.pdf")).unwrap())
        .sign(&rsa, &SignRequest::at(T0), &SaveOptions::identity())
        .unwrap();
    assert!(invisible.appearance_lines.is_empty());
}

#[test]
fn a_rectangle_too_small_for_the_appearance_is_refused_before_any_write() {
    let rsa = pfx("rsa2048-modern.pfx");
    let mut s = EditSession::new(Document::from_bytes(read("hello.pdf")).unwrap());
    let err = s
        .sign(&rsa, &visible(20.0, 8.0), &SaveOptions::identity())
        .expect_err("four lines do not fit 20 x 8 pt at 4 pt");
    assert!(
        matches!(err, SignApplyError::AppearanceOverflow { lines: 4, .. }),
        "{err:?}"
    );
    assert!(err.to_string().contains("enlarge --visible"));
    assert_eq!(s.undo_depth(), 0, "nothing was staged");
    assert!(!s.is_modified());
    // The same session signs fine with a rectangle that fits.
    s.sign(&rsa, &visible(230.0, 60.0), &SaveOptions::identity())
        .expect("a fitting rectangle signs");
}

// ---------------------------------------------------------------------------
// 6. ECDSA P-384 end to end (criterion 6)
// ---------------------------------------------------------------------------

#[test]
fn ecdsa_p384_signs_and_both_verifiers_accept_it() {
    let ec = pfx("ecp384-modern.pfx");
    let mut s = EditSession::new(Document::from_bytes(read("hello.pdf")).unwrap());
    let (bytes, report) = s
        .sign(&ec, &SignRequest::at(T0), &SaveOptions::identity())
        .unwrap();
    assert_eq!(report.algorithm, SignatureAlgorithm::EcdsaP384Sha384);
    assert!(is_verified(verdict(&verdicts(&bytes), "Signature1")));
    openssl_verifies(
        &cms_of(&bytes, "Signature1"),
        &signed_content(&bytes, "Signature1"),
    )
    .expect("openssl verifies the P-384 signature");
    assert_eq!(ec.certificate_chain()[0], read("signing/ecp384.cer"));
}
