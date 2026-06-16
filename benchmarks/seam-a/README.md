# Seam A Bridge Benchmark Harness

Measures end-to-end streaming performance of the OpenDAL `Reader::into_stream()` → `StreamingBufReader` → `grep_searcher` pipeline vs the full-buffer Seam C path.

## Architecture

- **Seam A (streaming)**: `Reader::into_stream(..)` → `StreamingBufReader` (chunk-at-a-time bridge via `Handle::block_on`)
- **Seam C (full-buffer)**: `op.read(path).await?.to_vec()` → `BufReader` (in-memory, PoC-validated pattern)

## Usage

```bash
# Build
cargo build --release

# Run against minio
OPENDAL_S3_ENDPOINT=http://127.0.0.1:9000 \
OPENDAL_S3_REGION=us-east-1 \
AWS_ACCESS_KEY_ID=minioadmin \
AWS_SECRET_ACCESS_KEY=minioadmin \
BENCH_ITERS=3 \
./target/release/seam-a-bench
```

## Environment

| Variable | Default | Description |
|----------|---------|-------------|
| `BENCH_BUCKET` | `rg-test` | Minio bucket |
| `BENCH_PREFIX` | `bench-seam-a/` | Object prefix for fixture files |
| `BENCH_ITERS` | `3` | Iterations per file per seam |
| `OPENDAL_S3_ENDPOINT` | — | S3-compatible endpoint |
| `OPENDAL_S3_REGION` | — | AWS region |
| `AWS_ACCESS_KEY_ID` | — | Minio credentials |
| `AWS_SECRET_ACCESS_KEY` | — | Minio credentials |

## Output

JSON array of bench rows with matrix v2.7 schema tags:
- `verdict`, `attribution`, `tool_name`, `definition_pin`, `corpus_kind`, `arch`
- Per-row: `seam`, `file`, `size_bytes`, `elapsed_us`, `block_on_calls`, `iteration`

## Results

### v0.1 — 4 sizes (msg `ffe3ce18`, DeepSeek review `4ceda755`, matrix v2.6)

| File | Size | Seam C (med) | Seam A (med) | Ratio | block_on |
|------|------|:-----:|:-----:|:-----:|:-----:|
| 1mb.txt | 2.8 MB | 542 µs | 1,732 µs | 3.2× | 14 |
| 8mb.txt | 16.8 MB | 2,678 µs | 10,885 µs | 4.1× | 81 |
| 50mb.txt | 104.9 MB | 35,313 µs | 79,402 µs | 2.2× | 575 |
| 100mb.txt | 209.7 MB | 58,872 µs | 164,522 µs | 2.8× | 1,165 |

### v0.1+500MB — 5 sizes (msg `9b5cb481`, DeepSeek review `ef1844b7`, matrix v2.7)

| File | Size | Seam C (med) | Seam A (med) | Ratio | block_on |
|------|------|:-----:|:-----:|:-----:|:-----:|
| 1mb.txt | 2.8 MB | 545 µs | 2,204 µs | 4.0× | 21 |
| 8mb.txt | 16.8 MB | 3,893 µs | 10,327 µs | 2.7× | 55 |
| 50mb.txt | 104.9 MB | 17,292 µs | 57,080 µs | 3.3× | 613 |
| 100mb.txt | 209.7 MB | 71,308 µs | 129,082 µs | 1.8× | 1,156 |
| 500mb.txt | 525.8 MB | 44,611 µs | 175,977 µs | 3.9× | 2,654 |

**Conclusion**: No asymptotic cliff. Seam A overhead 1.8–4.0× across 2.7–525.8 MB. Per-block_on cost ~50 µs at ≥100MB (minio cache effect on sequential chunk reads).

## Caveats

- **Minio is local-loopback**, not real S3. Real S3 latency would increase per-chunk cost.
- **Fixture files are base64-generated** — file sizes on disk are ~2× the logical content size.
- **OpenDAL chunk size is service-driven** (~133–305 KB observed), not controllable via public API.
- **Iteration 0 cold-start** observed at larger sizes (DeepSeek `4ceda755` Observation 1). v0.2 should add a warm-up iteration.
