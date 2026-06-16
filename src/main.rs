mod cli;
mod opendal_io;
mod walker;

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Target};
use grep_printer::{Standard, StandardBuilder};
use grep_regex::RegexMatcher;
use grep_searcher::SearcherBuilder;
use opendal::{Operator, services::S3};
use std::io::Write;
use termcolor::{ColorChoice, StandardStream};

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

fn build_printer(stream: StandardStream) -> Standard<StandardStream> {
    StandardBuilder::new()
        .heading(false)
        .column(false)
        .stats(false)
        .build(stream)
}

/// Construct a `Read` over `data`. Today this is always the full-buffer
/// `BufReader`; once Pi's `StreamingBufReader` is promoted into
/// `opendal_io`, this is the call site that swaps to it (with a `Handle`
/// threaded through for the chunk-at-a-time bridge).
fn make_reader(data: Vec<u8>) -> opendal_io::BufReader {
    opendal_io::BufReader::new(data)
}

/// First-deliverable run statistics. Wired to the printer once `--stats`
/// (FF3) lands; counted now so the surface doesn't churn at that point.
#[derive(Default)]
#[allow(dead_code)]
struct RunStats {
    total_matches: u64,
    total_files: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let target = Target::parse(&cli.target)?;
    let matcher = build_matcher(&cli.pattern, cli.ignore_case)?;
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .build();

    let stdout = StandardStream::stdout(ColorChoice::Never);
    let mut printer = build_printer(stdout);

    let mut any_match = false;
    let mut stats = RunStats::default();

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

                stats.total_files += 1;
                let display_path = format!("s3://{bucket}/{path}");
                let mut reader = make_reader(data);

                let mut sink = printer.sink_with_path(&matcher, &display_path);
                searcher
                    .search_reader(&matcher, &mut reader, &mut sink)
                    .with_context(|| format!("search failed for {display_path}"))?;

                if sink.has_match() {
                    any_match = true;
                    stats.total_matches += sink.match_count();
                }
            }
        }
    }

    let _ = std::io::stderr().flush();

    if !any_match {
        std::process::exit(1);
    }
    Ok(())
}
