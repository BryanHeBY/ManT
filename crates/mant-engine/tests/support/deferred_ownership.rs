//! Independent source members, actual rich scalar ranges and their receivers.
use mant_ir::{Block, Inline, LinkTarget, ResolvedContent, Section};
use serde::Deserialize;
use serde_json::Value;
#[path = "deferred_ownership/members.rs"]
mod members;
pub use members::check_portable;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct Style {
    pub strong: bool,
    pub emphasis: bool,
    pub code: bool,
}
#[derive(Clone, Debug, Deserialize)]
pub struct ExpectedLink {
    pub target: LinkTarget,
    pub text: String,
    pub style: Style,
    pub source: Vec<Value>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Recovery {
    pub text: String,
    pub ordinal: usize,
    pub occurrences: usize,
    pub style: Style,
    pub link: Option<usize>,
    pub source: Value,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Case {
    pub id: String,
    pub family: String,
    pub source: String,
    pub links: Vec<ExpectedLink>,
    pub recoveries: Vec<Recovery>,
    pub reading_rows: Option<Vec<String>>,
    pub diagnostics: Option<Vec<Value>>,
    pub old_x: Option<Value>,
    pub constraints: Vec<Value>,
    pub after_style: Style,
    pub facts: Option<Value>,
}
#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}
pub fn cases() -> Vec<Case> {
    let cases = serde_json::from_str::<Fixture>(include_str!(
        "../roff_lowering/deferred_ownership/cases.json"
    ))
    .unwrap()
    .cases;
    assert_eq!(cases.iter().filter(|case| case.family == "D1").count(), 180);
    assert_eq!(cases.iter().filter(|case| case.family == "D2").count(), 39);
    cases
}
pub fn description(content: &ResolvedContent) -> &Section {
    content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| mant_ir::inline_plain_text(&section.heading.content) == "DESCRIPTION")
        .unwrap()
}
#[derive(Debug)]
pub struct Cell {
    pub value: char,
    pub style: Style,
    pub link: Option<usize>,
    pub owner: String,
}
#[derive(Debug)]
pub struct Link {
    pub target: LinkTarget,
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub owner: String,
}
#[derive(Default, Debug)]
pub struct Snapshot {
    pub cells: Vec<Cell>,
    pub links: Vec<Link>,
    pub codes: Vec<String>,
}
impl Snapshot {
    pub fn text(&self) -> String {
        self.cells.iter().map(|cell| cell.value).collect()
    }
    fn inline(&mut self, nodes: &[Inline], style: Style, link: Option<usize>, owner: &str) {
        for node in nodes {
            match node {
                Inline::Text { value } | Inline::Equation { value, .. } => {
                    self.cells.extend(value.chars().map(|value| Cell {
                        value,
                        style,
                        link,
                        owner: owner.into(),
                    }));
                }
                Inline::Code { value } => {
                    self.codes.push(value.clone());
                    self.cells.extend(value.chars().map(|value| Cell {
                        value,
                        style: Style {
                            code: true,
                            ..style
                        },
                        link,
                        owner: owner.into(),
                    }));
                }
                Inline::LineBreak { .. } => self.cells.push(Cell {
                    value: '\n',
                    style,
                    link,
                    owner: owner.into(),
                }),
                Inline::Anchor { .. } => {}
                Inline::Strong { children } => self.inline(
                    children,
                    Style {
                        strong: true,
                        ..style
                    },
                    link,
                    owner,
                ),
                Inline::Emphasis { children } => self.inline(
                    children,
                    Style {
                        emphasis: true,
                        ..style
                    },
                    link,
                    owner,
                ),
                Inline::Link {
                    target, children, ..
                } => {
                    let ordinal = self.links.len();
                    let start = self.cells.len();
                    self.links.push(Link {
                        target: target.clone(),
                        text: mant_ir::inline_plain_text(children),
                        start,
                        end: start,
                        owner: owner.into(),
                    });
                    self.inline(children, style, Some(ordinal), owner);
                    self.links[ordinal].end = self.cells.len();
                }
            }
        }
    }
    fn blocks(&mut self, blocks: &[Block], owner: &str) {
        for (i, block) in blocks.iter().enumerate() {
            let owner = format!("{owner}/{i}");
            match block {
                Block::Paragraph { children, .. } => self.inline(
                    children,
                    Style::default(),
                    None,
                    &format!("{owner}/paragraph"),
                ),
                Block::Preformatted { children, .. } => self.inline(
                    children,
                    Style::default(),
                    None,
                    &format!("{owner}/preformatted"),
                ),
                Block::List { items, .. } => {
                    for (i, item) in items.iter().enumerate() {
                        self.blocks(&item.blocks, &format!("{owner}/list/{i}"));
                    }
                }
                Block::DefinitionList { items, .. } => {
                    for (i, item) in items.iter().enumerate() {
                        for (j, term) in item.terms.iter().enumerate() {
                            self.inline(
                                &term.content,
                                Style::default(),
                                None,
                                &format!("{owner}/definition/{i}/term/{j}"),
                            );
                        }
                        self.blocks(&item.description, &format!("{owner}/definition/{i}/body"));
                    }
                }
                Block::Table { rows, .. } => {
                    for (i, row) in rows.iter().enumerate() {
                        for (j, cell) in row.cells.iter().enumerate() {
                            self.blocks(&cell.blocks, &format!("{owner}/row/{i}/column/{j}"));
                        }
                    }
                }
                Block::Unsupported { text, .. } => {
                    self.cells.extend(text.chars().map(|value| Cell {
                        value,
                        style: Style::default(),
                        link: None,
                        owner: owner.clone(),
                    }));
                }
                _ => {}
            }
            self.cells.push(Cell {
                value: '\n',
                style: Style::default(),
                link: None,
                owner,
            });
        }
    }
}
pub fn snapshot(content: &ResolvedContent) -> Snapshot {
    let mut snapshot = Snapshot::default();
    snapshot.blocks(&description(content).blocks, "description");
    snapshot
}
pub fn positions(text: &str, value: &str) -> Vec<usize> {
    text.match_indices(value)
        .map(|(at, _)| text[..at].chars().count())
        .collect()
}
pub fn assert_rich(content: &ResolvedContent, case: &Case, check_style: bool) {
    let actual = snapshot(content);
    let text = actual.text();
    if check_style {
        members::check(content, case, &actual);
        check_portable(
            case,
            &actual,
            &(0..case.links.len()).map(Some).collect::<Vec<_>>(),
        );
    }
    assert_eq!(
        actual.links.len(),
        case.links.len(),
        "{}: {actual:#?}",
        case.id
    );
    for (ordinal, (link, gold)) in actual.links.iter().zip(&case.links).enumerate() {
        assert_eq!(
            (&link.target, &link.text),
            (&gold.target, &gold.text),
            "{} link{ordinal}",
            case.id
        );
        if check_style {
            for cell in &actual.cells[link.start..link.end] {
                assert_eq!(
                    cell.style, gold.style,
                    "{} link{ordinal}: {cell:?}",
                    case.id
                );
            }
        }
    }
    if check_style {
        assert_native_facts(content, case, &actual);
        for expected in &case.links {
            for member in &expected.source {
                let bytes = &member["byteRange"];
                let range = usize::try_from(bytes["start"].as_u64().unwrap()).unwrap()
                    ..usize::try_from(bytes["end"].as_u64().unwrap()).unwrap();
                assert_ne!(&case.source[range], "", "{} original label member", case.id);
            }
        }
        if let Some(section) = content
            .document
            .as_ref()
            .unwrap()
            .sections
            .iter()
            .find(|section| mant_ir::inline_plain_text(&section.heading.content) == "NEXT")
        {
            let mut after = Snapshot::default();
            after.blocks(&section.blocks, "after");
            let text = after.text();
            let at = positions(&text, "AFTER");
            assert_eq!(at.len(), 1, "{} after owner", case.id);
            for cell in &after.cells[at[0]..at[0] + 5] {
                assert_eq!(cell.style, case.after_style);
                assert_eq!(cell.link, None);
            }
        }
    }
    for recovery in &case.recoveries {
        let spans = positions(&text, &recovery.text);
        assert_eq!(spans.len(), recovery.occurrences, "{}: {text}", case.id);
        let start = spans[recovery.ordinal];
        for cell in &actual.cells[start..start + recovery.text.chars().count()] {
            assert_eq!(
                cell.link, recovery.link,
                "{}: source occurrence {} {cell:?}",
                case.id, recovery.ordinal
            );
            if check_style {
                assert_eq!(cell.style, recovery.style, "{}: {cell:?}", case.id);
            }
        }
        if check_style {
            let range = &recovery.source["byteRange"];
            let input = &case.source[usize::try_from(range["start"].as_u64().unwrap()).unwrap()
                ..usize::try_from(range["end"].as_u64().unwrap()).unwrap()];
            assert_ne!(input, "", "{} original source member", case.id);
        }
    }
}

pub fn assert_constraints(content: &ResolvedContent, case: &Case) {
    let actual = snapshot(content);
    let text = actual.text();
    for constraint in &case.constraints {
        match constraint["kind"].as_str().unwrap() {
            "code-root" => assert_eq!(actual.codes, [constraint["text"].as_str().unwrap()]),
            "styled-word" => {
                let value = constraint["text"].as_str().unwrap();
                let at = positions(&text, value);
                assert_eq!(at.len(), 1);
                let style: Style = serde_json::from_value(constraint["style"].clone()).unwrap();
                for cell in &actual.cells[at[0]..at[0] + value.chars().count()] {
                    assert_eq!(cell.style, style);
                }
            }
            "definition-term" | "column-cell" | "preformatted-owner" => {
                let owner = match constraint["kind"].as_str().unwrap() {
                    "definition-term" => "/term/",
                    "column-cell" => "/row/0/column/1/",
                    _ => "/preformatted",
                };
                for ordinal in constraint["contains_link_ordinals"].as_array().unwrap() {
                    assert!(
                        actual.links[usize::try_from(ordinal.as_u64().unwrap()).unwrap()]
                            .owner
                            .contains(owner)
                    );
                }
                for ordinal in constraint["contains_recovery_occurrences"]
                    .as_array()
                    .unwrap()
                {
                    let rec = &case.recoveries[usize::try_from(ordinal.as_u64().unwrap()).unwrap()];
                    let at = positions(&text, &rec.text)[rec.ordinal];
                    assert!(actual.cells[at].owner.contains(owner));
                }
            }
            "field-acceptance" | "non-rejecting-control" => {
                let rejected = constraint["rejected_source_graph"].as_str();
                if let Some(rejected) = rejected {
                    assert_eq!(positions(&text, rejected), Vec::<usize>::new());
                } else {
                    assert_eq!(positions(&text, "Z").len(), 1);
                }
                assert_eq!(positions(&text, "BODY").len(), 1);
            }
            "source-backed-table-recovery" => {
                let accepted = constraint["accepted"].as_bool().unwrap();
                let cell_links = actual
                    .links
                    .iter()
                    .filter(|link| link.owner.contains("/row/0/column/0/"))
                    .count();
                assert_eq!(cell_links, usize::from(accepted));
                assert_eq!(
                    actual
                        .cells
                        .iter()
                        .filter(|cell| cell.value == 'Z' && cell.owner.contains("/row/0/column/1/"))
                        .count(),
                    1
                );
            }
            unknown => panic!("unregistered owner obligation {unknown}"),
        }
    }
}

/// Stable, preselected consumer domain. It never expands from a passing output.
pub fn consumers() -> Vec<(Case, bool)> {
    let subset: Value = serde_json::from_str(include_str!(
        "../roff_lowering/deferred_ownership/consumers.json"
    ))
    .unwrap();
    let mut cases = cases();
    let selected = subset["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let at = cases
                .iter()
                .position(|case| case.id == row["id"].as_str().unwrap())
                .unwrap();
            (
                cases.remove(at),
                row["narrow_actual_app"].as_bool().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 42);
    selected
}

fn assert_native_facts(content: &ResolvedContent, case: &Case, actual: &Snapshot) {
    if let Some(diagnostics) = &case.diagnostics {
        assert_eq!(
            serde_json::to_value(&content.document.as_ref().unwrap().diagnostics).unwrap(),
            serde_json::to_value(diagnostics).unwrap(),
            "{} exact native diagnostics",
            case.id
        );
    }
    if let Some(rows) = &case.reading_rows {
        let text = mant_render::render_query_man(content);
        let lines = text.split('\n').collect::<Vec<_>>();
        let start = lines
            .iter()
            .position(|line| *line == "DESCRIPTION")
            .unwrap()
            + 1;
        let end = lines.iter().rposition(|line| *line == "NEXT").unwrap();
        let mut actual = lines[start..end].to_vec();
        if actual.last() == Some(&"") {
            actual.pop();
        }
        assert_eq!(actual, *rows, "{} complete enhanced reading rows", case.id);
    }
    if let Some(old_x) = &case.old_x {
        let owner = old_x
            .get("link")
            .and_then(Value::as_u64)
            .map(|v| usize::try_from(v).unwrap());
        let positions = actual
            .cells
            .iter()
            .enumerate()
            .filter_map(|(at, cell)| (cell.value == 'X' && cell.link == owner).then_some(at))
            .collect::<Vec<_>>();
        let present = old_x["present"].as_bool().unwrap();
        assert_eq!(
            positions.len(),
            usize::from(present),
            "{} old native glyph",
            case.id
        );
        if present {
            let style: Style = serde_json::from_value(old_x["style"].clone()).unwrap();
            assert_eq!(actual.cells[positions[0]].style, style);
            assert_eq!(actual.cells[positions[0]].link, owner);
        }
    }
}
