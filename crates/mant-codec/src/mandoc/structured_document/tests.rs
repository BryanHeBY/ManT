use super::*;
use mant_ir::{EntryKind, ParameterKind, ResolvedContent};
use mant_protocol::{EntryProjection, EvidenceBasis, ExplanationOptions, ExplanationQuery};

#[test]
fn cross_wrapper_terms_reach_real_entry_facts_without_body_borrowing() {
    // The same source was run through the pinned reference before this
    // assertion. `man_macro.c::blk_imp` binds TQ to the preceding TP body;
    // `man_term.c::pre_TP` executes BR/B wrappers in formatter order.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "c03.1",
            br#".TH C03 1
.SH OPTIONS
.TP
.BR --output , " -o=" FILE
.TQ
.B -O
Write file.
.TP
.B --empty
.TP
.B --same
Body A.
.TP
.B --same
Body B.
"#
            .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("c03.1", &bundle, InputFormat::Man)
        .expect("native structure lowers to semantic IR");
    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("first structured block is a definition list")
    };
    assert_eq!(items.len(), 1, "each man TP owns its native list block");
    assert_eq!(items[0].terms.len(), 2);
    let facts = items[0].entry.as_ref().expect("option facts discovered");
    assert_eq!(
        facts.kind,
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Option
        }
    );
    assert_eq!(facts.names, ["--output", "-o", "-O"]);
    assert_eq!(items[0].description.len(), 1);

    let definition_items = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items.as_slice()),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(definition_items.len(), 4);
    assert!(definition_items[1].description.is_empty());
    assert_eq!(definition_items[2].description.len(), 1);
    assert_eq!(definition_items[3].description.len(), 1);
    assert_ne!(definition_items[2].source, definition_items[3].source);

    let query = ResolvedContent {
        label: "c03(1)".to_owned(),
        address: None,
        document: Some(document),
        tldr: None,
    };
    let outline = mant_query::build_outline_projection(&query, EntryProjection::All, None)
        .expect("the actual outline consumer accepts native EntryFacts");
    let outline_json = serde_json::to_string(&outline).unwrap();
    assert!(outline_json.contains("--output"));
    assert!(outline_json.contains("-o=FILE"));

    let explanation = mant_query::explain_query(
        &query,
        &ExplanationQuery {
            entry: "-o".to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .expect("the actual explanation consumer accepts native EntryFacts");
    assert_eq!(explanation.total, 1);
    assert_eq!(explanation.returned, 1);
    let evidence = &explanation.evidence[0];
    let entry = evidence.entry.as_ref().expect("semantic facts retained");
    assert_eq!(entry.names, ["--output", "-o", "-O"]);
    assert!(evidence.bases.iter().any(|basis| matches!(
        basis,
        EvidenceBasis::Name { matches }
            if matches.iter().any(|matched| matched.name == "-o")
    )));
    assert_eq!(entry.name_bindings.len(), 3);
}

#[test]
fn mdoc_native_kinds_nesting_and_targets_survive_ir_lowering() {
    // Reference output and the corresponding
    // `mdoc_term.c::termp_bl_pre/termp_it_pre` path were inspected before
    // adding this assertion; `tag.c::tag_move_id` owns Tg attachment.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "c03.1",
            br".Dd September 21, 2026
.Dt C03 1
.Os
.Sh OPTIONS
.Bl -tag -compact
.It Fl o Ar file
Write file.
.Tg item-target
.It Fl q
.Bl -enum
.It
Nested one.
.It
Nested two.
.El
.El
"
            .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("c03.1", &bundle, InputFormat::Mdoc)
        .expect("mdoc structure lowers to semantic IR");
    let Block::DefinitionList { items, compact, .. } = &document.sections[0].blocks[0] else {
        panic!("tag list retained")
    };
    assert!(*compact);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].entry.as_ref().unwrap().names, ["-o"]);
    assert_eq!(items[1].entry.as_ref().unwrap().names, ["-q"]);
    let Block::List {
        kind,
        items: nested,
        ..
    } = &items[1].description[0]
    else {
        panic!("nested enum retained inside its owning item")
    };
    assert_eq!(*kind, ListKind::Ordered { start: Some(1) });
    assert_eq!(nested.len(), 2);
    assert!(
        items[1].terms[0].iter().any(
            |inline| matches!(inline, Inline::Anchor { id, .. } if id.as_str() == "item-target")
        )
    );
}
