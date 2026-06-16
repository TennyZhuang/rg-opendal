mod cli;
mod opendal_io;
mod printer;
mod walker;

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Target};
use grep_printer::Stats;
use grep_regex::RegexMatcher;
use grep_searcher::SearcherBuilder;
use opendal::{services::S3, Operator};
use std::io::Write;
use std::path::Path;
use termcolor::NoColor;

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

/// Construct a `Read` over `data`. Today this is always the full-buffer
/// `BufReader`; once Pi's `StreamingBufReader` is promoted into
/// `opendal_io`, this is the call site that swaps to it (with a `Handle`
/// threaded through for the chunk-at-a-time bridge).
fn make_reader(data: Vec<u8>) -> opendal_io::BufReader {
    opendal_io::BufReader::new(data)
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
    eprintln!("{:.6} seconds", stats.elapsed().as_secs_f64());
}

#[tokio::main]
async fn main() -> Result<()> {
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

    let mut any_match = false;
    let mut aggregate_stats = Stats::new();

    match target {
        Target::S3 { bucket, prefix } => {
            let op = build_s3_operator(bucket)?;
            let filter = walker::WalkFilter::new(&cli.globs, &cli.types, &cli.types_not)?;
            let paths = walker::list_objects(&op, prefix, Some(&filter)).await?;

            for path in paths {
                let data = match op.read(&path).await {
                    Ok(buf) => buf.to_vec(),
                    Err(e) => {
                        eprintln!("rg-opendal: error reading {path}: {e}");
                        continue;
                    }
                };

                let display_path = format!("s3://{bucket}/{path}");
                let mut reader = make_reader(data);

                let mut sink = printer.sink_with_path(&matcher, Path::new(&display_path));
                searcher
                    .search_reader(&matcher, &mut reader, &mut sink)
                    .with_context(|| format!("search failed for {display_path}"))?;

                if sink.has_match() {
                    any_match = true;
                }
                if cli.stats {
                    if let Some(stats) = sink.stats() {
                        aggregate_stats += stats.clone();
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
}
