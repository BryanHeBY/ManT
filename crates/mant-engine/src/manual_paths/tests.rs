use std::{collections::HashMap, env, ffi::OsString, fs, path::PathBuf};

use super::{
    BsdManConfig, ManDbConfig, ManualPathPlatform, config_directive, deduplicate_manual_paths,
    developer_manual_roots, discover_manual_roots_from, discover_manual_roots_from_for,
    environment_value_for, expand_man_db_systems, parse_bsd_man_config, parse_man_db_config,
    parse_mandoc_manpaths, supplemental_manual_roots_for, wildcard_matches,
};

#[test]
fn injected_windows_discovery_uses_only_its_config_and_environment() {
    let root = temporary_root("injected-windows-config");
    fs::create_dir_all(&root).unwrap();
    let config = root.join("man.conf");
    fs::write(&config, "MANPATH C:\\isolated\\manuals\n").unwrap();
    let environment = HashMap::from([(
        OsString::from("AppData"),
        OsString::from(r"C:\isolated\roaming"),
    )]);
    let context = super::DiscoveryContext {
        environment: &environment,
        platform: ManualPathPlatform::Windows,
        mant_config: Some(&config),
    };
    let result = super::host_default_manual_roots(&context);
    assert!(result.diagnostics.is_empty());
    assert_eq!(
        result.roots,
        [
            PathBuf::from(r"C:\isolated\manuals"),
            PathBuf::from(r"C:\isolated\roaming").join("ManT/man")
        ]
    );
    let absent = super::DiscoveryContext {
        mant_config: None,
        ..context
    };
    assert_eq!(
        super::host_default_manual_roots(&absent).roots,
        [PathBuf::from(r"C:\isolated\roaming").join("ManT/man")]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn macos_man_conf_reads_paths_and_imports_port_fragments() {
    let root = temporary_root("macos-man-conf");
    let fragments = root.join("man.d");
    fs::create_dir_all(&fragments).expect("create fragment root");
    let primary = root.join("primary");
    let port = root.join("port");
    fs::create_dir_all(&primary).expect("create primary root");
    fs::create_dir_all(&port).expect("create port root");
    let fragment = fragments.join("tool.conf");
    fs::write(&fragment, format!("MANPATH {}\n", port.display())).expect("write fragment");
    let configuration = parse_bsd_man_config(&format!(
        "MANPATH {}\nMANCONFIG {}/*.conf\n",
        primary.display(),
        fragments.display()
    ));
    assert_eq!(configuration.paths, vec![primary]);
    assert_eq!(
        configuration.include_pattern,
        Some(fragments.join("*.conf"))
    );
    assert_eq!(
        super::macos_configuration_roots(&root.join("man.conf")).roots,
        Vec::<PathBuf>::new()
    );
    fs::write(
        root.join("man.conf"),
        format!(
            "MANPATH {}\nMANCONFIG {}/*.conf\n",
            root.join("primary").display(),
            fragments.display()
        ),
    )
    .expect("write man.conf");
    assert_eq!(
        super::macos_configuration_roots(&root.join("man.conf")).roots,
        vec![root.join("primary"), port]
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn mandoc_man_conf_uses_lowercase_manpath_only() {
    let paths = parse_mandoc_manpaths(
        "# comment\nmanpath /usr/share/man\nMANPATH /not-mandoc\noutput style css\n",
    );
    assert_eq!(paths, vec![PathBuf::from("/usr/share/man")]);
}

#[test]
fn directive_parser_preserves_internal_path_whitespace() {
    assert_eq!(
        config_directive("manpath   C:\\Program Files\\Tool\\man   "),
        Some(("manpath", "C:\\Program Files\\Tool\\man"))
    );
    assert_eq!(config_directive("manpath   "), None);
    assert_eq!(config_directive("manpath"), None);
}

#[test]
fn active_macos_developer_tree_contributes_tool_and_sdk_manuals() {
    let developer = temporary_root("macos-developer");
    let tool = developer.join("usr/share/man");
    let sdk = developer.join("Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk/usr/share/man");
    fs::create_dir_all(&tool).expect("create tool manuals");
    fs::create_dir_all(&sdk).expect("create SDK manuals");
    assert_eq!(developer_manual_roots(&developer), vec![tool, sdk]);
    fs::remove_dir_all(developer).expect("remove fixture");
}

#[test]
fn man_db_maps_path_then_appends_mandatory_roots_and_systems() {
    let root = temporary_root("man-db-mappings");
    let binary = root.join("tool/bin");
    let manual = root.join("tool/man");
    let shared = root.join("tool/share/man");
    let mandatory = root.join("usr/share/man");
    let configuration = parse_man_db_config(&format!(
        "MANPATH_MAP {} {}\nMANPATH_MAP {} {}\nMANDATORY_MANPATH {}\n",
        binary.display(),
        manual.display(),
        binary.display(),
        shared.display(),
        mandatory.display(),
    ));
    assert_eq!(
        configuration,
        ManDbConfig {
            mappings: vec![
                (binary.clone(), manual.clone()),
                (binary.clone(), shared.clone()),
            ],
            mandatory: vec![mandatory.clone()],
        }
    );
    let environment = HashMap::from([
        (
            OsString::from("PATH"),
            env::join_paths([binary]).expect("join PATH"),
        ),
        (OsString::from("SYSTEM"), OsString::from("man")),
    ]);
    assert_eq!(
        super::man_db_manual_roots(&environment, &configuration),
        vec![manual, shared, mandatory]
    );
}

#[test]
fn empty_manpath_components_insert_one_native_default_sequence() {
    let root = temporary_root("empty-manpath");
    let first = root.join("first");
    let empty = PathBuf::new();
    let last = root.join("last");
    let environment = HashMap::from([(
        OsString::from("MANPATH"),
        env::join_paths([&first, &empty, &last]).expect("join MANPATH"),
    )]);
    let native_a = root.join("native/a");
    let native_b = root.join("native/b");
    assert_eq!(
        discover_manual_roots_from(&environment, vec![native_a.clone(), native_b.clone()],),
        vec![first, native_a, native_b, last]
    );
}

#[test]
fn windows_supplemental_roots_prefer_mant_data_before_profile_compatibility() {
    let data_root = PathBuf::from(r"C:\Users\demo\AppData\Roaming");
    let profile = PathBuf::from(r"C:\Users\demo");
    let environment = HashMap::from([
        (OsString::from("AppData"), data_root.as_os_str().to_owned()),
        (
            OsString::from("UserProfile"),
            profile.as_os_str().to_owned(),
        ),
    ]);

    assert_eq!(
        supplemental_manual_roots_for(&environment, ManualPathPlatform::Windows),
        vec![
            data_root.join("ManT").join("man"),
            profile.join(".local/share/man")
        ]
    );
}

#[test]
fn windows_environment_names_are_ascii_case_insensitive_only_on_windows() {
    let path = env::join_paths([PathBuf::from("/tools")]).expect("join PATH");
    let environment = HashMap::from([
        (OsString::from("Path"), path.clone()),
        (OsString::from("ManPath"), OsString::from("/manuals")),
        (OsString::from("Mant_ManPath"), OsString::from("/override")),
    ]);

    assert_eq!(
        environment_value_for(&environment, "PATH", ManualPathPlatform::Windows),
        Some(&path)
    );
    assert_eq!(
        environment_value_for(&environment, "PATH", ManualPathPlatform::Linux),
        None
    );
    assert_eq!(
        discover_manual_roots_from_for(
            &environment,
            vec![PathBuf::from("/default")],
            ManualPathPlatform::Windows,
        ),
        vec![PathBuf::from("/override")]
    );

    let environment = HashMap::from([(
        OsString::from("ManPath"),
        env::join_paths([PathBuf::from("/first"), PathBuf::from("/second")]).expect("join MANPATH"),
    )]);
    assert_eq!(
        discover_manual_roots_from_for(
            &environment,
            vec![PathBuf::from("/default")],
            ManualPathPlatform::Windows,
        ),
        vec![PathBuf::from("/first"), PathBuf::from("/second")]
    );
}

#[test]
fn windows_final_roots_deduplicate_case_and_separator_variants() {
    assert_eq!(
        deduplicate_manual_paths(
            [
                PathBuf::from(r"C:\Users\demo\ManT\man"),
                PathBuf::from("c:/users/DEMO/mant/man/"),
            ],
            ManualPathPlatform::Windows,
        ),
        vec![PathBuf::from(r"C:\Users\demo\ManT\man")]
    );
}

#[test]
fn windows_supplemental_roots_do_not_require_a_profile_fallback() {
    let data_root = PathBuf::from(r"D:\Roaming");
    let environment =
        HashMap::from([(OsString::from("APPDATA"), data_root.as_os_str().to_owned())]);

    assert_eq!(
        supplemental_manual_roots_for(&environment, ManualPathPlatform::Windows),
        vec![data_root.join("ManT").join("man")]
    );
}

#[test]
fn wildcard_expansion_matches_configuration_file_globs_deterministically() {
    assert!(wildcard_matches("*.conf", "perl.conf"));
    assert!(wildcard_matches("?.conf", "x.conf"));
    assert!(!wildcard_matches("?.conf", "xy.conf"));
}

#[cfg(windows)]
#[test]
fn wildcard_expansion_preserves_an_absolute_windows_root() {
    let root = temporary_root("windows-absolute-glob");
    fs::create_dir_all(&root).expect("create fragment root");
    let fragment = root.join("tool.conf");
    fs::write(&fragment, "MANPATH C:\\manuals\n").expect("write fragment");

    assert_eq!(super::expand_path_pattern(&root.join("*.conf")), [fragment]);
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn systems_without_man_omit_the_native_root() {
    let environment = HashMap::from([(OsString::from("SYSTEM"), OsString::from("other"))]);
    let root = temporary_root("man-db-system");
    fs::create_dir_all(root.join("other")).expect("create system root");
    assert_eq!(
        expand_man_db_systems(vec![root.clone()], &environment),
        vec![root.join("other")]
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn bsd_config_keeps_only_path_related_directives() {
    assert_eq!(
        parse_bsd_man_config("MANPATH /usr/share/man\nMANLOCALE ja_JP\n"),
        BsdManConfig {
            paths: vec![PathBuf::from("/usr/share/man")],
            include_pattern: None,
            truncated: false,
        }
    );
}

fn temporary_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mant-manual-paths-{label}-{}", std::process::id()))
}
