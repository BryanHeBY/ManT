//! Target ownership behavior contracts.

use super::*;
#[test]
fn argumentless_target_requests_bind_each_following_source_owner() {
    let document = parse_manual_bytes(
        std::path::Path::new("derived-target-owners.7"),
        b".Dd September 4, 2026\n.Dt DERIVED-TARGET-OWNERS 7\n.Os\n.Sh DESCRIPTION\n\
.Tg\n\
.Ic first-command\n\
.Pp\n\
Paragraph before the second request.\n\
.Tg\n\
.Ic second-command\n\
.Tg explicit-third\n\
.Ic third-command\n",
    )
    .expect("lower consecutive target ownership forms");

    let index = mant_ir::DocumentIndex::build(&document);
    for target in ["first-command", "second-command", "explicit-third"] {
        assert!(
            index.fragment_target(target).is_some(),
            "missing target request {target}"
        );
    }
    assert_eq!(
        index.fragment_target("third-command"),
        None,
        "an explicit .Tg spelling must replace the automatic owner spelling"
    );
}

#[test]
fn preserves_targets_moved_to_paragraphs_and_displays() {
    let paragraph = parse_manual_bytes(
        std::path::Path::new("paragraph-target.3"),
        b".Dd September 3, 2026\n.Dt PARAGRAPH-TARGET 3\n.Os\n.Sh DESCRIPTION\n\
intro\n.Pp\n.Fn alpha\n",
    )
    .expect("lower an automatic function target moved to Pp");
    assert!(anchor_ids(&paragraph).iter().any(|id| id == "alpha"));
    assert!(
        anchor_owner_lines(&paragraph)
            .iter()
            .any(|(id, line)| id == "alpha" && *line == 6)
    );
    assert!(visible_document_text(&paragraph).contains("alpha"));

    for (name, display) in [
        ("Bd", ".Bd -literal\nhello\n.Ed"),
        ("D1", ".D1 hello"),
        ("Dl", ".Dl hello"),
    ] {
        let source = format!(
            ".Dd September 3, 2026\n.Dt DISPLAY-TARGET 1\n.Os\n.Sh DESCRIPTION\n.Tg display-target\n{display}\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("{name}-target.1")),
            source.as_bytes(),
        )
        .unwrap_or_else(|error| panic!("lower {name} target: {error}"));
        assert_eq!(anchor_ids(&document), ["display-target"]);
        assert_eq!(visible_document_text(&document).trim(), "DESCRIPTION hello");
        assert!(matches!(
            document.sections[0].blocks.first(),
            Some(Block::Preformatted { children, .. })
                if matches!(children.first(), Some(Inline::Anchor { id, .. }) if id == "display-target")
        ));
    }
}

#[test]
fn preserves_targets_moved_to_list_items_and_containers() {
    let item = parse_manual_bytes(
        std::path::Path::new("list-item-target.1"),
        b".Dd September 3, 2026\n.Dt LIST-ITEM-TARGET 1\n.Os\n.Sh DESCRIPTION\n\
.Bl -bullet\n.It\n.Tg bullet-target\n.Em bullet text\n.El\n",
    )
    .expect("lower a target moved to an ordinary list item");
    assert_eq!(anchor_ids(&item), ["bullet-target"]);
    assert!(visible_document_text(&item).contains("bullet text"));

    let container = parse_manual_bytes(
        std::path::Path::new("list-container-target.1"),
        b".Dd September 3, 2026\n.Dt LIST-CONTAINER-TARGET 1\n.Os\n.Sh DESCRIPTION\n\
.Tg list-target\n.Bl -bullet\n.It\nhello\n.El\n",
    )
    .expect("lower a target moved to a list container");
    assert_eq!(anchor_ids(&container), ["list-target"]);
    assert_eq!(
        visible_document_text(&container).trim(),
        "DESCRIPTION hello"
    );
}

#[test]
fn same_named_list_target_owners_retain_their_individual_source_positions() {
    let document = parse_manual_bytes(
        std::path::Path::new("list-target-owner-positions.7"),
        b".Dd September 4, 2026\n.Dt LIST-TARGET-OWNER-POSITIONS 7\n.Os\n.Sh DESCRIPTION\n\
.Bl -bullet\n.Tg same\n.It\nfirst\n.Tg same\n.It\nsecond\n.El\n",
    )
    .expect("lower repeated targets onto separate list items");

    let owners = anchor_owner_lines(&document)
        .into_iter()
        .filter(|(id, _)| id == "same" || id.starts_with("same-"))
        .collect::<Vec<_>>();
    assert_eq!(owners, [("same".to_owned(), 7), ("same-2".to_owned(), 10)]);
}

#[test]
fn list_target_recovery_preserves_native_and_authored_fragment_spellings() {
    for style in ["-bullet", "-enum", "-item", "-tag", "-column one two"] {
        for (request, target, body) in [
            (".Tg\n.Sm off", "off", "CONTENT"),
            (".Tg Mixed.Target", "Mixed.Target", ""),
        ] {
            let source = format!(
                ".Dd September 5, 2026\n.Dt TARGET 7\n.Os\n.Sh DESCRIPTION\n.Bl {style}\n{request}\n.It\n{body}\n.El\n"
            );
            let document =
                parse_manual_bytes(std::path::Path::new("target.7"), source.as_bytes()).unwrap();
            let index = mant_ir::DocumentIndex::build(&document);
            assert!(index.fragment_target(target).is_some(), "{style}: {target}");
            assert!(
                !document
                    .diagnostics
                    .iter()
                    .any(|d| d.code.as_deref() == Some("ir.invalid-identity"))
            );
            assert!(!visible_document_text(&document).contains(target));
        }
    }
}

#[test]
fn pending_targets_retain_their_native_source_lines_when_no_item_owns_them() {
    let source = b".Dd September 5, 2026\n.Dt TARGETS 7\n.Os\n.Sh DESCRIPTION\n.Bl -bullet\n.Tg\n.Sm off\n.It\nCONTENT\n.El\n.Sm on\n.Bl -column one two\n.Tg Mixed.Target\n.It\n.El\n";
    let document = parse_manual_bytes(std::path::Path::new("target-provenance.7"), source).unwrap();
    let owners = anchor_owner_lines(&document);
    assert!(owners.contains(&("off".to_owned(), 6)));
    assert!(owners.contains(&("mixed-target".to_owned(), 13)));
}

#[test]
fn preserves_explicit_targets_on_empty_mdoc_list_items() {
    let document = parse_manual_bytes(
        std::path::Path::new("empty-list-targets.7"),
        b".Dd September 4, 2026\n.Dt EMPTY-LIST-TARGETS 7\n.Os\n.Sh DESCRIPTION\n\
.Bl -bullet\n.Tg empty-bullet-target\n.It\n.El\n\
.Bl -enum\n.Tg empty-enum-target\n.It\n.El\n\
.Bl -item\n.Tg empty-plain-target\n.It\n.El\n\
.Bl -column \"one\" \"two\"\n.Tg empty-column-target\n.It\n.El\n\
.Bl -tag\n.Tg empty-definition-target\n.It\n.El\n\
.Bl -bullet\n.It\nbody\n.Tg trailing-list-target\n.El\n\
.Bl -bullet\n.Tg target-without-item\n.El\n",
    )
    .expect("lower targets at empty and trailing mdoc list positions");

    let index = mant_ir::DocumentIndex::build(&document);
    for target in [
        "empty-bullet-target",
        "empty-enum-target",
        "empty-plain-target",
        "empty-column-target",
        "empty-definition-target",
        "trailing-list-target",
        "target-without-item",
    ] {
        assert_eq!(
            index.fragment_target(target).map(mant_ir::NodeId::as_str),
            Some(target),
            "missing {target}"
        );
    }
    assert!(document.diagnostics.iter().all(|diagnostic| {
        !matches!(
            diagnostic.code.as_deref(),
            Some("ir.invalid-identity" | "ir.duplicate-identity" | "ir.ambiguous-fragment-alias")
        )
    }));
    let visible = visible_document_text(&document);
    assert_eq!(visible.trim(), "DESCRIPTION body");
    assert!(!visible.contains("target"));
}

#[test]
fn moved_list_target_uses_its_surviving_native_owner_once() {
    // Exact fixed CVS tree/HTML: post_tg() moves NODE_ID to Pp at line 9;
    // the NOPRT Tg at line 7 no longer owns it. Recovery must not invent an
    // additional definition item before that real paragraph destination.
    let source = b".Dd September 9, 2026\n.Dt TARGET 1\n.Os\n.Sh DESCRIPTION\nBEFORE\n.Bl -tag -width Ds\n.Tg empty-target\n.El\n.Pp\nAFTER\n";
    let document = parse_manual_bytes(std::path::Path::new("moved-empty-target.1"), source)
        .expect("lower the validated paragraph target");
    let json = serde_json::to_string(&document).expect("serialize target ownership");
    let decoded: mant_ir::Document =
        serde_json::from_str(&json).expect("decode actual target JSON");
    assert_eq!(decoded, document);
    assert_eq!(anchor_owner_lines(&decoded), [("empty-target".into(), 9)]);
    assert!(
        decoded.sections[0].blocks.iter().any(|block| {
            matches!(block, Block::DefinitionList { items, .. } if items.is_empty())
        })
    );
    assert_eq!(
        mant_ir::DocumentIndex::build(&decoded)
            .fragment_target("empty-target")
            .map(mant_ir::NodeId::as_str),
        Some("empty-target")
    );
    assert!(
        decoded
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.impact == mant_ir::DiagnosticImpact::None })
    );
}

#[test]
fn removed_empty_column_owner_retains_authored_target_recovery() {
    // Exact fixed CVS tree: post_it() removes the empty column It; the Tg
    // remains NOPRT with no surviving NODE_ID anywhere. The established
    // authored-target recovery retains that one zero-width source identity.
    let source = b".Dd September 9, 2026\n.Dt TARGET 1\n.Os\n.Sh DESCRIPTION\nBEFORE\n.Bl -column one two\n.Tg empty-column-target\n.It\n.El\n.Pp\nAFTER\n";
    let document = parse_manual_bytes(std::path::Path::new("removed-empty-column.1"), source)
        .expect("recover an authored target whose native owner was removed");
    let json = serde_json::to_string(&document).expect("serialize recovered target");
    let decoded: mant_ir::Document =
        serde_json::from_str(&json).expect("decode recovered target JSON");
    assert_eq!(decoded, document);
    assert_eq!(
        anchor_owner_lines(&decoded),
        [("empty-column-target".into(), 7)]
    );
    assert_eq!(
        mant_ir::DocumentIndex::build(&decoded)
            .fragment_target("empty-column-target")
            .map(mant_ir::NodeId::as_str),
        Some("empty-column-target")
    );
    assert!(
        decoded
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.impact == mant_ir::DiagnosticImpact::None })
    );
}

#[test]
fn surviving_same_named_owner_takes_precedence_over_removed_owner_recovery() {
    // Exact fixed CVS tree/HTML: the first owner is deleted with the empty
    // column It. Only the later Pp has NODE_ID. Recovery preserves target
    // availability, not an extra speculative owner when that spelling has
    // already survived on the real paragraph. This differs from two actual
    // NODE_ID owners, covered by the duplicate-owner test below.
    let source = b".Dd September 9, 2026\n.Dt TARGET 1\n.Os\n.Sh DESCRIPTION\nBEFORE\n.Bl -column one two\n.Tg repeated-target\n.It\n.El\n.Pp\n.Tg repeated-target\n.No AFTER\n";
    let document = parse_manual_bytes(std::path::Path::new("recovery-precedence.1"), source)
        .expect("prefer the surviving same-named target owner");
    assert_eq!(
        anchor_owner_lines(&document),
        [("repeated-target".into(), 10)]
    );
    assert_eq!(
        mant_ir::DocumentIndex::build(&document)
            .fragment_target("repeated-target")
            .map(mant_ir::NodeId::as_str),
        Some("repeated-target")
    );
    assert!(
        document
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.impact == mant_ir::DiagnosticImpact::None })
    );
}

#[test]
fn surviving_authored_owners_keep_real_duplicates_and_source_positions() {
    // Each exact source was run with fixed CVS before these assertions.
    // tag_put() can retain two real NODE_ID owners with the same authored
    // spelling. The recovery filter cannot erase either native owner or
    // suppress their genuine alias ambiguity. A Tg that still has NODE_ID
    // itself also takes precedence over the recovery-only lookup.
    for (source, owners, ambiguous) in [
        (
            b".Dd September 9, 2026\n.Dt TARGET 1\n.Os\n.Sh DESCRIPTION\nBEFORE\n.Bl -bullet\n.Tg repeated-target\n.It\nFIRST\n.Tg repeated-target\n.It\nSECOND\n.El\n".as_slice(),
            [("repeated-target", 8), ("repeated-target-2", 11)],
            true,
        ),
        (
            b".Dd September 9, 2026\n.Dt TARGET 1\n.Os\n.Sh DESCRIPTION\nBEFORE\n.Bl -bullet\n.Tg first-target\n.It\nFIRST\n.Tg second-target\n.It\nSECOND\n.El\n".as_slice(),
            [("first-target", 8), ("second-target", 11)],
            false,
        ),
        (
            b".Dd September 9, 2026\n.Dt TARGET 1\n.Os\n.Sh DESCRIPTION\nBEFORE\n.Bl -bullet\n.Tg repeated-target\n.It\n.El\n.Pp\n.Tg repeated-target\n.No AFTER\n".as_slice(),
            [("repeated-target", 7), ("repeated-target-2", 10)],
            true,
        ),
    ] {
        let document = parse_manual_bytes(std::path::Path::new("native-target-owners.1"), source)
            .expect("retain actual native target owners");
        assert_eq!(
            anchor_owner_lines(&document),
            owners.map(|(name, line)| (name.to_owned(), line))
        );
        assert_eq!(
            document.diagnostics.iter().any(|diagnostic| {
                diagnostic.code.as_deref() == Some("ir.ambiguous-fragment-alias")
            }),
            ambiguous
        );
        assert!(document.diagnostics.iter().all(|diagnostic| {
            diagnostic.impact != mant_ir::DiagnosticImpact::ContentCoverage
        }));
    }
}

#[test]
fn preserves_targets_moved_to_paragraph_breaks_inside_no_fill_displays() {
    // Exact source rechecked with the fixed five profiles. termp_pp_pre
    // contributes an empty row inside Bd; NODE_ID still marks its following
    // semantic owner with the original source coordinate.
    let document = parse_manual_bytes(
        std::path::Path::new("no-fill-paragraph-target.8"),
        b".Dd September 4, 2026\n.Dt NO-FILL-PARAGRAPH-TARGET 8\n.Os\n\
.Sh EXAMPLES\n\
.Bd -unfilled\n\
.Li first line\n\
.Pp\n\
.Li prompt Sy hp(0,0)\n\
.Ed\n",
    )
    .expect("lower a target moved onto a no-fill paragraph break");

    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "complete display retains the executed paragraph gap: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(children), "first line\n\nprompt hp(0,0)");
    assert!(matches!(children.first(), Some(Inline::Code { value }) if value == "first line"));
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline,
                Inline::Anchor { id, owner_source: Some(source), .. }
                if id == "hp-0-0" && source.line == 7))
            .count(),
        1
    );
    assert!(inline_text(children).contains("prompt hp(0,0)"));
}
