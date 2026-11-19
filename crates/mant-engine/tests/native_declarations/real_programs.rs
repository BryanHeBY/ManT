//! Standalone program declarations in complete release manuals.
use mant_ir::EntryKind;

use super::real_invocations::{check_owner, direct_lines, fixture_in};
use super::support::{check_head, explain};

#[test]
fn limits_binds_each_original_usage_and_body() {
    // CVS termp_nm_pre renders the lone program before optional operands.
    for (file, lines, assignment) in [
        ("freebsd-limits.1.gz", [52, 64, 80], "name=value ..."),
        ("dragonflybsd-limits.1.gz", [64, 75, 94], "name=value"),
    ] {
        let content = fixture_in("program-declarations", file);
        direct_lines(&content, "limits", &lines);
        for operand in ["limitflags", "name=value", "command"] {
            direct_lines(&content, operand, &[]);
        }
        let command = format!("limits [limitflags] [{assignment}] command");
        for (line, form, body) in [
            (
                lines[0],
                command.as_str(),
                "This usage sets limits according to",
            ),
            (
                lines[1],
                "limits [limitflags]",
                "does not attempt to set them",
            ),
            (
                lines[2],
                "limits -e [limitflags]",
                "suitable for the calling shell.",
            ),
        ] {
            check_owner(&content, line, "limits", form, EntryKind::Command, body);
        }
    }
}

#[test]
fn pkg_keeps_standalone_and_compound_owners_distinct() {
    // The Nm and Cm nodes are adjacent only in the add form. CVS prints
    // the intervening Oo BODY before Cm bootstrap in the other form.
    let content = fixture_in("program-declarations", "freebsd-pkg.7.gz");
    direct_lines(&content, "pkg", &[64, 91]);
    direct_lines(&content, "pkg add", &[71]);
    for non_name in ["command", "pkg.pkg", "pkg bootstrap", "bootstrap"] {
        direct_lines(&content, non_name, &[]);
    }
    for (line, name, form, body) in [
        (64, "pkg", "pkg command ...", "have its signature verified"),
        (
            71,
            "pkg add",
            "pkg add [-fy] [-r reponame] pkg.pkg",
            "from a local package instead of fetching from remote.",
        ),
        (91, "pkg", "pkg -N", "Do not bootstrap"),
    ] {
        check_owner(&content, line, name, form, EntryKind::Command, body);
    }
    let form = "pkg [-46] bootstrap [-fy] [-r reponame]";
    let owner = super::definitions(content.document.as_ref().unwrap())
        .into_iter()
        .find(|item| item.source.unwrap().line == 98)
        .unwrap();
    check_head(owner, form, &[]);
    assert_eq!(explain(&content, form).counts.direct_entry.total, 1);
}
