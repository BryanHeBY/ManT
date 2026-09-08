//! Borrowed source markup and semantic decoration, independent of line layout.
use super::InlineNameRange;
use mant_ir::{EntryKind, Inline, LinkTarget};

/// Orthogonal source modifiers and an optional validated name role.
/// No colors, terminal state, query matching or persistent IR are stored here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
// Independent modifiers combine freely; they are not mutually exclusive states.
#[allow(clippy::struct_excessive_bools)]
pub struct InlinePresentation {
    /// A source Strong wrapper encloses this text.
    pub strong: bool,
    /// A source Emphasis wrapper encloses this text.
    pub emphasis: bool,
    /// This is the visible text of a source Code node.
    pub code: bool,
    /// A source link encloses this text; the target is supplied separately.
    pub link: bool,
    /// This exact source range belongs to a validated semantic name.
    pub entry_kind: Option<EntryKind>,
}

/// Visit original visible text with composable source and name roles.
///
/// `names` must be source-bound ranges for exactly this inline root, normally
/// obtained from [`super::EntryStyleMap`]. Positions count Unicode scalars,
/// including source line breaks; no text searching, layout or escaping occurs.
/// Ranges must be ordered and nonoverlapping; malformed decoration is ignored.
/// The callback receives borrowed pieces and never needs to reconstruct styles
/// from output text. A link target remains attached across all styled pieces.
pub fn visit_inline_text<'a>(
    nodes: &'a [Inline],
    names: &[InlineNameRange],
    mut emit: impl FnMut(InlinePresentation, Option<&'a LinkTarget>, &'a str),
) {
    let length = if names.is_empty() {
        0
    } else {
        super::bindings::scalar_len(nodes)
    };
    let names = if names
        .iter()
        .all(|name| name.chars.start < name.chars.end && name.chars.end <= length)
        && names
            .windows(2)
            .all(|pair| pair[0].chars.end <= pair[1].chars.start)
    {
        names
    } else {
        &[]
    };
    let mut cursor = 0;
    walk(
        nodes,
        InlinePresentation::default(),
        None,
        names,
        &mut cursor,
        &mut emit,
    );
}

fn walk<'a>(
    nodes: &'a [Inline],
    style: InlinePresentation,
    target: Option<&'a LinkTarget>,
    names: &[InlineNameRange],
    cursor: &mut usize,
    emit: &mut impl FnMut(InlinePresentation, Option<&'a LinkTarget>, &'a str),
) {
    for node in nodes {
        match node {
            Inline::Text { value } => text(value, style, target, names, cursor, emit),
            Inline::Code { value } => text(
                value,
                InlinePresentation {
                    code: true,
                    ..style
                },
                target,
                names,
                cursor,
                emit,
            ),
            Inline::Strong { children } => walk(
                children,
                InlinePresentation {
                    strong: true,
                    ..style
                },
                target,
                names,
                cursor,
                emit,
            ),
            Inline::Emphasis { children } => walk(
                children,
                InlinePresentation {
                    emphasis: true,
                    ..style
                },
                target,
                names,
                cursor,
                emit,
            ),
            Inline::Link {
                children, target, ..
            } => walk(
                children,
                InlinePresentation {
                    link: true,
                    ..style
                },
                Some(target),
                names,
                cursor,
                emit,
            ),
            Inline::LineBreak => text("\n", style, target, names, cursor, emit),
            Inline::Anchor { .. } => {}
        }
    }
}

fn text<'a>(
    value: &'a str,
    style: InlinePresentation,
    target: Option<&'a LinkTarget>,
    names: &[InlineNameRange],
    cursor: &mut usize,
    emit: &mut impl FnMut(InlinePresentation, Option<&'a LinkTarget>, &'a str),
) {
    if names.is_empty() {
        *cursor += value.chars().count();
        if !value.is_empty() {
            emit(style, target, value);
        }
        return;
    }
    let mut start = 0;
    let mut kind = None;
    for (offset, _) in value.char_indices() {
        let index = names.partition_point(|name| name.chars.end <= *cursor);
        let next = names
            .get(index)
            .filter(|name| name.chars.contains(cursor))
            .map(|name| name.kind);
        if next != kind {
            if offset > start {
                emit(
                    InlinePresentation {
                        entry_kind: kind,
                        ..style
                    },
                    target,
                    &value[start..offset],
                );
            }
            start = offset;
            kind = next;
        }
        *cursor += 1;
    }
    if start < value.len() {
        emit(
            InlinePresentation {
                entry_kind: kind,
                ..style
            },
            target,
            &value[start..],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_markup_and_split_name_remain_orthogonal() {
        let nodes = [Inline::Emphasis {
            children: vec![Inline::Link {
                target: LinkTarget::External {
                    uri: "https://example.test".into(),
                },
                title: None,
                children: vec![Inline::Strong {
                    children: vec![Inline::Code {
                        value: "é名 rest".into(),
                    }],
                }],
            }],
        }];
        let mut spans = Vec::new();
        visit_inline_text(
            &nodes,
            &[InlineNameRange {
                chars: 0..2,
                kind: EntryKind::Command,
            }],
            |style, link, text| {
                spans.push((style, link.is_some(), text.to_owned()));
            },
        );
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].2, "é名");
        assert_eq!(spans[0].0.entry_kind, Some(EntryKind::Command));
        assert_eq!(spans[1].0.entry_kind, None);
        for (style, link, _) in spans {
            assert!(style.strong && style.emphasis && style.code && style.link && link);
        }
    }
}
