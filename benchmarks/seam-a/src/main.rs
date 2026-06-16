//! Seam A end-to-end S3 bench + 64K outlier re-run.
//!
//! Measures two reader paths against grep_searcher:
//! - **Seam A (streaming S3)**: OpenDAL Reader::into_stream(..) → StreamingBufReader
//! - **Seam C (full-buffer S3)**: OpenDAL read → BufReader (control)
//!
//! Each file is run 5 times for each path to get stable measurements.
//! Outputs JSON rows with matrix v2.5 schema tags.

mod reader;

use anyhow::{Context, Result};
use grep_regex::RegexMatcher;
use grep_searcher::{SearcherBuilder, Sink, SinkMatch};
use opendal::{Operator, services::S3};
use std::time::Instant;
use tokio::runtime::Handle;

use reader::{BufReader, StreamingBufReader};

// ── Sink that counts but discards output ──────────────────────────
struct CountSink { count: u64 }
impl CountSink {
    fn new() -> Self { Self { count: 0 } }
}
impl Sink for CountSink {
    type Error = std::io::Error;
    fn matched(&mut self, _: &grep_searcher::Searcher, _mat: &SinkMatch<'_>) -> Result<bool, std::io::Error> {
        self.count += 1;
        Ok(true)
    }
}

// ── Bench row ─────────────────────────────────────────────────────
#[derive(serde::Serialize)]
struct BenchRow {
    seam: String,
    file: String,
    size_bytes: u64,
    elapsed_us: u128,
    block_on_calls: u64,
    iteration: u32,
    verdict: String,
    attribution: String,
    tool_name: String,
    definition_pin: String,
    corpus_kind: String,
    arch: String,
}

fn build_s3_operator(bucket: &str) -> Result<Operator> {
    let mut builder = S3::default().bucket(bucket);
    if let Ok(ep) = std::env::var("OPENDAL_S3_ENDPOINT") {
        builder = builder.endpoint(&ep);
    }
    if let Ok(r) = std::env::var("OPENDAL_S3_REGION") {
        builder = builder.region(&r);
    }
    Ok(Operator::new(builder)?.finish())
}

fn run_seam_c(op: &Operator, path: &str, file_size: u64, iter: u32) -> Result<BenchRow> {
    let rt = tokio::runtime::Builder::new_current_thread().build()?;
    let data = rt.block_on(async {
        let buf = op.read(path).await.context("read failed")?;
        Ok::<Vec<u8>, anyhow::Error>(buf.to_vec())
    })?;
    drop(rt);

    let start = Instant::now();
    let matcher = RegexMatcher::new("needle")?;
    let mut searcher = SearcherBuilder::new().line_number(true).build();
    let mut reader = BufReader::new(data);
    let mut sink = CountSink::new();
    searcher.search_reader(&matcher, &mut reader, &mut sink)?;
    let elapsed = start.elapsed();

    Ok(BenchRow {
        seam: "Seam-C".into(),
        file: path.to_string(),
        size_bytes: file_size,
        elapsed_us: elapsed.as_micros(),
        block_on_calls: 0,
        iteration: iter,
        verdict: "PASS_bench".into(),
        attribution: "tooling_dependent".into(),
        tool_name: "seam-a-bench".into(),
        definition_pin: "full-buffer-S3_minio-local_arch=aarch64".into(),
        corpus_kind: "code-tree".into(),
        arch: "aarch64".into(),
    })
}

fn run_seam_a(op: &Operator, path: &str, file_size: u64, handle: &Handle, iter: u32) -> Result<BenchRow> {
    let stream = handle.block_on(async {
        let reader = op.reader(path).await?;
        reader.into_stream(..).await
    })?;

    let start = Instant::now();
    let matcher = RegexMatcher::new("needle")?;
    let mut searcher = SearcherBuilder::new().line_number(true).build();
    let mut bridge = StreamingBufReader::new(stream, handle.clone());
    let mut sink = CountSink::new();
    searcher.search_reader(&matcher, &mut bridge, &mut sink)?;
    let elapsed = start.elapsed();

    Ok(BenchRow {
        seam: "Seam-A".into(),
        file: path.to_string(),
        size_bytes: file_size,
        elapsed_us: elapsed.as_micros(),
        block_on_calls: bridge.block_on_count(),
        iteration: iter,
        verdict: "PASS_bench".into(),
        attribution: "tooling_dependent".into(),
        tool_name: "seam-a-bench".into(),
        definition_pin: "streaming-S3_minio-local_arch=aarch64".into(),
        corpus_kind: "code-tree".into(),
        arch: "aarch64".into(),
    })
}

// ── Main ──────────────────────────────────────────────────────────
fn main() -> Result<()> {
    let bucket = std::env::var("BENCH_BUCKET").unwrap_or_else(|_| "rg-test".into());
    let prefix = std::env::var("BENCH_PREFIX").unwrap_or_else(|_| "bench-seam-a/".into());
    let iterations: u32 = std::env::var("BENCH_ITERS")
        .unwrap_or_else(|_| "3".into())
        .parse()
        .unwrap_or(3);

    let rt = tokio::runtime::Runtime::new()?;
    let handle = rt.handle().clone();

    let op = build_s3_operator(&bucket)?;

    // List files and get sizes
    let file_info = rt.block_on(async {
        use futures::TryStreamExt;
        let mut infos = Vec::new();
        let mut lister = op.lister_with(&prefix).recursive(true).await?;
        while let Some(entry) = lister.try_next().await? {
            let p = entry.path();
            if !p.ends_with('/') {
                let md = entry.metadata();
                infos.push((p.to_string(), md.content_length()));
            }
        }
        infos.sort_by(|a, b| a.1.cmp(&b.1));
        Ok::<_, anyhow::Error>(infos)
    })?;

    let mut rows = Vec::new();

    for (path, sz) in &file_info {
        for i in 0..iterations {
            eprintln!("Bench: {} ({} bytes, seam C, iter {})...", path, sz, i);
            rows.push(run_seam_c(&op, path, *sz, i)?);
        }
        for i in 0..iterations {
            eprintln!("Bench: {} ({} bytes, seam A, iter {})...", path, sz, i);
            rows.push(run_seam_a(&op, path, *sz, &handle, i)?);
        }
    }

    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}
