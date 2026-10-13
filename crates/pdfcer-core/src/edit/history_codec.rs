//! The byte codec under [`EditSession::export_history`]: LEB128 integers,
//! length-prefixed byte strings and a tagged COS [`Object`] encoding.
//!
//! The reader treats its input as untrusted: every count is checked against
//! the bytes left before anything is allocated, nesting is bounded by
//! [`MAX_DEPTH`], and every failure is a [`Malformed`] carrying the offset.
//!
//! [`EditSession::export_history`]: super::EditSession::export_history

use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::span::ByteSpan;

/// Deepest array/dictionary nesting the reader accepts (ARCHITECTURE.md §10).
pub(super) const MAX_DEPTH: usize = 256;

/// A decode failure at a byte offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Malformed {
    pub(super) offset: usize,
}

pub(super) type Res<T> = Result<T, Malformed>;

const NULL: u8 = 0;
const FALSE: u8 = 1;
const TRUE: u8 = 2;
const INTEGER: u8 = 3;
const REAL: u8 = 4;
const STRING: u8 = 5;
const NAME: u8 = 6;
const ARRAY: u8 = 7;
const DICT: u8 = 8;
const STREAM: u8 = 9;
const REFERENCE: u8 = 10;

/// Appends encoded values to a buffer.
#[derive(Debug, Default)]
pub(super) struct Writer {
    pub(super) out: Vec<u8>,
}

impl Writer {
    /// One raw byte.
    pub(super) fn u8(&mut self, v: u8) {
        self.out.push(v);
    }

    /// Unsigned LEB128.
    pub(super) fn uint(&mut self, mut v: u64) {
        loop {
            let low = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                self.out.push(low);
                return;
            }
            self.out.push(low | 0x80);
        }
    }

    /// A length or offset, as [`Self::uint`].
    pub(super) fn usize(&mut self, v: usize) {
        self.uint(v as u64);
    }

    /// Zig-zag, so small negatives stay short.
    pub(super) fn int(&mut self, v: i64) {
        self.uint(((v << 1) ^ (v >> 63)) as u64);
    }

    /// `0` or `1`.
    pub(super) fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    /// Length-prefixed bytes.
    pub(super) fn bytes(&mut self, v: &[u8]) {
        self.usize(v.len());
        self.out.extend_from_slice(v);
    }

    /// Object number, then generation.
    pub(super) fn id(&mut self, id: ObjId) {
        self.uint(u64::from(id.num));
        self.uint(u64::from(id.generation));
    }

    /// Entry count, then each key and value in order.
    pub(super) fn dict(&mut self, d: &Dict) {
        self.usize(d.0.len());
        for (k, v) in &d.0 {
            self.bytes(&k.0);
            self.object(v);
        }
    }

    /// A presence flag, then the object.
    pub(super) fn opt_object(&mut self, o: Option<&Object>) {
        self.bool(o.is_some());
        if let Some(o) = o {
            self.object(o);
        }
    }

    /// A tag byte, then the value; a stream keeps its dictionary and data span.
    pub(super) fn object(&mut self, o: &Object) {
        match o {
            Object::Null => self.u8(NULL),
            Object::Boolean(b) => self.u8(if *b { TRUE } else { FALSE }),
            Object::Integer(i) => {
                self.u8(INTEGER);
                self.int(*i);
            }
            Object::Real(r) => {
                self.u8(REAL);
                self.out.extend_from_slice(&r.to_bits().to_le_bytes());
            }
            Object::String(s) => {
                self.u8(STRING);
                self.bytes(s);
            }
            Object::Name(n) => {
                self.u8(NAME);
                self.bytes(&n.0);
            }
            Object::Array(items) => {
                self.u8(ARRAY);
                self.usize(items.len());
                for item in items {
                    self.object(item);
                }
            }
            Object::Dict(d) => {
                self.u8(DICT);
                self.dict(d);
            }
            Object::Stream(s) => {
                self.u8(STREAM);
                self.dict(&s.dict);
                self.usize(s.data_span.start);
                self.usize(s.data_span.len);
            }
            Object::Reference(id) => {
                self.u8(REFERENCE);
                self.id(*id);
            }
        }
    }
}

/// Reads values back, refusing anything a [`Writer`] could not have written.
#[derive(Debug)]
pub(super) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    /// Every stream's data must lie below this offset (base plus staging).
    pub(super) span_limit: usize,
}

impl<'a> Reader<'a> {
    /// A reader at the start of `buf`, accepting no stream spans until `span_limit` is set.
    pub(super) fn new(buf: &'a [u8]) -> Self {
        Self {
            buf,
            pos: 0,
            span_limit: 0,
        }
    }

    /// A [`Malformed`] at the current offset.
    pub(super) fn fail<T>(&self) -> Res<T> {
        Err(Malformed { offset: self.pos })
    }

    /// Whether every byte was consumed.
    pub(super) fn at_end(&self) -> bool {
        self.pos == self.buf.len()
    }

    /// The next `n` bytes, or [`Malformed`] past the end.
    pub(super) fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.buf.len());
        let Some(end) = end else {
            return self.fail();
        };
        let s = self
            .buf
            .get(self.pos..end)
            .ok_or(Malformed { offset: self.pos })?;
        self.pos = end;
        Ok(s)
    }

    /// One raw byte.
    pub(super) fn u8(&mut self) -> Res<u8> {
        Ok(self.take(1)?.first().copied().unwrap_or(0))
    }

    /// Unsigned LEB128 of at most ten bytes.
    pub(super) fn uint(&mut self) -> Res<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.u8()?;
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        self.fail()
    }

    /// A [`Self::uint`] that fits `usize`.
    pub(super) fn usize(&mut self) -> Res<usize> {
        let v = self.uint()?;
        usize::try_from(v).or_else(|_| self.fail())
    }

    /// A count of items each at least one byte long: never more than the
    /// bytes left, so a hostile count cannot drive an allocation.
    pub(super) fn count(&mut self) -> Res<usize> {
        let n = self.usize()?;
        if n > self.buf.len() - self.pos {
            return self.fail();
        }
        Ok(n)
    }

    /// A zig-zag integer.
    pub(super) fn int(&mut self) -> Res<i64> {
        let z = self.uint()?;
        Ok(((z >> 1) as i64) ^ -((z & 1) as i64))
    }

    /// Exactly `0` or `1`.
    pub(super) fn bool(&mut self) -> Res<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => self.fail(),
        }
    }

    /// Length-prefixed bytes.
    pub(super) fn bytes(&mut self) -> Res<Vec<u8>> {
        let n = self.usize()?;
        Ok(self.take(n)?.to_vec())
    }

    /// An object id whose number fits `u32` and generation `u16`.
    pub(super) fn id(&mut self) -> Res<ObjId> {
        let num = u32::try_from(self.uint()?).or_else(|_| self.fail())?;
        let generation = u16::try_from(self.uint()?).or_else(|_| self.fail())?;
        Ok(ObjId { num, generation })
    }

    /// A dictionary whose values nest one level below `depth`.
    pub(super) fn dict(&mut self, depth: usize) -> Res<Dict> {
        let n = self.count()?;
        let mut entries = Vec::with_capacity(n);
        for _ in 0..n {
            let key = Name(self.bytes()?);
            entries.push((key, self.object_at(depth)?));
        }
        Ok(Dict(entries))
    }

    /// A presence flag, then an object.
    pub(super) fn opt_object(&mut self) -> Res<Option<Object>> {
        if self.bool()? {
            Ok(Some(self.object_at(0)?))
        } else {
            Ok(None)
        }
    }

    /// One top-level object.
    pub(super) fn object(&mut self) -> Res<Object> {
        self.object_at(0)
    }

    fn object_at(&mut self, depth: usize) -> Res<Object> {
        if depth > MAX_DEPTH {
            return self.fail();
        }
        let tag = self.u8()?;
        Ok(match tag {
            NULL => Object::Null,
            FALSE => Object::Boolean(false),
            TRUE => Object::Boolean(true),
            INTEGER => Object::Integer(self.int()?),
            REAL => {
                let raw = self.take(8)?;
                let mut b = [0u8; 8];
                b.copy_from_slice(raw);
                Object::Real(f64::from_bits(u64::from_le_bytes(b)))
            }
            STRING => Object::String(self.bytes()?),
            NAME => Object::Name(Name(self.bytes()?)),
            ARRAY => {
                let n = self.count()?;
                let mut items = Vec::with_capacity(n);
                for _ in 0..n {
                    items.push(self.object_at(depth + 1)?);
                }
                Object::Array(items)
            }
            DICT => Object::Dict(self.dict(depth + 1)?),
            STREAM => {
                let dict = self.dict(depth + 1)?;
                let start = self.usize()?;
                let len = self.usize()?;
                if start.checked_add(len).is_none_or(|e| e > self.span_limit) {
                    return self.fail();
                }
                Object::Stream(Stream {
                    dict,
                    data_span: ByteSpan { start, len },
                })
            }
            REFERENCE => Object::Reference(self.id()?),
            _ => return self.fail(),
        })
    }
}
