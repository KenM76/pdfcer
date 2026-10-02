# Decision 184 — Program OCR add-ons; Tesseract is the first

- **Date:** 2026-10-02
- **Status:** DECIDED; shipped in `Pass 442.3`.
- **Authored by:** `pdfcer-engineer`.
- **Trigger:** the operator's ruling (2026-10-02): an OCR add-on folder may
  carry an executable engine that pdfcer runs. Tesseract installs by
  dropping a folder, as the data models do. Uninstalling is deleting the
  folder. Every model in the OCR folders must be runnable from pdfcer-gui's
  model drop-down.
- **Rule:** R262.
- **Amends:** decision 182. A bare `tesseract` folder is no longer a
  model, and the manifest gains `kind`, `program` and `data`.
- **Leaves standing:** R13 (never execute anything fetched without a
  ruling) for everything that is not an OCR engine add-on. This decision
  is that ruling for OCR program add-ons only.

## 1. Manifest keys

| Key | Rule |
|---|---|
| `kind` | `data` (default) or `program` |
| `program` | A bare file name in the add-on folder. 1–64 characters; no `/`, `\`, `:`, `.`, `..` or control characters. Required with `kind = program`; refused otherwise. |
| `data` | A relative folder (same path rules as `sha256` paths) passed to the program. Optional, `kind = program` only. Tesseract defaults to `tessdata`. |

```text
name = tesseract
engine = tesseract
kind = program
program = tesseract.exe
data = tessdata
licence = Apache-2.0
sha256 = tesseract.exe <hex>
sha256 = tessdata/eng.traineddata <hex>
```

A program add-on without a `sha256` line for its program still **parses
and lists**, but it is never run. The listing says `runnable=no`, and the
reason names the missing line. Requiring the hash is a run-time rule, not a
parse rule, so the operator can see the add-on and is told what is wrong.

## 2. What runs, and when

`pdfcer_ocr_host::program_status` checks these in order:
1. not a program;
2. refused by policy;
3. no protocol for the engine;
4. no program hash;
5. program file missing.

Only `tesseract` has a protocol today (PGM on stdin, TSV on stdout). Other
engine tokens are refused by name.

- **Before each run** (each page), every file the manifest hashes is opened,
  hashed and compared. A mismatch refuses that run and names the file and
  both digests. The same check also runs at load.
- **Exactly the named file** is started, with `std::process::Command`, no
  shell. Its arguments are fixed by the protocol, plus the data folder
  passed explicitly (`--tessdata-dir`) and `-l` checked against
  `[a-z_]+` joined by `+`.
- On Windows the child gets `CREATE_NO_WINDOW`. Its stdout is capped at
  64 MiB.
- Files the manifest does not hash (for example a `.traineddata` added
  later) are used but not verified. `tools/tesseract/write-ocr-manifest.py`
  re-hashes the folder.

## 3. The hash-to-spawn window (TOCTOU)

- **Windows: closed.** Each hashed file is opened with `FILE_SHARE_READ`
  only, hashed from that handle, and the handles are held until the child
  exits. No other process can write, rename or delete those files between
  the check and the spawn. `CreateProcess` still loads an image that is held
  this way; this was verified with the real Tesseract bundle.
- **Elsewhere: open, and accepted.** Unix has no mandatory share modes. A
  process that can write the add-on folder can swap the program after it is
  hashed. Such a process can already replace `pdfcer` itself, so the check
  defends against accidental change and partial copies, not against a local
  attacker with write access. The module doc of
  `crates/pdfcer-ocr-host/src/program.rs` says so.

## 4. Policy: `ocr_program_addons = allow|refuse`

The settings key defaults to `allow`, as the operator ruled.

- `refuse` still lists program add-ons, as `runnable=no` with the reason
  "running OCR programs is turned off". It never starts a process.
- The CLI flag `--refuse-ocr-programs` (on `ocr` and `ocr-models`) does the
  same.
- The flag and the setting combine to the stricter of the two; neither can
  loosen the other.
- Engines that run inside pdfcer are unaffected.
- `refuse` also refuses a stock folder named by `--model-dir` (§6).

## 5. Disclosure (rule 4)

- `ocr-models` adds `kind=data|program`, `program="FILE"` for a program, and
  `runnable=yes|no` after `in-build=`. The fields are appended, so existing
  prefixes still match. A `runnable=no` reason goes to stderr.
- `ocr` prints the program it starts, from which add-on, and how many files
  are re-checked before each page.

## 6. A stock install named with `--model-dir`

`--model-dir "C:\Program Files\Tesseract-OCR"` keeps working. The folder has
no manifest, so the add-on search never finds it. The operator named the
folder, so pdfcer runs `tesseract.exe` there as named: unhashed, and the run
line says *"it has no manifest, so nothing is hashed"*. If that folder does
hold a manifest, it is treated as the add-on it describes.

## 7. Packaging

- `tools/tesseract/write-ocr-manifest.py` writes the manifest: name
  `tesseract`, so `--ocr-model tesseract` keeps its meaning, with hashes for
  the exe and every `tessdata/*.traineddata`.
- `build-tesseract.py` runs it on `target/tesseract-bundle`.
- `package-portable.py` runs it again on the staged `models/tesseract`, so
  the shipped hashes are of the shipped bytes.
- Nothing binary and no hash is committed.

## 8. Where the runner lives: a new crate, `pdfcer-ocr-host`

- `pdfcer-core` must not spawn processes (wasm32). The CLI is a binary that
  the GUI cannot depend on. `pdfcer-print` is printing-only, and
  `pdfcer-fetch` is network-only, deliberately (decision 061).
- So the runner moved from `pdfcer-cli/src/tesseract.rs` into a new library
  crate. It has no dependency beyond `pdfcer-core` and `thiserror`; std
  only, plus `cfg(windows)` share modes.
- **One call for every model kind:** `OcrRunner::load(model, &RunOptions)`
  then `recognize(w, h, grey)`. `check_runnable(model, policy)` answers "can
  this run, and if not why" for a drop-down without loading anything.
- The CLI uses `ProgramEngine` for Tesseract and keeps its own in-process
  arms. Moving the CLI fully onto `OcrRunner` is a follow-up, so that every
  OCR engine has one place to be added.
- A new in-process engine must be added to `OcrRunner` as well as to the CLI
  until then.

## 9. Not decided here

- Executing any fetched program other than an OCR program add-on (R13
  stands).
- Program protocols for engines other than Tesseract.
- Signature-based trust (who signed the add-on). Hashes prove the files
  match their manifest; they do not prove who wrote the manifest. A dropped
  folder is trusted the way a dropped font is: the operator put it there.
