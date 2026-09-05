# NEXT_SESSION.md — engineer handoff

**Read this FIRST on resume**, then the latest `docs/SESSION_LOG.md` entry for
detail. This file is engineer-owned (write it directly; it is NOT a librarian
doc). It is replaced each session with the current handoff.

**Written:** 2026-09-05 (second session of the day). **HEAD at write time:**
`80ca24d` (435th filing) + this handoff commit. **Released:** v0.39.0 (GitHub +
OneDrive pdfcer1). **Ledger:** filings **435**, Pass ceiling **255.0**, decisions
**135** (next 136), rules R241 (next R242), open questions next free `(ce)`.
Workspace version in `Cargo.toml` is `0.39.0` (bump to `0.40.0` at the next
release).

---

## THE PLAN (operator-approved 2026-09-05) — one item left in the batch

The operator said **"build all before the next portable release unless I say
otherwise"** — accumulate the whole batch on `main` (push freely, CI validates),
and cut ONE portable release when the batch is done.

### ✅ DONE — Pass 255.0 markup-shape vertex editing (`35ca5be`, filed `80ca24d`)
Read model (`Annotation::vertices/line/ink_list`), five `EditSession` verbs
(`reshape_annotation`, `_preview`, `move/insert/remove_annotation_vertex`),
`annotation-vertex` CLI, `list-annotations` prints geometry. Channel reply on
`D:\Dev\FeatureRequests\pdfce_FeatureRequests\open\reply_2026-09-05-markup-vertices-SHIPPED.md`.
Nothing owed; pdfcer-gui wires it on their side (FEATURES gui column stays `[ ]`).

### ▶ NEXT — Digital signing, the large arc (operator approved the shape)
**Fully grounded already** — read these before writing any crypto (project rule 1):
- Spec (construction side): `D:\Dev\Rag-Specialized\PDF_Spec\iso32000\iso32000__ref__signature_creation.md`
  (SC-1…SC-8: Table 252 keys, the two-pass hole, `/ByteRange` to EOF, `/SigFlags 3`,
  incremental-only), `…\security\security__cms_signeddata_build.md` (CB-1…CB-11:
  version 1, detached `id-data`, signedAttrs content-type + message-digest +
  **signing-certificate-v2 (SHA-256 default, RFC 5035)**, the **0x31 retag**, DER-sorted
  SET OF), `…\pades\pades__ref__creation_by_level.md` (PC-1…PC-12: B-B is the only
  in-core level; `/SubFilter /ETSI.CAdES.detached`; **omit CMS signing-time, set `/M`**;
  no `/Cert`), `…\security\security__pkcs12_import.md` (P12-0…P12-14: MAC first,
  BMPString password for the PKCS#12 KDF, PBES2/AES modern vs 3DES/RC2 legacy, BER
  inside), `…\security\security__rfc3161_timestamp.md` (B-T, later).
- Acrobat behaviour / ID sources: `D:\Dev\Rag-Specialized\Acrobat_Features\signatures__digital_id_sources.md`
  (+ `signatures__signing_operation_options.md`, `…__format_and_level_choices.md`,
  `…__signing_defaults_and_limits.md`).
- **Crate survey (this session, research agent):** `docs/signing-crate-survey.md` —
  the constant-time RSA/ECDSA signing crate, PKCS#12/PBES decryption crates, and
  whether RustCrypto `cms` builds SignedData or an in-house DER *writer* is cheaper.
  Decision 129 forbids reusing the in-crate verify-only bignum/ecdsa with a private
  key. **Read it, decide the stack, and have the librarian mint a decision record
  + PRIOR_ART rows BEFORE adding to `Cargo.toml`** (rule 13; all candidates are
  MIT/Apache so no operator flag is needed, but the Marvin advisory on `rsa` must be
  addressed in the record).

**Operator-approved shape (2026-09-05):**
- **First build = PAdES B-B (basic), default format CAdES, first key source = a
  .pfx / PKCS#12 file.** .pfx is the ONLY source where the raw key is legitimately
  in-memory, so B-B-from-a-file is fully self-contained (no network, no OS key
  store). SHA-256 default.
- **Then** layer on: Windows-cert-store + PKCS#11-token key sources; B-T
  (timestamp); B-LT/B-LTA (need the backlogged revocation piece, Pass 10.6).
- **Architecture — the load-bearing design:** all four ID sources collapse to
  **hash-in / signature-out**, so `pdfcer-core` defines a `Signer` trait
  (`sign(digest) -> sig`, `certificate_chain()`) and ships a `Pkcs12Signer`
  (in-core). Windows-store / token signers are shell-side impls (key never leaves
  its custodian). Keeps the no-network invariant; one pipeline for any source.
- Signing MUST be an **incremental update** with the two-pass `/ByteRange`
  placeholder + back-patch (`SC-2`/`SC-7`). Design sketch: serialise the session's
  incremental save with a zero-filled `/Contents <000…>` of reserved size L and a
  fixed-width `/ByteRange` placeholder; scan the appended revision for the hole;
  patch `/ByteRange`; digest spans; CMS build; hex; zero-pad; back-patch in place;
  **self-verify with the existing `signature_verify` before returning** (the
  verifier already knows the 0x31 retag — use it as the oracle).
- Refusals to match: one certifying signature per doc; signing an encrypted doc
  is permission-bit-gated not blanket; refuse a signed/certified doc past its MDP.
- CLI: a `Sign { input, --cert, --output }` clap variant ALREADY EXISTS at
  `crates/pdfcer-cli/src/main.rs` ~line 1821, marked `[not yet implemented]` — fill
  it in (add `--password`, `--reason/--location/--contact`, `--field-name`,
  `--visible x0,y0,x1,y1 --page`, `--signing-time D:…` — pdfcer reads no clock, so
  `/M` is caller-supplied or the CLI derives it from the system clock AND PRINTS
  that it did (rule 4/11)). Pass ID: scope with the librarian (a `10.x` sibling of
  the verification family or a new family head — librarian's call).
- Test fixture: OpenSSL 1.1.1s is on PATH — generate a synthetic self-signed RSA
  and an EC P-256 `.pfx` (modern PBES2/AES AND a legacy `-descert` 3DES one) via a
  committed `tools/gen-signing-fixtures.py`, category (a) synthetic; verify the
  output with pdfcer's own verifier AND with `openssl cms -verify` / `openssl smime`
  as an independent oracle.

---

## Build environment — READ before any release build (this cost hours)
This box has **~4.4 GB free RAM**. The normal parallel release build OOMs — it
presents as Windows **`0xC0000142` / STATUS_DLL_INIT_FAILED**, NOT a timeout or
disk-full. Full lesson in `D:\dev\rag\rust`. Recipe that works:
- Build with **`CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_BUILD_JOBS=2`** (least
  memory; also best-optimized). Slow but survives.
- The environment reaps long detached processes; **finished crates cache**, so
  relaunch the SAME config repeatedly until an uninterrupted pass finishes.
- Keep `target/debug` pruned (`rm -rf target/debug` freed 185 GB when disk hit 97%).
- **`tools/run-gates.sh` cannot be backgrounded here** (it gets reaped). What
  worked 2026-09-05: `CARGO_BUILD_JOBS=2 cargo test --workspace --all-features
  --no-run` first (compiles, caches), then `cargo test --workspace --all-features`
  (193/193 in < 10 min with a warm cache), then every other line of
  `python tools/check-ci-parity.py --list` individually in the foreground.
  `check-string-gaps.sh` runs AFTER `cargo fmt`.

## Release procedure (when the batch is done — bump to 0.40.0)
bump `Cargo.toml` version → commit chore → tag `v0.40.0` at the chore → build
(low-memory recipe) → `tools/package-portable.py` (rebuilds cli clean + packages;
run it from a CLEAN tree or the binary stamps `-dirty`) → fresh-folder smoke test
→ zip `pdfcer-vX-windows-x64.zip` + `.sha256` → `gh release create` →
`tools/deploy-onedrive.py` (alternates pdfcer1/pdfcer2; pdfcer1=0.39.0 now, so
0.40.0 → pdfcer2) → `tools/verify-release.py v0.40.0` → librarian release filing.
Releasing is standing-authorized (decision 121); pushing `main` is (decision 090).

## Standing habits this session reinforced
- **A Python patch script eats a Rust `\`-continuation** even when written with the
  Write tool: Python itself treats `\`+newline in a non-raw string as a continuation.
  Use `r"""…"""` or `\\`. `check-string-gaps.sh` caught two before commit.
- **Anchor a splice on the DOC BLOCK, not the item** — inserting before a clap
  variant whose `///` block you did not see lands your variant inside its docs.
- **Check BOTH FeatureRequests channels every session**:
  `D:\Dev\FeatureRequests\pdfce_FeatureRequests` (open/ dir). Requests outrank backlog.
  (Nothing new inbound as of this session's end.)
- Dispatch `pdfcer-librarian` for every ROADMAP/FEATURES/SESSION_LOG/decision write;
  `pdfcer-acrobat-librarian`/`pdfcer-spec-librarian` to ground a Pass before building.
- Never bundle code into a filing commit; a chore commit (even `fuzz/Cargo.lock`) is
  a CODE commit for `check-commits-filed` — name it in the filing.
- README/public claims are sourced from `docs/FEATURES.md` every time.

## Not for the engineer to decide (operator's)
- **Buy-me-a-coffee link** in the MIT copyright notice — the OPERATOR is handling
  this himself. Nothing owed from the engineer.
- Any B-LT/B-LTA signing → depends on Pass 10.6 revocation (backlogged; open
  operator question `(bl)` about the CC-BY-SA OCR model still stands separately).

## What shipped across 2026-09-05 (all on main)
Session 1: Pass 5.4 encryption authoring; Pass 10.2–10.5 signature trust; Pass
250.0–250.3 layers/redaction; 251.0/251.1 fixes; 254.0 hairline display mode;
releases v0.38.0 and v0.39.0. Session 2: **Pass 255.0** markup-shape vertex editing
(`35ca5be`), fuzz lockfile chore (`13a0a13`), 435th filing (`80ca24d`).
