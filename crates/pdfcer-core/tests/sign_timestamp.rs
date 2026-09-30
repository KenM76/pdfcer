//! PAdES B-T signature time-stamps (`Pass 10.11`), with OpenSSL's `ts -reply`
//! as the time-stamping authority and `cms -verify` as the oracle.
#![cfg(feature = "signing")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::sign::apply::{SignApplyError, SignReport, SignRequest};
use pdfcer_core::sign::pkcs12::Pkcs12Signer;
use pdfcer_core::sign::timestamp::{TimestampAuthority, TimestampError};
use pdfcer_core::signature_verify::{Integrity, verify_all};
use pdfcer_core::writer::SaveOptions;

const T0: &str = "D:20260927120000Z";
const TSA_CONFIG: &str = "[ tsa ]\ndefault_tsa = cfg\n[ cfg ]\nserial = ./serial\n\
crypto_device = builtin\nsigner_digest = sha256\ndefault_policy = 1.2.3.4.1\n\
digests = sha256, sha384, sha512\naccuracy = secs:1\nordering = no\ntsa_name = no\n\
ess_cert_id_chain = no\ness_cert_id_alg = sha256\n";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic")
}

fn signer(name: &str) -> Pkcs12Signer {
    let bytes = std::fs::read(fixtures().join("signing").join(name)).unwrap();
    Pkcs12Signer::from_der(&bytes, "pdfcer").unwrap()
}

/// A fresh scratch directory per call (tests run in parallel threads).
fn scratch(tag: &str) -> PathBuf {
    static CALL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = CALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "pdfcer-ts-{tag}-{}-{n}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn openssl(dir: &Path, args: &[&str]) -> std::process::Output {
    let out = std::process::Command::new("openssl")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("openssl on PATH");
    assert!(
        out.status.success(),
        "openssl {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// `openssl ts -reply` over the synthetic TSA key, optionally rewriting
/// the response before pdfcer sees it. Records every response it gave.
struct OpensslTsa {
    dir: PathBuf,
    tamper: fn(Vec<u8>) -> Vec<u8>,
    given: Mutex<Vec<Vec<u8>>>,
}

impl OpensslTsa {
    fn new(tamper: fn(Vec<u8>) -> Vec<u8>) -> Self {
        let dir = scratch("tsa");
        let signing = fixtures().join("signing");
        std::fs::write(dir.join("tsa.cnf"), TSA_CONFIG).unwrap();
        std::fs::write(dir.join("serial"), "01\n").unwrap();
        let key = signing.join("tsa-rsa2048.key.der");
        let cert = signing.join("tsa-rsa2048.cer");
        openssl(
            &dir,
            &[
                "pkey",
                "-inform",
                "DER",
                "-in",
                key.to_str().unwrap(),
                "-out",
                "k.pem",
            ],
        );
        openssl(
            &dir,
            &[
                "x509",
                "-inform",
                "DER",
                "-in",
                cert.to_str().unwrap(),
                "-out",
                "c.pem",
            ],
        );
        Self {
            dir,
            tamper,
            given: Mutex::new(Vec::new()),
        }
    }
}

impl TimestampAuthority for OpensslTsa {
    fn time_stamp(&self, request_der: &[u8]) -> Result<Vec<u8>, String> {
        std::fs::write(self.dir.join("q.tsq"), request_der).unwrap();
        openssl(
            &self.dir,
            &[
                "ts",
                "-reply",
                "-config",
                "tsa.cnf",
                "-queryfile",
                "q.tsq",
                "-inkey",
                "k.pem",
                "-signer",
                "c.pem",
                "-out",
                "r.tsr",
            ],
        );
        let response = (self.tamper)(std::fs::read(self.dir.join("r.tsr")).unwrap());
        self.given.lock().unwrap().push(response.clone());
        Ok(response)
    }
}

/// A fixed answer, for replay and hand-built rejections.
struct Canned(Result<Vec<u8>, String>);

impl TimestampAuthority for Canned {
    fn time_stamp(&self, _: &[u8]) -> Result<Vec<u8>, String> {
        self.0.clone()
    }
}

fn sign_hello(
    pfx: &str,
    tsa: &dyn TimestampAuthority,
) -> Result<(Vec<u8>, SignReport), SignApplyError> {
    let mut s = EditSession::new(Document::load(&fixtures().join("hello.pdf")).unwrap());
    s.sign_with_timestamp(
        &signer(pfx),
        tsa,
        &SignRequest::at(T0),
        &SaveOptions::identity(),
    )
}

/// The trimmed DER CMS from the `/Contents` hole, found via the verifier's
/// byte range: the hole is the gap between the two spans.
fn embedded_cms(bytes: &[u8]) -> Vec<u8> {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let v = verify_all(&doc.view(), bytes).remove(0);
    let (a_off, a_len) = v.coverage.ranges[0];
    let (b_off, _) = v.coverage.ranges[1];
    let hex = &bytes[(a_off + a_len) as usize + 1..b_off as usize - 1];
    let raw: Vec<u8> = hex
        .chunks(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect();
    let len = match raw[1] {
        n if n < 0x80 => 2 + n as usize,
        n => {
            let k = (n & 0x7F) as usize;
            2 + k
                + raw[2..2 + k]
                    .iter()
                    .fold(0usize, |a, &b| (a << 8) | b as usize)
        }
    };
    raw[..len].to_vec()
}

#[test]
fn a_timestamped_signature_is_b_t_and_openssl_accepts_both_the_cms_and_the_token() {
    let tsa = OpensslTsa::new(|r| r);
    let (bytes, report) = sign_hello("rsa2048-modern.pfx", &tsa).expect("sign");
    assert_eq!(report.pades_level, "B-T");
    let ts = report.timestamp.as_ref().expect("timestamp reported");
    assert!(
        ts.tsa_subject.contains("pdfcer synthetic TSA"),
        "{}",
        ts.tsa_subject
    );
    assert_eq!(ts.policy_oid, "1.2.3.4.1");
    assert_eq!(ts.digest_algorithm, "SHA-256");
    assert!(
        ts.gen_time.ends_with('Z') && ts.gen_time.len() == 20,
        "{}",
        ts.gen_time
    );
    assert!(report.cms_bytes > ts.token_bytes);

    // pdfcer's own verifier still says Verified (the token is unsigned).
    let doc = Document::from_bytes(bytes.clone()).unwrap();
    let v = verify_all(&doc.view(), &bytes).remove(0);
    assert!(
        matches!(v.integrity, Integrity::Verified { .. }),
        "{:?}",
        v.integrity
    );

    // Oracle 1: OpenSSL verifies the SignerInfo after the token was added.
    let dir = scratch("oracle");
    let cms = embedded_cms(&bytes);
    let mut content = Vec::new();
    for (off, len) in &v.coverage.ranges {
        content.extend_from_slice(&bytes[*off as usize..(*off + *len) as usize]);
    }
    std::fs::write(dir.join("sig.der"), &cms).unwrap();
    std::fs::write(dir.join("content.bin"), &content).unwrap();
    openssl(
        &dir,
        &[
            "cms",
            "-verify",
            "-noverify",
            "-binary",
            "-inform",
            "DER",
            "-in",
            "sig.der",
            "-content",
            "content.bin",
            "-out",
            "out.bin",
        ],
    );
    // ...and parses the unsigned attribute as a time-stamp token.
    let printed = openssl(
        &dir,
        &[
            "cms", "-cmsout", "-print", "-inform", "DER", "-in", "sig.der",
        ],
    );
    let text = String::from_utf8_lossy(&printed.stdout);
    assert!(text.contains("id-smime-aa-timeStampToken"), "{text}");

    // Oracle 2: the embedded token is byte-identical to the TSA's, and
    // OpenSSL agrees the response stamps THIS signature value (TS-6).
    let response = tsa.given.lock().unwrap()[0].clone();
    std::fs::write(dir.join("r.tsr"), &response).unwrap();
    let token = openssl(
        &dir,
        &[
            "ts",
            "-reply",
            "-in",
            "r.tsr",
            "-token_out",
            "-out",
            "token.der",
        ],
    );
    drop(token);
    let token = std::fs::read(dir.join("token.der")).unwrap();
    assert!(cms.windows(token.len()).any(|w| w == token.as_slice()));
    let sig_value = signature_value(&cms);
    std::fs::write(dir.join("sigvalue.bin"), sig_value).unwrap();
    let tsa_pem = tsa.dir.join("c.pem");
    openssl(
        &dir,
        &[
            "ts",
            "-verify",
            "-in",
            "r.tsr",
            "-data",
            "sigvalue.bin",
            "-CAfile",
            tsa_pem.to_str().unwrap(),
        ],
    );
}

/// The last OCTET STRING of the (single) SignerInfo before its `[1]`
/// unsigned attributes — read with OpenSSL's `asn1parse` so the test does
/// not trust pdfcer's own reader.
fn signature_value(cms: &[u8]) -> Vec<u8> {
    let dir = scratch("asn1");
    std::fs::write(dir.join("c.der"), cms).unwrap();
    let out = openssl(&dir, &["asn1parse", "-inform", "DER", "-in", "c.der", "-i"]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    // The first `cont [ 1 ]` at SignerInfo depth follows the signature.
    let lines: Vec<&str> = text.lines().collect();
    let unsigned = lines
        .iter()
        .rposition(|l| l.contains("cont [ 1 ]"))
        .expect("unsignedAttrs");
    let sig_line = lines[..unsigned]
        .iter()
        .rev()
        .find(|l| l.contains("OCTET STRING"))
        .unwrap();
    let offset: usize = sig_line.split(':').next().unwrap().trim().parse().unwrap();
    let hl: usize = sig_line
        .split("hl=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let l: usize = sig_line
        .split(" l=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    cms[offset + hl..offset + hl + l].to_vec()
}

#[test]
fn a_replayed_response_is_refused_for_its_nonce() {
    // RSA PKCS#1 v1.5 is deterministic: the same document, time and key give
    // the same signature value, so a replayed response matches the imprint
    // and fails ONLY on the nonce.
    let tsa = OpensslTsa::new(|r| r);
    sign_hello("rsa2048-modern.pfx", &tsa).expect("first sign");
    let replay = Canned(Ok(tsa.given.lock().unwrap()[0].clone()));
    let err = sign_hello("rsa2048-modern.pfx", &replay).unwrap_err();
    assert!(
        matches!(
            err,
            SignApplyError::Timestamp(TimestampError::NonceMismatch)
        ),
        "{err:?}"
    );
}

#[test]
fn a_response_for_another_signature_is_refused_for_its_imprint() {
    let tsa = OpensslTsa::new(|r| r);
    sign_hello("rsa2048-modern.pfx", &tsa).expect("first sign");
    let replay = Canned(Ok(tsa.given.lock().unwrap()[0].clone()));
    let err = sign_hello("ecp256-modern.pfx", &replay).unwrap_err();
    assert!(
        matches!(
            err,
            SignApplyError::Timestamp(TimestampError::ImprintMismatch)
        ),
        "{err:?}"
    );
}

#[test]
fn a_token_with_a_damaged_tsa_signature_is_refused() {
    // The token's SignerInfo.signature is the last element of the response.
    let tsa = OpensslTsa::new(|mut r| {
        let last = r.len() - 1;
        r[last] ^= 0x01;
        r
    });
    let err = sign_hello("rsa2048-modern.pfx", &tsa).unwrap_err();
    assert!(
        matches!(
            err,
            SignApplyError::Timestamp(TimestampError::TokenSignatureInvalid(_))
        ),
        "{err:?}"
    );
}

#[test]
fn a_rejection_and_a_transport_failure_are_refused_by_name() {
    // TimeStampResp { PKIStatusInfo { rejection(2) } }
    let rejection = vec![0x30, 0x05, 0x30, 0x03, 0x02, 0x01, 0x02];
    let err = sign_hello("rsa2048-modern.pfx", &Canned(Ok(rejection))).unwrap_err();
    assert!(
        matches!(
            err,
            SignApplyError::Timestamp(TimestampError::Rejected { status: 2, .. })
        ),
        "{err:?}"
    );
    let err = sign_hello("rsa2048-modern.pfx", &Canned(Err("HTTP 503".into()))).unwrap_err();
    assert!(err.to_string().contains("HTTP 503"), "{err}");
}

#[test]
fn a_p384_signature_is_stamped_with_sha384() {
    let tsa = OpensslTsa::new(|r| r);
    let (_, report) = sign_hello("ecp384-modern.pfx", &tsa).expect("sign");
    assert_eq!(report.pades_level, "B-T");
    assert_eq!(report.timestamp.unwrap().digest_algorithm, "SHA-384");
}

/// The `timestamp_response` fuzz seed is a GRANTED answer to the target's
/// fixed request, so the fuzzer starts inside the accept path rather than
/// at the first length check.
#[test]
fn the_fuzz_seed_is_accepted_by_the_fuzz_hook() {
    let seed = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fuzz/corpus/timestamp_response/seed_openssl_granted"),
    )
    .unwrap();
    let len = usize::from(u16::from_be_bytes([seed[0], seed[1]]));
    let response = &seed[2..2 + len];
    assert!(pdfcer_core::sign::timestamp::fuzz_accept_and_embed(
        response,
        &[]
    ));
    let mut flipped = response.to_vec();
    let last = flipped.len() - 1;
    flipped[last] ^= 1;
    assert!(!pdfcer_core::sign::timestamp::fuzz_accept_and_embed(
        &flipped,
        &[]
    ));
}

// ---------------------------------------------------------------------
// Document time-stamps (PAdES B-LTA; ISO 32000-2 §12.8.5)
// ---------------------------------------------------------------------

use pdfcer_core::sign::ltv::ValidationMaterial;
use pdfcer_core::sign::timestamp::{DocTimestampReport, DocTimestampRequest};

fn doc_stamp(
    base: Vec<u8>,
    tsa: &dyn TimestampAuthority,
) -> Result<(Vec<u8>, DocTimestampReport), SignApplyError> {
    let mut s = EditSession::new(Document::from_bytes(base).unwrap());
    s.add_document_timestamp(
        tsa,
        &DocTimestampRequest::default(),
        &SaveOptions::identity(),
    )
}

fn signed_hello() -> Vec<u8> {
    let mut s = EditSession::new(Document::load(&fixtures().join("hello.pdf")).unwrap());
    s.sign(
        &signer("rsa2048-modern.pfx"),
        &SignRequest::at(T0),
        &SaveOptions::identity(),
    )
    .expect("sign")
    .0
}

/// The `/Contents` hex of the verdict named `field`, decoded and trimmed
/// to its own DER length, plus the covered bytes.
fn hole_and_covered(bytes: &[u8], field: &str) -> (Vec<u8>, Vec<u8>) {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let v = verify_all(&doc.view(), bytes)
        .into_iter()
        .find(|v| v.field_name.as_deref() == Some(field))
        .unwrap();
    let (a_off, a_len) = v.coverage.ranges[0];
    let (b_off, b_len) = v.coverage.ranges[1];
    let hex = &bytes[(a_off + a_len) as usize + 1..b_off as usize - 1];
    let raw: Vec<u8> = hex
        .chunks(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect();
    let len = match raw[1] {
        n if n < 0x80 => 2 + n as usize,
        n => {
            let k = (n & 0x7F) as usize;
            2 + k
                + raw[2..2 + k]
                    .iter()
                    .fold(0usize, |a, &b| (a << 8) | b as usize)
        }
    };
    let mut covered = bytes[a_off as usize..(a_off + a_len) as usize].to_vec();
    covered.extend_from_slice(&bytes[b_off as usize..(b_off + b_len) as usize]);
    (raw[..len].to_vec(), covered)
}

#[test]
fn a_document_timestamp_after_the_dss_is_b_lta_and_openssl_verifies_the_token() {
    let signed = signed_hello();
    let mut s = EditSession::new(Document::from_bytes(signed.clone()).unwrap());
    s.add_validation_material(&ValidationMaterial::default())
        .expect("dss");
    let with_dss = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;

    let tsa = OpensslTsa::new(|r| r);
    let (bytes, report) = doc_stamp(with_dss.clone(), &tsa).expect("stamp");
    assert_eq!(&bytes[..with_dss.len()], with_dss.as_slice(), "incremental");
    assert_eq!(report.pades_level, Some("B-LTA"));
    assert_eq!((report.prior_signatures, report.dss_present), (1, true));
    assert_eq!(report.field_name, "Signature2");
    assert_eq!(report.timestamp.digest_algorithm, "SHA-256");
    assert_eq!(
        report.byte_range[3] + report.byte_range[2],
        bytes.len() as u64
    );

    // The dictionary Table 255 describes, and nothing it forbids.
    let text = String::from_utf8_lossy(&bytes[with_dss.len()..]);
    assert!(text.contains("/Type /DocTimeStamp"), "{text}");
    assert!(text.contains("/SubFilter /ETSI.RFC3161"), "{text}");
    let sig_obj = &text[text.find("/DocTimeStamp").unwrap()..];
    let sig_obj = &sig_obj[..sig_obj.find("endobj").unwrap()];
    for forbidden in ["/M ", "/Reference", "/Changes", "/Cert", "/Name"] {
        assert!(!sig_obj.contains(forbidden), "{forbidden} in {sig_obj}");
    }

    // pdfcer's verifier: both verify; the stamp's time is the TSA's.
    let doc = Document::from_bytes(bytes.clone()).unwrap();
    let verdicts = verify_all(&doc.view(), &bytes);
    assert_eq!(verdicts.len(), 2);
    for v in &verdicts {
        assert!(
            matches!(v.integrity, Integrity::Verified { .. }),
            "{:?}",
            v.integrity
        );
    }
    let dts = &verdicts[1];
    assert!(dts.coverage.covers_to_eof());
    assert_eq!(
        dts.signing_time.as_deref(),
        Some(report.timestamp.gen_time.as_str())
    );

    // Oracle: OpenSSL checks the token stamps exactly the covered bytes.
    let (token, covered) = hole_and_covered(&bytes, "Signature2");
    let dir = scratch("dts");
    std::fs::write(dir.join("token.der"), &token).unwrap();
    std::fs::write(dir.join("covered.bin"), &covered).unwrap();
    let tsa_pem = tsa.dir.join("c.pem");
    openssl(
        &dir,
        &[
            "ts",
            "-verify",
            "-token_in",
            "-in",
            "token.der",
            "-data",
            "covered.bin",
            "-CAfile",
            tsa_pem.to_str().unwrap(),
        ],
    );
}

#[test]
fn a_stamp_without_a_dss_or_a_signature_claims_no_pades_level() {
    let tsa = OpensslTsa::new(|r| r);
    let (_, report) = doc_stamp(signed_hello(), &tsa).expect("stamp");
    assert_eq!(report.pades_level, None);
    assert!(report.notes[0].contains("no /DSS"), "{:?}", report.notes);
    // Notes are shown verbatim by every front end: none names a shell's verb.
    assert!(
        report.notes[0].contains("add validation material (PAdES B-LT)")
            && !report.notes[0].contains("add-"),
        "{:?}",
        report.notes
    );

    let hello = std::fs::read(fixtures().join("hello.pdf")).unwrap();
    let (bytes, report) = doc_stamp(hello, &tsa).expect("stamp");
    assert_eq!((report.pades_level, report.prior_signatures), (None, 0));
    assert_eq!(report.field_name, "Signature1");
    let doc = Document::from_bytes(bytes.clone()).unwrap();
    let v = verify_all(&doc.view(), &bytes).remove(0);
    assert!(matches!(v.integrity, Integrity::Verified { .. }));
}

#[test]
fn an_altered_byte_under_a_document_timestamp_is_a_digest_mismatch() {
    let tsa = OpensslTsa::new(|r| r);
    let hello = std::fs::read(fixtures().join("hello.pdf")).unwrap();
    let (mut bytes, _) = doc_stamp(hello, &tsa).expect("stamp");
    // A byte inside the original page content, covered by the stamp.
    let at = bytes
        .windows(5)
        .position(|w| w == b"Hello")
        .expect("page text");
    bytes[at] = b'J';
    let doc = Document::from_bytes(bytes.clone()).unwrap();
    let v = verify_all(&doc.view(), &bytes).remove(0);
    assert!(
        matches!(v.integrity, Integrity::DigestMismatch),
        "{:?}",
        v.integrity
    );
}

#[test]
fn a_document_timestamp_refuses_a_rejection_and_a_replayed_token() {
    let rejection = vec![0x30, 0x05, 0x30, 0x03, 0x02, 0x01, 0x02];
    let hello = std::fs::read(fixtures().join("hello.pdf")).unwrap();
    let err = doc_stamp(hello.clone(), &Canned(Ok(rejection))).unwrap_err();
    assert!(
        matches!(
            err,
            SignApplyError::Timestamp(TimestampError::Rejected { status: 2, .. })
        ),
        "{err:?}"
    );
    // A token for another document stamps another imprint.
    let tsa = OpensslTsa::new(|r| r);
    doc_stamp(signed_hello(), &tsa).expect("stamp");
    let replay = Canned(Ok(tsa.given.lock().unwrap()[0].clone()));
    let err = doc_stamp(hello, &replay).unwrap_err();
    assert!(
        matches!(
            err,
            SignApplyError::Timestamp(TimestampError::ImprintMismatch)
        ),
        "{err:?}"
    );
}
