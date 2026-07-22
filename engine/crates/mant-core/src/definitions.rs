//! Identifies addressable semantic entries after renderer-specific lowering.
//!
//! Both libmandoc and the groff HTML fallback produce definition lists. This
//! pass assigns one canonical option identity without leaking source macros
//! into the stable document contract.

use std::{
    collections::{HashSet, VecDeque},
    mem,
};

use mant_ast::{
    Block, DefinitionIdentity, DefinitionItem, DefinitionRole, Inline, LayoutHint, Section,
};

use crate::mandoc::inline::plain_text;

/// Annotate reliably recognizable command-line options and return every
/// inline anchor that the navigation resolver must retain.
pub(crate) fn identify_definitions(
    sections: &mut [Section],
    reserved_targets: &HashSet<String>,
) -> HashSet<String> {
    let mut used = HashSet::new();
    collect_section_ids(sections, &mut used);
    let mut retained = HashSet::new();
    for section in sections {
        identify_blocks(
            &mut section.blocks,
            &mut used,
            reserved_targets,
            &mut retained,
        );
        identify_sections(
            &mut section.children,
            &mut used,
            reserved_targets,
            &mut retained,
        );
    }
    retained
}

fn collect_section_ids(sections: &[Section], output: &mut HashSet<String>) {
    for section in sections {
        output.insert(section.id.clone());
        collect_section_ids(&section.children, output);
    }
}

fn identify_sections(
    sections: &mut [Section],
    used: &mut HashSet<String>,
    reserved: &HashSet<String>,
    retained: &mut HashSet<String>,
) {
    for section in sections {
        identify_blocks(&mut section.blocks, used, reserved, retained);
        identify_sections(&mut section.children, used, reserved, retained);
    }
}

fn identify_blocks(
    blocks: &mut Vec<Block>,
    used: &mut HashSet<String>,
    reserved: &HashSet<String>,
    retained: &mut HashSet<String>,
) {
    normalize_hanging_definitions(blocks);
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    identify_blocks(&mut item.blocks, used, reserved, retained);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    identify_item(item, used, reserved, retained);
                    identify_blocks(&mut item.description, used, reserved, retained);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        identify_blocks(&mut cell.blocks, used, reserved, retained);
                    }
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

/// Turn renderer-neutral hanging-indent runs into semantic definitions.
///
/// Some man(7) generators use `.PP` followed by `.RS` instead of `.TP` for
/// option entries. libmandoc and groff both correctly retain that layout, but
/// neither representation is a definition list on its own. Recognising the
/// shared visible shape here keeps option identity independent of the source
/// macro and of the renderer selected by the query pipeline.
fn normalize_hanging_definitions(blocks: &mut Vec<Block>) {
    let mut pending: VecDeque<Block> = mem::take(blocks).into();
    let mut normalized = Vec::with_capacity(pending.len());

    while let Some(block) = pending.pop_front() {
        let Some(term_indent) = option_term_indent(&block) else {
            normalized.push(block);
            continue;
        };

        let mut description = Vec::new();
        while let Some(next) = pending.front() {
            if option_term_indent(next) == Some(term_indent) {
                break;
            }
            if matches!(next, Block::VerticalSpace { .. }) {
                if description.is_empty() {
                    break;
                }
                description.push(pending.pop_front().expect("front exists"));
                continue;
            }
            if block_indent(next) <= term_indent {
                break;
            }
            description.push(pending.pop_front().expect("front exists"));
        }

        if description.is_empty() {
            normalized.push(block);
            continue;
        }

        let Block::Paragraph {
            children,
            layout,
            source,
        } = block
        else {
            unreachable!("option_term_indent only accepts paragraphs");
        };
        let description_origin = term_indent.saturating_add(4);
        for child in &mut description {
            shift_block_indent(child, description_origin);
        }
        normalized.push(Block::DefinitionList {
            items: vec![DefinitionItem {
                identity: None,
                terms: vec![children],
                description,
                spacing_before_lines: Some(layout.spacing_before_lines),
            }],
            compact: true,
            layout: LayoutHint {
                indent_columns: term_indent,
                spacing_before_lines: 0,
            },
            source,
        });
    }

    *blocks = normalized;
}

fn option_term_indent(block: &Block) -> Option<u16> {
    let Block::Paragraph {
        children, layout, ..
    } = block
    else {
        return None;
    };
    let text = plain_text(children);
    let trimmed = text.trim_start();
    (trimmed.starts_with('-')
        && !option_names_from_terms(std::slice::from_ref(children)).is_empty())
    .then_some(layout.indent_columns)
}

fn block_indent(block: &Block) -> u16 {
    match block {
        Block::Paragraph { layout, .. }
        | Block::Preformatted { layout, .. }
        | Block::List { layout, .. }
        | Block::DefinitionList { layout, .. }
        | Block::Table { layout, .. }
        | Block::Equation { layout, .. }
        | Block::Unsupported { layout, .. } => layout.indent_columns,
        Block::VerticalSpace { .. } => 0,
    }
}

fn shift_block_indent(block: &mut Block, origin: u16) {
    let layout = match block {
        Block::Paragraph { layout, .. }
        | Block::Preformatted { layout, .. }
        | Block::List { layout, .. }
        | Block::DefinitionList { layout, .. }
        | Block::Table { layout, .. }
        | Block::Equation { layout, .. }
        | Block::Unsupported { layout, .. } => layout,
        Block::VerticalSpace { .. } => return,
    };
    layout.indent_columns = layout.indent_columns.saturating_sub(origin);
}

fn identify_item(
    item: &mut DefinitionItem,
    used: &mut HashSet<String>,
    reserved: &HashSet<String>,
    retained: &mut HashSet<String>,
) {
    let names = option_names(item);
    if names.is_empty() {
        return;
    }

    let mut anchors = Vec::new();
    for term in &item.terms {
        collect_anchor_ids(term, &mut anchors);
    }
    retained.extend(anchors.iter().cloned());

    let existing = anchors.first().cloned();
    let preferred = existing
        .clone()
        .unwrap_or_else(|| format!("option-{}", slug(&names[0])));
    // A copied libmandoc anchor may itself be an explicit `.Tg` destination,
    // so it is allowed to match the reserved set. Generated IDs are not.
    let id = if existing.is_some() && !used.contains(&preferred) {
        used.insert(preferred.clone());
        preferred
    } else {
        unique_id(&preferred, used, reserved)
    };
    if !anchors.iter().any(|anchor| anchor == &id)
        && let Some(term) = item.terms.first_mut()
    {
        term.insert(0, Inline::Anchor { id: id.clone() });
    }
    retained.insert(id.clone());
    item.identity = Some(DefinitionIdentity {
        id,
        role: DefinitionRole::Option,
        names,
    });
}

fn option_names(item: &DefinitionItem) -> Vec<String> {
    option_names_from_terms(&item.terms)
}

fn option_names_from_terms(terms: &[Vec<Inline>]) -> Vec<String> {
    let mut names = Vec::new();
    for term in terms {
        let text = plain_text(term);
        for token in text.split(|character: char| {
            character.is_whitespace() || matches!(character, ',' | '|' | '/' | ';')
        }) {
            let token = token.trim_matches(|character: char| {
                matches!(
                    character,
                    '[' | ']' | '(' | ')' | '{' | '}' | '“' | '”' | '‘' | '’'
                )
            });
            let Some(name) = option_prefix(token) else {
                continue;
            };
            if !names.iter().any(|existing| existing == name) {
                names.push(name.to_owned());
            }
        }
    }
    names
}

fn option_prefix(token: &str) -> Option<&str> {
    if !token.starts_with('-') || token == "-" {
        return None;
    }
    let end = token
        .char_indices()
        .skip(1)
        .take_while(|(_, character)| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '?')
        })
        .map(|(index, character)| index + character.len_utf8())
        .last()?;
    let candidate = &token[..end];
    let body = candidate.trim_start_matches('-');
    (!body.is_empty()
        && body
            .chars()
            .any(|character| character.is_ascii_alphanumeric() || character == '?'))
    .then_some(candidate)
}

fn collect_anchor_ids(nodes: &[Inline], output: &mut Vec<String>) {
    for node in nodes {
        match node {
            Inline::Anchor { id } => output.push(id.clone()),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => collect_anchor_ids(children, output),
            Inline::Text { .. } | Inline::Code { .. } | Inline::LineBreak => {}
        }
    }
}

fn slug(value: &str) -> String {
    let slug = value
        .trim_start_matches('-')
        .chars()
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "entry".to_owned()
    } else {
        slug
    }
}

fn unique_id(base: &str, used: &mut HashSet<String>, reserved: &HashSet<String>) -> String {
    let mut candidate = base.to_owned();
    let mut suffix = 2;
    while used.contains(&candidate) || reserved.contains(&candidate) {
        candidate = format!("{base}-{suffix}");
        suffix += 1;
    }
    used.insert(candidate.clone());
    candidate
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use mant_ast::{Block, DefinitionItem, Inline, LayoutHint, Section};

    use super::{identify_definitions, option_names};

    fn item(value: &str) -> DefinitionItem {
        DefinitionItem {
            identity: None,
            terms: vec![vec![Inline::Text {
                value: value.into(),
            }]],
            description: Vec::new(),
            spacing_before_lines: None,
        }
    }

    #[test]
    fn extracts_aliases_without_argument_placeholders() {
        assert_eq!(
            option_names(&item("-g, --listed-incremental=FILE")),
            ["-g", "--listed-incremental"]
        );
        assert_eq!(option_names(&item("ordinary term")), Vec::<String>::new());
    }

    #[test]
    fn normalizes_hanging_option_layout_before_assigning_identity() {
        let paragraph = |value: &str, indent_columns| Block::Paragraph {
            children: vec![Inline::Text {
                value: value.to_owned(),
            }],
            layout: LayoutHint {
                indent_columns,
                spacing_before_lines: 1,
            },
            source: None,
        };
        let mut sections = vec![Section {
            id: "options".to_owned(),
            title: "OPTIONS".to_owned(),
            spacing_before_lines: 0,
            blocks: vec![
                paragraph("-v, --version", 0),
                paragraph("Print version information.", 4),
                paragraph("-C <path>", 0),
                paragraph("Run from path.", 4),
            ],
            children: Vec::new(),
            source: None,
        }];

        identify_definitions(&mut sections, &HashSet::new());

        assert_eq!(sections[0].blocks.len(), 2);
        let Block::DefinitionList { items, layout, .. } = &sections[0].blocks[0] else {
            panic!("hanging option should become a definition list");
        };
        assert_eq!(layout.indent_columns, 0);
        assert_eq!(
            items[0].identity.as_ref().expect("option identity").names,
            ["-v", "--version"]
        );
        assert!(matches!(
            &items[0].description[0],
            Block::Paragraph { layout, .. } if layout.indent_columns == 0
        ));
        let Block::DefinitionList { items, .. } = &sections[0].blocks[1] else {
            panic!("second option should remain independently addressable");
        };
        assert_eq!(
            items[0].identity.as_ref().expect("option identity").names,
            ["-C"]
        );
    }
}
