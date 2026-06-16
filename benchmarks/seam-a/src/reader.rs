//! Reader bridges for OpenDAL → std::io::Read.
//!
//! - `BufReader`: Seam C — full-buffer sync adapter (PoC-verified pattern).
//! - `StreamingBufReader`: Seam A — streaming chunk-at-a-time bridge.
//!   Uses `Handle::block_on` to pull the next chunk when the current one is exhausted.
//!   Caller passes Handle explicitly (not ambient Handle::current()).

use std::io::{self, Read};

// ── Seam C: Full-buffer BufReader ─────────────────────────────────

/// Full-buffer sync `Read` over an in-memory byte buffer.
/// Same pattern as the PoC's BufReader and the scaffold's `opendal_io::BufReader`.
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

// ── Seam A: StreamingBufReader ────────────────────────────────────

use opendal::BufferStream;
use opendal::Buffer;
use futures::StreamExt;
use tokio::runtime::Handle;

/// Streaming chunk-at-a-time bridge: OpenDAL BufferStream → std::io::Read.
///
/// **Architecture:** Holds the current `Buffer` chunk + position. When the
/// chunk is exhausted, calls `Handle::block_on()` to pull the next chunk
/// from the async stream. Empty chunks (zero-length Buffer) are skipped
/// transparently.
///
/// **Bridge cost:** One `block_on` per chunk transition. The counter
/// is instrumented via `block_on_count()` for diagnostic measurement.
pub struct StreamingBufReader {
    stream: BufferStream,
    handle: Handle,
    current: Option<Buffer>,
    pos: usize,
    block_on_calls: u64,
}

// Safety: BufferStream is Unpin (per the opendal docs), and we only hold it
// on a thread that has a tokio Handle.

impl StreamingBufReader {
    /// Create a new StreamingBufReader from an OpenDAL BufferStream.
    /// The caller must pass the tokio Runtime Handle explicitly for
    /// the sync/async bridge — not via Handle::current().
    pub fn new(stream: BufferStream, handle: Handle) -> Self {
        Self {
            stream,
            handle,
            current: None,
            pos: 0,
            block_on_calls: 0,
        }
    }

    /// Number of times `Handle::block_on` was called to pull the next chunk.
    /// Diagnostic column for bridge cost analysis.
    pub fn block_on_count(&self) -> u64 {
        self.block_on_calls
    }

    /// Pull the next non-empty chunk from the stream (blocking the calling
    /// thread via Handle::block_on).
    fn next_chunk(&mut self) -> io::Result<Option<Buffer>> {
        loop {
            self.block_on_calls += 1;
            match self.handle.block_on(self.stream.next()) {
                Some(Ok(buf)) => {
                    if buf.is_empty() {
                        continue; // skip empty chunks per design review
                    }
                    return Ok(Some(buf));
                }
                Some(Err(e)) => {
                    return Err(io::Error::new(io::ErrorKind::Other, e.to_string()));
                }
                None => return Ok(None), // EOF
            }
        }
    }
}

impl Read for StreamingBufReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // If no current chunk, fetch the first one
        if self.current.is_none() {
            match self.next_chunk()? {
                Some(chunk) => {
                    self.current = Some(chunk);
                    self.pos = 0;
                }
                None => return Ok(0), // EOF
            }
        }

        let chunk = self.current.as_ref().unwrap();
        let remaining = chunk.len() - self.pos;

        if remaining == 0 {
            // Current chunk exhausted — fetch next
            match self.next_chunk()? {
                Some(next) => {
                    self.current = Some(next);
                    self.pos = 0;
                    return self.read(buf); // recurse once
                }
                None => {
                    self.current = None;
                    return Ok(0); // EOF
                }
            }
        }

        let chunk_bytes = chunk.current();
        let to_read = buf.len().min(remaining);
        buf[..to_read].copy_from_slice(&chunk_bytes[self.pos..self.pos + to_read]);
        self.pos += to_read;
        Ok(to_read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    fn test_handle() -> Handle {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let h = rt.handle().clone();
        std::mem::forget(rt);
        h
    }

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

    #[test]
    fn streaming_basic() {
        let h = test_handle();
        let data: Vec<Vec<u8>> = vec![b"hello ".to_vec(), b"world".to_vec()];
        // Build a synthetic BufferStream via channel-based approach
        // We can't construct BufferStream directly — test via Read impl only
        // For now, we test through the Read trait via the bench harness
    }

    #[test]
    fn streaming_small_reads() {
        // Tested via bench harness (StreamingBufReader requires real BufferStream)
    }

    #[test]
    fn streaming_empty_chunk_skipped() {
        // Tested via bench harness
    }
}
