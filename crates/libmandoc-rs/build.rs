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

use std::{collections::HashSet, env, fmt::Write as _, fs, path::PathBuf};

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

const RENDER_SOURCES: &[&str] = &[
    "out.c",
    "term.c",
    "term_ascii.c",
    "term_tab.c",
    "roff_term.c",
    "roff_html.c",
    "man_term.c",
    "mdoc_term.c",
    "tbl_term.c",
    "eqn_term.c",
    "html.c",
    "man_html.c",
    "mdoc_html.c",
    "tbl_html.c",
    "eqn_html.c",
];

fn main() {
    let crate_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let vendor_dir = crate_dir.join("vendor/mandoc-1.14.6");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("target operating system");
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let memory_only = target_os == "windows";
    let thread_sanitizer = env::var_os("LIBMANDOC_RS_TSAN").is_some();
    let address_sanitizer = env::var_os("LIBMANDOC_RS_ASAN").is_some();
    let deny_native_warnings = env::var_os("LIBMANDOC_RS_DENY_WARNINGS").is_some();
    let render = env::var_os("CARGO_FEATURE_RENDER").is_some();
    let (config, compat_sources) = target_configuration(&target_os, &target_env);

    assert!(
        !(thread_sanitizer && address_sanitizer),
        "libmandoc-rs cannot enable ThreadSanitizer and AddressSanitizer together"
    );
    assert!(
        !thread_sanitizer || matches!(target_os.as_str(), "linux" | "macos"),
        "libmandoc-rs ThreadSanitizer instrumentation supports Linux and macOS targets"
    );
    assert!(
        !address_sanitizer || matches!(target_os.as_str(), "linux" | "macos"),
        "libmandoc-rs AddressSanitizer instrumentation supports Linux and macOS targets"
    );

    fs::copy(crate_dir.join(config), out_dir.join("config.h"))
        .expect("copy checked mandoc target configuration");
    generate_special_character_table(&vendor_dir, &out_dir);

    let mut build = cc::Build::new();
    build
        .include(&out_dir)
        .include(&vendor_dir)
        .include(crate_dir.join("config"))
        .include(crate_dir.join("shim"))
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
        .flag_if_supported("-Wno-unused-parameter");
    if deny_native_warnings {
        // Project verification opts into a warning-free native boundary on
        // every supported compiler. Do not impose -Werror on ordinary
        // downstream builds, where a newer compiler could add diagnostics
        // independently of this crate release.
        build.warnings_into_errors(true);
    }
    if thread_sanitizer {
        // Rust's sanitizer flag does not instrument the separately compiled
        // vendored C parser. Keep this explicit opt-in paired with the local
        // runner so a passing test covers both sides of the FFI boundary.
        build
            .flag("-fsanitize=thread")
            .flag("-fno-omit-frame-pointer")
            .flag("-g");
    }
    if address_sanitizer {
        // Match Rust's opt-in AddressSanitizer build so reads in the vendored
        // C parser are checked at the FFI boundary too.
        build
            .flag("-fsanitize=address")
            .flag("-fno-omit-frame-pointer")
            .flag("-g");
    }
    if !memory_only {
        // The local libmandoc patch uses C11 thread-local storage for the
        // parser's mutable static state. Windows/MSVC uses its native static
        // TLS spelling and does not need this language-mode flag.
        build.flag_if_supported("-std=c11");
    }
    if memory_only {
        build.define("MANDOC_MEMORY_ONLY", None);
    } else {
        // Only read.c calls open() in the selected parser sources. Redirecting
        // it avoids a process-wide chdir while preserving source-relative .so.
        build.define("open", "mant_mandoc_source_open");
    }

    let mut upstream_sources = LIBMANDOC_SOURCES
        .iter()
        .chain(compat_sources.iter())
        .map(|source| vendor_dir.join(source))
        .collect::<Vec<_>>();
    if render {
        build.define("MANT_MANDOC_RENDER", None);
        upstream_sources.extend(RENDER_SOURCES.iter().map(|source| vendor_dir.join(source)));
    }
    let mut owned_sources = Vec::new();
    if render {
        owned_sources.push(crate_dir.join("shim/mant_mandoc_output.c"));
    }
    if memory_only {
        owned_sources.push(crate_dir.join("shim/windows_compat.c"));
    }
    owned_sources.push(crate_dir.join("shim/mant_mandoc_shim.c"));

    compile_native_archive(
        build,
        &upstream_sources,
        &owned_sources,
        deny_native_warnings && target_env == "msvc",
    );

    if !memory_only {
        // Unix native-file parsing retains libmandoc's gzip transport.
        println!("cargo:rustc-link-lib=z");
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=LIBMANDOC_RS_DENY_WARNINGS");
    println!("cargo:rerun-if-env-changed=LIBMANDOC_RS_TSAN");
    // Target selection lives here and is pulled in via #[path]; Cargo does not
    // discover that dependency, so track it explicitly or edits to the config
    // map would reuse a stale config.h and source list on incremental builds.
    println!("cargo:rerun-if-changed=src/build_config.rs");
    println!("cargo:rerun-if-changed=config");
    println!("cargo:rerun-if-changed=shim");
    println!("cargo:rerun-if-changed={}", vendor_dir.display());
    println!("cargo:rerun-if-env-changed=LIBMANDOC_RS_ASAN");
}

fn compile_native_archive(
    mut build: cc::Build,
    upstream_sources: &[PathBuf],
    owned_sources: &[PathBuf],
    strict_msvc: bool,
) {
    if strict_msvc {
        // Pinned upstream 1.14.6 uses unused callback parameters and POSIX-
        // sized integer conversions that MSVC diagnoses much more broadly
        // than GCC or Clang. Compile that immutable source group with an
        // explicit five-warning baseline, while compiling every ManT-owned
        // translation unit separately under the full /WX policy. Archive the
        // resulting objects together to retain the existing link boundary.
        let mut upstream_build = build.clone();
        upstream_build
            .flag("/wd4100")
            .flag("/wd4146")
            .flag("/wd4200")
            .flag("/wd4244")
            .flag("/wd4267")
            .files(upstream_sources);
        let mut objects = upstream_build.compile_intermediates();

        let mut owned_build = build;
        owned_build.files(owned_sources);
        objects.extend(owned_build.compile_intermediates());

        cc::Build::new().objects(objects).compile("mant_mandoc");
    } else {
        build.files(upstream_sources).files(owned_sources);
        build.compile("mant_mandoc");
    }
}

/// Generate the Rust lookup from the same pinned table compiled into
/// libmandoc. Keeping one source of truth prevents parser upgrades from
/// silently leaving the higher-level text projection behind.
fn generate_special_character_table(vendor_dir: &std::path::Path, out_dir: &std::path::Path) {
    let source = fs::read_to_string(vendor_dir.join("chars.c"))
        .expect("read pinned mandoc character catalog");
    let mut entries = Vec::new();
    let mut names = HashSet::new();

    for line in source.lines().map(str::trim) {
        if !line.starts_with("{ \"") {
            continue;
        }
        let (name, remainder) = parse_c_string(&line[2..])
            .unwrap_or_else(|| panic!("invalid character name in chars.c: {line}"));
        let fields = remainder
            .strip_prefix(',')
            .unwrap_or_else(|| panic!("missing character fields in chars.c: {line}"));
        let codepoint = fields
            .strip_suffix(',')
            .and_then(|fields| fields.strip_suffix('}'))
            .and_then(|fields| fields.rsplit(',').next())
            .map(str::trim)
            .and_then(parse_c_integer)
            .unwrap_or_else(|| panic!("invalid Unicode value in chars.c: {line}"));
        assert!(
            names.insert(name.clone()),
            "duplicate roff character {name}"
        );
        entries.push((name, codepoint));
    }

    assert!(
        entries.len() >= 300,
        "pinned mandoc character catalog unexpectedly contains only {} entries",
        entries.len()
    );
    entries.sort_unstable_by(|left, right| left.0.cmp(&right.0));

    let mut generated = String::from(
        "// Generated from vendor/mandoc-1.14.6/chars.c; do not edit.\n\
         const CATALOG: &[(&str, u32)] = &[\n",
    );
    for (name, codepoint) in entries {
        assert!(
            codepoint == 0 || char::from_u32(codepoint).is_some(),
            "invalid Unicode scalar U+{codepoint:04X} for {name}"
        );
        writeln!(generated, "    ({name:?}, 0x{codepoint:X}),")
            .expect("write generated character entry");
    }
    generated.push_str(
        "];\n\
         pub(super) fn lookup(name: &str) -> Option<u32> {\n\
             CATALOG\n\
                 .binary_search_by_key(&name, |(candidate, _)| *candidate)\n\
                 .ok()\n\
                 .map(|index| CATALOG[index].1)\n\
         }\n",
    );
    fs::write(out_dir.join("special_characters.rs"), generated)
        .expect("write generated mandoc character catalog");
}

fn parse_c_string(source: &str) -> Option<(String, &str)> {
    let mut characters = source.char_indices();
    if characters.next()?.1 != '"' {
        return None;
    }

    let mut output = String::new();
    while let Some((index, character)) = characters.next() {
        match character {
            '"' => return Some((output, &source[index + 1..])),
            '\\' => {
                let (_, escaped) = characters.next()?;
                output.push(match escaped {
                    '\\' => '\\',
                    '"' => '"',
                    '\'' => '\'',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    _ => return None,
                });
            }
            _ => output.push(character),
        }
    }
    None
}

fn parse_c_integer(source: &str) -> Option<u32> {
    source.strip_prefix("0x").map_or_else(
        || source.parse().ok(),
        |hex| u32::from_str_radix(hex, 16).ok(),
    )
}
