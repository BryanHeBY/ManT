//! Document loaders for the official Windows release fixture corpus.

use std::{path::PathBuf, sync::OnceLock};

use mant_core::ResolvedContent;
use mant_core::parse_manual_source;
use mant_ir::Document;

use crate::common::query_for_document;

static RG: OnceLock<Document> = OnceLock::new();
static RCLONE: OnceLock<Document> = OnceLock::new();
static NPM: OnceLock<Document> = OnceLock::new();
static SCAN_BUILD: OnceLock<Document> = OnceLock::new();

pub fn windows_release_manual(name: &str) -> &'static Document {
    let slot = match name {
        "rg" => &RG,
        "rclone" => &RCLONE,
        "npm" => &NPM,
        "scan-build" => &SCAN_BUILD,
        _ => panic!("unknown official Windows release fixture {name}"),
    };
    slot.get_or_init(|| {
        parse_manual_source(&windows_release_fixture_path(name)).unwrap_or_else(|error| {
            panic!("parse official Windows release {name} fixture: {error}")
        })
    })
}

pub fn windows_release_query(name: &str) -> ResolvedContent {
    query_for_document(name, windows_release_manual(name))
}

pub fn windows_release_fixture_path(name: &str) -> PathBuf {
    match name {
        "rg" | "rclone" | "npm" | "scan-build" => {}
        _ => panic!("unknown official Windows release fixture {name}"),
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/roff/real/windows-releases")
        .join(format!("{name}.1.zst"))
}
