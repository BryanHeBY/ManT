//! Loaders for the immutable BSD closure fixtures.

use std::{path::PathBuf, sync::OnceLock};

use mant_engine::parse_manual_source;
use mant_ir::Document;

static NETBSD_DRM: OnceLock<Document> = OnceLock::new();
static DRAGONFLY_ADDUSER: OnceLock<Document> = OnceLock::new();
static DRAGONFLY_GDB: OnceLock<Document> = OnceLock::new();
static OPENBSD_CURRENT_TERM: OnceLock<Document> = OnceLock::new();

pub fn bsd_manual(name: &str) -> &'static Document {
    let slot = match name {
        "netbsd-drm" => &NETBSD_DRM,
        "dragonfly-adduser" => &DRAGONFLY_ADDUSER,
        "dragonfly-gdb" => &DRAGONFLY_GDB,
        "openbsd-current-term" => &OPENBSD_CURRENT_TERM,
        _ => panic!("unknown BSD closure fixture {name}"),
    };
    slot.get_or_init(|| {
        parse_manual_source(&fixture_path(name))
            .unwrap_or_else(|error| panic!("parse BSD closure fixture {name}: {error}"))
    })
}

fn fixture_path(name: &str) -> PathBuf {
    let file = match name {
        "netbsd-drm" => "netbsd-drm.4",
        "dragonfly-adduser" => "dragonfly-adduser.8.gz",
        "dragonfly-gdb" => "dragonfly-gdb.1.gz",
        "openbsd-current-term" => "openbsd-current-term.5",
        _ => panic!("unknown BSD closure fixture {name}"),
    };
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/roff/real/bsd-closure")
        .join(file)
}
