//! Native blocks and lists retain their independent body ownership.
use super::super::{Block, DocumentView, ResolvedContent};

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
    assert_ne!(rendered.search("BODY").len(), 0);
    assert_ne!(rendered.search("real option").len(), 0);
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
    assert_ne!(rendered.search("BODY").len(), 0);
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
