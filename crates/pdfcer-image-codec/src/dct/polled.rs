//! A `zune-jpeg` input that answers a cancel poll from inside the entropy
//! decoder.
//!
//! `zune-jpeg` 0.5.15 has no row or MCU callback, but with its `std`
//! feature it reads any `BufRead + Seek` input, and its bit reader pulls
//! the codestream a few bytes at a time: a four-byte `read` on the fast
//! path, else single-byte `read`s each followed by an end-of-stream check
//! (`fill_buf`). Failing those calls once the poll says "cancelled" ends
//! the scan at the next refill. In non-strict mode the decoder treats the
//! error as a damaged stream, fills the undecoded rows with grey and
//! returns `Ok`; [`super::decode`] then sees the poll set and discards the
//! buffer, so the grey never leaves the codec.

use std::io::{self, BufRead, Read, Seek, SeekFrom};

/// Reads between polls. A baseline scan refills four bytes per call, so
/// 1024 reads is ~4 KiB of entropy-coded data, well under a millisecond
/// of decode.
const READS_PER_POLL: u32 = 1024;

/// An in-memory reader whose reads fail, permanently, once `cancelled`
/// answers `true`.
pub(super) struct PolledCursor<'a> {
    inner: io::Cursor<&'a [u8]>,
    cancelled: &'a dyn Fn() -> bool,
    countdown: u32,
    tripped: bool,
}

impl<'a> PolledCursor<'a> {
    /// A cursor over `data` that asks `cancelled` every [`READS_PER_POLL`] reads.
    pub(super) fn new(data: &'a [u8], cancelled: &'a dyn Fn() -> bool) -> Self {
        Self {
            inner: io::Cursor::new(data),
            cancelled,
            countdown: READS_PER_POLL,
            tripped: false,
        }
    }

    /// Count one read; every [`READS_PER_POLL`]th asks the poll.
    #[inline(always)]
    fn tick(&mut self) -> io::Result<()> {
        self.countdown -= 1;
        if self.countdown == 0 {
            self.countdown = READS_PER_POLL;
            self.tripped = self.tripped || (self.cancelled)();
        }
        if self.tripped {
            return Err(io::Error::other("decode cancelled by the caller"));
        }
        Ok(())
    }
}

impl Read for PolledCursor<'_> {
    #[inline(always)]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.tick()?;
        self.inner.read(buf)
    }
}

impl BufRead for PolledCursor<'_> {
    #[inline(always)]
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.tick()?;
        self.inner.fill_buf()
    }

    #[inline(always)]
    fn consume(&mut self, amount: usize) {
        self.inner.consume(amount);
    }
}

impl Seek for PolledCursor<'_> {
    #[inline(always)]
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test assertions
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn reads_fail_for_good_once_the_poll_fires() {
        let data = [7u8; 4096];
        let flag = Cell::new(false);
        let poll = || flag.get();
        let mut c = PolledCursor::new(&data, &poll);
        let mut b = [0u8; 1];
        for _ in 0..10 {
            c.read_exact(&mut b).unwrap();
        }
        flag.set(true);
        let mut failed_at = None;
        for i in 0..2 * READS_PER_POLL {
            if c.read(&mut b).is_err() {
                failed_at = Some(i);
                break;
            }
        }
        assert!(failed_at.is_some_and(|i| i < READS_PER_POLL));
        flag.set(false);
        assert!(c.read(&mut b).is_err() && c.fill_buf().is_err());
    }

    #[test]
    fn the_sample_buffer_fill_stops_once_the_poll_fires() {
        let calls = Cell::new(0u32);
        let poll = || {
            calls.set(calls.get() + 1);
            calls.get() >= 2
        };
        assert!(matches!(
            crate::zeroed_polled(64 << 20, &poll),
            Err(crate::ImageCodecError::Cancelled)
        ));
        assert_eq!(calls.get(), 2, "stops at the poll that fired");
        assert_eq!(
            crate::zeroed_polled(9 << 20, &|| false).unwrap(),
            vec![0; 9 << 20]
        );
    }
}
