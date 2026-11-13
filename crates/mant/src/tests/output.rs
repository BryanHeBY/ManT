//! Output behavior contracts.

use super::*;
#[test]
fn markdown_checks_untrusted_heading_controls_without_mutating_input() {
    let mut document = semantic_markdown();
    document.heading = Some("ris\u{1b}c".into());
    let host = FakeHost {
        document: Some(document),
        ..FakeHost::new()
    };
    let path = "input.md";
    let arguments = ["--input", path, "--format", "markdown"];

    let (status, redirected, diagnostics) = invoke(&arguments, b"", &host);
    assert_eq!(status, 0);
    assert_eq!(diagnostics.len(), 0);
    // Markdown's checked hand-IR projection masks unsafe content before
    // serialization, including redirected output. JSON retains the input IR.
    assert!(!redirected.contains('\u{1b}'));
    assert!(redirected.contains("ris c"));
    let (status, json, diagnostics) = invoke(&["--input", path, "--format", "json"], b"", &host);
    assert_eq!(status, 0);
    assert_eq!(diagnostics.len(), 0);
    assert!(json.contains("\\u001b"));
    assert_eq!(
        host.document
            .as_ref()
            .unwrap()
            .heading
            .as_ref()
            .unwrap()
            .plain_text(),
        "ris\u{1b}c"
    );

    let (status, terminal, diagnostics) = invoke_with_terminal_output(&arguments, b"", &host);
    assert_eq!(status, 0);
    assert_eq!(diagnostics.len(), 0);
    assert!(!terminal.contains('\u{1b}'));
    assert!(terminal.contains("ris�c"));
}

#[test]
fn mathematical_markdown_accepts_full_partial_and_scope_json() {
    for input in [
        br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"git"},"view":{"kind":"outline"}}"#.as_slice(),
        br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"git"},"view":{"kind":"excerpt","selectors":[{"kind":"path","path":"1"}]}}"#.as_slice(),
    ] {
        let host = FakeHost::with_manual_and_tldr();
        let (status, output, diagnostics) = invoke(
            &["--request-json", "--markdown-math", "latex"], input, &host,
        );
        assert_eq!(status, 0, "{diagnostics}");
        assert_ne!(output, "");
        assert_eq!(diagnostics, "");
        assert_eq!(host.query_calls.get(), 1);
    }
    #[cfg(feature = "roff")]
    {
        let host = FakeHost::with_manual_and_tldr();
        let (status, output, diagnostics) =
            invoke(&["git", "--tldr", "--markdown-math", "latex"], &[], &host);
        assert_eq!(status, 0, "{diagnostics}");
        assert_ne!(output, "");
    }
    let host = FakeHost::new();
    let (status, _, diagnostics) = invoke(
        &["--request-json", "--markdown-math", "latex"],
        br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"git"},"view":{"kind":"full"}}"#,
        &host,
    );
    assert_eq!(status, 0, "{diagnostics}");
    assert!(diagnostics.is_empty(), "{diagnostics}");
    assert_eq!(host.query_calls.get(), 1);
    let (status, _, diagnostics) = invoke(
        &["--request-json", "--markdown-math", "readable"],
        br#"{"schema":"mant.scope-request/v0.14","scope":{"documents":[{"selector":"git"}]},"view":{"kind":"search","pattern":"x"}}"#,
        &host,
    );
    assert_ne!(status, 0);
    assert!(
        diagnostics.contains("document scope queries are unavailable"),
        "{diagnostics}"
    );
}

#[test]
fn markdown_is_clean_by_default_and_preserves_anchors_on_request() {
    let host = FakeHost::with_manual();
    let (status, output, diagnostics) = invoke(&["demo", "--format", "markdown"], b"", &host);
    assert_eq!(status, 0);
    assert!(!output.contains("<a "));
    assert_eq!(diagnostics.len(), 0);

    let (status, output, diagnostics) = invoke(&["demo", "--preserve-anchors"], b"", &host);
    assert_eq!(status, 0);
    assert!(output.contains("<a id=\"name-1\"></a>"));
    assert!(output.contains("<a id=\"options-2\"></a>"));
    assert_eq!(diagnostics.len(), 0);
}

#[test]
fn man_format_rejects_a_tldr_only_result() {
    let host = FakeHost::with_tldr();
    let (status, output, diagnostics) = invoke(&["demo", "--format", "man"], b"", &host);

    assert_eq!(status, 1);
    assert_eq!(output.len(), 0);
    assert_eq!(
        diagnostics,
        "mant: manual page is unavailable; --format man cannot render tldr-only content\n"
    );
}

#[test]
fn mathml_cli_selection_reaches_documents_excerpts_and_explanations() {
    use mant_ir::{
        EquationAtomRole, EquationExpression, EquationFont, EquationKind, EquationOperandScope,
        EquationOperator,
    };
    let expression = EquationExpression {
        kind: EquationKind::Text,
        font: EquationFont::None,
        operator: EquationOperator::None,
        atom_role: EquationAtomRole::Identifier,
        operand_scope: EquationOperandScope::Atomic,
        text: Some("x".into()),
        left: None,
        right: None,
        top: None,
        bottom: None,
        children: Vec::new(),
    };
    let mut document = explainable_manual();
    let Block::DefinitionList { items, .. } = &mut document.sections[1].blocks[1] else {
        panic!("explainable entry");
    };
    let Block::Paragraph { children, .. } = &mut items[0].description[0] else {
        panic!("entry description");
    };
    children.push(Inline::Equation {
        value: expression.readable_text(),
        expression,
    });
    let host = FakeHost {
        document: Some(document),
        ..FakeHost::new()
    };
    for arguments in [
        vec!["demo", "--markdown-math", "mathml"],
        vec!["demo", "--node", "2", "--markdown-math", "mathml"],
        vec![
            "demo",
            "--explain",
            "--exclude",
            "--markdown-math",
            "mathml",
        ],
    ] {
        let (status, output, diagnostics) = invoke(&arguments, b"", &host);
        assert_eq!(status, 0, "{arguments:?}: {diagnostics}");
        assert_eq!(diagnostics, "");
        assert!(
            output.contains("<math xmlns=\"http://www.w3.org/1998/Math/MathML\""),
            "{output}"
        );
        assert!(output.contains("<mi>x</mi>"), "{output}");
    }
    let input = br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"demo"},"view":{"kind":"full"}}"#;
    let (status, output, diagnostics) = invoke(
        &["--request-json", "--markdown-math", "mathml"],
        input,
        &host,
    );
    assert_eq!(status, 0, "{diagnostics}");
    assert!(output.contains("<mi>x</mi>"));
    assert_eq!(host.document.as_ref().unwrap().sections.len(), 2);
}
