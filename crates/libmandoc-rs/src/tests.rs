//! Behavioral parser regressions; FFI transfer and native renderer tests stay
//! beside their distinct lifetime owners.

#[cfg(feature = "serde")]
use crate::Diagnostic;
use crate::{
    AuthorMode, Compression, DefinitionListStyle, DiagnosticCode, DiagnosticLevel, DisplayKind,
    Document, IncludePolicy, InputFormat, MacroSet, Node, NodeKind, NormalizedFont,
    NormalizedListKind, ParseError, ParseOptions, Parser, SourceBundle, TableAlignment,
};
use std::{
    fs, process,
    sync::{Arc, Barrier},
};

mod bundles;
mod includes;
mod input;
mod lifecycle;
mod limits;
#[cfg(feature = "serde")]
mod serialization;
mod syntax;
mod tables;
mod targets;
#[cfg(windows)]
mod windows_root;

fn source_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("mant-{label}-{}.1", process::id()))
}

fn measured_depth(node: &Node) -> usize {
    1 + node.children.iter().map(measured_depth).max().unwrap_or(0)
}

fn parse_file(path: &std::path::Path, allow_includes: bool) -> Result<Document, ParseError> {
    Parser::new(ParseOptions {
        includes: if allow_includes {
            IncludePolicy::SourceTree
        } else {
            IncludePolicy::Deny
        },
        compression: Compression::Auto,
    })
    .parse_file(path)
    .map(|report| report.document)
}

fn find_macro<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    (node.macro_name.as_deref() == Some(name))
        .then_some(node)
        .or_else(|| {
            node.children
                .iter()
                .find_map(|child| find_macro(child, name))
        })
}

fn find_kind(node: &Node, kind: NodeKind) -> Option<&Node> {
    (node.kind == kind).then_some(node).or_else(|| {
        node.children
            .iter()
            .find_map(|child| find_kind(child, kind))
    })
}

fn find_node<'a>(node: &'a Node, predicate: &impl Fn(&Node) -> bool) -> Option<&'a Node> {
    predicate(node).then_some(node).or_else(|| {
        node.children
            .iter()
            .find_map(|child| find_node(child, predicate))
    })
}

fn collect_visible_text<'a>(node: &'a Node, visible: &mut Vec<&'a str>) {
    if !node.flags.no_print
        && let Some(text) = node.text.as_deref()
    {
        visible.push(text);
    }
    for child in &node.children {
        collect_visible_text(child, visible);
    }
}
