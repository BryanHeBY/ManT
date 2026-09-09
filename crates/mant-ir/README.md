# mant-ir

`mant-ir` defines `ManT`'s normalized, source-neutral document intermediate
representation. Native man/mdoc syntax and Markdown parser events are lowered
into the same owned tree before projection, search, rendering, or interactive
navigation.

Use this crate when an in-process component needs to inspect, transform, index,
or render a document without depending on the parser that produced it. It does
not perform source lookup, parsing, rendering, filesystem access, or process
protocol handling.

## Model at a glance

```text
ResolvedContent
├── address: DocumentAddress?       registered catalog identity
├── document: Document?             normalized full document
│   ├── meta + diagnostics
│   ├── heading?                    original displayed title inlines
│   ├── blocks                      content before the first heading
│   ├── sections[]                  recursive heading-backed content
│   │   └── blocks[]                paragraphs, lists, definitions, tables, …
│   └── ListItem / DefinitionItem   actual owners inside those blocks
│       └── entry: EntryFacts?      optional facts, never replacement content
│           └─> SemanticIndex       rebuildable entry hierarchy
└── tldr: TldrDocument?             distinct quick-reference channel
```

`Heading.content` preserves the authoritative inline title, including links,
styles, anchors and hard breaks. `Section.heading` is required; its plain label
is derived with `Heading::plain_text()`. A leading Markdown H1 becomes
`Document.heading`, not a duplicate paragraph or metadata title. Native TH/Dt
titles remain bibliographic metadata and do not fabricate displayed headings.
The ordinary immutable/mutable visitors include heading content in source order.

Names and kinds are independent: a `Term` may have exact bound names, while a
native template can have a proved parameter/environment kind but no supported
exact name. `terms` are native displayed content, `forms` reference authored
syntax, and `names` reference selectable spellings; none replaces an owner's
description. Nested entries record content ownership, not automatically a
complete runtime value domain. Configuration keys may describe fields inside
an option's argument. Only explicit `valueDomain` facts make that relationship.

`SemanticIndex::owner_at` maps a semantic outline path back to its exact
original list/definition item, including transparent nested containers and
table cells. This operation-local sidecar is built during the same owner walk,
not reconstructed by matching IDs or names. It does not add rendering facts to
the document, and compact summary traversal does not need to materialize it.

`Block::DefinitionList.declaration_groups` optionally records recovered reading
context: consecutive empty declaration heads followed by one described item.
Each `DeclarationGroup` is a checked half-open range in its containing list.
The last member supplies context, not inherited ownership or proof of aliases;
items, sources, children and value domains remain independent. Whole-document
rendering ignores the annotation. Transformations must rebase complete groups
or drop partial groups when slicing; IR round trips retain the annotation.

The important public families are:

| API | Purpose |
| --- | --- |
| `Document`, `Section`, `Block`, `Inline` | Source-neutral content tree |
| `ListItem`, `DefinitionItem`, `ListKind` | Distinct content shapes, independent of optional semantic annotation |
| `EntryFacts`, `EntryKind`, `NameCase` | Common facts and classification attached through either item's `entry` field |
| `EntryOwner`, `EntryForm`, `EntryContentSlice` | Borrowed content owners and validated final-IR form references, without duplicate bodies |
| `SemanticIndex`, `SemanticEntry`, `EntrySummary` | Rebuildable role-aware hierarchy, authored forms, and compact coverage |
| `DocumentAddress`, `MarkdownOrigin` | Exact identity in `ManT`'s catalog rather than a physical path |
| `NodeId`, `FragmentAlias`, `OutlinePath`, `TextRange` | Normalized local identities, exact source fragments, and coordinates |
| `DocumentIndex` | Immutable lookup sidecar derived from one document |
| `validate_document` | Shared structural invariant checks |
| `DocumentValidation` | One immutable document's index, typed relationship issues and complete invariant findings, reused within an operation |
| `entry_relation_issues`, `EntryRelationIssueKind` | Typed owner-specific relationship failures for producers and evidence consumers; the same policy backs document diagnostics |
| `is_normalized_node_id` | Shared canonical ID grammar, separate from uniqueness and selector reservation |
| `Visit`, `VisitMut` | Exhaustive read-only or mutable traversal |
| `ContentLocation`, `ContentBlockStep`, `EntryOwnerLocationRef` | Checked snapshot-relative heading/content/item positions; explicit mapping from entry-local slices |
| `scan_reference_scope`, `ReferenceScope`, `ReferenceScanLimits` | Bounded borrowed occurrences from original links, independent of semantic discovery |
| `ReferenceWorkBudget`, `reference_form_associations` | One shared work account for scanning and optional validated original-form association |

Reference scans visit authoritative heading and body inlines, preserving empty
labels and repeated destinations. They do not create entries or duplicate form
links. Callbacks borrow reusable position stacks; retaining a position requires
an explicit bounded `ContentLocationRef::to_owned()` operation. Reports distinguish
a complete scan from a work, depth, byte, position or consumer stop. A semantic
owner exposed during traversal means attached facts, not that every field in
those facts has been validated; optional form association checks original slices
atomically under the same work budget. Ordinary overview, section, block and
unannotated item roots do not require a `SemanticIndex`.

Positions describe the currently loaded IR, not source bytes, visible Unicode
scalar offsets or terminal cells. They can be rebuilt after serde but have no
cross-edit stability guarantee. Explanation block positions reuse the same
checked item/cell traversal while remaining relative to their returned block.

`project_content_slice` converts a checked owner-local UTF-8 slice into a
`RootTextRange` within that original inline root. `inline_scalar_len` supplies
the common scalar-counting rule: wrappers and anchors have no width in this
coordinate space, while an authored hard break occupies one position. Queries
and renderers share this conversion without a presentation dependency. It
validates coordinates, not semantic names; a valid slice alone does not prove
that its text is a bound executable name. Display cells and generated padding
are deliberately excluded.

`DocumentMeta::manual_section` is a native manual category such as `1` or
`3p`. A `Section` is a heading-backed content subtree. The names are kept
deliberately separate so consumers cannot confuse storage lookup with
within-document navigation.

## Content and semantic indexes

Layout is source-neutral geometry, not terminal decoration. Signed block
offsets are relative to the actual content parent; compose them once and clamp
only at the displayed leaf. A definition resolves its label/body displacement,
and a paragraph can separately displace continuation lines. Reparent only the
moved root, preserving the descendants' coordinates and source text.

`geometry` supplies the shared small core for display-cell measurement, signed
origin composition, definition run-in width, table outdent preservation and
bounded gap composition. These operations inspect resolved content without
allocating a rendered document. They do not choose a viewport, terminal style
or output format. Display cells are distinct from source bytes and semantic
scalar coordinates; gap request identities are operation-local, not serialized
document identities.

`geometry::block_layout`, `block_layout_mut` and `block_source` provide exhaustive
access to the optional fields on block variants. `geometry::rebase_roots` moves
only the supplied roots between resolved parent origins; descendants, source
spans, continuation offsets and spacing remain unchanged. Source macro/unit
interpretation stays in the producer, not in these helpers.

Block spacing is resolved before IR: zero is tight. Independent explicit gaps
add across transparent containers; repeated projections of one request belong
at only one consumption point. List-item and definition-item absent spacing instead inherits
compactness. Frontends cap a resolved boundary at 4096 rows, without merging
literal blank lines. Visual wrapping never changes source lines or entry facts.

Literal rows are content: an explicit empty text row in `Preformatted` is
different from an anchor-only block and ends the preceding gap boundary.
Frontends retain leading, trailing and entirely blank literal rows. For a
multi-line definition term, run-in fitting measures the final open label row,
not the longest earlier row; explicit hard breaks close that row.

`DocumentIndex` addresses content nodes. `SemanticIndex` separately groups
identified content owners into commands, parameter families, configuration keys,
variables, values, and terms. A semantic entry keeps exact selectable names,
complete authored forms, explicit alias groups and same-document alias relations,
linked-document destinations, and nested ownership; it does not
replace or merge their original content. Unannotated list and definition items,
including those in table cells, are transparent to direct-child discovery;
annotated owners establish a boundary even when their forms or names are invalid.

Build both indexes and run shared validation after obtaining a document from
`mant-engine` or another trusted producer:

```rust
use mant_ir::{Document, DocumentIndex, SemanticIndex, validate_document};

fn inspect(document: &Document) {
    let content = DocumentIndex::build(document);
    let semantics = SemanticIndex::build(document);
    println!("{} addressable identities", content.iter().count());

    for entry in semantics.root() {
        println!("{}: {:?}", entry.id, entry.names);
        assert!(content.get(&entry.id).is_some());
        for target in &entry.document_targets {
            println!("  linked document: {:?}", target.reference);
        }
    }

    for diagnostic in validate_document(document) {
        eprintln!("{}", diagnostic.message);
    }
}
```

Entries owned by a heading are available through `SemanticIndex::section`.
Nested entries remain under their parent `SemanticEntry`; callers should not
flatten that hierarchy when ownership affects interpretation.

### Construct either owner without changing content

This complete example starts with an ordinary item, adds facts to an identical
copy, and then constructs a distinct term-and-description item. Annotation does
not convert one shape into the other. A native or custom producer must record
known forms explicitly; missing forms never imply “use the terms”.

```rust
use mant_ir::{
    Block, DefinitionItem, DefinitionLayout, EntryContentSlice, EntryFacts,
    EntryForm, EntryInlineRoot, EntryKind, EntryNameBinding, EntryNameEvidence,
    EntryOwner, Inline, LayoutHint, ListItem, NameCase,
};

fn facts(form: EntryForm) -> EntryFacts {
    EntryFacts {
        id: "command-run".into(), kind: EntryKind::Command,
        case: NameCase::Sensitive, names: vec!["run".into()],
        forms: vec![form.clone()],
        name_bindings: vec![EntryNameBinding {
            name: 0, evidence: EntryNameEvidence::Declared,
            occurrences: vec![form],
        }],
        alias_groups: Vec::new(), alias_of: None, value_domain: None,
    }
}
fn paragraph(children: Vec<Inline>) -> Block {
    Block::Paragraph { children, layout: LayoutHint::default(), source: None }
}

// 1. Ordinary content is useful on its own, with no semantic owner.
let ordinary = ListItem {
    layout: mant_ir::ListItemLayout::default(),
    source: None, entry: None,
    blocks: vec![paragraph(vec![
        Inline::Code { value: "run".into() },
        Inline::Text { value: ": Start the task.".into() },
    ])],
};
assert!(EntryOwner::List(&ordinary).facts().is_none());

// 2. The same blocks, with an explicit reference to only their displayed name.
let mut annotated = ordinary.clone();
annotated.entry = Some(facts(EntryForm { parts: vec![EntryContentSlice {
    root: EntryInlineRoot::Block { index: 0 }, path: vec![0], bytes: None,
}] }));
assert_eq!(ordinary.blocks, annotated.blocks);
assert_eq!(EntryOwner::List(&annotated).validated_names().unwrap(), ["run"]);

// 3. A source-neutral definition owns actual terms and a separate description.
// This is an alternative owner, not another node with the same ID in one tree.
let definition = DefinitionItem {
    source: None, layout: DefinitionLayout::default(),
    terms: vec![vec![Inline::Code { value: "run".into() }]],
    description: vec![paragraph(vec![Inline::Text {
        value: "Start the task.".into(),
    }])],
    entry: Some(facts(EntryForm::term(0))),
};
assert_eq!(EntryOwner::Definition(&definition).validated_names().unwrap(), ["run"]);
assert!(EntryOwner::Definition(&definition).forms().is_some());
```

`source: None` here denotes synthetic content. Parsers retain the original item
span, not the first surviving block after removing annotations. References bind
to the final owned content; complete term forms borrow the original inlines.
Invalid forms and invalid names are independent findings: neither deletes the
owner or its nested entries. Validate the completed document before consumption.

## Typed links and the document graph

`Inline::Link` carries a closed `LinkTarget` rather than an unclassified URL
string. `Section` targets stay inside the current document. `Document` and
`Manual` targets are logical cross-document edges that a resolver may follow
under explicit bounds. `External` and `Email` targets are host actions and must
not expand a documentation scope.

The IR intentionally does not resolve those edges. A `Document` target needs
the referring `DocumentAddress` so `mant-engine` can keep relative links inside
their registered source; a `Manual` target still requires catalog lookup and
explicit ambiguity handling. Renderers that cannot activate a target should
preserve the link's visible children.

`LinkTarget::from_uri` and `LinkTarget::to_uri` share the single-pass URI boundary
used by Markdown and reference copying. Display labels are not link addresses:
serialization restores the equivalent `.md` suffix, encodes decoded components,
and distinguishes a manual topic containing parentheses from an explicit section.
Relative document addresses still refer to the original source namespace, not an
absolute catalog identity. Unrepresentable targets return `None` instead of a
modified destination; parsing preserves invalid input for ordinary IR diagnostics.
These helpers neither resolve a target nor authorize opening it.

`DocumentReference` is the document/manual subset shared by independent
entry-set relationships and scope traversal. It is not a second visible-link
store. `scan_navigation_scope` traverses original heading/body content under
one `ReferenceWorkBudget`, emitting selected link occurrences, optional exact
targets and independent entry-set relations. `ContentLocation` resolves original
inline paths; `ContentReveal` can also identify zero-width destinations without
pretending they are readable semantic entries. Consumers charge optional labels,
associations and materialization before copying, and must not report incomplete
scans as complete inventories. No visitor opens files or activates links.

`NodeId` is always the normalized internal identity used by indexes and typed
local links. A document root, section, or inline anchor may additionally carry
exact `FragmentAlias` values contributed by source syntax such as mdoc `.Tg`
or a Markdown heading ID. Those aliases preserve external deep links without
weakening the normalized-ID invariant. `DocumentIndex::fragment_target`
resolves either form only when it identifies one canonical target.
Inline anchors may additionally retain the `SourceSpan` of their addressable
owner. That provenance identifies the paragraph, definition, item, or cell to
which a zero-width destination belongs even when the anchor is physically
nested inside a descendant inline sequence.

## Stability boundary

`TableGrid` places table cells in logical columns without allocating by span
width. It is shared by CLI and TUI table projection; bounded slot expansion
retains empty covered columns instead of shifting later cells to the left.

This is a typed Rust library contract for trusted in-process components. Its
Serde representation supports projections and tests, but serializing an IR
type directly does not create a stable process protocol. External consumers
should use `mant-protocol`, whose envelopes carry explicit schema identifiers
and compatibility rules.

Versioned CLI JSON contracts and compact MCP query projections live in
[`mant-protocol`](https://crates.io/crates/mant-protocol); parsing and document
operations live in [`mant-engine`](https://crates.io/crates/mant-engine).
The complete node and stability reference is
[`mant-ir(7)`](https://github.com/BryanHeBY/ManT/blob/main/docs/manuals/mant-ir.md).
Compatibility and migration notes are recorded in the
[crate changelog](https://github.com/BryanHeBY/ManT/blob/main/CHANGELOG.md).

## License

Apache-2.0.
