//! The primary hint stream (F.4): the page offset table, the shared object
//! table and the outline table. Every column is byte-aligned, as qpdf
//! writes and reads it.

use std::collections::BTreeMap;

/// Where each output object is in the file written without the hint
/// stream, and how many bytes it spans: number -> (offset, length).
pub(super) type Spans = BTreeMap<u32, (u64, u64)>;

/// What the hint tables describe, by output object number.
pub(super) struct Tables {
    /// Per page, its objects: page 1's are part 6, later pages' a part-7
    /// group. Consecutive numbers, the page object first.
    pub pages: Vec<Vec<u32>>,
    /// Per page, indexes into `shared` (empty for page 1, F.4.1).
    pub page_shared: Vec<Vec<u32>>,
    /// The shared object table's entries: part 6, then part 8.
    pub shared: Vec<u32>,
    /// How many of `shared` are part 6.
    pub first_page_shared: usize,
    /// The outline dictionary and the count of its contiguous run.
    pub outlines: Option<(u32, usize)>,
}

/// The decoded hint stream, the offset of its shared object table (`/S`)
/// and of its outline table (`/O`).
pub(super) fn encode(tables: &Tables, spans: &Spans) -> (Vec<u8>, usize, Option<usize>) {
    let mut w = Bits::default();
    page_offsets(&mut w, tables, spans);
    let s = w.out.len();
    shared_objects(&mut w, tables, spans);
    let o = tables.outlines.map(|(first, count)| {
        let at = w.out.len();
        let (offset, _) = spans.get(&first).copied().unwrap_or_default();
        w.put(u64::from(first), 32);
        w.put(offset, 32);
        w.put(count as u64, 32);
        w.put(run_length(spans, first, count), 32);
        at
    });
    (w.out, s, o)
}

fn run_length(spans: &Spans, first: u32, count: usize) -> u64 {
    (0..count)
        .filter_map(|i| u32::try_from(i).ok())
        .filter_map(|i| spans.get(&first.saturating_add(i)))
        .map(|&(_, len)| len)
        .sum()
}

fn group_length(spans: &Spans, group: &[u32]) -> u64 {
    group
        .iter()
        .filter_map(|n| spans.get(n))
        .map(|&(_, len)| len)
        .sum()
}

/// Table F.3 header then F.4 columns.
fn page_offsets(w: &mut Bits, t: &Tables, spans: &Spans) {
    let counts: Vec<u64> = t.pages.iter().map(|g| g.len() as u64).collect();
    let lengths: Vec<u64> = t.pages.iter().map(|g| group_length(spans, g)).collect();
    let nshared: Vec<u64> = t.page_shared.iter().map(|s| s.len() as u64).collect();
    let (min_n, bits_n) = range(&counts);
    let (min_len, bits_len) = range(&lengths);
    let first_offset = t
        .pages
        .first()
        .and_then(|g| g.first())
        .and_then(|n| spans.get(n))
        .map_or(0, |&(offset, _)| offset);
    let bits_nshared = nbits(nshared.iter().copied().max().unwrap_or(0));
    let bits_id = nbits(t.shared.len() as u64);
    for (value, width) in [
        (min_n, 32),
        (first_offset, 32),
        (bits_n, 16),
        (min_len, 32),
        (bits_len, 16),
        (0, 32),
        (0, 16),
        (min_len, 32),
        (bits_len, 16),
        (bits_nshared, 16),
        (bits_id, 16),
        (0, 16),
        (4, 16),
    ] {
        w.put(value, width);
    }
    w.column(counts.iter().map(|c| c - min_n), bits_n);
    w.column(lengths.iter().map(|l| l - min_len), bits_len);
    w.column(nshared.iter().copied(), bits_nshared);
    w.column(
        t.page_shared.iter().flatten().map(|&i| u64::from(i)),
        bits_id,
    );
    w.column(lengths.iter().map(|l| l - min_len), bits_len);
}

/// Table F.5 header then F.6 columns: one single-object group per entry.
fn shared_objects(w: &mut Bits, t: &Tables, spans: &Spans) {
    let lengths: Vec<u64> = t
        .shared
        .iter()
        .map(|n| spans.get(n).map_or(0, |&(_, len)| len))
        .collect();
    let (min_len, bits_len) = range(&lengths);
    let later = t.shared.get(t.first_page_shared).copied();
    let later_offset = later.and_then(|n| spans.get(&n)).map_or(0, |&(o, _)| o);
    w.put(later.map_or(0, u64::from), 32);
    w.put(later_offset, 32);
    w.put(t.first_page_shared as u64, 32);
    w.put(t.shared.len() as u64, 32);
    w.put(0, 16);
    w.put(min_len, 32);
    w.put(bits_len, 16);
    w.column(lengths.iter().map(|l| l - min_len), bits_len);
    w.column(lengths.iter().map(|_| 0), 1);
}

/// The minimum and the bit width of the largest difference from it.
fn range(values: &[u64]) -> (u64, u64) {
    let min = values.iter().copied().min().unwrap_or(0);
    let max = values.iter().copied().max().unwrap_or(0);
    (min, nbits(max - min))
}

fn nbits(x: u64) -> u64 {
    u64::from(64 - x.leading_zeros())
}

/// Big-endian bit packing (F.4: "the most significant bit first").
#[derive(Default)]
struct Bits {
    out: Vec<u8>,
    acc: u8,
    used: u32,
}

impl Bits {
    fn put(&mut self, value: u64, width: u64) {
        for bit in (0..width).rev() {
            let one = bit < 64 && (value >> bit) & 1 == 1;
            self.acc = (self.acc << 1) | u8::from(one);
            self.used += 1;
            if self.used == 8 {
                self.out.push(self.acc);
                self.acc = 0;
                self.used = 0;
            }
        }
    }

    /// One table column, padded to a byte boundary.
    fn column(&mut self, values: impl Iterator<Item = u64>, width: u64) {
        for v in values {
            self.put(v, width);
        }
        if self.used > 0 {
            self.out.push(self.acc << (8 - self.used));
            self.acc = 0;
            self.used = 0;
        }
    }
}
