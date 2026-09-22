//! Conservative validation of response-relative locations from any producer.
use super::{ExplanationBlockStep as Step, ExplanationContentRange, ExplanationFormRange};
use mant_ir::{Block, ContentContext, Inline};

/// A borrowed original text root; not a flattened compound block or rendered line.
#[derive(Debug, Clone, Copy)]
pub enum ExplanationTextRoot<'a> {
    /// One paragraph, preformatted block or definition term's visible inlines.
    Inline(&'a [Inline]),
    /// One equation or preserved unsupported-source leaf.
    Text(&'a str),
}

impl<'a> ExplanationTextRoot<'a> {
    /// Canonical safe text whose Unicode scalars define the location domain.
    #[must_use]
    pub fn safe_text(self, content: ContentContext<'a>) -> Option<String> {
        let mut text = String::new();
        let mut append = |value: &str| {
            text.extend(value.chars().map(|c| {
                if c.is_control() && !matches!(c, '\n' | '\t') {
                    '\u{fffd}'
                } else {
                    c
                }
            }));
        };
        match self {
            Self::Inline(nodes) => content.visit_plain_text(nodes, &mut append).ok()?,
            Self::Text(value) => append(value),
        }
        Some(text)
    }

    fn scalar_len(self, content: ContentContext<'a>) -> Option<usize> {
        match self {
            Self::Text(value) => Some(value.chars().count()),
            Self::Inline(nodes) => content.scalar_len(nodes).ok(),
        }
    }
}

impl ExplanationContentRange {
    /// Validated borrowed target rooted in a returned content block.
    /// Invalid paths, wrong owner kinds, empty/inverted or out-of-range spans
    /// return None. Consumers must not compensate by searching the text again.
    #[must_use]
    pub fn resolve<'a>(
        &self,
        context: ContentContext<'a>,
        block: &'a Block,
    ) -> Option<ExplanationTextRoot<'a>> {
        let root = match self {
            Self::BlockText { path, .. } => match block_at(block, path)? {
                Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                    ExplanationTextRoot::Inline(children)
                }
                Block::Equation { value, .. } | Block::Unsupported { text: value, .. } => {
                    ExplanationTextRoot::Text(value)
                }
                _ => return None,
            },
            Self::DefinitionTerm {
                path,
                item_index,
                term_index,
                ..
            } => {
                let Block::DefinitionList { items, .. } = block_at(block, path)? else {
                    return None;
                };
                ExplanationTextRoot::Inline(
                    items
                        .get(*item_index as usize)?
                        .terms
                        .get(*term_index as usize)?,
                )
            }
        };
        let range = self.char_range();
        (range.start < range.end && range.end <= root.scalar_len(context)?).then_some(root)
    }

    /// Half-open scalar range, to be used only after target validation.
    #[must_use]
    pub fn char_range(&self) -> std::ops::Range<usize> {
        let (Self::BlockText {
            start_char,
            end_char,
            ..
        }
        | Self::DefinitionTerm {
            start_char,
            end_char,
            ..
        }) = self;
        *start_char as usize..*end_char as usize
    }
}

impl ExplanationFormRange {
    /// Resolve a complete, nonempty scalar range in returned metadata forms.
    #[must_use]
    pub fn resolve<'a>(
        &self,
        content: ContentContext<'a>,
        forms: &'a [Vec<Inline>],
    ) -> Option<&'a [Inline]> {
        let form = forms.get(self.form_index as usize)?;
        (self.start_char < self.end_char
            && self.end_char as usize <= ExplanationTextRoot::Inline(form).scalar_len(content)?)
        .then_some(form)
    }
}

pub(super) fn block_at<'a>(block: &'a Block, path: &[Step]) -> Option<&'a Block> {
    mant_ir::resolve_block_descendant(block, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_text_preserves_scalar_positions_and_layout_controls() {
        let raw = "\0A\r\x1b\t\n\u{85}e\u{301}👩‍💻";
        let expected = "\u{fffd}A\u{fffd}\u{fffd}\t\n\u{fffd}e\u{301}👩‍💻";
        let projection = mant_ir::ContentProjection {
            content_store: mant_ir::ContentStore::default(),
        };
        let root = ExplanationTextRoot::Text(raw);
        assert_eq!(
            root.safe_text(projection.content()).as_deref(),
            Some(expected)
        );
        assert_eq!(
            root.scalar_len(projection.content()),
            Some(expected.chars().count())
        );
        assert_eq!(raw.chars().count(), expected.chars().count());
    }

    #[test]
    fn safe_inline_text_uses_only_original_visible_leaves() {
        let mut store = mant_ir::ContentStoreBuilder::new();
        let owner = store.push_owner(
            mant_ir::ContentOwnerKind::Content,
            mant_ir::Provenance::Unknown,
        );
        let root = store.push_root(
            owner,
            mant_ir::ContentRootKind::Body,
            mant_ir::Provenance::Unknown,
        );
        let point = store.push_point(
            root,
            mant_ir::PointBoundary::BetweenAtoms { atom_boundary: 0 },
            0,
            mant_ir::Provenance::Unknown,
        );
        let empty = store.push_text(
            root,
            String::new(),
            None,
            mant_ir::ContentStyle::default(),
            None,
            None,
            mant_ir::Provenance::Unknown,
        );
        let link = store.push_link(
            owner,
            mant_ir::LinkTarget::External {
                uri: "https://not-visible.test".into(),
            },
            Some("not visible".into()),
            mant_ir::Provenance::Unknown,
        );
        let code = store.push_text(
            root,
            "e\u{301}👩‍💻\r".into(),
            None,
            mant_ir::ContentStyle::default(),
            None,
            Some(link),
            mant_ir::Provenance::Unknown,
        );
        let hard_break = store.push_hard_break(root, None, mant_ir::Provenance::Unknown);
        let tail = store.push_text(
            root,
            "尾\t\0".into(),
            None,
            mant_ir::ContentStyle::default(),
            None,
            None,
            mant_ir::Provenance::Unknown,
        );
        let nodes = vec![
            Inline::anchor(point, "invisible-target"),
            Inline::Strong {
                children: vec![
                    Inline::Text { content: empty },
                    Inline::Emphasis {
                        children: vec![Inline::Link {
                            occurrence: link,
                            children: vec![Inline::Code { content: code }],
                        }],
                    },
                ],
            },
            Inline::LineBreak { atom: hard_break },
            Inline::Text { content: tail },
            Inline::Emphasis { children: vec![] },
        ];
        let projection = mant_ir::ContentProjection {
            content_store: store.finish(),
        };
        let expected = "e\u{301}👩‍💻\u{fffd}\n尾\t\u{fffd}";
        let root = ExplanationTextRoot::Inline(&nodes);
        assert_eq!(
            root.safe_text(projection.content()).as_deref(),
            Some(expected)
        );
        assert_eq!(
            root.scalar_len(projection.content()),
            Some(expected.chars().count())
        );

        let form_range = ExplanationFormRange {
            form_index: 0,
            start_char: 0,
            end_char: u32::try_from(expected.chars().count()).expect("small fixture"),
        };
        let forms = vec![nodes];
        let resolved = form_range
            .resolve(projection.content(), &forms)
            .expect("unchanged scalar domain");
        assert!(std::ptr::eq(resolved, forms[0].as_slice()));
        assert_eq!(
            ExplanationTextRoot::Inline(resolved)
                .safe_text(projection.content())
                .as_deref(),
            Some(expected)
        );
    }
}
