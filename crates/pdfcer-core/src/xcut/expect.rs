//! [`Expect`], [`Change`] and [`Disclosure`]: what one matrix cell asserts.

use std::fmt;

/// What an operation must do to one kind of object: one matrix cell.
///
/// `Display` writes the cell grammar of `docs/cross-cutting.md`:
/// `ok[:changes]`, `remove`, `consume`, `represent`, `refused:<cause>`,
/// `n/a:"<reason>"` or `pass:<id>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    /// The object's property dump after the operation equals the dump
    /// before, except for exactly these declared changes.
    Preserve(&'static [Change]),
    /// The object is removed and these keys' values are absent from every
    /// byte of the saved file (redact, sanitize).
    Remove(&'static [&'static str]),
    /// The object is consumed into page content (flatten); with
    /// `render_equal`, the page render must not change.
    Consume {
        /// Whether the page render before and after must be equal.
        render_equal: bool,
    },
    /// The kind appears in the operation's output (export, print, search).
    Represent,
    /// The operation refuses with a named error or a disclosure; a silent
    /// no-op fails.
    Refused(&'static str),
    /// The operation cannot apply; the reason cites a spec clause or a
    /// decision.
    NotApplicable(&'static str),
    /// Not yet driven: the filed Pass that will drive it.
    Pending(&'static str),
}

impl Expect {
    /// Whether the gate test drives this cell (everything but
    /// [`Self::NotApplicable`] and [`Self::Pending`]).
    #[must_use]
    pub const fn is_driven(&self) -> bool {
        !matches!(self, Self::NotApplicable(_) | Self::Pending(_))
    }
}

impl fmt::Display for Expect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preserve([]) => f.write_str("ok"),
            Self::Preserve(changes) => {
                f.write_str("ok:")?;
                for (i, c) in changes.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{c}")?;
                }
                Ok(())
            }
            Self::Remove(_) => f.write_str("remove"),
            Self::Consume { .. } => f.write_str("consume"),
            Self::Represent => f.write_str("represent"),
            Self::Refused(cause) => write!(f, "refused:{cause}"),
            Self::NotApplicable(reason) => write!(f, "n/a:\"{reason}\""),
            Self::Pending(pass) => write!(f, "pass:{pass}"),
        }
    }
}

/// One declared difference between the dump before and after a
/// [`Expect::Preserve`] operation. Anything not declared must be equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Change {
    /// The key's numbers move by exactly the operation's offset (x at even
    /// positions, y at odd), within 1e-9.
    Translated(&'static str),
    /// `/P` or `/Parent`: must resolve to the destination's page or parent.
    Rebound(&'static str),
    /// The key is regenerated: present, and unique among its siblings
    /// (`/NM`, ISO 32000-1 §12.5.2 Table 164).
    Regenerated(&'static str),
    /// The key is removed; passes only if `disclosed_by` counted it.
    Dropped {
        /// The removed key.
        key: &'static str,
        /// The report counter that must be above zero.
        disclosed_by: Disclosure,
        /// The filed Pass that will carry it instead, if any.
        gap: Option<&'static str>,
    },
    /// The key is re-derived (an `/AP`); only when `only_when` holds.
    Rebaked {
        /// The re-derived key.
        key: &'static str,
        /// The condition that justifies re-deriving it.
        only_when: &'static str,
    },
    /// Object numbers change; the dump is reference-blind, so this is
    /// always allowed and recorded only for the matrix.
    Renumbered,
    /// The value is carried verbatim but not registered in the destination
    /// (an `/OC` naming an OCG the destination's `/OCProperties` lacks);
    /// `pass` fixes the registration.
    Unbound {
        /// The carried key.
        key: &'static str,
        /// The filed Pass that registers it.
        pass: &'static str,
    },
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Translated(k) => write!(f, "translated({k})"),
            Self::Rebound(k) => write!(f, "rebound({k})"),
            Self::Regenerated(k) => write!(f, "regenerated({k})"),
            Self::Dropped {
                key,
                gap: Some(gap),
                ..
            } => write!(f, "dropped({key}->{gap})"),
            Self::Dropped { key, gap: None, .. } => write!(f, "dropped({key})"),
            Self::Rebaked { key, .. } => write!(f, "rebaked({key})"),
            Self::Renumbered => f.write_str("renumbered"),
            Self::Unbound { key, pass } => write!(f, "unbound({key}->{pass})"),
        }
    }
}

/// The report counter a [`Change::Dropped`] key must be counted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Disclosure {
    /// `PasteOutcome::dropped_keys`, keyed by the key's name.
    PasteDroppedKeys,
    /// `PasteOutcome::replies_unthreaded`.
    PasteRepliesUnthreaded,
    /// `FieldPasteOutcome::dropped_keys`, keyed by the key's name.
    FieldPasteDroppedKeys,
}
