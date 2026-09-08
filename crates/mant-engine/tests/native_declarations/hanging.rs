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
#[test]
fn unstyled_dotted_keys_italic_settings_and_repeated_arguments_keep_owners() {
    let source = b".TH LOCAL 1\n.SH VARIABLES\n.PP\ncore.editor\n.RS 4\nThe chosen editor.\n.RE\n.PP\nuser.name, user.email\n.RS 4\nIdentity settings.\n.RE\n.SH PATHS\n.PP\n.I WorkingDirectory=\n.RS 4\nSet the working directory.\n.RE\n.SH OPTIONS\n.PP\n.B -O, --test-opts\n.I option\n...\n.RS 4\nTest the supplied options.\n.RE\n";
    let content = mant_engine::query_roff_bytes(source).unwrap();
    for (name, kind, body) in [
        (
            "core.editor",
            mant_ir::EntryKind::ConfigurationKey,
            "The chosen editor",
        ),
        (
            "user.email",
            mant_ir::EntryKind::ConfigurationKey,
            "Identity settings",
        ),
        (
            "WorkingDirectory",
            mant_ir::EntryKind::ConfigurationKey,
            "Set the working directory",
        ),
        (
            "--test-opts",
            mant_ir::EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Option,
            },
            "Test the supplied options",
        ),
    ] {
        let result = mant_engine::select_explanation(&content, name).unwrap();
        let direct: Vec<_> = result
            .evidence
            .iter()
            .filter(|e| e.class == mant_protocol::EvidenceClass::DirectEntry)
            .collect();
        assert_eq!(direct.len(), 1, "{name}: {result:?}");
        assert_eq!(direct[0].entry.as_ref().unwrap().kind, kind);
        assert!(mant_engine::render_explanation_text(&result).contains(body));
    }
    let negative = mant_engine::query_roff_bytes(b".TH NO 1\n.SH NOTES\n.PP\nfile.md\n.RS 4\nA file example, not a configuration declaration.\n.RE\n").unwrap();
    assert_eq!(
        mant_engine::select_explanation(&negative, "file.md")
            .unwrap()
            .counts
            .direct_entry
            .total,
        0
    );
}
#[test]
fn compact_parameter_grammar_and_opaque_environment_templates_remain_declarations() {
    let source = br".TH HEADS 1
.SH OPTIONS
.PP
\fB-L\fR\fI<start>\fR,\fI<end>\fR:\fI<file>\fR, \fB-L\fR:\fI<funcname>\fR:\fI<file>\fR
.RS 4
RANGE_BODY
.RE
.PP
\fI-<number>\fR, \fB-n\fR \fI<number>\fR, \fB--max-count\fR=\fI<number>\fR
.RS 4
COUNT_BODY
.RE
.PP
.B --color-moved-ws=<mode>,...
.RS 4
COLOR_BODY
.RE
.PP
\fB--map-groups, --map-users\fR \fIinner\fR:_outer_:\fIcount\fR
.RS 4
MAP_BODY
.RE
.PP
.B --map-users /proc/PID/ns/user
.RS 4
PATH_BODY
.RE
.PP
.B --sd-param name=value
.RS 4
ASSIGNMENT_BODY
.RE
.PP
\fB--sd-id\fR \fIname\fR[\fB@\fR\fIdigits\fR]
.RS 4
IDENTIFIER_BODY
.RE
.PP
\fB--trailer\fR \fI<token>\fR[(\fB=\fR|\fB:\fR)\fI<value>\fR]
.RS 4
TRAILER_BODY
.RE
.SH ENVIRONMENT
.PP
.B GIT_CONFIG_COUNT, GIT_CONFIG_KEY_<n>, GIT_CONFIG_VALUE_<n>
.RS 4
ENV_BODY
.RE
.PP
.B GIT_CONFIG_COUNT, words are not declarations
.RS 4
PROSE_BODY
.RE
";
    let content = mant_engine::query_roff_bytes(source).unwrap();
    for (query, total, body) in [
        ("-L", 1, "RANGE_BODY"),
        ("--max-count", 1, "COUNT_BODY"),
        ("--color-moved-ws", 1, "COLOR_BODY"),
        ("--map-users", 2, "MAP_BODY"),
        ("--sd-param", 1, "ASSIGNMENT_BODY"),
        ("GIT_CONFIG_COUNT", 1, "ENV_BODY"),
        ("--sd-id", 1, "IDENTIFIER_BODY"),
        ("--trailer", 1, "TRAILER_BODY"),
    ] {
        let result = mant_engine::select_explanation(&content, query).unwrap();
        assert_eq!(
            result.counts.direct_entry.total, total,
            "{query}: {result:?}"
        );
        assert!(mant_engine::render_explanation_text(&result).contains(body));
    }
    for query in [
        "GIT_CONFIG_KEY_",
        "GIT_CONFIG_KEY_1",
        "n",
        "words",
        "name",
        "value",
        "funcname",
    ] {
        assert_eq!(
            mant_engine::select_explanation(&content, query)
                .unwrap()
                .counts
                .direct_entry
                .total,
            0,
            "{query}"
        );
    }
    let env = mant_engine::select_explanation(&content, "GIT_CONFIG_COUNT").unwrap();
    let direct = env
        .evidence
        .iter()
        .find(|e| e.class == mant_protocol::EvidenceClass::DirectEntry)
        .unwrap();
    assert_eq!(direct.entry.as_ref().unwrap().names, ["GIT_CONFIG_COUNT"]);
    assert_eq!(
        direct.entry.as_ref().unwrap().kind,
        mant_ir::EntryKind::EnvironmentVariable
    );
    assert!(direct.entry.as_ref().unwrap().alias_groups.is_empty());
}
