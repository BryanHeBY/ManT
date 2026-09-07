//! Original, redistributable regressions for styled names versus parameters.
use std::path::Path;

use mant_engine::parse_manual_bytes;
use mant_ir::SemanticIndex;

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
