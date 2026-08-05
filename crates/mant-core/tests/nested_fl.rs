#![cfg(unix)]

//! Regression test for nested `Fl` lowering.
//!
//! mdoc spells `--long` options as `.Fl Fl long`: each `Fl` contributes one
//! dash. An earlier guard suppressed the outer dash whenever the lowered
//! children already started with `-`, collapsing every `--option` in mdoc
//! manuals (such as bsdtar's `--acls`) to `-option`.

use std::path::PathBuf;

use mant_core::parse_manual_source;

#[path = "common/mod.rs"]
#[allow(dead_code)]
mod common;

#[test]
fn nested_fl_macros_produce_double_dash_options() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/roff/nested-fl-mdoc.1");
    let document = parse_manual_source(&path).expect("parse nested-fl fixture");

    let options = common::section(&document, "OPTIONS");
    let terms: Vec<String> = common::definition_items(options)
        .iter()
        .map(|item| common::inline_text(&item.terms[0]))
        .collect();

    assert_eq!(terms, ["-a, --acls", "--no-acls", "-v"]);
}
