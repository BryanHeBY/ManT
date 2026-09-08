//! One source-aware block layout for plain and decorated text.
//! Decorators must preserve visible content and boundary whitespace.
use super::{indent_lines, join_parts, prefix_text_item};
use mant_ir::{Block, DefinitionItem, Inline, ListItem, ListKind, Section, TableCell};
use mant_protocol::{EntryStyleMap, TextPresentation, TextRole, visit_inline_text};

pub(super) struct BlockRenderer<'a> {
    pub(super) names: Option<EntryStyleMap<'a>>,
    pub(super) decorate: &'a dyn Fn(TextPresentation, &str) -> String,
}

impl BlockRenderer<'_> {
    pub(super) fn paint(&self, role: TextRole, text: &str) -> String {
        (self.decorate)(role.into(), text)
    }

    fn inline_text(&self, children: &[Inline], role: TextRole) -> String {
        let mut text = String::new();
        let names = self
            .names
            .as_ref()
            .map_or(&[][..], |map| map.ranges(children));
        visit_inline_text(children, names, |inline, _, value| {
            text.push_str(&(self.decorate)(
                TextPresentation {
                    role,
                    inline,
                    matched: false,
                },
                value,
            ));
        });
        text
    }

    pub(super) fn render_sections(&self, sections: &[Section], depth: usize) -> String {
        sections
            .iter()
            .map(|section| self.render_section(section, depth))
            .filter(|section| !section.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub(super) fn render_section(&self, section: &Section, depth: usize) -> String {
        let heading_indent = "  ".repeat(depth);
        let mut parts = vec![format!(
            "{heading_indent}{}",
            self.paint(TextRole::Heading, &section.title)
        )];
        let blocks = self.render_blocks(&section.blocks, depth.saturating_mul(2));
        if !blocks.is_empty() {
            parts.push(blocks);
        }
        let children = self.render_sections(&section.children, depth + 1);
        if !children.is_empty() {
            parts.push(children);
        }
        join_parts(parts)
    }

    pub(super) fn render_blocks(&self, blocks: &[Block], base_indent: usize) -> String {
        self.render_block_sequence(blocks, base_indent, None)
    }

    pub(super) fn render_block_sequence(
        &self,
        blocks: &[Block],
        base_indent: usize,
        leading_gap: Option<usize>,
    ) -> String {
        // Blocks are separated by a single blank line by default. An explicit
        // vertical-space node *sets* the gap before the next block rather than
        // adding to it, so `.sp` and blank input lines are not double-counted
        // against the default paragraph separation (which previously turned one
        // requested blank line into several). Leading and trailing gaps are
        // dropped so a section never opens or closes with blank lines.
        let mut output = String::new();
        // A definition term is preceding content too. Its first body block is
        // normally tight (Some(0)); a continuation of an inline paragraph uses
        // the normal block gap (Some(1)). Explicit space can override either.
        let mut has_content = leading_gap.is_some();
        let mut default_gap = leading_gap.unwrap_or(1);
        let mut pending_blank_lines: Option<usize> = None;
        for block in blocks {
            if let Block::VerticalSpace { lines, .. } = block {
                if has_content {
                    let requested = usize::from(*lines);
                    pending_blank_lines = Some(pending_blank_lines.unwrap_or(0).max(requested));
                }
                continue;
            }
            let Some(text) = self.render_block(block, base_indent) else {
                continue;
            };
            if has_content {
                let blank_lines = pending_blank_lines.unwrap_or(default_gap);
                output.push_str(&"\n".repeat(blank_lines + 1));
            }
            output.push_str(&text);
            has_content = true;
            default_gap = 1;
            pending_blank_lines = None;
        }
        output
    }

    fn render_block(&self, block: &Block, base_indent: usize) -> Option<String> {
        let (value, layout_indent) = match block {
            Block::Paragraph {
                children, layout, ..
            }
            | Block::Preformatted {
                children, layout, ..
            } => (
                self.inline_text(children, TextRole::Body),
                usize::from(layout.indent_columns),
            ),
            Block::List {
                kind,
                items,
                layout,
                ..
            } => (
                self.render_list(*kind, items, base_indent),
                usize::from(layout.indent_columns),
            ),
            Block::DefinitionList {
                items,
                compact,
                layout,
                ..
            } => (
                self.render_definitions(items, *compact, base_indent),
                usize::from(layout.indent_columns),
            ),
            Block::Table { rows, layout, .. } => (
                super::super::table::table_rows(rows, |cell| self.cell_text(cell)).join("\n"),
                usize::from(layout.indent_columns),
            ),
            Block::Equation { value, layout, .. }
            | Block::Unsupported {
                text: value,
                layout,
                ..
            } => (
                self.paint(TextRole::Body, value),
                usize::from(layout.indent_columns),
            ),
            // Vertical space is handled as an inter-block separator in
            // `render_blocks`, never as a standalone rendered block.
            Block::VerticalSpace { .. } => return None,
            Block::ThematicBreak { .. } => ("---".to_owned(), 0),
        };
        let value = value.trim_matches('\n');
        (!value.trim().is_empty()).then(|| indent_lines(value, base_indent + layout_indent))
    }

    fn render_list(&self, kind: ListKind, items: &[ListItem], base_indent: usize) -> String {
        items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let marker = match kind {
                    ListKind::Ordered { .. } => {
                        format!("{}. ", kind.ordinal(index).expect("ordered list ordinal"))
                    }
                    ListKind::Bullet => "- ".to_owned(),
                    ListKind::Plain => String::new(),
                };
                prefix_text_item(&self.render_blocks(&item.blocks, base_indent), &marker)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn render_definitions(
        &self,
        items: &[DefinitionItem],
        compact: bool,
        base_indent: usize,
    ) -> String {
        let rendered = items
            .iter()
            .filter_map(|item| {
                let terms = item
                    .terms
                    .iter()
                    .map(|term| self.inline_text(term, TextRole::DefinitionTerm))
                    .filter(|term| !term.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join(", ");
                let body_indent = usize::from(DefinitionItem::DESCRIPTION_INDENT_COLUMNS);
                let value = if !terms.is_empty() && item.inline_description().is_some() {
                    let head = self.render_blocks(&item.description[..1], base_indent);
                    // Continue the same block stream so leading .sp in the tail
                    // remains an inter-block gap, not discarded leading space.
                    let tail =
                        self.render_block_sequence(&item.description[1..], base_indent, Some(1));
                    Some(format!(
                        "{terms} {}{}",
                        head.trim_start(),
                        indent_lines(&tail, body_indent)
                    ))
                } else {
                    let description = self.render_block_sequence(
                        &item.description,
                        base_indent,
                        (!terms.is_empty()).then_some(0),
                    );
                    match (terms.is_empty(), description.is_empty()) {
                        (false, false) => Some(format!(
                            "{terms}{}",
                            indent_lines(&description, body_indent)
                        )),
                        (false, true) => Some(terms),
                        (true, false) => Some(description),
                        (true, true) => None,
                    }
                }?;
                Some((value, item.layout.spacing_before_lines))
            })
            .collect::<Vec<_>>();

        let Some((first, rest)) = rendered.split_first() else {
            return String::new();
        };
        let mut output = first.0.clone();
        for (item, spacing_before_lines) in rest {
            let blank_lines = spacing_before_lines.unwrap_or(u16::from(!compact));
            output.push_str(&"\n".repeat(usize::from(blank_lines) + 1));
            output.push_str(item);
        }
        output
    }

    fn cell_text(&self, cell: &TableCell) -> String {
        self.render_blocks(&cell.blocks, 0).replace('\n', " ")
    }
}
