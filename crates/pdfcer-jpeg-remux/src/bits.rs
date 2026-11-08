//! Entropy-coded segment bit I/O with T.81 F.1.2.3 byte stuffing.

use crate::RemuxError;

/// Reads bits MSB-first from an entropy-coded segment.
///
/// A `0xFF 0x00` pair is one `0xFF` data byte. Any other marker, or the end
/// of the data, stops the input: further reads return zero bits, as T.81
/// decoders conventionally do, and the marker stays unconsumed for
/// [`BitReader::restart`].
pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u64,
    nbits: u32,
    stopped: bool,
}

impl<'a> BitReader<'a> {
    /// A reader positioned at the first byte of `data`.
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            acc: 0,
            nbits: 0,
            stopped: false,
        }
    }

    fn fill(&mut self) {
        while self.nbits <= 56 {
            let byte = if self.stopped {
                0
            } else {
                match (self.data.get(self.pos), self.data.get(self.pos + 1)) {
                    (Some(&0xFF), Some(&0x00)) => {
                        self.pos += 2;
                        0xFF
                    }
                    (Some(&0xFF), _) | (None, _) => {
                        self.stopped = true;
                        0
                    }
                    (Some(&b), _) => {
                        self.pos += 1;
                        b
                    }
                }
            };
            self.acc |= u64::from(byte) << (56 - self.nbits);
            self.nbits += 8;
        }
    }

    /// The next `n` bits (`n <= 16`) as an unsigned value.
    pub(crate) fn bits(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        if self.nbits < n {
            self.fill();
        }
        let v = (self.acc >> (64 - n)) as u32;
        self.acc <<= n;
        self.nbits -= n;
        v
    }

    /// The next single bit.
    pub(crate) fn bit(&mut self) -> u32 {
        self.bits(1)
    }

    /// Discards the partial byte and consumes the `RSTm` marker that must
    /// follow (T.81 F.1.2.3, E.1.4).
    pub(crate) fn restart(&mut self) -> Result<(), RemuxError> {
        self.acc = 0;
        self.nbits = 0;
        self.stopped = false;
        while self.data.get(self.pos) == Some(&0xFF) && self.data.get(self.pos + 1) == Some(&0xFF) {
            self.pos += 1;
        }
        match (self.data.get(self.pos), self.data.get(self.pos + 1)) {
            (Some(&0xFF), Some(&(0xD0..=0xD7))) => {
                self.pos += 2;
                Ok(())
            }
            _ => Err(RemuxError::Malformed("missing restart marker")),
        }
    }
}

/// Writes bits MSB-first, stuffing a `0x00` after every `0xFF` byte.
#[derive(Default)]
pub(crate) struct BitWriter {
    out: Vec<u8>,
    acc: u32,
    nbits: u32,
}

impl BitWriter {
    /// Appends the low `n` bits of `value` (`n <= 16`).
    pub(crate) fn put(&mut self, value: u32, n: u32) {
        if n == 0 {
            return;
        }
        self.acc = (self.acc << n) | (value & ((1 << n) - 1));
        self.nbits += n;
        while self.nbits >= 8 {
            self.nbits -= 8;
            let byte = (self.acc >> self.nbits) as u8;
            self.out.push(byte);
            if byte == 0xFF {
                self.out.push(0x00);
            }
        }
        self.acc &= (1 << self.nbits) - 1;
    }

    /// Pads the last byte with 1-bits (T.81 F.1.2.3) and returns the bytes.
    pub(crate) fn finish(mut self) -> Vec<u8> {
        if self.nbits > 0 {
            let pad = 8 - self.nbits;
            self.put((1 << pad) - 1, pad);
        }
        self.out
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_written_stream_reads_back_with_its_stuffing_removed() {
        let mut w = BitWriter::default();
        w.put(0xFF, 8);
        w.put(0b101, 3);
        w.put(0x1234, 16);
        let bytes = w.finish();
        assert_eq!(&bytes[..2], &[0xFF, 0x00]);
        let mut r = BitReader::new(&bytes);
        assert_eq!(r.bits(8), 0xFF);
        assert_eq!(r.bits(3), 0b101);
        assert_eq!(r.bits(16), 0x1234);
    }

    #[test]
    fn a_marker_stops_the_input_and_restart_consumes_it() {
        let data = [0xAB, 0xFF, 0xD3, 0xCD];
        let mut r = BitReader::new(&data);
        assert_eq!(r.bits(8), 0xAB);
        assert_eq!(r.bits(8), 0, "reads past a marker are zero");
        r.restart().unwrap();
        assert_eq!(r.bits(8), 0xCD);
        assert!(BitReader::new(&[0x00]).restart().is_err());
    }
}
