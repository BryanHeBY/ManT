//! Integration tests for the Arch Linux fixture corpus.
//!
//! Each page module covers roff bytes extracted from an immutable Arch Linux
//! Archive package and exercises the full libmandoc lowering pipeline.

#[path = "../common/mod.rs"]
#[allow(dead_code)]
mod common;
mod fixtures;

mod archive_entry_stat;
mod bsdunzip;
mod clang;
mod expand_number;
mod gawk;
mod gcc;
mod git;
mod ls;
mod rsync;
mod sh;
mod tar;
mod zip_source_function;
