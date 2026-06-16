//! Bridges OpenDAL byte sources to `std::io::Read`.
//!
//! Two paths:
//! - `BufReader`: full-buffer (Seam C). Reuses the PoC's verified-correct
//!   pattern — `copy_from_slice` + `min(remaining)`, no UB.
//! - `StreamingBufReader`: stub for Seam A. Real implementation lives in
//!   the bench harness (Pi's `seam_a_bridge.rs`); we re-stub here so the
//!   independent crate's interface is stable while the streaming track
//!   matures separately.

use std::io::{self, Read};

/// Full-buffer sync `Read` over an OpenDAL `Buffer::to_vec()`.
/// PoC equivalent: `OpenDalSyncReader` (main.rs) + `BufReader` (bin/walk).
/// Consolidated to a single name per `2e3964da`.
pub struct BufReader {
    data: Vec<u8>,
    pos: usize,
}

impl BufReader {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }
}

impl Read for BufReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self.data.len() - self.pos;
        let to_read = buf.len().min(remaining);
        buf[..to_read].copy_from_slice(&self.data[self.pos..self.pos + to_read]);
        self.pos += to_read;
        Ok(to_read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_all_bytes() {
        let mut r = BufReader::new(b"hello world".to_vec());
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"hello world");
    }

    #[test]
    fn reads_in_chunks() {
        let mut r = BufReader::new(b"abcdef".to_vec());
        let mut buf = [0u8; 2];
        assert_eq!(r.read(&mut buf).unwrap(), 2);
        assert_eq!(&buf, b"ab");
        assert_eq!(r.read(&mut buf).unwrap(), 2);
        assert_eq!(&buf, b"cd");
        assert_eq!(r.read(&mut buf).unwrap(), 2);
        assert_eq!(&buf, b"ef");
        assert_eq!(r.read(&mut buf).unwrap(), 0);
    }

    #[test]
    fn empty_buffer_returns_zero() {
        let mut r = BufReader::new(Vec::new());
        let mut buf = [0u8; 8];
        assert_eq!(r.read(&mut buf).unwrap(), 0);
    }
}
