//! Printer abstraction that lets `main.rs` switch between rg's standard
//! human-readable output and JSON-lines output without duplicating the search
//! loop.
//!
//! Both backends are built over the same underlying writer type (`NoColor<W>`),
//! so tests can capture output into a `Vec<u8>`.

use std::io::{self, Write};
use std::path::Path;

use grep_matcher::Matcher;
use grep_printer::{JSON, JSONBuilder, JSONSink, Standard, StandardBuilder, StandardSink, Stats};
use grep_searcher::Sink;
use termcolor::NoColor;

/// Active printer. Owns the output writer and produces per-file sinks.
pub enum Printer<W: Write> {
    Standard(Standard<NoColor<W>>),
    Json(JSON<NoColor<W>>),
}

impl<W: Write> Printer<W> {
    /// Build a standard (human-readable) printer.
    ///
    /// `stats` controls whether the printer gathers per-file statistics.
    pub fn standard(wtr: NoColor<W>, stats: bool) -> Self {
        Printer::Standard(
            StandardBuilder::new()
                .heading(false)
                .column(false)
                .stats(stats)
                .build(wtr),
        )
    }

    /// Build a JSON-lines printer.
    pub fn json(wtr: NoColor<W>) -> Self {
        Printer::Json(JSONBuilder::new().build(wtr))
    }

    /// Create a sink for the next file.
    pub fn sink_with_path<'p, 's, M: Matcher>(
        &'s mut self,
        matcher: M,
        path: &'p Path,
    ) -> PrinterSink<'p, 's, M, W> {
        match self {
            Printer::Standard(p) => {
                PrinterSink::Standard(p.sink_with_path(matcher, path))
            }
            Printer::Json(p) => PrinterSink::Json(p.sink_with_path(matcher, path)),
        }
    }
}

/// Per-file sink that delegates to whichever printer backend is active.
pub enum PrinterSink<'p, 's, M: Matcher, W: Write> {
    Standard(StandardSink<'p, 's, M, NoColor<W>>),
    Json(JSONSink<'p, 's, M, NoColor<W>>),
}

impl<'p, 's, M: Matcher, W: Write> PrinterSink<'p, 's, M, W> {
    /// Returns true if the current file contained at least one match.
    pub fn has_match(&self) -> bool {
        match self {
            PrinterSink::Standard(s) => s.has_match(),
            PrinterSink::Json(s) => s.has_match(),
        }
    }

    /// Statistics for the current file, if the backend is tracking them.
    ///
    /// * Standard: only when `.stats(true)` was set on the builder.
    /// * JSON: always tracked.
    pub fn stats(&self) -> Option<&Stats> {
        match self {
            PrinterSink::Standard(s) => s.stats(),
            PrinterSink::Json(s) => Some(s.stats()),
        }
    }
}

impl<'p, 's, M: Matcher, W: Write> Sink for PrinterSink<'p, 's, M, W> {
    type Error = io::Error;

    fn matched(
        &mut self,
        searcher: &grep_searcher::Searcher,
        mat: &grep_searcher::SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        match self {
            PrinterSink::Standard(s) => s.matched(searcher, mat),
            PrinterSink::Json(s) => s.matched(searcher, mat),
        }
    }

    fn context(
        &mut self,
        searcher: &grep_searcher::Searcher,
        context: &grep_searcher::SinkContext<'_>,
    ) -> Result<bool, io::Error> {
        match self {
            PrinterSink::Standard(s) => s.context(searcher, context),
            PrinterSink::Json(s) => s.context(searcher, context),
        }
    }

    fn context_break(
        &mut self,
        searcher: &grep_searcher::Searcher,
    ) -> Result<bool, io::Error> {
        match self {
            PrinterSink::Standard(s) => s.context_break(searcher),
            PrinterSink::Json(s) => s.context_break(searcher),
        }
    }

    fn binary_data(
        &mut self,
        searcher: &grep_searcher::Searcher,
        binary_byte_offset: u64,
    ) -> Result<bool, io::Error> {
        match self {
            PrinterSink::Standard(s) => s.binary_data(searcher, binary_byte_offset),
            PrinterSink::Json(s) => s.binary_data(searcher, binary_byte_offset),
        }
    }

    fn begin(
        &mut self,
        searcher: &grep_searcher::Searcher,
    ) -> Result<bool, io::Error> {
        match self {
            PrinterSink::Standard(s) => s.begin(searcher),
            PrinterSink::Json(s) => s.begin(searcher),
        }
    }

    fn finish(
        &mut self,
        searcher: &grep_searcher::Searcher,
        finish: &grep_searcher::SinkFinish,
    ) -> Result<(), io::Error> {
        match self {
            PrinterSink::Standard(s) => s.finish(searcher, finish),
            PrinterSink::Json(s) => s.finish(searcher, finish),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grep_regex::RegexMatcher;
    use grep_searcher::SearcherBuilder;
    use termcolor::NoColor;

    #[test]
    fn standard_printer_tracks_matches_and_stats() {
        let buf = Vec::new();
        let mut printer = Printer::standard(NoColor::new(buf), true);
        let matcher = RegexMatcher::new("foo").unwrap();
        let mut searcher = SearcherBuilder::new().line_number(true).build();
        let data = b"line one\nline two foo\nline three\n";

        let mut sink = printer.sink_with_path(&matcher, Path::new("test.txt"));
        searcher
            .search_reader(&matcher, &data[..], &mut sink)
            .unwrap();
        assert!(sink.has_match());

        let stats = sink.stats().unwrap();
        assert_eq!(stats.matches(), 1);
        assert_eq!(stats.matched_lines(), 1);
        assert_eq!(stats.searches(), 1);
        assert_eq!(stats.searches_with_match(), 1);
        assert_eq!(stats.bytes_searched(), data.len() as u64);
    }

    #[test]
    fn json_printer_emits_machine_readable_lines() {
        let buf = Vec::new();
        let mut printer = Printer::json(NoColor::new(buf));
        let matcher = RegexMatcher::new("foo").unwrap();
        let mut searcher = SearcherBuilder::new().line_number(true).build();
        let data = b"line one\nline two foo\n";

        {
            let mut sink = printer.sink_with_path(&matcher, Path::new("test.txt"));
            searcher
                .search_reader(&matcher, &data[..], &mut sink)
                .unwrap();
            assert!(sink.has_match());
        }

        let output = match printer {
            Printer::Json(p) => String::from_utf8(p.into_inner().into_inner()).unwrap(),
            Printer::Standard(_) => panic!("expected JSON printer"),
        };
        // JSON-lines format: each line is a JSON object with a "type" key.
        assert!(!output.is_empty());
        for line in output.lines() {
            assert!(line.starts_with('{'));
            assert!(line.contains("\"type\""));
        }
    }

    #[test]
    fn context_lines_are_reported_by_standard_printer() {
        let buf = Vec::new();
        let mut printer = Printer::standard(NoColor::new(buf), false);
        let matcher = RegexMatcher::new("foo").unwrap();
        let mut searcher = SearcherBuilder::new()
            .line_number(true)
            .after_context(1)
            .before_context(1)
            .build();
        let data = b"line one\nline two foo\nline three\n";

        {
            let mut sink = printer.sink_with_path(&matcher, Path::new("test.txt"));
            searcher
                .search_reader(&matcher, &data[..], &mut sink)
                .unwrap();
        }

        let output = match printer {
            Printer::Standard(p) => String::from_utf8(p.into_inner().into_inner()).unwrap(),
            Printer::Json(_) => panic!("expected standard printer"),
        };
        // Match line plus one before and one after context line.
        assert!(output.contains("line one"));
        assert!(output.contains("line two foo"));
        assert!(output.contains("line three"));
    }
}
