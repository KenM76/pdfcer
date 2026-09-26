//! Encryption **authoring** — building an AES-256 `/R` 6 standard encryption
//! dictionary from a user password, an owner password and a permission set
//! (`Pass 5.4`).
//!
//! This is the write-side mirror of [`super::standard::EncryptionConfig::parse`]
//! and the read-side algorithms. It implements ISO 32000-2:2020 Algorithms 8
//! (`/U`, `/UE`), 9 (`/O`, `/OE`) and 10 (`/Perms`) at `/R` 6 — and NOTHING
//! else: no RC4, no `/R` 2–4, no `/R` 5 authoring, by the requester's explicit
//! ask and W17's `shall not`. The one hash all three algorithms use is
//! Algorithm 2.B ([`super::r6`]), reached through [`super::r5::Hasher::R6`], so
//! a file written here opens under the exact code that reads a foreign one.
//!
//! # The self-consistency that makes this safe to trust
//!
//! Every value written here is verified by *reopening*: `Pass 5.4`'s tests
//! encrypt with this module and authenticate with the read side (both
//! passwords), and cross-check against pypdf in both directions. A wrong `/O`
//! or `/UE` cannot pass that, so the write path is proven by the read path it
//! was built to be the inverse of.
//!
//! # Licence line
//!
//! As [`super::r6`]: the algorithm numbers, step structure, byte layouts and
//! constants are cited facts; ISO's prose is not transcribed.

use super::aes::{KEY_LEN_256, encrypt_ecb_256_block, wrap_key_cbc_256};
use super::r5::Hasher;
use super::r5::PreparedPassword;
use super::r6::A13Reading;
use super::rng::{RngError, array, fill};
use super::standard::{Aes256Keys, Cipher, EncryptionConfig, PermissionBit};

/// The result of building an encryption dictionary: the parsed
/// [`EncryptionConfig`] to serialise into the file, plus the 32-byte file
/// encryption key the [writer's encoder](crate::writer) needs to encrypt every
/// string and stream.
#[derive(Debug, Clone)]
pub struct BuiltEncryption {
    /// The dictionary values — `/V 5 /R 6 /CFM AESV3`, with `/O`/`/U`/`/OE`/
    /// `/UE`/`/Perms` filled by Algorithms 8–10.
    pub config: EncryptionConfig,
    /// The file encryption key. **Secret.** Held only long enough to encrypt
    /// the document on the way out; never written to the file (that is exactly
    /// what `/UE`/`/OE` wrap).
    pub file_key: [u8; KEY_LEN_256],
}

/// Assemble the `/P` permission integer from a set of GRANTED bits, applying
/// the three write-path rules ISO 32000-1 does not state (W19,
/// `security__aes256_r6.md` §5.4):
///
/// - bits **1–2** must be **0** (a naïve "set all reserved bits to 1" gets
///   these wrong);
/// - bits **7–8** and **13–32** must be **1**;
/// - bit **10** — writers `shall` always set it to 1 for 1.7-reader
///   compatibility, regardless of whether accessibility extraction is granted
///   (at `/R` 6 the bit no longer gates it).
///
/// Returned as an `i32` (the conventional signed form Acrobat writes — e.g.
/// `-4` for "everything permitted"); the read side interprets it as the same
/// 32 bits either way.
#[must_use]
pub fn assemble_permissions(granted: &[PermissionBit]) -> i32 {
    let mut p: u32 = 0;
    let set = |p: &mut u32, bit: u32| *p |= 1 << (bit - 1);
    // The mandatory-one reserved bits.
    set(&mut p, 7);
    set(&mut p, 8);
    for b in 13..=32 {
        set(&mut p, b);
    }
    // W19: bit 10 is always 1 on the write path.
    set(&mut p, 10);
    // The operator's grants.
    for bit in granted {
        set(&mut p, bit.position());
    }
    // Bits 1 and 2 stay 0 by construction.
    #[allow(clippy::cast_possible_wrap)]
    {
        p as i32
    }
}

/// Build an AES-256 `/R` 6 encryption dictionary.
///
/// `user_pw` / `owner_pw` are the raw password bytes (SASLprep is applied by
/// [`super::standard::PreparedPassword`] — ASCII passes through unchanged; the
/// W20 gap for non-ASCII is disclosed by the caller). `permissions` is the
/// `/P` integer from [`assemble_permissions`]. `encrypt_metadata` sets byte 8
/// of `/Perms` and the dictionary's `/EncryptMetadata`. `reading` selects the
/// A13 loop-exit reading — [`A13Reading::PerformThenTest`] is the only
/// interoperable choice (it is what the read side authenticates with, and what
/// pypdf/Acrobat write), so callers pass the default; the parameter exists so
/// a test can prove the two readings differ, not so production writes vary.
///
/// # Errors
///
/// [`RngError::Unavailable`] if the OS CSPRNG cannot be reached (never on a
/// desktop target; always on `wasm32`, where encryption authoring does not
/// happen — see [`super::rng`]). A weak key is never substituted for entropy.
pub fn build_aes256_r6(
    user_pw: &[u8],
    owner_pw: &[u8],
    permissions: i32,
    encrypt_metadata: bool,
    reading: A13Reading,
) -> Result<BuiltEncryption, RngError> {
    let hasher = Hasher::R6(reading);

    // The 256-bit file encryption key — the one value that must be strong
    // (Algorithms 8/9 §7.6.4.4.1: "generated with a strong random number
    // generator"). Everything the file stores wraps or checks this; it is
    // never written directly.
    let file_key: [u8; KEY_LEN_256] = array()?;

    let user = PreparedPassword::new(user_pw);
    let owner = PreparedPassword::new(owner_pw);

    // ---- Algorithm 8: /U, /UE (user) ----
    let mut u_salts = [0u8; 16];
    fill(&mut u_salts)?;
    let uvs = &u_salts[0..8]; // User Validation Salt
    let uks = &u_salts[8..16]; // User Key Salt
    // /U = 2.B(pw ‖ UVS) ‖ UVS ‖ UKS. No U on the user path.
    let u_hash = hasher.hash(&[user.as_bytes(), uvs], user.as_bytes(), None);
    let mut u = Vec::with_capacity(48);
    u.extend_from_slice(&u_hash);
    u.extend_from_slice(uvs);
    u.extend_from_slice(uks);
    // /UE = AES-256-CBC(key = 2.B(pw ‖ UKS), iv = 0, no pad, file key).
    let uk = hasher.hash(&[user.as_bytes(), uks], user.as_bytes(), None);
    let ue = wrap_key_cbc_256(&uk, &file_key);

    // ---- Algorithm 9: /O, /OE (owner) — needs /U ----
    let u48: [u8; 48] = u.as_slice().try_into().unwrap_or([0u8; 48]);
    let mut o_salts = [0u8; 16];
    fill(&mut o_salts)?;
    let ovs = &o_salts[0..8];
    let oks = &o_salts[8..16];
    // /O = 2.B(pw ‖ OVS ‖ U) ‖ OVS ‖ OKS. U feeds both the input string and
    // every K0 (proven against pypdf; see `Hasher::R6`).
    let o_hash = hasher.hash(&[owner.as_bytes(), ovs], owner.as_bytes(), Some(&u48));
    let mut o = Vec::with_capacity(48);
    o.extend_from_slice(&o_hash);
    o.extend_from_slice(ovs);
    o.extend_from_slice(oks);
    let ok = hasher.hash(&[owner.as_bytes(), oks], owner.as_bytes(), Some(&u48));
    let oe = wrap_key_cbc_256(&ok, &file_key);

    // ---- Algorithm 10: /Perms ----
    let perms = build_perms(&file_key, permissions, encrypt_metadata)?;

    #[allow(clippy::cast_sign_loss)]
    let p_u32 = permissions as u32;
    let config = EncryptionConfig {
        version: 5,
        revision: 6,
        key_len: KEY_LEN_256,
        o,
        u,
        p: p_u32,
        encrypt_metadata,
        stream_cipher: Cipher::Aes256,
        string_cipher: Cipher::Aes256,
        aes256: Some(Aes256Keys { oe, ue, perms }),
    };

    Ok(BuiltEncryption { config, file_key })
}

/// Algorithm 10 — the 16-byte `/Perms`, AES-256-**ECB** under the file key.
///
/// Layout (§7.6.4.4.9): bytes 0–3 = `/P` low 32 bits, **low-order byte first**;
/// bytes 4–7 = the upper 32 bits of the 64-bit permission extension, all `1`;
/// byte 8 = `'T'`/`'F'` per `/EncryptMetadata`; bytes 9–11 = `'a' 'd' 'b'`;
/// bytes 12–15 = random ("which will be ignored").
fn build_perms(
    file_key: &[u8; KEY_LEN_256],
    permissions: i32,
    encrypt_metadata: bool,
) -> Result<[u8; 16], RngError> {
    let mut block = [0u8; 16];
    #[allow(clippy::cast_sign_loss)]
    let p = permissions as u32;
    block[0..4].copy_from_slice(&p.to_le_bytes());
    block[4..8].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
    block[8] = if encrypt_metadata { b'T' } else { b'F' };
    block[9] = b'a';
    block[10] = b'd';
    block[11] = b'b';
    let mut tail = [0u8; 4];
    fill(&mut tail)?;
    block[12..16].copy_from_slice(&tail);
    Ok(encrypt_ecb_256_block(file_key, &block))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::crypto::standard::AuthKind;

    /// The whole point: what this module WRITES, the read side OPENS — both
    /// passwords, and the `/Perms` self-checks.
    #[test]
    fn what_is_written_opens_under_both_passwords() {
        let p = assemble_permissions(&[PermissionBit::Print, PermissionBit::Copy]);
        let built = build_aes256_r6(b"userpw", b"ownerpw", p, true, A13Reading::default())
            .expect("desktop CSPRNG");
        let cfg = &built.config;

        // The read side authenticates each password against the written /U//O.
        let auth = |pw: &[u8]| -> Option<AuthKind> {
            // Round-trip through the same authenticate the loader uses.
            cfg.authenticate(Some(pw), b"").map(|(_, kind)| kind)
        };
        assert_eq!(auth(b"userpw"), Some(AuthKind::User), "user opens");
        assert_eq!(auth(b"ownerpw"), Some(AuthKind::Owner), "owner opens");
        assert_eq!(auth(b"wrong"), None, "a wrong password does not open");

        // And the recovered key is the file key we generated.
        let (fk_user, _) = cfg.authenticate(Some(b"userpw"), b"").expect("user auth");
        assert_eq!(
            fk_user.raw_key(),
            &built.file_key,
            "user recovers the file key"
        );
        let (fk_owner, _) = cfg.authenticate(Some(b"ownerpw"), b"").expect("owner auth");
        assert_eq!(
            fk_owner.raw_key(),
            &built.file_key,
            "owner recovers the file key"
        );
    }

    #[test]
    fn permission_bits_follow_w19() {
        let none = assemble_permissions(&[]);
        #[allow(clippy::cast_sign_loss)]
        let n = none as u32;
        assert_eq!(n & 0b11, 0, "bits 1-2 are zero");
        assert_ne!(n & (1 << 6), 0, "bit 7 is one");
        assert_ne!(n & (1 << 9), 0, "bit 10 is always one");
        assert_eq!(n & (1 << 2), 0, "print (bit 3) is not granted");
        let printable = assemble_permissions(&[PermissionBit::Print]);
        #[allow(clippy::cast_sign_loss)]
        let pv = printable as u32;
        assert_ne!(pv & (1 << 2), 0, "print (bit 3) granted");
    }
}
