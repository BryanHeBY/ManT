//! Source-derived section boundaries across query and terminal consumers.
use super::*;
use mant_ir::{geometry, inline_plain_text, visit};
use mant_protocol::{ContentSelector, ExcerptSelection, QueryBundle, QueryExcerpt};
use serde::Deserialize;

mod markdown;
mod reader;

const BODY: &str = "AFTERMARK";
const WIDTHS: [u16; 4] = [12, 24, 78, 120];
const REFERENCE_SHA256: &str = "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e";

#[derive(Deserialize)]
struct Matrix {
    oracle_sha256: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    native_rows: Vec<String>,
    section_gap: u16,
    #[serde(default)]
    device_shared_heading: bool,
    link_target: Option<String>,
}

impl Case {
    fn input(&self) -> ResolvedContent {
        let input = mant_loader::load_roff_bytes(self.source.as_bytes()).unwrap();
        let document = input.document.as_ref().unwrap();
        assert_eq!(mant_ir::validate_document(document), [], "{}", self.id);
        let wire = serde_json::to_string(&QueryBundle::from(&input)).unwrap();
        let roundtrip = ResolvedContent::from(serde_json::from_str::<QueryBundle>(&wire).unwrap());
        assert_eq!(input, roundtrip, "JSON transport {}", self.id);
        roundtrip
    }

    fn heading(&self) -> &str {
        if self.id == "bare-z" { "EXT" } else { "NEXT" }
    }

    fn reading_rows(&self) -> Vec<&str> {
        let mut rows: Vec<_> = self.native_rows.iter().map(String::as_str).collect();
        if self.device_shared_heading {
            // With PD 0, man_term.c::pre_SS need not end the old native line.
            // ManT retains both source owners as separate reading rows.
            assert!(matches!(rows[1], "ALPHA NEXT" | "ALPHANEXT"));
            rows.splice(1..2, ["ALPHA", "NEXT"]);
        }
        rows
    }

    fn target<'a>(&self, document: &'a Document) -> &'a Section {
        fn find(sections: &[Section]) -> Option<&Section> {
            for section in sections {
                if section.id.as_str() == "next" {
                    return Some(section);
                }
                if let Some(target) = find(&section.children) {
                    return Some(target);
                }
            }
            None
        }
        find(&document.sections).unwrap_or_else(|| panic!("missing NEXT source owner {}", self.id))
    }

    fn expected_body(&self) -> Vec<&str> {
        self.reading_rows()
            .into_iter()
            .skip_while(|row| *row != self.heading())
            .skip(1)
            .collect()
    }
}

fn matrix() -> Vec<Case> {
    let matrix: Matrix =
        serde_json::from_str(include_str!("section_execution/cases.json")).unwrap();
    assert_eq!(matrix.oracle_sha256, REFERENCE_SHA256);
    assert_eq!(matrix.cases.len(), 13);
    matrix.cases
}

fn assert_body_owner(case: &Case, target: &Section) {
    assert_eq!(target.spacing_before_lines, case.section_gap, "{}", case.id);
    assert_eq!(
        inline_plain_text(&target.heading.content).trim_start_matches(' '),
        case.heading(),
        "{}",
        case.id
    );
    assert!(
        target
            .blocks
            .iter()
            .all(|block| !matches!(block, AstBlock::Preformatted { .. })),
        "filled BODY must not inherit any literal rows: {} {:#?}",
        case.id,
        target.blocks
    );
    let last = target.blocks.last().unwrap();
    let AstBlock::Paragraph { children, .. } = last else {
        panic!(
            "AFTERMARK should remain in its source paragraph: {}",
            case.id
        );
    };
    assert_eq!(
        inline_plain_text(children),
        case.expected_body().last().unwrap().to_string(),
        "{}",
        case.id
    );
    let source = geometry::block_source(last).expect("authored paragraph source");
    let expected_line = case.source.lines().position(|line| line == BODY).unwrap() + 1;
    assert_eq!(source.line as usize, expected_line, "{}", case.id);
    assert_links(case, children);
}

fn assert_links(case: &Case, children: &[Inline]) {
    #[derive(Default)]
    struct Links(Vec<(String, String)>);
    impl<'a> visit::Visit<'a> for Links {
        fn visit_inline(&mut self, inline: &'a Inline) {
            if let Inline::Link {
                target, children, ..
            } = inline
            {
                self.0.push((
                    target.to_uri().expect("safe link"),
                    inline_plain_text(children),
                ));
            }
            visit::walk_inline(self, inline);
        }
    }
    let mut links = Links::default();
    for node in children {
        visit::Visit::visit_inline(&mut links, node);
    }
    let expected: Vec<_> = case
        .link_target
        .iter()
        .map(|uri| (uri.clone(), BODY.to_owned()))
        .collect();
    assert_eq!(links.0, expected, "{}", case.id);
}

fn text_rows(text: &str) -> Vec<&str> {
    // Native and viewport block origins are deliberately independent. None
    // of these sources authors leading spaces on the measured rows.
    text.lines()
        .map(|row| row.trim_start_matches(' '))
        .collect()
}

fn report(id: &str, route: &str, width: Option<u16>) {
    if std::env::var_os("MANT_CONSUMER_MATRIX_REPORT").is_some() {
        let width = width.map_or_else(|| "-".to_owned(), |width| width.to_string());
        println!("MATRIX\tM4\t{id}\t{route}\t{width}\tpass");
    }
}

fn ansi(_: mant_render::TextPresentation, text: &str) -> String {
    format!("\x1b[1m{text}\x1b[0m")
}

fn unstyle(text: &str) -> String {
    text.replace("\x1b[1m", "").replace("\x1b[0m", "")
}

#[test]
fn section_source_owners_survive_full_and_selected_text() {
    for case in matrix() {
        let input = case.input();
        let document = input.document.as_ref().unwrap();
        let target = case.target(document);
        assert_body_owner(&case, target);

        let expected = case.reading_rows();
        for width in WIDTHS {
            let plain = mant_render::render_query_man_at_width(&input, usize::from(width));
            let styled = mant_render::render_query_text_at_width(&input, usize::from(width), ansi);
            assert_eq!(plain, unstyle(&styled), "{} width {width}", case.id);
            let rows = text_rows(&plain);
            assert!(
                rows.windows(expected.len()).any(|rows| rows == expected),
                "{} width {width}: wanted {expected:?}, got {rows:?}",
                case.id
            );
            report(&case.id, "full/plain-and-ansi", Some(width));
        }

        // Excerpts deliberately have no viewport width argument. Check the
        // real selection/transport/render path independently of full text.
        let excerpt = mant_query::select_excerpt(&input, &[ContentSelector::id("next")]).unwrap();
        let wire = serde_json::to_string(&excerpt).unwrap();
        let excerpt = serde_json::from_str::<QueryExcerpt>(&wire).unwrap();
        let [ExcerptSelection::DocumentSection { section, .. }] = excerpt.selections.as_slice()
        else {
            panic!("selected source section {}", case.id);
        };
        assert_eq!(section, target, "selected source owner {}", case.id);
        let plain = mant_render::render_excerpt_text(&excerpt);
        assert_eq!(
            plain,
            unstyle(&mant_render::render_excerpt_text_with(&excerpt, ansi)),
            "selected {}",
            case.id
        );
        let mut selected = vec![case.heading()];
        selected.extend(case.expected_body());
        assert!(
            text_rows(&plain).ends_with(&selected),
            "{}: {plain:?}",
            case.id
        );
        report(&case.id, "selected/plain-and-ansi", None);
    }
}
