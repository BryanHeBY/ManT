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

#[test]
fn stats_keeps_the_whole_executed_subcommand_in_its_command_directory() {
    // termp_nm_pre writes both words from one Nm under the original It.
    let content = fixture_in("program-declarations", "freebsd-stats.7.gz");
    direct_lines(&content, "zpool iostat", &[87]);
    direct_lines(&content, "zpool", &[]);
    direct_lines(&content, "iostat", &[57]);
    check_owner(
        &content,
        87,
        "zpool iostat",
        "zpool iostat",
        EntryKind::Command,
        "Report ZFS I/O statistics",
    );
}

#[test]
fn hammer_version_headings_keep_both_descriptions_without_a_command_name() {
    // CVS termp_nm_pre/HTML mdoc_nm_pre emit the subject of this manual
    // inside each It HEAD; the following No supplies an ordinary version label.
    let content = fixture_in("program-declarations", "dragonflybsd-hammer.8.gz");
    direct_lines(&content, "HAMMER", &[]);
    let items = super::definitions(content.document.as_ref().unwrap());
    for (version, lines) in [("2-", [608, 1331]), ("3+", [613, 1354])] {
        let form = format!("HAMMER version {version}");
        direct_lines(&content, &form, &lines);
        for line in lines {
            let owner = items
                .iter()
                .find(|it| it.source.is_some_and(|span| span.line == line))
                .unwrap();
            check_head(owner, &form, &[]);
            assert_eq!(owner.entry.as_ref().unwrap().kind, EntryKind::Term);
            let hit = explain(&content, &form)
                .evidence
                .into_iter()
                .find(|hit| hit.source == owner.source)
                .unwrap();
            let excerpt = mant_query::select_excerpt(
                &content,
                &[mant_protocol::ContentSelector::path(hit.outline.path())],
            )
            .unwrap();
            let expected = if line < 1000 {
                "The configuration file"
            } else {
                "The snapshots directory"
            };
            assert!(
                mant_render::render_excerpt_text(&excerpt).contains(expected),
                "{line}"
            );
        }
    }
}
