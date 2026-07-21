//! Build the parsing subset of the pinned mandoc source tree.
//!
//! The vendored source at `vendor/mandoc-1.14.6/` is a pre-patched snapshot
//! maintained by `scripts/sync-vendor`.  See `upstream/SOURCE` for provenance
//! and `patches/series` for any local modifications.
//!
//! Upstream's `configure` script probes the build host by compiling and
//! executing binaries. That is useful for a system installation, but it makes
//! cross compilation non-deterministic. `ManT` instead checks in the small
//! target-family configurations that its release matrix supports.

use std::{env, fs, path::PathBuf};

#[path = "src/build_config.rs"]
mod build_config;

use build_config::target_configuration;

const LIBMANDOC_SOURCES: &[&str] = &[
    "man.c",
    "man_macro.c",
    "man_validate.c",
    "att.c",
    "lib.c",
    "mdoc.c",
    "mdoc_argv.c",
    "mdoc_macro.c",
    "mdoc_state.c",
    "mdoc_validate.c",
    "st.c",
    "eqn.c",
    "roff.c",
    "roff_validate.c",
    "tbl.c",
    "tbl_data.c",
    "tbl_layout.c",
    "tbl_opts.c",
    "arch.c",
    "chars.c",
    "mandoc.c",
    "mandoc_aux.c",
    "mandoc_msg.c",
    "mandoc_ohash.c",
    "mandoc_xr.c",
    "msec.c",
    "preconv.c",
    "read.c",
    "tag.c",
];

fn main() {
    let crate_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let vendor_dir = crate_dir.join("vendor/mandoc-1.14.6");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("target operating system");
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let (config, compat_sources) = target_configuration(&target_os, &target_env);

    fs::copy(crate_dir.join(config), out_dir.join("config.h"))
        .expect("copy checked mandoc target configuration");

    let mut build = cc::Build::new();
    build
        .include(&out_dir)
        .include(&vendor_dir)
        .warnings(true)
        .flag_if_supported("-W")
        .flag_if_supported("-Wmissing-prototypes")
        .flag_if_supported("-Wstrict-prototypes")
        .flag_if_supported("-Wwrite-strings")
        .flag_if_supported("-Wno-discarded-qualifiers")
        // GCC's optimizer reports a false positive in pinned upstream roff.c
        // on every incremental Cargo invocation. Clang ignores this through
        // flag_if_supported, while GCC development output remains readable.
        .flag_if_supported("-Wno-maybe-uninitialized")
        .flag_if_supported("-Wno-unused-parameter")
        // Only read.c calls open() in the selected parser sources. Redirecting
        // it avoids a process-wide chdir while preserving source-relative .so.
        .define("open", "mant_mandoc_source_open");

    for source in LIBMANDOC_SOURCES.iter().chain(compat_sources.iter()) {
        build.file(vendor_dir.join(source));
    }
    build.file(crate_dir.join("shim/mant_mandoc_shim.c"));
    build.compile("mant_mandoc");

    // read.c transparently handles compressed manual sources through zlib.
    println!("cargo:rustc-link-lib=z");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=config");
    println!("cargo:rerun-if-changed=shim");
    println!("cargo:rerun-if-changed={}", vendor_dir.display());
}
