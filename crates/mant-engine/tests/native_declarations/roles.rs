use super::{EvidenceClass, ExplanationOptions, ExplanationQuery, common, definitions};
use mant_ir::{EntryKind, ParameterKind};

#[test]
fn split_literal_command_heads_keep_the_whole_name_and_stop_before_arguments() {
    let source = b".Dd September 8, 2026\n.Dt TREE 1\n.Os\n.Sh COMMANDS\n.Bl -tag -width Ds\n.It Nm zfs Cm get Op Fl r Ns | Ns Fl d Ar depth\nGET_BODY\n.It Nm zfs Cm set Ar property Ns = Ns Ar value\nSET_BODY\n.It Sy { Ar list Ns Sy ;}\nCOMPOUND_BODY\n.El\n.Sh SESSIONS\n.Bl -tag -width Ds\n.It Ic new-session Op Fl Ad Ar name\nSESSION_BODY\n.It Ic label Ar value\nAMBIGUOUS_BODY\n.El\n";
    let content = mant_loader::load_roff_bytes(source).unwrap();
    for (name, kind, body) in [
        ("zfs get", EntryKind::Command, "GET_BODY"),
        ("zfs set", EntryKind::Command, "SET_BODY"),
        ("{", EntryKind::Command, "COMPOUND_BODY"),
        ("new-session", EntryKind::Command, "SESSION_BODY"),
        ("label", EntryKind::Term, "AMBIGUOUS_BODY"),
    ] {
        let result = mant_query::select_explanation(&content, name).unwrap();
        let direct: Vec<_> = result
            .evidence
            .iter()
            .filter(|e| e.class == EvidenceClass::DirectEntry)
            .collect();
        assert_eq!(direct.len(), 1, "{name}: {result:?}");
        assert_eq!(direct[0].entry.as_ref().unwrap().names, [name]);
        assert_eq!(direct[0].entry.as_ref().unwrap().kind, kind);
        assert!(mant_render::render_explanation_text(&result).contains(body));
        assert!(direct[0].entry.as_ref().unwrap().alias_groups.is_empty());
    }
    for name in ["zfs", "get", "depth", "property", "{+"] {
        assert_eq!(
            mant_query::select_explanation(&content, name)
                .unwrap()
                .counts
                .direct_entry
                .total,
            0,
            "{name}"
        );
    }
    let source = b".Dd September 8, 2026\n.Dt DOT 1\n.Os\n.Sh Builtins\n.Bl -tag -width Ds\n.It \\&. file\nRead commands from the file.\n.El\n";
    let content = mant_loader::load_roff_bytes(source).unwrap();
    let result = mant_query::select_explanation(&content, ".").unwrap();
    assert_eq!(result.counts.direct_entry.total, 1);
    assert_eq!(result.evidence[0].entry.as_ref().unwrap().names, ["."]);
    assert!(mant_render::render_explanation_text(&result).contains("Read commands from the file"));
}

#[test]
fn local_definitions_override_inherited_values_without_inventing_domains() {
    let source = b".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --outer\nOUTER_BODY\n.RS 4\n.TP\n.B --inner=fast\nINNER_BODY\n.TP\n.B true\nVALUE_BODY\n.TP\n.B -42\nNEGATIVE_BODY\n.TP\n.B PROCESS_HOME\nVARIABLE_BODY\n.TP\n.B color=[yes|no]\nKEY_BODY\n.TP\n.B .*-fallthrough.*\nREGEX_BODY\n.RE\n.TP\n.B --next\nNEXT_BODY\n";
    let query = mant_loader::load_roff_bytes(source).unwrap();
    let document = query.document.as_ref().unwrap();
    assert!(mant_ir::validate_document(document).is_empty());
    let items = definitions(document);
    assert_eq!(items.len(), 8);
    for (name, kind, body) in [
        (
            "--inner",
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Option,
            },
            "INNER_BODY",
        ),
        // CVS man_term.c::pre_TP/pre_RS prove separate heads and indentation,
        // not an independent Value role or a value domain.
        ("true", EntryKind::Term, "VALUE_BODY"),
        ("-42", EntryKind::Term, "NEGATIVE_BODY"),
        ("PROCESS_HOME", EntryKind::Term, "VARIABLE_BODY"),
        ("color", EntryKind::Term, "KEY_BODY"),
    ] {
        let item = items
            .iter()
            .find(|item| {
                item.entry
                    .as_ref()
                    .is_some_and(|entry| entry.names.iter().any(|n| n == name))
            })
            .unwrap_or_else(|| panic!("missing {name}"));
        let entry = item.entry.as_ref().unwrap();
        assert_eq!(entry.kind, kind, "{name}");
        assert!(entry.value_domain.is_none());
        assert!(entry.alias_groups.is_empty());
        assert!(common::block_slice_text(document.content(), &item.description).contains(body));
        let result = mant_query::explain_query(
            &query,
            &ExplanationQuery {
                entry: name.into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        let direct: Vec<_> = result
            .evidence
            .iter()
            .filter(|e| e.class == EvidenceClass::DirectEntry)
            .collect();
        assert_eq!(direct.len(), 1, "{name}");
        assert_eq!(direct[0].source, item.source);
    }
    assert!(
        items
            .iter()
            .filter_map(|item| item.entry.as_ref())
            .all(|entry| !entry.names.iter().any(|n| n == "-fallthrough"))
    );
}

#[test]
fn weak_hanging_heads_need_independent_declaration_evidence() {
    for (heading, head) in [
        ("Environment Commands", ".B show-environment"),
        ("Command Descriptions", ".B list-units"),
    ] {
        let source = format!(".TH PROBE 1\n.SH \"{heading}\"\n.PP\n{head}\n.RS 4\nBODY\n.RE\n");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        // CVS man_term.c::pre_PP/pre_RS gives layout, not a Command fact.
        assert!(
            definitions(query.document.as_ref().unwrap()).is_empty(),
            "{head}"
        );
        assert!(mant_render::render_query_text(&query).contains("BODY"));
    }
    for (heading, head, name) in [
        (
            "Environment",
            ".B WorkingDirectory=PATH",
            "WorkingDirectory",
        ),
        ("Environment", ".B HOME=/path", "HOME"),
    ] {
        let source = format!(".TH PROBE 1\n.SH \"{heading}\"\n.PP\n{head}\n.RS 4\nBODY\n.RE\n");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let items = definitions(query.document.as_ref().unwrap());
        assert_eq!(items.len(), 1, "{heading}/{head}");
        assert_eq!(
            items[0].entry.as_ref().unwrap().kind,
            EntryKind::EnvironmentVariable
        );
        assert_eq!(items[0].entry.as_ref().unwrap().names, [name]);
    }
    // The exact input ran pinned CVS -Tutf8 -Owidth=78 first:
    // man_term.c::pre_PP/pre_RS lays out the label and body, but an empty
    // assignment has no complete weak declaration evidence.
    let source = b".TH PROBE 1\n.SH Environment\n.PP\n.B Environment=\n.RS 4\nBODY\n.RE\n";
    let query = mant_loader::load_roff_bytes(source).unwrap();
    assert!(definitions(query.document.as_ref().unwrap()).is_empty());
    assert!(mant_render::render_query_text(&query).contains("BODY"));
    let source = b".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh TOPIC\n.Bl -tag -width Ds\n.It Cm -\nALIGNMENT_BODY\n.El\n";
    let query = mant_loader::load_roff_bytes(source).unwrap();
    let items = definitions(query.document.as_ref().unwrap());
    let entry = items[0].entry.as_ref().unwrap();
    assert_eq!(entry.kind, EntryKind::Term);
    assert_eq!(entry.names, ["-"]);
}

#[test]
fn literal_dash_requires_a_visible_authored_macro_instance() {
    // This exact input ran pinned CVS -Tutf8 -Owidth=78 first. In
    // mdoc_term.c::termp_it_pre a tag HEAD is distinct from a list bullet;
    // termp_bold_pre executes Cm, but zero-width Cm contributes no glyph.
    let source = b".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh TOPIC\n.Bl -tag -width Ds\n.It Cm \\& Ns -\nBODY\n.It Cm -\nDASH_BODY\n.It Cm \\& Ns --\nBODY2\n.It Cm --\nDOUBLE_BODY\n.El\n";
    let query = mant_loader::load_roff_bytes(source).unwrap();
    let document = query.document.as_ref().unwrap();
    assert!(mant_ir::validate_document(document).is_empty());
    let items = definitions(document);
    assert_eq!(items.len(), 4);
    assert!(items[0].entry.is_none());
    assert_eq!(items[1].entry.as_ref().unwrap().names, ["-"]);
    assert!(items[2].entry.is_none());
    assert_eq!(items[3].entry.as_ref().unwrap().names, ["--"]);
    let display = mant_render::render_query_text(&query);
    for body in ["BODY", "DASH_BODY", "BODY2", "DOUBLE_BODY"] {
        assert!(display.contains(body));
    }
}
