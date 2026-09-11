//! Exact native destination retention and public tag normalization.

use super::*;

#[test]
fn parser_copies_validated_same_document_navigation() {
    let path = source_path("navigation-mandoc-session");
    fs::write(
        &path,
        ".Dd July 19, 2026\n.Dt NAVIGATION 1\n.Os\n.Sh FIRST\n\
         See\n\
         .Sx TARGET\n\
         for details.\n\
         .Tg explicit-target\n\
         .Fl x\n\
         .Sh TARGET\nTarget text.\n",
    )
    .expect("write navigation mdoc source");

    let document = parse_file(&path, false).expect("parse navigation mdoc source");
    fs::remove_file(path).expect("remove navigation mdoc source");

    assert!(find_macro(&document.root, "Sx").is_some());
    let explicit_target = find_node(&document.root, &|node| {
        node.flags.deep_link_target && node.tag.as_deref() == Some("explicit-target")
    });
    let explicit_target = explicit_target.expect("Tg must annotate its resolved destination");
    assert!(explicit_target.flags.permalink);
}

#[test]
fn explicit_target_before_an_already_tagged_subsection_remains_separate() {
    let report = Parser::default()
        .parse_bytes(
            "tagged-subsection.1",
            b".Dd September 3, 2026\n.Dt TAGGED-SUBSECTION 1\n.Os\n\
.Sh DESCRIPTION\n.Tg subsection-target\n.Ss CUSTOM SUBSECTION\ntext\n",
        )
        .expect("parse explicit target before subsection");

    let explicit_target = find_node(&report.document.root, &|node| {
        node.flags.deep_link_target && node.tag.as_deref() == Some("subsection-target")
    })
    .expect("explicit target must remain represented");
    assert_eq!(explicit_target.macro_name.as_deref(), Some("Ss"));

    let subsection = find_macro(&report.document.root, "Ss").expect("subsection block");
    assert!(
        find_node(subsection, &|node| {
            node.kind == NodeKind::Head && node.flags.deep_link_target
        })
        .is_some()
    );
}

#[test]
fn parser_normalizes_internal_sentinels_in_validated_tags() {
    let report = Parser::default()
        .parse_bytes(
            "tag-sentinel.1",
            b".TH TAG-SENTINEL 1\n.SH OPTIONS\n.TP\n\\fB\\-\\-new-window\\fR\nOpen a window.\n",
        )
        .expect("parse tagged paragraph");
    let tagged = find_node(&report.document.root, &|node| {
        node.tag
            .as_deref()
            .is_some_and(|tag| tag.contains("new-window"))
    });

    assert!(
        tagged.is_some(),
        "normalized TP tag must remain addressable"
    );
    assert!(
        find_node(&report.document.root, &|node| {
            node.tag.as_deref().is_some_and(|tag| {
                tag.chars().any(|character| {
                    ['\u{1a}', '\u{1c}', '\u{1d}', '\u{1e}', '\u{1f}'].contains(&character)
                })
            })
        })
        .is_none()
    );
}

#[test]
fn automatic_option_tags_normalize_escaped_hyphens_at_the_same_owner() {
    for spelling in ["new-window", "new\\-window"] {
        let source = format!(
            ".Dd September 11, 2026\n.Dt PROBE 1\n.Os ManT\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl {spelling}\nOpen a window.\n.El\n"
        );
        let report = Parser::default()
            .parse_bytes("tag-hyphen.1", source.as_bytes())
            .expect("parse option tag with equivalent hyphen spelling");
        let target = find_node(&report.document.root, &|node| {
            node.flags.deep_link_target && node.tag.as_deref() == Some("new-window")
        })
        .expect("normalized automatic option destination");
        assert_eq!(target.line, 6, "{spelling}: {target:?}");
        assert!(find_macro(target, "Fl").is_some(), "{spelling}: {target:?}");
    }
}
