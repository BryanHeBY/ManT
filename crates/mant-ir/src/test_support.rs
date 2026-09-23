//! Shared constructors for key-backed unit-test topology.
#![allow(dead_code, missing_docs)]

use crate::{
    ContentOwnerKey, ContentOwnerKind, ContentRef, ContentRootKey, ContentRootKind, ContentStore,
    ContentStoreBuilder, ContentStyle, Inline, LinkOccurrenceKey, LinkTarget, PointBoundary,
    Provenance,
};
use serde_json::Value;

pub(crate) struct ContentFixture {
    builder: ContentStoreBuilder,
    owner: ContentOwnerKey,
    root: ContentRootKey,
}

impl ContentFixture {
    pub(crate) fn new(kind: ContentRootKind) -> Self {
        let mut builder = ContentStoreBuilder::new();
        let owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
        let root = builder.push_root(owner, kind, Provenance::Unknown);
        Self {
            builder,
            owner,
            root,
        }
    }

    pub(crate) fn body() -> Self {
        Self::new(ContentRootKind::Body)
    }

    pub(crate) fn root(&self) -> ContentRootKey {
        self.root
    }

    pub(crate) fn new_root(&mut self, kind: ContentRootKind) -> ContentRootKey {
        self.builder
            .push_root(self.owner, kind, Provenance::Unknown)
    }

    pub(crate) fn text(&mut self, value: impl Into<String>) -> Inline {
        let content = self.text_ref(value, ContentStyle::default(), None);
        Inline::Text { content }
    }

    pub(crate) fn styled_text(&mut self, value: impl Into<String>, style: ContentStyle) -> Inline {
        let content = self.text_ref(value, style, None);
        Inline::Text { content }
    }

    pub(crate) fn code(&mut self, value: impl Into<String>) -> Inline {
        let content = self.text_ref(
            value,
            ContentStyle {
                literal: true,
                ..ContentStyle::default()
            },
            None,
        );
        Inline::Code { content }
    }

    pub(crate) fn text_ref(
        &mut self,
        value: impl Into<String>,
        style: ContentStyle,
        link: Option<LinkOccurrenceKey>,
    ) -> ContentRef {
        self.builder.push_text(
            self.root,
            value.into(),
            None,
            style,
            None,
            link,
            Provenance::Unknown,
        )
    }

    pub(crate) fn hard_break(&mut self) -> Inline {
        Inline::LineBreak {
            atom: self
                .builder
                .push_hard_break(self.root, None, Provenance::Unknown),
        }
    }

    pub(crate) fn anchor(&mut self, id: impl Into<crate::NodeId>) -> Inline {
        self.anchor_with_aliases(id, Vec::new())
    }

    pub(crate) fn anchor_with_aliases(
        &mut self,
        id: impl Into<crate::NodeId>,
        fragment_aliases: Vec<crate::FragmentAlias>,
    ) -> Inline {
        let atom_boundary = u32::try_from(
            self.builder
                .content_store()
                .root(self.root)
                .expect("fixture root exists")
                .atoms
                .len(),
        )
        .expect("fixture atom count fits u32");
        let scalar_boundary = u32::try_from(
            self.builder
                .content_store()
                .root_logical_text(self.root)
                .expect("fixture root resolves")
                .chars()
                .count(),
        )
        .expect("fixture scalar count fits u32");
        let point = self.builder.push_point(
            self.root,
            PointBoundary::BetweenAtoms { atom_boundary },
            scalar_boundary,
            Provenance::Generated { trigger: None },
        );
        Inline::anchor_with_aliases(point, id, fragment_aliases)
    }

    pub(crate) fn link_text(
        &mut self,
        target: LinkTarget,
        title: Option<String>,
        value: impl Into<String>,
        literal: bool,
    ) -> Inline {
        let occurrence = self
            .builder
            .push_link(self.owner, target, title, Provenance::Unknown);
        let style = ContentStyle {
            literal,
            ..ContentStyle::default()
        };
        let content = self.text_ref(value, style, Some(occurrence));
        let child = if literal {
            Inline::Code { content }
        } else {
            Inline::Text { content }
        };
        Inline::Link {
            occurrence,
            children: vec![child],
        }
    }

    pub(crate) fn empty_link(&mut self, target: LinkTarget, title: Option<String>) -> Inline {
        let occurrence = self
            .builder
            .push_link(self.owner, target, title, Provenance::Unknown);
        Inline::Link {
            occurrence,
            children: Vec::new(),
        }
    }

    pub(crate) fn content(&self) -> crate::ContentContext<'_> {
        self.builder.content_store().content()
    }

    pub(crate) fn store(&self) -> &ContentStore {
        self.builder.content_store()
    }

    pub(crate) fn finish(self) -> ContentStore {
        self.builder.finish()
    }
}

/// Upgrade compact pre-C04 JSON fixtures into the key-backed document wire.
///
/// This is deliberately test-only: product decoders must reject the retired
/// inline-owned text and target representation instead of accepting two
/// content authorities.
pub(crate) fn document_from_legacy_json(mut value: Value) -> crate::Document {
    let mut wire = LegacyWireContent::new();
    wire.lower_value(&mut value, None, ContentStyle::default());
    let mut body = serde_json::Map::new();
    body.insert("kind".to_owned(), Value::String("flow".to_owned()));
    body.insert(
        "contentStore".to_owned(),
        serde_json::to_value(wire.builder.finish()).unwrap(),
    );
    for field in ["heading", "blocks", "sections"] {
        if let Some(value) = value.as_object_mut().unwrap().remove(field) {
            body.insert(field.to_owned(), value);
        }
    }
    value["body"] = Value::Object(body);
    serde_json::from_value(value).unwrap()
}

struct LegacyWireContent {
    builder: ContentStoreBuilder,
    owner: ContentOwnerKey,
    root: ContentRootKey,
}

impl LegacyWireContent {
    fn new() -> Self {
        let mut builder = ContentStoreBuilder::new();
        let owner = builder.push_owner(ContentOwnerKind::Document, Provenance::Unknown);
        let root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
        Self {
            builder,
            owner,
            root,
        }
    }

    fn lower_value(
        &mut self,
        value: &mut Value,
        active_link: Option<LinkOccurrenceKey>,
        style: ContentStyle,
    ) {
        match value {
            Value::Array(values) => {
                for value in values {
                    self.lower_value(value, active_link, style);
                }
            }
            Value::Object(object) => {
                let kind = object
                    .get("type")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                match kind.as_deref() {
                    Some("text" | "code") if object.contains_key("value") => {
                        let text = object
                            .remove("value")
                            .and_then(|value| value.as_str().map(str::to_owned))
                            .expect("legacy fixture inline text is a string");
                        let mut atom_style = style;
                        atom_style.literal |= kind.as_deref() == Some("code");
                        let content = self.builder.push_text(
                            self.root,
                            text,
                            None,
                            atom_style,
                            None,
                            active_link,
                            Provenance::Unknown,
                        );
                        object.insert("content".to_owned(), serde_json::to_value(content).unwrap());
                    }
                    Some("strong" | "emphasis") => {
                        let mut child_style = style;
                        child_style.strong |= kind.as_deref() == Some("strong");
                        child_style.emphasis |= kind.as_deref() == Some("emphasis");
                        if let Some(children) = object.get_mut("children") {
                            self.lower_value(children, active_link, child_style);
                        }
                    }
                    Some("link") if object.contains_key("target") => {
                        let target = serde_json::from_value::<LinkTarget>(
                            object.remove("target").expect("legacy fixture link target"),
                        )
                        .expect("legacy fixture target is typed");
                        let title = object
                            .remove("title")
                            .map(serde_json::from_value)
                            .transpose()
                            .expect("legacy fixture link title is a string")
                            .flatten();
                        let occurrence =
                            self.builder
                                .push_link(self.owner, target, title, Provenance::Unknown);
                        if let Some(children) = object.get_mut("children") {
                            self.lower_value(children, Some(occurrence), style);
                        }
                        object.insert("occurrence".to_owned(), occurrence.get().into());
                    }
                    Some("anchor") if !object.contains_key("point") => {
                        let root = self
                            .builder
                            .content_store()
                            .root(self.root)
                            .expect("fixture root exists");
                        let atom_boundary = u32::try_from(root.atoms.len()).unwrap();
                        let scalar_boundary = u32::try_from(
                            self.builder
                                .content_store()
                                .root_logical_text(self.root)
                                .unwrap()
                                .chars()
                                .count(),
                        )
                        .unwrap();
                        let point = self.builder.push_point(
                            self.root,
                            PointBoundary::BetweenAtoms { atom_boundary },
                            scalar_boundary,
                            Provenance::Unknown,
                        );
                        object.insert("point".to_owned(), point.get().into());
                    }
                    Some("line-break") if !object.contains_key("atom") => {
                        let atom = self.builder.push_hard_break(
                            self.root,
                            active_link,
                            Provenance::Unknown,
                        );
                        object.insert("atom".to_owned(), atom.get().into());
                    }
                    _ => {
                        for value in object.values_mut() {
                            self.lower_value(value, active_link, style);
                        }
                    }
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
}
