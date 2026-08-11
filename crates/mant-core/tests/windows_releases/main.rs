//! Integration tests for manuals shipped in official Windows release ZIPs.
//!
//! The corpus preserves the upstream CRLF bytes after decompression and runs
//! on every CI host, including native Windows.

#[path = "../common/mod.rs"]
#[allow(dead_code)]
mod common;
mod fixtures;

mod npm;
mod rclone;
mod rg;
mod scan_build;
