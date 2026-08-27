//! Temporary owned-tree projection from `mantdoc` into the legacy lowering shape.
//!
//! Engine lowering still consumes recursive nodes while M8 moves its modules to
//! arena traversal. This adapter is deliberately parser-only: no libmandoc C
//! code participates after `mantdoc` has produced its report.

use super::syntax as legacy;
use mantdoc::{self as native, NodeRef};

pub(super) fn project(report: &native::ParseReport) -> legacy::ParseReport {
    legacy::ParseReport {
        document: legacy::Document {
            macro_set: macro_set(report.document.macro_set()),
            metadata: metadata(report.document.metadata()),
            root: node(
                &report.document,
                report
                    .document
                    .node(report.document.root())
                    .expect("finished documents always contain their synthetic root"),
            ),
        },
        diagnostics: report
            .diagnostics
            .iter()
            .map(|diagnostic| legacy::Diagnostic {
                level: severity(diagnostic.severity),
                message: diagnostic.message.to_string(),
                location: diagnostic
                    .primary
                    .as_ref()
                    .and_then(|span| report.document.source_position(span))
                    .map(|position| legacy::SourceLocation {
                        line: position.line,
                        column: position.column,
                    }),
            })
            .collect(),
    }
}

fn node(document: &native::Document, value: NodeRef<'_>) -> legacy::Node {
    let position = value
        .location()
        .and_then(|span| document.source_position(span));
    legacy::Node {
        kind: node_kind(value.kind()),
        macro_name: value.macro_name().map(str::to_owned),
        text: value.text().map(str::to_owned),
        tag: value.tag().map(str::to_owned),
        line: position.map_or(0, |position| position.line),
        column: position.map_or(0, |position| position.column),
        flags: flags(value.flags()),
        list_kind: value.list_kind().map(list_kind),
        display_kind: value.display_kind().map(display_kind),
        font: value.font().map(font),
        author_mode: value.author_mode().map(author_mode),
        enclosure: value
            .enclosure()
            .map(|enclosure| legacy::NormalizedEnclosure {
                opening: enclosure.opening.to_string(),
                closing: enclosure.closing.as_deref().map(str::to_owned),
            }),
        compact: value.compact(),
        offset: value.offset().map(str::to_owned),
        width: value.width().map(str::to_owned),
        table_cells: value
            .table_cells()
            .iter()
            .map(|cell| legacy::TableCell {
                text: cell.text.as_deref().map(str::to_owned),
                text_block: cell.text_block,
                vertical_continuation: cell.vertical_continuation,
                column_span: cell.column_span,
                row_span: cell.row_span,
                alignment: table_alignment(cell.alignment),
            })
            .collect(),
        equation: value.equation().map(str::to_owned),
        children: value
            .children()
            .map(|child| node(document, child))
            .collect(),
    }
}

fn macro_set(value: native::MacroSet) -> legacy::MacroSet {
    match value {
        native::MacroSet::None => legacy::MacroSet::None,
        native::MacroSet::Mdoc => legacy::MacroSet::Mdoc,
        native::MacroSet::Man => legacy::MacroSet::Man,
    }
}

fn metadata(value: &native::Metadata) -> legacy::Metadata {
    legacy::Metadata {
        title: value.title.as_deref().map(str::to_owned),
        section: value.section.as_deref().map(str::to_owned),
        volume: value.volume.as_deref().map(str::to_owned),
        os: value.os.as_deref().map(str::to_owned),
        arch: value.arch.as_deref().map(str::to_owned),
        name: value.name.as_deref().map(str::to_owned),
        date: value.date.as_deref().map(str::to_owned),
        alias_target: value.alias_target.as_deref().map(str::to_owned),
        has_body: value.has_body,
    }
}

fn severity(value: native::Severity) -> legacy::DiagnosticLevel {
    match value {
        native::Severity::Unsupported => legacy::DiagnosticLevel::Unsupported,
        native::Severity::Error => legacy::DiagnosticLevel::Error,
        native::Severity::Warning => legacy::DiagnosticLevel::Warning,
        native::Severity::Style => legacy::DiagnosticLevel::Style,
    }
}

fn node_kind(value: native::NodeKind) -> legacy::NodeKind {
    match value {
        native::NodeKind::Root => legacy::NodeKind::Root,
        native::NodeKind::Block => legacy::NodeKind::Block,
        native::NodeKind::Head => legacy::NodeKind::Head,
        native::NodeKind::Body => legacy::NodeKind::Body,
        native::NodeKind::Tail => legacy::NodeKind::Tail,
        native::NodeKind::Element => legacy::NodeKind::Element,
        native::NodeKind::Text => legacy::NodeKind::Text,
        native::NodeKind::Comment => legacy::NodeKind::Comment,
        native::NodeKind::Table => legacy::NodeKind::Table,
        native::NodeKind::Equation => legacy::NodeKind::Equation,
    }
}

fn flags(value: native::NodeFlags) -> legacy::NodeFlags {
    legacy::NodeFlags {
        generated: value.generated,
        sentence_end: value.sentence_end,
        no_print: value.no_print,
        no_fill: value.no_fill,
        deep_link_target: value.deep_link_target,
        permalink: value.permalink,
        line_start: value.line_start,
        delimiter_open: value.delimiter_open,
        delimiter_close: value.delimiter_close,
        line_continuation: value.line_continuation,
        synopsis_pretty: value.synopsis_pretty,
    }
}

fn list_kind(value: native::NormalizedListKind) -> legacy::NormalizedListKind {
    match value {
        native::NormalizedListKind::Bullet => legacy::NormalizedListKind::Bullet,
        native::NormalizedListKind::Ordered => legacy::NormalizedListKind::Ordered,
        native::NormalizedListKind::Definition => legacy::NormalizedListKind::Definition,
        native::NormalizedListKind::Column => legacy::NormalizedListKind::Column,
        native::NormalizedListKind::Plain => legacy::NormalizedListKind::Plain,
    }
}

fn display_kind(value: native::DisplayKind) -> legacy::DisplayKind {
    match value {
        native::DisplayKind::Literal => legacy::DisplayKind::Literal,
        native::DisplayKind::Filled => legacy::DisplayKind::Filled,
    }
}

fn font(value: native::NormalizedFont) -> legacy::NormalizedFont {
    match value {
        native::NormalizedFont::Emphasis => legacy::NormalizedFont::Emphasis,
        native::NormalizedFont::Literal => legacy::NormalizedFont::Literal,
        native::NormalizedFont::Symbolic => legacy::NormalizedFont::Symbolic,
    }
}

fn author_mode(value: native::AuthorMode) -> legacy::AuthorMode {
    match value {
        native::AuthorMode::Split => legacy::AuthorMode::Split,
        native::AuthorMode::NoSplit => legacy::AuthorMode::NoSplit,
    }
}

fn table_alignment(value: native::TableAlignment) -> legacy::TableAlignment {
    match value {
        native::TableAlignment::Left => legacy::TableAlignment::Left,
        native::TableAlignment::Center => legacy::TableAlignment::Center,
        native::TableAlignment::Right => legacy::TableAlignment::Right,
    }
}

#[cfg(test)]
mod tests {
    use mantdoc::{DiagnosticProfile, Parser, ParserConfig, Source, SourceName};

    use super::project;

    #[test]
    fn retains_metadata_locations_and_recursive_children() {
        let name = SourceName::new("adapter.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(&name, b".TH ADAPTER 1\n.SH NAME\nadapter\n"))
            .unwrap();
        let projected = project(&report);
        assert_eq!(
            projected.document.metadata.title.as_deref(),
            Some("ADAPTER")
        );
        assert!(
            projected
                .document
                .root
                .children
                .iter()
                .any(|node| !node.children.is_empty())
        );
    }

    #[test]
    fn retains_each_distinct_badly_nested_recovery_message() {
        let name = SourceName::new("broken.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BL-BROKEN 1\n.Os\n.Sh NAME\n.Nm Bl-broken\n.Nd list broken by another block\n.Sh DESCRIPTION\nbefore both\n.Bo before list\n.Bl -enum -offset indent\n.It\ninside both\n.Bc\nafter bracket\n.El\nafter list\n.Bo before list\n.Bl -enum -offset indent\n.It\ninside list\n.Bd -ragged -offset indent\ninside display\n.Bc\nafter bracket\n.It\nnext item\n.El\nafter list\n",
            ))
            .unwrap();
        let projected = project(&report);
        let messages = projected
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            messages,
            [
                "blocks badly nested: Bo breaks Bl",
                "blocks badly nested: Bo breaks Bd",
                "inserting missing end of block: It breaks Bd",
            ]
        );
    }

    #[test]
    fn preserves_legacy_diagnostic_projection_without_muting_native_findings() {
        let name = SourceName::new("openbsd.1").unwrap();
        let upstream = Parser::default()
            .parse(Source::new(
                &name,
                b".\\\" $OpenBSD: openbsd.1,v 1.1 2026/08/28 00:00:00 user Exp $\n.Dd bad date\n.Dt OPENBSD 1\n.Os\n.Sh NAME\n.Nm openbsd\n",
            ))
            .unwrap();
        assert!(upstream.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_str() == mantdoc::DiagnosticCode::MDOC_MDOCDATE_MISSING
        }));

        let report = Parser::new(ParserConfig {
            diagnostic_profile: DiagnosticProfile::LibmandocRsV0_9,
            ..ParserConfig::default()
        })
        .parse(Source::new(
            &name,
            b".\\\" $OpenBSD: openbsd.1,v 1.1 2026/08/28 00:00:00 user Exp $\n.Dd bad date\n.Dt OPENBSD 1\n.Os\n.Sh NAME\n.Nm openbsd\n",
        ))
        .unwrap();

        let projected = project(&report);
        assert_eq!(
            projected
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_str())
                .collect::<Vec<_>>(),
            [
                "cannot parse date, using it verbatim: Dd bad date",
                "NAME section without description",
            ]
        );
    }
}
