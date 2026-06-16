mod cli;
mod opendal_io;
mod printer;
mod walker;

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Target};
use grep_printer::Stats;
use grep_regex::RegexMatcher;
use grep_searcher::{Searcher, SearcherBuilder};
use opendal::{services::S3, Operator};
use std::io::Write;
use std::path::Path;
use termcolor::NoColor;
use tokio::runtime::{Handle, Runtime};

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

fn build_matcher(pattern: &str, ignore_case: bool) -> Result<RegexMatcher> {
    if ignore_case {
        RegexMatcher::new(&format!("(?i){}", pattern))
            .with_context(|| format!("invalid regex (case-insensitive): {pattern}"))
    } else {
        RegexMatcher::new(pattern).with_context(|| format!("invalid regex: {pattern}"))
    }
}

/// Resolve context-line counts. Explicit `-A`/`-B` override `-C`; `-C`
/// supplies the default for both.
fn context_counts(cli: &Cli) -> (usize, usize) {
    let default = cli.context.unwrap_or(0);
    let after = cli.after_context.unwrap_or(default);
    let before = cli.before_context.unwrap_or(default);
    (after, before)
}

/// Print a ripgrep-compatible `--stats` summary to stderr.
fn print_stats(stats: &Stats) {
    eprintln!("{} matches", stats.matches());
    eprintln!("{} matched lines", stats.matched_lines());
    eprintln!("{} files contained matches", stats.searches_with_match());
    eprintln!("{} files searched", stats.searches());
    eprintln!("{} bytes printed", stats.bytes_printed());
    eprintln!("{} bytes searched", stats.bytes_searched());
    eprintln!("{:.6} seconds spent searching", stats.elapsed().as_secs_f64());
}

/// Search a single object. Branches on `streaming`:
/// - false (default): full-buffer `BufReader` (Seam C). Matrix v2.7 row #3
///   shows this is 1.8–4.0× faster than streaming on local minio.
/// - true: `StreamingBufReader` over `Reader::into_stream` (Seam A). Lower
///   peak memory (~1 chunk ≈ 200 KB) at the cost of one `block_on` per
///   chunk transition.
///
/// `handle` must be from a runtime that does NOT have main as one of its
/// worker threads — otherwise the `block_on` calls inside `StreamingBufReader`
/// would deadlock.
fn search_one_object<S: grep_searcher::Sink>(
    handle: &Handle,
    op: &Operator,
    path: &str,
    streaming: bool,
    searcher: &mut Searcher,
    matcher: &RegexMatcher,
    sink: &mut S,
) -> Result<()>
where
    S::Error: std::error::Error + Send + Sync + 'static,
{
    if streaming {
        let reader_async = handle
            .block_on(async { op.reader(path).await })
            .with_context(|| format!("opening streaming reader for {path}"))?;
        let stream = handle
            .block_on(async { reader_async.into_stream(..).await })
            .with_context(|| format!("opening byte stream for {path}"))?;
        let mut reader = opendal_io::StreamingBufReader::new(stream, handle.clone());
        searcher
            .search_reader(matcher, &mut reader, sink)
            .with_context(|| format!("search failed for {path}"))?;
    } else {
        let data = handle
            .block_on(async { op.read(path).await })
            .with_context(|| format!("reading {path}"))?
            .to_vec();
        let mut reader = opendal_io::BufReader::new(data);
        searcher
            .search_reader(matcher, &mut reader, sink)
            .with_context(|| format!("search failed for {path}"))?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let target = Target::parse(&cli.target)?;
    let matcher = build_matcher(&cli.pattern, cli.ignore_case)?;
    let (after_context, before_context) = context_counts(&cli);
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .after_context(after_context)
        .before_context(before_context)
        .build();

    let mut printer = if cli.json {
        printer::Printer::json(NoColor::new(std::io::stdout()))
    } else {
        printer::Printer::standard(NoColor::new(std::io::stdout()), cli.stats)
    };

    // Drive async OpenDAL operations from a side runtime; main is NOT a
    // worker of that runtime, so calling `handle.block_on` from main is safe
    // and is what makes `StreamingBufReader::read` (which also calls
    // `block_on` on this same handle) deadlock-free under `--streaming`.
    let rt = Runtime::new().context("creating tokio runtime")?;
    let handle = rt.handle().clone();

    let mut any_match = false;
    let mut aggregate_stats = Stats::new();

    match target {
        Target::S3 { bucket, prefix } => {
            let op = build_s3_operator(bucket)?;
            let filter = walker::WalkFilter::new(&cli.globs, &cli.types, &cli.types_not)?;
            let paths = handle
                .block_on(async { walker::list_objects(&op, prefix, Some(&filter)).await })?;

            for path in paths {
                let display_path = format!("s3://{bucket}/{path}");
                let mut sink =
                    printer.sink_with_path(&matcher, Path::new(&display_path));
                let res = search_one_object(
                    &handle,
                    &op,
                    &path,
                    cli.streaming,
                    &mut searcher,
                    &matcher,
                    &mut sink,
                );
                if let Err(e) = res {
                    eprintln!("rg-opendal: {e:#}");
                    continue;
                }
                if sink.has_match() {
                    any_match = true;
                }
                if cli.stats {
                    if let Some(stats) = sink.stats() {
                        aggregate_stats += stats;
                    }
                }
            }
        }
    }

    let _ = std::io::stderr().flush();

    if cli.stats {
        print_stats(&aggregate_stats);
    }

    if !any_match {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_counts_use_explicit_overrides() {
        let cli = Cli::parse_from([
            "rg-opendal",
            "-A",
            "3",
            "-C",
            "2",
            "pattern",
            "s3://bucket/prefix",
        ]);
        assert_eq!(context_counts(&cli), (3, 2));
    }

    #[test]
    fn context_counts_use_context_for_both() {
        let cli = Cli::parse_from([
            "rg-opendal",
            "-C",
            "5",
            "pattern",
            "s3://bucket/prefix",
        ]);
        assert_eq!(context_counts(&cli), (5, 5));
    }

    #[test]
    fn context_counts_default_to_zero() {
        let cli = Cli::parse_from(["rg-opendal", "pattern", "s3://bucket/prefix"]);
        assert_eq!(context_counts(&cli), (0, 0));
    }

    #[test]
    fn streaming_flag_defaults_to_false() {
        let cli = Cli::parse_from(["rg-opendal", "pattern", "s3://bucket/prefix"]);
        assert!(!cli.streaming);
    }

    #[test]
    fn streaming_flag_can_be_set() {
        let cli = Cli::parse_from([
            "rg-opendal",
            "--streaming",
            "pattern",
            "s3://bucket/prefix",
        ]);
        assert!(cli.streaming);
    }
}
