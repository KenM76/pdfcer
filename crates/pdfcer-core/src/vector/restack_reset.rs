//! Rebuilding the graphics state an `/ExtGState` controls when an object
//! moves to a position whose `gs` history is not an ancestor of its own.
//!
//! The destination's extra `gs` operations are undone by one new
//! `/ExtGState` restoring, for each parameter they set, the value in force at
//! the object's own position: the last `gs` that set it in the shared
//! history, else its page-start value (ISO 32000-2 §8.4.5 Table 57; Tables
//! 51 and 52; `/Default` for `BG2`/`UCR2`/`TR2`/`HT` means "the value at the
//! start of the page"). A parameter with a device-dependent initial value
//! (`SM`, `HTO`, a font) and no earlier setting cannot be restored, and the
//! position is refused.

use std::collections::HashSet;

use super::restack_state::{Frame, KEYS, Key, OpRef, Walk};
use crate::object::{Dict, Name, Object};

/// The parameters one `gs` node's dictionary sets, by normalised key; `None`
/// when its name did not resolve.
pub(super) type GsParams = Option<Vec<(&'static [u8], Object)>>;

/// Normalise an `/ExtGState` to one entry per parameter: `BG`/`BG2`,
/// `UCR`/`UCR2` and `TR`/`TR2` collapse onto the `2` key (which wins within
/// a dictionary), and `OP` also sets `op` unless the dictionary has one.
pub(super) fn params(d: &Dict) -> Vec<(&'static [u8], Object)> {
    let mut out: Vec<(&'static [u8], Object)> = Vec::new();
    let mut set = |k: &'static [u8], v: &Object| {
        out.retain(|(o, _)| *o != k);
        out.push((k, v.clone()));
    };
    for (k, v) in d.iter() {
        let key: Option<&'static [u8]> = match k.as_bytes() {
            b"BG" => d.get(b"BG2").is_none().then_some(b"BG2"),
            b"UCR" => d.get(b"UCR2").is_none().then_some(b"UCR2"),
            b"TR" => d.get(b"TR2").is_none().then_some(b"TR2"),
            other => PARAMS.iter().copied().find(|p| *p == other),
        };
        if let Some(key) = key {
            set(key, v);
        }
        if k.as_bytes() == b"OP" && d.get(b"op").is_none() {
            set(b"op", v);
        }
    }
    out
}

const PARAMS: [&[u8]; 24] = [
    b"LW",
    b"LC",
    b"LJ",
    b"ML",
    b"D",
    b"RI",
    b"OP",
    b"op",
    b"OPM",
    b"Font",
    b"BG2",
    b"UCR2",
    b"TR2",
    b"HT",
    b"FL",
    b"SM",
    b"SA",
    b"BM",
    b"SMask",
    b"CA",
    b"ca",
    b"AIS",
    b"TK",
    b"UseBlackPtComp",
];

/// The page-start value of a parameter no operator mirrors, or `None` when
/// it is device-dependent with no name for it.
fn initial(p: &[u8]) -> Option<Object> {
    let name = |n: &[u8]| Some(Object::Name(Name(n.to_vec())));
    match p {
        b"OP" | b"op" | b"SA" | b"AIS" => Some(Object::Boolean(false)),
        b"TK" => Some(Object::Boolean(true)),
        b"OPM" => Some(Object::Integer(0)),
        b"CA" | b"ca" => Some(Object::Real(1.0)),
        b"BM" => name(b"Normal"),
        b"SMask" => name(b"None"),
        b"BG2" | b"UCR2" | b"TR2" | b"HT" | b"UseBlackPtComp" => name(b"Default"),
        _ => None,
    }
}

/// Where the value of a parameter at some position came from.
enum Source {
    /// A `gs` in the history the destination shares.
    Shared(Object, u32, super::geometry::Matrix),
    /// A `gs` the object's wrapper replays anyway.
    Replayed(u32),
    /// Never set.
    Initial,
}

impl Source {
    const fn seq(&self) -> Option<u32> {
        match self {
            Self::Shared(_, seq, _) | Self::Replayed(seq) => Some(*seq),
            Self::Initial => None,
        }
    }
}

/// The `gs` history, its resolved dictionaries, and the frames compared.
pub(super) struct Restore<'a> {
    walk: &'a Walk,
    gs: &'a [GsParams],
    s: &'a Frame,
    d: &'a Frame,
    /// The newest `gs` node both histories share, and its ancestors.
    lca: u32,
    shared: HashSet<u32>,
}

/// What the wrapper must add for the `/ExtGState`-settable parameters.
#[derive(Default)]
pub(super) struct Restored {
    /// Entries of the new `/ExtGState`; empty when none is needed.
    pub reset: Dict,
    /// Operators to replay, by their original position.
    pub ops: Vec<OpRef>,
    /// Reset operators (`1 w`) to emit before them.
    pub lits: Vec<&'static [u8]>,
}

impl<'a> Restore<'a> {
    /// Compare object start state `s` with destination state `d`; `None` on
    /// a broken `gs` history.
    pub(super) fn new(
        walk: &'a Walk,
        gs: &'a [GsParams],
        s: &'a Frame,
        d: &'a Frame,
    ) -> Option<Self> {
        let lca = walk.lca(s.gs, d.gs);
        let shared = walk.gs_nodes_since(lca, 0)?.into_iter().collect();
        Some(Self {
            walk,
            gs,
            s,
            d,
            lca,
            shared,
        })
    }

    /// The `gs` operations the object's history has beyond the shared one,
    /// to replay after the reset.
    pub(super) fn own_extras(&self) -> Option<Vec<(OpRef, super::geometry::Matrix)>> {
        self.walk.gs_since(self.s.gs, self.lca)
    }

    /// `relevant` filters the operator-mirrored parameters by what the
    /// object reads. `None`: a value cannot be restored.
    pub(super) fn run(&self, relevant: impl Fn(Key) -> bool) -> Option<Restored> {
        let d_set = self.destination_params()?;
        let mut out = Restored::default();
        for (i, key) in KEYS.iter().enumerate() {
            let Some(p) = key.gs_key() else { continue };
            if !relevant(*key) {
                continue;
            }
            let want = self.s.keys.get(i).copied().flatten();
            let have = self.d.keys.get(i).copied().flatten();
            let differs = want != have || d_set.contains(p);
            let src = self.source(p)?;
            // The operator is in force when no `gs` set the parameter after it.
            if let Some(op) = want
                && src.seq().is_none_or(|s| op.seq > s)
            {
                if differs {
                    out.ops.push(op);
                }
                continue;
            }
            if !differs {
                continue;
            }
            match src {
                Source::Replayed(_) => {}
                Source::Shared(v, ..) => out.reset.insert(Name(p.to_vec()), v),
                Source::Initial => out.lits.push(key.reset()?),
            }
        }
        for p in PARAMS {
            if KEYS.iter().any(|k| k.gs_key() == Some(p)) || !d_set.contains(p) {
                continue;
            }
            match self.source(p)? {
                Source::Replayed(_) => {}
                Source::Shared(v, _, ctm) => {
                    if p == b"SMask" && matches!(v, Object::Dict(_)) && ctm != self.s.ctm {
                        return None; // the mask would land in a different space
                    }
                    out.reset.insert(Name(p.to_vec()), v);
                }
                Source::Initial => out.reset.insert(Name(p.to_vec()), initial(p)?),
            }
        }
        // `OP` alone would also set `op`.
        if out.reset.get(b"OP").is_some() && out.reset.get(b"op").is_none() {
            let v = match self.source(b"op")? {
                Source::Shared(v, ..) => v,
                Source::Replayed(_) | Source::Initial => Object::Boolean(false),
            };
            out.reset.insert(Name(b"op".to_vec()), v);
        }
        Some(out)
    }

    /// The parameters the destination's own `gs` operations set.
    fn destination_params(&self) -> Option<HashSet<&'static [u8]>> {
        let mut out = HashSet::new();
        for node in self.walk.gs_nodes_since(self.d.gs, self.lca)? {
            for (p, _) in self.gs.get(node as usize)?.as_ref()? {
                out.insert(*p);
            }
        }
        Some(out)
    }

    /// The newest `gs` in the object's history that set `p`.
    fn source(&self, p: &[u8]) -> Option<Source> {
        let mut node = self.s.gs;
        while node != 0 {
            let params = self.gs.get(node as usize)?.as_ref()?;
            if let Some((_, v)) = params.iter().find(|(k, _)| *k == p) {
                let seq = self.walk.gs_seq(node);
                return Some(if self.shared.contains(&node) {
                    Source::Shared(v.clone(), seq, self.walk.gs_ctm(node))
                } else {
                    Source::Replayed(seq)
                });
            }
            node = self.walk.gs_parent(node);
        }
        Some(Source::Initial)
    }
}
