use crate::{
    DiagnosticCode, FatalErrorKind, Limits, MacroSet, NodeKind, Parser, ParserConfig, Severity,
    Source, SourceBundle, SourceName, Syntax,
};

fn maximum_document_depth(document: &crate::Document) -> usize {
    let root = document.node(document.root()).unwrap();
    let mut maximum = 0;
    let mut pending = vec![(root, 1_usize)];
    while let Some((node, depth)) = pending.pop() {
        maximum = maximum.max(depth);
        pending.extend(node.children().map(|child| (child, depth + 1)));
    }
    maximum
}

mod conditions;
mod environment;
mod includes;
mod limits;
mod recovery;
mod scanning;
mod source_flow;
