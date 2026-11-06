//! Accepted literal text retains the logical rows of its original inline owner.
//!
//! A code CR can open an exported row without advancing the IR's LF-based row
//! identity. Resolve layout after that projection, before recording any owner
//! byte ranges; syntax framing must not normalize the accepted text again.

use mant_ir::{Inline, InlineLayout, RowLayoutHint};

#[derive(Clone, Copy)]
pub(super) enum ContentRole {
    Reading,
    Code,
}

#[derive(Clone, Copy)]
struct RowOrigin {
    source_row: usize,
    first_fragment: bool,
}

pub(super) struct AcceptedRows {
    text: String,
    rows: Vec<RowOrigin>,
    authored: bool,
    previous_code_cr: bool,
}

impl AcceptedRows {
    pub(super) fn from_inlines(nodes: &[Inline], role: ContentRole) -> Self {
        let mut accepted = Self::new();
        accepted.append_inlines(nodes, role);
        accepted
    }

    pub(super) fn from_text(value: &str, role: ContentRole) -> Self {
        let mut accepted = Self::new();
        accepted.append_content(value, role);
        accepted
    }

    fn new() -> Self {
        Self {
            text: String::new(),
            rows: vec![RowOrigin {
                source_row: 0,
                first_fragment: true,
            }],
            authored: false,
            previous_code_cr: false,
        }
    }

    pub(super) fn authored(&self) -> bool {
        self.authored
    }

    pub(super) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub(super) fn ends_in_break(&self) -> bool {
        self.text.ends_with('\n')
    }

    /// A paragraph's own frame may close its provisional empty tail. This is
    /// distinct from normalizing code CR, which never retires a source row.
    pub(super) fn retire_open_tail(&mut self) {
        if self.ends_in_break() {
            self.text.pop();
            self.rows.pop();
        }
    }

    /// Empty tails retain coordinates but acquire no cells until subsequent
    /// content joins that row. A CR tail still belongs to the same source row;
    /// a real LF tail belongs to the following source row.
    pub(super) fn open_row_origin(&self, layout: &InlineLayout) -> i32 {
        let origin = self.rows.last().expect("accepted initial row");
        projected_origin(layout.row_layout(origin.source_row), *origin)
    }

    pub(super) fn into_layout_text(self, layout: &InlineLayout, shared_first: bool) -> String {
        if layout.is_empty() {
            return self.text;
        }
        let mut output = String::with_capacity(self.text.len());
        let mut hints = layout.row_hints.iter().peekable();
        let mut hint = RowLayoutHint::default();
        for (index, (row, origin)) in self.text.split('\n').zip(self.rows).enumerate() {
            if index > 0 {
                output.push('\n');
            }
            // Original logical identities only advance. Consume each sparse
            // hint once, including repeated CR fragments of the same row.
            while hints.peek().is_some_and(|next| {
                usize::try_from(next.row).is_ok_and(|row| row <= origin.source_row)
            }) {
                hint = *hints.next().expect("peeked row hint");
            }
            let hint = if usize::try_from(hint.row) == Ok(origin.source_row) {
                hint
            } else {
                RowLayoutHint::default()
            };
            if !row.is_empty() {
                if index > 0 || !shared_first {
                    output.push_str(
                        &" ".repeat(mant_ir::geometry::padding(projected_origin(hint, origin))),
                    );
                }
                output.push_str(row);
            }
        }
        output
    }

    fn append_inlines(&mut self, nodes: &[Inline], role: ContentRole) {
        for node in nodes {
            match node {
                Inline::Text { value } => self.append_content(value, role),
                Inline::Code { value } | Inline::Equation { value, .. } => {
                    self.append_content(value, ContentRole::Code);
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => self.append_inlines(children, role),
                Inline::LineBreak {} => {
                    self.authored = true;
                    self.previous_code_cr = false;
                    self.append_break(true);
                }
                Inline::Anchor { .. } => {}
            }
        }
    }

    fn append_content(&mut self, value: &str, role: ContentRole) {
        // Text("") authors an empty literal row, while an Anchor does not.
        self.authored = true;
        if matches!(role, ContentRole::Reading) && !value.is_empty() {
            self.previous_code_cr = false;
        }
        for character in value.chars() {
            match (role, character) {
                (ContentRole::Code, '\r') => {
                    self.append_break(false);
                    self.previous_code_cr = true;
                }
                (ContentRole::Code, '\n') if self.previous_code_cr => {
                    // CRLF can straddle text/styles/links/anchors. The CR
                    // already emitted its LF; the actual original LF now
                    // advances that just-opened row's logical identity.
                    let origin = self.rows.last_mut().expect("CR opened row");
                    origin.source_row = origin.source_row.saturating_add(1);
                    origin.first_fragment = true;
                    self.previous_code_cr = false;
                }
                (_, '\n') => {
                    self.append_break(true);
                    self.previous_code_cr = false;
                }
                (ContentRole::Reading, '\r') => self.text.push('\u{fffd}'),
                (_, character) => {
                    self.previous_code_cr = false;
                    if crate::text_safety::is_safe_source_character(character) {
                        self.text.push(character);
                    } else {
                        self.text
                            .extend(std::iter::repeat_n(' ', character.len_utf8()));
                    }
                }
            }
        }
    }

    fn append_break(&mut self, original: bool) {
        let previous = self.rows.last().expect("accepted initial row");
        self.rows.push(RowOrigin {
            source_row: previous.source_row.saturating_add(usize::from(original)),
            first_fragment: original,
        });
        self.text.push('\n');
    }
}

fn projected_origin(hint: RowLayoutHint, origin: RowOrigin) -> i32 {
    let origins = hint.resolve(0, 0);
    if origin.first_fragment {
        origins.first_visual_origin
    } else {
        origins.continuation_origin
    }
}
