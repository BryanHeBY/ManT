//! Borrowed source markup and semantic decoration, independent of line layout.
use super::InlineNameRange;
use mant_ir::{
    ContentAtomKind, ContentContext, ContentReadError, EntryKind, Inline, InlineView,
    LinkOccurrenceKey, LinkTarget,
};

/// Store-resolved identity and destination for one visible link fragment.
///
/// `occurrence` is the identity used by navigation and hit testing. Equal
/// destinations do not imply equal occurrences, while fragments in different
/// roots may intentionally share one occurrence.
#[derive(Debug, Clone, Copy)]
pub struct InlineLink<'store> {
    /// Document- or projection-local logical occurrence identity.
    pub occurrence: LinkOccurrenceKey,
    /// Typed destination resolved from the authoritative content store.
    pub target: &'store LinkTarget,
}

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
    /// This span is an IR-authored structural line break, not untrusted text.
    pub structural_break: bool,
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
///
/// # Errors
///
/// Returns [`ContentReadError`] when a retained inline key does not resolve in
/// the supplied content context.
pub fn visit_inline_text<'a>(
    content: ContentContext<'a>,
    nodes: &'a [Inline],
    names: &[InlineNameRange],
    emit: impl FnMut(InlinePresentation, Option<InlineLink<'a>>, &'a str),
) -> Result<(), ContentReadError> {
    visit_inline_text_with(content, nodes, names, emit)
}

/// Compatibility spelling for [`visit_inline_text`].
///
/// Both entry points require an authoritative document or response-local
/// projection context; no detached inline fallback exists.
///
/// # Errors
///
/// Returns [`ContentReadError`] when a retained inline key does not resolve in
/// the supplied content context.
pub fn visit_inline_text_with<'store>(
    content: ContentContext<'store>,
    nodes: &'store [Inline],
    names: &[InlineNameRange],
    mut emit: impl FnMut(InlinePresentation, Option<InlineLink<'store>>, &'store str),
) -> Result<(), ContentReadError> {
    visit_inline_display_text(content, nodes, names, |style, link, logical, _| {
        emit(style, link, logical);
    })
}

/// Visit logical text and its optional profile-specific glyph projection.
///
/// `display_override` never changes the logical scalar cursor, entry-name
/// ranges, or link identity. A native projection occupies one logical atom;
/// callers may use its glyphs only for display and must retain `logical` for
/// search and copying. A partial slice of a non-linear projection cannot be
/// mapped without an atom-internal map, so it keeps its logical glyphs.
///
/// # Errors
///
/// Returns [`ContentReadError`] for an unresolved content key or range.
pub fn visit_inline_display_text<'store>(
    content: ContentContext<'store>,
    nodes: &'store [Inline],
    names: &[InlineNameRange],
    mut emit: impl FnMut(
        InlinePresentation,
        Option<InlineLink<'store>>,
        &'store str,
        Option<&'store str>,
    ),
) -> Result<(), ContentReadError> {
    let length = if names.is_empty() {
        0
    } else {
        content.scalar_len(nodes)?
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
    walk_with(
        content,
        nodes,
        InlinePresentation::default(),
        None,
        names,
        &mut cursor,
        &mut emit,
    )
}

fn walk_with<'store>(
    content: ContentContext<'store>,
    nodes: &'store [Inline],
    style: InlinePresentation,
    link: Option<InlineLink<'store>>,
    names: &[InlineNameRange],
    cursor: &mut usize,
    emit: &mut impl FnMut(
        InlinePresentation,
        Option<InlineLink<'store>>,
        &'store str,
        Option<&'store str>,
    ),
) -> Result<(), ContentReadError> {
    for node in nodes {
        match content.inline(node)? {
            InlineView::Text(value) => text(
                value,
                display_override(content, node),
                style,
                link,
                names,
                cursor,
                emit,
            ),
            InlineView::Code(value) => text(
                value,
                display_override(content, node),
                InlinePresentation {
                    code: true,
                    ..style
                },
                link,
                names,
                cursor,
                emit,
            ),
            InlineView::Strong(children) => walk_with(
                content,
                children,
                InlinePresentation {
                    strong: true,
                    ..style
                },
                link,
                names,
                cursor,
                emit,
            )?,
            InlineView::Emphasis(children) => walk_with(
                content,
                children,
                InlinePresentation {
                    emphasis: true,
                    ..style
                },
                link,
                names,
                cursor,
                emit,
            )?,
            InlineView::Link(link) => walk_with(
                content,
                link.children(),
                InlinePresentation {
                    link: true,
                    ..style
                },
                Some(InlineLink {
                    occurrence: link.occurrence(),
                    target: link.target(),
                }),
                names,
                cursor,
                emit,
            )?,
            InlineView::LineBreak => text(
                "\n",
                None,
                InlinePresentation {
                    structural_break: true,
                    ..style
                },
                link,
                names,
                cursor,
                emit,
            ),
            InlineView::Anchor(_) => {}
            _ => return Err(ContentReadError),
        }
    }
    Ok(())
}

fn text<'a>(
    value: &'a str,
    display_override: Option<&'a str>,
    style: InlinePresentation,
    link: Option<InlineLink<'a>>,
    names: &[InlineNameRange],
    cursor: &mut usize,
    emit: &mut impl FnMut(InlinePresentation, Option<InlineLink<'a>>, &'a str, Option<&'a str>),
) {
    if names.is_empty() {
        *cursor += value.chars().count();
        if !value.is_empty() {
            emit(style, link, value, display_override);
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
                    link,
                    &value[start..offset],
                    (start == 0 && offset == value.len())
                        .then_some(display_override)
                        .flatten(),
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
            link,
            &value[start..],
            (start == 0).then_some(display_override).flatten(),
        );
    }
}

fn display_override<'a>(content: ContentContext<'a>, node: &Inline) -> Option<&'a str> {
    let (Inline::Text { content: range } | Inline::Code { content: range }) = node else {
        return None;
    };
    let atom = content.atom(range.atom)?;
    let (ContentAtomKind::Text {
        text,
        display_override: Some(display),
    }
    | ContentAtomKind::Whitespace {
        text,
        display_override: Some(display),
        ..
    }) = &atom.kind
    else {
        return None;
    };
    (range.bytes.start == 0 && range.bytes.end as usize == text.len()).then_some(display.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document_with_nested_markup() -> mant_ir::Document {
        let linked = crate::test_content::link(
            LinkTarget::External {
                uri: "https://example.test".into(),
            },
            None,
            vec![Inline::Strong {
                children: vec![crate::test_content::code("é名 rest")],
            }],
        );
        mant_ir::Document {
            parser: None,
            sources: vec![mant_ir::SourceRecord {
                key: mant_ir::SourceKey::FIRST,
                identity: mant_ir::SourceIdentity::Anonymous {
                    name: "test".into(),
                },
                format: mant_ir::SourceFormat::Markdown,
                decoded_byte_length: 0,
                content_sha256: None,
                coordinates: mant_ir::SourceCoordinates::DecodedUtf8Bytes,
            }],
            root_source: mant_ir::SourceKey::FIRST,
            meta: mant_ir::DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            body: mant_ir::DocumentBody::Flow(mant_ir::FlowBody {
                content_store: crate::test_content::store(),
                heading: Some(mant_ir::Heading {
                    content: vec![Inline::Emphasis {
                        children: vec![linked],
                    }],
                    source: None,
                }),
                blocks: Vec::new(),
                sections: Vec::new(),
            }),
        }
    }

    #[test]
    fn nested_markup_and_split_name_remain_orthogonal() {
        let nodes = [Inline::Emphasis {
            children: vec![crate::test_content::link(
                LinkTarget::External {
                    uri: "https://example.test".into(),
                },
                None,
                vec![Inline::Strong {
                    children: vec![crate::test_content::code("é名 rest")],
                }],
            )],
        }];
        let mut spans = Vec::new();
        visit_inline_text(
            crate::test_content::content(),
            &nodes,
            &[InlineNameRange {
                chars: 0..2,
                kind: EntryKind::Command,
            }],
            |style, link, text| {
                spans.push((style, link.is_some(), text.to_owned()));
            },
        )
        .expect("fixture content resolves");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].2, "é名");
        assert_eq!(spans[0].0.entry_kind, Some(EntryKind::Command));
        assert_eq!(spans[1].0.entry_kind, None);
        for (style, link, _) in spans {
            assert!(style.strong && style.emphasis && style.code && style.link && link);
        }
    }

    #[test]
    fn contextual_visit_preserves_nested_roles_and_borrowed_link_target() {
        let document = document_with_nested_markup();
        let nodes = &document
            .flow()
            .expect("flow fixture")
            .heading
            .as_ref()
            .expect("heading")
            .content;
        let mut spans = Vec::new();
        visit_inline_text_with(
            document.content(),
            nodes,
            &[InlineNameRange {
                chars: 0..2,
                kind: EntryKind::Command,
            }],
            |style, link, text| spans.push((style, link.is_some(), text.to_owned())),
        )
        .expect("document content resolves");

        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].2, "é名");
        assert_eq!(spans[0].0.entry_kind, Some(EntryKind::Command));
        assert_eq!(spans[1].0.entry_kind, None);
        for (style, link, _) in spans {
            assert!(style.strong && style.emphasis && style.code && style.link && link);
        }
    }

    #[test]
    fn profile_projection_does_not_replace_logical_text_for_callers() {
        // Pinned CVS term.c::term_word/encode1: ASCII `\[em]` displays
        // `--` but keeps one logical em-dash scalar in the collector.
        let mut builder = mant_ir::ContentStoreBuilder::new();
        let owner = builder.push_owner(
            mant_ir::ContentOwnerKind::Document,
            mant_ir::Provenance::Unknown,
        );
        let root = builder.push_root(
            owner,
            mant_ir::ContentRootKind::Body,
            mant_ir::Provenance::Unknown,
        );
        let content = builder.push_text(
            root,
            "—".to_owned(),
            Some("--".to_owned()),
            mant_ir::ContentStyle::default(),
            None,
            None,
            mant_ir::Provenance::Unknown,
        );
        let projection = mant_ir::ContentProjection {
            content_store: builder.finish(),
        };
        let nodes = [Inline::Text { content }];
        let mut logical = String::new();
        visit_inline_text(projection.content(), &nodes, &[], |_, _, text| {
            logical.push_str(text);
        })
        .expect("logical projection resolves");
        let mut glyphs = String::new();
        visit_inline_display_text(projection.content(), &nodes, &[], |_, _, text, display| {
            glyphs.push_str(display.unwrap_or(text));
        })
        .expect("display projection resolves");
        assert_eq!(logical, "—");
        assert_eq!(glyphs, "--");
    }
}
