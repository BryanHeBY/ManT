//! Source-topology regressions from the layout/semantic-entry review.
use super::*;

#[test]
fn environment_assignments_retain_punctuation_in_values() {
    for form in ["FOO=one,two", "FOO=one|two"] {
        let parsed = parse_markdown(&format!("# Tool\n\n<!-- mant:entries role=environment-variable case=sensitive -->\n- `{form}`: Set values.\n"), None).unwrap();
        let index = mant_ir::SemanticIndex::build(&parsed.document);
        assert_eq!(index.root()[0].aliases, ["FOO"]);
        assert_eq!(index.root()[0].forms, [form]);
    }
}

#[test]
fn removing_directives_never_merges_independent_lists_or_roles() {
    for newline in ["\n", "\r\n"] {
        for first in [
            "<!-- mant:entries role=command case=sensitive -->\n- `run`: Run.",
            "- ordinary",
        ] {
            for marker in ["-", "*"] {
                let source = format!("# Tool\n\n{first}\n\n<!-- mant:entries role=value case=insensitive -->\n{marker} `auto`: Automatic.\n").replace('\n', newline);
                let parsed = parse_markdown(&source, None).unwrap();
                assert!(
                    parsed.document.diagnostics.is_empty(),
                    "{source}: {:?}",
                    parsed.document.diagnostics
                );
                assert_eq!(parsed.document.blocks.len(), 2);
                let index = mant_ir::SemanticIndex::build(&parsed.document);
                let entries = index.root();
                assert_eq!(entries.last().unwrap().kind, EntryKind::Value);
                assert_eq!(entries.last().unwrap().aliases, ["auto"]);
            }
        }
        let source = "# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n- `--color WHEN`: Color.\n\n  <!-- mant:domain choices=exhaustive -->\n\n  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: Automatic.\n\n  <!-- mant:entries role=command case=sensitive -->\n  - `run`: A command.\n".replace('\n', newline);
        let parsed = parse_markdown(&source, None).unwrap();
        let index = mant_ir::SemanticIndex::build(&parsed.document);
        let entry = &index.root()[0];
        assert_eq!(entry.children[0].kind, EntryKind::Value);
        assert_eq!(entry.children[1].kind, EntryKind::Command);
        assert!(!matches!(
            entry.value_domain,
            Some(mant_ir::ValueDomain::Choices { exhaustive: true })
        ));
        assert!(
            parsed
                .document
                .diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("markdown.semantic-value-domain"))
        );
    }
}
