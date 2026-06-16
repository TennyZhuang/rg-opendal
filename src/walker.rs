//! Serial OpenDAL walker — `list(prefix).recursive(true)` filtered to objects
//! (not directory markers ending in `/`), sorted for deterministic output.
//!
//! Scope per matrix v2.4 + DeepSeek PoC audit (`5edce449`):
//! - PoC-scoped serial walker. Parallel walker is a separate milestone.
//! - `paths.sort()` keeps all keys in memory; acceptable at first-deliverable
//!   scope (small fixture). Streaming walker is a separate milestone.
//! - Cell pin: `verdict=PoC_smoke`, `corpus_kind=code-tree`.
//!
//! FF2 (glob/type): an optional `WalkFilter` is applied per entry before the
//! key is added to the result. Built atop rg's `ignore` crate so semantics
//! match native rg's `--glob` / `--type` exactly.

use anyhow::Result;
use futures::TryStreamExt;
use ignore::overrides::{Override, OverrideBuilder};
use ignore::types::{Types, TypesBuilder};
use opendal::Operator;
use std::path::Path;

/// Filter applied at the walker layer. Built once from CLI flags, queried per
/// listed object key. Kept independent of the OpenDAL service so the same
/// filter applies to S3 / FS / future backends — per Kimi `40f8ba3f`'s
/// capability-gating discipline, this only relies on `list`, which is
/// universal.
pub struct WalkFilter {
    overrides: Option<Override>,
    types: Option<Types>,
    /// True iff at least one positive type alias was selected. Tracked
    /// explicitly because `ignore::Types` does not expose select/negate counts.
    has_positive_types: bool,
}

impl WalkFilter {
    pub fn new(globs: &[String], types: &[String], types_not: &[String]) -> Result<Self> {
        let overrides = if globs.is_empty() {
            None
        } else {
            // Anchor at "/" so globs match against the full key without the
            // walker imposing a working-directory baseline.
            let mut b = OverrideBuilder::new("/");
            for g in globs {
                b.add(g)?;
            }
            Some(b.build()?)
        };

        let has_positive_types = !types.is_empty();

        let types = if types.is_empty() && types_not.is_empty() {
            None
        } else {
            let mut b = TypesBuilder::new();
            b.add_defaults();
            // Positive selections first, then negations. When both are present
            // the ignore crate resolves by glob-set precedence; this ordering
            // matches the natural "select then subtract" intent.
            for t in types {
                b.select(t);
            }
            for t in types_not {
                b.negate(t);
            }
            Some(b.build()?)
        };

        Ok(Self {
            overrides,
            types,
            has_positive_types,
        })
    }

    /// True if the key should be included.
    pub fn matches(&self, key: &str) -> bool {
        // The `ignore` crate's matchers operate on Path. S3 keys are
        // forward-slash separated, which Path handles correctly on Unix.
        let path = Path::new(key);

        if let Some(ov) = &self.overrides {
            // is_dir=false: walker has already filtered out directory markers.
            let m = ov.matched(path, false);
            if m.is_ignore() {
                return false;
            }
            // If any positive globs are present, only whitelisted keys pass;
            // `ignore` returns Whitelist for matches, None for unmatched.
            let has_positive = ov.num_ignores() < ov.num_whitelists() + ov.num_ignores()
                && ov.num_whitelists() > 0;
            if has_positive && !m.is_whitelist() {
                return false;
            }
        }

        if let Some(ts) = &self.types {
            let m = ts.matched(path, false);
            if m.is_ignore() {
                return false;
            }
            // If any positive type aliases were selected, require an explicit
            // whitelist match; otherwise only negated types matter, and those
            // are already handled by is_ignore() above.
            if self.has_positive_types && !m.is_whitelist() {
                return false;
            }
        }

        true
    }
}

/// Returns object keys under `prefix` (recursive), sorted, excluding directory
/// markers, and (if a `WalkFilter` is provided) matching the FF2 glob/type
/// filter. Directory markers in S3 are zero-byte objects whose key ends in `/`.
pub async fn list_objects(
    op: &Operator,
    prefix: &str,
    filter: Option<&WalkFilter>,
) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    let mut lister = op.lister_with(prefix).recursive(true).await?;
    while let Some(entry) = lister.try_next().await? {
        let path = entry.path();
        if path.ends_with('/') {
            continue;
        }
        if let Some(f) = filter {
            if !f.matches(path) {
                continue;
            }
        }
        paths.push(path.to_string());
    }
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_filter_passes_everything() {
        let f = WalkFilter::new(&[], &[], &[]).unwrap();
        assert!(f.matches("a.rs"));
        assert!(f.matches("nested/b.txt"));
    }

    #[test]
    fn positive_glob_filters_to_match() {
        let f = WalkFilter::new(&["*.rs".into()], &[], &[]).unwrap();
        assert!(f.matches("a.rs"));
        assert!(f.matches("deep/nested/lib.rs"));
        assert!(!f.matches("a.txt"));
    }

    #[test]
    fn negative_glob_excludes() {
        let f = WalkFilter::new(&["!target/**".into()], &[], &[]).unwrap();
        assert!(f.matches("src/main.rs"));
        assert!(!f.matches("target/debug/build.log"));
    }

    #[test]
    fn type_alias_filters_rust() {
        let f = WalkFilter::new(&[], &["rust".into()], &[]).unwrap();
        assert!(f.matches("a.rs"));
        assert!(!f.matches("a.txt"));
    }

    #[test]
    fn type_not_excludes_alias() {
        let f = WalkFilter::new(&[], &[], &["rust".into()]).unwrap();
        assert!(!f.matches("a.rs"));
        assert!(f.matches("a.txt"));
    }

    #[test]
    fn type_not_only_does_not_affect_unmatched_files() {
        // Regression guard for DeepSeek review b5a4803a: -T/--type-not alone
        // must exclude the negated alias while leaving all other keys alone.
        let f = WalkFilter::new(&[], &[], &["rust".into()]).unwrap();
        assert!(!f.matches("src/main.rs"));
        assert!(!f.matches("lib.rs"));
        assert!(f.matches("README.md"));
        assert!(f.matches("Cargo.toml"));
        assert!(f.matches("package.json"));
    }

    #[test]
    fn combined_type_and_type_not() {
        let f = WalkFilter::new(&[], &["rust".into()], &["js".into()]).unwrap();
        assert!(f.matches("src/main.rs"));
        assert!(!f.matches("app.js"));
        assert!(!f.matches("README.md"));
    }
}
