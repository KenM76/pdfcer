//! Myers' O(ND) difference algorithm in linear space, over interned tokens.
//!
//! Recursion is replaced by an explicit task stack, and the work is bounded
//! by a step budget: a range whose middle snake cannot be found within the
//! budget is emitted as one delete-plus-insert, and the caller is told.

use std::ops::Range;

/// One edit-script operation over token indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Op {
    /// `len` tokens equal, starting at `old` and `new`.
    Equal { old: usize, new: usize, len: usize },
    /// `old` tokens present only in the old sequence.
    Delete { old: Range<usize> },
    /// `new` tokens present only in the new sequence.
    Insert { new: Range<usize> },
}

/// The edit script and whether any range fell back to a plain replace.
#[derive(Debug, Default)]
pub(crate) struct Script {
    pub(crate) ops: Vec<Op>,
    pub(crate) truncated: bool,
}

enum Task {
    Diff(Range<usize>, Range<usize>),
    Emit(Op),
}

/// `s[r]`, or empty when `r` is out of bounds. Every range built here is in
/// bounds; the checked form keeps a logic slip from becoming a panic on
/// untrusted input (crate lint policy).
fn sub(s: &[u32], r: Range<usize>) -> &[u32] {
    s.get(r).unwrap_or(&[])
}

/// Diff `a` against `b`, spending at most `budget` inner-loop steps.
pub(crate) fn diff(a: &[u32], b: &[u32], budget: u64) -> Script {
    let d_max = (a.len() + b.len()).div_ceil(2) + 1;
    let mut v = Frontier::new(d_max);
    let mut steps = budget;
    let mut script = Script::default();
    let mut stack = vec![Task::Diff(0..a.len(), 0..b.len())];
    while let Some(task) = stack.pop() {
        let (old, new) = match task {
            Task::Emit(op) => {
                push_op(&mut script.ops, op);
                continue;
            }
            Task::Diff(o, n) => (o, n),
        };
        let pre = common_prefix(sub(a, old.clone()), sub(b, new.clone()));
        if pre > 0 {
            push_op(
                &mut script.ops,
                Op::Equal {
                    old: old.start,
                    new: new.start,
                    len: pre,
                },
            );
        }
        let (o0, n0) = (old.start + pre, new.start + pre);
        let suf = common_suffix(sub(a, o0..old.end), sub(b, n0..new.end));
        let (o1, n1) = (old.end - suf, new.end - suf);
        if suf > 0 {
            stack.push(Task::Emit(Op::Equal {
                old: o1,
                new: n1,
                len: suf,
            }));
        }
        if o0 == o1 || n0 == n1 {
            emit_replace(&mut script.ops, o0..o1, n0..n1);
            continue;
        }
        let split = v.middle_snake(a, o0..o1, b, n0..n1, &mut steps);
        match split {
            Some((x, y)) if (x, y) != (o0, n0) && (x, y) != (o1, n1) => {
                stack.push(Task::Diff(x..o1, y..n1));
                stack.push(Task::Diff(o0..x, n0..y));
            }
            _ => {
                script.truncated |= split.is_none();
                emit_replace(&mut script.ops, o0..o1, n0..n1);
            }
        }
    }
    script
}

fn emit_replace(ops: &mut Vec<Op>, old: Range<usize>, new: Range<usize>) {
    if !old.is_empty() {
        push_op(ops, Op::Delete { old });
    }
    if !new.is_empty() {
        push_op(ops, Op::Insert { new });
    }
}

/// Append `op`, merging it into the previous op of the same kind when the
/// two are contiguous.
fn push_op(ops: &mut Vec<Op>, op: Op) {
    match (ops.last_mut(), &op) {
        (
            Some(Op::Equal { old, len, .. }),
            Op::Equal {
                old: o2, len: l2, ..
            },
        ) if *old + *len == *o2 => {
            *len += l2;
        }
        (Some(Op::Delete { old }), Op::Delete { old: o2 }) if old.end == o2.start => {
            old.end = o2.end;
        }
        (Some(Op::Insert { new }), Op::Insert { new: n2 }) if new.end == n2.start => {
            new.end = n2.end;
        }
        _ => ops.push(op),
    }
}

fn common_prefix(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

fn common_suffix(a: &[u32], b: &[u32]) -> usize {
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

/// The forward and backward furthest-reaching x per diagonal, indexed by
/// `k + offset`. Sized once for the whole input; every sub-range is smaller.
struct Frontier {
    vf: Vec<usize>,
    vb: Vec<usize>,
    offset: isize,
}

/// Which frontier a read or write addresses.
#[derive(Clone, Copy)]
enum Dir {
    Forward,
    Backward,
}

impl Frontier {
    fn new(d_max: usize) -> Self {
        let len = 2 * d_max + 3;
        Self {
            vf: vec![0; len],
            vb: vec![0; len],
            offset: d_max as isize + 1,
        }
    }

    fn slot(&self, k: isize) -> usize {
        usize::try_from(k + self.offset).unwrap_or(usize::MAX)
    }

    fn get(&self, dir: Dir, k: isize) -> usize {
        let v = match dir {
            Dir::Forward => &self.vf,
            Dir::Backward => &self.vb,
        };
        v.get(self.slot(k)).copied().unwrap_or(0)
    }

    fn set(&mut self, dir: Dir, k: isize, x: usize) {
        let i = self.slot(k);
        let v = match dir {
            Dir::Forward => &mut self.vf,
            Dir::Backward => &mut self.vb,
        };
        if let Some(slot) = v.get_mut(i) {
            *slot = x;
        }
    }

    /// The furthest x reachable on diagonal `k` at cost `d`, before the snake.
    fn step_x(&self, dir: Dir, k: isize, d: isize) -> usize {
        let up = self.get(dir, k + 1);
        if k == -d || (k != d && self.get(dir, k - 1) < up) {
            up
        } else {
            self.get(dir, k - 1) + 1
        }
    }

    /// The start of a middle snake of the shortest edit script between the
    /// two (non-empty) ranges, in absolute indices, or `None` once `steps`
    /// runs out.
    fn middle_snake(
        &mut self,
        a: &[u32],
        old: Range<usize>,
        b: &[u32],
        new: Range<usize>,
        steps: &mut u64,
    ) -> Option<(usize, usize)> {
        let (n, m) = (old.len(), new.len());
        let delta = n as isize - m as isize;
        let odd = delta & 1 == 1;
        self.set(Dir::Forward, 1, 0);
        self.set(Dir::Backward, 1, 0);
        let d_max = (n + m).div_ceil(2) as isize;
        let ranges = Ranges { a, old, b, new };
        for d in 0..=d_max {
            *steps = steps.checked_sub(2 * d as u64 + 2)?;
            if let Some(p) = self.forward(&ranges, d, delta, odd) {
                return Some(p);
            }
            if let Some(p) = self.backward(&ranges, d, delta, odd) {
                return Some(p);
            }
        }
        None
    }

    fn forward(
        &mut self,
        r: &Ranges<'_>,
        d: isize,
        delta: isize,
        odd: bool,
    ) -> Option<(usize, usize)> {
        let (n, m) = (r.old.len(), r.new.len());
        let mut k = d;
        while k >= -d {
            let mut x = self.step_x(Dir::Forward, k, d);
            let y0 = usize::try_from(x as isize - k).unwrap_or(usize::MAX);
            let x0 = x;
            if x < n && y0 < m {
                x += common_prefix(
                    sub(r.a, r.old.start + x..r.old.end),
                    sub(r.b, r.new.start + y0..r.new.end),
                );
            }
            self.set(Dir::Forward, k, x);
            if odd && (k - delta).abs() < d && x + self.get(Dir::Backward, delta - k) >= n {
                return Some((r.old.start + x0, r.new.start + y0));
            }
            k -= 2;
        }
        None
    }

    fn backward(
        &mut self,
        r: &Ranges<'_>,
        d: isize,
        delta: isize,
        odd: bool,
    ) -> Option<(usize, usize)> {
        let (n, m) = (r.old.len(), r.new.len());
        let mut k = d;
        while k >= -d {
            let mut x = self.step_x(Dir::Backward, k, d);
            let mut y = usize::try_from(x as isize - k).unwrap_or(usize::MAX);
            if x < n && y < m {
                let s = common_suffix(
                    sub(r.a, r.old.start..r.old.end - x),
                    sub(r.b, r.new.start..r.new.end - y),
                );
                x += s;
                y += s;
            }
            self.set(Dir::Backward, k, x);
            if !odd && (k - delta).abs() <= d && x + self.get(Dir::Forward, delta - k) >= n {
                return Some((
                    r.old.start + n.saturating_sub(x),
                    r.new.start + m.saturating_sub(y),
                ));
            }
            k -= 2;
        }
        None
    }
}

/// The two token sequences and the sub-ranges being split.
struct Ranges<'a> {
    a: &'a [u32],
    old: Range<usize>,
    b: &'a [u32],
    new: Range<usize>,
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // tests index sequences they just built
mod tests {
    use super::*;

    /// Rebuild `b` from `a` and the script, and count edits.
    fn apply(a: &[u32], b: &[u32], s: &Script) -> (Vec<u32>, usize) {
        let mut out = Vec::new();
        let mut edits = 0;
        let (mut oi, mut ni) = (0, 0);
        for op in &s.ops {
            match op {
                Op::Equal { old, new, len } => {
                    assert_eq!((*old, *new), (oi, ni));
                    assert_eq!(a[*old..old + len], b[*new..new + len]);
                    out.extend_from_slice(&a[*old..old + len]);
                    oi += len;
                    ni += len;
                }
                Op::Delete { old } => {
                    assert_eq!(old.start, oi);
                    oi = old.end;
                    edits += old.len();
                }
                Op::Insert { new } => {
                    assert_eq!(new.start, ni);
                    out.extend_from_slice(&b[new.clone()]);
                    ni = new.end;
                    edits += new.len();
                }
            }
        }
        assert_eq!((oi, ni), (a.len(), b.len()));
        (out, edits)
    }

    /// Edit distance (insert/delete only) by dynamic programming.
    fn lcs_distance(a: &[u32], b: &[u32]) -> usize {
        let mut prev = vec![0usize; b.len() + 1];
        for x in a {
            let mut cur = vec![0usize; b.len() + 1];
            for (j, y) in b.iter().enumerate() {
                cur[j + 1] = if x == y {
                    prev[j] + 1
                } else {
                    prev[j + 1].max(cur[j])
                };
            }
            prev = cur;
        }
        a.len() + b.len() - 2 * prev[b.len()]
    }

    #[test]
    fn scripts_are_minimal_and_rebuild_the_new_sequence() {
        // A deterministic pseudo-random sweep over small alphabets, where
        // shortest edit scripts are most ambiguous.
        let mut seed = 0x2545_f491_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for _ in 0..400 {
            let la = (next() % 14) as usize;
            let lb = (next() % 14) as usize;
            let a: Vec<u32> = (0..la).map(|_| next() % 3).collect();
            let b: Vec<u32> = (0..lb).map(|_| next() % 3).collect();
            let s = diff(&a, &b, u64::MAX);
            assert!(!s.truncated);
            let (rebuilt, edits) = apply(&a, &b, &s);
            assert_eq!(rebuilt, b, "a={a:?} b={b:?}");
            assert_eq!(edits, lcs_distance(&a, &b), "a={a:?} b={b:?}");
        }
    }

    #[test]
    fn an_exhausted_budget_falls_back_to_one_replace() {
        let a: Vec<u32> = (0..200).collect();
        let b: Vec<u32> = (0..200)
            .map(|i| if i % 7 == 0 { 1000 + i } else { i })
            .collect();
        let s = diff(&a, &b, 10);
        assert!(s.truncated);
        let (rebuilt, _) = apply(&a, &b, &s);
        assert_eq!(rebuilt, b);
    }

    #[test]
    fn identical_and_empty_inputs() {
        assert_eq!(
            diff(&[1, 2], &[1, 2], 0).ops,
            vec![Op::Equal {
                old: 0,
                new: 0,
                len: 2
            }]
        );
        assert_eq!(diff(&[], &[5], 0).ops, vec![Op::Insert { new: 0..1 }]);
        assert!(diff(&[], &[], 0).ops.is_empty());
    }
}
