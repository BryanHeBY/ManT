//! Compare actual clap metadata with semantic definitions, not help substrings.

use std::collections::{BTreeMap, BTreeSet};

use clap::CommandFactory;
use mant_engine::{build_outline_with_detail, query_markdown_text, select_explanation};
use mant_ir::{DefinitionCase, DefinitionRole, EntryKind, ParameterKind};
use mant_protocol::{ExcerptSelection, OutlineDetail, OutlineNode};

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

fn collect_option_aliases(nodes: &[OutlineNode], aliases: &mut BTreeMap<String, usize>) {
    for node in nodes {
        if let OutlineNode::DocumentEntry {
            entry_kind:
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
            aliases: names,
            ..
        } = node
        {
            for name in names {
                *aliases.entry(name.clone()).or_default() += 1;
            }
        }
        collect_option_aliases(node.children(), aliases);
    }
}

fn check_manual(command: &mut clap::Command, manual: &str) -> Result<(), String> {
    let groups = public_flags(command);
    let query = query_markdown_text(manual, None).map_err(|error| error.to_string())?;
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
    if observed != expected {
        return Err(format!(
            "public option coverage differs: expected {expected:?}, observed {observed:?}"
        ));
    }
    for names in groups {
        let mut entry_id = None;
        for name in &names {
            let excerpt = select_explanation(&query, name)
                .map_err(|error| format!("{name} is not uniquely explainable: {error}"))?;
            let [ExcerptSelection::DocumentEntry { entry, .. }] = excerpt.selections.as_slice()
            else {
                return Err(format!("{name} did not select one entry"));
            };
            let identity = entry
                .entry_owner()
                .and_then(mant_ir::EntryOwner::facts)
                .ok_or("entry without identity")?;
            if identity.role != DefinitionRole::Option || identity.case != DefinitionCase::Sensitive
            {
                return Err(format!("{name} must be a case-sensitive option"));
            }
            let aliases: BTreeSet<_> = identity.names.iter().cloned().collect();
            if aliases != names {
                return Err(format!(
                    "{name} alias grouping differs: expected {names:?}, observed {aliases:?}"
                ));
            }
            if entry_id.as_ref().is_some_and(|id| id != &identity.id) {
                return Err(format!("{name} aliases select different entries"));
            }
            entry_id = Some(identity.id.clone());
        }
    }
    Ok(())
}

#[test]
fn self_manual_covers_every_public_clap_option_and_alias() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    // Published source sets intentionally omit repository integration tests and
    // manuals. Like the engine's self_manuals suite, the real-document audit is
    // checkout-only; the checker fixtures below also run in packaged crates.
    if !manifest.join("tests").is_dir() {
        return;
    }
    let manual = std::fs::read_to_string(manifest.join("../../docs/manuals/mant.md"))
        .expect("the checkout must contain its authoritative self manual");
    check_manual(&mut super::super::Cli::command(), &manual).unwrap();
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
        &format!("{prefix}- `-o`, `--output`: Write output.\n"),
    )
    .unwrap();
    for body in [
        "The --output option writes output.",
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
