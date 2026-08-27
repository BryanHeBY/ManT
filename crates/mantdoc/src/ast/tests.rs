use std::{
    hint::black_box,
    mem::size_of,
    time::{Duration, Instant},
};

use crate::{Source, SourceName, SourcePosition, SourceSpan};

use super::{DocumentBuilder, MacroSet, NodeKind};

#[derive(Clone)]
struct RecursiveNode {
    record: super::NodeRecord,
    children: Vec<Self>,
}

impl RecursiveNode {
    fn root() -> Self {
        Self {
            record: super::NodeRecord::root(),
            children: Vec::new(),
        }
    }

    fn child(kind: NodeKind) -> Self {
        let mut record = super::NodeRecord::root();
        record.kind = kind;
        Self {
            record,
            children: Vec::new(),
        }
    }
}

#[test]
fn traversal_is_iterative_and_storage_indices_do_not_escape() {
    let mut builder = builder(MacroSet::Man);
    let root = DocumentBuilder::root();
    let section = builder.push(root, NodeKind::Block).unwrap();
    let text = builder.push(section, NodeKind::Text).unwrap();
    assert!(builder.text(text, "visible"));
    let document = builder.finish();

    let kinds = document
        .preorder()
        .map(super::NodeRef::kind)
        .collect::<Vec<_>>();
    assert_eq!(kinds, [NodeKind::Root, NodeKind::Block, NodeKind::Text]);
    let text = document.node(text).unwrap();
    assert_eq!(text.text(), Some("visible"));
    assert_eq!(text.ancestors().count(), 2);
    assert_eq!(document.node_count(), 3);
}

#[test]
fn finite_depth_prefix_keeps_the_legacy_root_counting_boundary() {
    let mut builder = builder(MacroSet::None);
    let mut parent = DocumentBuilder::root();
    for _ in 0..256 {
        parent = builder.push(parent, NodeKind::Element).unwrap();
    }

    assert!(builder.truncate_descendants_at_depth(256));
    let document = builder.finish();
    assert_eq!(document.node_count(), 256);
    assert_eq!(document.preorder().count(), 256);
    assert!(
        document.node(parent).is_none(),
        "discarded node IDs must not remain observable after compaction"
    );
}

#[test]
fn unknown_ids_are_checked_not_indexed_unconditionally() {
    let document = builder(MacroSet::None).finish();
    assert!(document.node(super::NodeId(u32::MAX)).is_none());
    assert_eq!(document.preorder().count(), 1);
}

/// Records a transparent M1 storage comparison; run explicitly with
/// `cargo test -p mantdoc arena_layout_microbenchmark --release -- --ignored --nocapture`.
#[test]
#[ignore = "microbenchmark output is recorded in the M0/M1 baseline manifest"]
fn arena_layout_microbenchmark() {
    const CHILDREN: usize = 50_000;
    const ROUNDS: usize = 100;

    let mut builder = builder(MacroSet::Man);
    let root = DocumentBuilder::root();
    for _ in 0..CHILDREN {
        builder.push(root, NodeKind::Element).unwrap();
    }
    let arena = builder.finish();

    let mut recursive = RecursiveNode::root();
    recursive.children.reserve(CHILDREN);
    for _ in 0..CHILDREN {
        recursive
            .children
            .push(RecursiveNode::child(NodeKind::Element));
    }

    let arena_bytes = arena.nodes.capacity() * size_of::<super::NodeRecord>()
        + arena.child_edges.capacity() * size_of::<super::NodeId>();
    let recursive_bytes = recursive_storage_bytes(&recursive);
    assert!(
        arena_bytes < recursive_bytes,
        "arena must reduce final topology storage"
    );

    let arena_time = time_rounds(ROUNDS, || arena.preorder().count());
    let recursive_time = time_rounds(ROUNDS, || recursive_preorder_count(&recursive));
    println!(
        "arena-layout\tchildren={CHILDREN}\tarena_bytes={arena_bytes}\trecursive_bytes={recursive_bytes}\tarena_ns={}\trecursive_ns={}",
        arena_time.as_nanos() / ROUNDS as u128,
        recursive_time.as_nanos() / ROUNDS as u128,
    );
}

fn recursive_storage_bytes(root: &RecursiveNode) -> usize {
    let mut bytes = size_of::<RecursiveNode>();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        bytes += node.children.capacity() * size_of::<RecursiveNode>();
        pending.extend(&node.children);
    }
    bytes
}

fn recursive_preorder_count(root: &RecursiveNode) -> usize {
    let mut count = 0;
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        black_box(node.record.kind);
        count += 1;
        pending.extend(&node.children);
    }
    count
}

fn time_rounds(rounds: usize, operation: impl Fn() -> usize) -> Duration {
    let start = Instant::now();
    for _ in 0..rounds {
        black_box(operation());
    }
    start.elapsed()
}

fn builder(macro_set: MacroSet) -> DocumentBuilder {
    let name = SourceName::new("test.1").expect("fixed source name");
    DocumentBuilder::new(macro_set, Source::new(&name, b""))
}

#[test]
fn source_ids_resolve_through_document_owned_line_indexes() {
    let name = SourceName::new("manual.1").expect("fixed source name");
    let bytes = b"first\nsecond";
    let mut builder = DocumentBuilder::new(MacroSet::Man, Source::new(&name, bytes));
    let text = builder
        .push(DocumentBuilder::root(), NodeKind::Text)
        .unwrap();
    let span = SourceSpan::new(DocumentBuilder::root_source(), 6, 12).expect("monotonic span");
    assert!(builder.location(text, span.clone()));
    let document = builder.finish();

    assert_eq!(document.source_count(), 1);
    assert_eq!(document.source_name(document.root_source()), Some(&name));
    assert_eq!(
        document.source_position(&span),
        Some(SourcePosition { line: 2, column: 1 })
    );
    assert_eq!(document.node(text).unwrap().location(), Some(&span));
}
