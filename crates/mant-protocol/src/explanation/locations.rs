//! Conservative validation of response-relative locations from any producer.
use super::{ExplanationBlockStep as Step, ExplanationContentRange, ExplanationFormRange};
use mant_ir::{Block, Inline};

/// A borrowed original text root; not a flattened compound block or rendered line.
#[derive(Debug, Clone, Copy)]
pub enum ExplanationTextRoot<'a> {
    /// One paragraph, preformatted block or definition term's visible inlines.
    Inline(&'a [Inline]),
    /// One equation or preserved unsupported-source leaf.
    Text(&'a str),
}

impl ExplanationTextRoot<'_> {
    /// Canonical safe text whose Unicode scalars define the location domain.
    #[must_use]
    pub fn safe_text(self) -> String {
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
            Self::Inline(nodes) => {
                crate::visit_inline_text(nodes, &[], |_, _, value| append(value));
            }
            Self::Text(value) => append(value),
        }
        text
    }

    fn scalar_len(self) -> usize {
        match self {
            Self::Text(value) => value.chars().count(),
            Self::Inline(nodes) => {
                let mut count = 0;
                crate::visit_inline_text(nodes, &[], |_, _, value| count += value.chars().count());
                count
            }
        }
    }
}

impl ExplanationContentRange {
    /// Validated borrowed target rooted in a returned content block.
    /// Invalid paths, wrong owner kinds, empty/inverted or out-of-range spans
    /// return None. Consumers must not compensate by searching the text again.
    #[must_use]
    pub fn resolve<'a>(&self, content: &'a Block) -> Option<ExplanationTextRoot<'a>> {
        let root = match self {
            Self::BlockText { path, .. } => match block_at(content, path)? {
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
                let Block::DefinitionList { items, .. } = block_at(content, path)? else {
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
        (range.start < range.end && range.end <= root.scalar_len()).then_some(root)
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
    pub fn resolve<'a>(&self, forms: &'a [Vec<Inline>]) -> Option<&'a [Inline]> {
        let form = forms.get(self.form_index as usize)?;
        (self.start_char < self.end_char
            && self.end_char as usize <= ExplanationTextRoot::Inline(form).scalar_len())
        .then_some(form)
    }
}

pub(super) fn block_at<'a>(block: &'a Block, path: &[Step]) -> Option<&'a Block> {
    mant_ir::resolve_block_descendant(block, path)
}
