//! Exact small-budget coverage for the shared native ownership boundary.

use super::{
    super::session,
    budget::{MAX_OWNED_SYNTAX_BYTES, MAX_OWNED_SYNTAX_ITEMS, with_remaining_budget},
};
use crate::{
    Document, IncludePolicy, InputFormat, Node, NodeKind, ParseOptions, Parser, RawDocument,
    SourceBundle, TableRowKind,
};
use std::{
    collections::HashSet,
    ffi::CString,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const HEADER: &str = ".Dd January 1, 2020\n.Dt BUDGET 1\n.Os Review\n.Sh BODY\n";
type EnclosureSpelling = (Option<&'static str>, Option<&'static str>);
const ENCLOSURES: &[(&str, &str, &[EnclosureSpelling])] = &[
    ("opening", ".Es ABC\n.En x\n", &[(Some("ABC"), None)]),
    (
        "closing",
        ".Es \"\" XYZ\n.En x\n",
        &[(Some(""), Some("XYZ"))],
    ),
    (
        "both-redefine",
        ".Es AB CD\n.En x\n.En y\n.Es EF GH\n.En z\n",
        &[
            (Some("AB"), Some("CD")),
            (Some("AB"), Some("CD")),
            (Some("EF"), Some("GH")),
        ],
    ),
    ("empty", ".Es \"\" \"\"\n.En x\n", &[(Some(""), Some(""))]),
    (
        "escaped",
        ".Es \\(la \\(ra\n.En x\n",
        &[(Some("\\(la"), Some("\\(ra"))],
    ),
    ("missing", ".Es\n.En x\n", &[(None, None)]),
];
const FIELDS: &str = ".Tg explicit-anchor\n.Bl -tag -width sample -offset indent\n.It tagged\nbody\n.El\n.Bl -column AA BBB\n.It first Ta second\n.El\n.Bd -literal -offset left\nwell-known\n.Ed\n";
const CELLS: &[u8] = b".TH BUDGET 1\n.SH BODY\n.TS\nl.\nwell-known\n.TE\n";

fn native_report(path: &str, source: &[u8]) -> RawDocument {
    session::parse_buffer(
        &CString::new(path).unwrap(),
        source,
        None,
        false,
        InputFormat::Auto,
        None,
    )
    .unwrap()
}

/// Independently total the retained public facts, including the historical
/// spelling charge for known macros. Shared table layouts own one allocation.
fn transfer_usage(raw: &RawDocument) -> (usize, usize) {
    fn visit(node: &Node, items: &mut usize, bytes: &mut usize, layouts: &mut HashSet<usize>) {
        *items += 1;
        *bytes += std::mem::size_of::<Node>();
        *bytes += [
            node.text.as_deref(),
            node.native_text.as_deref(),
            node.tag.as_deref(),
            node.offset.as_deref(),
            node.width.as_deref(),
        ]
        .into_iter()
        .flatten()
        .map(str::len)
        .sum::<usize>();
        *bytes += node
            .macro_token
            .as_ref()
            .map_or(0, |token| token.as_str().len());
        if let Some(enclosure) = &node.enclosure {
            *bytes += enclosure.opening.len() + enclosure.closing.as_deref().map_or(0, str::len);
        }
        *items += node.columns.len();
        *bytes += node.columns.len() * std::mem::size_of::<String>()
            + node.columns.iter().map(String::len).sum::<usize>();
        *items += node.table_cells.len();
        *items += node.table_rule_data_columns.len();
        *bytes += node.table_rule_data_columns.len() * std::mem::size_of::<u32>();
        *bytes += node.table_cells.len() * std::mem::size_of::<crate::TableCell>();
        for cell in &node.table_cells {
            *bytes += cell.text.as_deref().map_or(0, str::len)
                + cell.native_text.as_deref().map_or(0, str::len);
        }
        if let Some(TableRowKind::LayoutRule { cells }) = &node.table_row_kind {
            *items += cells.len();
            *bytes += cells.len() * std::mem::size_of::<crate::TableRuleCellKind>();
        }
        if let Some(layout) = &node.table_layout
            && layouts.insert(Arc::as_ptr(layout) as usize)
        {
            *items += 1 + layout.cells.len();
            *bytes += std::mem::size_of::<crate::TableLayout>()
                + layout.cells.len() * std::mem::size_of::<crate::TableLayoutCell>();
        }
        assert!(
            node.equation.is_none(),
            "equations use their independent truncation budget"
        );
        for child in &node.children {
            visit(child, items, bytes, layouts);
        }
    }
    let mut items = 0;
    let metadata = &raw.document.metadata;
    let mut bytes = raw.diagnostics.len()
        + [
            metadata.title.as_deref(),
            metadata.section.as_deref(),
            metadata.volume.as_deref(),
            metadata.os.as_deref(),
            metadata.arch.as_deref(),
            metadata.name.as_deref(),
            metadata.date.as_deref(),
            metadata.alias_target.as_deref(),
        ]
        .into_iter()
        .flatten()
        .map(str::len)
        .sum::<usize>();
    visit(
        &raw.document.root,
        &mut items,
        &mut bytes,
        &mut HashSet::new(),
    );
    (items, bytes)
}

fn nodes(node: &Node) -> Vec<&Node> {
    let mut result = vec![node];
    for child in &node.children {
        result.extend(nodes(child));
    }
    result
}

fn assert_exact_boundaries(path: &str, source: &[u8]) -> Document {
    let raw = native_report(path, source);
    let (items, bytes) = transfer_usage(&raw);
    assert!(
        items < 100 && bytes < 64 * 1024,
        "ordinary tests must stay small"
    );
    let parser = Parser::default();
    let accepted =
        with_remaining_budget(items, bytes, || parser.parse_bytes(path, source)).unwrap();
    assert_eq!(accepted.document, raw.document);
    for (item_limit, byte_limit) in [(items, bytes - 1), (items - 1, bytes), (items, bytes / 2)] {
        let error =
            with_remaining_budget(item_limit, byte_limit, || parser.parse_bytes(path, source))
                .unwrap_err();
        assert!(
            error.message.contains("cumulative node/byte budget"),
            "{error}"
        );
        assert_eq!(
            parser.parse_bytes(path, source).unwrap().document,
            raw.document,
            "a partially copied failed session must release native storage and reset its budget"
        );
    }
    accepted.document
}

#[test]
fn enclosure_fields_each_reserve_their_owned_copies_at_exact_boundaries() {
    // Exact inputs were first run with the fixed CVS reference -Tascii.
    // mdoc_validate.c::post_en/post_es bind the active Es; roff.c's node
    // append shares normalized data with HEAD/BODY, so all three owned facts
    // retain their own strings. termp_quote_pre/post consume them once.
    for (name, body, expected) in ENCLOSURES {
        let document = assert_exact_boundaries("budget.1", format!("{HEADER}{body}").as_bytes());
        let actual = nodes(&document.root)
            .into_iter()
            .filter(|node| node.macro_token.as_deref() == Some("En"))
            .map(|node| {
                (
                    node.kind,
                    node.enclosure.as_ref().map_or((None, None), |value| {
                        (Some(value.opening.as_str()), value.closing.as_deref())
                    }),
                )
            })
            .collect::<Vec<_>>();
        let expected = expected
            .iter()
            .flat_map(|value| {
                [NodeKind::Block, NodeKind::Head, NodeKind::Body].map(|kind| (kind, *value))
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "{name}");
        #[cfg(feature = "serde")]
        {
            let encoded = serde_json::to_vec(&document).unwrap();
            assert_eq!(
                serde_json::from_slice::<Document>(&encoded).unwrap(),
                document
            );
        }
    }
}

#[test]
fn ordinary_fields_columns_metadata_diagnostics_and_table_cells_share_reservations() {
    // Exact sources were first rendered by the pinned CVS. The original
    // native spelling is retained for roff.c's ASCII_HYPH normalization.
    let document = assert_exact_boundaries("budget.1", format!("{HEADER}{FIELDS}").as_bytes());
    let all = nodes(&document.root);
    // post_tg() supplies an explicit identity; tag.c::tag_put() stores a
    // separate tag string only when it differs from the node's own text.
    assert!(
        all.iter()
            .any(|node| node.tag.as_deref() == Some("explicit-anchor"))
    );
    assert!(all.iter().any(|node| node.offset.is_some()));
    assert!(all.iter().any(|node| node.width.is_some()));
    assert!(all.iter().any(|node| !node.columns.is_empty()));
    let hyphenated = all
        .iter()
        .find(|node| node.text.as_deref() == Some("well-known"))
        .unwrap();
    assert_eq!(hyphenated.native_text.as_deref(), Some("well\u{1c}known"));
    assert_eq!(document.metadata.title.as_deref(), Some("BUDGET"));
    assert_ne!(
        native_report("budget.1", format!("{HEADER}{FIELDS}").as_bytes()).diagnostics,
        ""
    );
    let cells = assert_exact_boundaries("budget.1", CELLS);
    assert!(
        nodes(&cells.root)
            .iter()
            .flat_map(|node| &node.table_cells)
            .any(|cell| cell.text.as_deref() == Some("well-known"))
    );
}

#[test]
fn bytes_bundle_and_root_file_parses_reject_exhaustion_and_recover() {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let directory = std::env::temp_dir().join(format!(
        "mant-owned-budget-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&directory).unwrap();
    let scratch = Scratch(directory);
    let source = format!("{HEADER}.Es AB CD\n.En x\n.En y\n.Es EF GH\n.En z\n");
    let mut bundle = SourceBundle::new();
    bundle
        .insert("budget.1", source.as_bytes().to_vec())
        .unwrap();
    let file = scratch.0.join("budget.1");
    fs::write(&file, &source).unwrap();
    let root_parser = Parser::new(ParseOptions {
        includes: IncludePolicy::Root(scratch.0.clone()),
        ..ParseOptions::default()
    });
    let parser = Parser::default();
    for entry in 0..3 {
        let path = if entry == 2 {
            file.to_str().unwrap()
        } else {
            "budget.1"
        };
        // Measure each native entry itself: the file reader's diagnostics
        // include its filename, while memory and bundle capture differ.
        let c_path = CString::new(path).unwrap();
        let raw = match entry {
            0 => native_report(path, source.as_bytes()),
            1 => session::parse_bundle(&c_path, &bundle, InputFormat::Auto, None).unwrap(),
            _ => {
                #[cfg(unix)]
                {
                    let root = CString::new(scratch.0.to_str().unwrap()).unwrap();
                    session::parse_file(&c_path, Some(&root), true, InputFormat::Auto, None)
                        .unwrap()
                }
                #[cfg(windows)]
                {
                    session::parse_buffer(
                        &c_path,
                        source.as_bytes(),
                        Some(&scratch.0),
                        true,
                        InputFormat::Auto,
                        None,
                    )
                    .unwrap()
                }
            }
        };
        let (items, bytes) = transfer_usage(&raw);
        let parse = || match entry {
            0 => parser.parse_bytes(path, source.as_bytes()),
            1 => parser.parse_bundle("budget.1", &bundle),
            _ => root_parser.parse_file(&file),
        };
        assert_eq!(
            with_remaining_budget(items, bytes, parse).unwrap().document,
            raw.document
        );
        for (item_limit, byte_limit) in [(items, bytes - 1), (items - 1, bytes), (items, bytes / 2)]
        {
            let error = with_remaining_budget(item_limit, byte_limit, parse).unwrap_err();
            assert!(
                error.message.contains("cumulative node/byte budget"),
                "entry {entry}: {error}"
            );
            assert_eq!(parse().unwrap().document, raw.document);
        }
    }
}

#[test]
fn syntax_items_and_bytes_are_independent_and_overflow_rejects_without_consumption() {
    use super::budget::TransferBudget;
    with_remaining_budget(0, 4, || {
        let mut budget = TransferBudget::default();
        budget.reserve_bytes(4).unwrap();
        assert!(budget.reserve_items(1).is_err());
        assert_eq!(budget.items, MAX_OWNED_SYNTAX_ITEMS);
        assert_eq!(budget.bytes, MAX_OWNED_SYNTAX_BYTES);
    });
    with_remaining_budget(1, 0, || {
        let mut budget = TransferBudget::default();
        budget.reserve_items(1).unwrap();
        assert!(budget.reserve_bytes(1).is_err());
        assert_eq!(budget.items, MAX_OWNED_SYNTAX_ITEMS);
        assert_eq!(budget.bytes, MAX_OWNED_SYNTAX_BYTES);
    });
    let mut budget = TransferBudget::default();
    assert!(budget.reserve_items(usize::MAX).is_err());
    assert!(budget.reserve_bytes(usize::MAX).is_err());
    assert_eq!((budget.items, budget.bytes), (0, 0));
}
