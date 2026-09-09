//! Markdown import combined with public query and presentation contracts.
use super::*;

#[test]
fn slash_option_aliases_preserve_forms_and_query_identity() {
    for declaration in ["", "<!-- mant:entries role=option case=sensitive -->\n"] {
        for form in ["-h/--help", "-h, --help", "-h|--help"] {
            let source = format!("# Probe\n\n{declaration}- `{form}`: Help payload.\n");
            let query = crate::load_markdown_text(&source, None).unwrap();
            let index = mant_ir::SemanticIndex::build(query.document.as_ref().unwrap());
            assert_eq!(index.root()[0].names, ["-h", "--help"], "{source}");
            assert_eq!(index.root()[0].forms, [form]);
            let outline =
                crate::build_outline_projection(&query, EntryProjection::All, None).unwrap();
            let OutlineNode::DocumentRoot { children, .. } = &outline.nodes[0] else {
                panic!("{outline:?}")
            };
            let OutlineNode::DocumentEntry { names, forms, .. } = &children[0] else {
                panic!("{children:?}")
            };
            assert_eq!(names, &["-h", "--help"]);
            assert_eq!(forms, &[form]);
            let short = crate::semantic_test_read::semantic_excerpt(&query, &["-h"]).unwrap();
            let long = crate::semantic_test_read::semantic_excerpt(&query, &["--help"]).unwrap();
            assert_eq!(short.selections, long.selections);
        }
    }
    for (declaration, form, names) in [
        ("", "--output /tmp/--help", vec!["--output"]),
        ("", "--output=path/--help", vec!["--output"]),
        (
            "<!-- mant:entries role=command case=sensitive -->\n",
            "create/remove",
            vec!["create/remove"],
        ),
        (
            "<!-- mant:entries role=environment-variable case=sensitive -->\n",
            "PATH=/tmp/--help",
            vec!["PATH"],
        ),
        (
            "<!-- mant:entries role=option case=sensitive -->\n",
            "/help",
            vec!["/help"],
        ),
    ] {
        let query = crate::load_markdown_text(
            &format!("# Probe\n\n{declaration}- `{form}`: Payload.\n"),
            None,
        )
        .unwrap();
        let index = mant_ir::SemanticIndex::build(query.document.as_ref().unwrap());
        assert_eq!(index.root()[0].names, names, "{form}");
        assert_eq!(index.root()[0].forms, [form]);
        assert!(crate::semantic_test_read::semantic_excerpt(&query, &["--help"]).is_err());
    }
}
