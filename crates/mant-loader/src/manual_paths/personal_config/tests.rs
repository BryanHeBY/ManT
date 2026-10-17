use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use super::{
    ConfigBudget, collect_include_paths_with, deduplicate_windows_paths, materialize, parse,
    parse_bounded, read_config, split_arguments, windows_paths_equivalent,
};
use crate::manual_paths::{MAX_EXPANDED_CONFIG_CANDIDATES, MAX_EXPANDED_CONFIG_PATHS};

#[test]
fn windows_directives_reject_incomplete_roots_and_device_namespaces() {
    let environment = HashMap::new();
    for value in [
        r"C:manuals",
        r"\manuals",
        "/manuals",
        r"\\server",
        r"\\server\",
        r"\\.\pipe\mant",
        r"\\?\C:/manuals",
    ] {
        let text = format!(
            "manpath \"{value}\"\nMANDATORY_MANPATH \"{value}\"\nMANCONFIG \"{value}\"\nMANPATH_MAP C:\\bin \"{value}\"\n"
        );
        let plan = parse(&text, Path::new("man.conf"), &environment, true);
        assert_eq!(plan.roots, [] as [PathBuf; 0]);
        assert_eq!(plan.mandatory, [] as [PathBuf; 0]);
        assert_eq!(plan.include_patterns, [] as [PathBuf; 0]);
        assert_eq!(plan.mappings, [] as [(PathBuf, PathBuf); 0]);
        assert_eq!(plan.diagnostics.len(), 4, "{value}");
    }
    for value in [
        r"\\?\C:\manuals",
        r"\\?\UNC\server\share\manuals",
        "//server/share/manuals",
    ] {
        let plan = parse(
            &format!("manpath \"{value}\"\n"),
            Path::new("man.conf"),
            &environment,
            true,
        );
        assert_eq!(plan.roots, [PathBuf::from(value)]);
        assert_eq!(plan.diagnostics, []);
    }
}

#[test]
fn windows_maps_and_deduplication_preserve_roots_and_namespace_boundaries() {
    assert!(windows_paths_equivalent(
        Path::new(r"C:\bin"),
        Path::new("c:/other/../bin/")
    ));
    assert!(!windows_paths_equivalent(
        Path::new(r"C:\"),
        Path::new("C:")
    ));
    assert!(!windows_paths_equivalent(
        Path::new(r"C:\bin"),
        Path::new(" C:/bin")
    ));
    let roots = deduplicate_windows_paths(
        [
            r"C:\",
            "c:/./",
            r"\\server\share",
            "//server/share/",
            r"C:\docs",
            "c:/other/../docs/",
            r"\\?\C:\docs",
        ]
        .map(PathBuf::from)
        .to_vec(),
    );
    assert_eq!(
        roots,
        [r"C:\", r"\\server\share", r"C:\docs", r"\\?\C:\docs"].map(PathBuf::from)
    );
}

#[cfg(windows)]
#[test]
fn windows_maps_retain_ordinary_relative_path_environment_entries() {
    assert!(windows_paths_equivalent(
        &std::env::current_dir().unwrap(),
        Path::new(".")
    ));
    assert!(!windows_paths_equivalent(
        &std::env::current_dir().unwrap(),
        Path::new("C:")
    ));
}

#[test]
fn single_paths_preserve_spaces_and_accept_one_optional_double_quote_pair() {
    let environment = HashMap::from([(
        OsString::from("ProgramFiles"),
        OsString::from(r"C:\Program Files"),
    )]);
    let plan = parse(
        "manpath C:\\Manual Trees\\plain\nMANPATH \"%PROGRAMFILES%\\Tool\\man\"\n",
        PathBuf::from(r"C:\config\man.conf").as_path(),
        &environment,
        true,
    );
    assert_eq!(
        plan.roots,
        vec![
            PathBuf::from(r"C:\Manual Trees\plain"),
            PathBuf::from(r"C:\Program Files\Tool\man")
        ]
    );
    assert_eq!(plan.diagnostics.len(), 0);
}

#[test]
fn environment_expansion_is_case_insensitive_single_pass_and_supports_literal_percent() {
    let environment = HashMap::from([
        (OsString::from("ROOT"), OsString::from(r"D:\Manuals")),
        (OsString::from("NESTED"), OsString::from("%ROOT%")),
    ]);
    let plan = parse(
        "manpath %root%\\one\nmanpath C:\\100%%\\man\nmanpath %NESTED%\\two\n",
        PathBuf::from(r"C:\config\man.conf").as_path(),
        &environment,
        true,
    );
    assert_eq!(
        plan.roots,
        vec![
            PathBuf::from(r"D:\Manuals\one"),
            PathBuf::from(r"C:\100%\man")
        ]
    );
    assert_eq!(plan.diagnostics.len(), 1);
}

#[test]
fn personal_toml_and_man_conf_use_identical_authored_expansion() {
    use std::collections::BTreeMap;
    for windows in [false, true] {
        let home = if windows {
            r"C:\Users\Alice"
        } else {
            "/home/alice"
        };
        let environment = HashMap::from([
            (OsString::from("HOME"), OsString::from(home)),
            (OsString::from("ROOT"), OsString::from(home)),
            (OsString::from("LITERAL"), OsString::from("%ROOT%")),
        ]);
        let settings_environment = environment
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<BTreeMap<_, _>>();
        let settings =
            mant_sources::settings::Settings::from_environment(&settings_environment, windows)
                .unwrap();
        for value in [
            "~",
            "~/manuals",
            r"~\manuals",
            "%ROOT%/manuals",
            "~/%LITERAL%/100%%",
        ] {
            let plan = parse_bounded(
                &format!("manpath \"{value}\"\n"),
                Path::new("man.conf"),
                &environment,
                true,
                &mut ConfigBudget {
                    windows,
                    ..ConfigBudget::default()
                },
            );
            assert!(
                plan.diagnostics.is_empty(),
                "{value}: {:?}",
                plan.diagnostics
            );
            assert_eq!(
                plan.roots,
                [settings.resolve_path(value).unwrap()],
                "{value}"
            );
        }
    }
}

#[test]
fn home_expansion_applies_to_maps_mandatory_roots_and_fragments() {
    let environment = HashMap::from([("UserProfile".into(), r"C:\Users\Alice".into())]);
    let plan = parse(
        "MANPATH_MAP \"~/bin\" \"~/manuals\"\nMANDATORY_MANPATH ~\nMANCONFIG \"~\\man.d\\*.conf\"\n",
        Path::new(r"C:\config\man.conf"),
        &environment,
        true,
    );
    let home = PathBuf::from(r"C:\Users\Alice");
    assert_eq!(plan.diagnostics, []);
    assert_eq!(plan.mappings, [(home.join("bin"), home.join("manuals"))]);
    assert_eq!(plan.mandatory.as_slice(), std::slice::from_ref(&home));
    assert_eq!(plan.include_patterns, [home.join(r"man.d\*.conf")]);
}

#[test]
fn malformed_home_and_variables_are_diagnosed_without_relative_fallback() {
    let plan = parse(
        "manpath ~/missing-home\nmanpath %MISSING%\\man\nmanpath C:\\bad%\nmanpath relative\n",
        Path::new(r"C:\config\man.conf"),
        &HashMap::new(),
        true,
    );
    assert_eq!(plan.roots, [] as [PathBuf; 0]);
    assert_eq!(plan.diagnostics.len(), 4);
}

#[test]
fn path_maps_require_two_arguments_and_match_windows_paths() {
    let environment = HashMap::new();
    let source = PathBuf::from(r"C:\config\man.conf");
    let plan = parse(
        "manpath C:\\direct\nMANPATH_MAP \"C:\\Program Files\\Tool\\bin\" \"D:\\Tool Manuals\"\nMANDATORY_MANPATH C:\\required\n",
        &source,
        &environment,
        true,
    );
    let discovery = materialize(plan, &[PathBuf::from("c:/program files/tool/bin/")]);
    assert_eq!(
        discovery.roots,
        vec![
            PathBuf::from(r"C:\direct"),
            PathBuf::from(r"D:\Tool Manuals"),
            PathBuf::from(r"C:\required")
        ]
    );
    assert_eq!(discovery.diagnostics.len(), 0);

    assert!(split_arguments(r"C:\one C:\two C:\three", 2).is_err());
}

#[test]
fn invalid_windows_paths_are_omitted_with_source_lines() {
    let plan = parse(
        "manpath relative\\man\nmanpath %MISSING%\\man\nmanpath \"C:\\unterminated\nMANPATH_MAP C:\\only-one\n",
        PathBuf::from(r"C:\config\man.conf").as_path(),
        &HashMap::new(),
        true,
    );
    assert_eq!(plan.roots.len(), 0);
    assert_eq!(
        plan.diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.line)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
}

#[test]
fn known_directives_without_arguments_are_diagnosed() {
    let plan = parse(
        "MANPATH\nMANCONFIG\nMANPATH_MAP\nMANDATORY_MANPATH\nUNKNOWN\n",
        PathBuf::from(r"C:\config\man.conf").as_path(),
        &HashMap::new(),
        true,
    );
    assert_eq!(
        plan.diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.line, diagnostic.message.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (Some(1), "directive has the wrong number of path arguments"),
            (Some(2), "directive has the wrong number of path arguments"),
            (Some(3), "directive has the wrong number of path arguments"),
            (Some(4), "directive has the wrong number of path arguments"),
        ]
    );
}

#[test]
fn configuration_tree_limits_lines_across_fragments_and_reads_actual_bytes() {
    let source = Path::new(r"C:\man.conf");
    let mut budget = ConfigBudget {
        bytes: 3,
        lines: 2,
        ..ConfigBudget::default()
    };
    let first = parse_bounded("MANPATH\n", source, &HashMap::new(), true, &mut budget);
    let second = parse_bounded(
        "MANPATH\nMANPATH\nMANPATH\n",
        source,
        &HashMap::new(),
        false,
        &mut budget,
    );
    assert_eq!(first.diagnostics.len(), 1);
    assert_eq!(second.diagnostics.len(), 2);
    assert_eq!(budget.lines, 0);
    assert!(second.diagnostics[1].message.contains("limit"));
    let path = std::env::temp_dir().join(format!("mant-config-read-budget-{}", std::process::id()));
    fs::write(&path, "four").unwrap();
    let mut diagnostics = Vec::new();
    assert!(read_config(&path, &mut diagnostics, &mut budget).is_none());
    assert_eq!(budget.bytes, 0);
    assert_eq!(diagnostics.len(), 1);
    fs::remove_file(path).unwrap();
}

#[test]
fn root_directives_are_literal_and_environment_prefers_exact_spelling() {
    let environment = HashMap::from([
        (OsString::from("Path"), OsString::from(r"C:\other")),
        (OsString::from("PATH"), OsString::from(r"C:\exact")),
    ]);
    assert_eq!(
        mant_sources::settings::expand_configuration_path(
            "%PATH%",
            &environment
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            true,
        )
        .unwrap(),
        PathBuf::from(r"C:\exact")
    );
    let plan = parse(
        "manpath C:\\tools\\*\\man\nMANDATORY_MANPATH C:\\required\\?\n",
        Path::new(r"C:\man.conf"),
        &environment,
        true,
    );
    let roots = materialize(plan, &[]).roots;
    assert_eq!(
        roots,
        [
            PathBuf::from(r"C:\tools\*\man"),
            PathBuf::from(r"C:\required\?")
        ]
    );
}

#[test]
fn reaching_the_fragment_limit_skips_later_patterns() {
    let patterns = [PathBuf::from("first"), PathBuf::from("must-not-expand")];
    let included = collect_include_paths_with(&patterns, |pattern, budget| {
        let (paths, exhausted) = {
            assert_eq!(budget.remaining, MAX_EXPANDED_CONFIG_CANDIDATES);
            assert_eq!(pattern, PathBuf::from("first"));
            (
                (0..MAX_EXPANDED_CONFIG_PATHS)
                    .map(|index| PathBuf::from(format!(r"C:\fragments\{index:03}.conf")))
                    .collect(),
                false,
            )
        };
        super::ExpansionOutcome { paths, exhausted }
    });

    assert_eq!(included.paths.len(), MAX_EXPANDED_CONFIG_PATHS);
    assert!(!included.candidate_truncated);
    assert!(included.fragment_truncated);
}

#[test]
fn overlapping_patterns_can_fill_the_unique_fragment_budget() {
    let patterns = [PathBuf::from("first"), PathBuf::from("second")];
    let mut calls = 0;
    let included = collect_include_paths_with(&patterns, |pattern, budget| {
        let (paths, exhausted) = {
            calls += 1;
            match pattern.to_string_lossy().as_ref() {
                "first" => {
                    assert_eq!(budget.remaining, MAX_EXPANDED_CONFIG_CANDIDATES);
                    budget.remaining -= MAX_EXPANDED_CONFIG_PATHS - 1;
                    (
                        (0..MAX_EXPANDED_CONFIG_PATHS - 1)
                            .map(|index| PathBuf::from(format!(r"C:\fragments\{index:03}.conf")))
                            .collect(),
                        false,
                    )
                }
                "second" => {
                    assert_eq!(
                        budget.remaining,
                        MAX_EXPANDED_CONFIG_CANDIDATES - (MAX_EXPANDED_CONFIG_PATHS - 1)
                    );
                    (
                        vec![
                            PathBuf::from(r"c:/FRAGMENTS/000.conf"),
                            PathBuf::from(r"C:\fragments\unique.conf"),
                        ],
                        false,
                    )
                }
                _ => panic!("unexpected pattern"),
            }
        };
        super::ExpansionOutcome { paths, exhausted }
    });

    assert_eq!(calls, 2);
    assert_eq!(included.paths.len(), MAX_EXPANDED_CONFIG_PATHS);
    assert!(
        included
            .paths
            .contains(&PathBuf::from(r"C:\fragments\unique.conf"))
    );
    assert!(!included.candidate_truncated);
    assert!(!included.fragment_truncated);
}

#[test]
fn candidate_scan_budget_is_independent_of_fragment_deduplication() {
    let patterns = [PathBuf::from("first"), PathBuf::from("must-not-expand")];
    let included = collect_include_paths_with(&patterns, |pattern, budget| {
        let (paths, exhausted) = {
            assert_eq!(pattern, PathBuf::from("first"));
            assert_eq!(budget.remaining, MAX_EXPANDED_CONFIG_CANDIDATES);
            budget.remaining = 0;
            (
                vec![PathBuf::from(r"C:\fragments\same.conf"); MAX_EXPANDED_CONFIG_CANDIDATES],
                false,
            )
        };
        super::ExpansionOutcome { paths, exhausted }
    });

    assert_eq!(included.paths, [PathBuf::from(r"C:\fragments\same.conf")]);
    assert!(included.candidate_truncated);
    assert!(!included.fragment_truncated);
}

#[test]
fn fragment_includes_are_not_recursive() {
    let environment = HashMap::new();
    let source = PathBuf::from(r"C:\config\fragment.conf");
    let plan = parse(
        "MANCONFIG C:\\nested\\*.conf\nMANPATH C:\\from-fragment\n",
        &source,
        &environment,
        false,
    );
    assert_eq!(plan.include_patterns.len(), 0);
    assert_eq!(plan.roots, vec![PathBuf::from(r"C:\from-fragment")]);
}

#[test]
fn windows_globs_match_mixed_case_components_and_deduplicate_overlaps() {
    let fixture =
        std::env::temp_dir().join(format!("mant-windows-glob-case-{}", std::process::id()));
    let fragments = fixture.join("MAN.D");
    fs::create_dir_all(&fragments).unwrap();
    let first = fragments.join("10-ROOT.CONF");
    let second = fragments.join("20-extra.conf");
    fs::write(&first, "").unwrap();
    fs::write(&second, "").unwrap();
    fs::write(fragments.join("30-ignore.conf.bak"), "").unwrap();
    let pattern = fixture.join("man.?").join("*.conf");
    let included =
        super::collect_include_paths(&[pattern.clone(), fixture.join("MAN.?").join("*.CONF")]);
    assert_eq!(included.paths, [first, second]);
    assert!(!included.candidate_truncated && !included.fragment_truncated);

    // The same shared expander still implements case-sensitive Unix glob
    // semantics, regardless of whether the host filesystem folds case.
    let mut budget = super::ScanBudget::new(MAX_EXPANDED_CONFIG_CANDIDATES);
    let unix = crate::manual_paths::expand_path_pattern_bounded(&pattern, &mut budget);
    assert!(unix.paths.is_empty() && !unix.exhausted);
    let mut budget = super::ScanBudget::new(1);
    let bounded = super::expand_path_pattern_with_case(
        &pattern,
        &mut budget,
        super::GlobCase::AsciiInsensitive,
    );
    assert!(bounded.paths.is_empty() && bounded.exhausted);
    fs::remove_dir_all(fixture).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_config_file_orders_direct_fragment_mapped_and_mandatory_roots() {
    use std::fs;

    let fixture =
        std::env::temp_dir().join(format!("mant-windows-man-conf-{}", std::process::id()));
    let _ = fs::remove_dir_all(&fixture);
    let fragments = fixture.join("man.d");
    fs::create_dir_all(&fragments).expect("create configuration fragments");

    let environment = HashMap::from([(
        OsString::from("MANT_TEST_ROOT"),
        fixture.as_os_str().to_owned(),
    )]);
    fs::write(
        fixture.join("man.conf"),
        "manpath \"%MANT_TEST_ROOT%\\direct root\"\n\
         MANCONFIG \"%MANT_TEST_ROOT%\\MAN.?\\*.conf\"\n\
         MANPATH_MAP \"%MANT_TEST_ROOT%\\bin path\" \"%MANT_TEST_ROOT%\\mapped root\"\n\
         MANDATORY_MANPATH \"%MANT_TEST_ROOT%\\required root\"\n",
    )
    .expect("write main configuration");
    fs::write(
        fragments.join("10-TOOL.CONF"),
        "MANPATH \"%MANT_TEST_ROOT%\\fragment root\"\n\
         MANCONFIG \"%MANT_TEST_ROOT%\\nested\\*.conf\"\n",
    )
    .expect("write configuration fragment");

    let discovery = super::load_personal(
        &fixture.join("man.conf"),
        &environment,
        &[fixture.join("BIN PATH")],
        true,
    );
    assert_eq!(
        discovery.roots,
        vec![
            fixture.join("direct root"),
            fixture.join("fragment root"),
            fixture.join("mapped root"),
            fixture.join("required root"),
        ]
    );
    assert_eq!(discovery.diagnostics.len(), 0);

    fs::remove_dir_all(fixture).expect("remove configuration fixture");
}
