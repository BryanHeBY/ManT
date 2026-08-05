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
        // Plan every signature before taking ownership so a mixed or prose
        // list remains untouched. Plans retain only delimiter coordinates:
        // successful conversion can then move the original AST exactly once,
        // including potentially large nested description blocks.
        let Some(signatures) = items
            .iter()
            .map(option_signature)
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let definitions = std::mem::take(items)
            .into_iter()
            .zip(signatures)
            .map(|(item, signature)| option_definition(item, signature))
            .collect();
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

#[derive(Clone, Copy)]
struct OptionSignature {
    inline_index: usize,
    byte_index: usize,
    width: usize,
}

/// Validate one leading paragraph and record how to split it after ownership
/// moves out of the source list.
fn option_signature(item: &ListItem) -> Option<OptionSignature> {
    let Some(Block::Paragraph { children, .. }) = item.blocks.first() else {
        return None;
    };
    let mut found_option = false;
    for (delimiter_inline, inline) in children.iter().enumerate() {
        match inline {
            Inline::Code { value } if is_option_code(value) => {
                found_option = true;
            }
            Inline::Text { value } => {
                if let Some((delimiter_byte, delimiter_width)) = delimiter_location(value) {
                    if !found_option || !is_alias_separator(&value[..delimiter_byte]) {
                        return None;
                    }
                    return Some(OptionSignature {
                        inline_index: delimiter_inline,
                        byte_index: delimiter_byte,
                        width: delimiter_width,
                    });
                }
                if !found_option || !is_alias_separator(value) {
                    return None;
                }
            }
            _ => return None,
        }
    }
    None
}

/// Move one previously validated item into its semantic definition.
fn option_definition(item: ListItem, signature: OptionSignature) -> DefinitionItem {
    let mut blocks = item.blocks.into_iter();
    let Some(Block::Paragraph {
        children,
        layout,
        source,
    }) = blocks.next()
    else {
        unreachable!("option_signature accepts only a leading paragraph");
    };
    let (terms, description_inlines) = apply_option_signature(children, signature);
    let mut description = Vec::new();
    if !description_inlines.is_empty() {
        description.push(Block::Paragraph {
            children: description_inlines,
            layout,
            source,
        });
    }
    description.extend(blocks);

    DefinitionItem {
        identity: None,
        inline_term: false,
        terms: vec![terms],
        description,
        spacing_before_lines: None,
    }
}

fn apply_option_signature(
    children: Vec<Inline>,
    signature: OptionSignature,
) -> (Vec<Inline>, Vec<Inline>) {
    let mut terms = Vec::new();
    let mut description = Vec::new();
    for (index, inline) in children.into_iter().enumerate() {
        if index < signature.inline_index {
            terms.push(inline);
            continue;
        }
        if index > signature.inline_index {
            description.push(inline);
            continue;
        }
        let Inline::Text { value } = inline else {
            unreachable!("option_signature records a text delimiter");
        };
        let after_start = signature.byte_index + signature.width;
        let before = &value[..signature.byte_index];
        if !before.is_empty() {
            terms.push(Inline::Text {
                value: before.to_owned(),
            });
        }
        let after = value[after_start..].trim_start();
        if !after.is_empty() {
            description.push(Inline::Text {
                value: after.to_owned(),
            });
        }
    }
    (terms, description)
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

fn delimiter_location(value: &str) -> Option<(usize, usize)> {
    value.char_indices().find_map(|(index, character)| {
        matches!(character, ':' | '—' | '–').then_some((index, character.len_utf8()))
    })
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
    fn moves_trailing_description_blocks_into_the_definition() {
        let mut blocks = vec![Block::List {
            kind: ListKind::Bullet,
            start: None,
            compact: false,
            items: vec![ListItem {
                blocks: vec![
                    paragraph(vec![
                        Inline::Code {
                            value: "--config".to_owned(),
                        },
                        Inline::Text {
                            value: ": Read configuration.".to_owned(),
                        },
                    ]),
                    Block::Preformatted {
                        children: vec![Inline::Text {
                            value: "tool --config path".to_owned(),
                        }],
                        language: None,
                        layout: LayoutHint::default(),
                        source: None,
                    },
                ],
            }],
            layout: LayoutHint::default(),
            source: None,
        }];

        normalize_option_lists(&mut blocks);

        let Block::DefinitionList { items, .. } = &blocks[0] else {
            panic!("explicit option list should become definitions");
        };
        assert!(matches!(
            items[0].description.as_slice(),
            [Block::Paragraph { .. }, Block::Preformatted { children, .. }]
                if matches!(&children[0], Inline::Text { value } if value == "tool --config path")
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
        let original = blocks.clone();

        normalize_option_lists(&mut blocks);

        assert_eq!(blocks, original, "a rejected mixed list remains untouched");
    }
}
