# NEXT_SESSION.md — engineer handoff

**Read this FIRST on resume**, then the latest `docs/SESSION_LOG.md` entry for
detail. This file is engineer-owned (write it directly; it is NOT a librarian
doc). It is replaced each session with the current handoff.

**Written:** 2026-09-05, end of a long session. **HEAD:** `9301164`. **Released:**
v0.39.0 (on GitHub + OneDrive pdfcer1). **Ledger:** filings **434**, Pass ceiling
**255.0**, decisions **135**. Workspace version in `Cargo.toml` is `0.39.0`
(bump to `0.40.0` at the next release).

---

## THE PLAN (operator-approved 2026-09-05) — do these two, in order

The operator said **"build all before the next portable release unless I say
otherwise"** — accumulate the whole batch on `main` (push freely, CI validates),
and cut ONE portable release when the batch is done. Two items remain:

### 1. Pass 255.0 — markup-shape vertex editing  ← BUILD THIS FIRST
Scoped-ready and decision-free. **Acceptance criteria are already in
`docs/ROADMAP.md`** (434th filing), sourced from
`D:\Dev\Rag-Specialized\Acrobat_Features\markup__vertex_editing_and_reshape.md`.
Core-first (`pdfcer-core` annotation model + `EditSession` verbs), then the GUI
consumes it. Key criteria:
- `/Polygon` + `/PolyLine` (`/Vertices`): MOVE + INSERT + REMOVE a vertex — this
  **exceeds** current Acrobat DC (drag-only) deliberately. Min floor Polygon ≥ 3,
  PolyLine ≥ 2 → named refusal at the floor.
- `/Line`: 2 endpoints, move only.
- `/Ink` (`/InkList`): **named refusal** for per-point insert/remove (Acrobat
  never had per-point ink editing — whole-stroke only).
- Each reshape: regenerate `/AP` `/N`, recompute `/Rect`, update `/M`. ★ Cloud
  `/BE` `/Polygon` has NO `/RD`, so `/Rect` = the full bulged outline — a
  separate recompute path from Square/Circle clouds (which may carry `/RD`).
- Two independent lock gates: `/F` **Locked** blocks reshape; **LockedContents**
  does NOT. Plus the standing certified-doc / `/DocMDP` refusal.
- `/Measure` caveat: moving an endpoint recomputes the value; never silently
  clobber a manual override if one is ever added.

### 2. Digital signing — the large arc  ← BUILD SECOND (operator approved the shape)
**Fully grounded already** — read these before writing any crypto (project rule 1):
- Spec (construction side): `D:\Dev\Rag-Specialized\PDF_Spec\iso32000\iso32000__ref__signature_creation.md`,
  `…\security\security__cms_signeddata_build.md`, `…\pades\pades__ref__creation_by_level.md`,
  `…\security\security__rfc3161_timestamp.md`, `…\security\security__pkcs12_import.md`.
- Acrobat behavior / ID sources: `D:\Dev\Rag-Specialized\Acrobat_Features\signatures__digital_id_sources.md`
  (+ `signatures__signing_operation_options.md`, `…__format_and_level_choices.md`, `…__signing_defaults_and_limits.md`).

**Operator-approved shape (2026-09-05):**
- **First build = PAdES B-B (basic), default format CAdES, first key source = a
  .pfx / PKCS#12 file.** Rationale: .pfx is the ONLY source where the raw key is
  legitimately in-memory, so B-B-from-a-file is fully self-contained (no network,
  no OS key store) — ships and tests on its own. SHA-256 default.
- **Then** layer on: Windows-cert-store + PKCS#11-token key sources; B-T
  (timestamp); B-LT/B-LTA (need the backlogged revocation piece, Pass 10.6).
- **Architecture — the load-bearing design:** all four ID sources collapse to
  **hash-in / signature-out**, so `pdfcer-core` defines a `Signer` trait
  (`sign(digest) -> sig`, `certificate_chain()`) and ships a `Pkcs12Signer`
  (in-core, the one source that also supplies a local key). Windows-store / token
  signers are shell-side impls (key never leaves its custodian). This keeps the
  no-network invariant and lets the operator pick any source through one pipeline.
- Signing MUST be an **incremental update** (append; prior signatures stay valid)
  with the two-pass `/ByteRange` placeholder + back-patch (`SC-2`/`SC-7`). Needs
  NEW crypto: RSA/ECDSA **private-key** signing (crate has verify only) + PKCS#12
  parse (RFC 7292). CMS `0x31` retag (`CB-4`).
- Refusals to match: one certifying signature per doc; signing an encrypted doc
  is permission-bit-gated not blanket; refuse a signed/certified doc past its MDP.

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
- `cargo test --workspace` OOMs on the LINK phase locally — **CI is the
  authoritative full-suite backstop** (it's green on everything pushed). Run
  affected test files individually with the low-memory env.

## Release procedure (when the batch is done — bump to 0.40.0)
bump `Cargo.toml` version → commit chore → tag `v0.40.0` at the chore → build
(low-memory recipe) → `tools/package-portable.py` (rebuilds cli clean + packages;
run it from a CLEAN tree or the binary stamps `-dirty`) → fresh-folder smoke test
→ zip `pdfcer-vX-windows-x64.zip` + `.sha256` → `gh release create` → 
`tools/deploy-onedrive.py` (alternates pdfcer1/pdfcer2; pdfcer1=0.39.0 now, so
0.40.0 → pdfcer2) → `tools/verify-release.py v0.40.0` → librarian release filing.
Releasing is standing-authorized (decision 121); pushing `main` is (decision 090).

## Standing habits this session reinforced
- **README/public claims are sourced from `docs/FEATURES.md` every time** (I
  shipped a stale "OCR not built" — OCR shipped Pass 129.0). Grep FEATURES per claim.
- **Reconcile the FEATURES `gui` column against `D:\Dev\pdfcer-gui\FEATURES.md` +
  `NO_SURFACE.md`**, not CONSUMED notices — it was stale by 11 capabilities.
- **Check BOTH FeatureRequests channels every session**:
  `D:\Dev\FeatureRequests\pdfce_FeatureRequests` (open/ dir). Requests outrank backlog.
- Dispatch `pdfcer-librarian` for every ROADMAP/FEATURES/SESSION_LOG/decision write;
  `pdfcer-acrobat-librarian`/`pdfcer-spec-librarian` to ground a Pass before building.
- Never bundle code into a filing commit; file a code commit before pushing it off
  the tip (check-commits-filed exempts only the tip).

## Not for the engineer to decide (operator's)
- **Buy-me-a-coffee link** in the MIT copyright notice — the OPERATOR is handling
  this himself, in the MIT-compatible spot (the notice). Nothing owed from the engineer.
- Any B-LT/B-LTA signing → depends on Pass 10.6 revocation (backlogged, open
  operator question `(bl)` about the CC-BY-SA OCR model still stands separately).

## What shipped this session (all on main, CI green)
Pass 5.4 encryption authoring; Pass 10.2–10.5 signature trust (import Acrobat
store, evaluate chain, validity dates, RFC-5280 CA/key-usage, RSA-PSS); Pass 250.0
object→layer; 250.1 finalizing redaction; 250.2 undo-preserving deferred redaction;
250.3 encryption refuses a pending redaction (a leak 250.2 opened); 251.0 add_text
duplication + reflow deletion fix; 251.1 delete_pages ancestor `/Count` fix (nested
tree); 254.0 line-weights hairline display mode. Releases v0.38.0 and v0.39.0.
README + FEATURES `gui` column corrected.
