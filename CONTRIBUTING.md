# Contributing to pdfcer

Thanks for your interest. A few things to know before opening an issue
or PR — this project has some non-default conventions worth reading
first.

## Project status

Pre-1.0 and moving fast. The Cargo workspace is real — four crates,
a desktop GUI and a 60-subcommand CLI. `docs/FEATURES.md` is the
current answer to what works, and it is kept true at HEAD rather than
at release time, so it may describe capabilities newer than any tag.

External contributions aren't actively solicited: this is a personal
project developed in the open rather than one seeking contributors.
Issues and observations are welcome. A large unsolicited PR is likely
to need rework simply because the architecture is still moving — open
an issue first if you are considering one.

## License — read before contributing code

**pdfcer is MIT-licensed** — `LICENSE` at the repo root, chosen
2026-08-01 (`docs/LEGAL.md` §1). By submitting
a contribution, you agree it's licensed under the terms in `LICENSE`
at the time of merge (the standard "inbound = outbound" convention
most Rust-ecosystem projects use — no separate CLA). A
Developer-Certificate-of-Origin-style sign-off (`git commit -s`,
certifying you have the right to submit the contribution under the
project's license) may be required once the license is set; watch for
that requirement to be added here.

## The documentation is the logic

This project follows a documentation-first discipline: `docs/ARCHITECTURE.md`
is the authoritative design description, `docs/ROADMAP.md` is the
plan/history, `docs/LEGAL.md` covers licensing/IP posture, and
`docs/PRIOR_ART.md` records what existing open-source work informed
which decisions. **Read the relevant doc before proposing a change** —
if your PR contradicts something documented there, the doc needs to
change too (in the same PR), not just the code.

## Two invariants that are not up for casual debate

If a contribution would violate either of these, expect it to need a
strong justification and explicit maintainer sign-off, not just review
comments:

1. **GUI-core separation** (`docs/ARCHITECTURE.md` §3) — `pdfcer-core`
   and `pdfcer-render` must never gain a GUI/windowing dependency.
2. **Round-trip / minimal-diff editing** (`docs/ARCHITECTURE.md` §5) —
   objects the user didn't touch must be re-emitted byte-identical or
   omitted from an incremental save, redaction aside.

## Code style

`cargo fmt` and `cargo clippy -- -D warnings` clean, no exceptions —
enforced by CI (`.github/workflows/ci.yml`) once the workspace exists.
Public API design follows the Rust API Guidelines — see
`docs/ARCHITECTURE.md` §8 for specifics.

## Security-relevant contributions

If your contribution touches parsing, filters, or anything that
handles untrusted PDF input, read `docs/ARCHITECTURE.md` §10
(adversarial input hardening) first — resource-limit guards and
fuzz-testing are requirements, not nice-to-haves, for this kind of
code. See `SECURITY.md` for how to report a vulnerability privately
instead of via a public issue.

## Test fixtures

Never commit a real-world PDF of unknown provenance as a test fixture
— see `docs/LEGAL.md` §5 and `fixtures/README.md` for what's actually
allowed (synthetic files, or files from a corpus with clear
redistribution rights).
