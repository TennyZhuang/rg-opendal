//! Bridges OpenDAL byte sources to `std::io::Read`.
//!
//! Two readers, both implementing `std::io::Read` so the call site at
//! `main.rs::make_reader` can swap them transparently:
//! - `BufReader` — full-buffer (Seam C). Caller `op.read(...)` collects the
//!   whole object, then we read from a `Vec<u8>`. Constant memory ≈ object
//!   size; no async bridge cost.
//! - `StreamingBufReader` — chunked (Seam A). Caller hands us a
//!   `Stream<Item=Result<Buffer>>` from `Reader::into_stream`; we hold one
//!   chunk at a time and `Handle::block_on(stream.next())` on chunk
//!   exhaustion. Memory peak ≈ one chunk; bridge cost is one `block_on` per
//!   chunk transition.
//!
//! Origin of `StreamingBufReader`: ported from the bench-side
//! `seam_a_bridge.rs` (validated end-to-end against minio in matrix v2.6
//! row #3). The matrix-attested behavior is preserved verbatim — same chunk
//! pull mechanics, same empty-chunk handling, same `block_on_calls`
//! diagnostic counter.

use bytes::Buf;
use futures::Stream;
use opendal::Buffer;
use std::io::{self, Read};
use std::pin::Pin;
use tokio::runtime::Handle;

/// Full-buffer sync `Read` over an OpenDAL `Buffer::to_vec()`.
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

/// Bridges an async OpenDAL `BufferStream` to sync `std::io::Read`.
///
/// Holds the current `Buffer` chunk and a position within it. When the
/// buffer is exhausted, blocks on pulling the next chunk from the
/// underlying async stream via `Handle::block_on`.
///
/// The `Handle` must point at a runtime that is *not* the one currently
/// driving the caller — calling `block_on` from inside the same runtime
/// would deadlock. In practice: pass `Handle::current()` from `main`'s
/// `#[tokio::main]` runtime *only* when the search loop runs on a dedicated
/// blocking thread (e.g. via `tokio::task::spawn_blocking`). When the
/// search loop lives directly on the runtime's executor, allocate a
/// separate runtime for the stream and pass its handle here.
///
/// Not yet wired into `main`: the wire-up is a follow-up PR that pairs this
/// type with a `spawn_blocking` boundary. Until that lands, this type is
/// exercised only by its own unit tests.
#[allow(dead_code)]
pub struct StreamingBufReader {
    stream: Pin<Box<dyn Stream<Item = Result<Buffer, opendal::Error>> + Send>>,
    handle: Handle,
    current: Option<Buffer>,
    pos: usize,
    exhausted: bool,
    /// Diagnostic — number of `block_on` calls (chunk transitions) so far.
    /// Stable across releases; matrix v2.6 row #3 uses this counter.
    pub block_on_calls: u64,
}

#[allow(dead_code)]
impl StreamingBufReader {
    pub fn new(
        stream: impl Stream<Item = Result<Buffer, opendal::Error>> + Send + 'static,
        handle: Handle,
    ) -> Self {
        Self {
            stream: Box::pin(stream),
            handle,
            current: None,
            pos: 0,
            exhausted: false,
            block_on_calls: 0,
        }
    }

    fn pull_next(&mut self) -> io::Result<Option<Buffer>> {
        if self.exhausted {
            return Ok(None);
        }
        self.block_on_calls += 1;
        let next = self.handle.block_on(async {
            use futures::StreamExt;
            self.stream.next().await
        });
        match next {
            Some(Ok(buffer)) => Ok(Some(buffer)),
            Some(Err(e)) => {
                self.exhausted = true;
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("OpenDAL stream error: {e}"),
                ))
            }
            None => {
                self.exhausted = true;
                Ok(None)
            }
        }
    }

    fn ensure_buffer(&mut self) -> io::Result<bool> {
        loop {
            if let Some(ref buf) = self.current {
                if self.pos < buf.remaining() {
                    return Ok(true);
                }
            }
            self.current = self.pull_next()?;
            self.pos = 0;
            match &self.current {
                None => return Ok(false),
                Some(buf) if buf.remaining() == 0 => {
                    self.current = None;
                    continue;
                }
                Some(_) => return Ok(true),
            }
        }
    }
}

impl Read for StreamingBufReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if !self.ensure_buffer()? {
            return Ok(0);
        }
        let current = self.current.as_ref().expect("ensure_buffer guarantees Some");
        let remaining = current.remaining() - self.pos;
        let to_read = buf.len().min(remaining);
        let chunk = current.chunk();
        buf[..to_read].copy_from_slice(&chunk[self.pos..self.pos + to_read]);
        self.pos += to_read;
        Ok(to_read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures::stream;

    #[test]
    fn buf_reader_reads_all_bytes() {
        let mut r = BufReader::new(b"hello world".to_vec());
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"hello world");
    }

    #[test]
    fn buf_reader_reads_in_chunks() {
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
    fn buf_reader_empty_buffer_returns_zero() {
        let mut r = BufReader::new(Vec::new());
        let mut buf = [0u8; 8];
        assert_eq!(r.read(&mut buf).unwrap(), 0);
    }

    fn run_streaming(data: Vec<Buffer>, expected: &str) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let handle = rt.handle().clone();
        let s = stream::iter(data.into_iter().map(Ok));
        let mut r = StreamingBufReader::new(s, handle);
        let mut out = String::new();
        r.read_to_string(&mut out).unwrap();
        assert_eq!(out, expected);
    }

    #[test]
    fn streaming_basic_multi_chunk() {
        run_streaming(
            vec![
                Buffer::from(Bytes::from("hello ")),
                Buffer::from(Bytes::from("world")),
            ],
            "hello world",
        );
    }

    #[test]
    fn streaming_small_reads_within_chunk() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let handle = rt.handle().clone();
        let s = stream::iter(vec![Buffer::from(Bytes::from("abcdef"))].into_iter().map(Ok));
        let mut r = StreamingBufReader::new(s, handle);
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
    fn streaming_skips_empty_chunks() {
        run_streaming(
            vec![
                Buffer::from(Bytes::new()),
                Buffer::from(Bytes::from("data")),
                Buffer::from(Bytes::new()),
            ],
            "data",
        );
    }

    #[test]
    fn streaming_block_on_counter_increments_per_chunk() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let handle = rt.handle().clone();
        let data = vec![
            Buffer::from(Bytes::from("aa")),
            Buffer::from(Bytes::from("bb")),
            Buffer::from(Bytes::from("cc")),
        ];
        let s = stream::iter(data.into_iter().map(Ok));
        let mut r = StreamingBufReader::new(s, handle);
        let mut out = String::new();
        r.read_to_string(&mut out).unwrap();
        assert_eq!(out, "aabbcc");
        assert_eq!(r.block_on_calls, 4); // 3 chunks + final None
    }
}
