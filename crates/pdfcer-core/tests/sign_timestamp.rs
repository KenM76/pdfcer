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
