//! CLI surface.
//!
//! FF1: pattern + s3:// URL + `-i`.
//! FF2: `--glob/-g`, `--type/-t`, `--type-not/-T` over object keys.
//! FF3: `--json`, `--stats`, `-A/-B/-C` context lines.
//! FF4: `--color` output coloring.
//! FF5: `-c/--count` and `-l/--files-with-matches`.
//! FF6: `-w/--word-regexp` and `-x/--line-regexp`.
//! FF7: `--max-count=N` per-file match cap.
//! FF8: `-v/--invert-match` print non-matching lines.
//! FF9: `-N/--no-line-number`, `--column`, `--heading` printer flags.
//! FF10: `-I/--no-filename` and `-H/--with-filename` path-prefix control
//!        with auto-detection (single file → hide, multi file → show) per rg.
//! FF11: `-0/--null` path terminator flag.
//! FF12: `-z/--null-data` null-byte line terminator.
//! FF13: `-a/--text` force text mode (disable binary detection).
//! FF16: `-F/--fixed-strings` literal pattern matching.

use clap::Parser;
use termcolor::ColorChoice;

#[derive(Parser, Debug)]
#[command(
    name = "rg-opendal",
    about = "ripgrep over OpenDAL backends (s3:// recursive prefix scan with glob/type filtering)"
)]
pub struct Cli {
    /// Pattern (regex)
    pub pattern: String,

    /// Target — either `s3://bucket/prefix` or a local path (via OpenDAL fs backend)
    pub target: String,

    /// Case-insensitive matching
    #[arg(short = 'i', long)]
    pub ignore_case: bool,

    /// Glob filter applied to object keys; repeat for multiple. Prefix with `!` to negate.
    /// Example: -g '*.rs' -g '!target/**'
    #[arg(short = 'g', long = "glob")]
    pub globs: Vec<String>,

    /// Only include files matching the given type alias (rg builtin types). Repeatable.
    #[arg(short = 't', long = "type")]
    pub types: Vec<String>,

    /// Exclude files matching the given type alias. Repeatable.
    #[arg(short = 'T', long = "type-not")]
    pub types_not: Vec<String>,

    /// Emit results as JSON lines (one JSON object per line).
    /// Mutually exclusive with the default human-readable output.
    #[arg(long = "json")]
    pub json: bool,

    /// Print statistics summary to stderr after searching.
    #[arg(long = "stats")]
    pub stats: bool,

    /// Show NUM lines of context after each match.
    #[arg(short = 'A', long = "after-context", value_name = "NUM")]
    pub after_context: Option<usize>,

    /// Show NUM lines of context before each match.
    #[arg(short = 'B', long = "before-context", value_name = "NUM")]
    pub before_context: Option<usize>,

    /// Show NUM lines of context before and after each match.
    /// Equivalent to `-B NUM -A NUM`.
    #[arg(short = 'C', long = "context", value_name = "NUM")]
    pub context: Option<usize>,

    /// Read each object via OpenDAL's streaming reader (Seam A) instead of
    /// loading the full body into memory (Seam C, default). Trades wall-time
    /// for peak memory: Seam A holds at most one chunk (~200 KB on S3) at a
    /// time but pays one tokio block_on per chunk transition — matrix v2.7
    /// row #3 measured 1.8–4.0× the wall time of Seam C on local minio.
    #[arg(long = "streaming")]
    pub streaming: bool,

    /// When to use colors in the output. Only affects the standard printer;
    /// `--json` output is never colored.
    #[arg(long = "color", value_enum, default_value_t = ColorArg::Auto)]
    pub color: ColorArg,

    /// Show only a count of matching lines per file.
    ///
    /// Mutually exclusive with `--json` and `--files-with-matches`.
    #[arg(short = 'c', long = "count", conflicts_with = "json", conflicts_with = "files_with_matches")]
    pub count: bool,

    /// Show only the names of files containing at least one match.
    ///
    /// Mutually exclusive with `--json` and `--count`.
    #[arg(short = 'l', long = "files-with-matches", conflicts_with = "json", conflicts_with = "count")]
    pub files_with_matches: bool,

    /// Only match the pattern at word boundaries (regex `\b…\b`).
    ///
    /// Mutually exclusive with `--line-regexp`.
    #[arg(short = 'w', long = "word-regexp", conflicts_with = "line_regexp")]
    pub word_regexp: bool,

    /// Only match the pattern when it spans an entire line (regex `^…$`).
    ///
    /// Mutually exclusive with `--word-regexp`.
    #[arg(short = 'x', long = "line-regexp", conflicts_with = "word_regexp")]
    pub line_regexp: bool,

    /// Stop searching each file after NUM matches.
    ///
    /// Mutually exclusive with `--files-with-matches` (which only needs one match).
    #[arg(short = 'm', long = "max-count", value_name = "NUM", conflicts_with = "files_with_matches")]
    pub max_count: Option<usize>,

    /// Invert matching: print non-matching lines instead of matching ones.
    #[arg(short = 'v', long = "invert-match")]
    pub invert_match: bool,

    /// Suppress line numbers in the standard printer output.
    #[arg(short = 'N', long = "no-line-number")]
    pub no_line_number: bool,

    /// Show the column number of the first match in each line.
    #[arg(long = "column")]
    pub column: bool,

    /// Group matches by file under a heading (path on its own line, then
    /// path-prefix omitted on each match line). Default: off — native rg's
    /// behavior for non-tty output is path-per-line.
    #[arg(long = "heading")]
    pub heading: bool,

    /// Suppress the file path prefix on each matching line.
    /// Mutually exclusive with `--with-filename`.
    #[arg(short = 'I', long = "no-filename", conflicts_with = "with_filename")]
    pub no_filename: bool,

    /// Always show the file path prefix on each matching line, even when a
    /// single file is searched. Mutually exclusive with `--no-filename`.
    /// Default: path is shown when multiple files match, hidden when exactly
    /// one file matches — same auto-detection as native rg.
    #[arg(short = 'H', long = "with-filename", conflicts_with = "no_filename")]
    pub with_filename: bool,

    /// Add a null byte after the file path (standard rg `-0/--null`).
    #[arg(short = '0', long = "null")]
    pub null: bool,

    /// Use a null byte as the line terminator instead of newline.
    /// Allows searching files with NUL-separated records (standard rg `-z/--null-data`).
    #[arg(short = 'z', long = "null-data")]
    pub null_data: bool,

    /// Force text mode: treat all files as text and search them even if they
    /// appear to contain binary data (standard rg `-a/--text`).
    #[arg(short = 'a', long = "text")]
    pub text: bool,

    /// Treat the pattern as a literal string, not a regex (standard rg `-F/--fixed-strings`).
    #[arg(short = 'F', long = "fixed-strings")]
    pub fixed_strings: bool,
}

/// `--color` argument values, mapped to `termcolor::ColorChoice`.
#[derive(Clone, Copy, Debug, Default, PartialEq, clap::ValueEnum)]
pub enum ColorArg {
    /// Use colors if stdout is a terminal.
    #[default]
    Auto,
    /// Always use colors.
    Always,
    /// Never use colors.
    Never,
}

impl ColorArg {
    /// Convert to `termcolor::ColorChoice` for the standard printer.
    pub fn to_color_choice(self) -> ColorChoice {
        match self {
            ColorArg::Auto => ColorChoice::Auto,
            ColorArg::Always => ColorChoice::Always,
            ColorArg::Never => ColorChoice::Never,
        }
    }
}

pub enum Target<'a> {
    S3 { bucket: &'a str, prefix: &'a str },
}

impl<'a> Target<'a> {
    pub fn parse(s: &'a str) -> Result<Self, anyhow::Error> {
        if let Some(rest) = s.strip_prefix("s3://") {
            let (bucket, prefix) = rest.split_once('/').unwrap_or((rest, ""));
            Ok(Target::S3 { bucket, prefix })
        } else {
            Err(anyhow::anyhow!(
                "target must be of the form s3://bucket/prefix; got {}",
                s
            ))
        }
    }
}
