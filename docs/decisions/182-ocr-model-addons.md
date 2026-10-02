# Decision 182 — OCR models as drop-in add-on folders

- **Date:** 2026-10-02
- **Status:** DECIDED; shipped in `Pass 442.0`.
- **Authored by:** `pdfcer-engineer`.
- **Trigger:** the operator: *"make it so that new OCR models are simple to
  install by just dropping a new folder with the configured model files under
  our existing OCR folder … We should be able to set up multiple locations to
  look for OCR files like we can with fonts. To uninstall a user should just
  have to delete the folder."*
- **Amends:** decision 176 (adds the `ocr_folder` settings key).
- **Out of scope:** what the portable package ships (`Pass 442.1`); how
  Tesseract is executed (unchanged).

## 1. The manifest: `pdfcer-ocr-model.txt`

Same grammar as `pdfcer-settings.txt` (decision 176): UTF-8, at most 16 KiB,
optional BOM, one `key = value` per line, `#`/`;` comment lines, a value may
be wrapped in double quotes.

```text
# pdfcer OCR model add-on manifest
name = ppocrv4-ch-en
engine = paddle
label = PP-OCRv4 Chinese + English
languages = zh, en
version = 4
licence = Apache-2.0
sha256 = det.onnx d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9
sha256 = rec.onnx 48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b
```

| Key | Rule | Required |
|---|---|---|
| `name` | `[A-Za-z0-9._-]`, 1–64, starts alphanumeric; the `--ocr-model` id | yes |
| `engine` | `[a-z0-9_-]`, 1–32; `ocrs`, `ocrcer`, `paddle`, `tesseract`, or a future token | yes |
| `label` | ≤ 200 chars | no |
| `languages` | comma-separated tags, each ≤ 64 | no |
| `version` | ≤ 64 | no |
| `licence` (alias `license`) | ≤ 64 | no |
| `sha256` | `RELPATH HEX64`; repeatable; path `/`-separated, relative, no `..`/`.`/empty component, `\` or `:` | no |

A malformed line, a repeated scalar key, a missing `name`/`engine` or a file
hashed twice skips the folder, and the reason is printed. **An unknown key is
kept and disclosed, not refused.** That differs from the settings file, on
purpose: a manifest is written by an add-on's author for builds that may be
older than it. A typo in the operator's own settings file is the operator's
error; a newer key in a downloaded manifest is not.

Control characters are refused in every value, because values are echoed to
the terminal.

## 2. Discovery

Roots, in priority order:
1. `models/` beside the executable;
2. each `ocr_folder` line in the settings file, in file order;
3. each `--ocr-folder`, in the order given.

Core takes the roots as arguments and never reads settings
(`pdfcer_core::ocr::addons::discover_ocr_models`; not compiled for wasm32).

Inside a root:
- A root that itself holds a manifest is one model.
- A folder with a manifest is a model, and is not descended into.
- A folder without a manifest, directly under a root and named `ocrs`,
  `ocrcer`, `paddle` or `tesseract`, is a model named after its engine. This
  is the legacy layout, so existing installs keep working.
- Other folders are descended into, to depth 3.

Guards (`ARCHITECTURE.md` §10): a canonical-path cycle guard; 2,000 folders
visited; 256 models.

**A name found twice keeps the first, and the shadowing is printed.**
Uninstalling is deleting the folder; no state is kept anywhere else.

## 3. Why bundled models come before add-ons

Root 1 has the highest priority, so `--ocr-engine paddle` with no other
flags means what it meant before any add-on was installed. To use an add-on
for an engine that also ships, name it with `--ocr-model`. Under the
opposite order, dropping a folder into a settings-named root would silently
change the output of every existing batch script. A model that is not
explicitly chosen is an inference, and rule 4 forbids a silent one.

## 4. Selection and verification

- `--ocr-engine E` picks the first model whose engine is `E` and that holds
  `E`'s required files. Incomplete folders are passed over, and each one is
  named.
- `--ocr-model NAME` picks a model by name; its engine comes from the
  manifest. A contradicting `--ocr-engine` is refused. An engine this
  pdfcer has never heard of exits 64.
- `--model-dir` is unchanged and bypasses discovery. If that folder holds a
  manifest, the manifest's engine must match.
- Every `sha256` line is checked on load, before the engine sees a byte. A
  mismatch is refused, naming the file, both digests and the remedy
  (reinstall, or delete the folder). Files the manifest does not list are
  not hashed.
- The chosen add-on's name, label, licence and languages are printed.

## 5. `pdfcer ocr-models`

Lists every model, one line each on stdout:

```text
ocr-model NAME engine=E in-build=yes|no|unknown-engine label="…" languages=a,b licence=L version=V folder="…"
```

`--verify` adds `verified=N` or `verified=FAILED` (and exits 1 on a
failure). Folders searched, skipped folders, bad manifests and shadowing go
to stderr. The command reads folder listings and manifests only, and makes
no network call.

## 6. Shipped paddle folder

`crates/pdfcer-core/assets/models/paddle/pdfcer-ocr-model.txt` is the
example above. Packaging copies every file in an asset folder, so it ships
without a packaging change.
