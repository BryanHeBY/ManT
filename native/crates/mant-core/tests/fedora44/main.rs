//! Integration tests for the Fedora Linux 44 zstd fixture corpus.
//!
//! These tests exercise `ManT`'s in-process zstd decoder and verify
//! independently packaged generator output against semantic assertions
//! (section counts, option outlines, metadata, search).

#[path = "../common/mod.rs"]
#[allow(dead_code)]
mod common;
mod fixtures;

mod clang;
mod gcc;
mod git;
mod tar;
