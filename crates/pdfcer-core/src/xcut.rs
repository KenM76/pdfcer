//! # Cross-cutting coverage: every object kind against every operation
//!
//! The contract of decision 228 (`docs/decisions/228-cross-cutting-coverage.md`):
//! every operation pdfcer offers must say, for every kind of object a document
//! can hold, what it does to that object — and a gate test proves it.
//!
//! - [`ObjectKind`] is the one list of kinds. It is deliberately **not**
//!   `#[non_exhaustive]`: adding a kind breaks the build in every policy
//!   table below, in the fixture builder and in any consumer's own exhaustive
//!   match. That breakage is the feature.
//! - [`Op`] is the one list of operations.
//! - [`expect`] is the policy: for each `(Op, ObjectKind)` cell, an
//!   [`Expect`] stating what must hold, built from one exhaustive match per
//!   operation group with no wildcard arm.
//! - Carrying an object (copy, cut, paste, duplicate) clones its COS
//!   dictionary and closure, then strips or patches a declared key list; it
//!   never rebuilds the dictionary from an allowlist. The crate-internal
//!   `carry::clone_then_patch` is that primitive.
//!
//! `docs/cross-cutting.md` is generated from [`expect`]: one cell per
//! `(kind, op)`, in the grammar [`Expect`]'s `Display` writes.

mod expect;
mod kind;
mod op;
mod policy;

pub(crate) mod carry;
// Only the gate test reads dumps until the 782.x columns drive them.
#[cfg(test)]
pub(crate) mod dump;

pub use expect::{Change, Disclosure, Expect};
pub use kind::{KindGroup, ObjectKind, classify_annot};
pub use op::{Op, OpGroup};
pub use policy::expect;

#[cfg(test)]
mod tests;
