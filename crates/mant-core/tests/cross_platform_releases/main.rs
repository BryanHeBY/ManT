//! Integration tests for byte-identical manuals in official Windows and Linux
//! toolchain release archives.

#[path = "../common/mod.rs"]
#[allow(dead_code)]
mod common;
mod fixtures;

mod cargo;
mod cmake;
mod rustc;
