# Security Policy

pdfcer parses PDF files by design from sources the user doesn't
control — email attachments, downloads, scans. Every PDF is untrusted,
potentially adversarial input. See `docs/ARCHITECTURE.md` §10 for the
hardening this implies at the design level (resource-limit guards,
fuzz-testing); this file is the disclosure process for when something
slips through anyway.

## Supported versions

Not applicable yet — pdfcer is pre-1.0 with **no tagged release**. Only
the current `main` is supported, and there is nothing older to support.
This section gets a real version-support table once there is a first
release.

Note that `docs/FEATURES.md` is kept true at HEAD rather than at
release time, so it may describe capabilities that no tag contains.

## Reporting a vulnerability

**Do not open a public GitHub issue for a security vulnerability.**
Until a dedicated security-contact channel exists (tracked as a
pre-first-release TODO — GitHub's private vulnerability reporting
feature, once this repository has a public GitHub presence, is the
likely mechanism), report privately to the project maintainer through
a GitHub security advisory on this repository (Security -> Report a vulnerability), or a private issue if that is unavailable.

When reporting, please include:

- The specific PDF file (or a minimal reproduction) that triggers the
  issue, if the report involves a crash, hang, or resource exhaustion.
  **If the triggering file contains sensitive/real-world content, say
  so** — don't attach it to any public-facing report; see
  `docs/LEGAL.md` §5 for why real-world files need careful handling
  even for bug reports.
- Whether the issue is a crash, a hang (possible DoS via decompression
  bomb or unbounded recursion — see `docs/ARCHITECTURE.md` §10), a
  memory-safety issue (unlikely given Rust, but `unsafe` blocks are
  possible in dependencies), or a logic bug with security implications
  (e.g. a redaction that doesn't actually remove content — see
  `docs/ARCHITECTURE.md` §5 corollary, this is treated as a
  **security-severity** bug, not a cosmetic one, given what redaction
  is used for).
- Rust version, OS, and pdfcer version/commit.

## Severity framing specific to this project

- **Redaction that leaves content recoverable** (the saved file still
  contains what the user asked to permanently remove) is treated as a
  **critical** severity issue, on par with a memory-safety
  vulnerability — the entire point of that feature is that removal is
  real, and a silent failure there is a trust-destroying, potentially
  legally-consequential bug for whoever used it. See
  `docs/ARCHITECTURE.md` §5.
- **A crafted PDF causing a hang, unbounded memory growth, or a crash**
  is treated as a real vulnerability (denial of service), not "just a
  parser bug" — this is exactly the adversarial-input class
  `docs/ARCHITECTURE.md` §10 exists to guard against, and any instance
  that gets through is evidence a guard is missing or wrong.
- **Digital-signature verification incorrectly reporting a tampered
  document as validly signed** is critical severity — the inverse
  (correctly-signed reported as invalid) is a correctness bug, not a
  security one, but still worth reporting.

## Disclosure timeline

No formal SLA published yet (pre-release project, single/small
maintainer team). Reasonable-effort acknowledgment and fix timeline
will be communicated directly in response to a report. This section
should be revisited and firmed up before the first public release.
