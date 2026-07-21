//! Integration tests for the Debian sid gzip fixture corpus.
//!
//! This directory adds the first section-7 fixtures (groff macro reference
//! pages) and a section-1 page from cpio that exercises escape-heavy man(7)
//! option lists with libmandoc-generated anchor tags.

#[path = "../common/mod.rs"]
#[allow(dead_code)]
mod common;
mod fixtures;

mod groff_man_style;
mod groff_me;
mod mt_gnu;
