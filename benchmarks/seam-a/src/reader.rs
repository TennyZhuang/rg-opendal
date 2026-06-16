//! Simple full-buffer `Read` adapter used by the Seam A bench harness.
//!
//! The streaming counterpart (`StreamingBufReader`) is imported from the
//! promoted `rg-opendal` crate so the bench and the scaffold share one
//! implementation.

use std::io::{self, Read};

/// Full-buffer sync `Read` over an in-memory byte buffer.
/// Same pattern as the scaffold's `rg_opendal::opendal_io::BufReader`;
/// kept local in the bench crate for measurement isolation of Seam C.
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
    fn bufreader_basic() {
        let mut r = BufReader::new(b"hello world".to_vec());
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"hello world");
    }

    #[test]
    fn bufreader_chunked() {
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
}
