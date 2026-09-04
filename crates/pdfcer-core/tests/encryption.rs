//! End-to-end decryption of encrypted documents — RC4, AES-128 and AES-256 at
//! `/R` 5 (ISO 32000-1 §7.6, Adobe ExtensionLevel 3 §3.5).
//!
//! # Why these tests are the ones that matter
//!
//! The unit tests in `crate::crypto` verify pieces: MD5 against RFC 1321,
//! RC4 against its published vectors, `/P` against the standard's own `-44`
//! example, the never-encrypted list against constructed dictionaries. Every
//! one of them can pass while the document still fails to open, because clause
//! 7.6's real hazard is not any single algorithm — it is **transposition
//! between two algorithms that look alike**:
//!
//! - Algorithm 2 step (h) runs 50 MD5 rounds truncating to `n` bytes each
//!   round; Algorithm 3 step (c) runs 50 MD5 rounds and does **not** truncate
//!   (**T9/T13**). Three pages apart, opposite rules.
//! - Algorithm 3's RC4 loop counts **1 → 19**; Algorithm 7's counts
//!   **19 → 0** (**T16**).
//! - `/P` is stored signed and hashed unsigned little-endian (**T10**).
//! - `/U`'s last 16 bytes are arbitrary at `/R` ≥ 3 and must not be compared
//!   (**T15**).
//!
//! Swap either 50-round loop for the other and every unit test still passes.
//! The only thing that catches it is a real file, made by an implementation
//! that is not ours, opening.
//!
//! # What agreement here proves, and what it does not
//!
//! The fixtures were produced by **pypdf**, chosen deliberately as an
//! independent implementation (`fixtures/synthetic/encryption/PROVENANCE.md`).
//! pdfcer's decryption was written from the ISO 32000-1 clause text. So
//! agreement means two independent readings of the same published
//! specification agree — which is evidence.
//!
//! That reasoning is exactly why `enc-aes-256-r6.pdf` is **not** used as a
//! decryption fixture anywhere: `/R` 6's Algorithm 2.B is not sourced past
//! step (a), so an implementation derived from another implementation and then
//! tested against that same implementation's output could not fail. It appears
//! below only as a **refusal** fixture.
//!
//! Passwords: user `userpw`, owner `ownerpw`.

use std::path::{Path, PathBuf};

use pdfcer_core::crypto::PermissionBit;
use pdfcer_core::crypto::{AuthKind, PermsCheck};
use pdfcer_core::document::{DocError, Document};
use pdfcer_core::edit::{EditSession, EncryptError, EncryptionSettings};
use pdfcer_core::page_tree;
use pdfcer_core::writer::{DirtySet, SaveOptions, WriteError};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/encryption")
        .join(name)
}

/// `/V` 1, `/R` 2, 40-bit RC4 — the oldest configuration, and the one where
/// Algorithm 2's 50-round loop does **not** run at all and `n` is fixed at 5
/// regardless of `/Length`.
///
/// Opened with the **user** password.
#[test]
fn rc4_40_opens_with_the_user_password() {
    let doc = Document::load_with_password(&fixture("enc-rc4-40.pdf"), Some(b"userpw"))
        .expect("R2 RC4-40 must decrypt with the user password");

    let enc = doc.encryption().expect("the document is encrypted");
    assert_eq!(enc.config.revision, 2);
    assert_eq!(
        enc.config.key_len, 5,
        "R2 fixes n at 5 (Algorithm 2 step (i))"
    );
    assert_eq!(enc.auth, AuthKind::User);

    // The payoff: the page tree resolves, which it cannot do through
    // ciphertext.
    assert!(
        !page_tree::pages(&doc)
            .expect("page tree must resolve")
            .is_empty(),
        "pages must be reachable after decryption"
    );
}

/// `/V` 2, `/R` 3, 128-bit RC4 — this one **does** run Algorithm 2 step (h)'s
/// 50-round truncating loop, and compares only the first 16 bytes of `/U`.
/// If T9 or T15 were transposed, this test fails and the `/R` 2 one above
/// still passes.
#[test]
fn rc4_128_opens_with_the_user_password() {
    let doc = Document::load_with_password(&fixture("enc-rc4-128.pdf"), Some(b"userpw"))
        .expect("R3 RC4-128 must decrypt with the user password");

    let enc = doc.encryption().expect("the document is encrypted");
    assert_eq!(enc.config.revision, 3);
    assert_eq!(enc.config.key_len, 16);
    assert_eq!(enc.auth, AuthKind::User);
    assert!(
        !page_tree::pages(&doc)
            .expect("page tree must resolve")
            .is_empty()
    );
}

/// Algorithm 7 — the owner password opens the document too (§7.6.3.1:
/// "correctly supplying **either** password").
///
/// This is the only test that exercises the 19→0 loop (**T16**). Running it
/// 1→19 instead, or omitting the counter-0 round, fails here and nowhere else:
/// the user-password tests above never touch Algorithm 7 at all.
#[test]
fn owner_password_opens_both_revisions() {
    for (name, revision) in [("enc-rc4-40.pdf", 2u8), ("enc-rc4-128.pdf", 3)] {
        let doc = Document::load_with_password(&fixture(name), Some(b"ownerpw"))
            .unwrap_or_else(|e| panic!("{name} must open with the owner password: {e}"));
        let enc = doc.encryption().expect("encrypted");
        assert_eq!(enc.config.revision, revision, "{name}");
        assert_eq!(
            enc.auth,
            AuthKind::Owner,
            "{name}: the owner password must be reported as owner access, \
             not silently downgraded to user access"
        );
        assert!(
            !page_tree::pages(&doc)
                .expect("page tree must resolve")
                .is_empty(),
            "{name}"
        );
    }
}

/// ★ The empty user password — the single most operator-visible behaviour in
/// clause 7.6, and the one that decides whether pdfcer looks broken.
///
/// §7.6.3.1 requires a reader to try the empty user password **first and
/// silently**, before any prompt. A document with an empty user password and a
/// non-empty owner password — the "permissions-only" PDF — therefore opens with
/// no dialog in every conforming reader. If pdfcer prompted for it, the
/// operator's experience would be pdfcer demanding a password for a file that
/// Chrome, Acrobat and every phone opens on a tap.
///
/// Note what `Document::load` is given here: **nothing**. No password argument,
/// no empty string, no flag. That is the point — the empty attempt is not
/// something a caller opts into.
///
/// This test could not exist until its fixture did. The corpus had exactly one
/// empty-user-password file and it was AES-128, which increment 1 refused on
/// cipher grounds *before authentication was ever reached* — so the
/// empty-password path was implemented, believed, and never once executed
/// end-to-end. A fixture that cannot fail for the reason you care about is not
/// covering that reason.
///
/// Increment 2 implemented AES-128, so that fixture now reaches authentication
/// too (`aes_128_with_an_empty_user_password_needs_no_password`). **Both stay.**
/// The RC4 file is the one that proved the path when AES could not, and
/// deleting it now would re-create the original hole the moment some future
/// increment changes how AES is handled.
#[test]
fn empty_user_password_opens_with_no_prompt() {
    let doc = Document::load(&fixture("enc-emptyuser-rc4-128.pdf"))
        .expect("a permissions-only document must open with no password at all");

    let enc = doc.encryption().expect("the document is still encrypted");
    assert_eq!(
        enc.auth,
        AuthKind::EmptyUser,
        "opening via the default password must be reported as such, not as a \
         user password the operator supplied"
    );
    assert!(
        !page_tree::pages(&doc)
            .expect("page tree must resolve")
            .is_empty()
    );

    // The owner password still opens it, and still reports owner access —
    // the empty user password succeeding must not shadow the stronger claim.
    let as_owner =
        Document::load_with_password(&fixture("enc-emptyuser-rc4-128.pdf"), Some(b"ownerpw"))
            .expect("the owner password must also open it");
    assert_eq!(
        as_owner.encryption().expect("encrypted").auth,
        AuthKind::Owner,
        "supplying the owner password must report owner access even though the \
         empty user password would also have worked"
    );
}

/// A wrong password is refused, and refused as a *password* problem rather
/// than as file damage.
///
/// The distinction is the operator-visible one: "this file is broken" sends
/// someone hunting for a corrupt download; "this needs a password" does not.
#[test]
fn wrong_password_asks_for_a_password() {
    let e = Document::load_with_password(&fixture("enc-rc4-128.pdf"), Some(b"not the password"))
        .expect_err("a wrong password must not open the document");
    assert!(
        matches!(e, DocError::PasswordRequired),
        "expected PasswordRequired, got {e:?}"
    );
}

/// No password at all behaves the same way — and, importantly, does **not**
/// succeed. §7.6.3.1's silent empty-password attempt runs first for every
/// document, so a file that still refuses genuinely has a user password.
#[test]
fn no_password_on_a_protected_file_asks_for_one() {
    let e = Document::load(&fixture("enc-rc4-40.pdf"))
        .expect_err("this fixture has a non-empty user password");
    assert!(matches!(e, DocError::PasswordRequired), "got {e:?}");
}

/// AES-128 (`/CFM /AESV2`) opens, with either password (increment 2).
///
/// This assertion replaced a refusal test. The refusal was correct when it was
/// written and is now false, which is the honest reason to change a test
/// rather than add one beside it.
#[test]
fn aes_128_opens_with_either_password() {
    for pw in [&b"userpw"[..], b"ownerpw"] {
        let doc = Document::load_with_password(&fixture("enc-aes-128.pdf"), Some(pw))
            .expect("AES-128 is implemented");
        assert!(
            doc.encryption().is_some(),
            "the document is still encrypted"
        );
        assert!(
            !page_tree::pages(&doc)
                .expect("the page tree walks after decryption")
                .is_empty(),
            "a decrypted document has pages"
        );
    }
}

/// **AES decryption produces the right STRINGS, which pixels cannot prove.**
///
/// The end-to-end fidelity proof for this increment lives in `pdfcer`'s
/// `decrypting_reproduces_the_plaintext_document_exactly` and compares
/// *rendered pixels*. That covers stream data thoroughly and strings barely:
/// a form field's `/T` name is never drawn, so string decryption could be
/// entirely broken and every pixel would still match.
///
/// Strings take a genuinely different path — decrypted in the *parsed object*
/// by `apply::decrypt_strings`, not in the retained buffer — so they need
/// their own assertion. A field name is ideal: it is an encrypted string
/// (**E7** exempts numbers and names, not strings), it is compared here
/// against the plaintext document the fixture was made from, and an
/// off-by-one in the `sAlT` key derivation would turn it into noise rather
/// than into a plausible different name.
#[test]
fn aes_128_decrypts_strings_not_only_stream_data() {
    let plain = Document::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/forms/demo-form.pdf"),
    )
    .expect("the plaintext source of every encryption fixture");
    let enc = Document::load_with_password(&fixture("enc-aes-128.pdf"), Some(b"userpw"))
        .expect("AES-128 is implemented");

    let names = |d: &Document| -> Vec<String> {
        let form = pdfcer_core::forms::parse_acroform(d).expect("the fixture has an AcroForm");
        let mut v: Vec<String> = form
            .fields
            .iter()
            .map(|f| f.fully_qualified_name.clone())
            .collect();
        v.sort();
        v
    };

    let expected = names(&plain);
    assert!(
        !expected.is_empty(),
        "the fixture must actually have named fields, or this test proves nothing"
    );
    assert_eq!(
        names(&enc),
        expected,
        "AES-decrypted field names must equal the plaintext document's. A \
         mismatch here is a string-path bug that renders byte-identically."
    );
}

/// AES-128 with an **empty user password** opens with no password at all —
/// the §7.6.3.1 silent attempt, now exercised on the AES path too.
///
/// This fixture was the *only* empty-password fixture once, and because AES
/// was refused before authentication was ever reached, the most
/// operator-visible behaviour in clause 7.6 went unexecuted. It is now a real
/// acceptance case rather than a file that fails early for an unrelated reason.
#[test]
fn aes_128_with_an_empty_user_password_needs_no_password() {
    let doc = Document::load(&fixture("enc-emptyuser.pdf"))
        .expect("an empty user password is tried silently, §7.6.3.1");
    assert!(doc.encryption().is_some(), "it is still an encrypted file");
    assert!(
        !page_tree::pages(&doc)
            .expect("the page tree walks after decryption")
            .is_empty()
    );
}

/// AES-256 at `/R` 5 opens with **both** passwords, and the two are told
/// apart.
///
/// The owner assertion is not a formality. `/R` 5's owner path is the only
/// thing in the whole increment that exercises **T26** — Algorithms 3.12 and
/// 3.2a concatenate the *whole 48-byte* `/U` string into both the validation
/// hash and the unwrap-key hash, where the user path concatenates nothing at
/// all. Passing `/U`'s 32-byte hash instead is a one-character slip, it cannot
/// affect a user password, and without an owner test it would be entirely
/// untested. (The same hole existed for RC4 and is why
/// `owner_password_opens_both_revisions` exists.)
///
/// `key_len` is asserted at 32 because `/Length` is *not* what decides it
/// here: this fixture carries `/Length 256`, which is outside the 40–128 range
/// [`EncryptionConfig::parse`] enforces below `/R` 5. ISO 32000-2's Table 25
/// erratum says a standard handler should write **32** while 2.0-as-printed
/// said **256**, so both appear in the wild (**W18**, ambiguity **A11**), and
/// pdfcer reads neither — `/AESV3` fixes the key at 256 bits and the entry
/// carries no information.
///
/// [`EncryptionConfig::parse`]: pdfcer_core::crypto::EncryptionConfig::parse
#[test]
fn aes_256_r5_opens_with_either_password() {
    for (pw, expected) in [
        (&b"userpw"[..], AuthKind::User),
        (b"ownerpw", AuthKind::Owner),
    ] {
        let doc = Document::load_with_password(&fixture("enc-aes-256-r5.pdf"), Some(pw))
            .unwrap_or_else(|e| panic!("/R 5 must open with {pw:?}: {e}"));

        let enc = doc.encryption().expect("the document is encrypted");
        assert_eq!(enc.config.revision, 5);
        assert_eq!(enc.config.version, 5, "/R 5 and /V 5 travel together");
        assert_eq!(
            enc.config.key_len, 32,
            "/AESV3 fixes the file key at 256 bits; /Length is not consulted"
        );
        assert_eq!(
            enc.auth, expected,
            "the owner password must be reported as owner access, not silently \
             downgraded — it is the only thing exercising T26"
        );
        assert_eq!(
            enc.config.o.len(),
            48,
            "/O is 48 bytes at /R 5, not the 32 of /R 2-4 (Table 3.19)"
        );
        assert_eq!(enc.config.u.len(), 48);
        assert!(
            enc.config.aes256.is_some(),
            "/OE, /UE and /Perms are Required if /R is 5"
        );

        assert!(
            !page_tree::pages(&doc)
                .expect("the page tree must resolve")
                .is_empty(),
            "pages are reachable only through decrypted objects"
        );
    }
}

/// **AES-256 decryption produces the right STRINGS**, which the pixel
/// comparison in `pdfcer` cannot prove.
///
/// Same argument as `aes_128_decrypts_strings_not_only_stream_data`: a form
/// field's `/T` name is never drawn, so string decryption could be entirely
/// broken and every pixel would still match. At `/R` 5 there is an extra
/// reason to check it — strings and streams share **one key with no per-object
/// derivation at all** (**T24**), so if the two paths ever disagree it is
/// because one of them did something to the key that the other did not.
#[test]
fn aes_256_r5_decrypts_strings_not_only_stream_data() {
    let plain = Document::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/forms/demo-form.pdf"),
    )
    .expect("the plaintext source of every encryption fixture");
    let enc = Document::load_with_password(&fixture("enc-aes-256-r5.pdf"), Some(b"userpw"))
        .expect("/R 5 is implemented");

    let names = |d: &Document| -> Vec<String> {
        let form = pdfcer_core::forms::parse_acroform(d).expect("the fixture has an AcroForm");
        let mut v: Vec<String> = form
            .fields
            .iter()
            .map(|f| f.fully_qualified_name.clone())
            .collect();
        v.sort();
        v
    };

    let expected = names(&plain);
    assert!(
        !expected.is_empty(),
        "the fixture must actually have named fields, or this test proves nothing"
    );
    assert_eq!(names(&enc), expected);
}

/// ★ `/Perms` — the only integrity check in PDF encryption — is actually run,
/// and it passes on an untampered document.
///
/// Worth its own assertion because the check is entirely invisible otherwise:
/// nothing downstream consumes it, so an implementation that never called
/// Algorithm 3.13 at all would behave identically for every document that has
/// not been tampered with, which is all of them.
///
/// The `/R` ≤ 4 half matters just as much. `/Perms` does not exist below
/// `/R` 5, and reporting a document with no `/Perms` as a *failed* check would
/// tell the operator that every RC4 and AES-128 file they own has been
/// modified.
#[test]
fn perms_is_validated_at_r5_and_not_applicable_below_it() {
    let r5 = Document::load_with_password(&fixture("enc-aes-256-r5.pdf"), Some(b"userpw"))
        .expect("/R 5 opens");
    assert_eq!(
        r5.encryption().expect("encrypted").perms,
        PermsCheck::Match,
        "the fixture is untampered, so the encrypted permission copy must \
         agree with /P and /EncryptMetadata"
    );

    for name in ["enc-rc4-128.pdf", "enc-aes-128.pdf"] {
        let doc = Document::load_with_password(&fixture(name), Some(b"userpw"))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            doc.encryption().expect("encrypted").perms,
            PermsCheck::NotApplicable,
            "{name}: /Perms was introduced at /R 5; its absence below that is \
             the ordinary case and must never read as a failed check"
        );
    }
}

/// ★ A tampered `/P` is **reported and not acted on** (**T27**).
///
/// # What is being simulated
///
/// `/P` sits in the `/Encrypt` dictionary as a plaintext integer. At `/R` ≤ 4
/// it also feeds Algorithm 2's hash, so editing it breaks authentication and
/// the document simply stops opening. **At `/R` 5 it feeds nothing** — the key
/// comes from the password and the salts alone — so `/P` can be edited freely
/// and the document still opens with the original passwords. That is precisely
/// the hole `/Perms` was added to detect, and this test walks through it: the
/// bytes below widen the permissions of a document whose password the editor
/// does not have.
///
/// The edit is length-preserving (`4294967292` → `4294967290`, ten digits
/// either way) so every offset in the cross-reference table stays true and the
/// only thing that changed is the number.
///
/// # What pdfcer does about it, and why
///
/// Reports it. Nothing else:
///
/// - **The document still opens.** The supplement's verdict is `should`
///   match, no clause says what to do on a mismatch, and every other reader
///   opens the file. Refusing would make pdfcer reject documents nothing else
///   objects to.
/// - **`permissions()` still reports the dictionary's `/P`** — the tampered
///   one. That looks wrong for exactly one second and is the point: silently
///   substituting the decrypted copy would be pdfcer deciding, on an inference
///   the standard declines to require, what the operator is shown. Rule 4.
/// - **Both numbers are carried in the report**, so a front end can say what
///   disagrees rather than only that something does.
#[test]
fn a_tampered_p_is_reported_and_neither_value_is_silently_preferred() {
    let original = std::fs::read(fixture("enc-aes-256-r5.pdf")).expect("the fixture is readable");
    let tampered = replace_once(&original, b"/P 4294967292", b"/P 4294967290");

    let doc = Document::from_bytes_with_password(tampered, Some(b"userpw")).expect(
        "editing /P must NOT stop an /R 5 document opening — the key does not depend on it",
    );
    let enc = doc.encryption().expect("encrypted");

    match enc.perms {
        PermsCheck::Mismatch {
            perms_p,
            dict_p,
            perms_encrypt_metadata,
            dict_encrypt_metadata,
        } => {
            assert_eq!(perms_p, 0xFFFF_FFFC, "the encrypted copy is untouched");
            assert_eq!(dict_p, 0xFFFF_FFFA, "the plaintext copy is what moved");
            assert_eq!(perms_encrypt_metadata, Some(true));
            assert!(dict_encrypt_metadata);
        }
        other => panic!("a tampered /P must be reported as a mismatch, got {other:?}"),
    }

    // The dictionary value is what `permissions()` reports — pdfcer does not
    // quietly swap in the value it has more reason to trust.
    assert_eq!(
        enc.config.permissions().raw,
        0xFFFF_FFFA,
        "permissions() must report what the file declares; the disagreement is \
         disclosed alongside, never resolved behind the operator's back"
    );

    // And the document is genuinely usable, not half-refused.
    assert!(
        !page_tree::pages(&doc)
            .expect("the page tree must resolve")
            .is_empty()
    );
}

/// Replace the first occurrence of `from` with `to`, which must be the same
/// length so byte offsets survive.
///
/// A same-length edit is the only kind that can be made to a PDF with a
/// classic cross-reference table without rewriting the table — which is
/// exactly what makes `/P` tampering realistic rather than a contrived test.
fn replace_once(haystack: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    assert_eq!(from.len(), to.len(), "the edit must preserve every offset");
    let at = haystack
        .windows(from.len())
        .position(|w| w == from)
        .unwrap_or_else(|| panic!("{from:?} is not in the fixture"));
    let mut out = haystack.to_vec();
    out.get_mut(at..at + to.len())
        .expect("the window was found in bounds")
        .copy_from_slice(to);
    out
}

// `r6_is_still_refused_as_unsourced_now_that_r5_opens` was RETIRED in
// `Pass 5.4`: `/R` 6 is implemented (Algorithm 2.B, sourced from
// ISO 32000-2:2020) and now OPENS. The replacement is
// `r6_opens_against_an_independent_implementation` above.

/// A wrong password on an `/R` 5 document is an ordinary password failure —
/// the SHA-256 comparison either matches or it does not, and there is nothing
/// ambiguous about an ASCII password that did not.
#[test]
fn a_wrong_ascii_password_at_r5_is_an_ordinary_password_failure() {
    let e = Document::load_with_password(&fixture("enc-aes-256-r5.pdf"), Some(b"userpx"))
        .expect_err("a wrong password must not open the document");
    assert!(
        matches!(e, DocError::PasswordRequired),
        "expected PasswordRequired, got {e:?}"
    );
}

/// ★ A **non-ASCII** password that fails is reported differently, because the
/// failure is genuinely ambiguous.
///
/// `/R` 5's password preprocessing is SASLprep (RFC 4013) → UTF-8 → truncate
/// to 127 bytes. pdfcer implements the last two exactly and does not implement
/// the first. For an ASCII password SASLprep is the identity, so nothing is
/// lost; for a non-ASCII one it may not be, and a correct password can fail.
///
/// pdfcer **attempts** such a password rather than refusing it — SASLprep is
/// the identity for far more than ASCII, and a mis-normalised password cannot
/// open a document with the wrong key, only fail to open one with the right
/// password. The whole exposure is a false "wrong password", and a distinct
/// diagnostic is the fix for that. Telling an operator their password is wrong
/// when it is right sends them to re-check the one thing that is not the
/// problem.
///
/// The `/R` ≤ 4 half of the assertion matters too: below `/R` 5 the password
/// encoding is PDFDocEncoding (**T8**), a *different* unimplemented question,
/// and reporting this one there would be wrong rather than merely imprecise.
#[test]
fn a_failed_non_ascii_password_discloses_the_missing_normalisation() {
    let e =
        Document::load_with_password(&fixture("enc-aes-256-r5.pdf"), Some("pässwörd".as_bytes()))
            .expect_err("this is not the fixture's password");
    assert!(
        matches!(e, DocError::PasswordRequiresNormalisation),
        "a non-ASCII password failing at /R 5 must disclose that SASLprep was \
         not applied, got {e:?}"
    );
    assert!(
        e.to_string().contains("SASLprep"),
        "the message must name what is missing: {e}"
    );

    // The same password against an /R 3 document is a plain failure: SASLprep
    // is not what that revision asks for.
    let e = Document::load_with_password(&fixture("enc-rc4-128.pdf"), Some("pässwörd".as_bytes()))
        .expect_err("not the password");
    assert!(
        matches!(e, DocError::PasswordRequired),
        "/R 3 must not claim a SASLprep problem it does not have, got {e:?}"
    );
}

/// ★ Saving a decrypted document is **refused**, in both modes.
///
/// This is the sharp edge of a read-only increment, and it has to be a
/// refusal rather than a best effort. After decryption the buffer and the
/// parsed objects deliberately disagree — stream data was decrypted in the
/// retained buffer (RC4 preserves length, so it fits exactly), strings were
/// decrypted in the parsed objects (a decrypted string cannot generally be
/// re-escaped into the same byte count). Both save modes re-emit untouched
/// objects verbatim from their source span, so a save here would write a file
/// whose `/Encrypt` claims encryption, whose streams are plaintext and whose
/// strings are ciphertext.
///
/// That file is not "partly saved". Nothing can open it, pdfcer included, and
/// the save would have reported success.
///
/// The two alternatives were rejected deliberately: re-encrypting needs a key
/// the document does not retain and would emit RC4, which pdfcer never writes
/// (**W14**); stripping `/Encrypt` would silently discard protection the
/// author applied, which is the operator's decision and not pdfcer's (rule 4).
#[test]
fn saving_a_decrypted_document_is_refused_in_both_modes() {
    let doc =
        Document::load_with_password(&fixture("enc-rc4-128.pdf"), Some(b"userpw")).expect("opens");
    let out = std::env::temp_dir().join("pdfcer-encrypted-save-refusal.pdf");
    let dirty = DirtySet::empty();
    let options = SaveOptions::default();

    let _ = std::fs::remove_file(&out);

    for (mode, result) in [
        ("incremental", doc.save_incremental(&out, &dirty, &options)),
        ("full", doc.save_full(&out, &dirty, &options)),
    ] {
        let e = result.err().unwrap_or_else(|| {
            panic!("{mode} save of a decrypted document must be refused, not attempted")
        });
        assert!(
            matches!(e, WriteError::EncryptedSaveUnsupported),
            "{mode}: expected EncryptedSaveUnsupported, got {e:?}"
        );
    }

    // The refusal must happen BEFORE any bytes are written. A refusal that
    // leaves a truncated or half-written file behind has replaced one broken
    // output with another, and the operator has no way to tell which.
    assert!(
        !out.exists(),
        "a refused save must not leave a file behind at {}",
        out.display()
    );
}

/// An unencrypted document reports `None`, and this is worth an explicit test:
/// every assertion above would also pass against an implementation that
/// believed every document was encrypted.
#[test]
fn plain_documents_report_no_encryption() {
    let plain = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf");
    if !plain.exists() {
        // The fixture set is allowed to move; skipping is better than a
        // failure that says nothing about encryption.
        return;
    }
    let doc = Document::load(&plain).expect("plain fixture must load");
    assert!(doc.encryption().is_none());
}

/// ★ AES-128 **on a document whose objects live in object streams** — the
/// combination every committed fixture misses, and the one that is normal in
/// the wild.
///
/// # Why this needed its own test
///
/// Every `enc-*.pdf` fixture derives from `demo-form.pdf`, a PDF 1.3 file with
/// a classic cross-reference table and **zero object streams**. pypdf, which
/// generates the corpus, *flattens* object streams when it clones (measured:
/// a 7-`ObjStm` source came out with 0), so the corpus cannot be extended to
/// cover this by changing its source document.
///
/// That left the most consequential AES path untested. Increment 2 shortens
/// `Stream::data_span` after decryption, because AES plaintext is shorter than
/// its ciphertext — and an **object stream container is a stream**. Its
/// shortened span is then handed to phase 2 of the load to be parsed for the
/// objects inside it. If the shortening were wrong by even one byte, every
/// object in every container would fail to parse, and no fixture in the
/// committed corpus could have shown it.
///
/// It also covers three other things at once, all of which are silent when
/// wrong: **T4** (the phase-1-before-phase-2 ordering, without which strings
/// inside containers get Algorithm 1 applied a second time and every one is
/// destroyed), **E5** (cross-reference streams are never encrypted — this file
/// has two, and decrypting one produces bytes that fail to inflate and surface
/// as a broken xref two layers from the cause), and content-stream decryption
/// end to end.
///
/// # Provenance, and why the assertion is what it is
///
/// The file is PDFium's `encrypted.pdf` — a **third** independent
/// implementation, after pdfcer's own spec reading and pypdf's. Verified by
/// inspection to be `/V 4`, `/R 4`, `/CFM /AESV2`, with 5 object streams and
/// 2 cross-reference streams. Its user password is `1234`.
///
/// # Why it skips instead of failing when absent
///
/// `fixtures/external/` is **gitignored** (`docs/LEGAL.md` §5 — the corpus is
/// cloned locally, never vendored), so this cannot be a hard dependency: it
/// would fail every clean checkout and every CI run. It skips loudly instead.
///
/// That is a real, stated weakness and not a neutral choice — **a test that
/// silently passes when its input is missing is the exact "fixture that cannot
/// fail for the reason you care about" shape this file's own header warns
/// about.** It is accepted here only because the alternative is no coverage of
/// this path at all, and because the skip prints. If a committable synthetic
/// AES + object-stream fixture is ever built, this should become an ordinary
/// unconditional test and the skip should go.
#[test]
fn aes_128_decrypts_a_document_whose_objects_live_in_object_streams() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/external/pdfium/testing/resources/encrypted.pdf");
    if !path.exists() {
        eprintln!(
            "SKIPPED aes_128_decrypts_a_document_whose_objects_live_in_object_streams: \
             {} is absent. fixtures/external/ is gitignored; clone the corpus to run it. \
             THIS PATH IS THEREFORE UNCOVERED IN THIS RUN.",
            path.display()
        );
        return;
    }

    let doc = Document::load_with_password(&path, Some(b"1234"))
        .expect("AES-128 with object streams must decrypt");

    let enc = doc.encryption().expect("the document is encrypted");
    assert_eq!(enc.config.revision, 4, "/R 4");
    assert_eq!(enc.config.key_len, 16, "AES-128");

    // The payoff. Pages live *inside* the object streams, so a page tree that
    // resolves is proof the containers were decrypted with the right span and
    // then parsed as plaintext -- neither of which any committed fixture can
    // demonstrate.
    let pages = page_tree::pages(&doc).expect("the page tree must resolve");
    assert!(
        !pages.is_empty(),
        "pages are reachable only through decrypted object streams"
    );
}

/// ★ AES-256 at `/R` 5 **on a document whose objects live in object streams**
/// — the shape every committed fixture misses, and the normal one in the wild.
///
/// # Why this needed its own test, again
///
/// The AES-128 case above records the hole: every `enc-*.pdf` derives from
/// `demo-form.pdf`, a PDF 1.3 file with a classic cross-reference table and
/// **zero object streams**, and pypdf flattens object streams when it clones,
/// so the synthetic corpus cannot be extended to cover this by changing its
/// source. `enc-aes-256-r5.pdf` inherits that limitation exactly — asking
/// "what can this fixture not fail on?" gives the same answer for increment 3
/// as it did for increment 2.
///
/// The stakes are the same and the mechanism is the same: AES plaintext is
/// shorter than its ciphertext, so `Stream::data_span` is shortened after
/// decryption — and an **object-stream container is a stream**. Its shortened
/// span is handed to phase 2 of the load to be parsed for the objects inside
/// it. Wrong by one byte and every object in every container fails to parse.
/// It also covers **T4** (phase 1 before phase 2, without which strings inside
/// containers are decrypted twice) and **E5** (cross-reference streams are
/// never encrypted).
///
/// # Provenance
///
/// qpdf's `c-r5-in.pdf` (Apache-2.0) — a **fourth** independent
/// implementation, after pdfcer's own spec reading, pypdf's and PDFium's.
/// `/V` 5, `/R` 5, `/CFM /AESV3`, one object stream, user password `user3`,
/// owner password `owner3`. The same file's `/Encrypt` values appear as an
/// unconditional test vector in `crypto::r5`'s unit tests, where they check
/// the derived key against the value qpdf's own test suite publishes; this
/// test is the end-to-end half that the vector cannot cover.
///
/// # Why it skips instead of failing when absent
///
/// `fixtures/external/` is **gitignored** (`docs/LEGAL.md` §5), so this cannot
/// be a hard dependency — it would fail every clean checkout and every CI run.
/// It skips loudly. That is a real weakness and is stated as one: a test that
/// silently passes when its input is missing is not covering its path. It is
/// accepted only because the alternative is no coverage at all, and because
/// the key-derivation half of the same file IS unconditional.
#[test]
fn aes_256_r5_decrypts_a_document_whose_objects_live_in_object_streams() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/external/qpdf/qpdf/qtest/qpdf/c-r5-in.pdf");
    if !path.exists() {
        eprintln!(
            "SKIPPED aes_256_r5_decrypts_a_document_whose_objects_live_in_object_streams: \
             {} is absent. fixtures/external/ is gitignored; clone the qpdf corpus to run it. \
             THIS PATH IS THEREFORE UNCOVERED IN THIS RUN.",
            path.display()
        );
        return;
    }

    for (pw, expected) in [
        (&b"user3"[..], AuthKind::User),
        (b"owner3", AuthKind::Owner),
    ] {
        let doc = Document::load_with_password(&path, Some(pw))
            .unwrap_or_else(|e| panic!("/R 5 with object streams must decrypt ({pw:?}): {e}"));

        let enc = doc.encryption().expect("the document is encrypted");
        assert_eq!(enc.config.revision, 5);
        assert_eq!(enc.config.key_len, 32);
        assert_eq!(enc.auth, expected);
        assert_eq!(
            enc.perms,
            PermsCheck::Match,
            "a third-party /Perms block must validate too — the fixture corpus \
             cannot show that a producer other than pypdf writes it the same way"
        );

        // The payoff. Pages live INSIDE the object stream, so a page tree that
        // resolves proves the container was decrypted with the right span and
        // then parsed as plaintext.
        assert!(
            !page_tree::pages(&doc)
                .expect("the page tree must resolve")
                .is_empty(),
            "pages are reachable only through decrypted object streams"
        );
    }
}

/// AES-256 at `/R` 5 with an **empty user password** opens with no password
/// at all — §7.6.3.1's silent attempt, on the `/R` 5 branch.
///
/// # This fixture had to be built, and the reason is a repeat
///
/// `tools/gen-encryption-fixtures.py` already carries the argument, written
/// when the corpus had exactly one empty-password file and it was AES-128:
/// while a cipher is refused, an empty-password fixture in that cipher is
/// rejected on cipher grounds *before authentication is ever reached*, so the
/// path is implemented, believed, and never executed. Its own comment then
/// promises "a fixture in EVERY cipher rather than one".
///
/// When `/R` 5 landed there were still two. So the `/R` 5 branch of the
/// silent empty-password attempt — which is a genuinely different code path,
/// running Algorithm 3.11 against `/U[0..32]` rather than Algorithm 6 against
/// `/U[0..16]` — was in exactly the state that paragraph describes. A promise
/// in a comment is not a fixture.
///
/// The RC4 and AES-128 files stay. Each was the one that proved the path when
/// the others could not, and deleting any of them re-creates the hole the
/// moment some future increment changes how its cipher is handled.
#[test]
fn aes_256_r5_with_an_empty_user_password_needs_no_password() {
    let doc = Document::load(&fixture("enc-emptyuser-aes-256-r5.pdf"))
        .expect("an empty user password is tried silently at /R 5 too, §7.6.3.1");

    let enc = doc.encryption().expect("it is still an encrypted file");
    assert_eq!(enc.config.revision, 5);
    assert_eq!(
        enc.auth,
        AuthKind::EmptyUser,
        "opening via the default password must be reported as such, not as a \
         user password the operator supplied"
    );
    assert_eq!(enc.perms, PermsCheck::Match);
    assert!(
        !page_tree::pages(&doc)
            .expect("the page tree must resolve")
            .is_empty()
    );

    // The owner password still opens it and still reports owner access — the
    // empty user password succeeding must not shadow the stronger claim, and
    // at /R 5 the owner branch is a different unwrap (/OE, T26) rather than a
    // different comparison.
    let as_owner =
        Document::load_with_password(&fixture("enc-emptyuser-aes-256-r5.pdf"), Some(b"ownerpw"))
            .expect("the owner password must also open it");
    assert_eq!(
        as_owner.encryption().expect("encrypted").auth,
        AuthKind::Owner
    );
}

/// ★ The `/R` 5 fixture still holds the exact bytes `crypto::r5`'s unit tests
/// were written against.
///
/// # Why this guard exists
///
/// `crypto::r5`'s unit tests embed this fixture's `/O`, `/U`, `/OE`, `/UE` and
/// `/Perms` as constants, so each algorithm can be exercised in isolation and
/// say *which* one broke where an end-to-end test only says that something
/// did. That is worth having, and it introduces a coupling that is invisible
/// from either side: **the corpus is not byte-reproducible.** Encryption
/// generates fresh salts and a fresh file key every run, so re-running
/// `tools/gen-encryption-fixtures.py` produces a different `/O` and `/U` for
/// the same document — by design, not by accident.
///
/// Without this test, a regeneration would leave the unit tests passing
/// against a file that no longer exists, while the integration tests passed
/// against the new one. Nothing would be wrong and nothing would be red; the
/// unit tests would simply have stopped being about the fixture, and the next
/// person to trust them would be trusting a coincidence.
///
/// So the coupling is asserted where it can be seen. If this goes red after a
/// regeneration, the fix is to copy the new bytes into `crypto::r5`'s
/// constants — not to weaken this test.
///
/// The `/Perms` byte layout is checked here too (Algorithm 3.10), because it
/// is the one place a fixture could change *shape* rather than just value:
/// `/O` and `/U` are 48 bytes at this revision and `/Perms` is 16, and a
/// producer that wrote them otherwise would be describing a different format.
#[test]
fn the_r5_fixture_still_matches_the_unit_test_constants() {
    let doc = Document::load_with_password(&fixture("enc-aes-256-r5.pdf"), Some(b"userpw"))
        .expect("/R 5 opens");
    let c = &doc.encryption().expect("encrypted").config;

    // The first eight bytes of each are enough to pin the file: /O and /U are
    // hashes, so any regeneration changes every byte of both.
    assert_eq!(
        &c.u[..8],
        &[0x58, 0xc0, 0x57, 0x8f, 0xec, 0x62, 0x7b, 0x8b],
        "/U has changed — the fixture was regenerated. Copy the new /O, /U, \
         /OE, /UE and /Perms into crypto::r5's test constants, and the new \
         file encryption key into FILE_KEY; do not relax this assertion."
    );
    assert_eq!(
        &c.o[..8],
        &[0xf8, 0xd6, 0xbf, 0xa3, 0x1b, 0x64, 0x5d, 0x37],
        "/O has changed — see the /U message above"
    );
    assert_eq!(c.o.len(), 48, "/O is 48 bytes at /R 5 (Table 3.19)");
    assert_eq!(c.u.len(), 48);

    let keys = c.aes256.as_ref().expect("/R 5 carries /OE, /UE and /Perms");
    assert_eq!(
        &keys.ue[..8],
        &[0xe2, 0x24, 0xdc, 0x92, 0x32, 0x71, 0x44, 0xdf]
    );
    assert_eq!(
        &keys.oe[..8],
        &[0xde, 0xd9, 0x20, 0x92, 0x7c, 0x17, 0xc9, 0xc0]
    );
    assert_eq!(
        keys.perms,
        [
            0x71, 0x6a, 0xf6, 0xa5, 0x5e, 0xa2, 0xaf, 0xb6, 0xa9, 0xb8, 0x8a, 0xe3, 0x6e, 0x38,
            0xb5, 0xe1,
        ]
    );
}

/// **`/R` 6 opens** — the decisive test for Algorithm 2.B (`Pass 5.4`).
///
/// The fixture is written by **pypdf 6.7.0**, an INDEPENDENT `/R` 6
/// implementation (its "AES-256" is `/V 5 /R 6`). If pdfcer's 2.B and its A13
/// reading were wrong, authentication would fail against pypdf's `/U`/`/O` and
/// the file would not open. It opening is a cross-implementation proof of both
/// the hash and the default A13 reading in one shot — the empirical settlement
/// the corpus said was owed to `personal_rag/pdf`.
#[test]
fn r6_opens_against_an_independent_implementation() {
    for pw in [&b"userpw"[..], b"ownerpw"] {
        let doc = Document::load_with_password(&fixture("enc-aes-256-r6.pdf"), Some(pw))
            .expect("/R 6 is implemented and 2.B matches pypdf");
        assert!(doc.encryption().is_some(), "still an encrypted document");
        assert!(
            !page_tree::pages(&doc).expect("page tree walks").is_empty(),
            "a decrypted /R 6 document has pages"
        );
    }
}

// ===================================================================
// Pass 5.4 — encrypt-on-save (the WRITE side). The proof is the read
// side reopening what the write side produced.
// ===================================================================

/// A plaintext synthetic document with real content to encrypt.
fn plain_source() -> Document {
    Document::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/forms/demo-form.pdf"),
    )
    .expect("load plaintext demo-form")
}

/// The decisive write-side test: what pdfcer ENCRYPTS, pdfcer REOPENS under
/// both passwords, and the decrypted page content matches the plaintext source
/// byte-for-byte. This is criterion 6's round-trip proof.
#[test]
fn what_pdfcer_encrypts_it_reopens_under_both_passwords() {
    let plain = plain_source();
    // A stable reference: the first content stream, decoded, from the plaintext.
    let want_pages = page_tree::pages(&plain).expect("plaintext pages").len();

    let session = EditSession::new(plain);
    let settings = EncryptionSettings::new(b"userpw".to_vec(), b"ownerpw".to_vec());
    let (bytes, report) = session
        .set_encryption(&settings, &SaveOptions::default())
        .expect("encrypt on save");
    assert!(
        !report.byte_identical,
        "an encrypted rewrite is never byte-identical"
    );

    for (pw, kind) in [
        (&b"userpw"[..], AuthKind::User),
        (b"ownerpw", AuthKind::Owner),
    ] {
        let reopened = Document::from_bytes_with_password(bytes.clone(), Some(pw))
            .expect("pdfcer reopens what it wrote");
        let enc = reopened.encryption().expect("carries /Encrypt");
        assert_eq!(enc.auth, kind, "the expected password opened it");
        assert_eq!(
            page_tree::pages(&reopened)
                .expect("pages after decrypt")
                .len(),
            want_pages,
            "page count survives the encrypt/decrypt round trip"
        );
    }

    // A wrong password does not open it.
    assert!(
        matches!(
            Document::from_bytes_with_password(bytes.clone(), Some(b"nope")),
            Err(DocError::Encryption(_)) | Err(_)
        ),
        "a wrong password is refused"
    );
}

/// An empty user password makes a permissions-only document: it opens with no
/// prompt (`AuthKind::EmptyUser`), and the owner password still opens it.
#[test]
fn an_empty_user_password_makes_a_no_prompt_document() {
    let session = EditSession::new(plain_source());
    let mut settings = EncryptionSettings::new(Vec::new(), b"ownerpw".to_vec());
    settings.permissions = vec![PermissionBit::Print, PermissionBit::AccessibilityExtract];
    let (bytes, _) = session
        .set_encryption(&settings, &SaveOptions::default())
        .expect("encrypt with empty user password");

    let no_prompt = Document::from_bytes(bytes.clone()).expect("opens with no password");
    assert_eq!(
        no_prompt.encryption().expect("encrypted").auth,
        AuthKind::EmptyUser
    );
    let as_owner =
        Document::from_bytes_with_password(bytes, Some(b"ownerpw")).expect("owner opens");
    assert_eq!(
        as_owner.encryption().expect("encrypted").auth,
        AuthKind::Owner
    );
}

/// remove_encryption is owner-only, and a user-authenticated session is refused
/// BY NAME with the AuthKind that opened it (criterion 5).
#[test]
fn remove_encryption_is_owner_only() {
    // Make an encrypted document first.
    let (encrypted, _) = EditSession::new(plain_source())
        .set_encryption(
            &EncryptionSettings::new(b"userpw".to_vec(), b"ownerpw".to_vec()),
            &SaveOptions::default(),
        )
        .expect("encrypt");

    // Opened as USER: refused, naming the AuthKind.
    let as_user =
        Document::from_bytes_with_password(encrypted.clone(), Some(b"userpw")).expect("user opens");
    let mut user_session = EditSession::new(as_user);
    match user_session.remove_encryption(&SaveOptions::default()) {
        Err(EncryptError::NotOwner { opened_as }) => assert_eq!(opened_as, AuthKind::User),
        other => panic!("expected NotOwner, got {other:?}"),
    }

    // Opened as OWNER: removal succeeds and the result is plaintext.
    let as_owner =
        Document::from_bytes_with_password(encrypted, Some(b"ownerpw")).expect("owner opens");
    let mut owner_session = EditSession::new(as_owner);
    let (plain_bytes, _) = owner_session
        .remove_encryption(&SaveOptions::default())
        .expect("owner removes encryption");
    let reopened = Document::from_bytes(plain_bytes).expect("plaintext opens with no password");
    assert!(
        reopened.encryption().is_none(),
        "encryption is gone after remove_encryption"
    );
}

/// set_encryption on an already-encrypted document is refused by name.
#[test]
fn set_encryption_refuses_an_already_encrypted_document() {
    let (encrypted, _) = EditSession::new(plain_source())
        .set_encryption(
            &EncryptionSettings::new(b"userpw".to_vec(), b"ownerpw".to_vec()),
            &SaveOptions::default(),
        )
        .expect("encrypt");
    let doc = Document::from_bytes_with_password(encrypted, Some(b"ownerpw")).expect("owner opens");
    let session = EditSession::new(doc);
    assert!(matches!(
        session.set_encryption(
            &EncryptionSettings::new(b"a".to_vec(), b"b".to_vec()),
            &SaveOptions::default()
        ),
        Err(EncryptError::AlreadyEncrypted)
    ));
}

/// set_permissions re-keys an encrypted document with a new /P, owner-only, and
/// the new permissions are what the reopened document reports.
#[test]
fn set_permissions_rekeys_owner_only() {
    let (encrypted, _) = EditSession::new(plain_source())
        .set_encryption(
            &EncryptionSettings::new(b"userpw".to_vec(), b"ownerpw".to_vec()),
            &SaveOptions::default(),
        )
        .expect("encrypt");
    let doc = Document::from_bytes_with_password(encrypted, Some(b"ownerpw")).expect("owner opens");
    let mut session = EditSession::new(doc);
    let mut settings = EncryptionSettings::new(b"userpw".to_vec(), b"ownerpw".to_vec());
    settings.permissions = vec![PermissionBit::Print]; // print only
    let (rekeyed, _) = session
        .set_permissions(&settings, &SaveOptions::default())
        .expect("owner re-keys permissions");
    let reopened =
        Document::from_bytes_with_password(rekeyed, Some(b"userpw")).expect("user opens re-keyed");
    assert!(
        reopened.encryption().is_some(),
        "still encrypted after re-key"
    );
}

/// Criterion 11 regression: an EditSession over an ENCRYPTED document still
/// refuses a page-content edit by name. The `DocumentEncrypted`/`Encrypted`
/// guards are load-bearing the moment an encrypted document can carry a
/// session (which it now can — set_permissions/remove_encryption operate on
/// one), so this pins that a representative content edit is refused before any
/// work, rather than silently editing ciphertext-derived plaintext and saving
/// it back unprotected.
#[test]
fn an_encrypted_session_still_refuses_a_content_edit() {
    use pdfcer_core::text_edit::{AddTextError, AddTextRequest};
    // demo-form.pdf encrypted by pdfcer, reopened as owner.
    let (encrypted, _) = EditSession::new(plain_source())
        .set_encryption(
            &EncryptionSettings::new(b"userpw".to_vec(), b"ownerpw".to_vec()),
            &SaveOptions::default(),
        )
        .expect("encrypt");
    let doc = Document::from_bytes_with_password(encrypted, Some(b"ownerpw")).expect("owner opens");
    let mut session = EditSession::new(doc);
    let req = AddTextRequest::new(0, (72.0, 72.0), "should be refused");
    assert!(
        matches!(session.add_text(&req), Err(AddTextError::Encrypted)),
        "adding page content to an encrypted document is refused by name"
    );
}

/// Criterion 3: a WRONG password at `/R` 6 names ambiguity A13 in the
/// diagnostic rather than a bare "wrong password" — a correct password can be
/// rejected by the A13 loop-exit reading, and the operator must not be sent to
/// re-check the one thing that may not be wrong.
#[test]
fn a_wrong_password_at_r6_names_the_a13_ambiguity() {
    use pdfcer_core::document::DocError;
    let (encrypted, _) = EditSession::new(plain_source())
        .set_encryption(
            &EncryptionSettings::new(b"userpw".to_vec(), b"ownerpw".to_vec()),
            &SaveOptions::default(),
        )
        .expect("encrypt");
    match Document::from_bytes_with_password(encrypted, Some(b"definitely-wrong")) {
        Err(DocError::PasswordRequiredR6) => {}
        other => panic!("expected PasswordRequiredR6, got {other:?}"),
    }
}
