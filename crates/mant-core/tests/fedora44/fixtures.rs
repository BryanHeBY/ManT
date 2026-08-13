//! Statics, path functions, and document loaders for the Fedora Linux 44
//! zstd fixture corpus.

use std::{path::PathBuf, sync::OnceLock};

use mant_core::parse_manual_source;
use mant_ir::Document;

static CLANG: OnceLock<Document> = OnceLock::new();
static GCC: OnceLock<Document> = OnceLock::new();
static GIT: OnceLock<Document> = OnceLock::new();
static TAR: OnceLock<Document> = OnceLock::new();
static SH: OnceLock<Document> = OnceLock::new();

pub fn fedora44_manual(name: &str) -> &'static Document {
    let slot = match name {
        "clang" => &CLANG,
        "gcc" => &GCC,
        "git" => &GIT,
        "tar" => &TAR,
        "sh" => &SH,
        _ => panic!("unknown Fedora Linux 44 fixture {name}"),
    };
    slot.get_or_init(|| {
        parse_manual_source(&fedora44_fixture_path(name))
            .unwrap_or_else(|error| panic!("parse Fedora Linux 44 {name} fixture: {error}"))
    })
}

pub fn fedora44_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/roff/real/fedora44")
        .join(format!("{name}.1.zst"))
}
