//! Element reachability adapter for the HTML5 tree builder. Report navigation
//! reads attributes, not text or sibling order, so neither is retained.
use html5ever::{
    Attribute, LocalName, Namespace, QualName,
    interface::{ElemName, ElementFlags, NodeOrText, QuirksMode, TreeSink},
    tendril::StrTendril,
};
use std::{borrow::Cow, cell::RefCell};

#[derive(Default)]
pub(super) struct Node {
    name: Option<QualName>,
    attrs: Vec<Attribute>,
    children: Vec<usize>,
    // Parent and offset in its unordered child list. Moves are O(1).
    parent: Option<(usize, usize)>,
    template: Option<usize>,
    integration_point: bool,
}

impl Node {
    pub(super) fn has_name(&self, name: &str) -> bool {
        self.name.as_ref().is_some_and(|tag| &*tag.local == name)
    }

    pub(super) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|attr| &*attr.name.local == name)
            .map(|attr| &*attr.value)
    }
}

#[derive(Debug)]
pub(super) struct Name(QualName);

impl ElemName for Name {
    fn ns(&self) -> &Namespace {
        &self.0.ns
    }

    fn local_name(&self) -> &LocalName {
        &self.0.local
    }
}

pub(super) struct ReportTree(RefCell<Vec<Node>>);

impl Default for ReportTree {
    fn default() -> Self {
        Self(RefCell::new(vec![Node::default()]))
    }
}

const ROOT: usize = 0;

impl ReportTree {
    fn create(&self, node: Node) -> usize {
        let mut nodes = self.0.borrow_mut();
        let id = nodes.len();
        nodes.push(node);
        id
    }

    fn detach(nodes: &mut [Node], id: usize) {
        if let Some((parent, offset)) = nodes[id].parent.take() {
            nodes[parent].children.swap_remove(offset);
            if let Some(&moved) = nodes[parent].children.get(offset) {
                nodes[moved].parent = Some((parent, offset));
            }
        }
    }

    pub(super) fn elements(nodes: &[Node]) -> impl Iterator<Item = &Node> {
        let mut pending = vec![ROOT];
        std::iter::from_fn(move || {
            while let Some(id) = pending.pop() {
                let node = &nodes[id];
                // Template contents deliberately are not ordinary children.
                pending.extend(&node.children);
                if node.name.is_some() {
                    return Some(node);
                }
            }
            None
        })
    }
}

impl TreeSink for ReportTree {
    type Handle = usize;
    type Output = Vec<Node>;
    type ElemName<'a> = Name;

    fn finish(self) -> Self::Output {
        self.0.into_inner()
    }

    fn parse_error(&self, _: Cow<'static, str>) {}

    fn get_document(&self) -> usize {
        ROOT
    }

    fn elem_name(&self, target: &usize) -> Name {
        Name(self.0.borrow()[*target].name.clone().expect("element name"))
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> usize {
        self.create(Node {
            name: Some(name),
            attrs,
            template: flags.template.then(|| self.create(Node::default())),
            integration_point: flags.mathml_annotation_xml_integration_point,
            ..Node::default()
        })
    }

    fn create_comment(&self, _: StrTendril) -> usize {
        self.create(Node::default())
    }

    fn create_pi(&self, _: StrTendril, _: StrTendril) -> usize {
        self.create(Node::default())
    }

    fn append(&self, parent: &usize, child: NodeOrText<usize>) {
        let NodeOrText::AppendNode(id) = child else {
            return;
        };
        let mut nodes = self.0.borrow_mut();
        Self::detach(&mut nodes, id);
        nodes[id].parent = Some((*parent, nodes[*parent].children.len()));
        nodes[*parent].children.push(id);
    }

    fn append_based_on_parent_node(
        &self,
        element: &usize,
        fallback: &usize,
        child: NodeOrText<usize>,
    ) {
        let parent = self.0.borrow()[*element].parent.map(|(id, _)| id);
        self.append(&parent.unwrap_or(*fallback), child);
    }

    fn append_doctype_to_document(&self, _: StrTendril, _: StrTendril, _: StrTendril) {}

    fn get_template_contents(&self, target: &usize) -> usize {
        self.0.borrow()[*target]
            .template
            .expect("template contents")
    }

    fn same_node(&self, x: &usize, y: &usize) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, _: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &usize, child: NodeOrText<usize>) {
        let parent = self.0.borrow()[*sibling]
            .parent
            .expect("attached sibling")
            .0;
        self.append(&parent, child);
    }

    fn add_attrs_if_missing(&self, target: &usize, attrs: Vec<Attribute>) {
        let mut nodes = self.0.borrow_mut();
        let existing = &mut nodes[*target].attrs;
        for attr in attrs {
            if !existing.iter().any(|prior| prior.name == attr.name) {
                existing.push(attr);
            }
        }
    }

    fn remove_from_parent(&self, target: &usize) {
        Self::detach(&mut self.0.borrow_mut(), *target);
    }

    fn reparent_children(&self, node: &usize, new_parent: &usize) {
        let mut nodes = self.0.borrow_mut();
        for id in std::mem::take(&mut nodes[*node].children) {
            nodes[id].parent = Some((*new_parent, nodes[*new_parent].children.len()));
            nodes[*new_parent].children.push(id);
        }
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &usize) -> bool {
        self.0.borrow()[*handle].integration_point
    }
}
