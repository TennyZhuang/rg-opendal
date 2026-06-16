# rg-opendal

> Search remote object stores with ripgrep — `s3://bucket/prefix` via [OpenDAL](https://github.com/apache/opendal).

[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)
[![Rust](https://img.shields.io/badge/rust-2021%20edition-orange)](https://www.rust-lang.org)
[![Matrix](https://img.shields.io/badge/matrix-v2.15-informational)](MATRIX.md)

**Status:** Phase 3 feasibility — FF1–FF13+FF16 implemented and harness-attested against local MinIO. Real-S3 perf characterization, CI, and crates.io publishing pending.

An independent crate consuming [ripgrep](https://github.com/BurntSushi/ripgrep)'s public crates (`grep-searcher`, `grep-regex`, `grep-matcher`, `grep-printer`). Uses OpenDAL as the I/O layer to recursively search S3-compatible object stores with ripgrep's full regex engine. See [MATRIX.md](MATRIX.md) for the detailed verification status.

## Quick Start

```bash
# 1. Build
cargo build --release

# 2. Point at a MinIO instance (or any S3-compatible endpoint)
export OPENDAL_S3_ENDPOINT=http://127.0.0.1:9000
export OPENDAL_S3_REGION=us-east-1
export AWS_ACCESS_KEY_ID=minioadmin
export AWS_SECRET_ACCESS_KEY=minioadmin

# 3. Search
./target/release/rg-opendal needle s3://my-bucket/prefix/
```

Output format matches `rg` default: `s3://bucket/path:LINE:CONTENT`. On single-file searches the filename prefix is omitted (matching `rg`'s auto-with-filename behavior). Use `-H`/`--with-filename` to always show it, `-I`/`--no-filename` to always suppress.

## Features

All output parity is verified against native `rg` on the same fixture.

| Flag | Feature | Status |
|------|---------|:------:|
| `pattern`, `-i` | Regex search + case-insensitive (FF1) | ✅ |
| `-g`/`--glob`, `-t`/`--type`, `-T`/`--type-not` | Glob + type filtering (FF2) | ✅ |
| `--json`, `-A`/`-B`/`-C`, `--stats` | JSON output, context lines, stats (FF3) | ✅ |
| `--color=auto\|always\|never` | Color output (FF4) | ✅ |
| `-c`/`--count`, `-l`/`--files-with-matches` | Count + files-with-matches (FF5) | ✅ |
| `-w`/`--word-regexp`, `-x`/`--line-regexp` | Word + line regexp (FF6) | ✅ |
| `-m`/`--max-count` | Per-file match cap (FF7) | ✅ |
| `-v`/`--invert-match` | Invert match (FF8) | ✅ |
| `-N`/`--no-line-number`, `--column`, `--heading` | Printer flags (FF9) | ✅ |
| `-I`/`--no-filename`, `-H`/`--with-filename` | Path prefix control (FF10) | ✅ |
| `-0`/`--null` | NUL path terminator (FF11) | ✅ |
| `-z`/`--null-data` | NUL line terminator (FF12) | ✅ |
| `-a`/`--text` | Force text mode on binary files (FF13) | ✅ |
| `-F`/`--fixed-strings` | Literal matching (FF16) | ✅ |
| `--streaming` | Stream via OpenDAL chunks (Seam A) | ✅ |

## Examples

```bash
# Basic search
rg-opendal needle s3://my-bucket/data/

# Case-insensitive regex with glob filter
rg-opendal -i -g '*.log' error s3://my-bucket/logs/

# JSON output with stats
rg-opendal --json --stats pattern s3://my-bucket/data/

# Count matches per file
rg-opendal -c TODO s3://my-bucket/codebase/

# Stream large files without loading into memory
rg-opendal --streaming -i security s3://my-bucket/audit/

# Invert match — show lines that DON'T match
rg-opendal -v DEBUG s3://my-bucket/logs/app.log

# Word-boundary search
rg-opendal -w -i error s3://my-bucket/docs/
```

## Build & Configure

**Requirements:** Rust 1.78+, OpenDAL-compatible object store.

```bash
cargo build --release
cargo test --release
```

**Environment variables:**

| Variable | Description |
|----------|-------------|
| `OPENDAL_S3_ENDPOINT` | S3-compatible endpoint URL |
| `OPENDAL_S3_REGION` | AWS region |
| `AWS_ACCESS_KEY_ID` | Access key |
| `AWS_SECRET_ACCESS_KEY` | Secret key |

For local development with [MinIO](https://min.io): start MinIO on port 9000, set the env vars above with `minioadmin`/`minioadmin` credentials, and use `s3://my-bucket/` as the target.

## Architecture

```
s3://bucket/prefix/
    │
    ▼
┌─────────────┐     ┌──────────────────┐     ┌──────────────┐
│  OpenDAL    │────▶│  BufReader (C)   │────▶│ grep-searcher│
│  Reader     │     │  or              │     │ grep-regex   │
│  list/read  │     │  StreamingBuf-   │     │ grep-matcher │
│             │     │  Reader (A)      │     │ grep-printer │
└─────────────┘     └──────────────────┘     └──────────────┘
                                                     │
                                                     ▼
                                                stdout/stderr
```

- **Seam C (default):** Full-buffer reader. Fastest on local MinIO — reads the entire object into memory, then searches in-process. Matrix v2.7 row #3 measured 1.8–4.0× wall time advantage over streaming on local MinIO.
- **Seam A (`--streaming`):** Chunk-at-a-time bridge via `Handle::block_on`. Reads objects in ~180KB OpenDAL chunks, keeping peak memory per request low. Verifiably output-identical to Seam C (harness-attested, PR #11).

The crate is `[lib] + [[bin]]`: library types (`opendal_io`, `walker`, `cli`, `printer`) are reusable; the binary is the CLI entry point. A separate `benchmarks/seam-a/` crate provides the Seam A bridge microbenchmark.

## Contributing

- **PR workflow:** branch → open PR (ready, not draft) → DeepSeek review → squash-merge.
- **Tests:** `cargo test --release` runs 81 unit tests. Harness scripts in `scripts/` validate feature parity against native `rg` on a MinIO fixture.
- **Fixture:** requires a running MinIO instance with a bucket at `s3://rg-test/harness-ff2/`. See `scripts/` for fixture setup commands.
- **Matrix:** feature completeness and evidence discipline tracked in [MATRIX.md](MATRIX.md). All harness PASS claims must be verified against merged `main`.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
