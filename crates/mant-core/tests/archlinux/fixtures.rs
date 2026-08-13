//! Statics, path functions, and document loaders for the Arch Linux gzip
//! fixture corpus.

use std::{path::PathBuf, sync::OnceLock};

use mant_core::ResolvedContent;
use mant_core::parse_manual_source;
use mant_ir::Document;

use crate::common::query_for_document;

static LS: OnceLock<Document> = OnceLock::new();
static GIT: OnceLock<Document> = OnceLock::new();
static GCC: OnceLock<Document> = OnceLock::new();
static CLANG: OnceLock<Document> = OnceLock::new();
static GAWK: OnceLock<Document> = OnceLock::new();
static RSYNC: OnceLock<Document> = OnceLock::new();
static TAR: OnceLock<Document> = OnceLock::new();
static SH: OnceLock<Document> = OnceLock::new();

pub fn archlinux_manual(name: &str) -> &'static Document {
    let slot = match name {
        "ls" => &LS,
        "git" => &GIT,
        "gcc" => &GCC,
        "clang" => &CLANG,
        "gawk" => &GAWK,
        "rsync" => &RSYNC,
        "tar" => &TAR,
        "sh" => &SH,
        _ => panic!("unknown Arch Linux fixture {name}"),
    };
    slot.get_or_init(|| {
        parse_manual_source(&archlinux_fixture_path(name))
            .unwrap_or_else(|error| panic!("parse Arch Linux {name} fixture: {error}"))
    })
}

pub fn archlinux_manual_query(name: &str) -> ResolvedContent {
    query_for_document(name, archlinux_manual(name))
}

pub fn archlinux_fixture_path(name: &str) -> PathBuf {
    if name == "sh" {
        return PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("tests/fixtures/roff/real/archlinux/sh.1p.gz");
    }
    let extension = match name {
        "gawk" | "rsync" => "zst",
        _ => "gz",
    };
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/roff/real/archlinux")
        .join(format!("{name}.1.{extension}"))
}
