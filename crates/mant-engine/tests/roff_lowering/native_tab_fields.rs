//! Native field TABs resolve once before publishing source-neutral owners.

use mant_ir::{
    Block, Inline, inline_plain_text,
    visit::{self, Visit},
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    count: usize,
    cases: Vec<Case>,
}

#[derive(Clone, Deserialize)]
struct Case {
    id: String,
    source: String,
    source_sha256: String,
    kind: String,
    source_word: String,
    period: usize,
    control: bool,
    expected_rows: Vec<String>,
}

fn inset_separator_column(case: &Case) -> Option<usize> {
    let hash = match (case.id.as_str(), case.period) {
        ("detached-head-3", 3) => {
            "cde1dcd5f07816868172c38f2d4e3c1af37f98a601f0bfaa08b8f221c62dc0c3"
        }
        ("detached-head-5", 5) => {
            "63b299c7b78739c474c20cd31f0574d2a902b462fc206617563704cb294615ea"
        }
        ("detached-head-8", 8) => {
            "5841378f2726baa075aa1470f6a4033068eab86d4576fa932f33829d51fba05f"
        }
        _ => return None,
    };
    let source = format!(
        ".Dd November 3, 2026\n.Dt TAB-FIELD 1\n.Os\n.Sh DESCRIPTION\n.Bl -inset\n.It \"A\t\"\n.ta T {}n\n.No B\n.El\n.Sh NEXT\n.No END\n",
        case.period
    );
    let row = format!("     A{}\u{a0}B", " ".repeat(case.period - 1));
    (case.kind == "inset"
        && case.source == source
        && case.source_sha256 == hash
        && case.expected_rows == [row])
    .then_some(5 + case.period)
}

fn find_ast(
    node: &libmandoc_rs::Node,
    predicate: impl Fn(&libmandoc_rs::Node) -> bool + Copy,
) -> Option<&libmandoc_rs::Node> {
    if predicate(node) {
        Some(node)
    } else {
        node.children
            .iter()
            .find_map(|child| find_ast(child, predicate))
    }
}

fn expected_rows(case: &Case, ast: &libmandoc_rs::Node) -> Vec<String> {
    let mut expected = case.expected_rows.clone();
    if case.kind != "inset" {
        return expected;
    }
    // mdoc_term.c:760-766 writes a generated "\\ " at this It BODY,
    // matching the existing native_replacements projection for inset/diag.
    // These three complete sources contain no authored NBSP or escape; the
    // frozen raw row pins the one generated scalar's exact column. All other
    // NBSP, source scalars, owners, rows and word seams remain untouched.
    let column = inset_separator_column(case).expect("exact generated inset separator card");
    let item = find_ast(ast, |node| {
        node.kind == libmandoc_rs::NodeKind::Block
            && node.macro_token.as_deref() == Some("It")
            && node.line == 6
            && node.column == 2
    })
    .unwrap();
    let head = item
        .children
        .iter()
        .find(|node| node.kind == libmandoc_rs::NodeKind::Head)
        .unwrap();
    let body = item
        .children
        .iter()
        .find(|node| node.kind == libmandoc_rs::NodeKind::Body)
        .unwrap();
    assert!(
        find_ast(head, |node| node.text.as_deref() == Some("A\t")
            && node.line == 6
            && node.column == 5)
        .is_some()
    );
    assert!(
        find_ast(body, |node| node.text.as_deref() == Some("B")
            && node.line == 8
            && node.column == 5)
        .is_some()
    );
    assert_eq!(
        expected[0]
            .chars()
            .filter(|character| *character == '\u{a0}')
            .count(),
        1
    );
    assert_eq!(expected[0].chars().nth(column), Some('\u{a0}'));
    expected[0].replace_range(column..column + '\u{a0}'.len_utf8(), " ");
    expected
}

fn responsive(row: &str) -> String {
    // Same existing native matrix policy: generated ASCII padding responds;
    // every hard row, NBSP and zero/nonzero word separator stays observable.
    super::control_request_matrix::responsive_cells(row)
}

fn original_tab_count(node: &libmandoc_rs::Node) -> usize {
    node.text
        .as_deref()
        .unwrap_or_default()
        .matches('\t')
        .count()
        + node.children.iter().map(original_tab_count).sum::<usize>()
}

struct Tabs(usize);
impl<'ir> Visit<'ir> for Tabs {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        match inline {
            Inline::Text { value } | Inline::Code { value } => {
                self.0 += value.matches('\t').count();
            }
            _ => {}
        }
        visit::walk_inline(self, inline);
    }
}

fn check_owner(case: &Case, block: &Block) {
    if case.kind == "column" {
        let Block::Table { rows, .. } = block else {
            panic!("{}: original table owner", case.id)
        };
        assert_eq!(rows.len(), 1, "{}", case.id);
        assert_eq!(rows[0].cells.len(), 2, "{}", case.id);
        if !case.control {
            let [
                Block::Paragraph {
                    children, source, ..
                },
            ] = rows[0].cells[0].blocks.as_slice()
            else {
                panic!("{}: original paragraph root", case.id)
            };
            assert_eq!(
                source.as_ref().map(|source| (source.line, source.column)),
                Some((8, 2)),
                "{}",
                case.id
            );
            let mut expected = String::new();
            let mut column = 0;
            for character in case.source_word.chars() {
                if character == '\t' {
                    let spaces = case.period - column % case.period;
                    expected.extend(std::iter::repeat_n(' ', spaces));
                    column += spaces;
                } else {
                    expected.push(character);
                    column += 1;
                }
            }
            expected.push_str(" AFTER");
            assert_eq!(
                inline_plain_text(children),
                expected,
                "{}: resolved units in original owner",
                case.id
            );
        }
    } else {
        let Block::DefinitionList { items, .. } = block else {
            panic!("{}: detached HEAD owner", case.id)
        };
        assert_eq!(items.len(), 1, "{}", case.id);
        assert_eq!(items[0].terms.len(), 1, "{}", case.id);
        assert_eq!(
            inline_plain_text(&items[0].terms[0]),
            format!("A{}", " ".repeat(case.period - 1)),
            "{}: late ta reaches detached HEAD",
            case.id
        );
        assert_eq!(
            items[0]
                .source
                .as_ref()
                .map(|source| (source.line, source.column)),
            Some((6, 2))
        );
        let [
            Block::Paragraph {
                children, source, ..
            },
        ] = items[0].description.as_slice()
        else {
            panic!("{}: original BODY paragraph owner", case.id)
        };
        // append_run_in_cells retains the generated BODY separator in this
        // exact owner as ASCII space, independently of the HEAD's tab pad.
        assert_eq!(inline_plain_text(children), " B");
        assert_eq!(
            source.as_ref().map(|source| (source.line, source.column)),
            Some((8, 2))
        );
    }
}

fn json_roundtrip(id: &str, source: &str) -> mant_ir::ResolvedContent {
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let bundle = mant_protocol::QueryBundle::from(&query);
    let value = serde_json::to_value(&bundle).unwrap();
    let json = serde_json::to_string(&bundle).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        value,
        "{id}: actual JSON string roundtrip"
    );
    decoded.into()
}

#[test]
fn native_tab_fields_resolve_stops_controls_styles_and_detached_head_owners() {
    // All 51 complete sources ran pinned ASCII/UTF-8(width78)/HTML/tree/lint
    // before these assertions. mdoc_term.c:257-259 and roff_term.c:219-221
    // configure stops; term.c:394-427 prints one deferred advance only at a
    // real graph; term.c:475-481 retires graphless TABs without a new glyph.
    // The inset HEAD returns before BODY print (mdoc_term.c:939-945), so a
    // later .ta still controls its TAB. Its original AST/source is immutable.
    let fixture: Fixture =
        serde_json::from_str(include_str!("native_tab_fields/cases.json")).unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture.count, 51);
    assert_eq!(fixture.cases.len(), fixture.count);
    for case in fixture.cases {
        let parsed = libmandoc_rs::Parser::default()
            .parse_bytes("tab-field.1", case.source.as_bytes())
            .unwrap();
        assert_eq!(
            original_tab_count(&parsed.document.root),
            case.source_word.matches('\t').count(),
            "{}: authored AST scalars",
            case.id
        );
        let content = json_roundtrip(&case.id, &case.source);
        let document = content.document.as_ref().unwrap();
        let mut tabs = Tabs(0);
        tabs.visit_document(document);
        assert_eq!(tabs.0, 0, "{}: no native TAB instruction escapes", case.id);
        check_owner(&case, &document.sections[0].blocks[0]);
        let text = mant_render::render_query_man(&content);
        let region = text
            .split_once("DESCRIPTION\n")
            .unwrap()
            .1
            .split_once("NEXT\n")
            .unwrap()
            .0;
        let mut actual: Vec<_> = region.split_terminator('\n').map(responsive).collect();
        assert_eq!(
            actual.pop().as_deref(),
            Some(""),
            "{}: final section gap",
            case.id
        );
        let expected: Vec<_> = expected_rows(&case, &parsed.document.root)
            .iter()
            .map(|row| responsive(row))
            .collect();
        assert_eq!(actual, expected, "{}: exact rows and word seams", case.id);
    }
}

#[test]
fn generated_inset_separator_permission_rejects_authored_nonbreaking_blanks() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("native_tab_fields/cases.json")).unwrap();
    let cases: Vec<_> = fixture
        .cases
        .into_iter()
        .filter(|case| case.kind == "inset")
        .collect();
    assert_eq!(cases.len(), 3);
    for case in cases {
        assert!(inset_separator_column(&case).is_some());
        for authored in ["\u{a0}B", r"\~B", r"\0B"] {
            let mut changed = case.clone();
            changed.source = changed.source.replace(".No B", &format!(".No {authored}"));
            assert!(
                inset_separator_column(&changed).is_none(),
                "{}: authored {authored:?} cannot inherit generated seam",
                case.id
            );
        }
    }
}

#[test]
fn native_empty_tab_fields_keep_their_physical_empty_row_receipts() {
    // These exact sources ran pristine ASCII/UTF-8(width78)/HTML/tree/lint
    // first. term_fill() accepts a TAB-only field, but term_field() leaves
    // its deferred padding unprinted. Its final vbr and
    // trailspace still run the term.c:250-253 row-tail comparison. The
    // column BODY post calls term_flushln() (mdoc_term.c:946-948), so width1
    // owns a real empty row; width4/8 leave the row open for RIGHT. No raw
    // TAB in the executed IR may stand in for either physical-row receipt.
    for (width, source, native_rows) in [
        (
            1,
            include_str!("native_tab_fields/empty-tab-1.1"),
            vec!["", "          RIGHT"],
        ),
        (
            4,
            include_str!("native_tab_fields/empty-tab-4.1"),
            vec!["             RIGHT"],
        ),
        (
            8,
            include_str!("native_tab_fields/empty-tab-8.1"),
            vec!["                 RIGHT"],
        ),
    ] {
        let parsed = libmandoc_rs::Parser::default()
            .parse_bytes("empty-tab.1", source.as_bytes())
            .unwrap();
        assert_eq!(original_tab_count(&parsed.document.root), 1);
        assert!(
            find_ast(&parsed.document.root, |node| {
                node.text.as_deref() == Some("\t") && node.line == 7 && node.column == 5
            })
            .is_some()
        );
        let content = json_roundtrip("empty-tab-field", source);
        let document = content.document.as_ref().unwrap();
        let mut tabs = Tabs(0);
        tabs.visit_document(document);
        assert_eq!(tabs.0, 0);
        let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
            panic!("original empty field table owner");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].cells.len(), 2);
        let empty = &rows[0].cells[0];
        assert_eq!(empty.break_after, width == 1);
        if width == 1 {
            assert!(matches!(
                empty.blocks.as_slice(),
                [Block::VerticalSpace { lines: 1, .. }]
            ));
        } else {
            assert_eq!(empty.blocks, [] as [Block; 0]);
        }
        let text = mant_render::render_query_man(&content);
        let region = text
            .split_once("DESCRIPTION\n")
            .unwrap()
            .1
            .split_once("NEXT\n")
            .unwrap()
            .0;
        let mut actual: Vec<_> = region.split_terminator('\n').map(responsive).collect();
        assert_eq!(actual.pop().as_deref(), Some(""));
        assert_eq!(
            actual,
            native_rows.into_iter().map(responsive).collect::<Vec<_>>(),
            "width{width}: complete physical rows"
        );
    }
}

#[test]
fn structured_equation_tab_value_and_expression_remain_one_projection() {
    // This exact quoted eqn TAB source ran all five pristine profiles before
    // the assertion. eqn_term.c::eqn_box passes its text to term_word; the
    // producer retains a structured equation and its matching readable value.
    // Native field Text/Code TAB normalization must not rewrite one half of
    // that mathematical representation or invent a second equation owner.
    let source = include_str!("native_tab_fields/quoted_equation.1");
    let content = json_roundtrip("quoted-equation-tab", source);
    let document = content.document.as_ref().unwrap();
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("original equation table owner");
    };
    let Block::Equation {
        value,
        expression,
        source,
        ..
    } = &rows[0].cells[0].blocks[0]
    else {
        panic!("original structured equation owner");
    };
    assert_eq!(value, "A\tB");
    assert_eq!(value, &expression.as_ref().unwrap().readable_text());
    assert_eq!(
        source.as_ref().map(|source| (source.line, source.column)),
        Some((7, 2))
    );
    assert_eq!(rows[0].cells.len(), 2);
}
