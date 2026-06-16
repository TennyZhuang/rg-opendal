# rg-opendal

`ripgrep` over [OpenDAL](https://opendal.apache.org/) backends — search remote object stores (S3-compatible), with the same regex engine and search semantics as native [ripgrep](https://github.com/BurntSushi/ripgrep), driven by `rg`'s public crates (`grep-searcher`, `grep-regex`, `grep-printer`, `ignore`).

This is a feasibility study & first-class implementation, not a fork of `rg`. It's an independent crate that consumes the official `grep-*` crates from crates.io.

## Status

Early. Phase 3 of an internal feasibility study. The first deliverable runs against an S3-compatible endpoint (tested with MinIO):

```sh
rg-opendal needle s3://bucket/prefix/
```

### Implemented

- Recursive prefix scan over S3-compatible endpoints (via OpenDAL).
- Regex matching using `grep-regex`.
- Case-insensitive flag (`-i`).
- Glob filtering (`-g`/`--glob`, including negative `!` prefix).
- Type-alias filtering (`-t`/`--type`, `-T`/`--type-not`) using `rg`'s built-in type defaults.
- Output format `s3://bucket/path:LINE:CONTENT`, sorted for deterministic results.

### On the roadmap

- Streaming reader (`Seam A`): bridge OpenDAL `Reader::into_stream` to `std::io::Read` to avoid full-buffer reads. End-to-end bench validated; module promotion pending.
- JSON output, context lines, stats (`--json`, `-A`/`-B`/`-C`, `--stats`).
- Local FS backend parity (capability-gated).
- Real-S3 perf characterization (current data is local MinIO).

## Build

```sh
cargo build --release
```

## Configure

S3 credentials follow the OpenDAL S3 default chain (env vars / IAM). Endpoint and region overrides:

```sh
export OPENDAL_S3_ENDPOINT=http://127.0.0.1:9000
export OPENDAL_S3_REGION=us-east-1
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
```

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
