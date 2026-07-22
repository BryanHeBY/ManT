//! Statics, path functions, and document loaders for the Debian sid gzip
//! fixture corpus.

use std::{path::PathBuf, sync::OnceLock};

use mant_ast::MantDocument;
use mant_core::parse_manual_source;

static MT_GNU: OnceLock<MantDocument> = OnceLock::new();
static GROFF_ME: OnceLock<MantDocument> = OnceLock::new();
static GROFF_MAN_STYLE: OnceLock<MantDocument> = OnceLock::new();

pub fn debian_manual(name: &str) -> &'static MantDocument {
    let slot = match name {
        "mt-gnu" => &MT_GNU,
        "groff_me" => &GROFF_ME,
        "groff_man_style" => &GROFF_MAN_STYLE,
        _ => panic!("unknown Debian fixture {name}"),
    };
    slot.get_or_init(|| {
        parse_manual_source(&debian_fixture_path(name))
            .unwrap_or_else(|error| panic!("parse Debian {name} fixture: {error}"))
    })
}

pub fn debian_fixture_path(name: &str) -> PathBuf {
    let section = match name {
        "mt-gnu" => "1",
        "groff_me" | "groff_man_style" => "7",
        _ => panic!("unknown Debian fixture {name}"),
    };
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/roff/real/debian")
        .join(format!("{name}.{section}.gz"))
}
