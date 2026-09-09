# mant-query

Bounded semantic queries over existing `ManT` document snapshots. This crate
selects content, projects outlines and references, searches canonical content,
and collects independent explanation evidence. It does not discover sources,
load files, resolve remote documents, render reports, or modify input IR.

## Existing content in, query results out

Single-document operations borrow `mant_ir::ResolvedContent`. Requests and
versioned response DTOs belong to `mant-protocol`; original document nodes,
typed locations, identities and reference scanning belong to `mant-ir`.

```rust
use mant_ir::{Document, ResolvedContent};
use mant_protocol::OutlineNode;
use mant_query::build_outline;

// The caller already has IR: no source parser or loader is needed here.
let document: Document = serde_json::from_value(serde_json::json!({
    "source": { "format": "markdown" },
    "meta": {},
    "sections": [{
        "id": "usage",
        "heading": { "content": [{ "type": "text", "value": "Usage" }] },
        "blocks": [],
        "children": []
    }]
}))?;
let content = ResolvedContent {
    label: "demo".to_owned(),
    address: None,
    document: Some(document),
    tldr: None,
};
let outline = build_outline(&content)?;
assert!(matches!(
    outline.nodes.as_slice(),
    [OutlineNode::DocumentSection { title, .. }] if title == "Usage"
));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Content selection uses explicit paths or IDs, never name guessing. Explain is
a separate evidence query: repeated names produce independent evidence, not a
first-match selection. Classification, global order, paging and content-copy
budgets remain explicit in the result, including partial coverage.

## Collections without a loader dependency

`QueryScopeView::new` validates a borrowed logical graph against its ordered
content slice. It checks address and traversal consistency without cloning,
loading or serializing the documents. Callers retain ownership and must supply
one coherent snapshot; this validation does not prove remote freshness.
`search_scope` and `explain_scope` share global query budgets and pagination.

Use `mant-loader` to acquire content when needed, or compose an application's
complete request with `mant-engine`. Neither is a dependency of this package.
Query errors stay separate from source acquisition and host delivery errors.

## Format boundary

Markdown-scope search uses the canonical Markdown artifact and source mapping
from `mant-codec`, with native features disabled. Its text and ranges refer to
the same borrowed source snapshot. This format dependency does not enable
native parsing or report rendering. Reference destination checks inspect
typed logical addresses and already supplied content only; they perform no IO.

See the [protocol manual](https://github.com/BryanHeBY/ManT/blob/dev/docs/manuals/mant-protocol.md)
for process contracts and the [IR manual](https://github.com/BryanHeBY/ManT/blob/dev/docs/manuals/mant-ir.md)
for authoritative content ownership. The crate is Apache-2.0 licensed.
