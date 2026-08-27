//! Versioned storage-independent AST and diagnostic projections for L2.
//!
//! The projection deliberately represents only the contract enumerated in
//! The projection never serializes arena IDs, allocation order, or diagnostic
//! prose.

use std::collections::BTreeSet;

use mantdoc::{Document, NodeRef, ParseReport, SourcePosition, SourceSpan};
use serde::Serialize;

/// Stable schema identifier for [`CanonicalParse`].
pub const CANONICAL_AST_SCHEMA: &str = "mantdoc.canonical-ast/v1";
/// Stable schema identifier for [`CanonicalDiagnostic`].
pub const CANONICAL_DIAGNOSTIC_SCHEMA: &str = "mantdoc.canonical-diagnostic/v1";
/// Pinned replacement for host-derived bare mdoc `.Os` values in L2 runs.
pub const CANONICAL_MDOC_OPERATING_SYSTEM: &str = "mantdoc canonical differential";

/// Complete canonical parser result for one backend.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CanonicalParse {
    /// Schema for the tree portion of this record.
    pub ast_schema: &'static str,
    /// Schema for the diagnostic portion of this record.
    pub diagnostic_schema: &'static str,
    /// Parser-owned logical document.
    pub document: CanonicalDocument,
    /// Findings in source order, with prose intentionally excluded.
    pub diagnostics: Vec<CanonicalDiagnostic>,
}

/// Storage-independent document header and preorder node list.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CanonicalDocument {
    /// Selected macro package.
    pub macro_set: String,
    /// Normalized document metadata.
    pub metadata: CanonicalMetadata,
    /// Nodes in preorder; each has a sibling-index path from the root.
    pub nodes: Vec<CanonicalNode>,
}

/// Metadata fields common to both parser implementations.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CanonicalMetadata {
    /// Canonical title.
    pub title: Option<String>,
    /// Manual section.
    pub section: Option<String>,
    /// Manual volume.
    pub volume: Option<String>,
    /// Declared operating system.
    pub os: Option<String>,
    /// Declared architecture.
    pub arch: Option<String>,
    /// NAME-section primary name.
    pub name: Option<String>,
    /// Normalized document date.
    pub date: Option<String>,
    /// Alias target for a redirect page.
    pub alias_target: Option<String>,
    /// Whether a visible body was produced.
    pub has_body: bool,
}

/// One canonical node, independent of backing-tree representation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CanonicalNode {
    /// Sibling-index path from the synthetic root (the root is `[]`).
    pub path: Vec<u32>,
    /// Renderer-neutral node role.
    pub kind: String,
    /// Request or macro name without a leading dot.
    pub macro_name: Option<String>,
    /// Normalized visible text.
    pub text: Option<String>,
    /// Validated same-document target.
    pub tag: Option<String>,
    /// Comparable one-based source position, when available.
    pub location: Option<CanonicalLocation>,
    /// Lowering-relevant source and rendering flags.
    pub flags: CanonicalFlags,
    /// Normalized list behavior.
    pub list_kind: Option<String>,
    /// Normalized display fill behavior.
    pub display_kind: Option<String>,
    /// Normalized font behavior.
    pub font: Option<String>,
    /// Normalized author behavior.
    pub author_mode: Option<String>,
    /// Resolved mdoc enclosure state.
    pub enclosure: Option<CanonicalEnclosure>,
    /// Whether the enclosing list is compact.
    pub compact: bool,
    /// Normalized layout offset.
    pub offset: Option<String>,
    /// Normalized layout width.
    pub width: Option<String>,
    /// tbl payload when this is a table row.
    pub table_cells: Vec<CanonicalTableCell>,
    /// Flattened eqn payload when this is an equation node.
    pub equation: Option<String>,
    /// Direct-child count, independent of node IDs.
    pub child_count: usize,
}

/// One comparable source position.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CanonicalLocation {
    /// One-based physical source line.
    pub line: u32,
    /// One-based byte column.
    pub column: u32,
}

/// Node flags shared by the two public ASTs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct CanonicalFlags {
    /// Generated rather than source-authored.
    pub generated: bool,
    /// Ends a sentence.
    pub sentence_end: bool,
    /// Does not contribute visible output.
    pub no_print: bool,
    /// Is in a no-fill region.
    pub no_fill: bool,
    /// Is a validated destination.
    pub deep_link_target: bool,
    /// Renders a self-link.
    pub permalink: bool,
    /// Begins a roff input line.
    pub line_start: bool,
    /// Suppresses spacing after opening punctuation.
    pub delimiter_open: bool,
    /// Suppresses spacing before closing punctuation.
    pub delimiter_close: bool,
    /// Ends with a roff line continuation.
    pub line_continuation: bool,
    /// Uses synopsis presentation semantics.
    pub synopsis_pretty: bool,
}

/// Resolved mdoc enclosure delimiters.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CanonicalEnclosure {
    /// Opening delimiter.
    pub opening: String,
    /// Optional closing delimiter.
    pub closing: Option<String>,
}

/// One canonical tbl cell.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CanonicalTableCell {
    /// Visible content, if any.
    pub text: Option<String>,
    /// Whether source used a `T{` text block.
    pub text_block: bool,
    /// Whether this continues a vertical span.
    pub vertical_continuation: bool,
    /// Logical column span.
    pub column_span: u16,
    /// Logical row span.
    pub row_span: u16,
    /// Requested horizontal alignment.
    pub alignment: String,
}

/// One canonical diagnostic without unstable prose.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CanonicalDiagnostic {
    /// Source-order ordinal.
    pub ordinal: usize,
    /// Stable broad severity.
    pub severity: String,
    /// Comparable primary location, when available.
    pub location: Option<CanonicalLocation>,
    /// Native typed diagnostic code.
    pub code: String,
}

/// First canonical mismatch, retained for focused regression diagnostics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CanonicalDifference {
    /// RFC 6901 JSON Pointer into the canonical record.
    pub pointer: String,
    /// Left-hand value at this pointer.
    pub expected: serde_json::Value,
    /// Right-hand value at this pointer.
    pub actual: serde_json::Value,
}

/// Canonicalize one native parser report.
#[must_use]
pub fn canonicalize_mantdoc(report: &ParseReport) -> CanonicalParse {
    CanonicalParse {
        ast_schema: CANONICAL_AST_SCHEMA,
        diagnostic_schema: CANONICAL_DIAGNOSTIC_SCHEMA,
        document: canonical_document_mantdoc(&report.document),
        diagnostics: report
            .diagnostics
            .iter()
            .enumerate()
            .map(|(ordinal, diagnostic)| CanonicalDiagnostic {
                ordinal,
                severity: enum_name(diagnostic.severity),
                location: diagnostic
                    .primary
                    .as_ref()
                    .and_then(|span| native_location(&report.document, span)),
                code: diagnostic.code.to_string(),
            })
            .collect(),
    }
}

#[allow(clippy::too_many_lines)] // The explicit compatibility map is intentionally auditable.
fn native_compatibility_codes(code: &str) -> (Option<String>, Option<String>) {
    // The wrapper added these two findings after it copied C-owned data.  They
    // have a stable legacy identifier and therefore occupy that field in the
    // common schema, while the native report still exposes its typed code.
    match code {
        mantdoc::DiagnosticCode::LEGACY_SYNTAX_TREE_DEPTH_LIMIT => {
            return (Some("SyntaxTreeDepthLimit".to_owned()), None);
        }
        mantdoc::DiagnosticCode::LEGACY_EQUATION_TREE_DEPTH_LIMIT => {
            return (Some("EquationTreeDepthLimit".to_owned()), None);
        }
        _ => {}
    }

    // The frozen pre-native report shape exposes these upstream findings
    // without a stable code.
    // The native report retains its typed identity; the canonical adapter
    // deliberately projects only the shared legacy observable shape.
    let native_code = (!matches!(
        code,
        "man.title-not-uppercase"
            | "man.title-date-unparseable"
            | "man.title-date-missing"
            | "man.title-missing"
            | "man.title-section-missing"
            | "man.no-document-body"
            | "man.empty-paragraph"
            | "man.excess-arguments"
            | "man.missing-resource"
            | "man.missing-option"
            | "man.all-arguments"
            | "man.fewer-indents"
            | "man.empty-block"
            | "man.redundant-fill-mode"
            | "man.unmatched-close"
            | "man.unclosed-block"
            | "man.line-scope-broken"
            | "man.blank-line-scope"
            | "escape.invalid"
            | "escape.unknown"
            | "escape.unterminated"
            | "escape.unsupported-unicode"
            | "escape.unknown-special-character"
            | "roff.excess-arguments"
            | "roff.unknown-font"
            | "roff.escaped-name"
            | "roff.division-by-zero"
            | "roff.condition"
            | "roff.shift"
            | "roff.undefined-reference"
            | "roff.non-numeric-argument"
            | "roff.invalid-character-argument"
            | "roff.unmatched-end"
            | "roff.unterminated-scope"
            | "roff.unclosed-ignore-block"
            | "roff.while-inner-scope"
            | "roff.while-out-of-scope"
            | "roff.while-cannot-continue"
            | "roff.while-nested"
            | "roff.unknown-macro"
            | "roff.return-outside-macro"
            | "roff.macro-argument-outside"
            | "arguments.unterminated-quote"
            | "roff.empty-request"
            | "roff.odd-translation"
            | "roff.all-arguments"
            | "input.invalid-byte"
            | "input.trailing-whitespace"
            | "input.bad-comment-style"
            | "input.line-too-long"
            | "input.tab-in-filled-text"
            | "input.blank-line-in-filled-text"
            | "limits.expansion-steps"
            | "tbl.empty-layout"
            | "tbl.unclosed-text-block"
            | "tbl.unknown-font"
            | "tbl.leading-span"
            | "tbl.macro"
            | "tbl.extra-data-cells"
            | "tbl.no-data"
            | "tbl.vertical-bar"
            | "tbl.leading-down"
            | "tbl.spanned-data"
            | "tbl.option-argument"
            | "tbl.option-argument-size"
            | "tbl.option-character"
            | "tbl.unknown-option"
            | "tbl.excessive-spacing"
            | "tbl.eqn-delimiter-option"
            | "mdoc.arguments"
            | "mdoc.unmatched-close"
            | "mdoc.unclosed-block"
            | "mdoc.empty-block"
            | "mdoc.empty-list-item"
            | "mdoc.content-outside-list"
            | "mdoc.empty-macro"
            | "mdoc.invalid-tag"
            | "mdoc.non-callable-macro"
            | "mdoc.duplicate-argument"
            | "mdoc.unknown-at-version"
            | "mdoc.empty-argument"
            | "mdoc.missing-display-type"
            | "mdoc.duplicate-display-type"
            | "mdoc.unsupported-display-file"
            | "mdoc.display-without-arguments"
            | "mdoc.nested-display"
            | "mdoc.broken-block"
            | "mdoc.missing-font-type"
            | "mdoc.unknown-font-type"
            | "mdoc.obsolete"
            | "mdoc.duplicate-prologue"
            | "mdoc.operating-system-explicit"
            | "mdoc.mdocdate-found"
            | "mdoc.rcs-id-missing"
            | "mdoc.late-operating-system"
            | "mdoc.operating-system-missing"
            | "mdoc.prefix-without-following"
            | "mdoc.useless-macro"
            | "mdoc.late-title"
            | "mdoc.title-not-uppercase"
            | "mdoc.title-section-unknown"
            | "mdoc.title-section-missing"
            | "mdoc.date-missing"
            | "mdoc.date-unparseable"
            | "mdoc.date-legacy"
            | "mdoc.late-prologue"
            | "mdoc.prologue-order"
            | "mdoc.title-missing"
            | "mdoc.no-document-body"
            | "mdoc.name-missing"
            | "mdoc.function-name-missing"
            | "mdoc.exit-name-missing"
            | "mdoc.standard-selector-missing"
            | "mdoc.content-before-section"
            | "mdoc.unexpected-section"
            | "mdoc.duplicate-section"
            | "mdoc.section-order"
            | "mdoc.paragraph-before-block"
            | "mdoc.paragraph-moved-out-of-list"
            | "mdoc.badly-nested-block"
            | "mdoc.item-outside-list"
            | "mdoc.column-outside-list"
            | "mdoc.trailing-delimiter-spacing"
            | "mdoc.trailing-delimiter"
            | "mdoc.no-space-macro"
            | "mdoc.boolean-argument"
            | "mdoc.reference-content"
            | "mdoc.empty-reference-block"
            | "mdoc.first-section-not-name"
            | "mdoc.reference-section-missing"
            | "mdoc.description-outside-name"
            | "mdoc.description-missing"
            | "mdoc.name-section-content"
            | "mdoc.name-section-comma-missing"
            | "mdoc.name-section-name-missing"
            | "mdoc.name-section-description-missing"
            | "mdoc.name-section-description-not-last"
            | "mdoc.authors-missing"
            | "mdoc.function-name-parenthesis"
            | "mdoc.function-argument-comma"
            | "mdoc.unknown-library"
            | "mdoc.unknown-standard"
            | "eqn.recursive-definition"
            | "eqn.empty-request"
            | "eqn.missing-box"
    ))
    .then(|| code.to_owned());
    (None, native_code)
}

/// Find the first schema-ordered mismatch between two canonical results.
#[must_use]
pub fn first_difference(
    legacy: &CanonicalParse,
    native: &CanonicalParse,
) -> Option<CanonicalDifference> {
    let legacy = serde_json::to_value(legacy).ok()?;
    let native = serde_json::to_value(native).ok()?;
    first_json_difference(&legacy, &native, String::new())
}

fn canonical_document_mantdoc(document: &Document) -> CanonicalDocument {
    let metadata = document.metadata();
    let mut nodes = Vec::with_capacity(document.node_count());
    let root = document
        .node(document.root())
        .expect("document always stores a synthetic root");
    let mut pending = vec![(root, Vec::new())];
    while let Some((node, path)) = pending.pop() {
        let children = node.children().collect::<Vec<_>>();
        nodes.push(canonical_node_mantdoc(
            document,
            node,
            path.clone(),
            children.len(),
        ));
        pending.extend(
            children
                .into_iter()
                .enumerate()
                .rev()
                .map(|(index, child)| {
                    let mut child_path = path.clone();
                    child_path.push(u32::try_from(index).expect("node child count fits u32"));
                    (child, child_path)
                }),
        );
    }
    CanonicalDocument {
        macro_set: enum_name(document.macro_set()),
        metadata: CanonicalMetadata {
            title: option_string(metadata.title.as_deref()),
            section: option_string(metadata.section.as_deref()),
            volume: option_string(metadata.volume.as_deref()),
            os: option_string(metadata.os.as_deref()),
            arch: option_string(metadata.arch.as_deref()),
            name: option_string(metadata.name.as_deref()),
            date: option_string(metadata.date.as_deref()),
            alias_target: option_string(metadata.alias_target.as_deref()),
            has_body: metadata.has_body,
        },
        nodes,
    }
}

fn canonical_node_mantdoc(
    document: &Document,
    node: NodeRef<'_>,
    path: Vec<u32>,
    child_count: usize,
) -> CanonicalNode {
    let flags = node.flags();
    CanonicalNode {
        path,
        kind: enum_name(node.kind()),
        macro_name: option_string(node.macro_name()),
        text: option_string(node.text()),
        tag: option_string(node.tag()),
        location: node
            .location()
            .and_then(|span| native_location(document, span)),
        flags: CanonicalFlags {
            generated: flags.generated,
            sentence_end: flags.sentence_end,
            no_print: flags.no_print,
            no_fill: flags.no_fill,
            deep_link_target: flags.deep_link_target,
            permalink: flags.permalink,
            line_start: flags.line_start,
            delimiter_open: flags.delimiter_open,
            delimiter_close: flags.delimiter_close,
            line_continuation: flags.line_continuation,
            synopsis_pretty: flags.synopsis_pretty,
        },
        list_kind: node.list_kind().map(enum_name),
        display_kind: node.display_kind().map(enum_name),
        font: node.font().map(enum_name),
        author_mode: node.author_mode().map(enum_name),
        enclosure: node.enclosure().map(|enclosure| CanonicalEnclosure {
            opening: enclosure.opening.to_string(),
            closing: option_string(enclosure.closing.as_deref()),
        }),
        compact: node.compact(),
        offset: option_string(node.offset()),
        width: option_string(node.width()),
        table_cells: node
            .table_cells()
            .iter()
            .map(|cell| CanonicalTableCell {
                text: option_string(cell.text.as_deref()),
                text_block: cell.text_block,
                vertical_continuation: cell.vertical_continuation,
                column_span: cell.column_span,
                row_span: cell.row_span,
                alignment: enum_name(cell.alignment),
            })
            .collect(),
        equation: option_string(node.equation()),
        child_count,
    }
}

fn native_location(document: &Document, span: &SourceSpan) -> Option<CanonicalLocation> {
    document
        .source_position(span)
        .map(canonical_source_position)
}

const fn canonical_source_position(position: SourcePosition) -> CanonicalLocation {
    CanonicalLocation {
        line: position.line,
        column: position.column,
    }
}

fn option_string(value: Option<&str>) -> Option<String> {
    value.map(str::to_owned)
}

fn enum_name(value: impl std::fmt::Debug) -> String {
    format!("{value:?}")
}

fn first_json_difference(
    legacy: &serde_json::Value,
    native: &serde_json::Value,
    pointer: String,
) -> Option<CanonicalDifference> {
    match (legacy, native) {
        (serde_json::Value::Object(legacy), serde_json::Value::Object(native)) => {
            let keys = legacy
                .keys()
                .chain(native.keys())
                .cloned()
                .collect::<BTreeSet<_>>();
            for key in keys {
                let path = format!("{pointer}/{}", escape_json_pointer(&key));
                match (legacy.get(&key), native.get(&key)) {
                    (Some(legacy), Some(native)) => {
                        if let Some(difference) = first_json_difference(legacy, native, path) {
                            return Some(difference);
                        }
                    }
                    (legacy, native) => {
                        return Some(CanonicalDifference {
                            pointer: path,
                            expected: legacy.cloned().unwrap_or(serde_json::Value::Null),
                            actual: native.cloned().unwrap_or(serde_json::Value::Null),
                        });
                    }
                }
            }
            None
        }
        (serde_json::Value::Array(legacy), serde_json::Value::Array(native)) => {
            for index in 0..legacy.len().max(native.len()) {
                let path = format!("{pointer}/{index}");
                match (legacy.get(index), native.get(index)) {
                    (Some(legacy), Some(native)) => {
                        if let Some(difference) = first_json_difference(legacy, native, path) {
                            return Some(difference);
                        }
                    }
                    (legacy, native) => {
                        return Some(CanonicalDifference {
                            pointer: path,
                            expected: legacy.cloned().unwrap_or(serde_json::Value::Null),
                            actual: native.cloned().unwrap_or(serde_json::Value::Null),
                        });
                    }
                }
            }
            None
        }
        _ if legacy == native => None,
        _ => Some(CanonicalDifference {
            pointer,
            expected: legacy.clone(),
            actual: native.clone(),
        }),
    }
}

fn escape_json_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_mantdoc, first_difference};

    #[test]
    fn native_projection_uses_paths_and_excludes_diagnostic_prose() {
        let name = mantdoc::SourceName::new("canonical.1").unwrap();
        let report = mantdoc::Parser::default()
            .parse(mantdoc::Source::new(
                &name,
                b".TH CANONICAL 1\n.SH NAME\ncanonical \\- projection\n",
            ))
            .unwrap();
        let canonical = canonicalize_mantdoc(&report);
        assert_eq!(canonical.ast_schema, super::CANONICAL_AST_SCHEMA);
        assert_eq!(canonical.document.nodes[0].path, Vec::<u32>::new());
        assert_eq!(canonical.document.nodes[0].kind, "Root");
        assert!(first_difference(&canonical, &canonical).is_none());
    }

    #[test]
    fn code_less_legacy_escape_findings_hide_only_the_canonical_native_code() {
        assert_eq!(
            super::native_compatibility_codes("escape.unknown"),
            (None, None)
        );
        assert_eq!(
            super::native_compatibility_codes("mdoc.duplicate-prologue"),
            (None, None)
        );
        assert_eq!(
            super::native_compatibility_codes("mdoc.late-title"),
            (None, None)
        );
        assert_eq!(
            super::native_compatibility_codes("mdoc.title-not-uppercase"),
            (None, None)
        );
        assert_eq!(
            super::native_compatibility_codes("mdoc.title-section-unknown"),
            (None, None)
        );
        assert_eq!(
            super::native_compatibility_codes("mdoc.date-unparseable"),
            (None, None)
        );
        assert_eq!(
            super::native_compatibility_codes("mdoc.prologue-order"),
            (None, None)
        );
        assert_eq!(
            super::native_compatibility_codes("mdoc.title-missing"),
            (None, None)
        );
        assert_eq!(
            super::native_compatibility_codes("mdoc.name-missing"),
            (None, None)
        );
        assert_eq!(
            super::native_compatibility_codes("mdoc.exit-name-missing"),
            (None, None)
        );
    }

    #[test]
    fn depth_boundary_keeps_a_finite_native_prefix() {
        let name = mantdoc::SourceName::new("depth-boundary.1").unwrap();
        let mut source = String::from(".TH DEPTH-BOUNDARY 1\n.SH BODY\n");
        for _ in 0..300 {
            source.push_str(".RS\n");
        }
        source.push_str("retained prefix\n");
        for _ in 0..300 {
            source.push_str(".RE\n");
        }

        let native = mantdoc::Parser::default()
            .parse(mantdoc::Source::new(&name, source.as_bytes()))
            .unwrap();
        assert_eq!(native_document_depth(&native.document), 256);
        assert_eq!(
            native.document.node_count(),
            native.document.preorder().count()
        );
        assert!(native.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_str() == mantdoc::DiagnosticCode::LEGACY_SYNTAX_TREE_DEPTH_LIMIT
        }));
    }

    #[test]
    fn equation_depth_boundary_is_finite_and_typed() {
        let name = mantdoc::SourceName::new("equation-depth-boundary.1").unwrap();
        let mut source = String::from(".TH EQUATION-DEPTH 1\n.EQ\n");
        for _ in 0..5_000 {
            source.push_str("sqrt { ");
        }
        source.push('x');
        for _ in 0..5_000 {
            source.push_str(" }");
        }
        source.push_str("\n.EN\n");

        let native = mantdoc::Parser::default()
            .parse(mantdoc::Source::new(&name, source.as_bytes()))
            .unwrap();
        let native_equation = native
            .document
            .preorder()
            .find(|node| node.kind() == mantdoc::NodeKind::Equation)
            .and_then(mantdoc::NodeRef::equation)
            .unwrap();
        assert!(native_equation.len() < 4_000);
        assert!(native.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_str() == mantdoc::DiagnosticCode::LEGACY_EQUATION_TREE_DEPTH_LIMIT
        }));
    }

    fn native_document_depth(document: &mantdoc::Document) -> usize {
        let mut maximum = 0;
        let mut pending = vec![(document.node(document.root()).unwrap(), 1_usize)];
        while let Some((node, depth)) = pending.pop() {
            maximum = maximum.max(depth);
            pending.extend(node.children().map(|child| (child, depth + 1)));
        }
        maximum
    }
}
