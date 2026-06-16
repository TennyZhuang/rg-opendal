//! Library surface of the `rg-opendal` crate.
//!
//! The binary (`src/main.rs`) consumes these modules. Exposing them as a lib
//! lets the internal `benchmarks/seam-a/` harness reuse the same
//! `StreamingBufReader`, `WalkFilter`, and printer implementations without
//! duplicating code.

pub mod cli;
pub mod opendal_io;
pub mod printer;
pub mod walker;
