//! Integration tests for the Arch Linux gzip fixture corpus.
//!
//! Each page module (`ls`, `git`, `gcc`, `clang`, `tar`) covers a single
//! compressed roff member extracted from an immutable Arch Linux Archive
//! package and exercises the full libmandoc lowering pipeline against it.

#[path = "../common/mod.rs"]
#[allow(dead_code)]
mod common;
mod fixtures;

mod clang;
mod gcc;
mod git;
mod ls;
mod tar;
