//! Statics, path functions, and document loaders for the Fedora Linux 44
//! zstd fixture corpus.

use std::{path::PathBuf, sync::OnceLock};

use mant_ast::MantDocument;
use mant_core::parse_manual_source;

static CLANG: OnceLock<MantDocument> = OnceLock::new();
static GCC: OnceLock<MantDocument> = OnceLock::new();
static GIT: OnceLock<MantDocument> = OnceLock::new();
static TAR: OnceLock<MantDocument> = OnceLock::new();

pub fn fedora44_manual(name: &str) -> &'static MantDocument {
    let slot = match name {
        "clang" => &CLANG,
        "gcc" => &GCC,
        "git" => &GIT,
        "tar" => &TAR,
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
