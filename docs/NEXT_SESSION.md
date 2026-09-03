# NEXT SESSION — start here

Engineer-owned handoff. Read this **before** `ROADMAP.md` — that says what
shipped, this says what to do next. **Overwrite it once acted on.**

Per standing rule `R216` this file carries **no edit-history layer**. What is
true now, plus a pointer. Corrections and their prior wording live in the
append-only record (`ROADMAP.md`, `SESSION_LOG.md`).

Written **2026-09-03**, at the end of the session that executed the fork:
**`Pass 247.0` / `247.1` / `247.2`** — the project is now **`pdfcer`**, in
**`D:\Dev\pdfcer`**, published at **`github.com/KenM76/pdfcer`**, released as
**`v0.28.0`**. Everything below was measured with a shell.

**For the ledger — Pass ceiling, rule ceiling, decision ceiling, filing count —
run `python tools/check-ledger-numbers.py`.** Do not mint from memory.

---

## §0 ★★ WHERE YOU ARE. Open the session in `D:\Dev\pdfcer`, not `D:\Dev\pdfce`

- **`D:\Dev\pdfcer` is the project.** `D:\Dev\pdfce` and `KenM76/pdfce` are
  the operator's frozen backup (archived on GitHub; two pointer commits,
  `fbc53ee` + `c0c67d3`, are the only writes it ever received after the
  clone). **Never write there again.** If a session opens in `D:\Dev\pdfce`,
  `cd D:\Dev\pdfcer` before doing anything.
- Agents are `.claude/agents/pdfcer-*.md`; dispatch by the new names
  (`pdfcer-librarian`, …). Their memory is `.claude/agent-memory/pdfcer-*`.
- Crates: `pdfcer-core`, `pdfcer-render`, `pdfcer-cli`, `pdfcer-print`,
  `pdfcer-fetch`. **The CLI binary is `pdfcer`** (`[[bin]] name`, clap
  `name`, `pdfcer.exe`, diagnostic prefix `pdfcer: `); the crate keeps
  `-cli` (`-p pdfcer-cli`, `crates/pdfcer-cli`). `PDFCER_*` env names.
  OneDrive slots `pdfcer1` / `pdfcer2`.
- **The operator still owes** the global `C:\Users\Ken\.claude\CLAUDE.md`
  (it references `D:\Dev\pdfce\`); not an agent's file to edit.
- **The GUI side has not re-pointed yet.** `D:\dev\pdfcer-gui`'s three
  dependency lines still target `file:///D:/Dev/pdfce` with
  `package = "pdfce-core"` shims; the exact replacement lines are in the
  channel note
  `note_pdfcer_is_live_re_point_your_three_dependency_lines_to_D_Dev_pdfcer.md`.
  Until they switch, nothing after `cce414e` reaches them.

---

## §1 ★ NEXT: `Pass 5.4` — encryption authoring, `/R` 6 / AES-256 only

Queued and scoped at the *Next up* head of `ROADMAP.md` (396th filing;
inbound pdfcer-gui request
`request_a_document_cannot_be_encrypted_or_have_its_permissions_set.md`).
In one paragraph: `EditSession::set_encryption` (owner + user password,
`/R` 6 only — `/R` 2–4 and 5 refused by name), `set_permissions`,
`remove_encryption`, all owner-authenticated or refused. Spec is SOURCED:
`PDF_Spec\security\security__aes256_r6.md` (Algorithm 2.B complete) and
`iso32000__ref__encryption_impl.md` rows A8/W17/W19/W20/A13. **Owed with
it:** the crate's stale "not available in the spec corpus" strings
(`crypto/standard.rs`'s `EncryptionUnsupported::UnsourcedRevision` and the
doc comments in `crypto/mod.rs`, `standard.rs`, `aes.rs`, `r5.rs`). Writer
side: `Contents` is never encrypted and a signature digest is over
CIPHERTEXT (ETSI EN 319 142-1 §5.5) — the exemption list must carry it.
Disclosure sentence for permissions is in
`reply_signature_integrity_first_then_encryption_and_your_two_sentences.md`.
Ship the `pdfcer` subcommands in the same Pass (rule 11).

## §2 QUEUED AFTER

1. Mesh shadings deposit spot planes; the 8 unresolved conformance patches;
   `sh` cutting under a redaction mark; `set_page_tabs` when asked.
2. Rename debt, all filed, none blocking: `pdfceF{n}`/`pdfceFm{n}`
   resource-name prefix is a **deliberate keep** (opaque `/Resources` keys;
   401st filing) — do not "fix" it; `ROADMAP.md`'s own header/Glossary
   still say `pdfce` (librarian's); FEATURES's `pdfcer-fetch` row still
   cites `pdfce-gui` (R203 re-basing).
3. iccce's `reply_all_four_asks_measured_and_your_bpc_would_have_done_nothing.md`
   is STILL unread by any engine session.

---

## §3 STATE OF THE TREE — verified 2026-09-03 at hand-off

- `main` = `562ca7e` + the 401st filing + the sidecar-compat removal and
  its filing (pushed; check with
  `git log --oneline origin/main..HEAD`). Tag `v0.28.0` at `562ca7e`; on
  GitHub as Latest (`pdfcer-v0.28.0-windows-x64.zip`, SHA-256
  `64d77604…1466`); OneDrive `pdfcer1` = 0.28.0, `pdfcer2` = 0.27.0 (a
  copy of `pdfce2`, still named `pdfce-cli.exe`); `verify-release.py
  v0.28.0` clean. CI green at `562ca7e` (run 33786657866).
- **Test total:** `cargo test --workspace` **4,733 / 0** (5,114 before the
  GUI strip − 384 GUI tests; the 3 sidecar-compat tests came and went).
  `run-gates.sh` PASS 29/29.
- **Channels:** our unconsumed outbound on the pdfce channel:
  `reply_signature_integrity_first…` and the new `note_pdfcer_is_live…`.
  Their two security requests stay open until they consume.
- **Disk:** a fresh clone's first build is ~1.5 min per profile now that
  the GUI is gone (was ~6). `target/` in the OLD folder can be deleted.

---

## §4 THINGS A NEW SESSION MUST KNOW BEFORE TOUCHING ANYTHING

- **★ A Python heredoc through the Bash tool eats one level of
  backslashes** — recurred AGAIN this session (a `\n` inside a patch
  became a real newline and broke the script). Any patch containing ANY
  backslash goes through the `Write` tool to `D:\Dev\temp\<name>.py`, then
  `python <file>`; every patch asserts its anchor count.
- **A mechanical rename's failure surface is punctuation.** Three classes
  the regex sweep could not decide, all found by the gates or the tests,
  none by reading: a spelling glued to an escape (`\nPDFCE_TEXT`, `\tpdfce`
  — `\b` sees a word char); a quoted path component (`"crates" /
  "pdfce-cli"` where the "means the tool" rule wanted `pdfcer`); and test
  expectations that must equal FIXTURE BYTES (certificate subjects,
  `/Reason`) — restored, with the generator told to keep the old strings.
- **Decision 131 (read legacy keys, retire on write) was OVERRULED the
  same day.** Ken, verbatim: *"This compatibility layer can be removed"* —
  nobody, including him, has used the software in production. The
  ce-dimension sidecar is read and written under `/pdfcer` only; the
  three deletion-collateral fixtures were regenerated with that key. Do
  not add back-compat shims for pre-release formats without asking.
- **0xc0000142 in a doctest is starvation, not a failure.** Run
  `run-gates.sh` alone; re-run the doctest on its own before believing it.
- **Stage by path. Never `git add -A`.** Push a code commit and its filing
  commit together. Read CI's colour from GitHub:
  `gh run list -R KenM76/pdfcer --limit 3 --json status,conclusion,headSha`.
- **`docs/core-api/` is engineer-owned and moves in the SAME commit** as
  any `pub` change; `check-core-api-verbs.py` also checks the index's
  stated line counts.

## §5 ★★ MEASURED NEGATIVES — DO NOT RE-DERIVE

1. `check-string-gaps.sh` is NOT GUI-only (it scans `crates/` and
   `tools/`); the plan that said so was wrong and is annotated.
2. Do NOT rename `pdfce-gui` / `pdfce_gui` / `PdfceApp` anywhere — that is
   the removed crate; the live GUI is `pdfcer-gui` and the two must stay
   distinguishable.
3. Do NOT rename `pdfce@cce414e:` citations, `D:\Dev\pdfce-backups\`, the
   channel folder `pdfce_FeatureRequests`, or fixture content/identifiers
   (`pdfceAttach.ttf`, "pdfce fixture RSA signer"). The deletion-collateral
   sidecar fixtures were regenerated under `/pdfcer` when the compat layer
   was removed.
4. The prior hand-off's negatives (redaction cells are paper not zero;
   `W n` stays; CMS signed attributes hash as `SET OF`; `adbe.pkcs7.sha1`
   digests `SHA1(D)`; no RustCrypto stack for verification) still stand —
   see `ROADMAP.md` Passes 245.0–246.1 and 10.1.

## §6 ITEMS OWED BY THE OPERATOR

- Global `CLAUDE.md`'s `D:\Dev\pdfce\` references (§0).
- Open questions `(ca)`, `(cb)`, `(cc)` — unchanged.
