use super::*;

fn assert_group(
    fixed: &mant_ir::FixedBody,
    owner: &mant_ir::OwnerMark,
    kind: EntryKind,
    expected: &[&str],
    evidence: mant_ir::EntryNameEvidence,
) {
    let facts = owner.entry.as_ref().expect("proved non-option entry");
    assert_eq!(facts.kind, kind);
    assert_eq!(facts.names, expected);
    assert_eq!(facts.forms.as_slice(), std::slice::from_ref(&owner.head));
    assert!(facts.alias_groups.is_empty());
    assert_eq!(facts.name_bindings.len(), expected.len());
    for (index, binding) in facts.name_bindings.iter().enumerate() {
        assert_eq!(binding.name, index);
        assert_eq!(binding.evidence, evidence);
        assert!(!binding.occurrences.is_empty());
        for occurrence in &binding.occurrences {
            assert_eq!(
                fixed.selection_text(occurrence).as_deref(),
                Some(expected[index])
            );
        }
    }
}

fn assert_direct_explain(document: &mant_ir::Document, requested: &str, body: &str) {
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document.clone()),
        tldr: None,
    };
    let response = mant_query::explain_query(
        &resolved,
        &ExplanationQuery {
            entry: requested.to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(response.counts.direct_entry.total, 1, "{requested}");
    let evidence = response
        .evidence
        .iter()
        .find(|evidence| evidence.class == EvidenceClass::DirectEntry)
        .expect("direct definition");
    let entry = evidence.entry.as_ref().expect("entry DTO");
    let name_index = entry
        .names
        .iter()
        .position(|name| name == requested)
        .expect("requested name");
    let binding = entry
        .name_bindings
        .iter()
        .find(|binding| binding.name_index as usize == name_index)
        .expect("projected name binding");
    assert!(binding.occurrences.iter().any(|occurrence| {
        occurrence
            .fixed_forms
            .iter()
            .any(|range| range.resolve(&entry.fixed_forms).as_deref() == Some(requested))
    }));
    let mant_protocol::ExplanationContent::FixedOwner { reading_body, .. } =
        evidence.content.as_ref().expect("Fixed reading body")
    else {
        panic!("not Fixed owner")
    };
    assert!(
        reading_body
            .parts
            .iter()
            .any(|part| part.text.contains(body)),
        "{requested}: missing {body}"
    );
    response.validate_references().unwrap();
}

#[test]
fn weak_pp_rs_variable_and_configuration_facts_share_read_time_proof() {
    // This exact SSH_CONFIG page ran pinned CVS -Tutf8 -Owidth=78 first.
    // man_term.c::pre_PP/pre_RS only establish paragraph/indentation;
    // complete visible syntax and an exact glyph selection prove each name.
    let input = b".TH SSH_CONFIG 5\n.SH VARIABLES\n.PP\n.B foo=bar\n.RS 4\nFoo assignment.\n.RE\n.PP\n.B foo <S>\n.RS 4\nFoo placeholder.\n.RE\n.PP\n.B foo\n.RS 4\nBare variable prose.\n.RE\n.PP\n.B foo=\n.RS 4\nIncomplete variable prose.\n.RE\n.PP\n.B <K><S>\n.RS 4\nPlaceholder prose.\n.RE\n.SH DESCRIPTION\n.PP\n.B BatchMode=yes\n.RS 4\nRoot undotted configuration assignment.\n.RE\n.PP\n.B BatchMode\n.RS 4\nBare root configuration prose.\n.RE\n.PP\n.B core.editor=vim\n.RS 4\nRoot configuration assignment.\n.RE\n.PP\n.B core.editor\n.RS 4\nBare root configuration prose.\n.RE\n.SH CONFIGURATION\n.PP\n.B local.key\n.RS 4\nLocal configuration key.\n.RE\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man)
        .expect("annotated Fixed page");
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(fixed.owners.len(), 10);
    let accepted = fixed
        .owners
        .iter()
        .filter(|owner| owner.hanging_candidate && owner.entry.is_some())
        .collect::<Vec<_>>();
    assert_eq!(accepted.len(), 5);
    for excluded in ["foo", "foo=", "<K><S>", "BatchMode", "core.editor"] {
        assert!(fixed.owners.iter().any(|owner| {
            fixed.selection_text(&owner.head).as_deref() == Some(excluded) && owner.entry.is_none()
        }));
    }
    for (owner, kind, name) in [
        (accepted[0], EntryKind::Variable, "foo"),
        (accepted[1], EntryKind::Variable, "foo"),
        (accepted[2], EntryKind::ConfigurationKey, "BatchMode"),
        (accepted[3], EntryKind::ConfigurationKey, "core.editor"),
        (accepted[4], EntryKind::ConfigurationKey, "local.key"),
    ] {
        assert_group(
            fixed,
            owner,
            kind,
            &[name],
            mant_ir::EntryNameEvidence::Lexical,
        );
    }
    let decoded: mant_ir::Document =
        serde_json::from_str(&serde_json::to_string(&document).unwrap()).unwrap();
    assert!(validate_document(&decoded).is_empty());
}

#[test]
fn weak_environment_pairs_bind_complete_assignments_and_name_groups() {
    // This exact input ran pinned CVS -Tutf8 -Owidth=78 before assertions.
    // man_term.c::pre_PP/pre_RS only establish the continuation; the full
    // assignment or delimited names, not boldness, establish ManT selectors.
    let input = b".TH T 1\n.SH ENVIRONMENT\n.PP\n.B HOME=/tmp\n.RS 4\nHome description.\n.RE\n.PP\n.B TMPDIR, TEMP\n.RS 4\nTemporary directories.\n.RE\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    let owners = fixed
        .owners
        .iter()
        .filter(|owner| owner.hanging_candidate)
        .collect::<Vec<_>>();
    assert_eq!(owners.len(), 2);
    assert_group(
        fixed,
        owners[0],
        EntryKind::EnvironmentVariable,
        &["HOME"],
        mant_ir::EntryNameEvidence::Lexical,
    );
    assert_group(
        fixed,
        owners[1],
        EntryKind::EnvironmentVariable,
        &["TMPDIR", "TEMP"],
        mant_ir::EntryNameEvidence::Lexical,
    );
    for (name, body) in [
        ("HOME", "Home description."),
        ("TMPDIR", "Temporary directories."),
        ("TEMP", "Temporary directories."),
    ] {
        assert_direct_explain(&document, name, body);
    }
    let roundtrip: mant_ir::Document =
        serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
    assert!(validate_document(&roundtrip).is_empty());
}

#[test]
fn explicit_variable_and_configuration_heads_bind_only_the_key_range() {
    // This exact input ran pinned CVS -Tutf8 -Owidth=78 before assertions.
    // man_term.c::pre_TP executes each explicit HEAD; pre_B only supplies
    // initial font, not a ManT Variable or ConfigurationKey type.
    let input = b".TH T 1\n.SH VARIABLES\n.TP\n.B foo=bar\nVariable description.\n.SH CONFIGURATION\n.TP\n.B local.key\nSetting description.\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(fixed.owners.len(), 2);
    assert_group(
        fixed,
        &fixed.owners[0],
        EntryKind::Variable,
        &["foo"],
        mant_ir::EntryNameEvidence::Lexical,
    );
    assert_group(
        fixed,
        &fixed.owners[1],
        EntryKind::ConfigurationKey,
        &["local.key"],
        mant_ir::EntryNameEvidence::Lexical,
    );
    for (name, body) in [
        ("foo", "Variable description."),
        ("local.key", "Setting description."),
    ] {
        assert_direct_explain(&document, name, body);
    }
    let roundtrip: mant_ir::Document =
        serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
    assert!(validate_document(&roundtrip).is_empty());
}

#[test]
fn explicit_tp_and_ip_bind_complete_variable_tokens_without_term_fallback() {
    // Each exact TP/IP label below ran pinned CVS -Tutf8 -Owidth=78 first.
    // man_macro.c::blk_imp gives both macros an authored HEAD; the renderer's
    // man_term.c::pre_TP/pre_IP paths print that tag but do not classify it.
    for macro_kind in ["TP", "IP"] {
        for (label, expected) in [
            ("foo", Some("foo")),
            ("FOO[bar]", Some("FOO[bar]")),
            ("$FOO[_index]", Some("$FOO[_index]")),
            ("FOO[bar", None),
            ("FOO::", None),
            ("$FOO::", None),
        ] {
            let head = if macro_kind == "TP" {
                format!(".TP\n.B {label}\n")
            } else {
                format!(".IP \"\\fB{label}\\fR\" 4\n")
            };
            let input = format!(".TH T 1\n.SH VARIABLES\n{head}Description.\n");
            let document =
                project_annotated_manual("t.1", &bundle(input.as_bytes()), InputFormat::Man)
                    .unwrap();
            assert!(
                validate_document(&document).is_empty(),
                "{macro_kind}: {label}"
            );
            let DocumentBody::Fixed(fixed) = &document.body else {
                panic!("not Fixed")
            };
            let [owner] = fixed.owners.as_slice() else {
                panic!("expected one physical owner: {macro_kind}: {label}")
            };
            assert_eq!(fixed.selection_text(&owner.head).as_deref(), Some(label));
            if let Some(name) = expected {
                assert_group(
                    fixed,
                    owner,
                    EntryKind::Variable,
                    &[name],
                    mant_ir::EntryNameEvidence::Lexical,
                );
                assert_direct_explain(&document, name, "Description.");
            } else {
                assert!(owner.entry.is_none(), "{macro_kind}: {label}");
            }
            let roundtrip: mant_ir::Document =
                serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
            assert!(validate_document(&roundtrip).is_empty());
        }
    }
}

#[test]
fn plain_mdoc_tag_keeps_malformed_variable_heads_readable_without_term_names() {
    // Each exact It input ran pinned CVS -Tutf8 -Owidth=78 first.
    // mdoc_macro.c::blk_full creates an It HEAD inside Bl; mdoc_term.c::
    // termp_it_pre prints tag-list heads independently of a macro role.
    for label in ["FOO::", "FOO[bar", "$FOO::"] {
        let input = format!(
            ".Dd September 27, 2026\n.Dt T 1\n.Os\n.Sh VARIABLES\n.Bl -tag -width Ds\n.It {label}\nbody\n.El\n"
        );
        let document =
            project_annotated_manual("t.1", &bundle(input.as_bytes()), InputFormat::Mdoc).unwrap();
        assert!(validate_document(&document).is_empty(), "{label}");
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed")
        };
        let [owner] = fixed.owners.as_slice() else {
            panic!("expected one physical It owner: {label}")
        };
        assert_eq!(owner.role, mant_ir::OwnerRole::Definition);
        assert_eq!(owner.head_role, None);
        assert_eq!(fixed.selection_text(&owner.head).as_deref(), Some(label));
        assert!(
            fixed
                .selection_text(&owner.direct_body)
                .is_some_and(|text| text.contains("body")),
            "{label}: native body disappeared"
        );
        assert!(owner.entry.is_none(), "{label}");
        let roundtrip: mant_ir::Document =
            serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
        assert!(validate_document(&roundtrip).is_empty());
    }
}

#[test]
fn weak_pp_rs_indexed_variable_still_needs_independent_declaration_syntax() {
    // This exact input ran pinned CVS -Tutf8 -Owidth=78 first. The PP/RS
    // continuation prints normally; unlike TP/IP, pre_PP/pre_RS supply no
    // authored variable HEAD, so the bare indexed token stays prose.
    let input = b".TH T 1\n.SH VARIABLES\n.PP\n.B FOO[bar]\n.RS 4\nDescription.\n.RE\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    let [owner] = fixed.owners.as_slice() else {
        panic!("expected one physical candidate")
    };
    assert!(owner.hanging_candidate);
    assert_eq!(
        fixed.selection_text(&owner.head).as_deref(),
        Some("FOO[bar]")
    );
    assert!(owner.entry.is_none());
}

#[test]
fn weak_commands_need_a_complete_call_or_key_binding_not_a_bold_word() {
    // This exact input ran pinned CVS -Tutf8 -Owidth=78 before assertions.
    // man_term.c::pre_PP/pre_RS show identical layout for run FILE, Note and
    // bare run; the complete call/binding is ManT's independent local proof.
    let input = b".TH T 1\n.SH COMMANDS\n.PP\n.B run FILE\n.RS 4\nRun description.\n.RE\n.PP\n.B backward-char (C-b)\n.RS 4\nMove backward.\n.RE\n.PP\n.B Note\n.RS 4\nIntroduction.\n.RE\n.PP\n.B run\n.RS 4\nAnother introduction.\n.RE\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    for (label, name) in [
        ("run FILE", "run"),
        ("backward-char (C-b)", "backward-char"),
    ] {
        let owner = fixed
            .owners
            .iter()
            .find(|owner| fixed.selection_text(&owner.head).as_deref() == Some(label))
            .expect("complete command head");
        assert_group(
            fixed,
            owner,
            EntryKind::Command,
            &[name],
            mant_ir::EntryNameEvidence::Lexical,
        );
    }
    for (name, body) in [
        ("run", "Run description."),
        ("backward-char", "Move backward."),
    ] {
        assert_direct_explain(&document, name, body);
    }
    for label in ["Note", "run"] {
        assert!(fixed.owners.iter().any(|owner| {
            fixed.selection_text(&owner.head).as_deref() == Some(label) && owner.entry.is_none()
        }));
    }
    let roundtrip: mant_ir::Document =
        serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
    assert!(validate_document(&roundtrip).is_empty());
}

#[test]
fn weak_bold_paragraphs_without_complete_local_declarations_remain_prose() {
    // This exact five-label input ran pinned CVS -Tutf8 -Owidth=78 first.
    // man_term.c::pre_PP/pre_RS render each paragraph and its indented
    // continuation; neither macro creates a semantic entry role.
    let input = b".TH T 1\n.SH ENVIRONMENT\n.PP\n.B GIT_DIR\n.RS 4\nBare environment prose.\n.RE\n.PP\n.B HOME=\n.RS 4\nIncomplete assignment.\n.RE\n.SH VARIABLES\n.PP\n.B foo\n.RS 4\nBare variable prose.\n.RE\n.SH COMMANDS\n.PP\n.B Note\n.RS 4\nIntroduction.\n.RE\n.PP\n.B run\n.RS 4\nAnother introduction.\n.RE\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    for label in ["GIT_DIR", "HOME=", "foo", "Note", "run"] {
        assert!(fixed.owners.iter().any(|owner| {
            owner.hanging_candidate
                && fixed.selection_text(&owner.head).as_deref() == Some(label)
                && owner.entry.is_none()
        }));
    }
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document.clone()),
        tldr: None,
    };
    for label in ["GIT_DIR", "HOME=", "foo", "Note", "run"] {
        let response = mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: label.to_owned(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(response.counts.direct_entry.total, 0, "{label}");
    }
    let roundtrip: mant_ir::Document =
        serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
    assert!(validate_document(&roundtrip).is_empty());
}

#[test]
fn man_environment_ip_tp_use_one_full_form_and_three_independent_names() {
    // Exact E01 input ran pinned CVS -Tutf8 first. man_term.c::pre_IP prints
    // the first IP HEAD operand and pre_TP skips only layout-width operands;
    // pre_B/term_word execute styling before our full visible HEAD scan.
    let input = b".TH ENTRY-ENV 1\n.SH ENVIRONMENT\n.IP \\fBCPATH\\fR 4\nAdditional include directories.\n.TP\n.B TMPDIR, TEMP, TMP\nThese variables are checked in order.\n.TP\n.B lower_name\nA documented environment variable.\n.PP\nOtherwise the default directory is used.\n.TP\n\\(bu\nThis is a bullet, not a named definition.\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    let environment = fixed
        .owners
        .iter()
        .filter(|owner| {
            owner
                .entry
                .as_ref()
                .is_some_and(|facts| facts.kind == EntryKind::EnvironmentVariable)
        })
        .collect::<Vec<_>>();
    assert_eq!(environment.len(), 3);
    assert_group(
        fixed,
        environment[0],
        EntryKind::EnvironmentVariable,
        &["CPATH"],
        mant_ir::EntryNameEvidence::Lexical,
    );
    assert_group(
        fixed,
        environment[1],
        EntryKind::EnvironmentVariable,
        &["TMPDIR", "TEMP", "TMP"],
        mant_ir::EntryNameEvidence::Lexical,
    );
    assert_eq!(
        fixed.selection_text(&environment[1].head).as_deref(),
        Some("TMPDIR, TEMP, TMP")
    );
    assert_group(
        fixed,
        environment[2],
        EntryKind::EnvironmentVariable,
        &["lower_name"],
        mant_ir::EntryNameEvidence::Lexical,
    );
    assert!(!fixed.owners.iter().any(|owner| {
        owner.entry.as_ref().is_some_and(|facts| {
            facts
                .names
                .iter()
                .any(|name| name == "Otherwise" || name == "default")
        })
    }));
}

#[test]
fn mdoc_ev_va_dv_roles_survive_through_fixed_facts_and_json() {
    // Exact E02 input ran pinned CVS -Tutf8 first. mdoc_macro.c::in_line
    // creates distinct ELEM instances; mdoc_term.c dispatches Ev/Va/Dv
    // through different font actions. Typography is not the semantic role.
    let input = b".Dd September 26, 2026\n.Dt ENTRY-ROLES 1\n.Os\n.Sh NAME\n.Nm entry-roles\n.Nd declaration recognition probe\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Ev DEMO_HOME\nHome directory for the program.\n.It Va counter\nA variable, not an environment variable.\n.It Ic activity-action\nA named setting without an explicit command context.\n.It Dv MODE_FAST\nA symbolic constant.\n.El\n.Sh CONFIGURATION\n.Bl -tag -width Ds\n.It Cm BatchMode\nSet the batch processing behavior.\n.El\n.Sh COMMANDS\n.Bl -tag -width Ds\n.It Ic attach-session Ar target\nAttach to a session.\n.El\n.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It Ev TMPDIR , Ev TEMP , Ev TMP\nVariables checked in this order.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(fixed.owners.len(), 7);
    for (index, role, kind, names) in [
        (
            0,
            OwnerHeadRole::Environment,
            EntryKind::EnvironmentVariable,
            &["DEMO_HOME"][..],
        ),
        (
            1,
            OwnerHeadRole::Variable,
            EntryKind::Variable,
            &["counter"][..],
        ),
        (
            2,
            OwnerHeadRole::Literal,
            EntryKind::Term,
            &["activity-action"][..],
        ),
        (
            3,
            OwnerHeadRole::DefinedVariable,
            EntryKind::Term,
            &["MODE_FAST"][..],
        ),
        (
            4,
            OwnerHeadRole::Literal,
            EntryKind::ConfigurationKey,
            &["BatchMode"][..],
        ),
        (
            5,
            OwnerHeadRole::Literal,
            EntryKind::Command,
            &["attach-session"][..],
        ),
        (
            6,
            OwnerHeadRole::Environment,
            EntryKind::EnvironmentVariable,
            &["TMPDIR", "TEMP", "TMP"][..],
        ),
    ] {
        let owner = &fixed.owners[index];
        assert_eq!(owner.head_role, Some(role));
        assert_group(
            fixed,
            owner,
            kind,
            names,
            mant_ir::EntryNameEvidence::NativeMarkup,
        );
        assert!(owner.head_components.iter().all(|component| {
            (component.source.is_some() || component.source_key.is_some())
                && (component.role == role
                    || index == 5 && component.role == OwnerHeadRole::Argument)
        }));
        if index == 5 {
            assert_eq!(owner.head_components.len(), 2);
            assert_eq!(owner.head_components[0].role, OwnerHeadRole::Literal);
            assert_eq!(owner.head_components[1].role, OwnerHeadRole::Argument);
            assert_eq!(
                fixed
                    .selection_text(&owner.head_components[1].selection)
                    .as_deref(),
                Some("target")
            );
        }
    }
    assert_eq!(fixed.owners[6].head_components.len(), 3);
    let json = serde_json::to_value(&document).unwrap();
    let decoded: mant_ir::Document = serde_json::from_value(json).unwrap();
    assert!(validate_document(&decoded).is_empty());
    let DocumentBody::Fixed(decoded_fixed) = &decoded.body else {
        panic!("not Fixed after round trip")
    };
    assert_eq!(
        decoded_fixed.owners[6].entry.as_ref().unwrap().names,
        ["TMPDIR", "TEMP", "TMP"]
    );
}

#[test]
fn explicit_literal_dash_is_a_term_not_a_generated_list_marker() {
    // This exact Cm - / Cm -- / Cm * / Fl - / bullet input ran pinned CVS
    // -Tutf8 -Owidth=78 first. mdoc_term.c::termp_it_pre keeps the tag HEADs
    // distinct from the bullet list, while termp_bold_pre executes Cm/Ic.
    let input = b".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh TOPIC\n.Bl -tag -width Ds\n.It Cm -\nDASH_BODY\n.It Cm --\nDOUBLE_BODY\n.It Cm *\nSTAR_BODY\n.It Fl -\nFLAG_BODY\n.El\n.Bl -bullet\n.It\nBULLET_BODY\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    for name in ["-", "--"] {
        let owner = fixed
            .owners
            .iter()
            .find(|owner| {
                owner.head_role == Some(OwnerHeadRole::Literal)
                    && fixed.selection_text(&owner.head).as_deref() == Some(name)
            })
            .expect("authored Cm tag owner");
        assert_group(
            fixed,
            owner,
            EntryKind::Term,
            &[name],
            mant_ir::EntryNameEvidence::NativeMarkup,
        );
    }
    assert!(fixed.owners.iter().any(|owner| {
        owner.head_role == Some(OwnerHeadRole::Literal)
            && fixed.selection_text(&owner.head).as_deref() == Some("*")
    }));
    assert!(
        fixed
            .owners
            .iter()
            .all(|owner| { owner.role == mant_ir::OwnerRole::Definition || owner.entry.is_none() })
    );
    let roundtrip: mant_ir::Document =
        serde_json::from_str(&serde_json::to_string(&document).unwrap()).unwrap();
    assert!(validate_document(&roundtrip).is_empty());
}

#[test]
fn empty_literal_macro_does_not_own_a_following_dash() {
    // This exact input ran pinned CVS -Tutf8 -Owidth=78 first. Cm uses
    // mdoc_term.c::termp_bold_pre, but its zero-width operand emits no glyph;
    // mdoc_term.c::termp_it_pre still keeps the visible tag HEAD and BODY.
    let input = b".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh TOPIC\n.Bl -tag -width Ds\n.It Cm \\& Ns -\nBODY\n.It Cm -\nDASH_BODY\n.It Cm \\& Ns --\nBODY2\n.It Cm --\nDOUBLE_BODY\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    let owners = fixed
        .owners
        .iter()
        .filter(|owner| owner.role == mant_ir::OwnerRole::Definition)
        .collect::<Vec<_>>();
    assert_eq!(owners.len(), 4);
    assert!(owners[0].entry.is_none());
    assert_eq!(owners[1].entry.as_ref().unwrap().names, ["-"]);
    assert!(owners[2].entry.is_none());
    assert_eq!(owners[3].entry.as_ref().unwrap().names, ["--"]);
}

#[test]
fn literal_roles_use_complete_head_and_local_or_root_configuration_context() {
    // This exact SSH_CONFIG input ran pinned CVS -Tutf8 -Owidth=78 before the
    // assertion. mdoc_macro.c::in_line keeps separate Cm instances in the It
    // HEAD; mdoc_term.c::termp_it_pre prints them without classifying ManT
    // entries. The root/configuration/prose distinction is checked IR policy.
    let input = b".Dd September 26, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Cm BatchMode\nSet mode.\n.It Cm Alpha , Cm Beta , Cm Alpha\nRepeated key group.\n.It Cm color=[yes|no]\nOptional value.\n.El\n.Sh SEE ALSO\n.Ss TOPIC\n.Bl -tag -width Ds\n.It Cm SeeKey\nA reference term.\n.El\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Cm LocalOnly\nAn option-scope term.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert!(fixed.root_configuration_hint);
    assert_eq!(fixed.owners.len(), 5);
    assert_group(
        fixed,
        &fixed.owners[0],
        EntryKind::ConfigurationKey,
        &["BatchMode"],
        mant_ir::EntryNameEvidence::NativeMarkup,
    );
    assert_group(
        fixed,
        &fixed.owners[1],
        EntryKind::ConfigurationKey,
        &["Alpha", "Beta"],
        mant_ir::EntryNameEvidence::NativeMarkup,
    );
    assert_eq!(
        fixed.owners[1].entry.as_ref().unwrap().name_bindings[0]
            .occurrences
            .len(),
        2
    );
    assert_group(
        fixed,
        &fixed.owners[2],
        EntryKind::ConfigurationKey,
        &["color"],
        mant_ir::EntryNameEvidence::NativeMarkup,
    );
    assert_group(
        fixed,
        &fixed.owners[3],
        EntryKind::Term,
        &["SeeKey"],
        mant_ir::EntryNameEvidence::NativeMarkup,
    );
    assert_group(
        fixed,
        &fixed.owners[4],
        EntryKind::Term,
        &["LocalOnly"],
        mant_ir::EntryNameEvidence::NativeMarkup,
    );

    // A metadata edit cannot make an obsolete weak root hint publish a
    // configuration fact. Both document indexes withhold it immediately; a
    // detached round trip retracts only dependent facts, not native display.
    let mut changed = document.clone();
    changed.meta.title = Some("TMUX".to_owned());
    changed.diagnostics.push(mant_ir::Diagnostic {
        level: mant_ir::DiagnosticLevel::Style,
        impact: mant_ir::DiagnosticImpact::None,
        code: Some("ir.invalid-root-configuration-hint".to_owned()),
        message: "unrelated spelling of the same code".to_owned(),
        source: None,
        source_key: None,
        coverage_scope: None,
    });
    assert!(!mant_ir::semantics_complete(
        &changed.projection_diagnostics()
    ));
    assert!(validate_document(&changed).iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("ir.invalid-root-configuration-hint")
    }));
    let DocumentBody::Fixed(changed_fixed) = &changed.body else {
        panic!("not Fixed")
    };
    let config_id = changed_fixed.owners[0].id.as_str();
    assert!(!DocumentIndex::build(&changed).contains(config_id));
    let semantic = mant_ir::SemanticIndex::build(&changed);
    assert!(
        !semantic
            .section(changed_fixed.headings[0].id.as_str())
            .iter()
            .any(|entry| { entry.names.iter().any(|name| name == "BatchMode") })
    );
    let decoded: mant_ir::Document =
        serde_json::from_value(serde_json::to_value(&changed).unwrap()).unwrap();
    let DocumentBody::Fixed(decoded_fixed) = &decoded.body else {
        panic!("not Fixed")
    };
    assert_eq!(decoded_fixed.surface, fixed.surface);
    assert!(!decoded_fixed.root_configuration_hint);
    assert!(decoded_fixed.owners[0].entry.is_none());
    assert_eq!(
        decoded_fixed.owners[3].entry.as_ref().unwrap().kind,
        EntryKind::Term
    );
    assert!(decoded.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("ir.invalid-root-configuration-hint")
    }));
}

#[test]
fn topical_command_signature_is_local_to_its_head() {
    // This exact input ran pinned CVS -Tutf8 -Owidth=78 before the assertion.
    // mdoc_macro.c::blk_full keeps Xo/Xc within one It HEAD;
    // mdoc_term.c::termp_it_pre and termp_fl_pre render the call signature.
    // Its semantic type is ManT policy, not an upstream AST flag.
    let input = b".Dd September 27, 2026\n.Dt T 1\n.Os\n.Sh CLIENTS AND SESSIONS\n.Bl -tag -width Ds\n.It Xo Ic new\\-session\n.Op Fl d\n.Op Ar target\n.Xc\nCreates a session.\n.El\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Xo Ic setting\n.Op Fl d\n.Xc\nA prose subject, not a command declaration.\n.El\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Ic activity-action Op Fl d Op Ar action\nSets the option.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(fixed.owners.len(), 3);
    assert_eq!(
        fixed.owners[0].entry.as_ref().unwrap().names,
        ["new-session"]
    );
    assert_eq!(
        fixed.owners[0].entry.as_ref().unwrap().kind,
        EntryKind::Command
    );
    assert!(fixed.owners[0].head_components.iter().any(|component| {
        component.role == OwnerHeadRole::Option
            && fixed.selection_text(&component.selection).as_deref() == Some("-d")
    }));
    assert_eq!(
        fixed.owners[1].entry.as_ref().unwrap().kind,
        EntryKind::Command
    );
    assert_eq!(
        fixed.owners[2].entry.as_ref().unwrap().kind,
        EntryKind::Term
    );
}

#[test]
fn lexical_fallback_keeps_pattern_and_marker_heads_unnamed() {
    // This exact input ran pinned CVS -Tutf8 -Owidth=78 first.
    // man_macro.c::blk_imp retains each TP/B head, and man_term.c::pre_TP
    // renders all three labels. Displayed text alone does not make a regex
    // family or a marker invocation an exact semantic name.
    let input = b".TH T 1\n.SH TERMS\n.TP\n.B ^find-new.*\nRegex family.\n.TP\n.B -- FILE\nMarker invocation.\n.TP\n.B working tree\nA concrete term.\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(fixed.owners.len(), 3);
    for (owner, form) in fixed.owners[..2].iter().zip(["^find-new.*", "-- FILE"]) {
        assert_eq!(fixed.selection_text(&owner.head).as_deref(), Some(form));
        let facts = owner.entry.as_ref().expect("readable unnamed Term");
        assert_eq!(facts.kind, EntryKind::Term);
        assert!(facts.names.is_empty());
        assert!(facts.name_bindings.is_empty());
    }
    assert_group(
        fixed,
        &fixed.owners[2],
        EntryKind::Term,
        &["working tree"],
        mant_ir::EntryNameEvidence::Lexical,
    );

    // A detached document cannot reintroduce the rejected regex selector
    // through the old Lexical witness fallback during read-time validation.
    let mut forged = document.clone();
    let DocumentBody::Fixed(fixed) = &mut forged.body else {
        unreachable!()
    };
    let owner = &mut fixed.owners[0];
    let facts = owner.entry.as_mut().unwrap();
    facts.names.push("^find-new.*".to_owned());
    facts.name_bindings.push(mant_ir::EntryNameBinding {
        name: 0,
        occurrences: vec![owner.head.clone()],
        evidence: mant_ir::EntryNameEvidence::Lexical,
    });
    assert!(
        validate_document(&forged)
            .iter()
            .any(|diagnostic| { diagnostic.code.as_deref() == Some("ir.invalid-fixed-body") })
    );
}

#[test]
fn literal_key_arguments_require_authored_ar_not_matching_font_or_prefix() {
    // This exact input ran pinned CVS -Tutf8 -Owidth=78 before the assertion.
    // mdoc_macro.c::in_line preserves Ar and Em as separate ELEM instances;
    // mdoc_term.c::termp_under_pre makes them look alike on screen. The C
    // collector therefore carries Ar's authored role into these checked
    // component selections instead of inferring it from underline style.
    let input = b".Dd September 26, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Cm AddressFamily Ar address_family\nThe address family.\n.It Cm ProxyCommand Ar command Ar argument\nThe proxy command.\n.It Cm Key No prose\nA prose tail.\n.It Cm Other Em prose\nAn emphasized tail.\n.It Cm Foo/bar\nSlash spelling.\n.It Cm Baz:qux\nColon spelling.\n.It Cm Empty=\nEmpty assignment.\n.It Cm color=[yes|no\nUnclosed optional assignment.\n.It Cm Full=bar\nComplete assignment.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(fixed.owners.len(), 9);
    assert_group(
        fixed,
        &fixed.owners[0],
        EntryKind::ConfigurationKey,
        &["AddressFamily"],
        mant_ir::EntryNameEvidence::NativeMarkup,
    );
    assert_eq!(
        fixed.owners[0].head_components[1].role,
        OwnerHeadRole::Argument
    );
    assert_group(
        fixed,
        &fixed.owners[1],
        EntryKind::ConfigurationKey,
        &["ProxyCommand"],
        mant_ir::EntryNameEvidence::NativeMarkup,
    );
    assert_eq!(
        fixed.owners[1]
            .head_components
            .iter()
            .map(|part| part.role)
            .collect::<Vec<_>>(),
        [
            OwnerHeadRole::Literal,
            OwnerHeadRole::Argument,
            OwnerHeadRole::Argument
        ]
    );
    for owner in &fixed.owners[2..8] {
        assert_ne!(
            owner.entry.as_ref().map(|entry| entry.kind),
            Some(EntryKind::ConfigurationKey)
        );
    }
    assert_group(
        fixed,
        &fixed.owners[8],
        EntryKind::ConfigurationKey,
        &["Full"],
        mant_ir::EntryNameEvidence::NativeMarkup,
    );
    let decoded: mant_ir::Document =
        serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
    assert!(validate_document(&decoded).is_empty());
}

#[test]
fn root_configuration_hint_stops_at_a_nested_option_ancestor() {
    // Exact input ran pinned CVS -Tutf8 -Owidth=78 first. mdoc_term.c::
    // termp_it_pre renders all three nested items; the owner ancestry, not
    // visual indentation or the last sibling, prevents a weak root key hint
    // from reappearing below Fl mode through the intermediate Dv group.
    let input = b".Dd September 26, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Fl mode\nMode description.\n.Bl -tag -width Ds\n.It Dv GROUP\nGroup description.\n.Bl -tag -width Ds\n.It Cm fast\nFast value.\n.El\n.El\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert!(fixed.root_configuration_hint);
    assert_eq!(fixed.owners.len(), 3);
    assert_eq!(
        fixed.owners[2].entry.as_ref().unwrap().kind,
        EntryKind::Term
    );
    assert_eq!(fixed.owners[2].entry.as_ref().unwrap().names, ["fast"]);
}

#[test]
fn directly_joined_literal_components_are_one_visible_term_not_two_keys() {
    // Exact input ran pinned CVS -Tutf8 -Owidth=78 first. mdoc_term.c prints
    // `Cm Alpha Ns Cm Beta` as AlphaBeta; Ns removes the declaration separator
    // without removing either native component instance. Two roles without a
    // surviving delimiter cannot become two independent configuration names.
    let input = b".Dd September 26, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Cm Alpha Ns Cm Beta\nJoined names.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(
        fixed.selection_text(&fixed.owners[0].head).as_deref(),
        Some("AlphaBeta")
    );
    let entry = fixed.owners[0].entry.as_ref().unwrap();
    assert_ne!(entry.kind, EntryKind::ConfigurationKey);
    assert!(
        !entry
            .names
            .iter()
            .any(|name| name == "Alpha" || name == "Beta")
    );
}

#[test]
fn unknown_section_inherits_root_config_but_prose_barrier_does_not() {
    // Exact input ran pinned CVS -Tutf8 -Owidth=78 first. mdoc_term.c prints
    // both native Cm heads; the finite ManT family rule inherits SSH_CONFIG
    // through SETTINGS but stops at SEE ALSO even below its unknown Ss TOPIC.
    let input = b".Dd September 26, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh SETTINGS\n.Bl -tag -width Ds\n.It Cm BatchMode\nSet mode.\n.El\n.Sh SEE ALSO\n.Ss TOPIC\n.Bl -tag -width Ds\n.It Cm LinkTopic\nA reference.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(fixed.owners.len(), 2);
    assert_group(
        fixed,
        &fixed.owners[0],
        EntryKind::ConfigurationKey,
        &["BatchMode"],
        mant_ir::EntryNameEvidence::NativeMarkup,
    );
    assert_group(
        fixed,
        &fixed.owners[1],
        EntryKind::Term,
        &["LinkTopic"],
        mant_ir::EntryNameEvidence::NativeMarkup,
    );
}

#[test]
fn root_configuration_description_cannot_reopen_an_examples_ancestor() {
    // Both exact inputs ran pinned CVS -Tutf8 -Owidth=78 first.
    // mdoc_macro.c::blk_full nests Ss below Sh and creates the It HEAD;
    // mdoc_term.c::termp_sh_pre/termp_ss_pre/termp_it_pre only render them.
    // A nested DESCRIPTION title cannot cancel its EXAMPLES parent in ManT.
    for (input, kind, body) in [
        (
            b".Dd September 27, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Cm BatchMode\nPositive body.\n.El\n"
                .as_slice(),
            EntryKind::ConfigurationKey,
            "Positive body.",
        ),
        (
            b".Dd September 27, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh EXAMPLES\n.Ss DESCRIPTION\n.Bl -tag -width Ds\n.It Cm BatchMode\nExample body.\n.El\n"
                .as_slice(),
            EntryKind::Term,
            "Example body.",
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc)
            .expect("annotated Fixed page");
        assert!(validate_document(&document).is_empty());
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed")
        };
        assert!(fixed.root_configuration_hint);
        let [owner] = fixed.owners.as_slice() else {
            panic!("expected one It owner")
        };
        assert_group(
            fixed,
            owner,
            kind,
            &["BatchMode"],
            mant_ir::EntryNameEvidence::NativeMarkup,
        );
        assert_direct_explain(&document, "BatchMode", body);
        let roundtrip: mant_ir::Document =
            serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
        assert!(validate_document(&roundtrip).is_empty());
    }
}

#[test]
fn literal_group_requires_complete_visible_separator_boundaries() {
    // Exact input ran pinned CVS -Tutf8 -Owidth=78 first. mdoc_term.c prints
    // both comma forms; native Cm roles prove independent names only when a
    // complete separator occurs *between* two instances, not at the end.
    let input = b".Dd September 26, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Cm Alpha,\nTrailing punctuation.\n.It Cm Alpha , Cm Beta\nComplete group.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_ne!(
        fixed.owners[0].entry.as_ref().map(|entry| entry.kind),
        Some(EntryKind::ConfigurationKey)
    );
    assert_group(
        fixed,
        &fixed.owners[1],
        EntryKind::ConfigurationKey,
        &["Alpha", "Beta"],
        mant_ir::EntryNameEvidence::NativeMarkup,
    );
}

#[test]
fn literal_group_keeps_interleaved_ar_arguments_out_of_names() {
    // Exact input ran pinned CVS -Tutf8 -Owidth=78 first. Each Cm and Ar is
    // a separate mdoc_macro.c::in_line instance; mdoc_term.c prints one
    // complete HEAD "Alpha a, Beta b". Only the two Cm ranges name keys.
    let input = b".Dd September 26, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Cm Alpha Ar a , Cm Beta Ar b\nTwo key declarations.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(fixed.owners.len(), 1);
    assert_eq!(
        fixed.owners[0]
            .head_components
            .iter()
            .map(|part| part.role)
            .collect::<Vec<_>>(),
        [
            OwnerHeadRole::Literal,
            OwnerHeadRole::Argument,
            OwnerHeadRole::Literal,
            OwnerHeadRole::Argument
        ]
    );
    assert_group(
        fixed,
        &fixed.owners[0],
        EntryKind::ConfigurationKey,
        &["Alpha", "Beta"],
        mant_ir::EntryNameEvidence::NativeMarkup,
    );
}

#[test]
fn literal_components_inside_one_argument_scope_do_not_become_names() {
    // This exact input ran pinned CVS -Tutf8 -Owidth=78 first. Its
    // mdoc_macro.c::blk_full/in_line path retains each Cm/Ar instance, while
    // mdoc_term.c::termp_it_pre and termp_bold_pre execute one visible HEAD.
    // A Cm instance inside an unclosed Ar bracket is still parameter text;
    // the external comma after the closing Ar permits the later Cm next.
    let input = b".Dd September 27, 2026\n.Dt T 1\n.Os\n.Sh COMMANDS\n.Bl -tag -width Ds\n.It Cm run Ar \"[first\" Cm fake Ar \"last]\" , Cm next\nBODY.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_group(
        fixed,
        &fixed.owners[0],
        EntryKind::Command,
        &["run", "next"],
        mant_ir::EntryNameEvidence::NativeMarkup,
    );
    assert_direct_explain(&document, "run", "BODY.");
    assert_direct_explain(&document, "next", "BODY.");
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document.clone()),
        tldr: None,
    };
    let fake = mant_query::explain_query(
        &resolved,
        &ExplanationQuery {
            entry: "fake".to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(fake.counts.direct_entry.total, 0);
    let decoded: mant_ir::Document =
        serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
    assert!(validate_document(&decoded).is_empty());
}

#[test]
fn literal_parameter_scope_survives_component_and_font_boundaries() {
    // This exact five-item input ran pinned CVS -Tutf8 -Owidth=78 first.
    // mdoc_macro.c::in_line supplies separate Ar/Cm instances; mdoc_term.c::
    // termp_under_pre/termp_bold_pre render their characters in one It HEAD.
    // Only punctuation outside those executed instances and closed scopes
    // can separate declarations.
    let input = b".Dd September 27, 2026\n.Dt T 1\n.Os\n.Sh COMMANDS\n.Bl -tag -width Ds\n.It Cm run Ar \"\\(dqfirst\" Cm fake Ar \"last\\(dq\" , Cm next\nQuoted body.\n.It Cm run Ar \"\\(lqfirst\" Cm fake Ar \"last\\(rq\" , Cm next\nTypographic body.\n.It Cm run Ar \"(first\" Cm fake Ar \"last)\" , Cm next\nParenthesized body.\n.It Cm run Ar first, Cm fake Ar last, Cm next\nComma-argument body.\n.It Cm run Ar first , Cm next\nExternal-comma body.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(fixed.owners.len(), 5);
    for (index, names) in [
        ["run", "next"].as_slice(),
        &["run", "next"],
        &["run", "next"],
        &["run"],
        &["run", "next"],
    ]
    .into_iter()
    .enumerate()
    {
        assert_group(
            fixed,
            &fixed.owners[index],
            EntryKind::Command,
            names,
            mant_ir::EntryNameEvidence::NativeMarkup,
        );
    }
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document),
        tldr: None,
    };
    let fake = mant_query::explain_query(
        &resolved,
        &ExplanationQuery {
            entry: "fake".to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(fake.counts.direct_entry.total, 0);
}

#[test]
fn nested_literal_names_keep_bindings_but_not_parent_key_or_command_type() {
    // Both exact inputs ran pinned CVS -Tutf8 -Owidth=78 first. The nested
    // mdoc_macro.c::blk_full It HEADs are distinct owners; termp_it_pre
    // renders them without giving the child a new top-level section context.
    // Native Cm name bindings survive a weak category downgrade to Term.
    for (input, parent_kind, parent_name, child_names, child_body) in [
        (
            b".Dd September 27, 2026\n.Dt SSH_CONFIG 5\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Cm ChannelTimeout\nAvailable channel types:\n.Bl -tag -width Ds\n.It Cm session\nInteractive channel.\n.It Cm direct-tcpip , Cm direct-streamlocal@openssh.com\nForwarded channels.\n.El\n.El\n"
                .as_slice(),
            EntryKind::ConfigurationKey,
            "ChannelTimeout",
            vec!["session", "direct-tcpip", "direct-streamlocal@openssh.com"],
            "Forwarded channels.",
        ),
        (
            b".Dd September 27, 2026\n.Dt T 1\n.Os\n.Sh COMMANDS\n.Bl -tag -width Ds\n.It Cm run\nOuter.\n.Bl -tag -width Ds\n.It Cm sub\nInner.\n.El\n.El\n"
                .as_slice(),
            EntryKind::Command,
            "run",
            vec!["sub"],
            "Inner.",
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc)
            .expect("native Fixed document");
        assert!(validate_document(&document).is_empty());
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed")
        };
        assert_eq!(fixed.owners[0].entry.as_ref().unwrap().kind, parent_kind);
        assert_eq!(fixed.owners[0].entry.as_ref().unwrap().names, [parent_name]);
        assert_eq!(fixed.owners[1].entry.as_ref().unwrap().kind, EntryKind::Term);
        for name in child_names {
            let owner = fixed
                .owners
                .iter()
                .find(|owner| {
                    owner
                        .entry
                        .as_ref()
                        .is_some_and(|entry| entry.names.iter().any(|candidate| candidate == name))
                })
                .expect("child name remains bound");
            assert_eq!(owner.entry.as_ref().unwrap().kind, EntryKind::Term);
            assert_direct_explain(
                &document,
                name,
                if name == "session" {
                    "Interactive channel."
                } else {
                    child_body
                },
            );
        }
        let decoded: mant_ir::Document =
            serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
        assert!(validate_document(&decoded).is_empty());
    }
}

#[test]
fn expanded_va_dv_keep_source_key_only_role_and_display_binding() {
    // Exact macro-expanded input ran pinned CVS -Tutf8 before assertion.
    // mdoc_macro.c::in_line executes Va/Dv even when the expansion does not
    // retain an authored per-token span; source identity must not be forged.
    let input = b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\n.de Vv\n.It Va counter\n..\n.de Kk\n.It Dv MODE_FAST\n..\n.Bl -tag -width Ds\n.Vv\nVariable body.\n.Kk\nConstant body.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    for (owner, kind, name, role) in [
        (
            &fixed.owners[0],
            EntryKind::Variable,
            "counter",
            OwnerHeadRole::Variable,
        ),
        (
            &fixed.owners[1],
            EntryKind::Term,
            "MODE_FAST",
            OwnerHeadRole::DefinedVariable,
        ),
    ] {
        assert_eq!(owner.head_role, Some(role));
        assert_group(
            fixed,
            owner,
            kind,
            &[name],
            mant_ir::EntryNameEvidence::NativeMarkup,
        );
        assert!(
            owner
                .head_components
                .iter()
                .all(|component| { component.source.is_none() && component.source_key.is_some() })
        );
    }
    let decoded: mant_ir::Document =
        serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
    assert!(validate_document(&decoded).is_empty());
}

#[test]
fn native_ev_repetition_keeps_occurrences_and_binding_order_is_independent() {
    // Exact input ran pinned CVS -Tutf8 first. Distinct Ev instances print
    // TMPDIR twice in one .It HEAD; that is occurrence repetition, not alias.
    let input = b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It Ev TMPDIR , Ev TEMP , Ev TMP , Ev TMPDIR\nRepeated environment name.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    let owner = &fixed.owners[0];
    let facts = owner.entry.as_ref().unwrap();
    assert_eq!(facts.names, ["TMPDIR", "TEMP", "TMP"]);
    assert_eq!(facts.forms.as_slice(), std::slice::from_ref(&owner.head));
    assert!(facts.alias_groups.is_empty());
    assert_eq!(facts.name_bindings[0].occurrences.len(), 2);
    for occurrence in &facts.name_bindings[0].occurrences {
        assert_eq!(fixed.selection_text(occurrence).as_deref(), Some("TMPDIR"));
    }
    let mut shuffled = document.clone();
    let DocumentBody::Fixed(shuffled_fixed) = &mut shuffled.body else {
        unreachable!()
    };
    shuffled_fixed.owners[0]
        .entry
        .as_mut()
        .unwrap()
        .name_bindings
        .reverse();
    assert!(validate_document(&shuffled).is_empty());
    let round_trip: mant_ir::Document =
        serde_json::from_value(serde_json::to_value(&shuffled).unwrap()).unwrap();
    assert!(validate_document(&round_trip).is_empty());
    let DocumentBody::Fixed(shuffled_fixed) = &mut shuffled.body else {
        unreachable!()
    };
    let bindings = &mut shuffled_fixed.owners[0]
        .entry
        .as_mut()
        .unwrap()
        .name_bindings;
    let wrong_occurrence = bindings[0].occurrences[0].clone();
    bindings[2].occurrences[0] = wrong_occurrence;
    assert!(!validate_document(&shuffled).is_empty());
}

#[test]
fn empty_native_components_do_not_erase_later_visible_roles() {
    // Exact input ran pinned CVS -Tutf8. mdoc_macro.c::in_line retains Ev/Va
    // instances even when term.c::term_word emits no glyph for \&.
    let input = b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It Ev ONE , Ev \\& , Ev TWO\nBody.\n.It Va \\& , Ev HOME\nBody.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(fixed.owners[0].head_components.len(), 2);
    assert_group(
        fixed,
        &fixed.owners[0],
        EntryKind::EnvironmentVariable,
        &["ONE", "TWO"],
        mant_ir::EntryNameEvidence::NativeMarkup,
    );
    assert_eq!(fixed.owners[1].head_role, Some(OwnerHeadRole::Environment));
    assert_eq!(fixed.owners[1].head_components.len(), 1);
    assert_group(
        fixed,
        &fixed.owners[1],
        EntryKind::EnvironmentVariable,
        &["HOME"],
        mant_ir::EntryNameEvidence::NativeMarkup,
    );

    // A persisted role edit cannot borrow the still-valid visible Ev binding.
    let mut forged = document.clone();
    let DocumentBody::Fixed(forged_fixed) = &mut forged.body else {
        unreachable!()
    };
    forged_fixed.owners[1].head_role = Some(OwnerHeadRole::DefinedVariable);
    assert!(!validate_document(&forged).is_empty());
}

#[test]
fn environment_name_budget_omits_semantics_but_keeps_native_text() {
    // Exact 65-member input ran through pinned CVS -Tutf8 before assertion.
    // The semantic cap is not a roff limit: man_term.c::pre_B prints the full
    // HEAD, which must survive with an explicit coverage diagnostic.
    let names = (1..=65)
        .map(|index| format!("A{index}"))
        .collect::<Vec<_>>();
    let input = format!(
        ".TH T 1\n.SH ENVIRONMENT\n.TP\n.B {}\nBody.\n",
        names.join(", ")
    );
    let document =
        project_annotated_manual("t.1", &bundle(input.as_bytes()), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    assert!(!mant_ir::semantics_complete(&document.diagnostics));
    assert!(
        document
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("semantic name budget") })
    );
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert!(fixed.surface.text.contains("A65"));
    assert!(fixed.owners.iter().all(|owner| owner.entry.is_none()));
}

#[test]
fn nearest_recognized_section_overrides_outer_environment_context() {
    // Exact input ran pinned CVS -Tutf8. man_term.c renders SS below SH;
    // title case and semantic inheritance are ManT's finite heading context.
    let input = b".TH T 1\n.SH Environment\n.TP\n.B HOME\nEnvironment description.\n.SS OPTIONS\n.TP\n.B FOO\nOption description.\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_group(
        fixed,
        &fixed.owners[0],
        EntryKind::EnvironmentVariable,
        &["HOME"],
        mant_ir::EntryNameEvidence::Lexical,
    );
    assert!(
        !fixed.owners[1]
            .entry
            .as_ref()
            .is_some_and(|facts| { facts.kind == EntryKind::EnvironmentVariable })
    );
}

#[test]
fn mixed_visible_native_roles_do_not_invent_a_group_or_hide_text() {
    // Exact input ran pinned CVS -Tutf8. mdoc_macro.c::in_line executes both
    // authored Ev and Va; one mixed HEAD is not a homogeneous declaration.
    let input = b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It Ev HOME , Va value\nBody.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert!(fixed.owners[0].entry.is_none());
    assert!(fixed.surface.text.contains("HOME"));
    assert!(fixed.surface.text.contains("value"));
}

#[test]
fn owned_va_dv_role_token_mismatch_isolated_from_native_body() {
    // E02's Va/Dv instances ran through pinned CVS before this owned-result
    // mutation. The token/role mismatch is synthetic unsafe metadata, not an
    // authored roff behavior claim.
    let input = b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag\n.It Va counter\nBody.\n.It Dv MODE_FAST\nBody.\n.El\n";
    let page = AnnotatedRenderer::default()
        .render_bundle("t.1", &bundle(input), InputFormat::Mdoc)
        .unwrap();
    for role in [2048_u32, 4096] {
        let mut forged = page.clone();
        let component = forged
            .marks
            .iter_mut()
            .find(|mark| mark.kind == 6 && mark.flags & role != 0)
            .unwrap();
        component.token = if role == 2048 { 276 } else { 295 };
        let expected = forged.text.clone();
        let degraded = lower_annotated_document(forged).unwrap();
        assert!(validate_document(&degraded).is_empty());
        assert!(!mant_ir::semantics_complete(&degraded.diagnostics));
        let DocumentBody::Fixed(fixed) = &degraded.body else {
            panic!("not Fixed")
        };
        assert_eq!(fixed.surface.text, expected);
        assert!(fixed.owners.is_empty());
    }
}
