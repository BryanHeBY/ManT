//! Identifies addressable semantic entries after source-specific lowering.
//!
//! Both libmandoc macro sets and Markdown produce definition lists. This
//! pass assigns one canonical option identity without leaking source macros
//! into the stable document contract.

use std::{
    collections::{HashSet, VecDeque},
    mem,
};

use mant_ir::{
    Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, Inline, LayoutHint,
    Section, SourceSpan,
    visit::{self, Visit},
};

use crate::inline::{DEFAULT_INLINE_TERM_MAX_WIDTH, plain_text, terms_fit_inline};

/// Annotate reliably recognizable command-line options and return every
/// inline anchor that the navigation resolver must retain.
pub(crate) fn identify_definitions(
    blocks: &mut Vec<Block>,
    sections: &mut [Section],
    reserved_targets: &HashSet<String>,
) -> HashSet<String> {
    let mut used = HashSet::new();
    collect_section_ids(sections, &mut used);
    let mut retained = HashSet::new();
    identify_blocks(blocks, &mut used, reserved_targets, &mut retained);
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

/// Return identified definition items in the single source order shared by
/// projection and search ownership, including the source of their containing
/// definition list.
pub(crate) fn definition_entries(blocks: &[Block]) -> Vec<(&DefinitionItem, Option<SourceSpan>)> {
    let mut entries = Vec::new();
    collect_definition_entries(blocks, &mut entries);
    entries
}

fn collect_definition_entries<'a>(
    blocks: &'a [Block],
    output: &mut Vec<(&'a DefinitionItem, Option<SourceSpan>)>,
) {
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    collect_definition_entries(&item.blocks, output);
                }
            }
            Block::DefinitionList { items, source, .. } => {
                for item in items {
                    if item.identity.is_some() {
                        output.push((item, *source));
                    }
                    collect_definition_entries(&item.description, output);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        collect_definition_entries(&cell.blocks, output);
                    }
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn collect_section_ids(sections: &[Section], output: &mut HashSet<String>) {
    struct Collector<'a>(&'a mut HashSet<String>);

    impl<'ir> Visit<'ir> for Collector<'_> {
        fn visit_section(&mut self, section: &'ir Section) {
            self.0.insert(section.id.to_string());
            visit::walk_section(self, section);
        }
    }

    let mut collector = Collector(output);
    for section in sections {
        collector.visit_section(section);
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
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

/// Turn renderer-neutral hanging-indent runs into semantic definitions.
///
/// Some man(7) generators use `.PP` followed by `.RS` instead of `.TP` for
/// option entries. Native parsers correctly retain that layout, but
/// neither representation is a definition list on its own. Recognising the
/// shared visible shape here keeps option identity independent of the source
/// macro set or source parser used by the query pipeline.
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
        // A relative-indent wrapper continues the hanging term on the next
        // line; it does not introduce a paragraph-distance gap of its own.
        // The outer term retains the `.PP`/`.PD` distance between entries,
        // while an explicit leading vertical-space block would have stopped
        // this normalization before reaching this point.
        clear_block_spacing(description.first_mut());
        let terms = vec![children];
        normalized.push(Block::DefinitionList {
            items: vec![DefinitionItem {
                identity: None,
                inline_term: terms_fit_inline(&terms, DEFAULT_INLINE_TERM_MAX_WIDTH),
                terms,
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
        Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => 0,
    }
}

fn shift_block_indent(block: &mut Block, origin: u16) {
    if let Some(layout) = block_layout_mut(block) {
        layout.indent_columns = layout.indent_columns.saturating_sub(origin);
    }
}

fn clear_block_spacing(block: Option<&mut Block>) {
    if let Some(layout) = block.and_then(block_layout_mut) {
        layout.spacing_before_lines = 0;
    }
}

fn block_layout_mut(block: &mut Block) -> Option<&mut LayoutHint> {
    match block {
        Block::Paragraph { layout, .. }
        | Block::Preformatted { layout, .. }
        | Block::List { layout, .. }
        | Block::DefinitionList { layout, .. }
        | Block::Table { layout, .. }
        | Block::Equation { layout, .. }
        | Block::Unsupported { layout, .. } => Some(layout),
        Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => None,
    }
}

fn identify_item(
    item: &mut DefinitionItem,
    used: &mut HashSet<String>,
    reserved: &HashSet<String>,
    retained: &mut HashSet<String>,
) {
    let (role, case, names) = item.identity.as_ref().map_or_else(
        || {
            (
                DefinitionRole::Option,
                DefinitionCase::Sensitive,
                option_names(item),
            )
        },
        |identity| (identity.role, identity.case, identity.names.clone()),
    );
    if names.is_empty() {
        return;
    }

    let mut anchors = Vec::new();
    for term in &item.terms {
        collect_anchor_ids(term, &mut anchors);
    }
    retained.extend(anchors.iter().cloned());

    let existing = anchors.first().cloned();
    let preferred = existing.clone().unwrap_or_else(|| {
        format!(
            "{}-{}",
            role_id_prefix(role),
            role_name_slug(role, &names[0])
        )
    });
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
        term.insert(
            0,
            Inline::Anchor {
                id: id.clone().into(),
            },
        );
    }
    retained.insert(id.clone());
    item.identity = Some(DefinitionIdentity {
        id: id.into(),
        role,
        case,
        names,
    });
}

fn role_name_slug(role: DefinitionRole, name: &str) -> String {
    if role == DefinitionRole::Variable {
        match name {
            "$?" => return "question-mark".to_owned(),
            "$$" => return "dollar-dollar".to_owned(),
            "$^" => return "caret".to_owned(),
            "$_" => return "underscore".to_owned(),
            _ => {}
        }
    }
    slug(name)
}

const fn role_id_prefix(role: DefinitionRole) -> &'static str {
    match role {
        DefinitionRole::Option => "option",
        DefinitionRole::Command => "command",
        DefinitionRole::EnvironmentVariable => "environment",
        DefinitionRole::Variable => "variable",
    }
}

fn option_names(item: &DefinitionItem) -> Vec<String> {
    option_names_from_terms(&item.terms)
}

pub(crate) fn option_names_from_terms(terms: &[Vec<Inline>]) -> Vec<String> {
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

pub(crate) fn option_prefix(token: &str) -> Option<&str> {
    if !token.starts_with('-') || token == "-" {
        return None;
    }
    let end = token
        .char_indices()
        .skip(1)
        .take_while(|(_, character)| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '?' | '.')
        })
        .map(|(index, character)| index + character.len_utf8())
        .last()?;
    let candidate = &token[..end];
    let body = candidate.trim_start_matches('-');
    is_option_name_body(body).then_some(candidate)
}

fn is_option_name_body(value: &str) -> bool {
    value.split('.').all(|segment| {
        !segment.is_empty()
            && segment.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '?')
            })
            && segment
                .chars()
                .any(|character| character.is_ascii_alphanumeric() || character == '?')
    })
}

fn collect_anchor_ids(nodes: &[Inline], output: &mut Vec<String>) {
    struct Collector<'a>(&'a mut Vec<String>);

    impl<'ir> Visit<'ir> for Collector<'_> {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Anchor { id } = inline {
                self.0.push(id.to_string());
            }
            visit::walk_inline(self, inline);
        }
    }

    let mut collector = Collector(output);
    for node in nodes {
        collector.visit_inline(node);
    }
}

fn slug(value: &str) -> String {
    if value.trim_start_matches(['-', '/']) == "?" {
        return "help".to_owned();
    }
    let slug = value
        .trim_start_matches(['-', '/'])
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

    use mant_ir::{Block, DefinitionItem, Inline, LayoutHint, Section};

    use super::{identify_definitions, option_names, option_prefix};

    fn item(value: &str) -> DefinitionItem {
        DefinitionItem {
            identity: None,
            inline_term: false,
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
        assert_eq!(option_prefix("-ca.cert"), Some("-ca.cert"));
        assert_eq!(option_prefix("--foo.bar=VALUE"), Some("--foo.bar"));
        assert_eq!(option_prefix("--foo..bar"), None);
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
            id: "options".to_owned().into(),
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

        identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new());

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
            Block::Paragraph { layout, .. }
                if layout.indent_columns == 0 && layout.spacing_before_lines == 0
        ));
        assert_eq!(items[0].spacing_before_lines, Some(1));
        let Block::DefinitionList { items, .. } = &sections[0].blocks[1] else {
            panic!("second option should remain independently addressable");
        };
        assert_eq!(
            items[0].identity.as_ref().expect("option identity").names,
            ["-C"]
        );
    }
}
