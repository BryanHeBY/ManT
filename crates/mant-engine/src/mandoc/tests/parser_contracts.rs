use super::*;

#[test]
fn byte_codec_never_opens_the_source_label_or_embedded_include() {
    let path = temporary_source("codec-no-io", ".TH ONDISK 1\n.SH NAME\nDISK_SECRET\n");
    let source = format!(
        ".TH INMEMORY 1\n.SH NAME\nBEFORE\n.so {}\nAFTER\n",
        path.to_string_lossy()
    );
    let result = super::super::parse_plain_manual_report(&path, source.as_bytes());
    fs::remove_file(&path).expect("remove inaccessible input witness");
    let (document, native) = result.expect("embedded include is a recoverable denied request");
    assert_eq!(document.meta.title.as_deref(), Some("INMEMORY"));
    assert_eq!(native.document.metadata.title.as_deref(), Some("INMEMORY"));
    assert_eq!(
        document.source.path.as_deref(),
        Some(path.to_string_lossy().as_ref())
    );
    let text = visible_document_text(&document);
    assert!(text.contains("BEFORE") && text.contains("AFTER"));
    assert!(!text.contains("DISK_SECRET"));
    assert!(
        document
            .diagnostics
            .iter()
            .any(|finding| finding.message.contains(".so"))
    );
}

#[test]
fn diagnoses_future_structural_macros_before_discarding_visible_parts() {
    let mut report = Parser::default()
        .parse_bytes(
            "future-structure.1",
            b".Dd August 17, 2026\n.Dt FUTURE 1\n.Os\n.Sh SYNOPSIS\n\
.Fo future_call\n.Fa argument\n.Fc\n",
        )
        .expect("parse structural fixture");
    let block = find_macro_mut(&mut report.document.root, "Fo").expect("Fo block");
    block.macro_name = Some("FutureBlock".to_owned());
    let mut second_body = block
        .children
        .iter()
        .find(|child| child.kind == libmandoc_rs::NodeKind::Body)
        .cloned()
        .expect("function body");
    assert!(replace_first_text(&mut second_body, "second_argument"));
    block.children.push(second_body);

    let document = lower_mandoc_document(std::path::Path::new("future-structure.1"), &report);

    assert!(document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("manual.unhandled-structural-parts")
            && diagnostic.message.contains("FutureBlock")
    }));
    let rendered = document.sections[0]
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { children, .. } => inline_text(children),
            block => panic!("expected fallback paragraph, got {block:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(rendered, ["argument", "second_argument"]);
}

#[test]
fn turns_captured_parser_findings_into_structured_diagnostics() {
    let path = temporary_source(
        "unsupported",
        ".Dd July 19, 2026\n.Dt BAD 1\n.Os\n.Sh NAME\n.Nm bad\n.ab\n",
    );

    let document = parse_manual_source(&path).expect("best-effort parse");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert!(
        document
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.level == DiagnosticLevel::Unsupported)
    );
}

#[test]
fn masks_terminal_controls_before_native_parsing() {
    let path = temporary_source("controls", ".TH SAFE 1\n.SH NAME\nsafe \x1b[2J text\n");

    let document = parse_manual_source(&path).expect("parse sanitized manual");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert!(
        document
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code.as_deref() == Some("manual.control-characters") })
    );
}
