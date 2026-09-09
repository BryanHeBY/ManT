//! Compare actual clap metadata with semantic definitions, not help substrings.

use std::collections::{BTreeMap, BTreeSet};

use clap::CommandFactory;
use mant_ir::{EntryKind, NameCase, ParameterKind};
use mant_loader::load_markdown_text;
use mant_protocol::{EvidenceBasis, OutlineDetail, OutlineNode};
use mant_query::{build_outline_with_detail, select_explanation};

fn public_flags(command: &mut clap::Command) -> Vec<BTreeSet<String>> {
    // Include generated help/version switches as well as authored arguments.
    command.build();
    command
        .get_arguments()
        .filter(|argument| !argument.is_hide_set())
        .map(|argument| {
            argument
                .get_long()
                .into_iter()
                .chain(argument.get_visible_aliases().into_iter().flatten())
                .map(|name| format!("--{name}"))
                .chain(
                    argument
                        .get_short()
                        .into_iter()
                        .chain(argument.get_visible_short_aliases().into_iter().flatten())
                        .map(|name| format!("-{name}")),
                )
                .collect::<BTreeSet<_>>()
        })
        .filter(|names| !names.is_empty())
        .collect()
}

fn collect_option_aliases(nodes: &[OutlineNode], observed: &mut BTreeMap<String, usize>) {
    for node in nodes {
        if let OutlineNode::DocumentEntry {
            entry_kind:
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
            names,
            ..
        } = node
        {
            for name in names {
                *observed.entry(name.clone()).or_default() += 1;
            }
        }
        collect_option_aliases(node.children(), observed);
    }
}

fn check_manual(command: &mut clap::Command, manual: &str) -> Result<(), String> {
    check_manual_coverage(command, manual, true)
}

fn check_manual_coverage(
    command: &mut clap::Command,
    manual: &str,
    exact: bool,
) -> Result<(), String> {
    let groups = public_flags(command);
    let query = load_markdown_text(manual, None).map_err(|error| error.to_string())?;
    let document = query.document.as_ref().ok_or("missing manual document")?;
    if !document.diagnostics.is_empty() {
        return Err(format!("manual diagnostics: {:?}", document.diagnostics));
    }
    let outline = build_outline_with_detail(&query, OutlineDetail::Entries)
        .map_err(|error| error.to_string())?;
    if !outline.semantics_complete {
        return Err("manual semantics are incomplete".into());
    }
    let mut observed = BTreeMap::new();
    collect_option_aliases(&outline.nodes, &mut observed);
    let expected: BTreeMap<_, _> = groups
        .iter()
        .flatten()
        .map(|name| (name.clone(), 1))
        .collect();
    if (exact && observed != expected)
        || expected
            .iter()
            .any(|(name, count)| observed.get(name) != Some(count))
        || observed.values().any(|count| *count != 1)
    {
        return Err(format!(
            "public option coverage differs: expected {expected:?}, observed {observed:?}"
        ));
    }
    for names in groups {
        let mut entry_id = None;
        for name in &names {
            let result = select_explanation(&query, name)
                .map_err(|error| format!("{name} evidence collection failed: {error}"))?;
            // The independent clap oracle describes one definition per flag;
            // ordinary supporting content in the same response is unrestricted.
            let direct = result
                .evidence
                .iter()
                .filter(|e| {
                    e.bases
                        .iter()
                        .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
                })
                .collect::<Vec<_>>();
            let [evidence] = direct.as_slice() else {
                return Err(format!(
                    "{name} did not retain its one documented name owner"
                ));
            };
            let identity = evidence.entry.as_ref().ok_or("entry without identity")?;
            if identity.kind
                != (EntryKind::Parameter {
                    parameter_kind: mant_ir::ParameterKind::Option,
                })
                || identity.case != NameCase::Sensitive
            {
                return Err(format!("{name} must be a case-sensitive option"));
            }
            let observed_names: BTreeSet<_> = identity.names.iter().cloned().collect();
            if observed_names != names {
                return Err(format!(
                    "{name} name grouping differs: expected {names:?}, observed {observed_names:?}"
                ));
            }
            if names.len() > 1
                && !identity
                    .alias_groups
                    .iter()
                    .any(|group| group.iter().cloned().collect::<BTreeSet<_>>() == names)
            {
                return Err(format!("{name} is missing its explicit clap alias group"));
            }
            if entry_id
                .as_ref()
                .is_some_and(|id| id != evidence.outline.node.id())
            {
                return Err(format!("{name} aliases select different entries"));
            }
            entry_id = Some(evidence.outline.node.id().to_owned());
        }
    }
    Ok(())
}

#[test]
fn self_manual_covers_every_public_clap_option_and_alias() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    // Published source sets include a terminal test helper under tests/support,
    // but intentionally omit the repository's integration suites and manuals.
    // Detect the actual checkout-only suite, not merely a tests directory. A
    // checkout missing its authoritative manual must still fail below.
    // The checker fixtures below also run in every packaged crate.
    if !manifest.join("tests/self_manual_links.rs").is_file() {
        return;
    }
    let manual = std::fs::read_to_string(manifest.join("../../docs/manuals/mant.md"))
        .expect("the checkout must contain its authoritative self manual");
    check_manual_coverage(
        &mut super::super::Cli::command(),
        &manual,
        cfg!(all(
            feature = "roff",
            feature = "tui",
            feature = "pager",
            feature = "mcp",
            feature = "update"
        )),
    )
    .unwrap();
}

#[test]
fn metadata_enumeration_includes_generated_and_visible_aliases_only() {
    let mut command = clap::Command::new("demo")
        .version("1.0")
        .arg(
            clap::Arg::new("query")
                .long("query")
                .short('q')
                .visible_alias("find")
                .visible_short_alias('f')
                .alias("old-query"),
        )
        .arg(clap::Arg::new("private").long("private").hide(true))
        .arg(clap::Arg::new("document"));
    let flags: BTreeSet<_> = public_flags(&mut command).into_iter().flatten().collect();
    assert_eq!(
        flags,
        [
            "--query",
            "--find",
            "-q",
            "-f",
            "--help",
            "-h",
            "--version",
            "-V"
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    );
}

#[test]
fn coverage_rejects_prose_only_obsolete_and_wrongly_grouped_options() {
    let command = || {
        clap::Command::new("demo")
            .disable_help_flag(true)
            .arg(clap::Arg::new("output").long("output").short('o'))
    };
    let prefix = "# demo\n\n## Parameters\n\n<!-- mant:entries role=option case=sensitive -->\n";
    check_manual(
        &mut command(),
        &format!("{prefix}- `-o`, `--output`: Write output. <!-- mant:entry {{\"aliasGroups\":[[\"-o\",\"--output\"]]}} -->\n"),
    )
    .unwrap();
    for body in [
        "The --output option writes output.",
        "- `-o`, `--output`: Shared names without an explicit alias group.",
        "- `--output`: Missing short alias.",
        "- `-o`: Separate entry.\n- `--output`: Separate entry.",
        "- `-o`, `--output`: Write output.\n- `--obsolete`: Removed flag.",
        "- `-o`, `--output`: First.\n- `-o`, `--output`: Duplicate.",
    ] {
        assert!(
            check_manual(&mut command(), &format!("{prefix}{body}\n")).is_err(),
            "{body}"
        );
    }
}

#[test]
fn partial_capabilities_require_all_exposed_flags_without_erasing_full_manual_contract() {
    let command = || {
        clap::Command::new("demo")
            .disable_help_flag(true)
            .arg(clap::Arg::new("query").long("query"))
    };
    let manual = "# demo\n\n## Options\n\n<!-- mant:entries role=option case=sensitive -->\n- `--query`: Query.\n- `--update`: Update.\n";
    assert!(check_manual_coverage(&mut command(), manual, false).is_ok());
    assert!(check_manual_coverage(&mut command(), manual, true).is_err());
    assert!(
        check_manual_coverage(
            &mut command(),
            &manual.replace("--query", "--missing"),
            false
        )
        .is_err()
    );
    assert!(
        check_manual_coverage(
            &mut command(),
            &format!("{manual}- `--query`: Duplicate.\n"),
            false
        )
        .is_err()
    );
}
