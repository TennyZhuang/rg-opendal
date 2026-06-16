use anyhow::{Context, Result};
use clap::Parser;
use grep_printer::{Stats, SummaryKind};
use grep_regex::RegexMatcher;
use grep_searcher::{Searcher, SearcherBuilder};
use opendal::{services::S3, Operator};
use rg_opendal::cli::{Cli, Target};
use rg_opendal::printer::Printer;
use rg_opendal::{opendal_io, walker};
use std::io::Write;
use std::path::Path;
use termcolor::{ColorChoice, StandardStream};
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

/// Apply optional `-w`/`-x` regex anchoring around the user pattern, then
/// compile it. Mirrors native rg's behavior:
/// - `-w` wraps as `(?:<pattern>)` between `\b` boundaries
/// - `-x` wraps as `^(?:<pattern>)$`
/// - `-i` adds the `(?i)` flag at the very start so it applies to the whole
///   anchored expression.
///
/// `-w` and `-x` are mutually exclusive at the CLI layer (clap conflict),
/// so this only ever applies one of them.
fn anchor_pattern(pattern: &str, word: bool, line: bool) -> String {
    if line {
        format!("^(?:{pattern})$")
    } else if word {
        format!(r"\b(?:{pattern})\b")
    } else {
        pattern.to_string()
    }
}

fn build_matcher(
    pattern: &str,
    ignore_case: bool,
    word: bool,
    line: bool,
) -> Result<RegexMatcher> {
    let anchored = anchor_pattern(pattern, word, line);
    let final_expr = if ignore_case {
        format!("(?i){anchored}")
    } else {
        anchored
    };
    RegexMatcher::new(&final_expr)
        .with_context(|| format!("invalid regex: {pattern}"))
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
    let matcher = build_matcher(
        &cli.pattern,
        cli.ignore_case,
        cli.word_regexp,
        cli.line_regexp,
    )?;
    let (after_context, before_context) = context_counts(&cli);
    let mut searcher = SearcherBuilder::new()
        .line_number(!cli.no_line_number)
        .after_context(after_context)
        .before_context(before_context)
        .max_matches(cli.max_count.map(|n| n as u64))
        .build();

    let mut printer = if cli.json {
        // JSON output is never colored; using a color-capable writer keeps the
        // Printer type identical in the other branches without affecting JSON.
        Printer::json(StandardStream::stdout(ColorChoice::Never))
    } else if cli.count {
        Printer::summary(
            StandardStream::stdout(cli.color.to_color_choice()),
            SummaryKind::Count,
            cli.stats,
        )
    } else if cli.files_with_matches {
        Printer::summary(
            StandardStream::stdout(cli.color.to_color_choice()),
            SummaryKind::PathWithMatch,
            cli.stats,
        )
    } else {
        Printer::standard(
            StandardStream::stdout(cli.color.to_color_choice()),
            cli.stats,
            cli.column,
            cli.no_heading,
        )
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
    use rg_opendal::cli::ColorArg;

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

    #[test]
    fn color_defaults_to_auto() {
        let cli = Cli::parse_from(["rg-opendal", "pattern", "s3://bucket/prefix"]);
        assert_eq!(cli.color, ColorArg::Auto);
    }

    #[test]
    fn color_can_be_always() {
        let cli = Cli::parse_from([
            "rg-opendal",
            "--color",
            "always",
            "pattern",
            "s3://bucket/prefix",
        ]);
        assert_eq!(cli.color, ColorArg::Always);
    }

    #[test]
    fn color_can_be_never() {
        let cli = Cli::parse_from([
            "rg-opendal",
            "--color",
            "never",
            "pattern",
            "s3://bucket/prefix",
        ]);
        assert_eq!(cli.color, ColorArg::Never);
    }

    #[test]
    fn color_choice_from_arg() {
        assert_eq!(ColorArg::Auto.to_color_choice(), ColorChoice::Auto);
        assert_eq!(ColorArg::Always.to_color_choice(), ColorChoice::Always);
        assert_eq!(ColorArg::Never.to_color_choice(), ColorChoice::Never);
    }

    #[test]
    fn count_flag_parses() {
        let cli = Cli::parse_from([
            "rg-opendal",
            "-c",
            "pattern",
            "s3://bucket/prefix",
        ]);
        assert!(cli.count);
        assert!(!cli.files_with_matches);
        assert!(!cli.json);
    }

    #[test]
    fn files_with_matches_flag_parses() {
        let cli = Cli::parse_from([
            "rg-opendal",
            "-l",
            "pattern",
            "s3://bucket/prefix",
        ]);
        assert!(cli.files_with_matches);
        assert!(!cli.count);
        assert!(!cli.json);
    }

    #[test]
    fn count_conflicts_with_json() {
        assert!(Cli::try_parse_from([
            "rg-opendal",
            "-c",
            "--json",
            "pattern",
            "s3://bucket/prefix",
        ])
        .is_err());
    }

    #[test]
    fn files_with_matches_conflicts_with_json() {
        assert!(Cli::try_parse_from([
            "rg-opendal",
            "-l",
            "--json",
            "pattern",
            "s3://bucket/prefix",
        ])
        .is_err());
    }

    #[test]
    fn count_conflicts_with_files_with_matches() {
        assert!(Cli::try_parse_from([
            "rg-opendal",
            "-c",
            "-l",
            "pattern",
            "s3://bucket/prefix",
        ])
        .is_err());
    }

    #[test]
    fn max_count_defaults_to_none() {
        let cli = Cli::parse_from(["rg-opendal", "pattern", "s3://bucket/prefix"]);
        assert_eq!(cli.max_count, None);
    }

    #[test]
    fn max_count_parses() {
        let cli = Cli::parse_from([
            "rg-opendal",
            "--max-count",
            "5",
            "pattern",
            "s3://bucket/prefix",
        ]);
        assert_eq!(cli.max_count, Some(5));
    }

    #[test]
    fn max_count_conflicts_with_files_with_matches() {
        assert!(Cli::try_parse_from([
            "rg-opendal",
            "-l",
            "-m",
            "3",
            "pattern",
            "s3://bucket/prefix",
        ])
        .is_err());
    }

    #[test]
    fn anchor_pattern_no_anchors_returns_unchanged() {
        assert_eq!(anchor_pattern("foo", false, false), "foo");
    }

    #[test]
    fn anchor_pattern_word_wraps_with_word_boundaries() {
        assert_eq!(anchor_pattern("foo", true, false), r"\b(?:foo)\b");
    }

    #[test]
    fn anchor_pattern_line_wraps_with_line_anchors() {
        assert_eq!(anchor_pattern("foo", false, true), "^(?:foo)$");
    }

    #[test]
    fn anchor_pattern_groups_alternations() {
        // Without the (?:...) group, `\bfoo|bar\b` would be `(\bfoo) | (bar\b)`.
        assert_eq!(anchor_pattern("foo|bar", true, false), r"\b(?:foo|bar)\b");
        assert_eq!(anchor_pattern("foo|bar", false, true), "^(?:foo|bar)$");
    }

    #[test]
    fn word_regexp_flag_parses() {
        let cli =
            Cli::parse_from(["rg-opendal", "-w", "pattern", "s3://bucket/prefix"]);
        assert!(cli.word_regexp);
        assert!(!cli.line_regexp);
    }

    #[test]
    fn line_regexp_flag_parses() {
        let cli =
            Cli::parse_from(["rg-opendal", "-x", "pattern", "s3://bucket/prefix"]);
        assert!(cli.line_regexp);
        assert!(!cli.word_regexp);
    }

    #[test]
    fn word_regexp_conflicts_with_line_regexp() {
        assert!(Cli::try_parse_from([
            "rg-opendal",
            "-w",
            "-x",
            "pattern",
            "s3://bucket/prefix",
        ])
        .is_err());
    }

    #[test]
    fn build_matcher_word_regexp_matches_whole_word() {
        use grep_matcher::Matcher;
        let m = build_matcher("foo", false, true, false).unwrap();
        assert!(m.is_match(b"a foo b").unwrap());
        assert!(!m.is_match(b"foobar").unwrap());
    }

    #[test]
    fn build_matcher_line_regexp_matches_full_line() {
        use grep_matcher::Matcher;
        let m = build_matcher("foo", false, false, true).unwrap();
        assert!(m.is_match(b"foo").unwrap());
        assert!(!m.is_match(b"foobar").unwrap());
        assert!(!m.is_match(b"a foo").unwrap());
    }

    #[test]
    fn build_matcher_ignore_case_composes_with_word_regexp() {
        use grep_matcher::Matcher;
        let m = build_matcher("foo", true, true, false).unwrap();
        assert!(m.is_match(b"a FOO b").unwrap());
        assert!(!m.is_match(b"FOOBAR").unwrap());
    }

    #[test]
    fn no_line_number_flag_parses() {
        let cli = Cli::parse_from([
            "rg-opendal",
            "-N",
            "pattern",
            "s3://bucket/prefix",
        ]);
        assert!(cli.no_line_number);
    }

    #[test]
    fn column_flag_parses() {
        let cli = Cli::parse_from([
            "rg-opendal",
            "--column",
            "pattern",
            "s3://bucket/prefix",
        ]);
        assert!(cli.column);
    }

    #[test]
    fn no_heading_flag_parses() {
        let cli = Cli::parse_from([
            "rg-opendal",
            "--no-heading",
            "pattern",
            "s3://bucket/prefix",
        ]);
        assert!(cli.no_heading);
    }
}
