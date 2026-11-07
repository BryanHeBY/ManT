//! Local annotation membership, independent of native glyph survival.
//!
//! An epoch is minted only at annotation entry, never per source scalar. A
//! deferred item keeps its birth epoch through native acceptance, replacement
//! and output drains. Only actually released older items carry a temporary
//! wrapper; it is removed when the last enclosing receiver no longer needs it.
use mant_ir::{Inline, LinkTarget};
use std::sync::Arc;

pub(in crate::mandoc) mod annotations;
pub(in crate::mandoc) mod markers;
#[cfg(test)]
pub(in crate::mandoc) mod metrics;

const ORIGIN: &str = "\0mant:deferred-origin:";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum Annotation {
    Link {
        target: LinkTarget,
        title: Option<String>,
    },
    Code,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Decoration {
    pub(super) annotation: Annotation,
    pub(super) previous: Option<Arc<Self>>,
}
impl Decoration {
    pub(super) fn apply_nodes(&self, nodes: Vec<Inline>) -> Vec<Inline> {
        let nodes = if let Some(previous) = &self.previous {
            previous.apply_nodes(nodes)
        } else {
            nodes
        };
        match &self.annotation {
            Annotation::Link { target, title } => vec![Inline::Link {
                target: target.clone(),
                title: title.clone(),
                children: nodes,
            }],
            Annotation::Code => vec![Inline::Code {
                value: mant_ir::inline_plain_text(&nodes),
            }],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) struct OutputMembership {
    pub(super) epoch: u64,
    pub(super) root: u64,
    pub(super) enclosing: Option<u64>,
}

pub(super) fn delayed_origin(epoch: u64, children: Vec<Inline>) -> Inline {
    let marker = format!("{ORIGIN}{epoch}");
    Inline::Link {
        target: LinkTarget::External {
            uri: marker.clone(),
        },
        title: Some(marker),
        children,
    }
}

fn origin(node: &Inline) -> Option<u64> {
    let Inline::Link {
        target: LinkTarget::External { uri },
        title: Some(title),
        ..
    } = node
    else {
        return None;
    };
    (uri == title)
        .then(|| uri.strip_prefix(ORIGIN)?.parse().ok())
        .flatten()
}
pub(in crate::mandoc) fn is_delayed_origin(node: &Inline) -> bool {
    origin(node).is_some()
}

/// Word positioning and a leading motion are layout, while a cached
/// predecessor glyph released at that boundary keeps its original birth.
pub(in crate::mandoc) fn mark_layout_origin(epoch: u64, nodes: Vec<Inline>) -> Vec<Inline> {
    let mut output = Vec::new();
    let mut layout = Vec::new();
    for node in nodes {
        if is_delayed_origin(&node) {
            if !layout.is_empty() {
                output.push(delayed_origin(epoch, std::mem::take(&mut layout)));
            }
            output.push(node);
        } else {
            layout.push(node);
        }
    }
    if !layout.is_empty() {
        output.push(delayed_origin(epoch, layout));
    }
    output
}

struct Part {
    own: bool,
    nodes: Vec<Inline>,
}
fn add(parts: &mut Vec<Part>, own: bool, nodes: Vec<Inline>) {
    if nodes.is_empty() {
        return;
    }
    if let Some(previous) = parts.last_mut().filter(|part| part.own == own) {
        previous.nodes.extend(nodes);
    } else {
        parts.push(Part { own, nodes });
    }
}

impl OutputMembership {
    pub(in crate::mandoc) fn has_enclosing(self) -> bool {
        self.enclosing.is_some()
    }

    pub(in crate::mandoc) fn end_marker(self) -> String {
        markers::end_marker(self.epoch)
    }

    /// Transform only this receiver's accepted members. Foreign runs remain
    /// in source order with their complete original style/link wrappers.
    pub(in crate::mandoc) fn map_owned(
        self,
        nodes: Vec<Inline>,
        mut map: impl FnMut(Vec<Inline>) -> Vec<Inline>,
    ) -> Vec<Inline> {
        let mut parts = Vec::new();
        self.partition(nodes, &mut parts);
        let mut output = Vec::new();
        let mut mapped = false;
        for part in parts {
            if part.own && part.nodes.iter().all(|node| matches!(node, Inline::Anchor { id, .. } if id.as_str().starts_with(super::flow::INTERNAL_FIELD_WORD) || id.as_str() == super::flow::INTERNAL_LINK_SPLIT)) {
                output.extend(part.nodes);
            } else if part.own {
                output.extend(map(part.nodes));
                mapped = true;
            } else {
                output.extend(part.nodes);
            }
        }
        // An authored empty label still carries its original destination.
        if !mapped {
            output.extend(map(Vec::new()));
        }
        output
    }

    fn partition(self, nodes: Vec<Inline>, parts: &mut Vec<Part>) {
        for node in nodes {
            if let Some(epoch) = origin(&node) {
                let Inline::Link { children, .. } = node else {
                    unreachable!()
                };
                if epoch < self.epoch {
                    let nodes = if self.enclosing.is_some_and(|parent| epoch < parent) {
                        vec![delayed_origin(epoch, children)]
                    } else {
                        children
                    };
                    add(parts, false, nodes);
                } else {
                    self.partition(children, parts);
                }
                continue;
            }
            match node {
                Inline::Strong { children } => {
                    self.partition_wrapper(children, parts, |children| Inline::Strong { children });
                }
                Inline::Emphasis { children } => {
                    self.partition_wrapper(children, parts, |children| Inline::Emphasis {
                        children,
                    });
                }
                Inline::Link {
                    target,
                    title,
                    children,
                } => self.partition_wrapper(children, parts, |children| Inline::Link {
                    target: target.clone(),
                    title: title.clone(),
                    children,
                }),
                node => add(parts, true, vec![node]),
            }
        }
    }
    fn partition_wrapper(
        self,
        children: Vec<Inline>,
        parts: &mut Vec<Part>,
        wrap: impl Fn(Vec<Inline>) -> Inline,
    ) {
        if children.is_empty() {
            add(parts, true, vec![wrap(Vec::new())]);
            return;
        }
        let mut nested = Vec::new();
        self.partition(children, &mut nested);
        for part in nested {
            if part.own {
                add(parts, true, vec![wrap(part.nodes)]);
            } else {
                add(parts, false, part.nodes);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text(value: &str) -> Inline {
        Inline::Text {
            value: value.into(),
        }
    }
    fn code(nodes: &[Inline]) -> Vec<Inline> {
        vec![Inline::Code {
            value: mant_ir::inline_plain_text(nodes),
        }]
    }

    #[test]
    fn only_actual_owned_runs_receive_new_annotation() {
        let old = Inline::Strong {
            children: vec![text("same")],
        };
        let receipt = OutputMembership {
            root: 0,
            epoch: 4,
            enclosing: None,
        };
        let result = receipt.map_owned(
            vec![
                delayed_origin(1, vec![old.clone()]),
                text("new"),
                delayed_origin(3, vec![text("same")]),
                text("last"),
            ],
            |nodes| code(&nodes),
        );
        assert_eq!(
            result,
            vec![
                old,
                Inline::Code {
                    value: "new".into()
                },
                text("same"),
                Inline::Code {
                    value: "last".into()
                }
            ]
        );
    }

    #[test]
    fn foreign_members_escape_new_wrappers_but_keep_original_link_and_style() {
        let old = Inline::Link {
            target: LinkTarget::External {
                uri: "https://same.test".into(),
            },
            title: None,
            children: vec![Inline::Emphasis {
                children: vec![text("same")],
            }],
        };
        let new = Inline::Strong {
            children: vec![delayed_origin(1, vec![old.clone()]), text("same")],
        };
        let result = OutputMembership {
            root: 0,
            epoch: 2,
            enclosing: None,
        }
        .map_owned(vec![new], |nodes| nodes);
        assert_eq!(
            result,
            vec![
                old,
                Inline::Strong {
                    children: vec![text("same")]
                }
            ]
        );
    }

    #[test]
    fn nested_receivers_retain_only_the_still_needed_origin() {
        let result = OutputMembership {
            root: 0,
            epoch: 4,
            enclosing: Some(2),
        }
        .map_owned(
            vec![
                delayed_origin(1, vec![text("old")]),
                delayed_origin(3, vec![text("parent")]),
            ],
            |nodes| nodes,
        );
        assert_eq!(
            result,
            vec![delayed_origin(1, vec![text("old")]), text("parent")]
        );
        assert_eq!(
            OutputMembership {
                root: 0,
                epoch: 2,
                enclosing: None
            }
            .map_owned(result, |nodes| code(&nodes)),
            vec![
                text("old"),
                Inline::Code {
                    value: "parent".into()
                }
            ]
        );
    }

    #[test]
    fn a_source_like_marker_is_not_a_trusted_origin_receipt() {
        let forged = Inline::Link {
            target: LinkTarget::External {
                uri: "mant:deferred-origin:1".into(),
            },
            title: Some("mant:deferred-origin:1".into()),
            children: vec![text("own")],
        };
        assert_eq!(
            OutputMembership {
                root: 0,
                epoch: 2,
                enclosing: None
            }
            .map_owned(vec![forged], |nodes| code(&nodes)),
            vec![Inline::Code {
                value: "own".into()
            }]
        );
    }
}
