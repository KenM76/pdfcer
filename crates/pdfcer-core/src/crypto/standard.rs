//! The `/Standard` security handler — ISO 32000-1 §7.6.3, Algorithms 1–7.
//!
//! This module answers three questions and nothing else:
//!
//! 1. **What does this `/Encrypt` dictionary ask for**, and is it something
//!    pdfcer can do? ([`EncryptionConfig::parse`])
//! 2. **Does this password open it**, and what is the file encryption key?
//!    ([`EncryptionConfig::authenticate`])
//! 3. **What key decrypts the string/stream in object `N G`?**
//!    ([`FileKey::object_key`], Algorithm 1)
//!
//! It does not touch the parser, does not know what a page is, and does not
//! decide policy — the caller decides what to do with a refusal or with a
//! permission bit. That separation is deliberate: every trap in clause 7.6 is
//! a *derivation* trap, and derivation is exactly what is testable in
//! isolation against a fixture whose password is known.
//!
//! # Scope: two key derivations, not one
//!
//! Implemented:
//!
//! | Configuration | Key derivation | Cipher |
//! |---|---|---|
//! | `/V` 1, 2 at `/R` 2, 3 | Algorithm 2 + Algorithm 1 | RC4, 40–128 bit |
//! | `/V` 4 at `/R` 4, `/CFM /V2` | Algorithm 2 + Algorithm 1 | RC4, 128 bit |
//! | `/V` 4 at `/R` 4, `/CFM /AESV2` | Algorithm 2 + Algorithm 1 (with `sAlT`, **T1**) | AES-128-CBC |
//! | `/V` 5 at `/R` 5, `/CFM /AESV3` | **Algorithm 3.2a** — SHA-256 + an unwrap of `/UE` or `/OE`; **no per-object step at all** (**T24**) | AES-256-CBC |
//!
//! The `/R` 5 row is a *different module* — [`r5`] — and that split
//! is the honest shape of the boundary. Adding AES-128 in increment 2 needed
//! no change here beyond deleting a refusal, because the derivation was
//! already right. Adding AES-256 in increment 3 changed almost nothing about
//! the cipher and replaced the whole key path: no MD5, no padding string, no
//! 50-round loop, no `/ID[0]`, no object number, no generation number. The
//! boundary in clause 7.6 has never been the cipher.
//!
//! Refused, by name, with the reason stated:
//!
//! | Configuration | Why refused |
//! |---|---|
//! | `/R` 6 (at any `/V`) | **Reads AND writes** (`Pass 5.4`). It is `/R` 5's harness with Algorithm 2.B substituted for SHA-256 at [`crate::crypto::r5::Hasher`]; 2.B was sourced from the ISO 32000-2:2020 primary (2026-08-12), and the write side ([`crate::crypto::encrypt`]) produces `/R` 6 files verified against pypdf |
//! | `/V 0`, `/V 3` | `/V 3` is an *unpublished* algorithm that "shall not appear in a conforming PDF file"; `/V 0` is undocumented. Nobody can open these |
//! | `/Filter` ≠ `/Standard` | Public-key and third-party handlers |
//! | **Writing** an encrypted document | **AES-256 `/R` 6 only** (`Pass 5.4`, [`crate::crypto::encrypt`]). RC4 and `/R` 2–5 are never written: ISO 32000-2 §7.6.4.1 deprecates handler revisions 1–5, and `/R` 6 is the only non-deprecated AES-256 revision (**W17**, W14) |
//!
//! A refusal names the configuration rather than saying "encrypted files are
//! not supported", because those are very different facts to an operator
//! holding a file that Chrome opens.
//!
//! # The five traps this module is built around
//!
//! Transcribed from `iso32000__ref__encryption_impl.md` §C; each produces a
//! **silently wrong key**, i.e. a file that fails to open with the right
//! password and gives no hint why.
//!
//! - **T9/T13** — Algorithm 2 step (h) truncates the digest to `n` bytes
//!   *between* its 50 rounds. Algorithm 3 step (c) runs the same 50 rounds and
//!   does **not** truncate. Two loops, three pages apart, opposite rules.
//! - **T10** — `/P` is hashed as an *unsigned little-endian 32-bit* value but
//!   stored as a *signed* PDF integer. `-44` hashes as `D4 FF FF FF`.
//! - **T11** — Algorithm 2 step (f) fires when `/EncryptMetadata` is **false**
//!   and only at `/R` ≥ 4. The default is `true`, so most files skip it.
//! - **T15** — at `/R` ≥ 3 the last 16 bytes of `/U` are *arbitrary padding*.
//!   Comparing all 32 rejects every conforming file.
//! - **T16** — Algorithm 3's RC4 loop counts **1 → 19**; Algorithm 7's counts
//!   **19 → 0** (twenty rounds, the last with key XOR 0, undoing Algorithm 3's
//!   plain pass). Reversing them fails silently.
//! - **T2** — Algorithm 1 truncates the object number to its **3** low bytes
//!   and the generation to **2**, little-endian. Normative, not a bug.
//!
//! # Authentication order is not ours to choose
//!
//! §7.6.3.1 requires trying the **empty user password** first, silently. A
//! file with an empty user password and a non-empty owner password — the
//! "permissions-only" PDF, the common case — opens with no prompt in every
//! conforming reader. Prompting for it would read to an operator as pdfcer
//! failing to open a file that every other viewer opens.
//! [`EncryptionConfig::authenticate`] takes the password as `Option`, and
//! `None` means "try the empty one", which is a different thing from the user
//! typing an empty box.
//!
//! # Permissions are disclosed, never silently enforced
//!
//! §7.6.3.1, verbatim: *"There is nothing inherent in PDF encryption that
//! enforces the document permissions."* The bits are the author's stated
//! intent, are trivially editable in the plaintext `/P`, and carry no
//! integrity protection at `/V` 1–4 (**N7**). ISO 32000-1 also specifies no
//! mapping from a bit to a *reader operation* (**N4**) — "assemble the
//! document" is not an object-level predicate. So [`Permissions`] reports what
//! the document asks for and pdfcer shows it; which pdfcer operations a bit
//! gates is a product decision that belongs in the shells, disclosed under
//! rule 4, not buried here.

use crate::crypto::aes::{KEY_LEN_256, decrypt_cbc_128, decrypt_cbc_256};
use crate::crypto::md5::{Md5, md5};
use crate::crypto::r5::{self, OE_UE_LEN, OU_LEN, PERMS_LEN, PermsCheck, PreparedPassword};
use crate::crypto::rc4::rc4;
use crate::object::{Dict, Name, ObjId, Object};

/// The 32-byte padding string, ISO 32000-1 §7.6.3.3 (Algorithm 2 step (a)).
///
/// Transcribed from the printed clause. Used three ways: to pad a short
/// password, *as* the password when there is none, and as the plaintext
/// Algorithm 4 encrypts to produce `/U`.
pub const PADDING: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

/// Why pdfcer will not decrypt a particular document.
///
/// Every variant names the *configuration*, not the capability. An operator
/// holding a file that another viewer opens needs to know which of "pdfcer
/// hasn't implemented this yet", "this is a different security handler" and
/// "no conforming reader may open this" applies — those have three different
/// next actions.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EncryptionUnsupported {
    /// `/Filter` names a handler other than `/Standard`.
    ///
    /// The public-key handler (`/SubFilter` beginning `adbe.pkcs7.`) lands
    /// here; so does any third-party handler.
    #[error("security handler /{0} is not the standard password handler")]
    Handler(String),

    /// `/Filter` is absent.
    ///
    /// Table 20 makes `/Filter` **required**: *"If this entry is absent,
    /// other security handlers shall not decrypt the document."* This is a
    /// conformance-correct refusal — no reader is permitted to open it.
    #[error("/Encrypt has no /Filter; Table 20 forbids any handler from decrypting this document")]
    NoFilter,

    /// `/V` 0 (including absent, whose default is 0) or `/V` 3.
    ///
    /// `/V` 0 is "undocumented"; `/V` 3 is "an unpublished algorithm … shall
    /// not appear in a conforming PDF file". Neither is openable by anyone
    /// except the producer.
    #[error(
        "/V {0} is an undocumented or unpublished algorithm that no conforming reader can open"
    )]
    UndocumentedAlgorithm(i64),

    /// A cipher pdfcer recognises but has not implemented. A capability gap
    /// with a known shape.
    ///
    /// The parenthetical names what *is* covered, because "AES-256 is not
    /// implemented" alone invites the reading that pdfcer cannot open encrypted
    /// files at all. It said "this increment covers RC4 only" until increment 2
    /// implemented AES-128 and made that false, and named only RC4 and AES-128
    /// until increment 3 implemented AES-256 at `/R` 5 — a message that
    /// describes the implementation's scope has to be revisited whenever the
    /// scope moves, so it is deliberately phrased as a capability list rather
    /// than as an increment number.
    ///
    /// **No configuration reaches this variant today.** It is kept rather than
    /// deleted because Table 25's `/CFM` set is closed and pdfcer now implements
    /// all four of its values, but the *next* thing the standard adds will
    /// land here, and a recognised-but-unimplemented cipher is a genuinely
    /// different fact from [`Self::UnknownCfm`]'s unrecognised one.
    #[error(
        "{0} encryption is not implemented yet (pdfcer reads RC4 40-128 bit, AES-128 and AES-256 at /R 5)"
    )]
    CipherNotImplemented(&'static str),

    /// `/CFM` outside the four names Table 25 defines.
    ///
    /// Table 25 puts a `shall` on the *diagnostic* here, unusually:
    /// applications "shall report that the file is encrypted with an
    /// unsupported algorithm".
    #[error("crypt filter method /{0} is not one of None, V2, AESV2, AESV3")]
    UnknownCfm(String),

    /// The `/Encrypt` dictionary is missing an entry the algorithms need, or
    /// holds a value of the wrong type.
    ///
    /// **N6**: ISO 32000-1 states no error model for this at all — no clause
    /// says what a reader should do when `/O` is the wrong length or `/R`
    /// disagrees with `/V`. Refusing with the specific field named is pdfcer
    /// policy.
    #[error("/Encrypt is malformed: {0}")]
    Malformed(&'static str),
}

/// The cipher a crypt filter selects (Table 25's `/CFM`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cipher {
    /// `/None` — the security handler decrypts privately. pdfcer cannot know
    /// how, so a document that routes real content through it is refused
    /// upstream; `Identity`-like passthrough is the only safe reading.
    None,
    /// `/V2` — RC4 with the file key length. **Implemented.**
    Rc4,
    /// `/AESV2` — AES-128 in CBC mode, IV prefixed to the data. **Implemented**
    /// (increment 2). Keyed by Algorithm 1 with the `sAlT` suffix, **T1**.
    Aes128,
    /// `/AESV3` — AES-256 in CBC mode. **Implemented** (increment 3) at
    /// `/R` 5 only.
    ///
    /// The cipher is reached through Algorithm 3.1a, whose entire content is
    /// "use the 32-byte file encryption key directly" — there is no per-object
    /// derivation to select here (**T24**), which is why
    /// [`FileKey::object_key`] short-circuits on `/V` ≥ 5 before it looks at
    /// the cipher at all.
    ///
    /// `/R` 6 also selects `/AESV3` and reaches the same cipher — it shares
    /// this whole path with `/R` 5, differing only in the hash ([`r5::Hasher`],
    /// Algorithm 2.B for `Pass 5.4`), never in the cipher.
    Aes256,
}

/// Access permissions as the document's author stated them (Table 22).
///
/// **This is a report, not an enforcement mechanism.** See the module docs:
/// the bits are unauthenticated at `/V` 1–4 and the standard explicitly
/// disclaims enforcement. Fields are named for what Table 22 says they
/// control, not for pdfcer operations, because ISO 32000-1 defines no mapping
/// between the two (**N4**).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    /// The raw flag word, unsigned. Preserved because a save must re-emit it
    /// verbatim — the bits pdfcer ignores still feed Algorithm 2's hash, so
    /// "normalising" them would break authentication (**N3**).
    pub raw: u32,
    /// Revision, which decides whether bits 9–12 are meaningful at all.
    pub revision: u8,
}

impl Permissions {
    /// Bit `n` (1-based, as Table 22 numbers them).
    fn bit(self, n: u32) -> bool {
        self.raw & (1 << (n - 1)) != 0
    }

    /// Bit 3 — print the document.
    ///
    /// At `/R` ≥ 3 this means "print, possibly at degraded quality"; bit 12
    /// controls whether full-fidelity printing is allowed.
    #[must_use]
    pub fn print(self) -> bool {
        self.bit(3)
    }

    /// Bit 4 — modify contents, other than what bits 6, 9 and 11 control.
    #[must_use]
    pub fn modify_contents(self) -> bool {
        self.bit(4)
    }

    /// Bit 5 — copy or extract text and graphics.
    ///
    /// At `/R` 2 this subsumes accessibility extraction; at `/R` ≥ 3 that
    /// splits out into bit 10.
    #[must_use]
    pub fn copy(self) -> bool {
        self.bit(5)
    }

    /// Bit 6 — add or modify annotations and fill form fields; with bit 4
    /// also set, create or modify form fields.
    #[must_use]
    pub fn annotate(self) -> bool {
        self.bit(6)
    }

    /// Bit 9 — fill in existing form fields, even if bit 6 is clear.
    /// Meaningful only at `/R` ≥ 3; `false` below that (the bit is reserved).
    #[must_use]
    pub fn fill_forms(self) -> bool {
        self.revision >= 3 && self.bit(9)
    }

    /// Bit 10 — extract text and graphics for accessibility.
    /// Meaningful only at `/R` ≥ 3.
    #[must_use]
    pub fn accessibility_extract(self) -> bool {
        self.revision >= 3 && self.bit(10)
    }

    /// Bit 11 — assemble: insert, rotate, delete pages; create bookmarks and
    /// thumbnails. Even if bit 4 is clear. Meaningful only at `/R` ≥ 3.
    #[must_use]
    pub fn assemble(self) -> bool {
        self.revision >= 3 && self.bit(11)
    }

    /// Bit 12 — print to a representation from which a faithful digital copy
    /// could be generated. Meaningful only at `/R` ≥ 3.
    #[must_use]
    pub fn print_high_quality(self) -> bool {
        self.revision >= 3 && self.bit(12)
    }

    /// Whether `bit` is granted — the iterable form of the accessors above.
    ///
    /// Returns `None` when the bit carries no meaning at this document's
    /// revision, which a front end must render differently from `Some(false)`:
    /// "the author did not permit this" and "this document's encryption
    /// revision has no such concept" are different statements, and collapsing
    /// them shows the operator a restriction nobody wrote.
    #[must_use]
    pub fn granted(self, bit: PermissionBit) -> Option<bool> {
        if bit.applies_at(self.revision) {
            Some(self.bit(bit.position()))
        } else {
            None
        }
    }
}

/// One permission a document's author may declare (Table 22).
///
/// Enumerated so a front end can iterate the whole set and show a complete
/// picture. A partial list would be worse than none: an operator seeing four
/// permissions cannot tell whether the other four were omitted because they
/// are allowed, because they are absent, or because nobody implemented them.
///
/// Ordered as Table 22 orders the bits, with the two print entries adjacent
/// because they are read together — bit 3 is "may print", bit 12 is "may
/// print at full quality", and bit 12 without bit 3 means nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionBit {
    /// Bit 3 — print the document.
    Print,
    /// Bit 12 — print at full fidelity. `/R` 3+ only.
    PrintHighQuality,
    /// Bit 4 — modify contents, other than what bits 6, 9 and 11 govern.
    ModifyContents,
    /// Bit 5 — copy or extract text and graphics.
    Copy,
    /// Bit 6 — add or modify annotations and fill form fields.
    Annotate,
    /// Bit 9 — fill existing form fields even if bit 6 is clear. `/R` 3+ only.
    FillForms,
    /// Bit 10 — extract for accessibility. `/R` 3+ only.
    AccessibilityExtract,
    /// Bit 11 — insert, rotate or delete pages. `/R` 3+ only.
    Assemble,
}

impl PermissionBit {
    /// Every permission, in Table 22 order.
    #[must_use]
    pub const fn all() -> [Self; 8] {
        [
            Self::Print,
            Self::PrintHighQuality,
            Self::ModifyContents,
            Self::Copy,
            Self::Annotate,
            Self::FillForms,
            Self::AccessibilityExtract,
            Self::Assemble,
        ]
    }

    /// The 1-based bit position Table 22 assigns.
    #[must_use]
    pub const fn position(self) -> u32 {
        match self {
            Self::Print => 3,
            Self::ModifyContents => 4,
            Self::Copy => 5,
            Self::Annotate => 6,
            Self::FillForms => 9,
            Self::AccessibilityExtract => 10,
            Self::Assemble => 11,
            Self::PrintHighQuality => 12,
        }
    }

    /// Whether this bit carries any meaning at handler revision `revision`.
    ///
    /// Bits 9–12 were introduced at `/R` 3. Below that they are reserved, and
    /// **reporting a reserved bit as "not allowed" would invent a restriction
    /// the document never expressed** — the author of an `/R` 2 file did not
    /// decline to permit form-filling; the concept did not exist to decline.
    #[must_use]
    pub const fn applies_at(self, revision: u8) -> bool {
        match self {
            Self::Print | Self::ModifyContents | Self::Copy | Self::Annotate => true,
            Self::FillForms
            | Self::AccessibilityExtract
            | Self::Assemble
            | Self::PrintHighQuality => revision >= 3,
        }
    }
}

/// Which password opened the document.
///
/// The distinction matters because the owner password grants full access
/// regardless of `/P` (§7.6.3.1), while the user password — and the empty
/// default password — grant `/P`-limited access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    /// The empty user password authenticated, so no prompt was shown. This is
    /// the "permissions-only" document.
    EmptyUser,
    /// A supplied user password authenticated.
    User,
    /// An owner password authenticated. Full access; `/P` is advisory only.
    Owner,
}

/// A parsed, supported `/Encrypt` dictionary.
///
/// Constructing one is the whole of "can pdfcer open this?"; it holds no key
/// and proves no password. [`Self::authenticate`] is the next step.
#[derive(Debug, Clone)]
pub struct EncryptionConfig {
    /// `/V` — the algorithm family.
    pub version: i64,
    /// `/R` — the handler revision, which selects between Algorithms 4 and 5,
    /// decides whether Algorithm 2's 50-round loop runs, and decides whether
    /// bits 9–12 of `/P` mean anything.
    pub revision: u8,
    /// File encryption key length in **bytes** (`n` in the algorithms).
    /// 5 at `/R` 2; `/Length` ÷ 8 above that; a fixed **32** at `/R` 5, where
    /// `/AESV3` mandates a 256-bit key and `/Length` is not consulted.
    pub key_len: usize,
    /// `/O`. **32 bytes at `/R` ≤ 4 and 48 at `/R` 5** (Table 3.19).
    ///
    /// The length change is not flagged by anything in the file: a 32-byte
    /// `/O` means `/R` ≤ 4 and a 48-byte one means `/R` ≥ 5, and **length is
    /// the only discriminator the format provides**. [`Self::parse`] resolves
    /// it once, against `/R`, and refuses a document whose lengths and
    /// revision disagree — the standard states no recovery for that case
    /// (**N6**).
    ///
    /// Opaque either way; never re-derived on save (**R33**, T15).
    pub o: Vec<u8>,
    /// `/U`. 32 bytes at `/R` ≤ 4 (only the first 16 are ever compared, T15)
    /// and 48 at `/R` 5, where all 48 are compared — the tail is two salts,
    /// not padding. See [`Self::o`].
    pub u: Vec<u8>,
    /// `/P` as an unsigned bit field (T10, A5).
    pub p: u32,
    /// `/EncryptMetadata`. Default `true`; `false` adds four `0xFF` bytes to
    /// Algorithm 2's hash at `/R` ≥ 4 (T11).
    pub encrypt_metadata: bool,
    /// The cipher for stream data (`/StmF`'s filter).
    pub stream_cipher: Cipher,
    /// The cipher for strings (`/StrF`'s filter). Independent of the stream
    /// cipher — a document may leave one in the clear via `/Identity`.
    pub string_cipher: Cipher,
    /// The three key strings ExtensionLevel 3 adds at `/R` 5, or `None` at
    /// `/R` ≤ 4 where they do not exist.
    pub aes256: Option<Aes256Keys>,
}

/// `/OE`, `/UE` and `/Perms` — the three entries Table 3.19 adds at `/R` 5,
/// each **Required if `/R` is 5**.
///
/// Held as fixed-size arrays rather than `Vec<u8>` so the "is this the right
/// length?" question is answered exactly once, in
/// [`EncryptionConfig::parse`], and every algorithm downstream is
/// total. A short `/UE` is a malformed document, not a runtime branch in the
/// middle of a key derivation.
///
/// All three are **opaque** and must be re-emitted verbatim on any future
/// save (**R33**). `/Perms` in particular carries four bytes of random data
/// "which will be ignored" (Algorithm 3.10, bytes 12–15), so it is not
/// reproducible — the same non-reproducibility `/U`'s tail has at `/R` ≥ 3
/// (**T15**).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Aes256Keys {
    /// `/OE` — the file encryption key wrapped under a key derived from the
    /// **owner** password.
    pub oe: [u8; OE_UE_LEN],
    /// `/UE` — the same key wrapped under a key derived from the **user**
    /// password. Both unwrap to the identical 32 bytes.
    pub ue: [u8; OE_UE_LEN],
    /// `/Perms` — the encrypted copy of `/P` and `/EncryptMetadata`, and the
    /// only integrity check in PDF encryption. Validated by
    /// [`EncryptionConfig::check_perms`]; never acted on (**T27**).
    pub perms: [u8; PERMS_LEN],
}

impl EncryptionConfig {
    /// Serialise this configuration back into a `/Encrypt` dictionary value
    /// (`Pass 5.4`, the write-side inverse of [`EncryptionConfig::parse`]).
    ///
    /// Emits exactly the entries [`parse`](EncryptionConfig::parse) reads and
    /// nothing decorative: `/Filter /Standard`, `/V`, `/R`, `/Length` (bits),
    /// signed `/P`, the byte-string `/O`/`/U`/`/OE`/`/UE`/`/Perms`, a `/CF`
    /// with a single `/StdCF` naming `/CFM /AESV3`, and `/StmF`/`/StrF` both
    /// `/StdCF`. `/EncryptMetadata` is written whenever it is `false` (its
    /// default is `true`, §7.6.2 Table 21), so a `true` document stays
    /// minimal.
    ///
    /// # Panics
    ///
    /// Only for a config with no [`Aes256Keys`] — an internal invariant of the
    /// `/V 5` writer, never reachable from a parsed `/R` ≤ 4 config because
    /// this Pass builds only `/V 5` configs. The `expect` documents that.
    #[must_use]
    #[allow(clippy::expect_used)] // documented panic (C-FAILURE); see `# Panics`
    pub fn to_encrypt_dict(&self) -> Object {
        let keys = self
            .aes256
            .as_ref()
            .expect("to_encrypt_dict is only called on a /V 5 config built by the encryptor");

        let mut stdcf = Dict::new();
        stdcf.insert(Name(b"CFM".to_vec()), Object::Name(Name(b"AESV3".to_vec())));
        stdcf.insert(
            Name(b"AuthEvent".to_vec()),
            Object::Name(Name(b"DocOpen".to_vec())),
        );
        // Table 26: /Length in a crypt filter is BYTES (32), distinct from the
        // top-level /Length which is BITS (256).
        stdcf.insert(Name(b"Length".to_vec()), Object::Integer(32));
        let mut cf = Dict::new();
        cf.insert(Name(b"StdCF".to_vec()), Object::Dict(stdcf));

        let mut d = Dict::new();
        d.insert(
            Name(b"Filter".to_vec()),
            Object::Name(Name(b"Standard".to_vec())),
        );
        d.insert(Name(b"V".to_vec()), Object::Integer(self.version));
        d.insert(
            Name(b"R".to_vec()),
            Object::Integer(i64::from(self.revision)),
        );
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        d.insert(
            Name(b"Length".to_vec()),
            Object::Integer((self.key_len as i64) * 8),
        );
        #[allow(clippy::cast_possible_wrap)]
        d.insert(
            Name(b"P".to_vec()),
            Object::Integer(i64::from(self.p as i32)),
        );
        d.insert(Name(b"CF".to_vec()), Object::Dict(cf));
        d.insert(
            Name(b"StmF".to_vec()),
            Object::Name(Name(b"StdCF".to_vec())),
        );
        d.insert(
            Name(b"StrF".to_vec()),
            Object::Name(Name(b"StdCF".to_vec())),
        );
        d.insert(Name(b"O".to_vec()), Object::String(self.o.clone()));
        d.insert(Name(b"U".to_vec()), Object::String(self.u.clone()));
        d.insert(Name(b"OE".to_vec()), Object::String(keys.oe.to_vec()));
        d.insert(Name(b"UE".to_vec()), Object::String(keys.ue.to_vec()));
        d.insert(Name(b"Perms".to_vec()), Object::String(keys.perms.to_vec()));
        if !self.encrypt_metadata {
            d.insert(Name(b"EncryptMetadata".to_vec()), Object::Boolean(false));
        }
        Object::Dict(d)
    }

    /// Parse an `/Encrypt` dictionary, refusing anything not implemented.
    ///
    /// `resolve` looks up an indirect reference, because `/O`, `/U` and the
    /// crypt-filter dictionaries **may** be indirect even though the
    /// `/Encrypt` dictionary itself must be direct in the trailer. It returns
    /// `None` for a dangling reference, matching §7.3.10's "shall not be
    /// considered an error".
    ///
    /// # Errors
    ///
    /// [`EncryptionUnsupported`] naming the specific configuration.
    ///
    /// Slicing is in bounds by construction: `/O` and `/U` are sliced to 32
    /// bytes only inside a `s.len() >= 32` guard, and the shorter case is a
    /// named refusal rather than a truncation.
    #[allow(clippy::indexing_slicing)]
    pub fn parse(
        dict: &Dict,
        resolve: &dyn Fn(ObjId) -> Option<Object>,
    ) -> Result<Self, EncryptionUnsupported> {
        let get = |key: &[u8]| -> Option<Object> {
            match dict.get(key) {
                Some(Object::Reference(id)) => resolve(*id),
                Some(other) => Some(other.clone()),
                None => None,
            }
        };

        // /Filter is Required (Table 20). Its absence is not "assume
        // Standard" — it is a document no handler may decrypt.
        match get(b"Filter") {
            Some(Object::Name(n)) => {
                if n.as_bytes() != b"Standard" {
                    return Err(EncryptionUnsupported::Handler(
                        String::from_utf8_lossy(n.as_bytes()).into_owned(),
                    ));
                }
            }
            _ => return Err(EncryptionUnsupported::NoFilter),
        }

        // /V defaults to 0, which is itself a refusal (Table 20: "shall not
        // be used"). Reading a missing /V as "probably 1" would open the door
        // to guessing at a document nobody can open.
        let version = match get(b"V") {
            Some(Object::Integer(v)) => v,
            None => 0,
            Some(_) => return Err(EncryptionUnsupported::Malformed("/V is not an integer")),
        };
        if version == 0 || version == 3 {
            return Err(EncryptionUnsupported::UndocumentedAlgorithm(version));
        }

        let revision = match get(b"R") {
            Some(Object::Integer(r)) if (2..=6).contains(&r) => r as u8,
            Some(Object::Integer(r)) => {
                return Err(EncryptionUnsupported::UndocumentedAlgorithm(r));
            }
            _ => {
                return Err(EncryptionUnsupported::Malformed(
                    "/R is missing or not an integer",
                ));
            }
        };
        // `/R` 6 is refused on the REVISION, before anything else is read.
        // The gap is Algorithm 2.B — one hash function inside an otherwise
        // fully implemented `/R` 5 harness — so every later check would pass
        // and the document would get all the way to a key derivation that
        // cannot be written. Refusing here keeps the diagnostic pointed at the
        // real cause.
        // `/R` 6 is implemented since `Pass 5.4`; it shares the whole `/R` 5
        // AES-256 harness and differs only in the hash (Algorithm 2.B), which
        // `crypto::r5::Hasher` selects by revision. So it is NOT refused here.
        // `/V` and `/R` are not independent: Table 3.19 says `/R` is 5 "if the
        // document is encrypted with a /V value of 5", and Table 3.18 says
        // `/V` 5 uses Algorithm 3.1a. A document claiming one without the
        // other has no stated reading (N6), and guessing which half to believe
        // would pick a key derivation on a coin flip.
        let is_r5 = revision == 5 || revision == 6;
        if is_r5 != (version == 5) {
            return Err(EncryptionUnsupported::Malformed(
                "/R 5 and /V 5 must appear together",
            ));
        }

        // `n`, the file key length in bytes.
        //
        // At /R 5 it is a fixed 32 and `/Length` is NOT consulted. Table 3.22:
        // "the key size (Length) shall be 256 bits" for /AESV3, so the entry
        // carries no information — and it is written inconsistently in the
        // wild, which is the practical reason not to read it. ISO 32000-2's
        // erratum to Table 25 says a STANDARD handler writes 32 (bytes) while
        // a public-key handler writes 256 (bits); 2.0 as printed said only
        // 256, so producers that followed the printed text emit `/Length 256`
        // with a standard handler. The corpus's own fixture does exactly that.
        // Accepting both by ignoring the entry is W18's "accept both on read".
        //
        // Below /R 5, §7.6.3 Algorithm 2 step (i): "shall always be 5 for
        // security handlers of revision 2" — /Length does not apply there
        // even if present.
        let key_len = if is_r5 {
            KEY_LEN_256
        } else if revision == 2 {
            5
        } else {
            match get(b"Length") {
                Some(Object::Integer(bits)) if (40..=128).contains(&bits) && bits % 8 == 0 => {
                    (bits / 8) as usize
                }
                // Absent /Length defaults to 40 bits (Table 20).
                None => 5,
                Some(_) => {
                    return Err(EncryptionUnsupported::Malformed(
                        "/Length is not a multiple of 8 in 40..=128",
                    ));
                }
            }
        };

        // `/O` and `/U` are 32 bytes at /R <= 4 and 48 at /R 5 (Table 3.19).
        // Nothing in the file marks which layout is in use; length IS the
        // discriminator, and a document whose lengths disagree with its /R has
        // no stated recovery (N6), so it is refused rather than truncated into
        // one interpretation.
        let ou_len = if is_r5 { OU_LEN } else { 32 };
        let o = match get(b"O") {
            Some(Object::String(s)) if s.len() >= ou_len => s[..ou_len].to_vec(),
            Some(Object::String(_)) => {
                return Err(EncryptionUnsupported::Malformed(
                    "/O is shorter than its revision requires (32 bytes at /R 2-4, 48 at /R 5)",
                ));
            }
            _ => {
                return Err(EncryptionUnsupported::Malformed(
                    "/O is missing or not a string",
                ));
            }
        };
        let u = match get(b"U") {
            Some(Object::String(s)) if s.len() >= ou_len => s[..ou_len].to_vec(),
            Some(Object::String(_)) => {
                return Err(EncryptionUnsupported::Malformed(
                    "/U is shorter than its revision requires (32 bytes at /R 2-4, 48 at /R 5)",
                ));
            }
            _ => {
                return Err(EncryptionUnsupported::Malformed(
                    "/U is missing or not a string",
                ));
            }
        };

        // T10/A5: stored signed, hashed unsigned. The cast is the whole of
        // the fix, and it must happen exactly here — a `/P` that reaches
        // Algorithm 2 as an `i64` produces a different hash and an
        // unexplainable authentication failure.
        let p = match get(b"P") {
            Some(Object::Integer(v)) => v as i32 as u32,
            _ => {
                return Err(EncryptionUnsupported::Malformed(
                    "/P is missing or not an integer",
                ));
            }
        };

        let encrypt_metadata = match get(b"EncryptMetadata") {
            Some(Object::Boolean(b)) => b,
            // Default true (Table 21). Absence is not "false".
            _ => true,
        };

        // At /V < 4 there are no crypt filters; the whole document is RC4.
        let (stream_cipher, string_cipher) = if version < 4 {
            (Cipher::Rc4, Cipher::Rc4)
        } else {
            let cf = match get(b"CF") {
                Some(Object::Dict(d)) => d,
                // /V 4 with no /CF means every filter name resolves to
                // Identity (Table 26), i.e. nothing is encrypted. Legal, and
                // handled by the lookup below returning None.
                _ => Dict::new(),
            };
            let named = |which: &[u8]| -> Result<Cipher, EncryptionUnsupported> {
                let name = match get(which) {
                    Some(Object::Name(n)) => n.as_bytes().to_vec(),
                    // Table 20: /StmF and /StrF default to /Identity.
                    _ => b"Identity".to_vec(),
                };
                if name == b"Identity" {
                    return Ok(Cipher::None);
                }
                let entry = match cf.get(&name) {
                    Some(Object::Reference(id)) => resolve(*id),
                    Some(other) => Some(other.clone()),
                    None => None,
                };
                let Some(Object::Dict(fd)) = entry else {
                    // N10: the standard is silent on a /StmF naming a filter
                    // absent from /CF. Treating it as Identity would silently
                    // hand ciphertext to the content parser, so refuse.
                    return Err(EncryptionUnsupported::Malformed(
                        "/StmF or /StrF names a crypt filter absent from /CF",
                    ));
                };
                match fd.get(b"CFM") {
                    Some(Object::Name(n)) => match n.as_bytes() {
                        b"None" => Ok(Cipher::None),
                        b"V2" => Ok(Cipher::Rc4),
                        b"AESV2" => Ok(Cipher::Aes128),
                        b"AESV3" => Ok(Cipher::Aes256),
                        other => Err(EncryptionUnsupported::UnknownCfm(
                            String::from_utf8_lossy(other).into_owned(),
                        )),
                    },
                    // Table 25: /CFM defaults to /None.
                    _ => Ok(Cipher::None),
                }
            };
            (named(b"StmF")?, named(b"StrF")?)
        };

        // Adobe supplement §3.5.2, amending the crypt-filter restriction:
        // "for version 4 the CFM may be V2 (RC4) or AESV2 (AES-128); for
        // version 5 the CFM SHALL be AESV3 (AES-256)". Both directions matter
        // and neither is caught anywhere else:
        //
        //   * `/AESV3` under `/V` 4 would take a 16-byte Algorithm-1 key into
        //     a cipher that needs 32, which `decrypt_cbc_256` refuses -- so
        //     every object would come back empty with no error raised.
        //   * `/V2` or `/AESV2` under `/V` 5 would run Algorithm 1 over a
        //     32-byte file key, producing a 16-byte per-object key that
        //     decrypts nothing correctly. `object_key` short-circuits on
        //     `/V` >= 5 (T24), so the mismatch would not even be visible
        //     there.
        //
        // `/Identity` (Cipher::None) stays legal at both versions: it is not a
        // CFM the restriction is about, it is the absence of one.
        for c in [stream_cipher, string_cipher] {
            let ok = match c {
                Cipher::None => true,
                Cipher::Rc4 | Cipher::Aes128 => !is_r5,
                Cipher::Aes256 => is_r5,
            };
            if !ok {
                return Err(EncryptionUnsupported::Malformed(
                    "/CFM does not match /V: version 4 takes V2 or AESV2, version 5 takes AESV3",
                ));
            }
        }

        // The three `/R` 5 key strings. Each is "Required if R is 5" in Table
        // 3.19, and each has one exact length -- a short one is a malformed
        // document, refused here so no algorithm downstream has to carry a
        // branch for it.
        let aes256 = if is_r5 {
            Some(Aes256Keys {
                oe: fixed_string(
                    get(b"OE").as_ref(),
                    "/OE is missing, is not a string, or is not the 32 bytes /R 5 requires",
                )?,
                ue: fixed_string(
                    get(b"UE").as_ref(),
                    "/UE is missing, is not a string, or is not the 32 bytes /R 5 requires",
                )?,
                perms: fixed_string(
                    get(b"Perms").as_ref(),
                    "/Perms is missing, is not a string, or is not the 16 bytes /R 5 requires",
                )?,
            })
        } else {
            None
        };

        Ok(Self {
            version,
            revision,
            key_len,
            o,
            u,
            p,
            encrypt_metadata,
            stream_cipher,
            string_cipher,
            aes256,
        })
    }

    /// Whether this is a `/R` 5 (AES-256) document.
    ///
    /// The single predicate every `/R` 5 branch below tests, named once so
    /// "is this AES-256?" is never spelled two different ways — `/V` 5 and
    /// `/R` 5 are checked for agreement in [`Self::parse`] and cannot
    /// disagree afterwards.
    fn is_r5(&self) -> bool {
        // True for BOTH /R 5 and /R 6 (`Pass 5.4`): they share the AES-256
        // /V5 harness and differ only in the hash. The name is kept for
        // continuity; read it as "the /V5 AES-256 path".
        self.revision == 5 || self.revision == 6
    }

    /// The permissions the document's author declared.
    #[must_use]
    pub fn permissions(&self) -> Permissions {
        Permissions {
            raw: self.p,
            revision: self.revision,
        }
    }

    /// Algorithm 2 — compute the file encryption key from a *user* password.
    ///
    /// `id0` is the first element of the trailer `/ID` array. It is hashed
    /// unconditionally by step (e); a file with no `/ID` hashes nothing there,
    /// which is what an empty slice gives.
    ///
    /// The two traps live in the last four lines: step (h)'s 50 rounds
    /// truncate the digest to `n` bytes **each round** (T9 — feeding the full
    /// 16 bytes back gives a different key for every `n < 16`, i.e. every
    /// 40-bit file), and step (f) fires only on `/R` ≥ 4 with
    /// `/EncryptMetadata false` (T11).
    ///
    /// Slicing is in bounds by construction: `self.key_len` is fixed by
    /// [`Self::parse`] to 5 at `/R` 2 and to `/Length / 8` for a `/Length` it
    /// has already range-checked to `40..=128`, so it is always `5..=16` --
    /// never longer than the 16-byte digest being sliced.
    #[allow(clippy::indexing_slicing)]
    fn file_key_from_user_password(&self, password: &[u8], id0: &[u8]) -> Vec<u8> {
        let mut h = Md5::new();
        h.update(&pad_password(password)); // (a), (b)
        h.update(&self.o); // (c)
        h.update(&self.p.to_le_bytes()); // (d) — unsigned, low byte first
        h.update(id0); // (e)
        if self.revision >= 4 && !self.encrypt_metadata {
            h.update(&[0xFF, 0xFF, 0xFF, 0xFF]); // (f)
        }
        let mut digest = h.finish(); // (g)

        if self.revision >= 3 {
            // (h) — 50 rounds, truncating to n each time. T9.
            for _ in 0..50 {
                digest = md5(&digest[..self.key_len]);
            }
        }
        digest[..self.key_len].to_vec() // (i)
    }

    /// Algorithm 3 steps (a)–(d) — the RC4 key derived from an *owner*
    /// password, used to encrypt `/O` (and, run backwards, to recover the
    /// user password from it).
    ///
    /// The 50-round loop here does **not** truncate (T13). That is the single
    /// most commonly transposed pair in clause 7.6: Algorithm 2 step (h)
    /// passes "the first `n` bytes", Algorithm 3 step (c) passes "**it**".
    ///
    /// `self.key_len` is `5..=16`; see [`Self::file_key_from_user_password`].
    #[allow(clippy::indexing_slicing)]
    fn owner_rc4_key(&self, owner_password: &[u8]) -> Vec<u8> {
        let mut digest = md5(&pad_password(owner_password)); // (a), (b)
        if self.revision >= 3 {
            // (c) — 50 rounds, WHOLE digest each time. T13.
            for _ in 0..50 {
                digest = md5(&digest);
            }
        }
        digest[..self.key_len].to_vec() // (d)
    }

    /// Algorithms 4 and 5 — compute what `/U` should be for a given file key.
    ///
    /// Returns 32 bytes at `/R` 2 (Algorithm 4) and 16 at `/R` ≥ 3 (Algorithm
    /// 5 stops before step (f), whose "16 bytes of arbitrary padding" is
    /// exactly the part that must not be compared — T15).
    fn expected_u(&self, file_key: &[u8], id0: &[u8]) -> Vec<u8> {
        if self.revision == 2 {
            // Algorithm 4 (b): RC4 the padding string with the file key.
            rc4(file_key, &PADDING)
        } else {
            // Algorithm 5 (b), (c): MD5 of padding ‖ ID[0].
            let mut h = Md5::new();
            h.update(&PADDING);
            h.update(id0);
            let digest = h.finish();

            // (d): RC4 with the file key.
            let mut out = rc4(file_key, &digest);

            // (e): 19 more rounds, key XOR counter 1..=19. T16 — this loop
            // counts UP; Algorithm 7's counts DOWN from 19 to 0.
            for counter in 1u8..=19 {
                let key: Vec<u8> = file_key.iter().map(|b| b ^ counter).collect();
                out = rc4(&key, &out);
            }
            out
        }
    }

    /// Algorithm 6 — does this password authenticate as the user password?
    ///
    /// Returns the file key on success. The comparison is on the first 16
    /// bytes at `/R` ≥ 3, per the algorithm's own parenthetical — comparing
    /// all 32 rejects every conforming file, because the tail is arbitrary
    /// (T15).
    ///
    /// Slicing is guarded on the same line: both lengths are checked `>= n`
    /// before either is sliced, so a malformed short `/U` is a failed
    /// authentication rather than a panic.
    #[allow(clippy::indexing_slicing)]
    fn try_user_password(&self, password: &[u8], id0: &[u8]) -> Option<Vec<u8>> {
        let key = self.file_key_from_user_password(password, id0);
        let expect = self.expected_u(&key, id0);
        let n = if self.revision == 2 { 32 } else { 16 };
        if self.u.len() >= n && expect.len() >= n && expect[..n] == self.u[..n] {
            Some(key)
        } else {
            None
        }
    }

    /// Algorithm 7 — does this password authenticate as the *owner* password?
    ///
    /// Owner authentication is definitionally two-stage (**N5**): decrypting
    /// `/O` with a key derived from the candidate yields *the user password*,
    /// which is then run through Algorithm 6. There is no owner key and no way
    /// to recover the owner password itself — knowing the owner password gives
    /// you the user password for free, and the reverse is impossible.
    ///
    /// The loop counts **19 down to 0** — twenty rounds. The `0` round has key
    /// XOR 0, i.e. the plain key, and is the inverse of Algorithm 3's
    /// un-countered step (f). Running 1..=19 instead, or counting up, fails
    /// silently for every `/R` ≥ 3 file (T16).
    fn try_owner_password(&self, password: &[u8], id0: &[u8]) -> Option<Vec<u8>> {
        let key = self.owner_rc4_key(password); // (a)

        let user_pw = if self.revision == 2 {
            // (b), R 2: a single RC4 pass. RC4 is its own inverse (T17).
            rc4(&key, &self.o)
        } else {
            // (b), R >= 3: 20 rounds, counters 19, 18, …, 1, 0.
            let mut data = self.o.clone();
            for counter in (0u8..=19).rev() {
                let k: Vec<u8> = key.iter().map(|b| b ^ counter).collect();
                data = rc4(&k, &data);
            }
            data
        };

        // (c): the result "purports to be the user password". It is a padded
        // 32-byte block, and `pad_password` truncating to 32 makes feeding it
        // back in idempotent.
        self.try_user_password(&user_pw, id0)
    }

    /// Algorithm 3.2a, steps 2–4 — `/R` 5 authentication and key recovery.
    ///
    /// Returns the 32-byte file encryption key and which password produced it,
    /// or `None` if neither did. The order matches [`Self::authenticate`]'s and
    /// is chosen for the same reason: reporting [`AuthKind::Owner`] for a
    /// document whose two passwords happen to be equal would overstate the
    /// access granted.
    ///
    /// `id0` is deliberately **not** a parameter. `/ID[0]` is an input to
    /// Algorithms 2 and 5 at `/R` ≤ 4 and to nothing at all here — the `/R` 5
    /// hashes take the password and salts only. That is worth stating rather
    /// than merely omitting, because R39 (`/ID[0]` is preserved verbatim on
    /// save) was justified partly by encryption, and at `/R` 5 that particular
    /// justification does not apply.
    ///
    /// Slicing is by fixed-size conversion: `self.o` and `self.u` are exactly
    /// [`OU_LEN`] at `/R` 5, enforced in [`Self::parse`], so the `try_into`s
    /// cannot fail — and if a future change broke that invariant they would
    /// fail closed as "no password authenticated" rather than panic.
    fn authenticate_r5(&self, password: &[u8]) -> Option<(Vec<u8>, AuthKind)> {
        let keys = self.aes256.as_ref()?;
        let o: &[u8; OU_LEN] = self.o.as_slice().try_into().ok()?;
        let u: &[u8; OU_LEN] = self.u.as_slice().try_into().ok()?;
        let prepared = PreparedPassword::new(password);
        // `/R` 5 hashes with SHA-256; `/R` 6 substitutes Algorithm 2.B at the
        // A13 reading pdfcer defaults to. Nothing else about the path differs.
        let hasher = if self.revision == 6 {
            r5::Hasher::R6(crate::crypto::r6::A13Reading::default())
        } else {
            r5::Hasher::Sha256
        };

        if r5::authenticates_as_user(&prepared, u, hasher) {
            let key = r5::file_key_from_user_password(&prepared, u, &keys.ue, hasher)?;
            let kind = if password.is_empty() {
                AuthKind::EmptyUser
            } else {
                AuthKind::User
            };
            return Some((key.to_vec(), kind));
        }
        if r5::authenticates_as_owner(&prepared, o, u, hasher) {
            let key = r5::file_key_from_owner_password(&prepared, o, u, &keys.oe, hasher)?;
            return Some((key.to_vec(), AuthKind::Owner));
        }
        None
    }

    /// Whether a *failed* authentication with `password` is ambiguous because
    /// pdfcer could not apply the password preprocessing the revision asks for.
    ///
    /// `true` only for a `/R` 5 document whose supplied password contains a
    /// byte RFC 4013's SASLprep step could have changed — see
    /// [`PreparedPassword`]'s docs for why pdfcer attempts such a password
    /// rather than refusing it, and why the disclosure belongs on the failure
    /// path rather than on the attempt.
    ///
    /// `false` for every `/R` ≤ 4 document. Password encoding below `/R` 5 is
    /// PDFDocEncoding rather than SASLprep'd UTF-8 (**T8**), which is a
    /// separate unimplemented question with a separate answer, and reporting
    /// this one there would be wrong rather than merely imprecise.
    #[must_use]
    pub fn password_may_need_normalisation(&self, password: &[u8]) -> bool {
        self.is_r5() && PreparedPassword::new(password).needs_saslprep()
    }

    /// Algorithm 3.13 — validate `/Perms` against this dictionary's `/P` and
    /// `/EncryptMetadata`.
    ///
    /// `key` must be the [`FileKey`] this configuration authenticated, because
    /// `/Perms` is encrypted under the file encryption key itself; that is
    /// exactly what makes it the only value in clause 7.6 an attacker cannot
    /// edit without the password.
    ///
    /// Returns [`PermsCheck::NotApplicable`] for every `/R` ≤ 4 document —
    /// `/Perms` was introduced by ExtensionLevel 3 and simply does not exist
    /// below `/R` 5. That is not a failed check and must not be rendered as
    /// one.
    ///
    /// **The result is a report.** pdfcer never prefers the encrypted copy over
    /// the dictionary's, never refuses a document over a mismatch, and never
    /// treats one as damage. See [`PermsCheck`]'s docs for the full argument
    /// (**T27**).
    #[must_use]
    pub fn check_perms(&self, key: &FileKey) -> PermsCheck {
        let Some(keys) = self.aes256.as_ref() else {
            return PermsCheck::NotApplicable;
        };
        let Ok(file_key) = <[u8; KEY_LEN_256]>::try_from(key.key.as_slice()) else {
            // Only reachable if a caller pairs a /R 5 config with a FileKey
            // from some other document. Reporting "the marker is not there" is
            // the honest answer -- with this key, it is not.
            return PermsCheck::MarkerMissing;
        };
        r5::validate_perms(&file_key, &keys.perms, self.p, self.encrypt_metadata)
    }

    /// Authenticate and derive the file encryption key.
    ///
    /// `password` is `None` to mean "try the default (empty) user password",
    /// which §7.6.3.1 requires a reader to do **first and silently**. That is
    /// deliberately distinct from `Some(b"")`, which is the operator typing an
    /// empty box — the two produce the same key here, but the returned
    /// [`AuthKind`] differs, and the shells use it to decide whether they ever
    /// showed a prompt.
    ///
    /// Order follows the clause: empty user password, then the supplied
    /// password as a user password, then as an owner password. Trying owner
    /// first would work, but would report [`AuthKind::Owner`] for a document
    /// whose two passwords happen to be equal, overstating the access granted.
    #[must_use]
    pub fn authenticate(&self, password: Option<&[u8]>, id0: &[u8]) -> Option<(FileKey, AuthKind)> {
        let make = |key: Vec<u8>, kind: AuthKind| {
            Some((
                FileKey {
                    key,
                    version: self.version,
                    stream_cipher: self.stream_cipher,
                    string_cipher: self.string_cipher,
                },
                kind,
            ))
        };

        // `/R` 5 runs an entirely different set of algorithms (3.2a / 3.11 /
        // 3.12) with no MD5, no padding string and no `/ID[0]`. The *order*
        // below is the same because §7.6.3.1's ordering rule is about
        // passwords, not about revisions: empty user password first and
        // silently, then the supplied password, owner last.
        if self.is_r5() {
            if let Some((key, kind)) = self.authenticate_r5(b"") {
                if password.is_none_or(<[u8]>::is_empty) {
                    return make(key, kind);
                }
                // The empty password works, but the operator supplied a
                // different one -- which may be the owner password, granting
                // more. Check before settling for the default.
                if let Some(pw) = password
                    && let Some((okey, AuthKind::Owner)) = self.authenticate_r5(pw)
                {
                    return make(okey, AuthKind::Owner);
                }
                return make(key, kind);
            }
            let pw = password?;
            let (key, kind) = self.authenticate_r5(pw)?;
            return make(key, kind);
        }

        // §7.6.3.1 step 1 — always, silently, before any prompt.
        if let Some(key) = self.try_user_password(b"", id0) {
            // A supplied password that also happens to be empty is still the
            // no-prompt case; report it as such.
            if password.is_none_or(<[u8]>::is_empty) {
                return make(key, AuthKind::EmptyUser);
            }
            // The empty password works but the operator supplied a different
            // one. Their password may be the owner password, which grants
            // more; check that before settling for the default.
            if let Some(pw) = password
                && let Some(okey) = self.try_owner_password(pw, id0)
            {
                return make(okey, AuthKind::Owner);
            }
            return make(key, AuthKind::EmptyUser);
        }

        let pw = password?;
        if let Some(key) = self.try_user_password(pw, id0) {
            return make(key, AuthKind::User);
        }
        if let Some(key) = self.try_owner_password(pw, id0) {
            return make(key, AuthKind::Owner);
        }
        None
    }
}

/// A file encryption key, plus what to do with it.
///
/// Held separately from [`EncryptionConfig`] because the config describes the
/// document and this describes a *successful authentication* — a document can
/// be parsed and refused, or parsed and prompted for, without one ever
/// existing.
#[derive(Debug, Clone)]
pub struct FileKey {
    /// The file encryption key, `n` bytes.
    key: Vec<u8>,
    /// `/V`, which Algorithm 1 needs to decide whether `n` is fixed at 5.
    version: i64,
    /// Cipher for stream data.
    stream_cipher: Cipher,
    /// Cipher for strings.
    string_cipher: Cipher,
}

impl FileKey {
    /// The raw file encryption key bytes.
    ///
    /// **Secret.** Exposed so the writer's encrypting encoder (`Pass 5.4`) can
    /// use the key at `/V` 5 (where the object key IS the file key, T24), and
    /// so a round-trip test can prove the key written into `/UE`/`/OE` is the
    /// one recovered on load.
    #[must_use]
    pub fn raw_key(&self) -> &[u8] {
        &self.key
    }

    /// Algorithm 1 — the per-object key for object `id`.
    ///
    /// Two traps, both in the byte layout:
    ///
    /// - **T2** — the object number contributes its **3** low bytes and the
    ///   generation its **2**, both little-endian. This is normative
    ///   truncation, not an implementation shortcut, and it means objects
    ///   whose numbers differ only above 2^24 share a key.
    /// - **T1** — for AES the four bytes `73 41 6C 54` (`sAlT`) extend the
    ///   *MD5 input*, not the derived key; the key stays `min(n+5, 16)`. This
    ///   was written in increment 1 while AES was still refused at parse time,
    ///   on the grounds that getting it wrong would be invisible. Increment 2
    ///   made it live and it needed no change — which is the argument for
    ///   writing the rule where it belongs rather than where it is first
    ///   reachable.
    ///
    /// Slicing is in bounds by construction: `n` is `min(key.len() + 5, 16)`
    /// and the digest is exactly 16 bytes, so the `min` is what makes it
    /// safe -- which is also the spec's own rule, not a defensive clamp.
    #[must_use]
    #[allow(clippy::indexing_slicing)]
    pub fn object_key(&self, id: ObjId, cipher: Cipher) -> Vec<u8> {
        // Algorithm 3.1a: at /V 5 the file key is used AS-IS, with no
        // per-object step (T24). This early return was written in increment 1,
        // while /V 5 was refused at parse time and it could not be reached;
        // increment 3 made it live and it needed no change. Same argument as
        // the `sAlT` branch below -- write the rule where it belongs, not
        // where it is first reachable.
        if self.version >= 5 {
            return self.key.clone();
        }

        let mut h = Md5::new();
        h.update(&self.key);
        let num = id.num.to_le_bytes();
        h.update(&num[..3]); // 3 low bytes, LE
        let generation = id.generation.to_le_bytes();
        h.update(&generation[..2]); // 2 low bytes, LE
        if cipher == Cipher::Aes128 {
            h.update(b"sAlT");
        }
        let digest = h.finish();
        let n = (self.key.len() + 5).min(16);
        digest[..n].to_vec()
    }

    /// Decrypt a **string** belonging to object `id`.
    ///
    /// **T3** — the `id` is the *containing indirect object's*, at any nesting
    /// depth. A string four levels inside a dictionary is keyed on the object
    /// that dictionary belongs to, not on anything nearer.
    #[must_use]
    pub fn decrypt_string(&self, id: ObjId, data: &[u8]) -> Vec<u8> {
        match self.string_cipher {
            Cipher::None => data.to_vec(),
            Cipher::Rc4 => rc4(&self.object_key(id, Cipher::Rc4), data),
            Cipher::Aes128 => decrypt_cbc_128(&self.object_key(id, Cipher::Aes128), data),
            // `object_key` returns the file key unchanged at /V 5 (T24), so
            // `id` contributes nothing here -- deliberately routed through the
            // same call anyway, so there is exactly one place that decides
            // what a per-object key is.
            Cipher::Aes256 => decrypt_cbc_256(&self.object_key(id, Cipher::Aes256), data),
        }
    }

    /// Decrypt a **stream's** raw data, belonging to object `id`.
    ///
    /// "Raw" is load-bearing: §7.6.2 W1 puts encryption *outside* the filter
    /// chain, so this runs **before** `/FlateDecode` and friends. Decrypting
    /// after decoding would attempt to inflate ciphertext.
    ///
    /// # The result is not necessarily the same length as the input
    ///
    /// Under RC4 it always was, and the whole of increment 1 leaned on that:
    /// plaintext was written back over ciphertext in the retained buffer and
    /// every span stayed true. Under `/AESV2` **and `/AESV3` alike** the
    /// ciphertext carries a 16-byte IV and at least one byte of padding
    /// (**T5**), so the plaintext is **strictly shorter — by at least 17
    /// bytes.** Callers must record `result.len()` rather than reuse the input
    /// length.
    #[must_use]
    pub fn decrypt_stream(&self, id: ObjId, data: &[u8]) -> Vec<u8> {
        match self.stream_cipher {
            Cipher::None => data.to_vec(),
            Cipher::Rc4 => rc4(&self.object_key(id, Cipher::Rc4), data),
            Cipher::Aes128 => decrypt_cbc_128(&self.object_key(id, Cipher::Aes128), data),
            Cipher::Aes256 => decrypt_cbc_256(&self.object_key(id, Cipher::Aes256), data),
        }
    }

    /// Whether strings are encrypted at all — `false` when `/StrF` is
    /// `/Identity`, which some producers use to leave metadata legible.
    #[must_use]
    pub fn strings_encrypted(&self) -> bool {
        self.string_cipher != Cipher::None
    }

    /// Whether stream data is encrypted at all.
    #[must_use]
    pub fn streams_encrypted(&self) -> bool {
        self.stream_cipher != Cipher::None
    }
}

/// Read a `/Encrypt` entry that must be a string of exactly `N` bytes.
///
/// Used for `/OE`, `/UE` and `/Perms`, each of which Table 3.19 marks
/// "Required if `/R` is 5" with one exact length. Enforcing the length **here**
/// rather than downstream is what lets [`crate::crypto::r5`]'s algorithms take
/// fixed-size arrays and be total: there is no "what if `/UE` is 31 bytes?"
/// branch in the middle of a key unwrap, because such a document never reaches
/// one.
///
/// Exactly `N`, not "at least `N`". `/O` and `/U` are read with a `>=` because
/// their tails are documented as arbitrary at some revisions (**T15**); these
/// three have no such allowance, and a longer one means the file disagrees
/// with the table about what it is.
///
/// # Errors
///
/// [`EncryptionUnsupported::Malformed`] carrying `message`, which names the
/// entry and its required length — the caller supplies it because
/// `Malformed` holds a `&'static str` and cannot format one.
fn fixed_string<const N: usize>(
    value: Option<&Object>,
    message: &'static str,
) -> Result<[u8; N], EncryptionUnsupported> {
    match value {
        Some(Object::String(s)) => {
            <[u8; N]>::try_from(s.as_slice()).map_err(|_| EncryptionUnsupported::Malformed(message))
        }
        _ => Err(EncryptionUnsupported::Malformed(message)),
    }
}

/// Algorithm 2 step (a) — pad or truncate a password to exactly 32 bytes.
///
/// An empty password becomes the padding string in full, which is what makes
/// the "no user password" case work: the default password *is* `PADDING`.
///
/// Slicing is in bounds by construction: `take` is `min`-clamped to 32, so
/// both `out[..take]` and `PADDING[..32 - take]` stay within their fixed
/// 32-byte arrays for every input length including zero and including
/// passwords longer than 32 bytes.
#[must_use]
#[allow(clippy::indexing_slicing)]
pub fn pad_password(password: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let take = password.len().min(32);
    out[..take].copy_from_slice(&password[..take]);
    out[take..].copy_from_slice(&PADDING[..32 - take]);
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::object::Name;

    #[test]
    fn empty_password_is_the_padding_string() {
        assert_eq!(pad_password(b""), PADDING);
    }

    #[test]
    fn short_password_takes_the_padding_prefix() {
        let p = pad_password(b"abc");
        assert_eq!(&p[..3], b"abc");
        assert_eq!(&p[3..], &PADDING[..29]);
    }

    /// "If the password string is more than 32 bytes long, use only its first
    /// 32 bytes" — no padding at all, and no hashing-down of the excess.
    #[test]
    fn long_password_is_truncated_not_hashed() {
        let long = vec![b'x'; 100];
        assert_eq!(pad_password(&long), [b'x'; 32]);
        // Exactly 32 is the boundary: still no padding bytes.
        let exact = vec![b'y'; 32];
        assert_eq!(pad_password(&exact), [b'y'; 32]);
    }

    /// T10 — `/P` is stored signed and hashed unsigned.
    ///
    /// The standard's own example: `-44`. As a `u32` that is `0xFFFFFFD4`,
    /// and low-byte-first it hashes as `D4 FF FF FF`. A parser that kept the
    /// value in an `i64` and took `to_le_bytes()` would feed eight bytes, not
    /// four, and would produce a wrong key for every document.
    #[test]
    fn p_is_hashed_as_unsigned_little_endian() {
        let p = -44i64 as i32 as u32;
        assert_eq!(p, 0xFFFF_FFD4);
        assert_eq!(p.to_le_bytes(), [0xD4, 0xFF, 0xFF, 0xFF]);
    }

    /// `granted` must distinguish "the author said no" from "the revision has
    /// no such concept". Collapsing them shows a restriction nobody wrote.
    #[test]
    fn reserved_bits_report_none_rather_than_false() {
        // `-44` has bits 9-12 SET. At R2 they are reserved, so every one of
        // them must report `None` — not `Some(true)` (which would invent a
        // permission) and not `Some(false)` (which would invent a
        // restriction).
        let raw = -44i64 as i32 as u32;
        let r2 = Permissions { raw, revision: 2 };
        let r3 = Permissions { raw, revision: 3 };

        for bit in [
            PermissionBit::FillForms,
            PermissionBit::AccessibilityExtract,
            PermissionBit::Assemble,
            PermissionBit::PrintHighQuality,
        ] {
            assert_eq!(r2.granted(bit), None, "{bit:?} is reserved at R2");
            assert_eq!(r3.granted(bit), Some(true), "{bit:?} is set at R3");
        }

        // The four that exist at every revision answer at both.
        for bit in [
            PermissionBit::Print,
            PermissionBit::ModifyContents,
            PermissionBit::Copy,
            PermissionBit::Annotate,
        ] {
            assert!(
                r2.granted(bit).is_some(),
                "{bit:?} applies at every revision"
            );
            assert_eq!(r2.granted(bit), r3.granted(bit));
        }
    }

    /// Every bit position must match Table 22, and no two may collide.
    #[test]
    fn permission_bit_positions_are_table_22() {
        let all = PermissionBit::all();
        assert_eq!(all.len(), 8, "Table 22 defines eight meaningful bits");
        let mut seen: Vec<u32> = Vec::new();
        for bit in all {
            let p = bit.position();
            assert!((3..=12).contains(&p), "{bit:?} at {p} is outside Table 22");
            assert!(!seen.contains(&p), "two permissions claim bit {p}");
            seen.push(p);
        }
        // Bits 7 and 8 are reserved and must not be claimed by anything.
        assert!(
            !seen.contains(&7) && !seen.contains(&8),
            "bits 7 and 8 are reserved"
        );
    }

    /// T12 — the standard's `-44` example is R2-only.
    ///
    /// At R2 it means print + copy. At R3+ the same number *also* grants
    /// fill-forms, accessibility extraction, assemble and high-quality print,
    /// because bits 9–12 are set in `0xFFFFFFD4` and become meaningful. An
    /// author copying the standard's example into an R4 file grants four
    /// permissions they did not intend.
    #[test]
    fn minus_44_grants_more_at_r3_than_at_r2() {
        let raw = -44i64 as i32 as u32;
        let r2 = Permissions { raw, revision: 2 };
        let r4 = Permissions { raw, revision: 4 };

        // Same at both revisions.
        assert!(r2.print() && r4.print());
        assert!(r2.copy() && r4.copy());
        assert!(!r2.modify_contents() && !r4.modify_contents());
        assert!(!r2.annotate() && !r4.annotate());

        // The four that differ.
        assert!(!r2.fill_forms() && r4.fill_forms());
        assert!(!r2.accessibility_extract() && r4.accessibility_extract());
        assert!(!r2.assemble() && r4.assemble());
        assert!(!r2.print_high_quality() && r4.print_high_quality());
    }

    /// T2 — the object number contributes 3 bytes and the generation 2.
    ///
    /// Verified by consequence rather than by inspecting the hash input:
    /// object 1 and object 0x1000001 differ only in the byte that is
    /// truncated away, so they must share a key. That is a normative
    /// property, and it is the only way to observe the truncation from
    /// outside the function.
    #[test]
    fn object_key_truncates_object_number_to_three_bytes() {
        let fk = FileKey {
            key: vec![1, 2, 3, 4, 5],
            version: 2,
            stream_cipher: Cipher::Rc4,
            string_cipher: Cipher::Rc4,
        };
        let a = fk.object_key(ObjId::new(1, 0), Cipher::Rc4);
        let b = fk.object_key(ObjId::new(0x0100_0001, 0), Cipher::Rc4);
        assert_eq!(a, b, "bits above 2^24 must not affect the key");

        // And a difference within the low 3 bytes must.
        let c = fk.object_key(ObjId::new(2, 0), Cipher::Rc4);
        assert_ne!(a, c);

        // Key length is min(n + 5, 16), so 5 + 5 = 10 here.
        assert_eq!(a.len(), 10);
    }

    /// The `min(n + 5, 16)` cap: at a 16-byte file key the object key does
    /// not grow to 21, it stops at the digest length.
    #[test]
    fn object_key_length_is_capped_at_sixteen() {
        let fk = FileKey {
            key: vec![0u8; 16],
            version: 4,
            stream_cipher: Cipher::Rc4,
            string_cipher: Cipher::Rc4,
        };
        assert_eq!(fk.object_key(ObjId::new(1, 0), Cipher::Rc4).len(), 16);
    }

    /// T1 — the `sAlT` bytes change the key's *value*, not its length.
    ///
    /// Written in increment 1 as a guard on the next one, while AES was still
    /// refused at parse time: if the salt were appended to the *key* instead of
    /// to the hash *input*, the length would change here and every AES document
    /// would fail. Increment 2 made it live, and the end-to-end proof is now
    /// `pdfcer`'s `decrypting_reproduces_the_plaintext_document_exactly`.
    /// This unit test stays because it localises the failure: it says *which*
    /// of the two salt mistakes was made, where the pixel comparison only says
    /// that something is wrong.
    #[test]
    fn aes_salt_changes_value_not_length() {
        let fk = FileKey {
            key: vec![9u8; 16],
            version: 4,
            stream_cipher: Cipher::Rc4,
            string_cipher: Cipher::Rc4,
        };
        let plain = fk.object_key(ObjId::new(7, 0), Cipher::Rc4);
        let salted = fk.object_key(ObjId::new(7, 0), Cipher::Aes128);
        assert_eq!(plain.len(), salted.len());
        assert_ne!(plain, salted);
    }

    fn name(s: &str) -> Object {
        Object::Name(Name(s.as_bytes().to_vec()))
    }

    fn nothing(_: ObjId) -> Option<Object> {
        None
    }

    fn minimal_encrypt(entries: Vec<(&str, Object)>) -> Dict {
        let mut d = Dict::new();
        d.insert(Name(b"Filter".to_vec()), name("Standard"));
        d.insert(Name(b"O".to_vec()), Object::String(vec![0u8; 32]));
        d.insert(Name(b"U".to_vec()), Object::String(vec![0u8; 32]));
        d.insert(Name(b"P".to_vec()), Object::Integer(-44));
        for (k, v) in entries {
            d.insert(Name(k.as_bytes().to_vec()), v);
        }
        d
    }

    /// `/V` 3 is the unpublished algorithm; `/V` 0 is undocumented. Both are
    /// refused as "nobody can open this", which is a different message from
    /// "pdfcer hasn't implemented it".
    #[test]
    fn refuses_undocumented_algorithms() {
        for v in [0i64, 3] {
            let d = minimal_encrypt(vec![("V", Object::Integer(v)), ("R", Object::Integer(3))]);
            assert_eq!(
                EncryptionConfig::parse(&d, &nothing).unwrap_err(),
                EncryptionUnsupported::UndocumentedAlgorithm(v)
            );
        }
        // Absent /V defaults to 0 and is refused the same way.
        let d = minimal_encrypt(vec![("R", Object::Integer(3))]);
        assert_eq!(
            EncryptionConfig::parse(&d, &nothing).unwrap_err(),
            EncryptionUnsupported::UndocumentedAlgorithm(0)
        );
    }

    /// A missing `/Filter` is not "assume Standard" — Table 20 makes it a
    /// document no handler is permitted to decrypt.
    #[test]
    fn refuses_missing_filter_as_a_conformance_matter() {
        let mut d = minimal_encrypt(vec![("V", Object::Integer(2)), ("R", Object::Integer(3))]);
        d.remove(b"Filter");
        assert_eq!(
            EncryptionConfig::parse(&d, &nothing).unwrap_err(),
            EncryptionUnsupported::NoFilter
        );
    }

    #[test]
    fn refuses_public_key_handler_by_name() {
        let mut d = minimal_encrypt(vec![("V", Object::Integer(2)), ("R", Object::Integer(3))]);
        d.insert(Name(b"Filter".to_vec()), name("Adobe.PubSec"));
        assert!(matches!(
            EncryptionConfig::parse(&d, &nothing),
            Err(EncryptionUnsupported::Handler(h)) if h == "Adobe.PubSec"
        ));
    }

    /// An `/R` 5 `/Encrypt` dictionary, with the fixture's real 48-byte `/O`
    /// and `/U` and 32-byte `/OE`/`/UE`/`/Perms` — the shape Table 3.19
    /// requires. Built separately from [`minimal_encrypt`] because at `/R` 5
    /// nearly every entry has a different length, and a helper that papered
    /// over that would be testing a document shape the standard does not
    /// define.
    fn r5_encrypt(overrides: Vec<(&str, Object)>) -> Dict {
        let mut cf = Dict::new();
        let mut stdcf = Dict::new();
        stdcf.insert(Name(b"CFM".to_vec()), name("AESV3"));
        cf.insert(Name(b"StdCF".to_vec()), Object::Dict(stdcf));

        let mut d = Dict::new();
        d.insert(Name(b"Filter".to_vec()), name("Standard"));
        d.insert(Name(b"V".to_vec()), Object::Integer(5));
        d.insert(Name(b"R".to_vec()), Object::Integer(5));
        // /Length 256, the value 2.0-as-printed asks for and the value pdfcer's
        // own fixture carries. It is outside the 40..=128 range /R <= 4
        // enforces, so a parse that consulted it here would refuse the file.
        d.insert(Name(b"Length".to_vec()), Object::Integer(256));
        d.insert(Name(b"P".to_vec()), Object::Integer(-4));
        d.insert(Name(b"O".to_vec()), Object::String(vec![0u8; 48]));
        d.insert(Name(b"U".to_vec()), Object::String(vec![0u8; 48]));
        d.insert(Name(b"OE".to_vec()), Object::String(vec![0u8; 32]));
        d.insert(Name(b"UE".to_vec()), Object::String(vec![0u8; 32]));
        d.insert(Name(b"Perms".to_vec()), Object::String(vec![0u8; 16]));
        d.insert(Name(b"CF".to_vec()), Object::Dict(cf));
        d.insert(Name(b"StmF".to_vec()), name("StdCF"));
        d.insert(Name(b"StrF".to_vec()), name("StdCF"));
        for (k, v) in overrides {
            d.insert(Name(k.as_bytes().to_vec()), v);
        }
        d
    }

    /// `/R` 5 parses, resolves to [`Cipher::Aes256`], and fixes `n` at 32
    /// **without reading `/Length`** — which here says 256, a value the
    /// `/R` ≤ 4 path would reject outright.
    ///
    /// Both halves matter. A parse that consulted `/Length` would refuse every
    /// file written to ISO 32000-2 as printed; a parse that read it as
    /// `256 / 8 = 32` would happen to work on those and break on the files
    /// that follow the erratum and write `/Length 32` (**W18**, **A11**).
    #[test]
    fn r5_parses_and_ignores_length_entirely() {
        for length in [Object::Integer(256), Object::Integer(32)] {
            let d = r5_encrypt(vec![("Length", length.clone())]);
            let c = EncryptionConfig::parse(&d, &nothing).expect("/R 5 is implemented");
            assert_eq!(c.revision, 5);
            assert_eq!(c.key_len, 32, "with /Length {length:?}");
            assert_eq!(c.stream_cipher, Cipher::Aes256);
            assert_eq!(c.string_cipher, Cipher::Aes256);
            assert_eq!(c.o.len(), 48, "/O is 48 bytes at /R 5");
            assert_eq!(c.u.len(), 48);
            assert!(c.aes256.is_some());
        }

        // And a /R 5 file with no /Length at all is fine, where /R 3 would
        // default to 40 bits.
        let mut d = r5_encrypt(vec![]);
        d.remove(b"Length");
        assert_eq!(
            EncryptionConfig::parse(&d, &nothing)
                .expect("supported")
                .key_len,
            32
        );
    }

    /// `/R` 6 PARSES (`Pass 5.4`, criterion 2). It used to be refused as
    /// "unsourced"; Algorithm 2.B was sourced from the ISO 32000-2 primary on
    /// 2026-08-12, `/R` 6 now decrypts through `/R` 5's harness with 2.B
    /// substituted, and the write side produces `/R` 6 files, so a refusal
    /// here would make a document pdfcer WROTE unopenable. It shares every
    /// structural check with `/R` 5 (identical dictionary shape), differing
    /// only in the hash at authentication time.
    #[test]
    fn r6_parses_like_r5_now_that_algorithm_2b_is_sourced() {
        let r6 = r5_encrypt(vec![("R", Object::Integer(6))]);
        let cfg = EncryptionConfig::parse(&r6, &nothing).expect("/R 6 parses");
        assert_eq!(cfg.revision, 6);
        assert_eq!(cfg.stream_cipher, Cipher::Aes256);

        // The structural checks it shares with /R 5 still bite: a /R 6
        // dictionary missing /UE is malformed the same way a /R 5 one is,
        // and is NOT waved through as "unsupported revision".
        let mut broken = r5_encrypt(vec![("R", Object::Integer(6))]);
        broken.remove(b"UE");
        assert!(
            EncryptionConfig::parse(&broken, &nothing).is_err(),
            "a structurally broken /R 6 dictionary is still refused"
        );

        // And /R 5 parses, as before.
        assert!(EncryptionConfig::parse(&r5_encrypt(vec![]), &nothing).is_ok());
    }

    /// `/V` and `/R` must agree. A document claiming one 5 without the other
    /// has no stated reading (**N6**), and picking a key derivation from a
    /// coin flip is worse than refusing.
    #[test]
    fn v5_and_r5_must_appear_together() {
        for (v, r) in [(4i64, 5i64), (5, 4), (5, 3), (2, 5)] {
            let d = r5_encrypt(vec![("V", Object::Integer(v)), ("R", Object::Integer(r))]);
            assert!(
                matches!(
                    EncryptionConfig::parse(&d, &nothing),
                    Err(EncryptionUnsupported::Malformed(_))
                ),
                "/V {v} with /R {r} must be refused"
            );
        }
    }

    /// The §3.5.2 crypt-filter restriction, **both ways**.
    ///
    /// Neither direction is caught anywhere else, and both are silent: an
    /// `/AESV3` filter under `/V` 4 hands a 16-byte key to a cipher needing
    /// 32 (every object decrypts to nothing, no error), and an `/AESV2`
    /// filter under `/V` 5 runs a 32-byte file key through a path that at
    /// `/V` 5 does not derive per-object keys at all (**T24**).
    #[test]
    fn cfm_must_match_the_version_in_both_directions() {
        let mut cf = Dict::new();
        let mut stdcf = Dict::new();
        stdcf.insert(Name(b"CFM".to_vec()), name("AESV3"));
        cf.insert(Name(b"StdCF".to_vec()), Object::Dict(stdcf));
        let v4_aesv3 = minimal_encrypt(vec![
            ("V", Object::Integer(4)),
            ("R", Object::Integer(4)),
            ("Length", Object::Integer(128)),
            ("CF", Object::Dict(cf)),
            ("StmF", name("StdCF")),
            ("StrF", name("StdCF")),
        ]);
        assert!(
            matches!(
                EncryptionConfig::parse(&v4_aesv3, &nothing),
                Err(EncryptionUnsupported::Malformed(_))
            ),
            "/AESV3 under /V 4 must be refused"
        );

        let mut cf = Dict::new();
        let mut stdcf = Dict::new();
        stdcf.insert(Name(b"CFM".to_vec()), name("AESV2"));
        cf.insert(Name(b"StdCF".to_vec()), Object::Dict(stdcf));
        let v5_aesv2 = r5_encrypt(vec![("CF", Object::Dict(cf))]);
        assert!(
            matches!(
                EncryptionConfig::parse(&v5_aesv2, &nothing),
                Err(EncryptionUnsupported::Malformed(_))
            ),
            "/AESV2 under /V 5 must be refused"
        );

        // /Identity stays legal at /V 5: it is the absence of a crypt filter,
        // not a competing one, and producers use it to leave metadata legible.
        let identity = r5_encrypt(vec![("StrF", name("Identity"))]);
        let c = EncryptionConfig::parse(&identity, &nothing).expect("/Identity is legal at /V 5");
        assert_eq!(c.string_cipher, Cipher::None);
        assert_eq!(c.stream_cipher, Cipher::Aes256);
    }

    /// The three `/R` 5 key strings are Required, and each has one exact
    /// length. Enforcing that here is what lets [`crate::crypto::r5`] take
    /// fixed-size arrays and have no length branch inside a key derivation.
    #[test]
    fn r5_key_strings_are_required_at_exactly_their_lengths() {
        for key in ["OE", "UE", "Perms"] {
            let mut d = r5_encrypt(vec![]);
            d.remove(key.as_bytes());
            assert!(
                matches!(
                    EncryptionConfig::parse(&d, &nothing),
                    Err(EncryptionUnsupported::Malformed(_))
                ),
                "a missing /{key} must be refused"
            );
        }
        // Wrong length, both directions -- 31 and 33 bytes of /UE are equally
        // not what Table 3.19 describes.
        for len in [31usize, 33] {
            let d = r5_encrypt(vec![("UE", Object::String(vec![0u8; len]))]);
            assert!(
                matches!(
                    EncryptionConfig::parse(&d, &nothing),
                    Err(EncryptionUnsupported::Malformed(_))
                ),
                "/UE of {len} bytes must be refused"
            );
        }
    }

    /// A `/R` 5 document with a 32-byte `/O` — the `/R` ≤ 4 length — is
    /// refused rather than read as either revision.
    ///
    /// **Length is the only discriminator the format provides** between the
    /// two layouts; nothing tags them. So a file whose `/R` and whose string
    /// lengths disagree is genuinely ambiguous, the standard states no
    /// recovery (**N6**), and truncating it into one reading would produce a
    /// wrong key with no diagnostic.
    #[test]
    fn r5_with_a_32_byte_o_is_refused_rather_than_guessed() {
        let d = r5_encrypt(vec![("O", Object::String(vec![0u8; 32]))]);
        assert!(matches!(
            EncryptionConfig::parse(&d, &nothing),
            Err(EncryptionUnsupported::Malformed(_))
        ));
    }

    /// `/R` 2 fixes `n` at 5 regardless of `/Length` — Algorithm 2 step (i)
    /// says "shall always be 5 for security handlers of revision 2".
    /// A `/Length 128` on an R2 file must not widen the key.
    #[test]
    fn r2_ignores_length() {
        let d = minimal_encrypt(vec![
            ("V", Object::Integer(1)),
            ("R", Object::Integer(2)),
            ("Length", Object::Integer(128)),
        ]);
        let c = EncryptionConfig::parse(&d, &nothing).expect("R2 RC4 is supported");
        assert_eq!(c.key_len, 5);
    }

    /// Absent `/Length` defaults to 40 bits, not to "whatever /V implies".
    #[test]
    fn absent_length_defaults_to_forty_bits() {
        let d = minimal_encrypt(vec![("V", Object::Integer(2)), ("R", Object::Integer(3))]);
        let c = EncryptionConfig::parse(&d, &nothing).expect("R3 RC4 is supported");
        assert_eq!(c.key_len, 5);
    }

    /// `/EncryptMetadata` defaults to **true**; its absence must not be read
    /// as false, which would add four bytes to Algorithm 2's hash (T11) and
    /// produce a wrong key for every ordinary R4 document.
    #[test]
    fn encrypt_metadata_defaults_true() {
        let d = minimal_encrypt(vec![("V", Object::Integer(2)), ("R", Object::Integer(3))]);
        let c = EncryptionConfig::parse(&d, &nothing).expect("supported");
        assert!(c.encrypt_metadata);
    }

    /// A `/V 4` document routing streams through `/AESV2` **parses**, and
    /// resolves to [`Cipher::Aes128`] on both the stream and string sides.
    ///
    /// This was a refusal assertion until increment 2 implemented AES-128. It
    /// is kept as an acceptance assertion rather than deleted, because the
    /// thing worth pinning never was the refusal — it is that `/CFM /AESV2`
    /// reaches the AES branch **at all**. Table 25 lists four `/CFM` names and
    /// an unrecognised one falls through to `Cipher::None`, i.e. "do not
    /// decrypt", which would leave every stream as ciphertext with no error
    /// raised. Asserting the resolved cipher catches that; asserting only that
    /// `parse` succeeded would not.
    #[test]
    fn aesv2_resolves_to_the_aes_128_cipher() {
        let mut cf = Dict::new();
        let mut stdcf = Dict::new();
        stdcf.insert(Name(b"CFM".to_vec()), name("AESV2"));
        cf.insert(Name(b"StdCF".to_vec()), Object::Dict(stdcf));

        let d = minimal_encrypt(vec![
            ("V", Object::Integer(4)),
            ("R", Object::Integer(4)),
            ("Length", Object::Integer(128)),
            ("CF", Object::Dict(cf)),
            ("StmF", name("StdCF")),
            ("StrF", name("StdCF")),
        ]);
        let c = EncryptionConfig::parse(&d, &nothing).expect("AES-128 is implemented");
        assert_eq!(c.stream_cipher, Cipher::Aes128);
        assert_eq!(c.string_cipher, Cipher::Aes128);
    }

    /// `/V 4` + `/CFM /V2` is the supported crypt-filter case, and `/StmF`
    /// and `/StrF` are independent — a document may encrypt streams and leave
    /// strings in the clear.
    #[test]
    fn v4_v2_filter_is_supported_and_stmf_strf_are_independent() {
        let mut cf = Dict::new();
        let mut stdcf = Dict::new();
        stdcf.insert(Name(b"CFM".to_vec()), name("V2"));
        cf.insert(Name(b"StdCF".to_vec()), Object::Dict(stdcf));

        let d = minimal_encrypt(vec![
            ("V", Object::Integer(4)),
            ("R", Object::Integer(4)),
            ("Length", Object::Integer(128)),
            ("CF", Object::Dict(cf)),
            ("StmF", name("StdCF")),
            ("StrF", name("Identity")),
        ]);
        let c = EncryptionConfig::parse(&d, &nothing).expect("V2 crypt filter is supported");
        assert_eq!(c.stream_cipher, Cipher::Rc4);
        assert_eq!(c.string_cipher, Cipher::None);
        assert_eq!(c.key_len, 16);
    }

    /// N10 — a `/StmF` naming a filter absent from `/CF` is refused rather
    /// than silently treated as Identity. Guessing Identity would hand
    /// ciphertext to the content parser, which fails much later and in a
    /// place that looks nothing like an encryption problem.
    #[test]
    fn refuses_stmf_naming_an_absent_crypt_filter() {
        let d = minimal_encrypt(vec![
            ("V", Object::Integer(4)),
            ("R", Object::Integer(4)),
            ("CF", Object::Dict(Dict::new())),
            ("StmF", name("StdCF")),
        ]);
        assert!(matches!(
            EncryptionConfig::parse(&d, &nothing),
            Err(EncryptionUnsupported::Malformed(_))
        ));
    }

    /// An unknown `/CFM` gets its own diagnostic — Table 25 puts a `shall` on
    /// *reporting* this case, which is unusual enough to be worth honouring
    /// precisely.
    #[test]
    fn unknown_cfm_is_named() {
        let mut cf = Dict::new();
        let mut f = Dict::new();
        f.insert(Name(b"CFM".to_vec()), name("Whirlpool"));
        cf.insert(Name(b"StdCF".to_vec()), Object::Dict(f));

        let d = minimal_encrypt(vec![
            ("V", Object::Integer(4)),
            ("R", Object::Integer(4)),
            ("CF", Object::Dict(cf)),
            ("StmF", name("StdCF")),
        ]);
        assert!(matches!(
            EncryptionConfig::parse(&d, &nothing),
            Err(EncryptionUnsupported::UnknownCfm(n)) if n == "Whirlpool"
        ));
    }
}
