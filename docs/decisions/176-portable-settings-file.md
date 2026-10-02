# Decision 176 — A portable settings file beside the CLI, opt-in OS font folders

- **Date:** 2026-10-02
- **Status:** DECIDED; implemented in `Pass 436.1` (cli).
- **Authored by:** `pdfcer-engineer` (defaults chosen per the Pass brief).
- **Amends:** the `render-page` "no system fonts are discovered" default
  (R19, decision 004) — by opt-in only. With no settings file the default is
  unchanged.
- **Boundary:** the CLI reads files; `pdfcer-core` and `pdfcer-render` stay
  filesystem-free (decision 012, the same shell/core split as decision 135).
- **Code:** `crates/pdfcer-cli/src/settings.rs`; registration in
  `inspect.rs::build_font_environment`; flags on `cli.rs::Cli`.

## 1. Where the file comes from

Precedence, first match wins:

1. `--no-settings` — no file is read. Output depends on the command line alone.
2. `--settings PATH` — this file. A missing or unreadable file exits 3
   (I/O error).
3. `pdfcer-settings.txt` beside the executable, if it exists. No file means
   the defaults apply, and output is byte-identical to a build without
   settings support.

Passing both flags is a usage error (exit 2). Beside the executable keeps the
single-folder portable package self-contained: no registry, no profile
directory, nothing written.

## 2. Format

UTF-8, at most 64 KiB, an optional BOM, one `key = value` per line. `#` and
`;` start comment lines. A value may be wrapped in double quotes.

| Key | Values | Default |
|---|---|---|
| `workarounds` | `always` / `offer` | `offer` |
| `system_fonts` | `on` / `off` | `off` |
| `font_folder` | a folder; repeatable; relative resolves beside the settings file; `~/` expands | none |
| `font_file_limit` | 1 … 1,000,000 | 10,000 |
| `ocr_folder` | a folder of OCR model add-ons; repeatable; resolved like `font_folder`; searched after `models/` beside the executable (decision 182) | none |

These are fatal and exit 1, naming the line: an unknown key, a bad value, an
empty value, a repeated scalar key, or a line with no `=`. The reason is that
a silently ignored typo (`workaround = always`) would be a forgotten setting
that changes nothing while the operator believes it does.

There is no new dependency. The format is a hand-written parser, which is
easier to audit than a TOML crate for five keys.

## 3. Disclosure (batch determinism)

Whenever a file is read, one stderr line names it and every value:
`pdfcer: settings: using PATH: workarounds=… system_fonts=… font_folders=N
font_file_limit=N ocr_folders=N (pass --no-settings to ignore it)`. A forgotten file can
therefore never change output silently (rule 4, fuzzy-never-sneaky). The
font walk adds its own `pdfcer: settings:` lines when it runs:

- the number of files and folders found,
- each folder skipped for depth,
- the point where the walk stopped at the ceiling.

## 4. Font folders

- **Searched folders:** the OS folders when `system_fonts = on`, then the
  folders named by `font_folder` in file order (so a named folder's face wins
  over an OS face of the same name).
  - Windows: `%WINDIR%\Fonts` and `%LOCALAPPDATA%\Microsoft\Windows\Fonts`.
  - macOS: `/System/Library/Fonts`, `/Library/Fonts` and `~/Library/Fonts`.
  - Other Unix: `/usr/share/fonts`, `/usr/local/share/fonts`,
    `~/.local/share/fonts` and `~/.fonts`.

  A named folder that is missing is noted. A missing OS folder is skipped
  quietly.
- **Walk:** recursive and sorted, with a canonical-path cycle guard. The
  limits (ARCHITECTURE §10):
  - depth 8 below each root (deeper folders are skipped and named);
  - `font_file_limit` font-extension files, counted per folder;
  - 20,000 folders visited in total.

  The limit is checked a whole folder at a time. The folder that would pass
  it, and every folder after it, is not searched, and that is reported. A
  partly read folder would make which face wins depend on directory order.
- **Lazy:** the walk runs on first use by a font-consuming command, so
  `info` and `merge` pay nothing for `system_fonts = on`.
- **Registration:** through the existing `build_font_environment`, so the
  same name and filename-stem lookup applies; there is no second lookup.
  Settings folders register first, and `--font-dir` folders register after
  them. Last registration wins, so an explicit `--font-dir` face beats a
  settings face of the same name. Settings registrations are summarised in
  one note, because an OS font folder holds thousands of files.

## 5. `workarounds`

The CLI parses, validates, stores and discloses this key, and exposes it as
`settings::workarounds()`. Its consumer, the retype workaround, is `Pass
436.0`. `offer` is the default, because a workaround that rewrites content is
something the operator asks for rather than something that happens silently.

## 6. Not fed by font folders

- `--fallback-font NAME` resolves only a page resource or a Standard-14 name
  in core (`FallbackFace::Named`). Settings faces cannot be named there. The
  matching ladder in `Pass 436.2` is where that belongs.
- `format-text --embed-styled-face` walks `--font-dir` itself
  (`style_donor_plans`) and still requires `--font-dir`.
- `embed-font` labels a face found through settings as `source: --font-dir
  face`, because the label predates settings.
