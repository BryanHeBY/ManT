//! Integration tests for the Arch Linux fixture corpus.
//!
//! Each page module covers roff bytes extracted from an immutable Arch Linux
//! Archive package and exercises the full libmandoc lowering pipeline.

#[path = "../common/mod.rs"]
#[allow(dead_code)]
mod common;
mod fixtures;

mod clang;
mod gawk;
mod gcc;
mod git;
mod ls;
mod rsync;
mod tar;
