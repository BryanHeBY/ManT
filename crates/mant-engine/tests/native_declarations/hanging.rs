use super::{EvidenceClass, ExplanationOptions, ExplanationQuery, definitions};
use mant_ir::EntryKind;

#[test]
fn complete_hanging_heads_share_spacing_and_owner_rules_across_roles() {
    for (section, head, name, kind) in [
        (
            "COMMANDS",
            ".B list-units\n.I PATTERN",
            "list-units",
            EntryKind::Command,
        ),
        (
            "CONFIGURATION",
            ".B core.editor",
            "core.editor",
            EntryKind::ConfigurationKey,
        ),
        (
            "CONFIGURATION",
            ".B Environment=",
            "Environment",
            EntryKind::ConfigurationKey,
        ),
        (
            "VARIABLES",
            ".B my-variable",
            "my-variable",
            EntryKind::Variable,
        ),
        (
            "ENVIRONMENT",
            ".B CACHE_HOME <directory>",
            "CACHE_HOME",
            EntryKind::EnvironmentVariable,
        ),
        (
            "CONFIGURATION",
            ".B --builddir\n.I DIR",
            "--builddir",
            EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Option,
            },
        ),
    ] {
        for spacing in ["", ".sp .6\n", ".sp\n.sp 2\n"] {
            let source = format!(
                ".TH PROBE 1\n.SH {section}\n.na\n.PP\n{head}\n{spacing}.RS 4n\nOWNER_BODY\n.RE\n.ad\n.PP\nNormal explanatory text follows.\n.SH NEXT\nOUTSIDE_BODY\n"
            );
            let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
            let document = query.document.as_ref().unwrap();
            assert!(mant_ir::validate_document(document).is_empty());
            let items = definitions(document);
            assert_eq!(items.len(), 1, "{source}");
            let entry = items[0].entry.as_ref().unwrap();
            assert_eq!(entry.kind, kind, "{source}");
            assert_eq!(entry.names, [name], "{source}");
            let body = serde_json::to_string(&items[0].description).unwrap();
            assert!(body.contains("OWNER_BODY") && !body.contains("OUTSIDE_BODY"));
            let result = mant_engine::explain_query(
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
            assert_eq!(direct.len(), 1);
            assert_eq!(direct[0].source, items[0].source);
        }
    }
}

#[test]
fn hanging_heads_cannot_cross_prose_new_heads_outer_content_or_eof() {
    for tail in [
        "",
        ".PP\nOutside.\n",
        ".SH NEXT\nOutside.\n",
        ".PP\n.B --next\n.RS 4\nNEXT_BODY\n.RE\n",
    ] {
        let source = format!(".TH PROBE 1\n.SH OPTIONS\n.PP\n.B --orphan\n.sp\n.sp 2\n{tail}");
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        let items = definitions(query.document.as_ref().unwrap());
        assert!(items.iter().all(|item| {
            item.entry
                .as_ref()
                .is_none_or(|entry| !entry.names.iter().any(|name| name == "--orphan"))
        }));
    }
    for section in ["COMMANDS", "CONFIGURATION", "VARIABLES", "ENVIRONMENT"] {
        let source = format!(
            ".TH PROBE 1\n.SH {section}\n.PP\nThis ordinary paragraph explains an example, otherwise it continues.\n.sp\n.RS 4\nEXAMPLE_BODY\n.RE\n"
        );
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        assert!(definitions(query.document.as_ref().unwrap()).is_empty());
    }
}

#[test]
fn finite_short_long_pairs_bind_names_without_rescanning_argument_tokens() {
    for head in [
        "-a --ascii",
        "-a or --ascii",
        "-a  or\t--ascii",
        "-a, --ascii",
    ] {
        let source = format!(".TH PROBE 1\n.SH OPTIONS\n.TP\n.B {head}\nBODY\n");
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        let document = query.document.as_ref().unwrap();
        let items = definitions(document);
        let entry = items[0].entry.as_ref().unwrap();
        assert_eq!(entry.names, ["-a", "--ascii"], "{head}");
        assert!(entry.alias_groups.is_empty());
        assert!(mant_ir::validate_document(document).is_empty());
        let result = mant_engine::explain_query(
            &query,
            &ExplanationQuery {
                entry: "--ascii".into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(
            result
                .evidence
                .iter()
                .filter(|e| e.class == EvidenceClass::DirectEntry)
                .count(),
            1
        );
    }
    for head in [
        "-a --argument -literal-value",
        "-o path --later",
        "--mode {a|b}",
        "--opt=value/with/path",
    ] {
        let source = format!(".TH PROBE 1\n.SH OPTIONS\n.TP\n.B {head}\nBODY\n");
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        let items = definitions(query.document.as_ref().unwrap());
        let names = &items[0].entry.as_ref().unwrap().names;
        assert_eq!(names.len(), 1, "{head}: {names:?}");
        assert!(mant_ir::validate_document(query.document.as_ref().unwrap()).is_empty());
    }
    let source = b".TH PROBE 1\n.SH OPTIONS\n.PP\n.B -a --ascii\n.I FILE\ncontains an ordinary explanation of an example.\n.RS 4\nEXAMPLE\n.RE\n";
    let query = mant_engine::query_roff_bytes(source).unwrap();
    assert!(definitions(query.document.as_ref().unwrap()).is_empty());
}
