//! Lowers man and mdoc list and definition structures.

use libmandoc_rs::{Node, NodeKind, NormalizedListKind};
use mant_ir::{
    Block, DefinitionItem, Inline, ListItem, ListKind, TableAlignment as AstTableAlignment,
    TableCell as AstTableCell, TableRow,
};

use super::super::{
    LoweringContext, first_part_children,
    inline::{
        InlineBuilder, lower_inline_nodes_with_spacing, plain_text, spacing_after_node,
        spacing_after_nodes, terms_fit_inline,
    },
    layout::{
        block_indent, display_indent, horizontal_distance_columns, layout, layout_with_spacing,
    },
    part_child_groups,
    roff_escape::visible_text,
    source_span,
};
use super::{
    ends_with_line_continuation, is_inline_equation, is_inline_equation_quote_artifact,
    lower_blocks_with_spacing,
};
use crate::block::block_layout_mut;

fn is_bullet_glyph(text: &str) -> bool {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        // `o` is the ASCII bullet convention in man pages; otherwise any single
        // non-alphanumeric mark (`*`, `•`, `-`, `+`, …) is a bullet.
        (Some(glyph), None) => glyph == 'o' || !glyph.is_alphanumeric(),
        _ => false,
    }
}

pub(super) fn lower_man_definition(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    state: ManDefinitionState<'_>,
    spacing_enabled: bool,
) {
    let ManDefinitionState {
        paragraph_distance,
        output,
        definition_hanging_width,
        pending_alias,
    } = state;
    // Capture the distance before lowering the body: a `.PD` request that
    // follows this item can live inside libmandoc's block scope and updates
    // spacing for the *next* item, not the current one.
    let spacing_before = if node.macro_name.as_deref() == Some("TQ") {
        0
    } else {
        *paragraph_distance
    };
    update_man_definition_width(node, definition_hanging_width);
    let max_width = definition_hanging_width.saturating_sub(1);
    let mut item = definition_item(
        node,
        context,
        indent_columns,
        paragraph_distance,
        max_width,
        spacing_enabled,
    );
    let macro_name = node.macro_name.as_deref();
    let merge_pending = macro_name == Some("IP")
        || matches!(macro_name, Some("TP" | "TQ"))
            && (macro_name == Some("TQ") || *pending_alias || spacing_before == 0);
    *pending_alias = item.description.is_empty()
        && (macro_name == Some("TQ")
            || visible_definition_head(node)
                .last()
                .is_some_and(ends_with_line_continuation));
    if node.macro_name.as_deref() == Some("IP")
        && item.terms.is_empty()
        && append_ip_continuation(output, &mut item, indent_columns, spacing_before)
    {
        return;
    }
    if node.macro_name.as_deref() == Some("IP") && is_ip_bullet_item(&item) {
        append_ip_bullet(
            output,
            item,
            indent_columns,
            spacing_before,
            source_span(node),
        );
    } else {
        append_definition(
            output,
            item,
            indent_columns,
            spacing_before,
            source_span(node),
            max_width,
            merge_pending,
        );
    }
}

pub(super) struct ManDefinitionState<'a> {
    pub(super) paragraph_distance: &'a mut u16,
    pub(super) output: &'a mut Vec<Block>,
    pub(super) definition_hanging_width: &'a mut usize,
    pub(super) pending_alias: &'a mut bool,
}

/// Attach an unlabelled `.IP` body to the preceding labelled item.
///
/// man(7) uses a headless `.IP` to begin another indented paragraph under the
/// current tag. It is a continuation only when the immediately preceding
/// item already has both a term and a description; otherwise the anonymous
/// block remains explicit so malformed or intentionally unlabelled input is
/// never discarded.
fn append_ip_continuation(
    output: &mut [Block],
    item: &mut DefinitionItem,
    indent_columns: u16,
    paragraph_distance: u16,
) -> bool {
    if item.description.is_empty() {
        return false;
    }
    let Some(Block::DefinitionList { items, compact, .. }) = output
        .last_mut()
        .filter(|block| block_indent(block) == Some(indent_columns))
    else {
        return false;
    };
    let Some(previous) = items
        .last_mut()
        .filter(|previous| !previous.terms.is_empty() && !previous.description.is_empty())
    else {
        return false;
    };
    if let Some(layout) = item.description.first_mut().and_then(block_layout_mut) {
        layout.spacing_before_lines = layout.spacing_before_lines.max(paragraph_distance);
    }
    previous.description.append(&mut item.description);
    *compact = *compact && paragraph_distance == 0;
    true
}

pub(super) fn lower_mdoc_list(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    paragraph_distance: &mut u16,
    initial_spacing: bool,
) -> Block {
    let items = mdoc_list_items(node, initial_spacing, context.default_name);
    let is_definition = matches!(
        node.list_kind,
        Some(NormalizedListKind::Definition | NormalizedListKind::Column)
    ) || (node.list_kind.is_none()
        && items
            .iter()
            .any(|item| !first_part_children(item.node, NodeKind::Head).is_empty()));
    let list_indent = indent_columns + display_indent(node);
    if node.list_kind == Some(NormalizedListKind::Column) {
        return lower_mdoc_column_list(
            node,
            items,
            context,
            indent_columns,
            list_indent,
            paragraph_distance,
        );
    }
    if is_definition {
        let max_term_width = node
            .width
            .as_deref()
            .and_then(horizontal_distance_columns)
            .unwrap_or(6);
        let lowered_items = items
            .into_iter()
            .map(|item| {
                definition_item(
                    item.node,
                    context,
                    list_indent,
                    paragraph_distance,
                    max_term_width,
                    item.spacing_enabled,
                )
            })
            .collect();
        Block::DefinitionList {
            items: coalesce_pending_definition_terms(lowered_items, max_term_width),
            compact: node.compact,
            layout: layout(indent_columns),
            source: source_span(node),
        }
    } else {
        Block::List {
            kind: match node.list_kind {
                Some(NormalizedListKind::Ordered) => ListKind::Ordered,
                Some(NormalizedListKind::Plain) => ListKind::Plain,
                _ => ListKind::Bullet,
            },
            start: (node.list_kind == Some(NormalizedListKind::Ordered)).then_some(1),
            compact: node.compact,
            items: items
                .into_iter()
                .map(|item| ListItem {
                    blocks: lower_blocks_with_spacing(
                        first_part_children(item.node, NodeKind::Body),
                        context,
                        list_indent,
                        paragraph_distance,
                        spacing_after_nodes(
                            first_part_children(item.node, NodeKind::Head),
                            item.spacing_enabled,
                            context.default_name,
                        ),
                    ),
                })
                .collect(),
            layout: layout(indent_columns),
            source: source_span(node),
        }
    }
}

/// Attach consecutive description-less definition heads to the next item.
///
/// Both man(7) `.TQ` and mdoc(7) commonly express several equivalent input
/// forms as a run of heads followed by one shared body.  The man lowering
/// path already folds pending `.TQ` heads in [`append_definition`]; mdoc lists
/// arrive as one complete collection, so perform the same structural
/// normalization before the source-specific list representation leaves this
/// module.  A trailing run stays intact because no shared description proves
/// that the terms belong to one definition.
fn coalesce_pending_definition_terms(
    items: Vec<DefinitionItem>,
    max_term_width: usize,
) -> Vec<DefinitionItem> {
    let mut output = Vec::with_capacity(items.len());
    let mut pending = Vec::new();

    for mut item in items {
        if item.description.is_empty() {
            pending.push(item);
            continue;
        }
        if is_option_definition(&item) && pending.iter().all(is_option_definition) {
            let pending_terms = pending
                .drain(..)
                .flat_map(|pending: DefinitionItem| pending.terms);
            item.terms.splice(0..0, pending_terms);
            item.inline_term = terms_fit_inline(&item.terms, max_term_width);
        } else {
            output.append(&mut pending);
        }
        output.push(item);
    }

    output.extend(pending);
    output
}

fn is_option_definition(item: &DefinitionItem) -> bool {
    item.terms.iter().any(|term| {
        let text = plain_text(term);
        let Some(token) = text.split_whitespace().next() else {
            return false;
        };
        let name =
            token.trim_matches(|character: char| matches!(character, '[' | ']' | '(' | ')' | ','));
        name.starts_with('-') && name != "-"
    })
}

#[derive(Clone, Copy)]
struct MdocListItem<'a> {
    node: &'a Node,
    spacing_enabled: bool,
}

/// Pair each mdoc list item with the formatter spacing state active at its
/// source position.
///
/// libmandoc keeps state-only `.Sm` requests as siblings of `.It` blocks.
/// Filtering the body directly to items therefore erased precisely the state
/// needed to render compact forms such as `Odevice`, `:S/old/new/`, and
/// `@newuser name:uid`. Walking the structural stream once also lets an
/// intentionally unbalanced transition inside an item affect later items.
fn mdoc_list_items<'a>(
    node: &'a Node,
    initial_spacing: bool,
    default_name: Option<&str>,
) -> Vec<MdocListItem<'a>> {
    let mut spacing_enabled = initial_spacing;
    let mut items = Vec::new();
    for child in first_part_children(node, NodeKind::Body) {
        if child.macro_name.as_deref() == Some("It") {
            items.push(MdocListItem {
                node: child,
                spacing_enabled,
            });
        }
        spacing_enabled = spacing_after_node(child, spacing_enabled, default_name);
    }
    items
}

/// Preserve every body sibling of an mdoc `Bl -column` item as one table cell.
///
/// libmandoc represents `Ta` separators by creating several `Body` siblings
/// below the same `It` block. The usual term/body helper intentionally returns
/// only one structural part, so treating a column list as a definition list
/// silently discarded every cell after the first.
fn lower_mdoc_column_list(
    node: &Node,
    items: Vec<MdocListItem<'_>>,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    cell_indent: u16,
    paragraph_distance: &mut u16,
) -> Block {
    let rows = items
        .into_iter()
        .map(|item| {
            let body_spacing = spacing_after_nodes(
                first_part_children(item.node, NodeKind::Head),
                item.spacing_enabled,
                context.default_name,
            );
            let mut cells = part_child_groups(item.node, NodeKind::Body)
                .map(|body| AstTableCell {
                    blocks: lower_blocks_with_spacing(
                        body,
                        context,
                        cell_indent,
                        paragraph_distance,
                        body_spacing,
                    ),
                    column_span: 1,
                    row_span: 1,
                    alignment: Some(AstTableAlignment::Left),
                })
                .collect::<Vec<_>>();
            if item.node.flags.deep_link_target
                && let Some(id) = item.node.tag.as_deref()
                && let Some(Block::Paragraph { children, .. }) =
                    cells.first_mut().and_then(|cell| cell.blocks.first_mut())
            {
                children.insert(0, Inline::Anchor { id: id.into() });
            }
            TableRow { cells }
        })
        .filter(|row| !row.cells.is_empty())
        .collect();
    Block::Table {
        rows,
        layout: layout(indent_columns),
        source: source_span(node),
    }
}

fn definition_item(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    paragraph_distance: &mut u16,
    max_term_width: usize,
    spacing_enabled: bool,
) -> DefinitionItem {
    let head = visible_definition_head(node);
    let body = first_part_children(node, NodeKind::Body);
    let (displaced_equations, body) = displaced_definition_equations(head, body);
    let mut term_builder = InlineBuilder::with_spacing(spacing_enabled);
    term_builder.append(lower_inline_nodes_with_spacing(
        head,
        context.default_name,
        spacing_enabled,
    ));
    for equation in displaced_equations {
        term_builder.append(lower_inline_nodes_with_spacing(
            std::slice::from_ref(equation),
            context.default_name,
            spacing_enabled,
        ));
    }
    let mut term = term_builder.finish();
    if let Some(id) = definition_head_anchor(node, &term) {
        term.insert(0, Inline::Anchor { id: id.into() });
    }
    let terms = split_definition_terms(term);
    DefinitionItem {
        identity: None,
        inline_term: terms_fit_inline(&terms, max_term_width),
        terms,
        description: lower_blocks_with_spacing(
            body,
            context,
            indent_columns + 4,
            paragraph_distance,
            spacing_after_nodes(head, spacing_enabled, context.default_name),
        ),
        spacing_before_lines: None,
    }
}

/// Recover inline eqn arguments that libmandoc moved from a man macro head to
/// the beginning of its owning definition body.
fn displaced_definition_equations<'a>(
    head: &[Node],
    body: &'a [Node],
) -> (Vec<&'a Node>, &'a [Node]) {
    let Some(head_line) = head.iter().map(maximum_node_line).max() else {
        return (Vec::new(), body);
    };
    let mut equations = Vec::new();
    let mut consumed = 0;
    while let Some(candidate) = body
        .get(consumed)
        .filter(|candidate| candidate.line == head_line)
    {
        if is_inline_equation(candidate) {
            equations.push(candidate);
            consumed += 1;
            continue;
        }
        if consumed > 0 && is_inline_equation_quote_artifact(body, consumed) {
            consumed += 1;
            continue;
        }
        break;
    }
    if equations.is_empty() {
        (equations, body)
    } else {
        (equations, &body[consumed..])
    }
}

fn maximum_node_line(node: &Node) -> u32 {
    node.children
        .iter()
        .map(maximum_node_line)
        .fold(node.line, u32::max)
}

/// Split alternatives embedded in one extended mdoc definition head.
///
/// libmandoc retains `.Pp` inside `It Xo ... Xc` as an inline child. In that
/// position it separates equivalent term spellings rather than starting a
/// new description paragraph. The IR already models such aliases as several
/// terms on one definition item, so preserve that structure explicitly.
fn split_definition_terms(term: Vec<Inline>) -> Vec<Vec<Inline>> {
    let mut terms = Vec::new();
    let mut current = Vec::new();
    for node in term {
        if node == Inline::LineBreak {
            if !current.is_empty() {
                terms.push(std::mem::take(&mut current));
            }
        } else {
            current.push(node);
        }
    }
    if !current.is_empty() {
        terms.push(current);
    }
    terms
}

/// Preserve libmandoc's tag on a man(7) `.TP`/`.IP` head. Unlike mdoc `Fl`
/// tags, this identity lives on the structural head rather than a visible
/// inline child, so it has to be copied before lowering discards that wrapper.
fn definition_head_anchor(node: &Node, term: &[Inline]) -> Option<String> {
    let head = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Head)?;
    if !head.flags.deep_link_target {
        return None;
    }
    head.tag.as_deref().map(visible_text).or_else(|| {
        plain_text(term)
            .trim_start_matches('-')
            .split_whitespace()
            .next()
            .map(ToOwned::to_owned)
    })
}

/// Return only document content from a definition macro's mixed-purpose head.
///
/// This follows mandoc's own HTML and terminal renderers: `.IP` prints its
/// first head node and treats later arguments as layout, while `.TP`/`.TQ`
/// print only nodes beginning on the following input line. The distinction is
/// structural; inspecting strings such as `96u` would incorrectly remove a
/// numeric term while still leaking non-numeric width expressions.
fn visible_definition_head(node: &Node) -> &[Node] {
    let head = first_part_children(node, NodeKind::Head);
    match node.macro_name.as_deref() {
        Some("IP") => head.first().map_or(&[], std::slice::from_ref),
        Some("TP" | "TQ") => head
            .iter()
            .position(|child| child.flags.line_start)
            .map_or(&[], |visible_start| &head[visible_start..]),
        _ => head,
    }
}

fn append_definition(
    output: &mut Vec<Block>,
    mut item: DefinitionItem,
    indent_columns: u16,
    paragraph_distance: u16,
    source: Option<mant_ir::SourceSpan>,
    max_term_width: usize,
    merge_pending: bool,
) {
    if let Some(Block::DefinitionList { items, compact, .. }) = output
        .last_mut()
        .filter(|block| block_indent(block) == Some(indent_columns))
    {
        if merge_pending && !item.description.is_empty() {
            let first_pending = items
                .iter()
                .rposition(|previous| !previous.description.is_empty())
                .map_or(0, |index| index + 1);
            for pending in items.drain(first_pending..) {
                item.terms.splice(0..0, pending.terms);
            }
            // Source-proven `.TQ` and `\c` aliases are collected as pending,
            // description-less items. Once joined, layout must be decided
            // from the complete visible term string rather than the final
            // alias alone.
            item.inline_term = terms_fit_inline(&item.terms, max_term_width);
        }
        item.spacing_before_lines = Some(if items.is_empty() {
            0
        } else {
            paragraph_distance
        });
        *compact = *compact && paragraph_distance == 0;
        items.push(item);
    } else {
        item.spacing_before_lines = Some(0);
        let spacing_before_lines = if output.is_empty() {
            0
        } else {
            paragraph_distance
        };
        output.push(Block::DefinitionList {
            items: vec![item],
            compact: paragraph_distance == 0,
            layout: layout_with_spacing(indent_columns, spacing_before_lines),
            source,
        });
    }
}

fn update_man_definition_width(node: &Node, current_width: &mut usize) {
    let head = first_part_children(node, NodeKind::Head);
    let argument = match node.macro_name.as_deref() {
        Some("TP" | "TQ") => head
            .iter()
            .find(|child| !child.flags.line_start)
            .and_then(first_node_text),
        Some("IP") => head.get(1).and_then(first_node_text),
        _ => None,
    };
    if let Some(width) = argument.and_then(horizontal_distance_columns) {
        *current_width = width;
    }
}

fn first_node_text(node: &Node) -> Option<&str> {
    node.text
        .as_deref()
        .or_else(|| node.children.iter().find_map(first_node_text))
}

/// Append a man(7) `.IP` bullet while the source macro is still known.
///
/// Inferring this later from the serialized term text is unsafe: a legitimate
/// `.TP *` glossary entry looks identical after lowering. Keeping the decision
/// at this boundary preserves real `.IP o`/`.IP \(bu` lists without erasing
/// punctuation-only definition terms.
fn append_ip_bullet(
    output: &mut Vec<Block>,
    item: DefinitionItem,
    indent_columns: u16,
    paragraph_distance: u16,
    source: Option<mant_ir::SourceSpan>,
) {
    let list_item = ListItem {
        blocks: item.description,
    };
    if let Some(Block::List {
        kind: ListKind::Bullet,
        compact,
        items,
        ..
    }) = output
        .last_mut()
        .filter(|block| block_indent(block) == Some(indent_columns))
    {
        *compact = *compact && paragraph_distance == 0;
        items.push(list_item);
        return;
    }

    let spacing_before_lines = if output.is_empty() {
        0
    } else {
        paragraph_distance
    };
    output.push(Block::List {
        kind: ListKind::Bullet,
        start: None,
        compact: paragraph_distance == 0,
        items: vec![list_item],
        layout: layout_with_spacing(indent_columns, spacing_before_lines),
        source,
    });
}

fn is_ip_bullet_item(item: &DefinitionItem) -> bool {
    let [term] = item.terms.as_slice() else {
        return false;
    };
    is_bullet_glyph(plain_text(term).trim())
}

#[cfg(test)]
mod tests {
    use mant_ir::{Block, DefinitionItem, Inline, LayoutHint};

    fn text(value: &str) -> Vec<Inline> {
        vec![Inline::Text {
            value: value.to_owned(),
        }]
    }

    fn definition(term: &str, description: &str) -> DefinitionItem {
        DefinitionItem {
            identity: None,
            inline_term: false,
            terms: vec![text(term)],
            description: vec![Block::Paragraph {
                children: text(description),
                layout: LayoutHint::default(),
                source: None,
            }],
            spacing_before_lines: None,
        }
    }

    #[test]
    fn only_single_glyph_definition_terms_are_ip_bullets() {
        assert!(super::is_ip_bullet_item(&definition("*", "multiply")));
        assert!(super::is_ip_bullet_item(&definition("o", "item")));
        assert!(!super::is_ip_bullet_item(&definition("&&", "logical and")));
        assert!(!super::is_ip_bullet_item(&definition(
            "-a, --all",
            "show all"
        )));
    }

    #[test]
    fn short_terms_hang_inline_but_long_ones_do_not() {
        // Matches man(1): a tag that fits the default hanging indent shares the
        // first description line; wider tags take their own line.
        assert!(super::terms_fit_inline(&[text("space")], 6));
        assert!(super::terms_fit_inline(&[text("* / %")], 6));
        assert!(!super::terms_fit_inline(&[text("--listed-incremental")], 6));
        assert!(!super::terms_fit_inline(&[], 6));
    }

    #[test]
    fn extended_definition_terms_split_only_at_semantic_line_breaks() {
        let terms = super::split_definition_terms(vec![
            Inline::Text {
                value: "first".to_owned(),
            },
            Inline::LineBreak,
            Inline::Strong {
                children: text("second"),
            },
        ]);

        assert_eq!(
            terms,
            [
                text("first"),
                vec![Inline::Strong {
                    children: text("second")
                }]
            ]
        );
    }
}
