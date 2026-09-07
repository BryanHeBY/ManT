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
    assert_eq!(options[0].aliases, ["-a", "--all"]);
    assert_eq!(options[0].forms, ["-a, --all"]);
    assert_eq!(options[1].aliases, ["-L"]);
    assert_eq!(options[1].forms, ["-Ldir"]);
    assert_eq!(options[2].aliases, ["-Wall"]);
    assert_eq!(options[3].aliases, ["-O2"]);
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
    assert_eq!(options[0].aliases, ["-L"]);
    assert_eq!(options[0].forms, ["-Ldir"]);
    assert_eq!(options[1].aliases, ["-n"]);
    assert_eq!(options[1].forms, ["-n/-NUM"]);
    assert_eq!(options[2].aliases, ["-a", "-all"]);
    assert_eq!(options[3].aliases, ["-Wall"]);
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
    assert_eq!(commands[0].aliases, ["launch"]);
    assert_eq!(commands[0].forms, ["launch -p [-x]"]);
    assert_eq!(commands[1].aliases, ["["]);
}
