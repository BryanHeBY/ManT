//! Reference inventory and exact source-row geometry remain independent of entries.
use super::*;

#[test]
fn four_tagged_bullets_keep_separate_bodies_across_consumers() {
    // This exact four-TP input was checked with pinned CVS
    // tree/HTML/UTF-8. man_html.c::list_continues gives the four literal
    // `\ \ \ \(bu` HEADs one Bl-tag DL, with separate DT/DD ownership.
    let source = br".TH GDB 1
.SH DESCRIPTION
.TP
\ \ \ \(bu
Start your program.
.TP
\ \ \ \(bu
Make your program stop.
.TP
\ \ \ \(bu
Examine what has happened.
.TP
\ \ \ \(bu
Change things in your program.
";
    let loaded = mant_loader::load_roff_bytes(source).expect("lower four tagged bullets");
    let document = loaded.document.as_ref().expect("document");
    let description = document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .expect("DESCRIPTION section");
    let items = description
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::DefinitionList { items, .. } if items.len() == 4 => Some(items),
            _ => None,
        })
        .expect("four native tagged bullet rows");
    let bodies = [
        "Start your program",
        "Make your program stop",
        "Examine what has happened",
        "Change things in your program",
    ];
    for (item, body) in items.iter().zip(bodies) {
        assert_eq!(mant_ir::inline_plain_text(&item.terms[0]).trim(), "•");
        assert!(item.entry.is_none());
        assert!(item.description.iter().any(|block| matches!(
            block,
            Block::Paragraph { children, .. }
                if mant_ir::inline_plain_text(children).contains(body)
        )));
    }
    let query = ResolvedContent {
        label: "gdb".into(),
        address: None,
        document: Some(document.clone()),
        tldr: None,
    };
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&query))
        .expect("serialize real gdb query");
    let decoded: mant_protocol::QueryBundle =
        serde_json::from_str(&wire).expect("decode gdb query");
    let query: ResolvedContent = decoded.into();
    let outline =
        mant_query::build_outline_projection(&query, mant_protocol::EntryProjection::All, None)
            .expect("project gdb outline");
    let outline_text = serde_json::to_string(&outline).expect("serialize gdb outline");
    assert!(!outline_text.contains('•'), "{outline_text}");
    for output in [
        mant_codec::encode::render_markdown(&query),
        mant_render::render_query_text(&query),
    ] {
        let mut previous = 0;
        for body in bodies {
            let next = output[previous..].find(body).map_or_else(
                || panic!("lost {body}: {output}"),
                |offset| previous + offset,
            );
            previous = next + body.len();
        }
    }
    for width in [20, 80, 160] {
        let rendered = DocumentView::new(&query).render(width);
        let rows = ["Start your", "Make your", "Examine what", "Change things"]
            .map(|term| rendered.search(term)[0].row);
        assert!(
            rows.windows(2).all(|pair| pair[0] < pair[1]),
            "width={width}: {rows:?}"
        );
        assert!(
            rendered
                .text
                .lines
                .iter()
                .filter(|line| line.to_string().contains('•'))
                .count()
                >= 4,
            "width={width}: bullet terms missing"
        );
    }
}

#[test]
fn representative_man_flow_survives_wire_outline_explanation_and_viewports() {
    // Exact source checked with pinned CVS -Ttree/-Thtml/-Tutf8.
    // man_html.c::man_IP_pre groups adjacent dash HEADs, while .RS supplies
    // one inset and .RE restores the following paragraph's origin.
    let source = b".TH MANFLOW 1\n.SH DESCRIPTION\nalpha\nbeta\n.br\ngamma\n.nf\nlineA\nlineB\n.fi\n.PP\n.TP\n.B --flag\noption body\n.RS 3\n.IP \\- 2\nfirst\n.IP \\- 2\nsecond\n.RE\ntail\n";
    let loaded = mant_loader::load_roff_bytes(source).expect("lower representative man page");
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&loaded))
        .expect("serialize representative man page");
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).expect("decode query");
    let query: ResolvedContent = decoded.into();
    let outline =
        mant_query::build_outline_projection(&query, mant_protocol::EntryProjection::All, None)
            .expect("project man outline");
    let outline_text = serde_json::to_string(&outline).expect("serialize outline");
    assert!(outline_text.contains("--flag"), "{outline_text}");
    let explanation = mant_query::explain_query(
        &query,
        &mant_protocol::ExplanationQuery {
            entry: "--flag".into(),
            options: mant_protocol::ExplanationOptions::default(),
        },
    )
    .expect("explain man option");
    assert_eq!(explanation.counts.direct_entry.total, 1);
    assert!(mant_render::render_explanation_text(&explanation).contains("option body"));
    let markdown = mant_codec::encode::render_markdown(&query);
    assert!(markdown.contains("alpha beta"), "{markdown}");
    assert!(markdown.contains("lineA\nlineB"), "{markdown}");
    assert!(markdown.contains("- first"), "{markdown}");
    assert!(markdown.contains("- second"), "{markdown}");
    let cli = mant_render::render_query_text(&query);
    assert!(cli.contains("alpha beta"), "{cli}");
    assert!(cli.contains("lineA\nlineB"), "{cli}");
    for width in [20, 80, 160] {
        let rendered = DocumentView::new(&query).render(width);
        let rows = [
            "alpha", "gamma", "lineA", "lineB", "--flag", "first", "second", "tail",
        ]
        .map(|term| rendered.search(term)[0].row);
        assert!(
            rows.windows(2).all(|pair| pair[0] < pair[1]),
            "width={width}: {rows:?}"
        );
        let list = rendered.text.lines[rows[5]].to_string();
        let tail = rendered.text.lines[rows[7]].to_string();
        assert!(list.contains("- first"), "width={width}: {list:?}");
        assert!(
            list.chars().take_while(|ch| *ch == ' ').count()
                > tail.chars().take_while(|ch| *ch == ' ').count(),
            "width={width}: list={list:?} tail={tail:?}"
        );
    }
}

#[test]
fn man_adjacent_dash_ips_keep_run_structure_and_nested_reading_widths() {
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        widgets::{Paragraph, Widget},
    };

    // Exact source checked with pinned CVS -Ttree/-Thtml/-Tutf8. CVS
    // man_html.c::list_continues() groups adjacent raw `\\-` IP HEADs into
    // Bl-dash; RS adds one parent inset and RE releases the following prose.
    let source =
        b".TH IPDASH 1\n.SH OPTIONS\n.RS 3\n.IP \\- 2\nfirst\n.IP \\- 2\nsecond\n.RE\nafter\n";
    let loaded = mant_loader::load_roff_bytes(source).expect("lower adjacent man IPs");
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&loaded))
        .expect("serialize man IP run");
    let value: serde_json::Value = serde_json::from_str(&wire).expect("inspect wire");
    let blocks = &value["document"]["sections"][0]["blocks"];
    assert_eq!(blocks[0]["kind"]["kind"], "dash");
    assert_eq!(blocks[0]["items"].as_array().map(Vec::len), Some(2));
    assert!(
        blocks[0]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item.get("entry").is_none())
    );
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).expect("decode query");
    let query: ResolvedContent = decoded.into();
    let markdown = mant_codec::encode::render_markdown(&query);
    assert!(markdown.contains("- first"), "{markdown}");
    assert!(markdown.contains("- second"), "{markdown}");
    assert!(markdown.contains("after"), "{markdown}");

    for width in [20, 80, 160] {
        let rendered = DocumentView::new(&query).render(width);
        let first_row = rendered.search("first")[0].row;
        let second_row = rendered.search("second")[0].row;
        let after_row = rendered.search("after")[0].row;
        assert!(first_row < second_row && second_row < after_row);
        let list_line = rendered.text.lines[first_row].to_string();
        let after_line = rendered.text.lines[after_row].to_string();
        assert!(
            list_line.contains("- first"),
            "width={width}: {list_line:?}"
        );
        assert!(
            list_line.chars().take_while(|ch| *ch == ' ').count()
                > after_line.chars().take_while(|ch| *ch == ' ').count(),
            "width={width}: list={list_line:?} after={after_line:?}"
        );
        let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
        let mut buffer = Buffer::empty(area);
        Paragraph::new(rendered.text).render(area, &mut buffer);
        let row = u16::try_from(first_row).expect("bounded sample row");
        let visible = (0..width)
            .map(|column| buffer[(column, row)].symbol())
            .collect::<String>();
        assert!(visible.contains("- first"), "width={width}: {visible:?}");
    }
}

#[test]
fn singleton_man_bullet_stays_readable_without_a_false_outline_entry() {
    // Pinned CVS man_html.c::man_IP_pre() retains independent DLs for this
    // exact bullet-plus-option input. The bullet is a visible DT, not a name.
    let source = b".TH MARK 1\n.SH OPTIONS\n.IP \\(bu 4\nBODY\n.TP\n.B --flag\nreal option\n";
    let loaded = mant_loader::load_roff_bytes(source).expect("lower man bullet and option");
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&loaded))
        .expect("serialize man bullet query");
    let value: serde_json::Value = serde_json::from_str(&wire).expect("inspect query wire");
    let blocks = &value["document"]["sections"][0]["blocks"];
    assert!(blocks[0]["items"][0].get("entry").is_none());
    assert_eq!(blocks[0]["items"][1]["entry"]["names"][0], "--flag");
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).expect("decode query");
    let query: ResolvedContent = decoded.into();
    let outline =
        mant_query::build_outline_projection(&query, mant_protocol::EntryProjection::All, None)
            .expect("project option outline");
    let outline_text = serde_json::to_string(&outline).expect("serialize outline");
    assert!(!outline_text.contains('•'), "{outline_text}");
    assert!(outline_text.contains("--flag"), "{outline_text}");
    let rendered = DocumentView::new(&query).render(80);
    assert!(!rendered.search("BODY").is_empty());
    assert!(!rendered.search("real option").is_empty());
}

#[test]
fn standalone_tq_bullet_remains_readable_without_a_semantic_entry() {
    // Exact source checked with pinned CVS -Ttree/-Thtml/-Tutf8.
    // man_macro.c::blk_imp gives TQ a next-line HEAD; man_html.c::man_IP_pre
    // renders that bullet as a DT in a Bl-tag DL, not as a declaration name.
    let source = b".TH MARK 1\n.SH OPTIONS\n.TQ\n\\(bu\nBODY\n";
    let loaded = mant_loader::load_roff_bytes(source).expect("lower standalone TQ bullet");
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&loaded))
        .expect("serialize TQ query");
    let value: serde_json::Value = serde_json::from_str(&wire).expect("inspect TQ wire");
    assert!(
        value["document"]["sections"][0]["blocks"][0]["items"][0]
            .get("entry")
            .is_none()
    );
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).expect("decode TQ query");
    let query: ResolvedContent = decoded.into();
    let outline =
        mant_query::build_outline_projection(&query, mant_protocol::EntryProjection::All, None)
            .expect("project TQ outline");
    let outline_text = serde_json::to_string(&outline).expect("serialize TQ outline");
    assert!(!outline_text.contains('•'), "{outline_text}");
    let explanation = mant_query::explain_query(
        &query,
        &mant_protocol::ExplanationQuery {
            entry: "•".into(),
            options: mant_protocol::ExplanationOptions::default(),
        },
    )
    .expect("explain bullet without a semantic owner");
    assert_eq!(explanation.counts.direct_entry.total, 0);
    let rendered = DocumentView::new(&query).render(80);
    assert!(
        rendered
            .text
            .lines
            .iter()
            .any(|line| line.to_string().contains('•'))
    );
    assert!(!rendered.search("BODY").is_empty());
}

#[test]
fn crossed_function_target_and_dash_list_survive_wire_markdown_and_terminal_buffer() {
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        widgets::{Paragraph, Widget},
    };

    // Exact source checked with pinned CVS -Ttree/-Thtml/-Tutf8. The Fo HEAD
    // owns Explicit.Call; mdoc_html.c::print_mdoc_node posts Fo at the body
    // end inside It, and mdoc_term.c emits a dash marker for Bl -dash.
    let source = b".Dd September 28, 2026\n.Dt FOTARGET 1\n.Os\n.Sh DESCRIPTION\n.Tg Explicit.Call\n.Fo call\n.Bl -dash\n.It\n.Fa arg\n.Fc\n.El\nnext\n";
    let loaded = mant_loader::load_roff_bytes(source).expect("lower crossed function and list");
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&loaded))
        .expect("serialize function query");
    let value: serde_json::Value = serde_json::from_str(&wire).expect("inspect wire");
    let blocks = &value["document"]["sections"][0]["blocks"];
    assert_eq!(blocks[0]["children"][0]["id"], "explicit-call");
    assert_eq!(
        blocks[0]["children"][0]["fragmentAliases"][0],
        "Explicit.Call"
    );
    assert_eq!(blocks[1]["kind"]["kind"], "dash");
    assert!(blocks[1]["items"][0].get("entry").is_none());
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).expect("decode query");
    let query: ResolvedContent = decoded.into();
    let markdown = mant_codec::encode::render_markdown(&query);
    assert!(markdown.contains("**call**("), "{markdown}");
    assert!(markdown.contains("- *arg*)"), "{markdown}");

    for width in [24, 80, 160] {
        let rendered = DocumentView::new(&query).render(width);
        let anchor_row = rendered
            .anchor_row("explicit-call")
            .expect("Fo HEAD anchor");
        assert!(
            rendered.text.lines[anchor_row]
                .to_string()
                .contains("call(")
        );
        let arg = &rendered.search("arg")[0];
        assert!(rendered.text.lines[arg.row].to_string().contains("- "));
        assert!(rendered.text.lines[arg.row].to_string().contains("arg)"));
        let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
        let mut buffer = Buffer::empty(area);
        Paragraph::new(rendered.text).render(area, &mut buffer);
        let row = u16::try_from(arg.row).expect("bounded sample row");
        let visible = (0..width)
            .map(|column| buffer[(column, row)].symbol())
            .collect::<String>();
        assert!(visible.contains("arg)"), "width={width}: {visible:?}");
    }
}

#[test]
fn roff_manual_name_link_excludes_surrounding_prose_after_wrapping() {
    // Keep packaged unit tests self-contained. The engine's repository-level
    // self_manual_authoring test separately checks the actual shipped labels.
    let source = "The following [man(7)](https://mandoc.bsd.lv/man/man.7.html) macros documented by mandoc have dedicated lowering behavior:";
    let query = mant_loader::load_markdown_text(source, None).unwrap();
    let view = DocumentView::new(&query);
    let target = LinkTarget::External(
        model::ExternalUri::parse("https://mandoc.bsd.lv/man/man.7.html").unwrap(),
    );
    for width in [4, 12, 40, 120] {
        let rendered = view.render(width);
        let mut clickable = String::new();
        for (row, line) in rendered.text.lines.iter().enumerate() {
            let mut column = 0;
            for character in line.to_string().chars() {
                if rendered.link_target_at(row, column) == Some(&target) {
                    clickable.push(character);
                }
                column += character.to_string().width();
            }
        }
        assert_eq!(clickable, "man(7)", "width={width}");
    }
}

#[test]
fn empty_man_link_bodies_use_clickable_head_text_in_markdown_and_tui() {
    // Both exact sources were checked with fixed CVS -Thtml/-Tascii/-Tlint.
    // man_html.c::man_UR_pre() uses HEAD as the anchor label only when BODY
    // has no child; the terminal still prints the generated target brackets.
    for (open, close, address, destination) in [
        ("UR", "UE", "https://example.com", "https://example.com"),
        ("MT", "ME", "user@example.com", "mailto:user@example.com"),
    ] {
        let source = format!(".TH TEST 1\n.SH DESCRIPTION\n.{open} {address}\n.{close}\n");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower empty BODY");
        let markdown = mant_codec::encode::render_markdown(&query);
        assert!(markdown.contains(address), "{open}: {markdown}");
        assert!(!markdown.contains("[]("), "{open}: {markdown}");
        assert!(
            markdown.contains(&format!("<{destination}>"))
                || markdown.contains(&format!("[{address}]({destination})")),
            "{open}: {markdown}"
        );

        let target = LinkTarget::External(model::ExternalUri::parse(destination).unwrap());
        let view = DocumentView::new(&query);
        for width in [12, 40, 120] {
            let rendered = view.render(width);
            let mut clickable = String::new();
            for (row, line) in rendered.text.lines.iter().enumerate() {
                let mut column = 0;
                for character in line.to_string().chars() {
                    if rendered.link_target_at(row, column) == Some(&target) {
                        clickable.push(character);
                    }
                    column += character.to_string().width();
                }
            }
            assert_eq!(clickable, address, "{open}, width={width}");
        }
    }
}

fn document_link(label: &str, fragment: Option<&str>) -> Inline {
    Inline::Link {
        target: mant_ir::LinkTarget::Document {
            name: "target".into(),
            fragment: fragment.map(str::to_owned),
        },
        title: None,
        children: vec![Inline::Text {
            value: label.into(),
        }],
    }
}

fn linked_block(prefix: &str, label: &str) -> Block {
    Block::Paragraph {
        children: vec![
            Inline::Text {
                value: prefix.into(),
            },
            document_link(label, None),
        ],
        layout: LayoutHint::default(),
        source: None,
    }
}

#[test]
fn associated_heading_and_forms_do_not_create_redundant_reference_groups() {
    let query = mant_loader::load_markdown_text(
        "# [Catalog](catalog.md)\n\n## [Commands](commands.md)\n\n<!-- mant:entries role=command case=sensitive -->\n- [`git-add`](git-add.md): See [tutorial](tutorial.md).\n",
        None,
    ).unwrap();
    let before = query.clone();
    let view = DocumentView::new(&query);
    let command = view
        .navigation
        .iter()
        .find(|node| node.title == "git-add")
        .unwrap();
    assert!(matches!(
        command.kind,
        NavKind::Entry(mant_ir::EntryKind::Command)
    ));
    assert_eq!(view.reference_badges[&command.id], "↗ git-add");
    assert_eq!(view.associated_reference_choices(&command.id).len(), 1);
    assert_eq!(view.reference_badges.len(), 3);
    assert_eq!(view.references.len(), 4);
    let body = view
        .navigation
        .iter()
        .filter(|node| node.kind == NavKind::Reference)
        .collect::<Vec<_>>();
    assert_eq!(body.len(), 1);
    assert!(body[0].title.contains("tutorial"));
    assert_eq!(query, before);
    for width in [12, 40, 80] {
        let rendered = view.render(width);
        for reference in &view.references {
            assert!(rendered.anchor_row(&reference.id).is_some());
        }
    }
}

#[test]
fn native_command_form_and_body_references_share_inventory_not_presentation() {
    let query = mant_loader::load_roff_bytes(b".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh COMMANDS\n.Bl -tag -width Ds\n.It Xr git-add 1\nSee\n.Xr git-add 1\nand\n.Xr gittutorial 7 .\n.El\n").unwrap();
    let view = DocumentView::new(&query);
    assert_eq!(view.references.len(), 3);
    assert_eq!(
        view.reference_text(&view.references[0].id).as_deref(),
        Some("man:git-add(1)")
    );
    assert_eq!(
        view.reference_badges.len(),
        1,
        "{:?}",
        crate::document::references::ReferenceNavigation::build(query.document.as_ref().unwrap())
    );
    let owner = view.reference_badges.keys().next().unwrap();
    assert!(
        view.navigation
            .iter()
            .any(|node| node.id == *owner && matches!(node.kind, NavKind::Entry(_)))
    );
    assert_eq!(view.associated_reference_choices(owner).len(), 1);
    assert_eq!(
        view.navigation
            .iter()
            .filter(|node| node.kind == NavKind::Reference)
            .count(),
        2
    );
}

#[test]
fn invalid_form_association_falls_back_without_hiding_any_source() {
    let mut query = mant_loader::load_markdown_text(
        "# Demo\n\n## Commands\n\n<!-- mant:entries role=command case=sensitive -->\n- [`command`](command.md): Description.\n", None).unwrap();
    let document = query.document.as_mut().unwrap();
    let Block::List { items, .. } = &mut document.sections[0].blocks[0] else {
        panic!("expected list")
    };
    items[0].entry.as_mut().unwrap().forms[0].parts[0].path = vec![999];
    let view = DocumentView::new(&query);
    assert!(view.reference_badges.is_empty());
    assert_eq!(view.references.len(), 1);
    assert_eq!(
        view.navigation
            .iter()
            .filter(|node| node.kind == NavKind::Reference)
            .count(),
        1
    );
}

#[test]
fn hidden_or_invalid_form_owner_uses_body_fallback_not_section_badge() {
    let original = mant_loader::load_markdown_text(
        "# Demo\n\n## Commands\n\n<!-- mant:entries role=command case=sensitive -->\n- [`command`](command.md): Description.\n", None).unwrap();
    let mut view = DocumentView::new(&original);
    view.navigation
        .retain(|node| !matches!(node.kind, NavKind::Entry(_) | NavKind::EntryGroup));
    let mut references = crate::document::references::ReferenceNavigation::build(
        original.document.as_ref().unwrap(),
    );
    references.check_source_owners(
        original.document.as_ref().unwrap(),
        &mant_ir::SemanticIndex::build(original.document.as_ref().unwrap()),
    );
    references.append_navigation(&mut view.navigation);
    assert!(references.associated.is_empty());
    assert_eq!(
        view.navigation
            .iter()
            .filter(|node| node.kind == NavKind::Reference)
            .count(),
        1
    );

    let mut invalid = original;
    let Block::List { items, .. } = &mut invalid.document.as_mut().unwrap().sections[0].blocks[0]
    else {
        panic!("expected list")
    };
    items[0].entry.as_mut().unwrap().id = "Invalid.Owner".into();
    let view = DocumentView::new(&invalid);
    assert!(view.reference_badges.is_empty());
    assert_eq!(view.references.len(), 1);
    assert!(
        view.navigation
            .iter()
            .any(|node| node.kind == NavKind::Reference)
    );
}

#[test]
fn hidden_owner_cannot_lend_its_badge_to_an_unrelated_visible_same_id() {
    let mut query = mant_loader::load_markdown_text(
        "# Demo\n\n## Commands\n\n<!-- mant:entries role=command case=sensitive -->\n- [`first`](first.md): First.\n- `second`: Unlinked.\n", None).unwrap();
    let Block::List { items, .. } = &mut query.document.as_mut().unwrap().sections[0].blocks[0]
    else {
        panic!("expected list")
    };
    let id = items[0].entry.as_ref().unwrap().id.clone();
    items[1].entry.as_mut().unwrap().id = id.clone();
    let view = DocumentView::new(&query);
    let mut nodes = view
        .navigation
        .into_iter()
        .filter(|node| {
            node.kind != NavKind::Reference
                && node.kind != NavKind::ReferenceGroup
                && node.title != "first"
        })
        .collect::<Vec<_>>();
    assert_eq!(
        nodes.iter().filter(|node| node.id == id.as_str()).count(),
        1
    );
    let mut references =
        crate::document::references::ReferenceNavigation::build(query.document.as_ref().unwrap());
    references.check_source_owners(
        query.document.as_ref().unwrap(),
        &mant_ir::SemanticIndex::build(query.document.as_ref().unwrap()),
    );
    references.append_navigation(&mut nodes);
    assert!(references.associated.is_empty());
    assert_eq!(
        nodes
            .iter()
            .filter(|node| node.kind == NavKind::Reference)
            .count(),
        1
    );
    assert!(
        nodes
            .iter()
            .filter(|node| node.kind == NavKind::ReferenceGroup)
            .all(|node| node.parent_id.as_deref() != Some(id.as_str()))
    );
}

#[test]
fn limited_associated_inventory_never_claims_a_single_target_is_unique() {
    let mut query = bundle();
    query.document.as_mut().unwrap().heading = Some(mant_ir::Heading {
        content: (0..1001)
            .map(|_| document_link("same", Some("part")))
            .collect(),
        source: None,
    });
    let view = DocumentView::new(&query);
    assert!(view.references_limited());
    assert!(view.references.len() <= 1000);
    let badge = &view.reference_badges[mant_ir::DOCUMENT_ROOT_ID];
    assert!(badge.contains("known") && badge.contains("more may exist"));
    assert_eq!(
        view.associated_reference_choices(mant_ir::DOCUMENT_ROOT_ID)
            .len(),
        view.references.len()
    );
}

#[test]
fn references_group_full_targets_without_promoting_entries_or_rewriting_body() {
    let mut query = bundle();
    query.document.as_mut().unwrap().sections[0].blocks = vec![Block::Paragraph {
        children: vec![
            document_link("first", None),
            Inline::Text {
                value: " then ".into(),
            },
            document_link("second", None),
            document_link("fragment", Some("part")),
            document_link("", None),
        ],
        layout: LayoutHint::default(),
        source: None,
    }];
    let before = query.clone();
    let view = DocumentView::new(&query);
    assert_eq!(view.references.len(), 4);
    assert_eq!(
        view.navigation()
            .iter()
            .filter(|node| matches!(node.kind, NavKind::Entry(_)))
            .count(),
        0
    );
    assert_eq!(
        view.navigation()
            .iter()
            .filter(|node| node.kind == NavKind::Reference)
            .count(),
        4
    );
    assert!(
        view.navigation()
            .iter()
            .any(|node| node.title.ends_with("3 locations"))
    );
    assert!(
        view.references
            .iter()
            .any(|reference| reference.label.contains("unlabelled"))
    );
    for width in [12, 40, 80] {
        let rendered = view.render(width);
        for reference in &view.references {
            assert!(
                rendered.anchor_row(&reference.id).is_some(),
                "{reference:?}"
            );
        }
        assert!(!rendered.text.to_string().contains("unlabelled"));
        assert!(!rendered.text.to_string().contains("DOCUMENT REFERENCES"));
    }
    assert_eq!(query, before);
    let second = DocumentView::new(&query);
    assert_eq!(
        view.references
            .iter()
            .map(|r| (&r.id, &r.location))
            .collect::<Vec<_>>(),
        second
            .references
            .iter()
            .map(|r| (&r.id, &r.location))
            .collect::<Vec<_>>()
    );
}

#[test]
fn reference_origins_follow_actual_occurrence_through_wrapping_and_table_stacking() {
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.heading = Some(mant_ir::Heading {
        content: vec![document_link("ROOTLINK", None)],
        source: None,
    });
    document.sections[0].heading = mant_ir::Heading {
        content: vec![document_link("HEADLINK", None)],
        source: None,
    };
    document.sections[0].blocks = vec![
        linked_block("日本 e\u{301}\tbefore before before ", "BODYLINK"),
        Block::List {
            kind: mant_ir::ListKind::Bullet,
            compact: true,
            items: vec![ListItem {
                blocks: vec![linked_block("list prefix ", "LISTLINK")],
                entry: None,
                layout: mant_ir::ListItemLayout::default(),
                source: None,
            }],
            layout: LayoutHint::default(),
            source: None,
        },
        Block::Table {
            rows: vec![TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![
                    TableCell {
                        kind: mant_ir::TableCellKind::Text,
                        blocks: vec![linked_block("left ", "CELLLINK")],
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    },
                    TableCell {
                        kind: mant_ir::TableCellKind::Text,
                        blocks: vec![paragraph("right column has substantial wrapped content")],
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    },
                ],
            }],
            layout: LayoutHint::default(),
            source: None,
        },
    ];
    let view = DocumentView::new(&query);
    assert_eq!(view.references.len(), 5);
    for width in [8, 12, 24, 80] {
        let rendered = view.render(width);
        for reference in &view.references {
            let found = rendered.search(&reference.label);
            assert!(
                !found.is_empty(),
                "width={width} {reference:?}\n{}",
                rendered.text
            );
            assert_eq!(
                rendered.anchor_row(&reference.id),
                Some(found[0].row),
                "width={width} {reference:?}\n{}",
                rendered.text
            );
        }
    }
}

#[test]
fn bounded_inventory_exposes_truncation_without_removing_body_links() {
    let mut query = bundle();
    query.document.as_mut().unwrap().sections[0].blocks = vec![Block::Paragraph {
        children: (0..1002).map(|_| document_link("x", None)).collect(),
        layout: LayoutHint::default(),
        source: None,
    }];
    let view = DocumentView::new(&query);
    assert_eq!(view.references.len(), 1000);
    assert!(
        view.navigation()
            .iter()
            .any(|node| node.kind == NavKind::ReferenceNotice)
    );
    assert_eq!(view.render(80).text.to_string().matches('x').count(), 1002);
}

#[test]
fn empty_only_reference_keeps_a_reveal_location_without_manufactured_text() {
    let mut query = bundle();
    query.document.as_mut().unwrap().sections[0].blocks =
        vec![linked_block("", ""), paragraph("AFTER")];
    let view = DocumentView::new(&query);
    let rendered = view.render(80);
    assert_eq!(view.references.len(), 1);
    assert_eq!(
        rendered.anchor_row(&view.references[0].id),
        Some(rendered.search("AFTER")[0].row)
    );
    assert!(!rendered.text.to_string().contains("target"));
}

#[test]
fn definition_term_and_run_in_description_keep_separate_source_origins() {
    for inline_term in [false, true] {
        let mut query = bundle();
        query.document.as_mut().unwrap().sections[0].blocks = vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![DefinitionItem {
                terms: vec![vec![
                    Inline::Text {
                        value: "日本 ".into(),
                    },
                    document_link("TERM", None),
                ]],
                description: vec![linked_block("body prefix ", "TAILREF")],
                entry: None,
                source: None,
                layout: mant_ir::DefinitionLayout {
                    head_body_relation: mant_ir::HeadBodyRelation::from(inline_term),
                    ..Default::default()
                },
            }],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }];
        let view = DocumentView::new(&query);
        assert_eq!(view.references.len(), 2);
        for width in [8, 12, 40, 80] {
            let rendered = view.render(width);
            for reference in &view.references {
                assert_eq!(
                    rendered.anchor_row(&reference.id),
                    Some(rendered.search(&reference.label)[0].row),
                    "inline={inline_term} width={width}\n{}",
                    rendered.text
                );
            }
        }
    }
}

#[test]
fn duplicate_invalid_owner_ids_do_not_duplicate_or_misassign_references() {
    let mut query = bundle();
    query.document.as_mut().unwrap().sections[0].blocks = vec![linked_block("", "FIRST")];
    let mut second = query.document.as_ref().unwrap().sections[0].clone();
    second.blocks = vec![linked_block("", "SECOND")];
    query.document.as_mut().unwrap().sections.push(second);
    let view = DocumentView::new(&query);
    assert_eq!(view.references.len(), 2);
    assert_eq!(
        view.navigation()
            .iter()
            .filter(|node| node.kind == NavKind::Reference)
            .count(),
        2
    );
    let rendered = view.render(80);
    for reference in &view.references {
        assert_eq!(
            rendered.anchor_row(&reference.id),
            Some(rendered.search(&reference.label)[0].row)
        );
    }
}
