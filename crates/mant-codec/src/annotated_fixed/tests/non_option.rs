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
            3,
            OwnerHeadRole::DefinedVariable,
            EntryKind::Term,
            &["MODE_FAST"][..],
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
            component.role == role && (component.source.is_some() || component.source_key.is_some())
        }));
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
