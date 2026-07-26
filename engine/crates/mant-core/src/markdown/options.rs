//! Recognizes explicitly marked command-line options in ordinary Markdown lists.
//!
//! Markdown has no portable definition-list syntax. `ManT` therefore treats a
//! complete bullet list as semantic options only when every item starts with
//! one or more code spans containing options and an explicit description
//! delimiter, for example ``- `-h`, `--help`: Show help.``.

use mant_ast::{Block, DefinitionItem, Inline, ListItem, ListKind};

use crate::definitions::option_names_from_terms;

/// Convert unambiguous option lists without changing mixed or prose lists.
pub(super) fn normalize_option_lists(blocks: &mut Vec<Block>) {
    for block in blocks.iter_mut() {
        normalize_nested_blocks(block);
    }

    for block in blocks {
        let Block::List {
            kind: ListKind::Bullet,
            items,
            compact,
            layout,
            source,
            ..
        } = block
        else {
            continue;
        };
        if items.is_empty() {
            continue;
        }
        // Convert every item exactly once, borrowing so a mixed or prose list
        // is left untouched: a single non-option item yields None and abandons
        // the whole conversion. This replaces a probe pass that cloned each
        // item's inlines only to test them and then reparsed on success.
        let Some(definitions) = items
            .iter()
            .map(option_definition)
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        *block = Block::DefinitionList {
            items: definitions,
            compact: *compact,
            layout: *layout,
            source: *source,
        };
    }
}

fn normalize_nested_blocks(block: &mut Block) {
    match block {
        Block::List { items, .. } => {
            for item in items {
                normalize_option_lists(&mut item.blocks);
            }
        }
        Block::DefinitionList { items, .. } => {
            for item in items {
                normalize_option_lists(&mut item.description);
            }
        }
        Block::Table { rows, .. } => {
            for cell in rows.iter_mut().flat_map(|row| &mut row.cells) {
                normalize_option_lists(&mut cell.blocks);
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

/// Convert one list item into an option definition, or None if it is not one.
///
/// The item's leading paragraph is parsed exactly once; a non-option item
/// returns None so the caller can abandon the whole list without a separate
/// probing pass over cloned inlines.
fn option_definition(item: &ListItem) -> Option<DefinitionItem> {
    let Some(Block::Paragraph {
        children,
        layout,
        source,
    }) = item.blocks.first()
    else {
        return None;
    };
    let (terms, description_inlines) = split_option_signature(children)?;
    let mut description = Vec::new();
    if !description_inlines.is_empty() {
        description.push(Block::Paragraph {
            children: description_inlines,
            layout: *layout,
            source: *source,
        });
    }
    description.extend(item.blocks.iter().skip(1).cloned());

    Some(DefinitionItem {
        identity: None,
        inline_term: false,
        terms: vec![terms],
        description,
        spacing_before_lines: None,
    })
}

fn split_option_signature(children: &[Inline]) -> Option<(Vec<Inline>, Vec<Inline>)> {
    let mut terms = Vec::new();
    let mut description = Vec::new();
    let mut found_option = false;
    let mut found_delimiter = false;

    for inline in children {
        if found_delimiter {
            description.push(inline.clone());
            continue;
        }
        match inline {
            Inline::Code { value } if is_option_code(value) => {
                found_option = true;
                terms.push(Inline::Code {
                    value: value.clone(),
                });
            }
            Inline::Text { value } => {
                if let Some((before, after)) = split_delimiter(value) {
                    if !found_option || !is_alias_separator(before) {
                        return None;
                    }
                    if !before.is_empty() {
                        terms.push(Inline::Text {
                            value: before.to_owned(),
                        });
                    }
                    let after = after.trim_start();
                    if !after.is_empty() {
                        description.push(Inline::Text {
                            value: after.to_owned(),
                        });
                    }
                    found_delimiter = true;
                } else if found_option && is_alias_separator(value) {
                    terms.push(Inline::Text {
                        value: value.clone(),
                    });
                } else {
                    return None;
                }
            }
            _ => return None,
        }
    }

    (found_option && found_delimiter).then_some((terms, description))
}

fn is_option_code(value: &str) -> bool {
    let terms = vec![vec![Inline::Code {
        value: value.to_owned(),
    }]];
    !option_names_from_terms(&terms).is_empty() && value.trim_start().starts_with('-')
}

fn is_alias_separator(value: &str) -> bool {
    value
        .chars()
        .all(|character| character.is_whitespace() || matches!(character, ',' | '/' | '|'))
}

fn split_delimiter(value: &str) -> Option<(&str, &str)> {
    let (index, width) = value.char_indices().find_map(|(index, character)| {
        matches!(character, ':' | '—' | '–').then_some((index, character.len_utf8()))
    })?;
    Some((&value[..index], &value[index + width..]))
}

#[cfg(test)]
mod tests {
    use mant_ast::{Block, Inline, LayoutHint, ListItem, ListKind};

    use super::normalize_option_lists;

    fn paragraph(children: Vec<Inline>) -> Block {
        Block::Paragraph {
            children,
            layout: LayoutHint::default(),
            source: None,
        }
    }

    #[test]
    fn converts_only_complete_explicit_option_lists() {
        let option = |name: &str, description: &str| ListItem {
            blocks: vec![paragraph(vec![
                Inline::Code {
                    value: name.to_owned(),
                },
                Inline::Text {
                    value: format!(": {description}"),
                },
            ])],
        };
        let mut blocks = vec![Block::List {
            kind: ListKind::Bullet,
            start: None,
            compact: true,
            items: vec![
                option("-h, --help", "Show help."),
                option("--version", "Print version."),
            ],
            layout: LayoutHint::default(),
            source: None,
        }];

        normalize_option_lists(&mut blocks);

        let Block::DefinitionList { items, .. } = &blocks[0] else {
            panic!("explicit option list should become definitions");
        };
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| item.identity.is_none()));
        assert!(matches!(
            &items[0].description[0],
            Block::Paragraph { children, .. }
                if matches!(&children[0], Inline::Text { value } if value == "Show help.")
        ));
    }

    #[test]
    fn leaves_mixed_lists_unchanged() {
        let mut blocks = vec![Block::List {
            kind: ListKind::Bullet,
            start: None,
            compact: true,
            items: vec![
                ListItem {
                    blocks: vec![paragraph(vec![
                        Inline::Code {
                            value: "--color".to_owned(),
                        },
                        Inline::Text {
                            value: ": Control colour.".to_owned(),
                        },
                    ])],
                },
                ListItem {
                    blocks: vec![paragraph(vec![Inline::Text {
                        value: "ordinary prose".to_owned(),
                    }])],
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        }];

        normalize_option_lists(&mut blocks);

        assert!(matches!(&blocks[0], Block::List { .. }));
    }
}
