//! Markdown-owned construction of the shared authoritative content store.

use mant_ir::{
    ContentOwnerKey, ContentOwnerKind, ContentRef, ContentRootKey, ContentRootKind, ContentStore,
    ContentStoreBuilder, ContentStyle, Inline, LinkOccurrenceKey, LinkTarget, Provenance,
    SourceSpan,
};

/// Builder retained only while Markdown structure and semantic metadata settle.
///
/// Visible bytes enter `ContentStoreBuilder` immediately.  This facade exposes
/// the narrow read/update operations needed by Markdown normalization without
/// keeping a second string-bearing inline model.
pub(super) struct MarkdownContent {
    builder: ContentStoreBuilder,
}

impl MarkdownContent {
    pub(super) const fn new() -> Self {
        Self {
            builder: ContentStoreBuilder::new(),
        }
    }

    pub(super) fn root(
        &mut self,
        kind: ContentRootKind,
        source: Option<SourceSpan>,
    ) -> InlineRoot<'_> {
        let provenance = provenance(source);
        let owner = self
            .builder
            .push_owner(ContentOwnerKind::Content, provenance);
        let root = self.builder.push_root(owner, kind, provenance);
        InlineRoot {
            builder: &mut self.builder,
            owner,
            root,
            link: None,
            style: ContentStyle::default(),
        }
    }

    pub(super) fn text(&self, content: ContentRef) -> Option<&str> {
        self.builder.text(content)
    }

    pub(super) fn replace_text(&mut self, content: &mut ContentRef, replacement: String) -> bool {
        self.builder.replace_text(content, replacement)
    }

    pub(super) fn link(&self, key: LinkOccurrenceKey) -> Option<&mant_ir::LinkOccurrence> {
        self.builder.link(key)
    }

    pub(super) fn content(&self) -> mant_ir::ContentContext<'_> {
        self.builder.content_store().content()
    }

    pub(super) fn inline_text(&self, inlines: &[Inline]) -> String {
        let mut output = String::new();
        self.append_inline_text(inlines, &mut output);
        output.trim().to_owned()
    }

    fn append_inline_text(&self, inlines: &[Inline], output: &mut String) {
        for inline in inlines {
            match inline {
                Inline::Text { content } | Inline::Code { content } => output.push_str(
                    self.text(*content)
                        .expect("Markdown inline content belongs to its active builder"),
                ),
                Inline::Strong { children } | Inline::Emphasis { children } => {
                    self.append_inline_text(children, output);
                }
                Inline::Link {
                    occurrence,
                    children,
                } => {
                    let _ = self
                        .link(*occurrence)
                        .expect("Markdown link belongs to its active builder");
                    self.append_inline_text(children, output);
                }
                Inline::Anchor { .. } => {}
                Inline::LineBreak { .. } => output.push(' '),
            }
        }
    }

    pub(super) fn finish(self) -> ContentStore {
        self.builder.finish()
    }
}

pub(super) struct InlineRoot<'builder> {
    builder: &'builder mut ContentStoreBuilder,
    owner: ContentOwnerKey,
    root: ContentRootKey,
    link: Option<LinkOccurrenceKey>,
    style: ContentStyle,
}

impl InlineRoot<'_> {
    pub(super) fn text(&mut self, value: String, source: Option<SourceSpan>) -> Inline {
        let content = self.builder.push_text(
            self.root,
            value,
            None,
            self.style,
            None,
            self.link,
            provenance(source),
        );
        Inline::Text { content }
    }

    pub(super) fn code(&mut self, value: String, source: Option<SourceSpan>) -> Inline {
        let content = self.builder.push_text(
            self.root,
            value,
            None,
            ContentStyle {
                literal: true,
                ..self.style
            },
            None,
            self.link,
            provenance(source),
        );
        Inline::Code { content }
    }

    pub(super) fn hard_break(&mut self, source: Option<SourceSpan>) -> Inline {
        Inline::LineBreak {
            atom: self
                .builder
                .push_hard_break(self.root, self.link, provenance(source)),
        }
    }

    pub(super) fn push_link(
        &mut self,
        target: LinkTarget,
        title: Option<String>,
        source: Option<SourceSpan>,
    ) -> LinkOccurrenceKey {
        self.builder
            .push_link(self.owner, target, title, provenance(source))
    }

    pub(super) fn with_link<T>(
        &mut self,
        link: LinkOccurrenceKey,
        lower: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let previous = self.link.replace(link);
        let output = lower(self);
        self.link = previous;
        output
    }

    pub(super) fn with_strong<T>(&mut self, lower: impl FnOnce(&mut Self) -> T) -> T {
        let previous = self.style.strong;
        self.style.strong = true;
        let output = lower(self);
        self.style.strong = previous;
        output
    }

    pub(super) fn with_emphasis<T>(&mut self, lower: impl FnOnce(&mut Self) -> T) -> T {
        let previous = self.style.emphasis;
        self.style.emphasis = true;
        let output = lower(self);
        self.style.emphasis = previous;
        output
    }

    pub(super) fn merge_text(&mut self, content: &mut ContentRef, suffix: &str) {
        let mut merged = self
            .builder
            .text(*content)
            .expect("Markdown inline content belongs to its active builder")
            .to_owned();
        merged.push_str(suffix);
        assert!(
            self.builder.replace_text(content, merged),
            "Markdown text merging only updates a complete atom"
        );
    }
}

fn provenance(source: Option<SourceSpan>) -> Provenance {
    source.map_or(Provenance::Unknown, |span| Provenance::Authored { span })
}
