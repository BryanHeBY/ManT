//! Declared and inferred Markdown entry forms preserve invocation semantics.
use super::*;

#[test]
fn environment_assignments_retain_punctuation_in_values() {
    for form in ["FOO=one,two", "FOO=one|two"] {
        let parsed = parse_markdown(&format!("# Tool\n\n<!-- mant:entries role=environment-variable case=sensitive -->\n- `{form}`: Set values.\n"), None).unwrap();
        let index = mant_ir::SemanticIndex::build(&parsed.document);
        assert_eq!(index.root()[0].names, ["FOO"]);
        assert_eq!(index.root()[0].forms, [form]);
    }
}

#[test]
fn inferred_literal_option_arguments_do_not_create_names() {
    for form in [
        "--number -10,--fake,20",
        "--number -10%,--fake,20",
        "--number -10:20,--fake,30",
        "--pattern \"one, --fake,two\"",
    ] {
        let parsed = parse_markdown(&format!("# Tool\n\n- `{form}`: Description.\n"), None)
            .expect("parse inferred Markdown option");
        let index = mant_ir::SemanticIndex::build(&parsed.document);
        assert_eq!(index.root().len(), 1, "{form}");
        assert_eq!(
            index.root()[0].names,
            [if form.starts_with("--pattern") {
                "--pattern"
            } else {
                "--number"
            }],
            "{form}"
        );
    }
}
