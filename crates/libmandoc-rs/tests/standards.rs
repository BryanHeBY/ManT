//! Deliberate upstream standard-name compatibility and its portable migration.

use libmandoc_rs::{DiagnosticLevel, Node, Parser};

fn standard_source(specifier: &str) -> String {
    format!(
        ".Dd September 11, 2026\n.Dt STANDARDS 7\n.Os ManT\n\
.Sh NAME\n.Nm standards\n.Nd standard name compatibility\n\
.Sh STANDARDS\n.St {specifier}\n"
    )
}

fn tree_text(node: &Node, output: &mut String) {
    if let Some(text) = &node.text {
        output.push_str(text);
    }
    for child in &node.children {
        tree_text(child, output);
    }
}

#[test]
fn removed_xsh42_alias_reports_an_error_without_synthesizing_old_text() {
    // Upstream st.c revision 1.17 intentionally removed this nonportable alias.
    let report = Parser::default()
        .parse_bytes("standards.7", standard_source("-xsh4.2").as_bytes())
        .expect("an unsupported standard remains a recoverable native diagnostic");

    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.level == DiagnosticLevel::Error
            && diagnostic.message == "unknown standard specifier: St -xsh4.2"
            && diagnostic.location.map(|location| location.line) == Some(8)
    }));
    let mut text = String::new();
    tree_text(&report.document.root, &mut text);
    assert!(!text.contains("XSH4.2"));
    assert!(!text.contains("X/Open System Interfaces and Headers"));
}

#[test]
fn portable_xpg42_replacement_expands_without_unknown_standard_diagnostic() {
    let report = Parser::default()
        .parse_bytes("standards.7", standard_source("-xpg4.2").as_bytes())
        .expect("parse the portable replacement standard name");

    assert!(
        !report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("unknown standard specifier"))
    );
    let mut text = String::new();
    tree_text(&report.document.root, &mut text);
    assert!(text.contains("X/Open Portability Guide"));
    assert!(text.contains("XPG4.2"));
}
