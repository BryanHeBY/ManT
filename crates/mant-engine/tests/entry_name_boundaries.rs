//! Original, redistributable regressions for styled names versus parameters.
use std::path::Path;

use mant_engine::parse_manual_bytes;
use mant_ir::SemanticIndex;

fn inline_text(inlines: &[mant_ir::Inline]) -> String {
    inlines
        .iter()
        .map(|inline| match inline {
            mant_ir::Inline::Text { value } | mant_ir::Inline::Code { value } => value.clone(),
            mant_ir::Inline::Strong { children }
            | mant_ir::Inline::Emphasis { children }
            | mant_ir::Inline::Link { children, .. } => inline_text(children),
            mant_ir::Inline::Anchor { .. } => String::new(),
            mant_ir::Inline::LineBreak => "\n".into(),
        })
        .collect()
}

fn assert_direct_names(query: &mant_ir::ResolvedContent, names: &[&str], form: &str) {
    use mant_protocol::{EvidenceBasis, EvidenceClass, ExplanationQuery};
    let document = query.document.as_ref().unwrap();
    assert!(
        mant_ir::validate_document(document).is_empty(),
        "{:?}",
        document.diagnostics
    );
    assert!(
        !document
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("ir.invalid-entry-name-binding")),
        "{:?}",
        document.diagnostics
    );
    for name in names {
        let explained = mant_engine::explain_query(
            query,
            &ExplanationQuery {
                entry: (*name).into(),
                options: mant_protocol::ExplanationOptions::default(),
            },
        )
        .unwrap();
        let direct = explained
            .evidence
            .iter()
            .find(|e| e.class == EvidenceClass::DirectEntry)
            .unwrap_or_else(|| panic!("{name}: {explained:?}"));
        assert!(direct.bases.contains(&EvidenceBasis::Name));
        let entry = direct.entry.as_ref().unwrap();
        assert_eq!(entry.names, names);
        assert_eq!(
            entry
                .forms
                .iter()
                .map(|f| inline_text(f))
                .collect::<Vec<_>>(),
            [form]
        );
        let excerpt = mant_engine::select_excerpt(query, &[direct.outline.path()]).unwrap();
        assert!(mant_engine::render_excerpt_text(&excerpt).contains("PAYLOAD"));
    }
    mant_ir::visit::Visit::visit_document(&mut Bindings, document);
}

struct Bindings;
impl<'a> mant_ir::visit::Visit<'a> for Bindings {
    fn visit_list_item(&mut self, item: &'a mant_ir::ListItem) {
        check_bindings(mant_ir::EntryOwner::List(item));
        mant_ir::visit::walk_list_item(self, item);
    }
    fn visit_definition_item(&mut self, item: &'a mant_ir::DefinitionItem) {
        check_bindings(mant_ir::EntryOwner::Definition(item));
        mant_ir::visit::walk_definition_item(self, item);
    }
}

#[test]
fn enclosure_spacing_preserves_option_forms_names_and_explanation_sources() {
    for head in ["Fl x Oo Ar arg Ns Oc Ar tail", "Fl x Oo Pf arg Oc Ar tail"] {
        let source = format!(
            ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.Tg Exact.Target\n.It {head}\nPAYLOAD.\n.El\n"
        );
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        assert_direct_names(&query, &["-x"], "-x [arg] tail");
        let document = query.document.as_ref().unwrap();
        let mant_ir::Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
            panic!("expected definition")
        };
        assert_eq!(items[0].source.unwrap().line, 7);
        assert!(mant_engine::render_query_text(&query).contains("-x [arg] tail"));
        assert!(items[0].terms[0].iter().any(|inline| matches!(
            inline, mant_ir::Inline::Strong { children } if inline_text(children) == "-x"
        )));
    }
}
fn check_bindings(owner: mant_ir::EntryOwner<'_>) {
    if let Some(facts) = owner.facts() {
        for binding in &facts.name_bindings {
            assert!(!binding.occurrences.is_empty());
            for occurrence in &binding.occurrences {
                assert_eq!(
                    inline_text(&owner.form(occurrence).unwrap()),
                    facts.names[binding.name]
                );
            }
        }
    }
}

#[test]
fn grammar_selected_ranges_survive_wrappers_arguments_and_styles() {
    for (head, names, form) in [
        ("\\-w{number}", vec!["-w"], "-w{number}"),
        (".B {--foo}", vec!["--foo"], "{--foo}"),
        (".B “--foo”", vec!["--foo"], "“--foo”"),
        (".B \" [-+]O\"", vec!["-O", "+O"], " [-+]O"),
        (".BR \" [\" - + ] O", vec!["-O", "+O"], " [-+]O"),
        (".BR “ -- foo ”", vec!["--foo"], "“--foo”"),
        (".IB \" \" --foo", vec!["--foo"], " --foo"),
        (".B --color[=WHEN]", vec!["--color"], "--color[=WHEN]"),
        (".B -D<NAME>", vec!["-D"], "-D<NAME>"),
        (".BI \"{-n/\" -NUM", vec!["-n"], "{-n/-NUM"),
    ] {
        let source = format!(".TH PROBE 1\n.SH OPTIONS\n.TP\n{head}\nPAYLOAD.\n");
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        assert_direct_names(&query, &names, form);
    }
}

#[test]
fn marker_prefixes_do_not_widen_the_existing_native_admission_grammar() {
    for (form, expected) in [
        ("--", vec!["--"]),
        ("-", vec!["-"]),
        ("-- FILE", vec![]),
        ("- FILE", vec![]),
    ] {
        let source = format!(".TH PROBE 1\n.SH OPTIONS\n.TP\n.B {form}\nPAYLOAD.\n");
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        let index = SemanticIndex::build(query.document.as_ref().unwrap());
        assert_eq!(index.section("options")[0].names, expected, "{form}");
    }
}

#[test]
fn inferred_markdown_uses_its_recognizer_evidence_not_declaration_boundaries() {
    for (form, names) in [
        ("--color[=WHEN]", vec!["--color"]),
        ("-D<NAME>", vec!["-D"]),
        ("-w{number}", vec!["-w"]),
        ("-a/--all", vec!["-a", "--all"]),
    ] {
        let query =
            mant_engine::query_markdown_text(&format!("# Options\n\n- `{form}`: PAYLOAD.\n"), None)
                .unwrap();
        assert_direct_names(&query, &names, form);
    }
}

#[test]
fn linked_names_and_adjacent_parameters_retain_independent_facts_and_forms() {
    let doc = parse_manual_bytes(
        Path::new("entry-name-boundaries-man.1"),
        include_bytes!("../../../tests/fixtures/roff/entry-name-boundaries-man.1"),
    )
    .unwrap();
    let index = SemanticIndex::build(&doc);
    let options = index.section("options");
    assert_eq!(options.len(), 4);
    assert_eq!(options[0].names, ["-a", "--all"]);
    assert_eq!(options[0].forms, ["-a, --all"]);
    assert_eq!(options[1].names, ["-L"]);
    assert_eq!(options[1].forms, ["-Ldir"]);
    assert_eq!(options[2].names, ["-Wall"]);
    assert_eq!(options[3].names, ["-O2"]);
}

#[test]
fn mdoc_adjacent_and_slash_arguments_never_become_selectable_names() {
    let doc = parse_manual_bytes(
        Path::new("entry-name-boundaries-mdoc.1"),
        include_bytes!("../../../tests/fixtures/roff/entry-name-boundaries-mdoc.1"),
    )
    .unwrap();
    let index = SemanticIndex::build(&doc);
    let options = index.section("options");
    assert_eq!(options.len(), 4);
    assert_eq!(options[0].names, ["-L"]);
    assert_eq!(options[0].forms, ["-Ldir"]);
    assert_eq!(options[1].names, ["-n"]);
    assert_eq!(options[1].forms, ["-n/-NUM"]);
    assert_eq!(options[2].names, ["-a", "-all"]);
    assert_eq!(options[3].names, ["-Wall"]);
}

#[test]
fn a_bold_invocation_fragment_is_not_a_complete_command_name() {
    let doc = parse_manual_bytes(
        Path::new("entry-name-boundaries-man.1"),
        include_bytes!("../../../tests/fixtures/roff/entry-name-boundaries-man.1"),
    )
    .unwrap();
    let index = SemanticIndex::build(&doc);
    let commands = index.section("commands");
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].names, ["launch"]);
    assert_eq!(commands[0].forms, ["launch -p [-x]"]);
    assert_eq!(commands[1].names, ["["]);
}

#[test]
fn plus_signs_inside_executable_options_are_not_argument_boundaries() {
    for (name, truncated) in [
        ("-nostdinc++", "-nostdinc"),
        ("-Wc++11-compat", "-Wc"),
        ("-ObjC++", "-ObjC"),
    ] {
        for source in [
            format!(".TH PLUS 1\n.SH OPTIONS\n.TP\n.B {name}\nPAYLOAD.\n"),
            format!(
                ".Dd September 7, 2026\n.Dt PLUS 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl {}\nPAYLOAD.\n.El\n",
                &name[1..]
            ),
        ] {
            let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
            let doc = query.document.as_ref().unwrap();
            assert!(
                !doc.diagnostics
                    .iter()
                    .any(|d| d.code.as_deref() == Some("ir.invalid-entry-name-binding")),
                "{:?}",
                doc.diagnostics
            );
            let index = SemanticIndex::build(doc);
            assert_eq!(index.section("options")[0].names, [name]);
            assert!(mant_engine::select_excerpt(&query, &[name]).is_ok());
            assert!(mant_engine::select_excerpt(&query, &[truncated]).is_err());
        }
    }
}

#[test]
fn recognized_option_prefixes_bind_before_angle_delimited_arguments() {
    for (form, name) in [
        ("-D<macroname>=<value>", "-D"),
        ("-U<macroname>", "-U"),
        ("-I<directory>", "-I"),
        ("-F<directory>", "-F"),
        ("-fno-builtin-<function>", "-fno-builtin-"),
        ("-fno-builtin-std-<function>", "-fno-builtin-std-"),
    ] {
        let source = format!(".TH CLANG 1 2026-09-08\n.SH OPTIONS\n.TP\n.B {form}\nDescription.\n");
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        let doc = query.document.as_ref().unwrap();
        assert!(doc.diagnostics.is_empty(), "{form}: {:?}", doc.diagnostics);
        let index = SemanticIndex::build(doc);
        assert_eq!(index.section("options")[0].names, [name]);
        assert_eq!(index.section("options")[0].forms, [form]);
        assert!(mant_engine::select_excerpt(&query, &[name]).is_ok());
    }
    // Existing licensed real inputs must not silently lose those names.
    for fixture in ["archlinux/clang.1.gz", "fedora44/clang.1.zst"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/roff/real")
            .join(fixture);
        let doc = mant_engine::parse_manual_source(&path).unwrap();
        assert!(
            !doc.diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("ir.invalid-entry-name-binding")),
            "{fixture}: {:?}",
            doc.diagnostics
        );
    }
}
