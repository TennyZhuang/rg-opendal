# Matrix — Evidence Status

Current matrix version: **v2.15** (channel-record-attested 2026-06-17).

The matrix tracks the verification status of each feature family and seam property for the `rg-opendal` feasibility study. All harness PASS claims are verified against merged `main`; evidence discipline (Decision #9) distinguishes existence smoke from quantitative bench from synthetic data.

## D₂ Existence Smoke (FF1–FF13, FF16)

| Feature | Flags | Verdict | Harness |
|---------|-------|:-------:|---------|
| FF1 | Literal + regex match, case-insensitive | PASS_harness | `harness-ff1-scaffold.sh` |
| FF2 | Glob + type filtering | PASS_harness | `harness-ff2-glob-type.sh` |
| FF3 | JSON output, context lines, stats | PASS_harness | `harness-ff3-json-stats.sh` |
| FF4 | `--color` | PASS_harness | `harness-ff4-color.sh` |
| FF5 | `-c`/`--count`, `-l`/`--files-with-matches` | PASS_harness | `harness-ff5-count-files.sh` |
| FF6 | `-w`/`-x` word/line regexp | PASS_harness | `harness-ff6-word-line.sh` |
| FF7 | `-m`/`--max-count` | PASS_harness | `harness-ff7-ff8-ff9.sh` |
| FF8 | `-v`/`--invert-match` | PASS_harness | `harness-ff7-ff8-ff9.sh` |
| FF9 | `-N`/`--no-line-number`, `--column`, `--heading` | PASS_harness | `harness-ff7-ff8-ff9.sh` |
| FF10 | `-I`/`-H` path prefix control + auto-detection | PASS_harness | `harness-ff10-ff11-ff12-ff13.sh` |
| FF11 | `-0`/`--null` path terminator | PASS_harness | `harness-ff10-ff11-ff12-ff13.sh` |
| FF12 | `-z`/`--null-data` record separator | PASS_harness | `harness-ff10-ff11-ff12-ff13.sh` |
| FF13 | `-a`/`--text` force text mode | PASS_harness† | `harness-ff10-ff11-ff12-ff13.sh` |
| FF16 | `-F`/`--fixed-strings` | PASS_harness | pending |

† Flag-effect-only smoke; content-parity deferred.

## Seam A Streaming Bridge

| Cell | Verdict | Note |
|------|:-------:|------|
| Existence (bench) | PASS_harness | `--streaming` reachable from CLI |
| Existence (CLI parity) | PASS_harness | `--streaming` output = default |
| Bridge cost (minio-local) | PASS_bench | 1.8–4.0× Seam C, 2.7–525.8 MB |

## Cells Still Blocked

- Real-S3 perf (minio is local-loopback)
- Seam A bytes-paid-before-reject, early-termination
- Parallel walker, gitignore, binary-detection-RTT, SIMD-prefilter
- Walker cost, mmap (lost in s3:// path)

Full matrix history is channel-record-attested. Contact the maintainers for access to the detailed evidence trail.
