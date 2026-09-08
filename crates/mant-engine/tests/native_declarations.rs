//! Source-owned declaration boundaries, independent of formatting conventions.
use mant_ir::{
    Block, DefinitionItem, Document,
    visit::{self, Visit},
};
use mant_protocol::{EvidenceClass, ExplanationOptions, ExplanationQuery};

fn definitions(document: &Document) -> Vec<&DefinitionItem> {
    struct Items<'a>(Vec<&'a DefinitionItem>);
    impl<'a> Visit<'a> for Items<'a> {
        fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
            self.0.push(item);
            visit::walk_definition_item(self, item);
        }
    }
    let mut result = Items(Vec::new());
    result.visit_document(document);
    result.0
}

#[test]
fn compact_independent_heads_do_not_share_explain_bodies_or_sources() {
    for source in [
        ".TH PROBE 1\n.SH OPTIONS\n.IP --first 4\n.PD 0\n.IP --second 4\n.PD\nSECOND_BODY\n.IP --last 4\n",
        ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --first\n.PD 0\n.TP\n.B --second\n.PD\nSECOND_BODY\n.TP\n.B --last\n",
        ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.Tg First.Target\n.It Fl -first\n.Tg Second.Target\n.It Fl -second\nSECOND_BODY\n.Tg Last.Target\n.It Fl -last\n.El\n",
    ] {
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        let document = query.document.as_ref().unwrap();
        assert!(mant_ir::validate_document(document).is_empty());
        let items = definitions(document);
        assert_eq!(items.len(), 3);
        assert_ne!(items[0].source, items[1].source);
        for (index, name) in ["--first", "--second", "--last"].iter().enumerate() {
            let entry = items[index].entry.as_ref().unwrap();
            assert_eq!(entry.names, [*name]);
            assert!(entry.alias_groups.is_empty());
            let result = mant_engine::explain_query(
                &query,
                &ExplanationQuery {
                    entry: (*name).into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            let direct = result
                .evidence
                .iter()
                .filter(|e| e.class == EvidenceClass::DirectEntry)
                .collect::<Vec<_>>();
            assert_eq!(direct.len(), 1);
            assert_eq!(direct[0].source, items[index].source);
            let excerpt = mant_engine::select_excerpt(&query, &[direct[0].outline.path()]).unwrap();
            let text = mant_engine::render_excerpt_text(&excerpt);
            assert_eq!(text.contains("SECOND_BODY"), index == 1, "{name}: {text}");
        }
        if source.contains("First.Target") {
            let json = serde_json::to_string(document).unwrap();
            for target in ["First.Target", "Second.Target", "Last.Target"] {
                assert!(json.contains(target), "lost {target}");
            }
        }
    }
}

#[test]
fn explicit_tq_groups_only_the_immediately_preceding_empty_head() {
    for tail in ["", "BODY\n"] {
        let source = format!(
            ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --orphan\n.TP\n.B --first\n.TQ\n.B --second\n.TQ\n.B --third\n{tail}"
        );
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        let items = definitions(query.document.as_ref().unwrap());
        assert_eq!(items.len(), 2);
        assert!(items[0].description.is_empty());
        assert_eq!(
            items[1].entry.as_ref().unwrap().names,
            ["--first", "--second", "--third"]
        );
        assert_eq!(items[1].description.is_empty(), tail.is_empty());
        assert!(items[1].entry.as_ref().unwrap().alias_groups.is_empty());
        assert_eq!(items[1].source.unwrap().line, 5);
    }
}

#[test]
fn an_explicit_tq_after_a_body_does_not_steal_that_body() {
    let source =
        b".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --first\nFIRST_BODY\n.TQ\n.B --second\nSECOND_BODY\n";
    let query = mant_engine::query_roff_bytes(source).unwrap();
    let items = definitions(query.document.as_ref().unwrap());
    assert_eq!(items.len(), 2);
    for (item, expected) in items.iter().zip(["FIRST_BODY", "SECOND_BODY"]) {
        let [Block::Paragraph { children, .. }] = item.description.as_slice() else {
            panic!("body")
        };
        assert!(serde_json::to_string(children).unwrap().contains(expected));
    }
}

#[test]
fn named_roff_bullets_are_lists_but_literal_operator_definitions_survive() {
    for marker in [r"\(bu", r"\[bu]", r"\ \(bu"] {
        let source = format!(
            ".TH PROBE 1\n.SH TOPIC\n.PD 0\n.TP 4\n{marker}\nFIRST\n.TP 4\n{marker}\nSECOND\n.RS 4\nNested continuation.\n.RE\n.TP 4\n.B *\nAn operator.\n.TP 4\n.B -\nStandard input.\n.TP 4\n.B +\nAnother operator.\n"
        );
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        let document = query.document.as_ref().unwrap();
        let items = definitions(document);
        assert_eq!(items.len(), 3, "{items:?}");
        let Block::List {
            kind: mant_ir::ListKind::Bullet,
            items,
            ..
        } = &document.sections[0].blocks[0]
        else {
            panic!("bullet list")
        };
        assert_eq!(items.len(), 2);
        assert!(
            items
                .iter()
                .all(|item| item.entry.is_none() && item.source.is_some())
        );
        let text = mant_engine::render_query_text(&query);
        assert!(
            text.contains("FIRST")
                && text.contains("SECOND")
                && text.contains("Nested continuation.")
        );
        assert!(mant_ir::validate_document(document).is_empty());
    }
}

#[test]
fn explicit_tp_and_ip_bullets_keep_equivalent_rendered_layout() {
    let body = "BODY\n.RS 4\nCONTINUATION\n.RE\n";
    let outputs = [".TP 4\n\\(bu", ".IP \\(bu 4"].map(|head| {
        let source = format!(".TH PROBE 1\n.SH TOPIC\n{head}\n{body}");
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        (
            mant_engine::render_query_text(&query),
            mant_engine::render_markdown(&query),
        )
    });
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
fn parameter_alternations_never_become_declared_command_names() {
    for (head, expected) in [
        (
            r"\fBset \fP[ {\fB+\fP|\fB\-\fP}\fIoptions\fP | {\fB+\fP|\fB\-\fP}\fBo\fP [ \fIoption_name\fP ] ]",
            "set",
        ),
        (
            r"\fBsnapshot\fP <source> <dest>|[<dest>/]<name>",
            "snapshot",
        ),
        (r"\fBresize\fP [<devid>:]max|<size>", "resize"),
        (r"\fBshow\fP <path>|<uuid>", "show"),
        (r"\fB[\fP", "["),
        (r"\fB.\fP", "."),
        (r"\fB:\fP", ":"),
    ] {
        let source = format!(".TH PROBE 1\n.SH COMMANDS\n.TP\n{head}\nBODY\n");
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        let document = query.document.as_ref().unwrap();
        assert!(mant_ir::validate_document(document).is_empty());
        let items = definitions(document);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].entry.as_ref().unwrap().names, [expected], "{head}");
        for fake in ["{+", "<uuid>", "[<dest>/]<name>"] {
            let result = mant_engine::explain_query(
                &query,
                &ExplanationQuery {
                    entry: fake.into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            assert_eq!(result.counts.direct_entry.total, 0, "{fake}: {head}");
        }
    }
}

#[test]
fn inferred_heads_require_whole_declarations_not_words_inside_prose() {
    for source in [
        ".TH PROBE 1\n.SH ENVIRONMENT\n.PP\nThe application chooses a mode, otherwise, it uses a default.\n.RS 4\nprogram --mode fast\n.RE\n",
        ".TH PROBE 1\n.SH ENVIRONMENT\n.PP\nThe line number is reported, as\n.RS 4\nprogram --line 1\n.RE\n",
        ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Pp\n.Fl T\nselects a terminal type for the next client.\n.Bd -literal -offset indent\nprogram -T EXAMPLE\n.Ed\n",
    ] {
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        assert!(
            definitions(query.document.as_ref().unwrap()).is_empty(),
            "{source}"
        );
        let text = mant_engine::render_query_text(&query);
        assert!(text.contains("program"));
        assert!(mant_ir::validate_document(query.document.as_ref().unwrap()).is_empty());
        for name in ["otherwise", "as", "-T"] {
            let result = mant_engine::explain_query(
                &query,
                &ExplanationQuery {
                    entry: name.into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            assert!(
                result
                    .evidence
                    .iter()
                    .all(|e| e.class != EvidenceClass::DirectEntry)
            );
        }
    }
    for (section, head, expected) in [
        ("ENVIRONMENT", "FIRST, SECOND", vec!["FIRST", "SECOND"]),
        ("ENVIRONMENT", "PATH=/one:/two", vec!["PATH"]),
        ("OPTIONS", ".B --type\n.I TYPE", vec!["--type"]),
        ("OPTIONS", ".B --name\nname", vec!["--name"]),
        ("OPTIONS", "--exec-path[=<path>]", vec!["--exec-path"]),
        ("OPTIONS", ".B -., --hidden", vec!["--hidden"]),
        (
            "OPTIONS",
            ".B --first, --second",
            vec!["--first", "--second"],
        ),
    ] {
        let source =
            format!(".TH PROBE 1\n.SH {section}\n.PP\n{head}\n.RS 4\nDESCRIPTION_BODY\n.RE\n");
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        let items = definitions(query.document.as_ref().unwrap());
        assert_eq!(items.len(), 1, "{source}");
        assert_eq!(items[0].entry.as_ref().unwrap().names, expected);
        assert!(mant_engine::render_query_text(&query).contains("DESCRIPTION_BODY"));
    }
}

#[test]
fn explicit_diagnostic_labels_remain_definitions_without_prose_fragment_names() {
    let source = b".TH PROBE 1\n.SH ENVIRONMENT\n.TP\nPermission denied, otherwise\nBODY\n";
    let query = mant_engine::query_roff_bytes(source).unwrap();
    let items = definitions(query.document.as_ref().unwrap());
    assert_eq!(items.len(), 1);
    let entry = items[0].entry.as_ref().unwrap();
    assert_eq!(entry.kind, mant_ir::EntryKind::Term);
    assert!(entry.names.is_empty());
    assert!(mant_engine::render_query_text(&query).contains("Permission denied, otherwise"));
}
