//! Authored IR keeps prose syntax coloring independent of native execution.
use super::*;
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

mod conventions;
mod geometry;
mod protection;

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn strong(value: &str) -> Inline {
    Inline::Strong {
        children: vec![text(value)],
    }
}

fn emphasis(value: &str) -> Inline {
    Inline::Emphasis {
        children: vec![text(value)],
    }
}

fn code(value: &str) -> Inline {
    Inline::Code {
        value: value.into(),
    }
}

fn paragraph(children: Vec<Inline>) -> Block {
    Block::Paragraph {
        children,
        inline_layout: mant_ir::InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
    }
}

fn content(children: Vec<Inline>) -> ResolvedContent {
    let mut content = bundle();
    content.document.as_mut().unwrap().sections[0].blocks = vec![paragraph(children)];
    content
}

fn definition(terms: Vec<Inline>, description: Vec<Block>, facts: EntryFacts) -> Block {
    Block::DefinitionList {
        items: vec![DefinitionItem {
            terms: vec![terms.into()],
            description,
            entry: Some(facts),
            layout: mant_ir::DefinitionLayout::default(),
            source: None,
            head_body_relation: mant_ir::HeadBodyRelation::from(false),
        }],
        compact: true,
        declaration_groups: vec![],
        layout: LayoutHint::default(),
        source: None,
    }
}

fn facts(kind: EntryKind) -> EntryFacts {
    EntryFacts {
        id: "group".into(),
        kind,
        case: NameCase::Sensitive,
        names: vec![],
        alias_groups: vec![],
        alias_of: None,
        value_domain: None,
        forms: vec![mant_ir::EntryForm::term(0)],
        name_bindings: vec![],
    }
}

fn style_at(rendered: &RenderedDocument, needle: &str) -> Style {
    style_at_occurrence(rendered, needle, 0)
}

fn style_at_occurrence(rendered: &RenderedDocument, needle: &str, occurrence: usize) -> Style {
    let hit = &rendered.search(needle)[occurrence];
    let area = Rect::new(0, 0, 240, 1);
    let mut buffer = Buffer::empty(area);
    rendered.text.lines[hit.row]
        .clone()
        .render(area, &mut buffer);
    buffer[(u16::try_from(hit.start_column).unwrap(), 0)].style()
}

fn copied_match(rendered: &RenderedDocument, needle: &str) -> String {
    let hit = &rendered.search(needle)[0];
    let (row, column) = hit
        .additional_fragments
        .last()
        .map_or((hit.row, hit.end_column), |fragment| {
            (fragment.row, fragment.end_column)
        });
    rendered.selected_text(RenderedSelection {
        anchor: TextPosition {
            row: hit.row,
            column: hit.start_column,
        },
        focus: TextPosition {
            row,
            column: column - 1,
        },
    })
}
