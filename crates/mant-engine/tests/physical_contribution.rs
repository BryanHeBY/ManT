//! Exact source-neutral rows at document and detached excerpt boundaries.
//!
//! A physical row can be empty; a navigation target need not own a row.
//! Expected rows below are assembled from authored contributions, not from
//! a text or Markdown consumer's output.

use mant_ir::*;
use mant_protocol::{ContentSelector, QueryBundle, QueryExcerpt};
use mant_render::{
    render_excerpt_text, render_excerpt_text_with, render_query_text, render_query_text_with,
};

#[path = "physical_contribution/markdown.rs"]
mod markdown;
#[path = "physical_contribution/metadata.rs"]
mod metadata;
#[path = "physical_contribution/structure.rs"]
mod structure;

const LABEL: &str = "Catalog";
const NEEDLE: &str = "NEEDLE中";
const URI: &str = "https://example.test/needle";
const EMPTY_URI: &str = "https://example.test/empty";
const BEFORE_URI: &str = "https://example.test/before";

fn audit(family: &str, id: &str, policy: &str) {
    if std::env::var_os("MANT_CONSUMER_MATRIX_REPORT").is_some() {
        println!("MATRIX\t{family}\t{id}\t{policy}\tpass");
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Body {
    None,
    EmptyParagraph,
    EmptyText,
    Anchor,
    EmptyLink,
    StyledEmptyLink,
    ZeroGap,
    NoLiteralRow,
    LiteralAnchor,
    TransparentSequence,
    Gap,
    BlockGap,
    HardRow,
    EmptyLiteralRow,
    Space,
    LinkedText,
}

const BODIES: [Body; 16] = [
    Body::None,
    Body::EmptyParagraph,
    Body::EmptyText,
    Body::Anchor,
    Body::EmptyLink,
    Body::StyledEmptyLink,
    Body::ZeroGap,
    Body::NoLiteralRow,
    Body::LiteralAnchor,
    Body::TransparentSequence,
    Body::Gap,
    Body::BlockGap,
    Body::HardRow,
    Body::EmptyLiteralRow,
    Body::Space,
    Body::LinkedText,
];

#[derive(Clone, Copy, Debug)]
enum Owner {
    Root,
    Section,
    NestedSection,
    PlainRoot,
    PlainSection,
    DefinitionRoot,
    DefinitionSection,
}

const OWNERS: [Owner; 7] = [
    Owner::Root,
    Owner::Section,
    Owner::NestedSection,
    Owner::PlainRoot,
    Owner::PlainSection,
    Owner::DefinitionRoot,
    Owner::DefinitionSection,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Neighbors {
    None,
    Both,
    GapAfter,
    GapBefore,
}

const NEIGHBORS: [Neighbors; 4] = [
    Neighbors::None,
    Neighbors::Both,
    Neighbors::GapAfter,
    Neighbors::GapBefore,
];

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn link(uri: &str, children: Vec<Inline>) -> Inline {
    Inline::Link {
        target: LinkTarget::External { uri: uri.into() },
        title: None,
        children,
    }
}

fn anchor() -> Inline {
    Inline::Anchor {
        id: "nav".into(),
        fragment_aliases: vec!["Source.Nav".into()],
        owner_source: None,
    }
}

fn paragraph(children: Vec<Inline>) -> Block {
    Block::Paragraph {
        children,
        inline_layout: InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
    }
}

fn literal(children: Vec<Inline>) -> Block {
    Block::Preformatted {
        children,
        inline_layout: InlineLayout::default(),
        language: None,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn gap(lines: u16) -> Block {
    Block::VerticalSpace {
        lines,
        source: None,
    }
}

impl Body {
    fn blocks(self) -> Vec<Block> {
        vec![match self {
            Self::None => return vec![],
            Self::EmptyParagraph => paragraph(vec![]),
            Self::EmptyText => paragraph(vec![text("")]),
            Self::Anchor => paragraph(vec![anchor()]),
            Self::EmptyLink => paragraph(vec![link(EMPTY_URI, vec![])]),
            Self::StyledEmptyLink => paragraph(vec![Inline::Strong {
                children: vec![link(EMPTY_URI, vec![])],
            }]),
            Self::ZeroGap => gap(0),
            Self::NoLiteralRow => literal(vec![]),
            Self::LiteralAnchor => literal(vec![anchor()]),
            Self::TransparentSequence => {
                return vec![
                    paragraph(vec![]),
                    paragraph(vec![anchor()]),
                    gap(0),
                    literal(vec![]),
                ];
            }
            Self::Gap => gap(1),
            Self::BlockGap => {
                let mut block = paragraph(vec![]);
                geometry::block_layout_mut(&mut block)
                    .unwrap()
                    .spacing_before_lines = 1;
                block
            }
            Self::HardRow => paragraph(vec![Inline::line_break()]),
            Self::EmptyLiteralRow => literal(vec![text("")]),
            Self::Space => paragraph(vec![text(" ")]),
            Self::LinkedText => paragraph(vec![link(URI, vec![text(NEEDLE)]), text(" TAIL")]),
        }]
    }

    const fn rows(self) -> &'static [&'static str] {
        match self {
            Self::Gap | Self::BlockGap | Self::HardRow | Self::EmptyLiteralRow => &[""],
            Self::Space => &[" "],
            Self::LinkedText => &["NEEDLE中 TAIL"],
            _ => &[],
        }
    }

    const fn anchor(self) -> bool {
        matches!(
            self,
            Self::Anchor | Self::LiteralAnchor | Self::TransparentSequence
        )
    }
}

impl Neighbors {
    const fn before(self) -> bool {
        matches!(self, Self::Both | Self::GapBefore)
    }
    const fn after(self) -> bool {
        matches!(self, Self::Both | Self::GapAfter)
    }

    fn blocks(self, body: Body) -> Vec<Block> {
        let mut blocks = vec![];
        if self.before() {
            blocks.push(paragraph(vec![link(BEFORE_URI, vec![text("BEFORE")])]));
        }
        if self == Self::GapBefore {
            blocks.push(gap(1));
        }
        blocks.extend(body.blocks());
        if self == Self::GapAfter {
            blocks.push(gap(1));
        }
        if self.after() {
            blocks.push(paragraph(vec![text("AFTER")]));
        }
        blocks
    }

    fn rows(self, body: Body) -> Vec<&'static str> {
        match self {
            Self::None => body.rows().to_vec(),
            Self::Both => [&["BEFORE"][..], body.rows(), &["AFTER"]].concat(),
            Self::GapAfter => [body.rows(), &["", "AFTER"]].concat(),
            Self::GapBefore => [&["BEFORE", ""][..], body.rows()].concat(),
        }
    }
}

impl Owner {
    const fn section(self) -> bool {
        matches!(
            self,
            Self::Section | Self::NestedSection | Self::PlainSection | Self::DefinitionSection
        )
    }

    const fn definition(self) -> bool {
        matches!(self, Self::DefinitionRoot | Self::DefinitionSection)
    }

    fn install(self, value: &mut ResolvedContent, rows: usize, blocks: Vec<Block>) {
        let blocks = match self {
            Self::PlainRoot | Self::PlainSection if !blocks.is_empty() => vec![Block::List {
                kind: ListKind::Plain,
                compact: true,
                items: vec![ListItem {
                    blocks,
                    source: None,
                    entry: None,
                    layout: ListItemLayout {
                        spacing_before_lines: Some(0),
                    },
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
            Self::DefinitionRoot | Self::DefinitionSection if !blocks.is_empty() => {
                vec![Block::DefinitionList {
                    items: vec![DefinitionItem {
                        terms: vec![],
                        description: blocks,
                        head_body_relation: HeadBodyRelation::Separate,
                        layout: DefinitionLayout {
                            body_indent_columns: 3,
                            spacing_before_lines: Some(0),
                            ..DefinitionLayout::default()
                        },
                        entry: None,
                        source: None,
                    }],
                    declaration_groups: vec![],
                    compact: true,
                    layout: LayoutHint::default(),
                    source: None,
                }]
            }
            _ => blocks,
        };
        let document = value.document.as_mut().unwrap();
        if matches!(self, Self::NestedSection) {
            document.sections.push(section("outer", "Outer", 0, vec![]));
            document.sections[0]
                .children
                .push(section("subject", "Subject", rows, blocks));
        } else if self.section() {
            document
                .sections
                .push(section("subject", "Subject", rows, blocks));
        } else {
            document.blocks = blocks;
        }
    }

    fn selected(self, value: &ResolvedContent) -> QueryExcerpt {
        let selector = if self.section() {
            ContentSelector::id("subject")
        } else {
            ContentSelector::path("root")
        };
        mant_query::select_excerpt(value, &[selector]).unwrap()
    }

    fn expected(
        self,
        heading_tail: usize,
        body: Body,
        neighbors: Neighbors,
        selected: bool,
    ) -> String {
        let mut expected = Vec::new();
        let nested = matches!(self, Self::NestedSection);
        if !selected || !self.section() {
            expected.push(LABEL.to_owned());
        }
        if !selected && self.section() {
            expected.extend(std::iter::repeat_n(String::new(), heading_tail + 1));
            if nested {
                expected.push("Outer".into());
            }
        }
        if self.section() {
            expected.push(
                if nested && !selected {
                    "  Subject"
                } else {
                    "Subject"
                }
                .into(),
            );
        }
        expected.extend(std::iter::repeat_n(String::new(), heading_tail));
        let body = neighbors.rows(body);
        if !self.section() && !body.is_empty() {
            expected.push(String::new());
        }
        let origin = 3 * usize::from(self.definition()) + 2 * usize::from(nested && !selected);
        expected.extend(body.into_iter().map(|row| {
            if row.is_empty() {
                String::new()
            } else {
                format!("{}{row}", " ".repeat(origin))
            }
        }));
        physical_text(&expected)
    }
}

fn section(id: &str, title: &str, tail: usize, blocks: Vec<Block>) -> Section {
    Section {
        id: id.into(),
        heading: heading(title, tail),
        fragment_aliases: vec![],
        spacing_before_lines: 0,
        blocks,
        children: vec![],
        source: None,
    }
}

fn heading(title: &str, tail: usize) -> Heading {
    Heading {
        content: [vec![text(title)], vec![Inline::line_break(); tail]].concat(),
        ..Heading::default()
    }
}

fn physical_text(rows: &[String]) -> String {
    let mut output = rows.join("\n");
    if rows.last().is_some_and(String::is_empty) {
        output.push('\n');
    }
    output
}

fn base(tail: usize) -> ResolvedContent {
    let mut value = mant_loader::load_markdown_text("# Catalog\n", None).unwrap();
    value.document.as_mut().unwrap().heading = Some(heading(LABEL, tail));
    value
}

fn query_wire(value: &ResolvedContent) -> ResolvedContent {
    serde_json::from_str::<QueryBundle>(&serde_json::to_string(&QueryBundle::from(value)).unwrap())
        .unwrap()
        .into()
}

fn excerpt_wire(value: &QueryExcerpt) -> QueryExcerpt {
    serde_json::from_str(&serde_json::to_string(value).unwrap()).unwrap()
}

fn ansi(_: mant_render::TextPresentation, value: &str) -> String {
    format!("\x1b[1m{value}\x1b[0m")
}
fn plain(value: &str) -> String {
    value.replace("\x1b[1m", "").replace("\x1b[0m", "")
}

fn outline_text(excerpt: &QueryExcerpt) -> String {
    let trail = excerpt.selections[0].outline();
    let crumb = trail
        .ancestors
        .iter()
        .map(|ancestor| ancestor.title.as_str())
        .chain(std::iter::once(trail.title()))
        .collect::<Vec<_>>()
        .join(" > ");
    format!(
        "{}\n\nOutline {}: {crumb}",
        excerpt.display_title.as_deref().unwrap_or(&excerpt.label),
        trail.path()
    )
}

fn assert_targets(
    value: &ResolvedContent,
    body: Body,
    neighbors: Neighbors,
    preserve_anchors: bool,
) {
    let document = value.document.as_ref().unwrap();
    let mut links = Vec::new();
    let receipt = scan_references(document, ReferenceScanLimits::default(), |found| {
        let address = found.location.to_owned().unwrap();
        assert!(std::ptr::eq(
            address.resolve_link(document).unwrap(),
            found.link
        ));
        links.push((found.target.clone(), inline_plain_text(found.label)));
        std::ops::ControlFlow::Continue(())
    });
    assert!(receipt.complete());
    let mut expected = vec![];
    if neighbors.before() {
        expected.push((BEFORE_URI, "BEFORE"));
    }
    if matches!(body, Body::EmptyLink | Body::StyledEmptyLink) {
        expected.push((EMPTY_URI, ""));
    }
    if body == Body::LinkedText {
        expected.push((URI, NEEDLE));
    }
    let mut expected = expected
        .into_iter()
        .map(|(uri, label)| (LinkTarget::External { uri: uri.into() }, label.into()))
        .collect::<Vec<_>>();
    if !preserve_anchors {
        // Markdown can relocate zero-width navigation to a receiving block.
        let compare = |a: &(LinkTarget, String), b: &(LinkTarget, String)| a.1.cmp(&b.1);
        links.sort_by(compare);
        expected.sort_by(compare);
    }
    assert_eq!(links, expected);
    if body.anchor() && preserve_anchors {
        assert_eq!(
            DocumentIndex::build(document).fragment_target("Source.Nav"),
            Some(&NodeId::from("nav"))
        );
    }
}

fn assert_visible_search_coordinates(value: &ResolvedContent, owner: Owner) {
    use mant_codec::encode::{MarkdownOptions, render_addressable_markdown_with_options};
    use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};

    let artifact =
        render_addressable_markdown_with_options(value, MarkdownOptions::READABLE_ADDRESSABLE);
    for pattern in [NEEDLE, "TAIL"] {
        let result = mant_query::search_query(
            value,
            &SearchQuery {
                pattern: pattern.into(),
                syntax: SearchSyntax::Literal,
                case: SearchCase::Sensitive,
                scope: SearchScope::Visible,
                word: false,
                context_lines: 0,
                limit: 2,
                offset: 0,
            },
        )
        .unwrap();
        let [hit] = result.matches.as_slice() else {
            panic!("one authored search hit for {pattern}")
        };
        let expected_owner = if owner.section() {
            "subject"
        } else {
            DOCUMENT_ROOT_ID
        };
        assert_eq!(hit.outline.node.id(), expected_owner);
        let [occurrence] = hit.occurrences.as_slice() else {
            panic!("one source occurrence for {pattern}")
        };
        let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
            ..usize::try_from(occurrence.markdown.end_byte).unwrap();
        assert_eq!(&artifact.text()[range], pattern);
    }
}

#[test]
fn physical_body_contribution_matrix() {
    let mut cases = 0;
    for body in BODIES {
        for owner in OWNERS {
            for tail in 0..=2 {
                for neighbors in NEIGHBORS {
                    let mut value = base(tail);
                    owner.install(&mut value, tail, neighbors.blocks(body));
                    let issues = validate_document(value.document.as_ref().unwrap());
                    assert!(
                        issues.is_empty(),
                        "{body:?}/{owner:?}/{tail}/{neighbors:?}: {issues:?}"
                    );
                    for wire in [false, true] {
                        let restored = if wire {
                            query_wire(&value)
                        } else {
                            value.clone()
                        };
                        assert_eq!(restored, value);
                        let excerpt = owner.selected(&restored);
                        let excerpt = if wire {
                            excerpt_wire(&excerpt)
                        } else {
                            excerpt
                        };
                        assert_targets(&restored, body, neighbors, true);
                        if body == Body::LinkedText {
                            assert_visible_search_coordinates(&restored, owner);
                        }
                        let id = format!("{body:?}/{owner:?}/{tail}/{neighbors:?}/{wire}");
                        let full = owner.expected(tail, body, neighbors, false);
                        let selected = format!(
                            "{}\n\n{}",
                            outline_text(&excerpt),
                            owner.expected(tail, body, neighbors, true)
                        );
                        assert_eq!(render_query_text(&restored), full, "full {id}");
                        assert_eq!(
                            plain(&render_query_text_with(&restored, ansi)),
                            full,
                            "decorated full {id}"
                        );
                        assert_eq!(render_excerpt_text(&excerpt), selected, "excerpt {id}");
                        assert_eq!(
                            plain(&render_excerpt_text_with(&excerpt, ansi)),
                            selected,
                            "decorated excerpt {id}"
                        );
                        if body.rows().is_empty() {
                            let mut control = base(tail);
                            owner.install(&mut control, tail, neighbors.blocks(Body::None));
                            assert_eq!(
                                render_query_text(&control),
                                full,
                                "zero-contribution control {id}"
                            );
                            assert_eq!(
                                render_excerpt_text(&owner.selected(&control)),
                                selected,
                                "zero-contribution excerpt control {id}"
                            );
                        }
                        audit(
                            "CG-A",
                            &id,
                            "exact-physical-rows/full-selected/plain-ansi/targets",
                        );
                        cases += 1;
                    }
                }
            }
        }
    }
    assert_eq!(cases, 2_688);
}

#[test]
fn empty_markdown_link_does_not_add_a_title_separator() {
    let control = mant_loader::load_markdown_text("# Catalog\n", None).unwrap();
    for source in [
        "[](https://example.test/empty)",
        "**[](https://example.test/empty)**",
        "[](#catalog)",
    ] {
        let value =
            mant_loader::load_markdown_text(&format!("# Catalog\n\n{source}\n"), None).unwrap();
        assert_eq!(render_query_text(&value), "Catalog", "{source}");
        let excerpt = Owner::Root.selected(&value);
        assert_eq!(
            render_excerpt_text(&excerpt),
            "Catalog\n\nOutline root: OVERVIEW\n\nCatalog",
            "{source}"
        );
        assert_eq!(
            render_excerpt_text(&excerpt),
            render_excerpt_text(&Owner::Root.selected(&control))
        );
    }
}
