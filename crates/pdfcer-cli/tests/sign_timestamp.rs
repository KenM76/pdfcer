//! `pdfcer sign --tsa-url` (`Pass 10.11`) as the SHIPPED BINARY reports it.
//!
//! Without the `download` feature the flag is refused by name and nothing is
//! written. With it, a local HTTP server answers through `openssl ts -reply`
//! over the synthetic TSA key, and the binary must print `level=B-T` and the
//! authority's assertion.

#![cfg(feature = "signing")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic")
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("pdfcer_tsa_{tag}_{}_{n}", std::process::id()))
}

fn sign(output: &Path, url: &str) -> Output {
    let pfx = fixtures().join("signing/rsa2048-modern.pfx");
    Command::new(BIN)
        .arg("sign")
        .arg(fixtures().join("hello.pdf"))
        .args(["--cert", pfx.to_str().unwrap(), "--password", "pdfcer"])
        .args(["--signing-time", "D:20260927000000Z", "--tsa-url", url])
        .arg("--output")
        .arg(output)
        .output()
        .expect("the binary runs")
}

fn timestamp(input: &Path, output: &Path, url: &str) -> Output {
    Command::new(BIN)
        .arg("timestamp")
        .arg(input)
        .args(["--tsa-url", url])
        .arg("--output")
        .arg(output)
        .output()
        .expect("the binary runs")
}

#[cfg(not(feature = "download"))]
#[test]
fn without_network_support_timestamp_is_refused_and_nothing_is_written() {
    let out_path = temp_path("dts_refused").with_extension("pdf");
    let out = timestamp(
        &fixtures().join("hello.pdf"),
        &out_path,
        "http://127.0.0.1:9/tsa",
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(9), "{stderr}");
    assert!(stderr.contains("the `download` feature is off"), "{stderr}");
    assert!(!out_path.exists());
}

#[cfg(not(feature = "download"))]
#[test]
fn without_network_support_a_tsa_url_is_refused_and_nothing_is_written() {
    let out_path = temp_path("refused").with_extension("pdf");
    let out = sign(&out_path, "http://127.0.0.1:9/tsa");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(9), "{stderr}");
    assert!(stderr.contains("the `download` feature is off"), "{stderr}");
    assert!(!out_path.exists());
}

#[cfg(feature = "download")]
mod live {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    const TSA_CONFIG: &str = "[ tsa ]\ndefault_tsa = cfg\n[ cfg ]\nserial = ./serial\n\
crypto_device = builtin\nsigner_digest = sha256\ndefault_policy = 1.2.3.4.1\n\
digests = sha256, sha384, sha512\naccuracy = secs:1\nordering = no\ntsa_name = no\n\
ess_cert_id_chain = no\ness_cert_id_alg = sha256\n";

    fn openssl(dir: &Path, args: &[&str]) {
        let out = Command::new("openssl")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("openssl on PATH");
        assert!(
            out.status.success(),
            "openssl {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Serves ONE RFC 3161 request over HTTP/1.1 and returns its URL.
    fn serve_once(status_line: &'static str) -> String {
        let dir = temp_path("tsa");
        std::fs::create_dir_all(&dir).unwrap();
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

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/tsa", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0usize;
            let mut content_type = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let line = line.trim_end();
                if line.is_empty() {
                    break;
                }
                let lower = line.to_ascii_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
                if let Some(v) = lower.strip_prefix("content-type:") {
                    content_type = v.trim().to_owned();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            assert_eq!(content_type, "application/timestamp-query");
            std::fs::write(dir.join("q.tsq"), &body).unwrap();
            openssl(
                &dir,
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
            let reply = std::fs::read(dir.join("r.tsr")).unwrap();
            let mut stream = stream;
            write!(
                stream,
                "{status_line}\r\nContent-Type: application/timestamp-reply\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                reply.len()
            )
            .unwrap();
            stream.write_all(&reply).unwrap();
        });
        url
    }

    #[test]
    fn a_tsa_url_makes_the_signature_b_t_and_prints_the_assertion() {
        let url = serve_once("HTTP/1.1 200 OK");
        let out_path = temp_path("bt").with_extension("pdf");
        let out = sign(&out_path, &url);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{stderr}");
        assert!(stdout.contains("level=B-T"), "{stdout}");
        assert!(stdout.contains("  timestamp: gen_time="), "{stdout}");
        assert!(
            stdout.contains("policy=1.2.3.4.1 digest=SHA-256"),
            "{stdout}"
        );
        assert!(stdout.contains("pdfcer synthetic TSA"), "{stdout}");
        let v = Command::new(BIN)
            .arg("verify-signatures")
            .arg(&out_path)
            .output()
            .unwrap();
        assert!(v.status.success(), "{}", String::from_utf8_lossy(&v.stderr));
    }

    #[test]
    fn a_tsa_http_error_is_refused_by_name_and_nothing_is_written() {
        let url = serve_once("HTTP/1.1 503 Service Unavailable");
        let out_path = temp_path("503").with_extension("pdf");
        let out = sign(&out_path, &url);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(12), "{stderr}");
        assert!(stderr.contains("HTTP 503"), "{stderr}");
        assert!(!out_path.exists());
    }

    #[test]
    fn timestamp_after_add_ltv_is_b_lta_and_verifies() {
        let pfx = fixtures().join("signing/rsa2048-modern.pfx");
        let signed = temp_path("dts_signed").with_extension("pdf");
        let out = Command::new(BIN)
            .arg("sign")
            .arg(fixtures().join("hello.pdf"))
            .args(["--cert", pfx.to_str().unwrap(), "--password", "pdfcer"])
            .args(["--signing-time", "D:20260927000000Z", "--output"])
            .arg(&signed)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let ltv = temp_path("dts_ltv").with_extension("pdf");
        let out = Command::new(BIN)
            .arg("add-ltv")
            .arg(&signed)
            .arg("--output")
            .arg(&ltv)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );

        let url = serve_once("HTTP/1.1 200 OK");
        let stamped = temp_path("dts_blta").with_extension("pdf");
        let out = timestamp(&ltv, &stamped, &url);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(stdout.contains("field=\"Signature2\""), "{stdout}");
        assert!(
            stdout.contains("level=B-LTA prior_signatures=1 dss=1 self_verified=1"),
            "{stdout}"
        );
        assert!(stdout.contains("pdfcer synthetic TSA"), "{stdout}");
        assert!(stdout.contains("  note: "), "{stdout}");
        let v = Command::new(BIN)
            .arg("verify-signatures")
            .arg(&stamped)
            .output()
            .unwrap();
        let vout = String::from_utf8_lossy(&v.stdout);
        assert!(v.status.success(), "{vout}");
        assert!(vout.contains("ETSI.RFC3161"), "{vout}");
    }

    #[test]
    fn timestamp_on_an_unsigned_document_claims_no_level() {
        let url = serve_once("HTTP/1.1 200 OK");
        let stamped = temp_path("dts_plain").with_extension("pdf");
        let out = timestamp(&fixtures().join("hello.pdf"), &stamped, &url);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            stdout.contains("level=none prior_signatures=0 dss=0"),
            "{stdout}"
        );
    }
}
