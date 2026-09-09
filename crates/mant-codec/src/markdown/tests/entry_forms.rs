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
