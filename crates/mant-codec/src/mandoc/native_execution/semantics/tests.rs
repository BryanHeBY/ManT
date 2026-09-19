use super::*;
use crate::mandoc::native_execution::project_semantic_document;
use libmandoc_rs::{ExecutionLimits, InputFormat, ParseOptions, Parser};
use mant_ir::{EntryNameEvidence, EntryOwnerLocationRef};

const REPEATED_TQ: &[u8] = br".TH PROBE 1
.SH OPTIONS
.de XX
.TP
.B -a
.TQ
.B --alpha
.TP
.B --beta
SHARED BODY.
.PP
SEPARATOR.
.TP
.B -a
.TQ
.B --alpha
.TP
.B --beta
SHARED BODY.
..
.XX
";

const MDOC_ENTRIES: &[u8] = br".Dd September 19, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd semantic projection probe
.Sh OPTIONS
.Bl -tag
.Tg Mixed.Target
.It Fl a , Fl Fl alpha Ar value
BODY mentions --wrong and uses
.Lk https://example.org details .
.It Ev PATH
Environment body.
.El
.Sh COMMANDS
.Bl -bullet
.It
.Cm run
.El
";

const MDOC_REFERENCE: &[u8] = br".Dd September 19, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd reference projection probe
.Sh COMMANDS
.Bl -tag
.It Cm git-add , Xr git-add 1
BODY.
.El
";

const EMPTY_ITEM_TARGET: &[u8] = br".Dd September 19, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd empty target probe
.Sh ITEMS
.Bl -bullet
.Tg empty-item-target
.It
.El
";

const EMPTY_LIST_TARGET: &[u8] = br".Dd September 19, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd empty list target probe
.Sh ITEMS
.Bl -bullet
.Tg only-target
.El
";

const ORDERED_EMPTY_ITEM_TARGETS: &[u8] = br".Dd September 20, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd list target order
.Sh ITEMS
.Bl -bullet
.Tg first-target
.Sm off
.It
.Tg second-target
.Sm on
.It
.El
";

const NESTED_MDOC_LIST: &[u8] = br".Dd September 19, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd nested owner probe
.Sh OPTIONS
.Bl -tag
.It Fl a
Outer body.
.Bl -tag
.It Fl b
Inner body.
.El
.El
";

const DUPLICATE_MDOC_TAGS: &[u8] = br".Dd September 19, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd duplicate native tags
.Sh OPTIONS
.Bl -tag
.It Fl a
First.
.It Fl a
Second.
.El
";

const NESTED_MDOC_BOUNDARIES: &[u8] = br".Dd September 19, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd nested boundary probe
.Sh OPTIONS
.Bl -tag
.It Fl a
Outer first.
.Pp
Outer second.
.Bl -tag
.It Fl b
Inner first.
.Pp
Inner second.
.El
.Pp
Outer tail.
.El
";

const LEADING_ROLE: &[u8] = br".Dd September 19, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd leading role probe
.Sh ITEMS
.Bl -tag -width 6n
.It Ar file Fl x
Argument-led item.
.It Ev PATH Fl p
Environment-led item.
.El
";

const MAN_IP_MARKS: &[u8] = br#".TH PROBE 1 "September 19, 2026"
.SH NAME
probe \- IP mark probe
.SH ITEMS
.IP "*" 4
Bullet prose.
.IP "\fB#\fP" 4
Styled key.
"#;

const MDOC_COLUMN: &[u8] = br".Dd September 19, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd column probe
.Sh TABLE
.Bl -column Name Value
.It Cm alpha Ta Ar beta
.El
";

const NATIVE_GRAMMAR: &[u8] = br".Dd September 19, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd native grammar evidence
.Sh COMMANDS
.Bl -tag
.It Cm query user
Command body.
.El
.Sh ENVIRONMENT
.Bl -tag
.It Ev GIT_CONFIG_KEY_ Ns Ar n
Template body.
.It Ev PATH HOME
Concrete variables.
.El
";

const MAN_GROUP_BARRIER: &[u8] = br#".TH PROBE 1 "September 19, 2026"
.SH OPTIONS
.TP
.B --alpha
.br
.TP
.B --beta
SHARED BODY.
.SH TERMS
.TP
.B Foo
.TP
.B Bar
TERM BODY.
"#;

const MDOC_DECLARATION_GROUP: &[u8] = br".Dd September 19, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd mdoc declaration group
.Sh OPTIONS
.Bl -tag
.It Fl a
.It Fl b
SHARED BODY.
.El
";

const TARGET_ENTRY_OWNER: &[u8] = br".Dd September 20, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd target entry owner
.Sh COMMANDS
.Bl -tag
.Tg command-foo
.It Cm foo
Body.
.El
";

const AUTHORED_CONTEXT: &[u8] = br".Dd September 20, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd authored context
.Sm off
.Sh ENVIRONMENT VARIABLES
.Bl -tag
.It Va PATH
Search path.
.El
";

const SECTION_ENTRY_COLLISION: &[u8] = br".Dd September 20, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd section entry collision
.Tg command-foo
.Sh COMMANDS
.Bl -tag
.It Cm foo
Body.
.El
";

const SECTION_IDENTITIES: &[u8] = br".Dd September 19, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd section identity probe
.Sh SEE ALSO
.No first
.Sh SEE ALSO
.No second
.Sh TERM GIT ADD
.Bl -tag -width 6n
.It Cm git-add
Command body.
.El
";

fn execute(name: &str, format: InputFormat, source: &[u8]) -> libmandoc_rs::ExecutionReport {
    Parser::new(ParseOptions::default())
        .with_input_format(format)
        .with_mdoc_operating_system("ManT")
        .unwrap()
        .execute_bytes(name, source, ExecutionLimits::default())
        .unwrap()
}

fn definition_lists(section: &Section) -> Vec<(&[DefinitionItem], &[mant_ir::DeclarationGroup])> {
    section
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList {
                items,
                declaration_groups,
                ..
            } => Some((items.as_slice(), declaration_groups.as_slice())),
            _ => None,
        })
        .collect()
}

#[test]
fn binds_repeated_tq_expansions_by_execution_owner_not_source_coordinates() {
    // Oracle was run before this assertion with the pinned CVS binary.  Its
    // man.c next-line scopes create independent TP/TQ/TP blocks for each
    // expansion, and man_term.c pre_TP()/post_TP() leaves only the final TP
    // body readable in each declaration run.
    let executed = execute("repeated-tq.1", InputFormat::Man, REPEATED_TQ);
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let options = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "OPTIONS")
        .unwrap();
    let lists = definition_lists(options);
    assert_eq!(lists.len(), 2);
    for (items, groups) in lists {
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].terms.len(), 2);
        assert_eq!(mant_ir::inline_plain_text(&items[0].terms[0]), "-a");
        assert_eq!(mant_ir::inline_plain_text(&items[0].terms[1]), "--alpha");
        assert_eq!(mant_ir::inline_plain_text(&items[1].terms[0]), "--beta");
        assert_eq!(
            groups,
            &[mant_ir::DeclarationGroup {
                start_item: 0,
                end_item: 2,
            }]
        );
    }
    assert_eq!(
        projected
            .receipts
            .iter()
            .filter(|receipt| receipt.origins.len() == 2)
            .count(),
        2,
        "each TP owner receives only its own immediately following TQ"
    );
}

#[test]
fn binds_markup_names_targets_links_and_owner_locations_without_device_rows() {
    // This exact fixture was first checked with pinned CVS -Tutf8, -Thtml,
    // and -Tlint.  mdoc_term.c emits Fl/Ev through their semantic macro
    // nodes, tag.c moves Mixed.Target to the actual It head, and the body Lk
    // opens one typed reference around its complete label.
    let executed = execute("semantic-mdoc.1", InputFormat::Mdoc, MDOC_ENTRIES);
    let projected = project_semantic_document(&executed.document, &executed.execution);
    assert!(mant_ir::validate_document(&projected.document).is_empty());
    let options = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "OPTIONS")
        .unwrap();
    let lists = definition_lists(options);
    let [(items, _)] = lists.as_slice() else {
        panic!("one native OPTIONS definition list")
    };
    let first = items[0].entry.as_ref().unwrap();
    assert_eq!(first.names, ["-a", "--alpha"]);
    assert!(!first.names.iter().any(|name| name == "--wrong"));
    assert!(
        first
            .name_bindings
            .iter()
            .all(|binding| binding.evidence == EntryNameEvidence::NativeMarkup)
    );
    assert!(format!("{:?}", items[0].terms).contains("Mixed.Target"));
    assert!(format!("{:?}", items[0].description).contains("https://example.org"));

    let path = projected
        .receipts
        .iter()
        .find(|receipt| receipt.id == first.id)
        .unwrap();
    let owner = EntryOwnerLocationRef {
        sections: &path.sections,
        blocks: &path.blocks,
        item_index: path.item_index,
    }
    .resolve(&projected.document)
    .expect("receipt resolves to its materialized owner");
    assert_eq!(owner.facts().unwrap().id, first.id);

    let commands = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "COMMANDS")
        .unwrap();
    let [Block::List { items, .. }] = commands.blocks.as_slice() else {
        panic!("ordinary mdoc bullet remains an ordinary list")
    };
    assert!(
        items[0].entry.is_none(),
        "Cm in prose is not an entry declaration"
    );

    let json = serde_json::to_string(&projected.document).unwrap();
    assert!(!json.contains("ExecutionNodeKey"));
    assert!(!json.contains("mant-native-definition-owner"));
    let round_trip: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip, projected.document);
}

#[test]
fn keeps_one_native_manual_reference_wrapper_inside_the_term() {
    // Pinned CVS HTML was checked before this assertion: Cm produces the
    // command spelling while Xr opens one typed git-add(1) reference.  The
    // comma and reference are presentation, not extra selectable names.
    let executed = execute("reference-mdoc.1", InputFormat::Mdoc, MDOC_REFERENCE);
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let commands = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "COMMANDS")
        .unwrap();
    let lists = definition_lists(commands);
    let [(items, _)] = lists.as_slice() else {
        panic!("one command definition list")
    };
    let facts = items[0].entry.as_ref().unwrap();
    assert_eq!(facts.names, ["git-add"]);
    assert_eq!(
        facts.name_bindings[0].evidence,
        EntryNameEvidence::NativeMarkup
    );
    let debug = format!("{:?}", items[0].terms[0]);
    assert_eq!(debug.matches("Manual").count(), 1, "one Xr wrapper");
    assert!(debug.contains("git-add"));
}

#[test]
fn keeps_a_zero_width_target_on_an_empty_native_list_item() {
    // Checked first with the pinned CVS HTML renderer.  post_tg() leaves this
    // Tg identity on the Bl body because it precedes the first It; it remains
    // a zero-width <mark> directly before the empty list item.
    let executed = execute("empty-item-target.1", InputFormat::Mdoc, EMPTY_ITEM_TARGET);
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let section = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "ITEMS")
        .expect("ITEMS section");
    let [Block::Paragraph { children, .. }, Block::List { items, .. }] = section.blocks.as_slice()
    else {
        panic!("container anchor remains before one native bullet item")
    };
    assert_eq!(items.len(), 1);
    assert!(items[0].blocks.is_empty());
    assert!(matches!(
        children.as_slice(),
        [Inline::Anchor { id, fragment_aliases, .. }]
            if id.as_str() == "target-empty-item-target"
                && fragment_aliases.iter().any(|alias| alias.as_str() == "empty-item-target")
    ));
}

#[test]
fn keeps_a_zero_width_target_owned_by_an_empty_native_list() {
    // Pinned CVS tag.c::tag_postprocess() deliberately leaves a Tg target on
    // an empty Bl container; its HTML output is a mark inside an empty ul.
    // IR has no container-level inline slot, so retain the zero-width anchor
    // immediately before the still-empty list without inventing an It owner.
    let executed = execute("empty-list-target.1", InputFormat::Mdoc, EMPTY_LIST_TARGET);
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let items = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "ITEMS")
        .unwrap();
    let [Block::Paragraph { children, .. }, Block::List { items, .. }] = items.blocks.as_slice()
    else {
        panic!("target paragraph and empty native list remain adjacent")
    };
    assert!(items.is_empty(), "no synthetic list item is introduced");
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Anchor { id, fragment_aliases, .. }
            if id.as_str() == "target-only-target"
                && fragment_aliases.iter().any(|alias| alias.as_str() == "only-target")
    )));
    assert!(mant_ir::validate_document(&projected.document).is_empty());
}

#[test]
fn list_level_targets_follow_their_exact_structural_owner() {
    // Checked first with the pinned CVS HTML renderer.  post_tg() leaves the
    // first target on the Bl body because no It precedes it; tag_move_id()
    // moves the second target onto the preceding empty It.  Transparent Sm
    // requests do not create or change target ownership.
    let executed = execute(
        "ordered-empty-item-targets.1",
        InputFormat::Mdoc,
        ORDERED_EMPTY_ITEM_TARGETS,
    );
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let section = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "ITEMS")
        .unwrap();
    let [Block::Paragraph { children, .. }, Block::List { items, .. }] = section.blocks.as_slice()
    else {
        panic!("container target remains before the two-item list")
    };
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Anchor { fragment_aliases, .. }
            if fragment_aliases.iter().any(|alias| alias.as_str() == "first-target")
    )));
    assert_eq!(items.len(), 2);
    assert!(format!("{:?}", items[0].blocks).contains("second-target"));
    assert!(!format!("{:?}", items[1].blocks).contains("second-target"));
    assert!(mant_ir::validate_document(&projected.document).is_empty());
}

#[test]
fn keeps_nested_native_list_owners_and_receipt_paths_inside_the_parent_body() {
    // The pinned CVS HTML renderer was run before this assertion.  In
    // mdoc_term.c::termp_it_pre()/termp_it_post(), the inner Bl remains in the
    // outer It body; it is not a sibling list at section scope.
    let executed = execute("nested-mdoc-list.1", InputFormat::Mdoc, NESTED_MDOC_LIST);
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let options = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "OPTIONS")
        .unwrap();
    let [Block::DefinitionList { items, .. }] = options.blocks.as_slice() else {
        panic!("one top-level native list")
    };
    let [outer] = items.as_slice() else {
        panic!("one outer definition")
    };
    assert_eq!(outer.entry.as_ref().unwrap().names, ["-a"]);
    let nested = outer
        .description
        .iter()
        .find_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items),
            _ => None,
        })
        .expect("inner list remains in the outer description");
    assert_eq!(nested[0].entry.as_ref().unwrap().names, ["-b"]);
    let nested_json = serde_json::to_string(&outer.description).unwrap();
    assert_eq!(nested_json.matches("Inner body.").count(), 1);
    assert_eq!(mant_ir::inline_plain_text(&nested[0].terms[0]), "-b");

    for expected in ["-a", "-b"] {
        let receipt = projected
            .receipts
            .iter()
            .find(|receipt| {
                EntryOwnerLocationRef {
                    sections: &receipt.sections,
                    blocks: &receipt.blocks,
                    item_index: receipt.item_index,
                }
                .resolve(&projected.document)
                .is_some_and(|owner| owner.facts().unwrap().names == [expected])
            })
            .expect("native receipt resolves through the final nested path");
        if expected == "-b" {
            assert!(
                receipt.blocks.len() > 1,
                "inner owner path traverses the outer definition body"
            );
        }
    }
}

#[test]
fn native_fragments_follow_cvs_html_case_and_duplicate_ordinals() {
    // Verified first with the pinned CVS HTML renderer.  html_make_id() keeps
    // the first authored fragment `a` and assigns `a~2` to the second target;
    // canonical source-neutral IDs remain a separate namespace.
    let executed = execute(
        "duplicate-mdoc-tags.1",
        InputFormat::Mdoc,
        DUPLICATE_MDOC_TAGS,
    );
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let options = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "OPTIONS")
        .unwrap();
    let [Block::DefinitionList { items, .. }] = options.blocks.as_slice() else {
        panic!("one native definition list")
    };
    let fragments = items
        .iter()
        .map(|item| {
            item.terms[0]
                .iter()
                .find_map(|inline| match inline {
                    Inline::Anchor {
                        id,
                        fragment_aliases,
                        ..
                    } if id.as_str().starts_with("target-") => Some((
                        id.as_str().to_owned(),
                        fragment_aliases
                            .iter()
                            .map(|alias| alias.as_str().to_owned())
                            .collect::<Vec<_>>(),
                    )),
                    _ => None,
                })
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        fragments,
        [
            ("target-a".to_owned(), vec!["a".to_owned()]),
            ("target-a-2".to_owned(), vec!["a~2".to_owned()]),
        ]
    );
    assert!(
        mant_ir::validate_document(&projected.document)
            .iter()
            .all(|diagnostic| diagnostic.code.as_deref() != Some("ir.ambiguous-fragment-alias"))
    );
}

#[test]
fn nested_list_ranges_do_not_duplicate_parent_or_child_boundary_content() {
    // Checked first with the pinned CVS HTML renderer.  mdoc_term.c keeps all
    // three outer paragraphs around one nested Bl, while the two inner
    // paragraphs remain owned by the inner It exactly once.
    let executed = execute(
        "nested-mdoc-boundaries.1",
        InputFormat::Mdoc,
        NESTED_MDOC_BOUNDARIES,
    );
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let section = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "OPTIONS")
        .unwrap();
    let json = serde_json::to_string(section).unwrap();
    for text in [
        "Outer first.",
        "Outer second.",
        "Inner first.",
        "Inner second.",
        "Outer tail.",
    ] {
        assert_eq!(json.matches(text).count(), 1, "{text} has one native owner");
    }
    assert!(mant_ir::validate_document(&projected.document).is_empty());
}

#[test]
fn native_markup_index_scales_with_definition_subtrees_not_document_product() {
    // The repeated `Bl -tag` / `It Fl` grammar was first checked with the
    // pinned CVS HTML renderer.  Dense DFS subtree and wrapper indexes must be
    // built once; each head may inspect only its own native descendants.
    const ITEM_COUNT: usize = 2_048;
    let mut source = String::from(
        ".Dd September 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd scale grammar\n.Sh OPTIONS\n.Bl -tag\n",
    );
    for item in 0..ITEM_COUNT {
        source.push_str(".It Fl x");
        source.push_str(&item.to_string());
        source.push_str("\nbody ");
        source.push_str(&item.to_string());
        source.push('\n');
    }
    source.push_str(".El\n");
    let executed = execute("semantic-scale.1", InputFormat::Mdoc, source.as_bytes());
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let section = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "OPTIONS")
        .unwrap();
    let [Block::DefinitionList { items, .. }] = section.blocks.as_slice() else {
        panic!("one native tag list")
    };
    assert_eq!(items.len(), ITEM_COUNT);
    assert!(items.iter().all(|item| item.entry.is_some()));
}

#[test]
fn native_reference_index_scales_with_each_owner_slice() {
    // The exact two-item grammar was checked first with pinned CVS HTML:
    // mdoc_term.c executes one Cm command and one Xr reference in each It
    // head.  The scale variant must retain exactly one typed reference per
    // owner without rescanning the document-wide reference table.
    const ITEM_COUNT: usize = 2_048;
    let mut source = String::from(
        ".Dd September 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd reference scale grammar\n.Sh COMMANDS\n.Bl -tag\n",
    );
    for item in 0..ITEM_COUNT {
        source.push_str(".It Cm cmd");
        source.push_str(&item.to_string());
        source.push_str(" , Xr printf 3\nbody\n");
    }
    source.push_str(".El\n");
    let executed = execute("reference-scale.1", InputFormat::Mdoc, source.as_bytes());
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let section = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "COMMANDS")
        .unwrap();
    let lists = definition_lists(section);
    let [(items, _)] = lists.as_slice() else {
        panic!("one native command list")
    };
    assert_eq!(items.len(), ITEM_COUNT);
    assert!(
        items
            .iter()
            .all(|item| { format!("{:?}", item.terms).matches("Manual").count() == 1 })
    );
}

#[test]
fn native_environment_word_index_scales_with_each_owner_subtree() {
    // The two-operand Ev grammar was checked first with pinned CVS HTML in
    // NATIVE_GRAMMAR: each operand is an independent identifier word.  This
    // scale form exercises the same native word-owner index for every item.
    const ITEM_COUNT: usize = 2_048;
    let mut source = String::from(
        ".Dd September 20, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd environment scale grammar\n.Sh ENVIRONMENT\n.Bl -tag\n",
    );
    for item in 0..ITEM_COUNT {
        source.push_str(".It Ev VAR");
        source.push_str(&item.to_string());
        source.push_str(" ALT");
        source.push_str(&item.to_string());
        source.push_str("\nbody\n");
    }
    source.push_str(".El\n");
    let executed = execute("environment-scale.1", InputFormat::Mdoc, source.as_bytes());
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let section = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "ENVIRONMENT")
        .unwrap();
    let lists = definition_lists(section);
    let [(items, _)] = lists.as_slice() else {
        panic!("one native environment list")
    };
    assert_eq!(items.len(), ITEM_COUNT);
    assert_eq!(items[0].entry.as_ref().unwrap().names, ["VAR0", "ALT0"]);
    assert_eq!(
        items[ITEM_COUNT - 1].entry.as_ref().unwrap().names,
        ["VAR2047", "ALT2047"]
    );
}

#[test]
fn list_target_index_scales_with_sibling_native_lists() {
    // The exact target placement rule was checked first with pinned CVS HTML
    // in ORDERED_EMPTY_ITEM_TARGETS.  Repeating independent lists must not
    // rescan the document-wide target table for every list.
    const LIST_COUNT: usize = 1_024;
    let mut source = String::from(
        ".Dd September 20, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd list target scale\n.Sh ITEMS\n",
    );
    for list in 0..LIST_COUNT {
        source.push_str(".Bl -bullet\n.Tg before-");
        source.push_str(&list.to_string());
        source.push_str("\n.It\n.Tg after-");
        source.push_str(&list.to_string());
        source.push_str("\n.El\n");
    }
    let executed = execute("list-target-scale.1", InputFormat::Mdoc, source.as_bytes());
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let section = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "ITEMS")
        .unwrap();
    assert_eq!(
        section
            .blocks
            .iter()
            .filter(|block| matches!(block, Block::List { .. }))
            .count(),
        LIST_COUNT
    );
    let json = serde_json::to_string(section).unwrap();
    assert_eq!(json.matches("before-").count(), LIST_COUNT * 2);
    assert_eq!(json.matches("after-").count(), LIST_COUNT * 2);
    assert!(mant_ir::validate_document(&projected.document).is_empty());
}

#[test]
fn canonical_section_allocator_scales_with_one_repeated_base() {
    // Duplicate SEE ALSO headings were checked first with pinned CVS HTML in
    // SECTION_IDENTITIES.  Canonical IR identities remain deterministic even
    // when CVS deliberately withholds duplicate authored fragments.
    const SECTION_COUNT: usize = 2_048;
    let mut source = String::from(
        ".Dd September 20, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd section identity scale\n",
    );
    for section in 0..SECTION_COUNT {
        source.push_str(".Sh SAME\nsection ");
        source.push_str(&section.to_string());
        source.push('\n');
    }
    let executed = execute(
        "section-identity-scale.1",
        InputFormat::Mdoc,
        source.as_bytes(),
    );
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let repeated = &projected.document.sections[1..];
    assert_eq!(repeated.len(), SECTION_COUNT);
    assert_eq!(repeated[0].id.as_str(), "section-same");
    assert_eq!(repeated[SECTION_COUNT - 1].id.as_str(), "section-same-2048");
}

#[test]
fn native_markup_qualifies_shared_name_grammar_without_replacing_it() {
    // Pinned CVS -Ttree/-Thtml confirms that `Cm query user` is one macro
    // with two authored operands and that Ev owns each identifier operand.
    // Native origin proves which ranges are declarations; the shared grammar
    // still rejects a joined Ev+Ar template and keeps a multiword command
    // atomic instead of manufacturing word aliases.
    let executed = execute("native-grammar.1", InputFormat::Mdoc, NATIVE_GRAMMAR);
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let commands = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "COMMANDS")
        .unwrap();
    let lists = definition_lists(commands);
    let [(commands, _)] = lists.as_slice() else {
        panic!("one command list")
    };
    assert_eq!(commands[0].entry.as_ref().unwrap().names, ["query user"]);

    let environment = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "ENVIRONMENT")
        .unwrap();
    let lists = definition_lists(environment);
    let [(environment, _)] = lists.as_slice() else {
        panic!("one environment list")
    };
    assert!(environment[0].entry.as_ref().unwrap().names.is_empty());
    assert_eq!(
        environment[1].entry.as_ref().unwrap().names,
        ["PATH", "HOME"]
    );
}

#[test]
fn declaration_groups_follow_native_flow_epochs_and_role_policy() {
    // Pinned CVS roff.c increments flow_epoch for .br/.sp.  Its HTML output
    // gives --alpha an independent <dd><br/></dd>, so it cannot borrow the
    // later option body. Generic terms remain outside declaration groups even
    // when adjacent, as required by the shared semantic grammar.
    let executed = execute("man-group-barrier.1", InputFormat::Man, MAN_GROUP_BARRIER);
    let projected = project_semantic_document(&executed.document, &executed.execution);
    for heading in ["OPTIONS", "TERMS"] {
        let section = projected
            .document
            .sections
            .iter()
            .find(|section| section.heading.plain_text() == heading)
            .unwrap();
        let lists = definition_lists(section);
        assert!(
            !lists.is_empty(),
            "at least one definition list in {heading}"
        );
        assert!(
            lists.iter().all(|(_, groups)| groups.is_empty()),
            "{heading}: {lists:?}"
        );
    }

    // Pinned CVS mdoc_term.c keeps adjacent tag items in one formatter flow;
    // the existing ManT contract records the empty first owner as sharing the
    // final option description when their parser flow epochs are equal.
    let executed = execute(
        "mdoc-declaration-group.1",
        InputFormat::Mdoc,
        MDOC_DECLARATION_GROUP,
    );
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let section = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "OPTIONS")
        .unwrap();
    let lists = definition_lists(section);
    let [(_, groups)] = lists.as_slice() else {
        panic!("one mdoc option list")
    };
    assert_eq!(
        groups,
        &[mant_ir::DeclarationGroup {
            start_item: 0,
            end_item: 2
        }]
    );
}

#[test]
fn native_targets_share_exact_semantic_owners_without_selector_collisions() {
    // Pinned CVS post_tg()/tag_move_id() attaches command-foo to the following
    // It head.  The semantic owner therefore keeps its role-qualified ID while
    // the authored fragment resolves to that same destination exactly once.
    let executed = execute("target-entry.1", InputFormat::Mdoc, TARGET_ENTRY_OWNER);
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let commands = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "COMMANDS")
        .unwrap();
    let lists = definition_lists(commands);
    let [(items, _)] = lists.as_slice() else {
        panic!("one command list")
    };
    assert_eq!(items[0].entry.as_ref().unwrap().id.as_str(), "command-foo");
    assert!(mant_ir::validate_document(&projected.document).is_empty());

    // A section-authored command-foo fragment is a different destination.
    // It reserves the selector namespace and deterministically disambiguates
    // the entry instead of making #command-foo ambiguous.
    let executed = execute(
        "section-entry-collision.1",
        InputFormat::Mdoc,
        SECTION_ENTRY_COLLISION,
    );
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let commands = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "COMMANDS")
        .unwrap();
    assert!(
        commands
            .fragment_aliases
            .iter()
            .any(|alias| alias.as_str() == "command-foo")
    );
    let lists = definition_lists(commands);
    let [(items, _)] = lists.as_slice() else {
        panic!("one command list")
    };
    assert_ne!(items[0].entry.as_ref().unwrap().id.as_str(), "command-foo");
    assert!(mant_ir::validate_document(&projected.document).is_empty());
}

#[test]
fn authored_section_phrase_selects_semantic_context_independently_of_display() {
    // Pinned CVS termp_sm_pre() removes display spacing, while HTML keeps the
    // authored ENVIRONMENT_VARIABLES fragment and Va remains an environment
    // variable declaration. Display text must not select the grammar.
    let executed = execute("authored-context.1", InputFormat::Mdoc, AUTHORED_CONTEXT);
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let section = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "ENVIRONMENTVARIABLES")
        .unwrap();
    let lists = definition_lists(section);
    let [(items, _)] = lists.as_slice() else {
        panic!("one environment list")
    };
    let entry = items[0].entry.as_ref().unwrap();
    assert_eq!(entry.kind, mant_ir::EntryKind::EnvironmentVariable);
    assert_eq!(entry.names, ["PATH"]);
    assert!(
        section
            .fragment_aliases
            .iter()
            .any(|alias| alias.as_str() == "ENVIRONMENT_VARIABLES")
    );
}

#[test]
fn native_markup_role_comes_from_the_first_meaningful_head_node() {
    // Pinned CVS mdoc_term.c executes Ar, Ev, and Fl in the authored sibling
    // order.  A later Fl styles only its own generated flag; it cannot change
    // the semantic role of the complete It head.
    let executed = execute("leading-role.1", InputFormat::Mdoc, LEADING_ROLE);
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let section = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "ITEMS")
        .unwrap();
    let lists = definition_lists(section);
    let [(items, _)] = lists.as_slice() else {
        panic!("one native definition list")
    };
    let first = items[0].entry.as_ref().unwrap();
    assert_ne!(
        first.kind,
        mant_ir::EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Option,
        }
    );
    assert!(!first.names.iter().any(|name| name == "-x"));
    let second = items[1].entry.as_ref().unwrap();
    assert_eq!(second.kind, mant_ir::EntryKind::EnvironmentVariable);
    assert_eq!(second.names, ["PATH"], "{second:?}");
    assert!(!second.names.iter().any(|name| name == "-p"));
}

#[test]
fn man_ip_presentation_marks_do_not_become_semantic_names() {
    // Pinned CVS man_term.c::pre_IP() executes both heads as tags.  The first
    // is pure presentation; the second carries explicit bold key evidence.
    // Neither fact is inferred from the rendered row or its source position.
    let executed = execute("ip-marks.1", InputFormat::Man, MAN_IP_MARKS);
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let section = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "ITEMS")
        .unwrap();
    let lists = definition_lists(section);
    let [(items, _)] = lists.as_slice() else {
        panic!("one native IP run")
    };
    assert!(items[0].entry.is_none());
    let styled = items[1].entry.as_ref().unwrap();
    assert_eq!(styled.kind, mant_ir::EntryKind::Term);
    assert!(!styled.names.iter().any(|name| name == "*"));
}

#[test]
fn staged_semantics_keeps_column_owners_nonsemantic_until_table_projection() {
    // Pinned CVS mdoc_term.c executes each column body with an independently
    // calculated offset. K20 owns the final table projection; K19 must retain
    // the row without treating Cm in a cell as a declaration or panicking.
    let executed = execute("column.1", InputFormat::Mdoc, MDOC_COLUMN);
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let table = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "TABLE")
        .unwrap();
    let [Block::List { items, .. }] = table.blocks.as_slice() else {
        panic!("K19 preserves the native column row for K20")
    };
    assert_eq!(items.len(), 1);
    assert!(items[0].entry.is_none());
}

#[test]
fn native_section_fragments_and_semantic_ids_keep_separate_namespaces() {
    // Pinned CVS html_make_id() exposes COMMAND_GIT_ADD, but duplicate SEE
    // ALSO headings receive no fragment at all. ManT reserves the semantic ID
    // prefix independently, so the section cannot shadow term-git-add.
    let executed = execute(
        "section-identities.1",
        InputFormat::Mdoc,
        SECTION_IDENTITIES,
    );
    let projected = project_semantic_document(&executed.document, &executed.execution);
    let see_also = projected
        .document
        .sections
        .iter()
        .filter(|section| section.heading.plain_text() == "SEE ALSO")
        .collect::<Vec<_>>();
    assert_eq!(see_also.len(), 2);
    assert!(
        see_also
            .iter()
            .all(|section| section.fragment_aliases.is_empty())
    );
    let commands = projected
        .document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "TERM GIT ADD")
        .unwrap();
    assert_eq!(commands.id.as_str(), "section-term-git-add-section");
    assert_eq!(commands.fragment_aliases[0].as_str(), "TERM_GIT_ADD");
    let lists = definition_lists(commands);
    let [(items, _)] = lists.as_slice() else {
        panic!("one command list")
    };
    let entry_id = items[0].entry.as_ref().unwrap().id.as_str();
    assert!(entry_id.starts_with("term-git-add"));
    assert_ne!(entry_id, commands.id.as_str());
    assert!(mant_ir::validate_document(&projected.document).is_empty());
}
