# Decision 198: zune-jpeg rc3 is taken from git, not the registry

Amends decision 197. Recorded at `Pass 630.0` (`32e2bff1`).

## Decision

- `zune-jpeg` in `pdfcer-image-codec`, `pdfcer-3d` (optional) and
  `pdfcer-render` (dev) is `git = "https://github.com/etemesi254/zune-image",
  rev = "c1c7f49776ee32928ed329f4436ae63a595b6283"`. `zune-core` comes from the
  same rev.
- The rc3 code is unchanged; 197's decoder fix and regression test stand.
- Revisit when stable 0.5.16 is on crates.io: move to a registry
  `version = "0.5.16"`. That is still a blocker for any crates.io publish
  (a git dependency blocks `cargo package`).

## Why

197's exact registry pin `=0.5.16-rc3` made `pdfcer-gui` unresolvable:
eframe -> egui-winit -> arboard -> image/tiff 0.11.3 requires zune-jpeg
`^0.5.1`, a caret never matches a pre-release, and Cargo allows one
semver-compatible version per source. A git source is a distinct package, so
the registry 0.5.15 (tiff's) and git 0.5.16-rc3 (pdfcer's) coexist. Measured
with a scratch crate on `pdfcer-image-codec` + `tiff =0.11.3`.

## Rev choice

The crates.io rc3 tarball's `.cargo_vcs_info.json` sha `b71f4180...` is not on
the GitHub remote. `c1c7f497` ("jpeg: make a release", dev branch) has
`crates/zune-jpeg/src` identical to the tarball except one unused
`use core::cmp::min;`.

## Relation to 197

197 rejected "`[patch]` to a git commit" as harder to audit than an immutable
rc and as blocking publishing. This is a dependency-level git source, not a
`[patch]`, chosen because the registry pin could not be consumed by a
downstream that also has a registry `^0.5` dependency. It does not make
publishing any harder than 197 already did (pre-release already blocked it).
Auditability is by the rev hash, which is immutable.

## Known gap

`tools/*/Cargo.lock` still pin the registry zune-jpeg; re-resolving adds about
9k lines, so they were left alone.
