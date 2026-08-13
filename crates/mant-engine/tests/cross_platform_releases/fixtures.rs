//! Document loaders for the cross-platform release fixture corpus.

use std::{path::PathBuf, sync::OnceLock};

use mant_engine::ResolvedContent;
use mant_engine::parse_manual_source;
use mant_ir::Document;

use crate::common::query_for_document;

static CARGO: OnceLock<Document> = OnceLock::new();
static RUSTC: OnceLock<Document> = OnceLock::new();
static CMAKE_TOOLCHAINS: OnceLock<Document> = OnceLock::new();

pub fn cross_platform_release_manual(name: &str) -> &'static Document {
    let slot = match name {
        "cargo" => &CARGO,
        "rustc" => &RUSTC,
        "cmake-toolchains" => &CMAKE_TOOLCHAINS,
        _ => panic!("unknown cross-platform release fixture {name}"),
    };
    slot.get_or_init(|| {
        parse_manual_source(&cross_platform_release_fixture_path(name))
            .unwrap_or_else(|error| panic!("parse cross-platform release {name} fixture: {error}"))
    })
}

pub fn cross_platform_release_query(name: &str) -> ResolvedContent {
    query_for_document(name, cross_platform_release_manual(name))
}

pub fn cross_platform_release_fixture_path(name: &str) -> PathBuf {
    let file = match name {
        "cargo" | "rustc" => format!("{name}.1.zst"),
        "cmake-toolchains" => "cmake-toolchains.7.zst".to_owned(),
        _ => panic!("unknown cross-platform release fixture {name}"),
    };
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/roff/real/cross-platform-releases")
        .join(file)
}
