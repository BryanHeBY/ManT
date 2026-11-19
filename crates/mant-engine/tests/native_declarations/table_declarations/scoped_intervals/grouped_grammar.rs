//! Accepted groups share an owner, with grammar scoped to their native bytes.
use mant_ir::{EntryKind, EntryOwner, semantics_complete};

use super::{
    continuations::{Match, check, insert_table},
    definitions,
};

const COMMA: &str = r".No \&,";
const PIPE: &str = r".No \&|";

#[test]
fn mixed_groups_keep_complete_option_environment_and_command_grammars() {
    // Pinned mdoc_macro.c::in_line identifies each native operand;
    // termp_fn_pre emits its signature in a single independent group.
    for (section, group, names, excluded) in [
        (
            "DESCRIPTION",
            &[".Ev USER", ".No or", ".Ev LOGNAME"][..],
            &["USER", "LOGNAME"][..],
            &["or"][..],
        ),
        (
            "DESCRIPTION",
            &[".Fl n Ns / Ns Fl -number"],
            &["-n", "--number"],
            &["-number"],
        ),
        (
            "DESCRIPTION",
            &[".Fl n Ns / Ns Ar -NUM"],
            &["-n"],
            &["-NUM"],
        ),
        (
            "DESCRIPTION",
            &[".Fl Fl color Ns = Ns Ar WHEN"],
            &["--color"],
            &["WHEN"],
        ),
        (
            "COMMANDS",
            &[".Cm zfs Cm get Ar prop"],
            &["zfs get"],
            &["zfs", "get", "prop"],
        ),
        (
            "COMMANDS",
            &[".Nm zfs Cm get Ar prop"],
            &["zfs get"],
            &["zfs", "get", "prop"],
        ),
    ] {
        for separator in [COMMA, PIPE] {
            for first in [true, false] {
                let (left, right) = if first {
                    (group, &[".Fn runfn"][..])
                } else {
                    (&[".Fn runfn"][..], group)
                };
                let head = left
                    .iter()
                    .copied()
                    .chain([separator])
                    .chain(right.iter().copied())
                    .collect::<Vec<_>>();
                let probes = names
                    .iter()
                    .copied()
                    .chain(["runfn"])
                    .map(|name| (name, Match::Owner))
                    .chain(excluded.iter().copied().map(|name| (name, Match::Absent)))
                    .collect::<Vec<_>>();
                check(section, &head, None, &probes);
                if separator == COMMA {
                    let table = insert_table(head.clone(), Some(left.len()));
                    check(section, &table, None, &probes);
                }
            }
        }
    }
}

#[test]
fn grouped_commands_inherit_their_enclosing_role_and_validate_the_full_form() {
    for macro_name in ["Cm", "Ic"] {
        for section in ["DESCRIPTION", "CONFIGURATION", "COMMANDS"] {
            let assignment = format!(".{macro_name} mode=fast");
            let (bound, excluded) = if section == "COMMANDS" {
                ("mode=fast", "mode")
            } else {
                ("mode", "mode=fast")
            };
            for head in [
                [assignment.as_str(), COMMA, ".Er EINVAL"],
                [".Er EINVAL", COMMA, assignment.as_str()],
            ] {
                check(
                    section,
                    &head,
                    None,
                    &[
                        (bound, Match::Owner),
                        ("EINVAL", Match::Owner),
                        (excluded, Match::Absent),
                    ],
                );
            }
        }
        let directive = format!(".{macro_name} directive");
        let content = check(
            "DESCRIPTION",
            &[&directive, COMMA, r#".Va "int x, y""#],
            None,
            &[
                ("directive", Match::Owner),
                ("x", Match::Absent),
                ("y", Match::Absent),
            ],
        );
        let items = definitions(content.document.as_ref().unwrap());
        assert_eq!(
            EntryOwner::Definition(items[0]).facts().unwrap().kind,
            EntryKind::Term,
        );
    }
}

#[test]
fn internal_commands_do_not_override_the_groups_leading_native_role() {
    // mdoc_macro.c::in_line closes Ar at the pipe and Er before Ic.
    // Both leave a distinct Ic. Only the styled Nm/Cm form starts a
    // command declaration under COMMANDS.
    for (group, names, exclusions) in [
        (".Ar cmd | Ic getline", &[][..], &["cmd", "getline"][..]),
        (".Er EINVAL Ic touch", &["EINVAL"][..], &["touch"][..]),
        (
            ".Nm zfs Cm get Ar prop",
            &["zfs get"][..],
            &["zfs", "get", "prop"][..],
        ),
    ] {
        for separator in [COMMA, PIPE] {
            for head in [
                [group, separator, ".Fn extra"],
                [".Fn extra", separator, group],
            ] {
                let probes = names
                    .iter()
                    .copied()
                    .chain(["extra"])
                    .map(|name| (name, Match::Owner))
                    .chain(exclusions.iter().copied().map(|name| (name, Match::Absent)))
                    .collect::<Vec<_>>();
                check("COMMANDS", &head, None, &probes);
            }
        }
    }
}

#[test]
fn name_coverage_limit_is_reported_once_at_either_end_of_the_group() {
    for role in ["Fl", "Ev", "Cm"] {
        for count in [255, 256, 257] {
            for first in [true, false] {
                let mut subjects = (0..count)
                    .map(|i| {
                        if role == "Fl" {
                            format!(".Fl Fl opt{i}")
                        } else {
                            format!(".{role} FLAG{i}")
                        }
                    })
                    .collect::<Vec<_>>();
                subjects.insert(if first { 0 } else { count }, ".Er PIVOTERR".into());
                let head = subjects
                    .iter()
                    .enumerate()
                    .flat_map(|(i, subject)| {
                        (i != 0)
                            .then_some(COMMA)
                            .into_iter()
                            .chain([subject.as_str()])
                    })
                    .collect::<Vec<_>>();
                let limited = count >= 256;
                let content = check(
                    "DESCRIPTION",
                    &head,
                    None,
                    &[(
                        "PIVOTERR",
                        if limited { Match::Absent } else { Match::Owner },
                    )],
                );
                let doc = content.document.unwrap();
                let diagnostics = doc
                    .diagnostics
                    .iter()
                    .filter(|diagnostic| {
                        diagnostic.code.as_deref() == Some("manual.semantic-entry.name-limit")
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    diagnostics.len(),
                    usize::from(limited),
                    "{role}/{count}/{first}"
                );
                assert_eq!(
                    semantics_complete(&doc.diagnostics),
                    !limited,
                    "{role}/{count}/{first}"
                );
            }
        }
    }
}
