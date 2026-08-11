//! Document loaders for the official Windows release fixture corpus.

use std::{path::PathBuf, sync::OnceLock};

use mant_ast::{MantDocument, QueryBundle};
use mant_core::parse_manual_source;

use crate::common::query_for_document;

static RG: OnceLock<MantDocument> = OnceLock::new();
static RCLONE: OnceLock<MantDocument> = OnceLock::new();

pub fn windows_release_manual(name: &str) -> &'static MantDocument {
    let slot = match name {
        "rg" => &RG,
        "rclone" => &RCLONE,
        _ => panic!("unknown official Windows release fixture {name}"),
    };
    slot.get_or_init(|| {
        parse_manual_source(&windows_release_fixture_path(name)).unwrap_or_else(|error| {
            panic!("parse official Windows release {name} fixture: {error}")
        })
    })
}

pub fn windows_release_query(name: &str) -> QueryBundle {
    query_for_document(name, windows_release_manual(name))
}

pub fn windows_release_fixture_path(name: &str) -> PathBuf {
    match name {
        "rg" | "rclone" => {}
        _ => panic!("unknown official Windows release fixture {name}"),
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/roff/real/windows-releases")
        .join(format!("{name}.1.zst"))
}
