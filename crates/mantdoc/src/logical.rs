//! Storage-independent Serde projection for the immutable arena tree.

use serde::{Deserialize, Serialize};

use crate::{
    AuthorMode, DiagnosticCode, DisplayKind, Document, MacroSet, Metadata, NodeFlags, NodeKind,
    NormalizedEnclosure, NormalizedFont, NormalizedListKind, ParseReport, ParseStatistics,
    Severity, SourceSpan, TableCell,
};

/// Schema version for [`LogicalParseReport`].
pub const LOGICAL_PARSE_REPORT_SCHEMA_VERSION: u16 = 1;

/// Logical owned document serialization independent of arena/string-table IDs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LogicalDocument {
    /// Selected macro package.
    pub macro_set: MacroSet,
    /// Normalized document metadata.
    pub metadata: Metadata,
    /// Nodes in preorder; paths are sibling indices from the synthetic root.
    pub nodes: Vec<LogicalNode>,
}

/// Complete storage-independent parse result suitable for durable exchange.
///
/// This is the Serde surface for a parser result. It deliberately serializes
/// source names and derived line/column positions rather than opaque arena or
/// source-map indices. Deserializing it reconstructs this logical value, not
/// a mutable parser session or an arena-backed [`Document`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LogicalParseReport {
    /// Version of this logical schema.
    pub schema_version: u16,
    /// Storage-independent syntax document.
    pub document: LogicalDocument,
    /// Recoverable diagnostics with logical source locations.
    pub diagnostics: Vec<LogicalDiagnostic>,
    /// Bounded work counters without parser-internal state.
    pub statistics: ParseStatistics,
}

/// One parser diagnostic with storage-independent locations.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LogicalDiagnostic {
    /// Stable parser-defined diagnostic identifier.
    pub code: DiagnosticCode,
    /// Severity independent from diagnostic wording.
    pub severity: Severity,
    /// Primary source range when known.
    pub primary: Option<LogicalSourceSpan>,
    /// Additional source ranges that explain this finding.
    pub related: Vec<LogicalRelatedSpan>,
    /// Human-readable explanation not used as a programmatic identifier.
    pub message: String,
}

/// One logical source range associated with a diagnostic.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LogicalRelatedSpan {
    /// Related source range.
    pub span: LogicalSourceSpan,
    /// Concise relationship label.
    pub message: String,
}

/// One canonical logical AST node.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LogicalNode {
    /// Zero-based sibling-index path from the synthetic root.
    pub path: Vec<u32>,
    /// Node role.
    pub kind: NodeKind,
    /// Macro/request name when applicable.
    pub macro_name: Option<String>,
    /// Public normalized visible text.
    pub text: Option<String>,
    /// Validated same-document destination.
    pub tag: Option<String>,
    /// Source location when parser state provides it.
    pub location: Option<LogicalSourceSpan>,
    /// Source and semantic flags.
    pub flags: NodeFlags,
    /// List behavior.
    pub list_kind: Option<NormalizedListKind>,
    /// Display fill behavior.
    pub display_kind: Option<DisplayKind>,
    /// Mdoc font behavior.
    pub font: Option<NormalizedFont>,
    /// Mdoc author behavior.
    pub author_mode: Option<AuthorMode>,
    /// Resolved mdoc enclosure delimiters.
    pub enclosure: Option<NormalizedEnclosure>,
    /// Compact-list behavior.
    pub compact: bool,
    /// Normalized roff offset.
    pub offset: Option<String>,
    /// Normalized list width.
    pub width: Option<String>,
    /// Logical table cells.
    pub table_cells: Vec<TableCell>,
    /// Normalized eqn text.
    pub equation: Option<String>,
    /// Direct child count.
    pub child_count: usize,
}

/// Storage-independent source location in a logical AST projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LogicalSourceSpan {
    /// Logical source identity, never a document's internal numeric ID.
    pub source: String,
    /// Zero-based inclusive byte offset.
    pub start: u32,
    /// Zero-based exclusive byte offset.
    pub end: u32,
    /// One-based physical source line derived by the document's line index.
    pub line: u32,
    /// One-based byte column derived by the document's line index.
    pub column: u32,
}

impl From<&Document> for LogicalDocument {
    fn from(document: &Document) -> Self {
        let mut nodes = Vec::with_capacity(document.node_count());
        let mut pending = vec![(document.root(), Vec::new())];
        while let Some((id, path)) = pending.pop() {
            let node = document
                .node(id)
                .expect("document traversal only stores valid node IDs");
            let children = node.children().collect::<Vec<_>>();
            nodes.push(LogicalNode {
                path: path.clone(),
                kind: node.kind(),
                macro_name: node.macro_name().map(str::to_owned),
                text: node.text().map(str::to_owned),
                tag: node.tag().map(str::to_owned),
                location: node
                    .location()
                    .map(|span| logical_source_span(document, span)),
                flags: node.flags(),
                list_kind: node.list_kind(),
                display_kind: node.display_kind(),
                font: node.font(),
                author_mode: node.author_mode(),
                enclosure: node.enclosure().cloned(),
                compact: node.compact(),
                offset: node.offset().map(str::to_owned),
                width: node.width().map(str::to_owned),
                table_cells: node.table_cells().to_vec(),
                equation: node.equation().map(str::to_owned),
                child_count: children.len(),
            });
            for (index, child) in children.into_iter().enumerate().rev() {
                let index =
                    u32::try_from(index).expect("NodeId bounds a node's direct-child count");
                let mut child_path = path.clone();
                child_path.push(index);
                pending.push((child.id(), child_path));
            }
        }
        Self {
            macro_set: document.macro_set(),
            metadata: document.metadata().clone(),
            nodes,
        }
    }
}

impl From<&ParseReport> for LogicalParseReport {
    fn from(report: &ParseReport) -> Self {
        let document = &report.document;
        Self {
            schema_version: LOGICAL_PARSE_REPORT_SCHEMA_VERSION,
            document: LogicalDocument::from(document),
            diagnostics: report
                .diagnostics
                .iter()
                .map(|diagnostic| LogicalDiagnostic {
                    code: diagnostic.code.clone(),
                    severity: diagnostic.severity,
                    primary: diagnostic
                        .primary
                        .as_ref()
                        .map(|span| logical_source_span(document, span)),
                    related: diagnostic
                        .related
                        .iter()
                        .map(|related| LogicalRelatedSpan {
                            span: logical_source_span(document, &related.span),
                            message: related.message.to_string(),
                        })
                        .collect(),
                    message: diagnostic.message.to_string(),
                })
                .collect(),
            statistics: report.statistics.clone(),
        }
    }
}

fn logical_source_span(document: &Document, span: &SourceSpan) -> LogicalSourceSpan {
    let position = document
        .source_position(span)
        .expect("logical locations retain valid document byte offsets");
    LogicalSourceSpan {
        source: document
            .source_name(span.source)
            .expect("logical locations retain a valid document source")
            .as_str()
            .to_owned(),
        start: span.start,
        end: span.end,
        line: position.line,
        column: position.column,
    }
}

#[cfg(test)]
mod tests {
    use crate::{MacroSet, NodeKind, Parser, Source, SourceName, SourceSpan, ast::DocumentBuilder};

    use super::{LOGICAL_PARSE_REPORT_SCHEMA_VERSION, LogicalDocument, LogicalParseReport};

    #[test]
    fn logical_projection_contains_paths_not_arena_indices() {
        let name = SourceName::new("logical.1").expect("fixed source name");
        let mut builder = DocumentBuilder::new(MacroSet::Man, Source::new(&name, b"first\nsecond"));
        let root = DocumentBuilder::root();
        let first = builder.push(root, NodeKind::Text).unwrap();
        builder.text(first, "first");
        assert!(builder.location(
            first,
            SourceSpan::new(DocumentBuilder::root_source(), 0, 5).expect("monotonic span")
        ));
        let second = builder.push(root, NodeKind::Text).unwrap();
        builder.text(second, "second");
        let logical = LogicalDocument::from(&builder.finish());
        assert_eq!(
            logical
                .nodes
                .iter()
                .map(|node| node.path.clone())
                .collect::<Vec<_>>(),
            [vec![], vec![0], vec![1]]
        );
        let json = serde_json::to_string(&logical).expect("logical AST serializes");
        assert!(!json.contains("NodeId"));
        assert!(!json.contains("SourceId"));
        assert!(json.contains("logical.1"));
        assert_eq!(
            serde_json::from_str::<LogicalDocument>(&json).expect("logical AST deserializes"),
            logical
        );
    }

    #[test]
    fn logical_parse_report_round_trips_without_internal_source_ids() {
        let name = SourceName::new("logical-report.1").expect("fixed source name");
        let report = Parser::default()
            .parse(Source::new(&name, b".TH lower 1\n.SH BODY\nvisible text\n"))
            .expect("source parses with a style diagnostic");
        let logical = LogicalParseReport::from(&report);

        assert_eq!(logical.schema_version, LOGICAL_PARSE_REPORT_SCHEMA_VERSION);
        assert_eq!(logical.diagnostics.len(), 1);
        assert_eq!(
            logical.diagnostics[0].primary.as_ref().map(|location| (
                location.source.as_str(),
                location.line,
                location.column
            )),
            Some(("logical-report.1", 1, 5))
        );
        let json = serde_json::to_string(&logical).expect("logical report serializes");
        assert!(!json.contains("SourceId"));
        assert!(!json.contains("NodeId"));
        assert!(json.contains("logical-report.1"));
        assert_eq!(
            serde_json::from_str::<LogicalParseReport>(&json).expect("logical report deserializes"),
            logical
        );
    }
}
