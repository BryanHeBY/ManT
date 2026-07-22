//! Statics, path functions, and document loaders for the Arch Linux gzip
//! fixture corpus.

use std::{path::PathBuf, sync::OnceLock};

use mant_ast::{MantDocument, QueryBundle};
use mant_core::parse_manual_source;

use crate::common::query_for_document;

static LS: OnceLock<MantDocument> = OnceLock::new();
static GIT: OnceLock<MantDocument> = OnceLock::new();
static GCC: OnceLock<MantDocument> = OnceLock::new();
static CLANG: OnceLock<MantDocument> = OnceLock::new();
static TAR: OnceLock<MantDocument> = OnceLock::new();

pub fn archlinux_manual(name: &str) -> &'static MantDocument {
    let slot = match name {
        "ls" => &LS,
        "git" => &GIT,
        "gcc" => &GCC,
        "clang" => &CLANG,
        "tar" => &TAR,
        _ => panic!("unknown Arch Linux fixture {name}"),
    };
    slot.get_or_init(|| {
        parse_manual_source(&archlinux_fixture_path(name))
            .unwrap_or_else(|error| panic!("parse Arch Linux {name} fixture: {error}"))
    })
}

pub fn archlinux_manual_query(name: &str) -> QueryBundle {
    query_for_document(name, archlinux_manual(name))
}

pub fn archlinux_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/roff/real/archlinux")
        .join(format!("{name}.1.gz"))
}
