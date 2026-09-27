//! Bound recursive source structure by its position in the final JSON document.

use mant_ir::{
    Block, Diagnostic, DiagnosticImpact, DiagnosticLevel, Document, EquationExpression,
    EquationFont, EquationKind, EquationPosition, Inline, LayoutHint, Section, SourceSpan,
};
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

// serde_json's default reader allows 128 nested containers. Count the two
// query/document envelopes, then every actual object and array on the route
// to an equation. Sixteen spare levels cover the expression's mandatory
// `children` array and auxiliary fields; a final parser probe checks unusual
// metadata shapes before the document leaves this boundary.
const QUERY_DOCUMENT_DEPTH: usize = 2;
const MAX_SAFE_CONTAINER_DEPTH: usize = 112;

#[derive(Default)]
struct Summary {
    equation: bool,
    equation_source: Option<SourceSpan>,
    structure: bool,
    structure_source: Option<SourceSpan>,
    max_object_depth: usize,
}

/// Bound both the surrounding document and any equation it contains. Every
/// collapsed subtree keeps its visible text while a diagnostic records lost
/// structure; shallow documents retain their original IR.
pub(super) fn bound_wire_structure(document: &mut Document) {
    let mut summary = Summary::default();
    if let Some(heading) = &mut document.heading {
        visit_inlines(&mut heading.content, QUERY_DOCUMENT_DEPTH + 2, &mut summary);
    }
    visit_blocks(&mut document.blocks, QUERY_DOCUMENT_DEPTH + 1, &mut summary);
    visit_sections(
        &mut document.sections,
        QUERY_DOCUMENT_DEPTH + 1,
        &mut summary,
    );
    if summary.equation {
        document.diagnostics.push(Diagnostic {
            impact: DiagnosticImpact::SemanticCoverage,
            level: DiagnosticLevel::Unsupported,
            code: Some("manual.equation-structure-depth-summarized".to_owned()),
            message: "deep equation structure was summarized as readable text for JSON consumers"
                .to_owned(),
            source: summary.equation_source,
        });
    }
    if summary.structure {
        document.diagnostics.push(Diagnostic {
            impact: DiagnosticImpact::SemanticCoverage,
            level: DiagnosticLevel::Unsupported,
            code: Some("manual.document-structure-depth-summarized".to_owned()),
            message: "deep document structure was summarized as readable text for JSON consumers"
                .to_owned(),
            source: summary.structure_source,
        });
    }
    // A rare combination of nested non-content metadata can add containers
    // outside the Block/Inline paths counted above. Exercise the actual JSON
    // parser near its boundary and retain all visible content if that shape
    // still exceeds the wire reader's recursion allowance.
    if (summary.max_object_depth >= 96 || summary.equation || summary.structure)
        && !wire_probe(document)
    {
        let text = readable_document(document);
        document.blocks = vec![Block::Unsupported {
            name: None,
            text,
            layout: LayoutHint::default(),
            source: None,
        }];
        document.sections.clear();
        if !summary.structure {
            document.diagnostics.push(Diagnostic {
                impact: DiagnosticImpact::SemanticCoverage,
                level: DiagnosticLevel::Unsupported,
                code: Some("manual.document-structure-depth-summarized".to_owned()),
                message:
                    "deep document structure was summarized as readable text for JSON consumers"
                        .to_owned(),
                source: None,
            });
        }
    }
}

fn visit_sections(sections: &mut [Section], array_depth: usize, summary: &mut Summary) {
    for section in sections {
        let object_depth = array_depth + 1;
        summary.max_object_depth = summary.max_object_depth.max(object_depth);
        visit_inlines(&mut section.heading.content, object_depth + 2, summary);
        visit_blocks(&mut section.blocks, object_depth + 1, summary);
        visit_sections(&mut section.children, object_depth + 1, summary);
    }
}

fn visit_blocks(blocks: &mut [Block], array_depth: usize, summary: &mut Summary) {
    for block in blocks {
        let object_depth = array_depth + 1;
        summary.max_object_depth = summary.max_object_depth.max(object_depth);
        if object_depth >= MAX_SAFE_CONTAINER_DEPTH
            && let Some((layout, source)) = recursive_block_layout_source(block)
        {
            let text = readable_block(block);
            *block = Block::Unsupported {
                name: None,
                text,
                layout,
                source,
            };
            if !summary.structure {
                summary.structure = true;
                summary.structure_source = source;
            }
            continue;
        }
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                visit_inlines(children, object_depth + 1, summary);
            }
            Block::List { items, .. } => {
                for item in items {
                    visit_blocks(&mut item.blocks, object_depth + 3, summary);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    for term in &mut item.terms {
                        visit_inlines(term, object_depth + 4, summary);
                    }
                    visit_blocks(&mut item.description, object_depth + 3, summary);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        visit_blocks(&mut cell.blocks, object_depth + 5, summary);
                    }
                }
            }
            Block::Equation {
                value,
                expression,
                source,
                ..
            } => {
                if let Some(expression) = expression {
                    let changed = bound_expression(expression, object_depth + 1, value);
                    if changed && !summary.equation {
                        summary.equation = true;
                        summary.equation_source = *source;
                    }
                }
            }
            Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn visit_inlines(inlines: &mut [Inline], array_depth: usize, summary: &mut Summary) {
    for inline in inlines {
        let object_depth = array_depth + 1;
        summary.max_object_depth = summary.max_object_depth.max(object_depth);
        if object_depth >= MAX_SAFE_CONTAINER_DEPTH
            && matches!(
                inline,
                Inline::Strong { .. } | Inline::Emphasis { .. } | Inline::Link { .. }
            )
        {
            let value = mant_ir::inline_plain_text(std::slice::from_ref(inline));
            *inline = Inline::Text { value };
            summary.structure = true;
            continue;
        }
        match inline {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                visit_inlines(children, object_depth + 1, summary);
            }
            Inline::Equation { value, expression } => {
                if bound_expression(expression, object_depth + 1, value) {
                    summary.equation = true;
                }
            }
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Anchor { .. }
            | Inline::LineBreak => {}
        }
    }
}

fn bound_expression(expression: &mut EquationExpression, object_depth: usize, value: &str) -> bool {
    let changed = fold_expression(expression, object_depth);
    if changed && expression.readable_text() != value {
        // A folded child can acquire a different sequence/operand context.
        // Fold at the expression root in that case, retaining its exact
        // authoritative readable payload and the parent document's text.
        *expression = readable_leaf(value.to_owned(), false);
    }
    changed
}

fn fold_expression(expression: &mut EquationExpression, object_depth: usize) -> bool {
    if object_depth >= MAX_SAFE_CONTAINER_DEPTH {
        let text = expression.readable_text();
        let grouped = expression.needs_operand_group();
        *expression = readable_leaf(text, grouped);
        return true;
    }
    let mut changed = false;
    for child in &mut expression.children {
        changed |= fold_expression(child, object_depth + 2);
    }
    changed
}

fn recursive_block_layout_source(block: &Block) -> Option<(LayoutHint, Option<SourceSpan>)> {
    match block {
        Block::Paragraph { layout, source, .. }
        | Block::Preformatted { layout, source, .. }
        | Block::List { layout, source, .. }
        | Block::DefinitionList { layout, source, .. }
        | Block::Table { layout, source, .. } => Some((*layout, *source)),
        Block::Equation { .. }
        | Block::VerticalSpace { .. }
        | Block::ThematicBreak { .. }
        | Block::Unsupported { .. } => None,
    }
}

fn readable_block(block: &Block) -> String {
    fn append(output: &mut String, text: &str) {
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(text);
    }
    fn collect(block: &Block, output: &mut String) {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                append(output, &mant_ir::inline_plain_text(children));
            }
            Block::List { items, .. } => {
                for item in items {
                    for block in &item.blocks {
                        collect(block, output);
                    }
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    for term in &item.terms {
                        append(output, &mant_ir::inline_plain_text(term));
                    }
                    for block in &item.description {
                        collect(block, output);
                    }
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        for block in &cell.blocks {
                            collect(block, output);
                        }
                    }
                }
            }
            Block::Equation { value, .. } => append(output, value),
            Block::Unsupported { text, .. } => append(output, text),
            Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => {}
        }
    }
    let mut output = String::new();
    collect(block, &mut output);
    output
}

fn readable_document(document: &Document) -> String {
    fn append_section(section: &Section, output: &mut String) {
        output.push_str(&section.heading.plain_text());
        output.push('\n');
        for block in &section.blocks {
            output.push_str(&readable_block(block));
            output.push('\n');
        }
        for child in &section.children {
            append_section(child, output);
        }
    }
    let mut output = String::new();
    for block in &document.blocks {
        output.push_str(&readable_block(block));
        output.push('\n');
    }
    for section in &document.sections {
        append_section(section, &mut output);
    }
    output
}

fn wire_probe(document: &Document) -> bool {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ProbeQuery<'a> {
        schema: &'static str,
        label: &'static str,
        document: ProbeDocument<'a>,
    }
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ProbeDocument<'a> {
        schema: &'static str,
        producer: ProbeProducer,
        source: &'a mant_ir::DocumentSource,
        meta: &'a mant_ir::DocumentMeta,
        heading: &'a Option<mant_ir::Heading>,
        fragment_aliases: &'a [mant_ir::FragmentAlias],
        diagnostics: &'a [Diagnostic],
        blocks: &'a [Block],
        sections: &'a [Section],
    }
    #[derive(serde::Serialize)]
    struct ProbeProducer {
        name: &'static str,
        version: &'static str,
    }
    let query = ProbeQuery {
        schema: "mant.query/v0.12",
        label: "",
        document: ProbeDocument {
            schema: "mant.document/v0.12",
            producer: ProbeProducer {
                name: "mant",
                version: "0.12.0",
            },
            source: &document.source,
            meta: &document.meta,
            heading: &document.heading,
            fragment_aliases: &document.fragment_aliases,
            diagnostics: &document.diagnostics,
            blocks: &document.blocks,
            sections: &document.sections,
        },
    };
    let Ok(encoded) = serde_json::to_string(&query) else {
        return false;
    };
    serde_json::from_str::<JsonDepthProbe>(&encoded).is_ok()
}

/// Parse every array/object boundary but retain no JSON values. In contrast
/// to `IgnoredAny`, this drives `serde_json`'s 128-level recursion accounting.
struct JsonDepthProbe;

impl<'de> Deserialize<'de> for JsonDepthProbe {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(JsonDepthVisitor)
    }
}

struct JsonDepthVisitor;

impl<'de> Visitor<'de> for JsonDepthVisitor {
    type Value = JsonDepthProbe;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self::Value, E> {
        Ok(JsonDepthProbe)
    }

    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Self::Value, E> {
        Ok(JsonDepthProbe)
    }

    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Self::Value, E> {
        Ok(JsonDepthProbe)
    }

    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> {
        Ok(JsonDepthProbe)
    }

    fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self::Value, E> {
        Ok(JsonDepthProbe)
    }

    fn visit_string<E: serde::de::Error>(self, _: String) -> Result<Self::Value, E> {
        Ok(JsonDepthProbe)
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        Ok(JsonDepthProbe)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        while sequence.next_element::<JsonDepthProbe>()?.is_some() {}
        Ok(JsonDepthProbe)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        while map
            .next_entry::<serde::de::IgnoredAny, JsonDepthProbe>()?
            .is_some()
        {}
        Ok(JsonDepthProbe)
    }
}

fn readable_leaf(text: String, summarized_operand_group: bool) -> EquationExpression {
    EquationExpression {
        kind: EquationKind::Text,
        font: EquationFont::None,
        position: EquationPosition::None,
        size: None,
        expected_args: None,
        actual_args: 0,
        summarized_operand_group,
        text: Some(text),
        left: None,
        right: None,
        top: None,
        bottom: None,
        children: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::{
        DefinitionItem, DefinitionLayout, DocumentMeta, DocumentSource, Heading, SourceFormat,
        TableCell, TableCellKind, TableRow, TableRowKind,
    };

    #[test]
    fn probe_uses_the_json_readers_recursion_guard() {
        let hostile = format!("{}0{}", "[".repeat(130), "]".repeat(130));
        assert!(serde_json::from_str::<JsonDepthProbe>(&hostile).is_err());
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the nested fixture covers every visible fallback owner"
    )]
    fn final_fallback_preserves_visible_terms_cells_and_equations() {
        let mut section = Section {
            id: "leaf".into(),
            fragment_aliases: Vec::new(),
            heading: Heading::from("leaf heading"),
            spacing_before_lines: 0,
            blocks: vec![
                Block::DefinitionList {
                    items: vec![DefinitionItem {
                        source: None,
                        entry: None,
                        terms: vec![vec![Inline::Text {
                            value: "definition term".into(),
                        }]],
                        description: vec![Block::Paragraph {
                            children: vec![Inline::Text {
                                value: "definition body".into(),
                            }],
                            layout: LayoutHint::default(),
                            source: None,
                        }],
                        layout: DefinitionLayout::default(),
                    }],
                    declaration_groups: Vec::new(),
                    compact: false,
                    layout: LayoutHint::default(),
                    source: None,
                },
                Block::Table {
                    rows: vec![TableRow {
                        kind: TableRowKind::Data,
                        cells: vec![TableCell {
                            kind: TableCellKind::Text,
                            blocks: vec![Block::Paragraph {
                                children: vec![Inline::Text {
                                    value: "table cell".into(),
                                }],
                                layout: LayoutHint::default(),
                                source: None,
                            }],
                            column_span: 1,
                            row_span: 1,
                            alignment: None,
                        }],
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                },
                Block::Equation {
                    value: "formula operand".into(),
                    expression: None,
                    display: true,
                    layout: LayoutHint::default(),
                    source: None,
                },
            ],
            children: Vec::new(),
            source: None,
        };
        for depth in 0..70 {
            section = Section {
                id: format!("level-{depth}").into(),
                fragment_aliases: Vec::new(),
                heading: Heading::from("parent heading"),
                spacing_before_lines: 0,
                blocks: Vec::new(),
                children: vec![section],
                source: None,
            };
        }
        let mut document = Document {
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Man,
                path: None,
            },
            meta: DocumentMeta::default(),
            heading: Some(Heading::from("document title")),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            sections: vec![section],
        };
        assert!(!wire_probe(&document));
        bound_wire_structure(&mut document);
        assert!(wire_probe(&document));
        assert!(document.sections.is_empty());
        assert_eq!(
            document.heading.as_ref().unwrap().plain_text(),
            "document title"
        );
        let Block::Unsupported { text, .. } = &document.blocks[0] else {
            panic!("readable fallback block");
        };
        assert!(!text.contains("document title"));
        for expected in [
            "leaf heading",
            "definition term",
            "definition body",
            "table cell",
            "formula operand",
        ] {
            assert!(text.contains(expected), "missing {expected}");
        }
        assert!(mant_ir::content_complete(&document.diagnostics));
        assert!(!mant_ir::semantics_complete(&document.diagnostics));
        assert_eq!(
            document
                .diagnostics
                .iter()
                .filter(|finding| {
                    finding.code.as_deref() == Some("manual.document-structure-depth-summarized")
                })
                .count(),
            1
        );
    }
}
