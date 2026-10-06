//! Exact pristine sources keep hard rows and lexical boundaries independent.

use std::io::Read as _;

use libmandoc_rs::{Node, NodeKind};
use serde::Deserialize;

#[derive(Deserialize)]
struct Header {
    count: usize,
    unique_sources: usize,
    expectations_from_product: bool,
    oracle_sha256: String,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    family: String,
    source: String,
    oracle_class: String,
    native_owner: Owner,
    expected_rows: Vec<String>,
    projection: serde_json::Value,
    #[serde(default)]
    authored_fixed_blank: Option<Position>,
}

#[derive(Deserialize)]
struct Owner {
    reachable: bool,
    after_owners: Vec<Position>,
    branch_nodes: Vec<Branch>,
    empty_texts: usize,
    literal_tabs: Vec<Position>,
    no_text_operand: bool,
}

#[derive(Deserialize)]
struct Position {
    line: u32,
    column: u32,
    owner: Option<String>,
    #[serde(default)]
    no_fill: bool,
}

#[derive(Deserialize)]
struct Branch {
    label: String,
    kind: String,
    line: u32,
    column: u32,
    owner: Option<String>,
    no_fill: bool,
}

fn fixture() -> (Header, Vec<Case>) {
    let mut raw = String::new();
    flate2::read::GzDecoder::new(
        include_bytes!("control_request_matrix/cases.jsonl.gz").as_slice(),
    )
    .read_to_string(&mut raw)
    .unwrap();
    let mut lines = raw.lines();
    let header = serde_json::from_str(lines.next().unwrap()).unwrap();
    let cases = lines
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    (header, cases)
}

struct ActualNode<'a> {
    node: &'a Node,
    owner: Option<&'static str>,
}

fn collect<'a>(node: &'a Node, owner: Option<&'static str>, output: &mut Vec<ActualNode<'a>>) {
    let owner = if node.macro_token.as_deref() == Some("It") {
        match node.kind {
            NodeKind::Head => Some("It HEAD"),
            NodeKind::Body => Some("It BODY"),
            _ => owner,
        }
    } else {
        owner
    };
    output.push(ActualNode { node, owner });
    for child in &node.children {
        collect(child, owner, output);
    }
}

fn at_position<'a>(nodes: &'a [ActualNode<'a>], position: &Position) -> &'a ActualNode<'a> {
    nodes
        .iter()
        .find(|actual| {
            actual.node.kind == NodeKind::Text
                && actual.node.line == position.line
                && actual.node.column == position.column
        })
        .expect("pristine TEXT source position remains reachable")
}

fn assert_owned_ast(case: &Case) {
    let parsed = libmandoc_rs::Parser::default()
        .parse_bytes("field-rule.1", case.source.as_bytes())
        .unwrap();
    let mut nodes = Vec::new();
    let document = parsed.document;
    collect(&document.root, None, &mut nodes);
    if let Some(expected) = &case.authored_fixed_blank {
        let actual = at_position(&nodes, expected);
        assert_eq!(actual.owner, expected.owner.as_deref(), "{}", case.id);
        assert_eq!(actual.node.text.as_deref(), Some(r"\0"), "{}", case.id);
    }
    for expected in &case.native_owner.after_owners {
        let actual = at_position(&nodes, expected);
        assert_eq!(actual.owner, expected.owner.as_deref(), "{}", case.id);
        assert_eq!(actual.node.flags.no_fill, expected.no_fill, "{}", case.id);
    }
    for expected in &case.native_owner.literal_tabs {
        let actual = at_position(&nodes, expected);
        assert_eq!(actual.owner, expected.owner.as_deref(), "{}", case.id);
        assert!(actual.node.text.as_deref().unwrap().contains('\t'));
    }
    let empty = nodes
        .iter()
        .filter(|actual| {
            actual.node.kind == NodeKind::Text
                && actual.node.text.as_deref().unwrap_or_default().is_empty()
        })
        .count();
    assert_eq!(empty, case.native_owner.empty_texts, "{}", case.id);
    if case.native_owner.no_text_operand {
        assert!(!case.source.lines().any(|line| line.starts_with(".No \"")));
    }
    for expected in &case.native_owner.branch_nodes {
        let kind = match expected.kind.as_str() {
            "block" => NodeKind::Block,
            "head" => NodeKind::Head,
            "body" => NodeKind::Body,
            "elem" => NodeKind::Element,
            "tbl" => NodeKind::Table,
            other => panic!("uncovered branch kind {other}"),
        };
        assert!(
            nodes.iter().any(|actual| {
                actual.node.kind == kind
                    && actual.node.line == expected.line
                    && actual.node.column == expected.column
                    && (kind == NodeKind::Table
                        || actual.node.macro_token.as_deref() == Some(&expected.label))
                    && actual.owner == expected.owner.as_deref()
                    && actual.node.flags.no_fill == expected.no_fill
            }),
            "{}: {} {} at {}:{}",
            case.id,
            expected.label,
            expected.kind,
            expected.line,
            expected.column
        );
    }
}

fn product_rows(case: &Case) -> Vec<String> {
    let query = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
    let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
    assert!(
        !json.contains("\\u0000mant:"),
        "{}: private owner escaped",
        case.id
    );
    let roundtrip: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let content: mant_ir::ResolvedContent = roundtrip.into();
    let text = mant_render::render_query_man(&content);
    let region = text
        .split_once("DESCRIPTION\n")
        .unwrap()
        .1
        .split_once("NEXT\n")
        .unwrap()
        .0;
    let mut rows: Vec<_> = region.split_terminator('\n').map(str::to_owned).collect();
    assert_eq!(rows.pop().as_deref(), Some(""), "{}: NEXT spacing", case.id);
    for seam in case.projection["product_table_seams"].as_array().unwrap() {
        let row = usize::try_from(seam["row"].as_u64().unwrap()).unwrap();
        assert_eq!(rows[row].trim_matches(' '), "INNER | CELL", "{}", case.id);
        rows[row] = "INNER CELL".into();
    }
    for site in case.projection["literal_tab_sites"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let row = usize::try_from(site["row"].as_u64().unwrap()).unwrap();
        let boundary = usize::try_from(site["scalar_boundary"].as_u64().unwrap()).unwrap();
        let tab = rows[row]
            .find('\t')
            .expect("literal tab remains in public projection");
        assert_eq!(
            rows[row][..tab].chars().filter(|ch| *ch != ' ').count(),
            boundary,
            "{}",
            case.id
        );
    }
    rows
}

fn responsive_cells(row: &str) -> String {
    // Only ordinary device padding and tab expansion are responsive. Authored
    // NBSP, scalars, every hard row and zero/nonzero word seam remain exact.
    let mut output = String::new();
    let mut pending = false;
    for ch in row.trim_matches([' ', '\t']).chars() {
        if matches!(ch, ' ' | '\t') {
            pending = true;
        } else {
            if pending {
                output.push(' ');
            }
            output.push(ch);
            pending = false;
        }
    }
    output
}

#[test]
fn every_pre_br_identity_replays_real_ast_json_and_physical_rows() {
    // All 3,906 complete sources ran the registered pristine five-profile
    // oracle before this fixture was frozen. roff_term.c:69-78,233-236 and
    // term.c:113-253,475-481 define old-buffer acceptance and actual row ends.
    let (header, cases) = fixture();
    assert_eq!((header.count, header.unique_sources), (3906, 3890));
    assert!(!header.expectations_from_product);
    assert_eq!(
        header.oracle_sha256,
        "482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6"
    );
    assert_eq!(cases.len(), header.count);
    assert_eq!(
        cases
            .iter()
            .filter(|case| case.family == "field-pre-br-core")
            .count(),
        2880
    );
    let mut failures = Vec::new();
    for case in cases {
        assert!(
            matches!(case.oracle_class.as_str(), "legal" | "diagnosed"),
            "{}",
            case.id
        );
        assert!(
            case.native_owner.reachable,
            "{}: unreachable AST branch",
            case.id
        );
        assert_owned_ast(&case);
        let actual: Vec<_> = product_rows(&case)
            .iter()
            .map(|row| responsive_cells(row))
            .collect();
        let expected: Vec<_> = case
            .expected_rows
            .iter()
            .map(|row| responsive_cells(row))
            .collect();
        if actual != expected {
            failures.push(format!(
                "{}\nnative {expected:?}\nproduct {actual:?}",
                case.id
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} field-rule failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn authored_fixed_blanks_survive_run_in_scope_and_output_row_boundaries() {
    // These exact 32 retained sources ran all five pristine profiles before
    // assertions. Inset \0 is an actual It HEAD cell; diagnosed diag parses
    // Xo as a literal HEAD and \0 inside It BODY. mdoc_term.c:760-774 adds
    // separate BODY cells, while term_field():389-427 prints the authored
    // Unicode fixed blank as an encoded graph, including at physical row end.
    #[derive(Deserialize)]
    struct FixedBlankFixture {
        header: Header,
        cases: Vec<Case>,
    }
    let fixture: FixedBlankFixture =
        serde_json::from_str(include_str!("control_request_matrix/fixed_blank_rows.json")).unwrap();
    assert_eq!(
        (fixture.header.count, fixture.header.unique_sources),
        (32, 32)
    );
    assert!(!fixture.header.expectations_from_product);
    assert_eq!(
        fixture.header.oracle_sha256,
        "6297105d1370a44fd306851ae5beab3f23492d756a682f8a0693896922249ea0"
    );
    assert_eq!(fixture.cases.len(), fixture.header.count);
    for case in fixture.cases {
        assert_owned_ast(&case);
        let actual: Vec<_> = product_rows(&case)
            .iter()
            .map(|row| responsive_cells(row))
            .collect();
        let expected: Vec<_> = case
            .expected_rows
            .iter()
            .map(|row| responsive_cells(row))
            .collect();
        assert_eq!(
            actual, expected,
            "{}: fixed glyph, exact hard rows",
            case.id
        );
        assert_eq!(
            actual.join("\n").matches('\u{a0}').count(),
            1,
            "{}",
            case.id
        );
    }
}

#[test]
fn kept_field_cells_follow_the_real_pass_scope_after_native_normalization() {
    // Each source ran all five registered pristine profiles first. Bk's
    // PREKEEP/KEEP emits direct ASCII_NBRSP cells (mdoc_term.c:1921-1942,
    // term.c:573-588), distinct from authored Unicode fixed blanks. Its
    // actual term_fill scan changes them to SP before the overflow test
    // (340-349); subsequent pass skips/tails use those changed bytes.
    #[derive(Deserialize)]
    struct KeptFieldFixture {
        header: Header,
        cases: Vec<Case>,
    }
    let fixture: KeptFieldFixture =
        serde_json::from_str(include_str!("control_request_matrix/kept_field_rows.json")).unwrap();
    assert_eq!(
        (fixture.header.count, fixture.header.unique_sources),
        (72, 72)
    );
    assert!(!fixture.header.expectations_from_product);
    assert_eq!(fixture.cases.len(), fixture.header.count);
    for case in fixture.cases {
        assert_owned_ast(&case);
        let actual: Vec<_> = product_rows(&case)
            .iter()
            .map(|row| responsive_cells(row))
            .collect();
        let expected: Vec<_> = case
            .expected_rows
            .iter()
            .map(|row| responsive_cells(row))
            .collect();
        assert_eq!(
            actual, expected,
            "{}: accepted words and physical rows",
            case.id
        );
    }
}

#[test]
fn pending_glyph_prefixes_and_rejected_suffixes_keep_their_native_owners() {
    // Every exact source ran registered pristine ASCII/UTF-8/HTML/tree/lint
    // before these assertions. encode1() first buffers the zero-advance graph
    // and then arms BACKBEFORE (term.c:901-927). term_flushln() accepts ordered
    // prefixes before rejecting a later pass (143-146,217,233-253), while
    // term_field() prints the accepted graph's own deferred pad (389-427).
    // The HEAD post must consume one receipt without erasing an accepted
    // pending glyph or attributing its padding to a previous source owner.
    #[derive(Deserialize)]
    struct PendingPrefixFixture {
        header: Header,
        cases: Vec<PendingPrefixCase>,
    }
    #[derive(Deserialize)]
    struct PendingPrefixCase {
        #[serde(flatten)]
        case: Case,
        kind: String,
        width: u16,
        carrier: String,
        entry: String,
        pattern: String,
    }
    let fixture: PendingPrefixFixture = serde_json::from_str(include_str!(
        "control_request_matrix/pending_prefix_rows.json"
    ))
    .unwrap();
    assert_eq!(
        (fixture.header.count, fixture.header.unique_sources),
        (540, 540)
    );
    assert!(!fixture.header.expectations_from_product);
    assert_eq!(
        fixture.header.oracle_sha256,
        "6297105d1370a44fd306851ae5beab3f23492d756a682f8a0693896922249ea0"
    );
    assert_eq!(fixture.cases.len(), fixture.header.count);
    let mut combinations = std::collections::BTreeSet::new();
    let mut failures = Vec::new();
    for pending in fixture.cases {
        let case = pending.case;
        assert_eq!(case.oracle_class, "legal", "{}", case.id);
        assert!(matches!(pending.kind.as_str(), "tag" | "hang"));
        assert!(matches!(pending.width, 0 | 2 | 4));
        assert!(matches!(
            pending.carrier.as_str(),
            "No" | "Em" | "Sy" | "Li" | "Lk"
        ));
        assert!(matches!(
            pending.entry.as_str(),
            "fresh-head" | "committed-head"
        ));
        assert!(combinations.insert((
            pending.kind,
            pending.width,
            pending.carrier,
            pending.entry,
            pending.pattern,
        )));
        assert_owned_ast(&case);
        let actual: Vec<_> = product_rows(&case)
            .iter()
            .map(|row| responsive_cells(row))
            .collect();
        let expected: Vec<_> = case
            .expected_rows
            .iter()
            .map(|row| responsive_cells(row))
            .collect();
        if actual != expected {
            failures.push(format!(
                "{}\nnative {expected:?}\nproduct {actual:?}",
                case.id
            ));
        }
    }
    assert_eq!(combinations.len(), 2 * 3 * 5 * 2 * 9);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
