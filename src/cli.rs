//! CLI surface.
//!
//! FF1: pattern + s3:// URL + `-i`.
//! FF2: `--glob/-g`, `--type/-t`, `--type-not/-T` over object keys.
//! FF3 (deferred): `--json/--stats/-A/-B/-C`.

use clap::Parser;

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
