---
name: only-an-out-of-crate-test-feels-a-consumers-constraints
description: Unit tests inside the crate cannot see `#[non_exhaustive]`, private-field, or visibility walls — write one integration test per new public spec type or the consuming project finds the wall first
metadata:
  type: feedback
---

**A test inside the crate cannot exercise the constraints a consumer lives
under.** For any new `pub` type a shell is meant to construct, write at least
one test in `crates/<crate>/tests/` — not in a `mod tests` — before calling
the API done.

**Why:** 2026-08-26, `Pass 134.0`. I added `FieldEdit` and `WidgetEdit` as
`#[non_exhaustive]` partial-update structs, which is the right choice (they
will grow, and a struct literal in `pdfceGUI` would break every time one
does). In-crate code compiled fine. The moment I wrote the integration test —
which is out-of-crate by construction — every struct literal failed:
`#[non_exhaustive]` **blocks construction entirely from outside the defining
crate**, and `Default` does not rescue it, because `Foo { ..Default::default() }`
is still a struct expression.

So the API as first written was *unconstructible by its only consumer*, and
nothing inside the crate could have told me. The fix was builders
(`new()` + chainable `with_*`), which is the convention `RenderOptions` and
the five `New*` specs already follow — I simply had not been forced to notice.

**The general shape:** `#[non_exhaustive]`, private fields, `pub(crate)`
re-exports, sealed traits and `#[doc(hidden)]` are all invisible to a test
that lives beside the code. They are the whole experience of the person
downstream.

**How to apply:** when a Pass adds a public type a shell constructs, the
acceptance criteria include one out-of-crate test that *constructs it the way
the shell will*. In this project that is `crates/pdfce-core/tests/` for
`pdfce-core` and `crates/pdfce-cli/tests/` for the binary contract. It costs
one file and it is the only mechanism that makes the consumer's constraints
compile-checked rather than reported back weeks later through
`D:\Dev\FeatureRequests\`.

**★ SECOND INSTANCE, 2026-08-28, `Pass 151.0` — and I made it again knowing
the first one.** `ResizeOptions` shipped `#[non_exhaustive]` with three `pub`
flags and no builders. Fourteen in-crate tests exercised every combination and
passed. The out-of-crate integration test refused to compile on the first line
that tried to construct it.

Two things the repeat teaches that the first instance did not:

- **Knowing the rule did not prevent it.** `#[non_exhaustive]` is *correct* on
  an options struct, so the attribute goes on by reflex and the builders do
  not, because nothing in the crate ever asks for them. The trigger is not
  "remember the rule", it is **write the out-of-crate test first, or at least
  before you believe the API is done.**
- **A second-order version bites the OTHER way.** `#[non_exhaustive]` on the
  *outcome* enum forces every consumer to write a `_` arm — and the
  compulsory, obvious form of that arm, `_ => {}`, is precisely how a future
  variant ships as **silence**. Write those arms to speak: report an
  unrecognised variant *as* unrecognised. The attribute that protects the
  consumer's build creates a disclosure hole in the same stroke.

Related: [[project_gui_request_channel]] — the channel is where this failure
arrives if the test does not catch it first.

**★ TWICE MORE IN ONE SESSION, 2026-08-29, and both were MISSING
CONSTRUCTORS rather than a blocked literal in a test.**

`Pass 171.0` and `172.0` added `PageClip` and `OutlineClip`, both
`#[non_exhaustive]`. The integration tests would not compile — and the fix was
not "make the test build it another way", it was **that the public API had a
hole**:

- **`PageClip`**: nothing outside the crate could turn a clip *file* back into
  a clip. A clipboard whose payload can be written and never read is not a
  clipboard. → `from_bytes` / `to_bytes`, with the counts **re-derived from
  the bytes** rather than taken on trust.
- **`OutlineClip`**: no way to express *"the clipboard holds nothing"*. →
  `empty()`.
- **`AttachmentClip`**: the same hole, spotted in advance this time. →
  `new(..)`, which also turns out to be the honest way to implement *"attach
  the file the operator just dropped on the window"* — it is a paste.

**The generalisation worth carrying: every `#[non_exhaustive]` type a
consumer is expected to HOLD needs at least one constructor a consumer can
reach.** `#[non_exhaustive]` is right (the invariants are the crate's), but it
converts "no constructor" from a mild inconvenience into a hard wall, and the
wall is invisible from inside the crate.

Ask it at design time, per type: *how does a shell that has bytes, or has
nothing, get one of these?*
