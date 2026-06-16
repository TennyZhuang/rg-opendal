//! CLI surface.
//!
//! FF1: pattern + s3:// URL + `-i`.
//! FF2: `--glob/-g`, `--type/-t`, `--type-not/-T` over object keys.
//! FF3: `--json`, `--stats`, `-A/-B/-C` context lines.
//! FF4: `--color` output coloring.

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
