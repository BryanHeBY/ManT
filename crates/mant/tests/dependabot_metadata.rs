//! Pin repository update policy; the option grammar follows GitHub's reference.
//!
//! These guards deliberately inspect the maintained YAML spelling, not a second
//! implementation of Dependabot's parser or a promise about bot resolution.

const CONFIG: &str = include_str!("../../../.github/dependabot.yml");

fn normalized_config(source: &str) -> String {
    source.replace("\r\n", "\n")
}

fn cargo_policy(config: &str) -> &str {
    let mut jobs = config.split("  - package-ecosystem: cargo\n");
    let _ = jobs.next();
    let policy = jobs.next().expect("one Cargo update job");
    assert!(jobs.next().is_none(), "Cargo jobs must not overlap");
    policy
        .split("  - package-ecosystem:")
        .next()
        .expect("Cargo job body")
}

fn group<'a>(cargo: &'a str, name: &str) -> &'a str {
    let header = format!("      {name}:\n");
    let body = cargo
        .split_once(&header)
        .unwrap_or_else(|| panic!("missing dependency group {name}"))
        .1;
    // Group fields and sequence members have at least eight spaces; the next
    // six-space mapping key starts another group.
    let end = body
        .match_indices('\n')
        .find_map(|(offset, _)| {
            let next_line = &body[offset + 1..];
            next_line
                .strip_prefix("      ")
                .and_then(|rest| (!rest.starts_with([' ', '#', '\n'])).then_some(offset + 1))
        })
        .unwrap_or(body.len());
    &body[..end]
}

#[test]
fn cargo_updates_cover_both_lockfiles_without_duplicate_jobs() {
    let config = normalized_config(CONFIG);
    let cargo = cargo_policy(&config);
    assert!(cargo.contains("    directories:\n      - /\n      - /fuzz\n"));
    assert!(!cargo.contains("    directory:"));
    assert!(cargo.contains("    target-branch: dev\n"));
    assert!(cargo.contains("      interval: weekly\n"));
    assert_eq!(
        config.matches("package-ecosystem: github-actions").count(),
        1
    );
}

#[test]
fn higher_risk_families_are_separate_from_the_patch_batch() {
    let config = normalized_config(CONFIG);
    let cargo = cargo_policy(&config);
    let families: [(&str, &[&str]); 4] = [
        ("mcp-sdk", &["rmcp", "rmcp-macros"]),
        (
            "rust-compression",
            &["flate2", "miniz_oxide", "zip", "zstd*"],
        ),
        ("rust-native-build", &["cc", "find-msvc-tools"]),
        (
            "rust-terminal",
            &["crossterm*", "signal-hook*", "crossbeam-channel"],
        ),
    ];
    let patches = group(cargo, "rust-patches");
    let exclusions = patches
        .split_once("        exclude-patterns:\n")
        .expect("patch batch excludes higher-risk families")
        .1;
    for (name, dependencies) in families {
        let family = group(cargo, name);
        assert!(!family.contains("group-by:"), "keep {name} together");
        for dependency in dependencies {
            let pattern = format!("          - \"{dependency}\"\n");
            assert!(family.contains(&pattern), "{name}: {dependency}");
            assert!(exclusions.contains(&pattern), "exclude {dependency}");
        }
    }
    assert!(patches.contains("        update-types:\n          - \"patch\"\n"));
    let individual = group(cargo, "rust-individual");
    assert!(individual.contains("        group-by: dependency-name\n"));
    assert!(individual.contains("          - \"*\"\n"));
}

#[test]
fn grouping_does_not_disable_security_or_larger_version_updates() {
    let config = normalized_config(CONFIG);
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        for forbidden in ["ignore:", "allow:", "open-pull-requests-limit: 0"] {
            assert!(
                !line.starts_with(forbidden),
                "unexpected restriction: {line}"
            );
        }
    }
    assert!(!group(cargo_policy(&config), "rust-individual").contains("update-types:"));
}

#[test]
fn policy_guards_receive_the_same_input_from_lf_and_crlf_checkouts() {
    let lf = normalized_config(CONFIG);
    let crlf = lf.replace('\n', "\r\n");
    assert!(crlf.contains("\r\n"));
    let restored = normalized_config(&crlf);
    assert_eq!(restored, lf);
    assert_eq!(cargo_policy(&restored), cargo_policy(&lf));
    for name in [
        "mcp-sdk",
        "rust-compression",
        "rust-native-build",
        "rust-terminal",
        "rust-patches",
        "rust-individual",
    ] {
        assert_eq!(
            group(cargo_policy(&restored), name),
            group(cargo_policy(&lf), name),
            "{name} must not depend on checkout line endings"
        );
    }
}
