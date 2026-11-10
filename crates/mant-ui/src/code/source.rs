//! One bounded original-text projection with orthogonal source markup ranges.

use std::borrow::Cow;
use std::ops::Range;

use mant_ir::Inline;

#[derive(Default)]
pub(super) struct Source {
    pub(super) text: String,
    pub(super) marked: Vec<Range<usize>>,
    pub(super) emphasis: Vec<Range<usize>>,
    pub(super) code: Vec<Range<usize>>,
    pub(super) protected: Vec<Range<usize>>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum LeafKind {
    #[default]
    Text,
    Code,
    Equation,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Markup {
    pub(crate) marked: bool,
    pub(crate) emphasis: bool,
    pub(crate) kind: LeafKind,
}

pub(crate) fn visit(nodes: &[Inline], mut emit: impl FnMut(Markup, &str) -> bool) -> bool {
    walk(nodes, Markup::default(), &mut emit)
}

fn walk(nodes: &[Inline], markup: Markup, emit: &mut impl FnMut(Markup, &str) -> bool) -> bool {
    for node in nodes {
        let complete = match node {
            Inline::Text { value } => emit(markup, value),
            Inline::Code { value } => emit(
                Markup {
                    marked: true,
                    kind: LeafKind::Code,
                    ..markup
                },
                value,
            ),
            Inline::Equation { value, .. } => emit(
                Markup {
                    kind: LeafKind::Equation,
                    ..markup
                },
                value,
            ),
            Inline::LineBreak { .. } => emit(markup, "\n"),
            Inline::Anchor { .. } => true,
            Inline::Strong { children } => walk(
                children,
                Markup {
                    marked: true,
                    ..markup
                },
                emit,
            ),
            Inline::Emphasis { children } => walk(
                children,
                Markup {
                    marked: true,
                    emphasis: true,
                    ..markup
                },
                emit,
            ),
            Inline::Link { children, .. } => walk(children, markup, emit),
        };
        if !complete {
            return false;
        }
    }
    true
}

impl Source {
    /// Math is an opaque semantic owner, not code punctuation. The private
    /// scan view preserves byte offsets; rendering always uses original text.
    pub(super) fn syntax_text(&self) -> Cow<'_, str> {
        syntax_text(&self.text, &self.protected)
    }

    pub(super) fn collect(nodes: &[Inline]) -> Option<Self> {
        let mut source = Self::default();
        visit(nodes, |markup, text| source.push(text, markup)).then_some(source)
    }

    fn push(&mut self, text: &str, markup: Markup) -> bool {
        if text.len() > super::MAX_BLOCK_BYTES - self.text.len() {
            return false;
        }
        let start = self.text.len();
        self.text.push_str(text);
        let range = start..self.text.len();
        for (enabled, ranges) in [
            (markup.marked, &mut self.marked),
            (markup.emphasis, &mut self.emphasis),
            (markup.kind == LeafKind::Code, &mut self.code),
            (markup.kind == LeafKind::Equation, &mut self.protected),
        ] {
            if enabled && !range.is_empty() {
                if let Some(previous) = ranges.last_mut()
                    && previous.end == start
                {
                    previous.end = range.end;
                } else {
                    ranges.push(range.clone());
                }
            }
        }
        true
    }
}

pub(crate) fn syntax_text<'a>(text: &'a str, protected: &[Range<usize>]) -> Cow<'a, str> {
    if protected.is_empty() {
        return Cow::Borrowed(text);
    }
    let mut bytes = text.as_bytes().to_vec();
    for range in protected {
        bytes[range.clone()].fill(b' ');
    }
    Cow::Owned(String::from_utf8(bytes).expect("whole protected inline owners"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_text_projection_merges_fonts_and_distinguishes_math_from_code() {
        let nodes = vec![Inline::Strong { children: vec![
            Inline::Text { value: "-fo".into() },
            Inline::Emphasis { children: vec![Inline::Text { value: "o".into() }] },
            Inline::Code { value: "=bar".into() },
            Inline::anchor("here"), Inline::line_break(),
            Inline::Equation { value: "-N".into(), expression: serde_json::from_str(r#"{"kind":"text","font":"none","operator":"none","text":"-N","children":[]}"#).unwrap() },
        ] }];
        let source = Source::collect(&nodes).unwrap();
        let mut original = String::new();
        mant_render::visit_inline_text(&nodes, &[], |_, _, text| original.push_str(text));
        assert_eq!(source.text, original);
        assert_eq!(source.text, "-foo=bar\n-N");
        assert_eq!(source.marked, std::iter::once(0..11).collect::<Vec<_>>());
        assert_eq!(source.emphasis, std::iter::once(3..4).collect::<Vec<_>>());
        assert_eq!(source.code, std::iter::once(4..8).collect::<Vec<_>>());
        assert_eq!(source.protected, std::iter::once(9..11).collect::<Vec<_>>());
        assert_eq!(source.syntax_text(), "-foo=bar\n  ");
    }
}
