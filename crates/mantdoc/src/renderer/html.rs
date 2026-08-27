use std::collections::HashMap;

use super::{
    BoundedOutput, Document, HtmlFont, Limits, MdocListMarker, NodeKind, NodeRef, NormalizedFont,
    NormalizedListKind, RenderError, TableAlignment, TableTerminalBorder, TableTerminalFont,
    append, escape_html, html_request_font_before, render_html_equation,
    render_html_visible_text_with_font, table_terminal_cell_starts, terminal_mdoc_section_named,
    terminal_previous_sibling,
};

/// Render the native HTML device from semantic blocks instead of flattening
/// the arena preorder.  The compatibility AST intentionally represents both
/// man and mdoc as generic nodes; headings, paragraphs, and lists nevertheless
/// need their Head/Body boundaries to produce stable HTML structure.
pub(super) fn render_html_document(
    document: &Document,
    fragment: bool,
    maximum: usize,
    limits: &Limits,
) -> Result<String, RenderError> {
    let mut output = BoundedOutput::new(maximum);
    let mut state = HtmlState::default();
    if !fragment {
        append(
            &mut output,
            "<!doctype html><html><body><main class=\"mantdoc\">",
            maximum,
        )?;
    }
    if let Some(root) = document.node(document.root()) {
        for node in root.children() {
            render_html_node(node, limits, &mut state, &mut output, maximum)?;
        }
    }
    if !fragment {
        append(&mut output, "</main></body></html>", maximum)?;
    }
    output.finish_trimmed()
}

/// Document-scoped HTML state which the arena deliberately does not expose as
/// syntax.  mandoc makes repeated heading destinations unique at render time:
/// the second `DESCRIPTION`, for example, becomes `DESCRIPTION~2`.
#[derive(Default)]
struct HtmlState {
    headings: HashMap<String, usize>,
    man_targets: HashMap<String, usize>,
    definition_targets: HashMap<String, usize>,
    display_targets: HashMap<String, usize>,
}

fn render_html_node(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if node.flags().no_print || node.ancestors().any(|ancestor| ancestor.flags().no_print) {
        return Ok(());
    }
    match (node.kind(), node.macro_name()) {
        (NodeKind::Block, Some("SH" | "SS" | "Sh" | "Ss")) => {
            render_html_section(node, limits, state, output, maximum)
        }
        (NodeKind::Block, Some("PP" | "LP")) => {
            render_html_man_paragraph_block(node, limits, state, output, maximum)
        }
        (NodeKind::Block, Some("Pp")) => {
            let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
                return Ok(());
            };
            render_html_paragraph(
                body.children().collect::<Vec<_>>(),
                limits,
                None,
                output,
                maximum,
            )
        }
        (NodeKind::Block, Some("TP" | "TQ")) => {
            render_html_man_tagged_paragraph(node, limits, state, output, maximum)
        }
        (NodeKind::Block, Some("IP")) => {
            render_html_man_indented_paragraph(node, limits, state, output, maximum)
        }
        (NodeKind::Block, Some("HP")) => {
            render_html_man_hanging_paragraph(node, limits, output, maximum)
        }
        (NodeKind::Block, Some("RS")) => {
            render_html_man_indent(node, limits, state, output, maximum)
        }
        (NodeKind::Block, Some("SY")) => render_html_man_synopsis(node, limits, output, maximum),
        (NodeKind::Block, Some("Bf")) => {
            render_html_font_block(node, limits, state, output, maximum)
        }
        (NodeKind::Block, Some("Bd")) => {
            render_html_mdoc_display(node, limits, state, output, maximum)
        }
        (NodeKind::Block, Some("D1" | "Dl")) => {
            render_html_one_line_display(node, limits, output, maximum)
        }
        (NodeKind::Block, Some("Bl"))
            if node.list_kind() == Some(NormalizedListKind::Definition) =>
        {
            render_html_mdoc_tag_list(node, limits, state, output, maximum)
        }
        (NodeKind::Block, Some("Bl"))
            if node.list_kind() == Some(NormalizedListKind::Bullet)
                && html_list_direct_target_tag(node).is_some() =>
        {
            render_html_mdoc_marker_list(node, limits, output, maximum)
        }
        (NodeKind::Block, Some("Bl"))
            if node.list_kind() == Some(NormalizedListKind::Column)
                && html_list_direct_target_tag(node).is_some() =>
        {
            render_html_mdoc_column_list(node, limits, output, maximum)
        }
        // mdoc paragraph and vertical controls are structural in HTML.  Their
        // retained arguments exist for diagnostics/AST compatibility, never
        // as prose.  `br` is the one inline exception and is handled by the
        // enclosing Body so it can remain inside the current paragraph.
        (NodeKind::Element, Some("Pp" | "sp" | "br" | "PD" | "ft")) => Ok(()),
        (NodeKind::Text | NodeKind::Equation, _) => {
            render_html_flat_node(node, limits, output, maximum)
        }
        (NodeKind::Table, _) if !node.table_cells().is_empty() => {
            render_html_table(node, limits, output, maximum)
        }
        _ => {
            for child in node.children() {
                render_html_node(child, limits, state, output, maximum)?;
            }
            Ok(())
        }
    }
}

/// Render a contiguous tbl range as one HTML table when private tbl layout
/// metadata is present.  Public `Table` nodes deliberately remain one row at
/// a time for owned-AST compatibility; HTML, like the terminal device, needs
/// their shared layout to recover borders, rule rows, column fonts, and
/// alignment without changing that public contract.
fn render_html_table(
    node: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(layout) = node.table_terminal() else {
        return render_html_plain_table(node, output, maximum);
    };
    if terminal_previous_sibling(node)
        .is_some_and(|previous| previous.kind() == NodeKind::Table && !layout.starts_table)
    {
        return Ok(());
    }
    let rows = html_table_range(node);
    let styled = rows
        .iter()
        .filter_map(|row| row.table_terminal())
        .any(|layout| {
            layout.outer_border != TableTerminalBorder::None
                || layout.all_box
                || layout.horizontal_rule != TableTerminalBorder::None
                || layout
                    .cells
                    .iter()
                    .any(|cell| cell.font != TableTerminalFont::Roman)
        });
    if !styled {
        return render_html_plain_table(node, output, maximum);
    }

    let outer = rows
        .iter()
        .filter_map(|row| row.table_terminal().map(|layout| layout.outer_border))
        .find(|border| *border != TableTerminalBorder::None)
        .unwrap_or(TableTerminalBorder::None);
    let all_box = rows
        .iter()
        .filter_map(|row| row.table_terminal())
        .any(|layout| layout.all_box);
    let mut data_rows: Vec<(NodeRef<'_>, Option<TableTerminalBorder>)> = Vec::new();
    for row in rows {
        let layout = row.table_terminal().cloned().unwrap_or_default();
        let rule = (layout.horizontal_rule != TableTerminalBorder::None)
            .then_some(layout.horizontal_rule)
            .or_else(|| {
                (row.table_cells().is_empty()).then(|| {
                    layout
                        .cells
                        .iter()
                        .map(|cell| cell.horizontal_rule)
                        .find(|rule| *rule != TableTerminalBorder::None)
                        .unwrap_or(TableTerminalBorder::None)
                })
            })
            .filter(|rule| *rule != TableTerminalBorder::None);
        if let Some(rule) = rule {
            if let Some((_, divider)) = data_rows.last_mut() {
                *divider = Some(rule);
            }
            continue;
        }
        if !row.table_cells().is_empty() {
            data_rows.push((row, None));
        }
    }
    if data_rows.is_empty() {
        return Ok(());
    }

    append(output, "<table class=\"tbl\"", maximum)?;
    if outer != TableTerminalBorder::None || all_box {
        append(
            output,
            &format!(
                " style=\"border-style: {};\"",
                html_table_border_style(if outer == TableTerminalBorder::None {
                    TableTerminalBorder::Single
                } else {
                    outer
                })
            ),
            maximum,
        )?;
    }
    append(output, ">\n", maximum)?;
    for (row_index, (row, divider)) in data_rows.iter().enumerate() {
        append(output, "  <tr", maximum)?;
        if let Some(divider) = divider.or_else(|| {
            (all_box && row_index + 1 < data_rows.len()).then_some(TableTerminalBorder::Single)
        }) {
            append(
                output,
                &format!(
                    " style=\"border-bottom-style: {};\"",
                    html_table_border_style(divider)
                ),
                maximum,
            )?;
        }
        append(output, ">\n", maximum)?;
        let layout = row.table_terminal();
        let column_count = layout.map_or_else(
            || row.table_cells().len(),
            |layout| layout.cells.len().max(row.table_cells().len()),
        );
        let starts = table_terminal_cell_starts(row, column_count);
        for (index, cell) in row.table_cells().iter().enumerate() {
            append(output, "    <td", maximum)?;
            if cell.column_span > 1 {
                append(
                    output,
                    &format!(" colspan=\"{}\"", cell.column_span),
                    maximum,
                )?;
            }
            if cell.row_span > 1 {
                append(output, &format!(" rowspan=\"{}\"", cell.row_span), maximum)?;
            }
            let alignment = match cell.alignment {
                TableAlignment::Left => None,
                TableAlignment::Center => Some("center"),
                TableAlignment::Right => Some("right"),
            };
            if let Some(alignment) = alignment {
                append(
                    output,
                    &format!(" style=\"text-align: {alignment};\""),
                    maximum,
                )?;
            }
            append(output, ">", maximum)?;
            if let Some(text) = &cell.text {
                let font = starts
                    .get(index)
                    .and_then(|column| layout.and_then(|layout| layout.cells.get(*column)))
                    .map_or(TableTerminalFont::Roman, |cell| cell.font);
                append(
                    output,
                    &render_html_table_cell_text(text, font, limits),
                    maximum,
                )?;
            }
            append(output, "</td>\n", maximum)?;
        }
        append(output, "  </tr>\n", maximum)?;
    }
    append(output, "</table>\n", maximum)
}

fn render_html_plain_table(
    node: NodeRef<'_>,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    append(output, "<table class=\"Tbl\"><tr>", maximum)?;
    for cell in node.table_cells() {
        append(output, "<td>", maximum)?;
        if let Some(text) = &cell.text {
            append(output, &escape_html(text), maximum)?;
        }
        append(output, "</td>", maximum)?;
    }
    append(output, "</tr></table>\n", maximum)
}

fn html_table_range(node: NodeRef<'_>) -> Vec<NodeRef<'_>> {
    let Some(parent) = node.parent() else {
        return vec![node];
    };
    parent
        .children()
        .skip_while(|sibling| sibling.id() != node.id())
        .enumerate()
        .take_while(|(index, sibling)| {
            sibling.kind() == NodeKind::Table
                && (*index == 0
                    || !sibling
                        .table_terminal()
                        .is_some_and(|layout| layout.starts_table))
        })
        .map(|(_, row)| row)
        .collect()
}

fn html_table_border_style(border: TableTerminalBorder) -> &'static str {
    match border {
        TableTerminalBorder::Double => "double",
        TableTerminalBorder::None | TableTerminalBorder::Single => "solid",
    }
}

fn render_html_table_cell_text(text: &str, font: TableTerminalFont, limits: &Limits) -> String {
    let font = match font {
        TableTerminalFont::Roman => HtmlFont::Roman,
        TableTerminalFont::Bold => HtmlFont::Bold,
        TableTerminalFont::Italic => HtmlFont::Italic,
    };
    render_html_visible_text_with_font(text, limits, font)
}

/// Render an mdoc display as a structural region.  A `Bd` Body may switch
/// from no-fill back to filled text, nest displays or lists, and carry a
/// `.Tg` destination.  Flattening it through the surrounding section loses
/// all four boundaries, so keep its source-order flow local to the display.
fn render_html_mdoc_display(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let target = html_node_target_tag(node).map(|tag| html_unique_display_target(tag, state));
    let class = match (
        node.literal_display(),
        node.offset().is_some(),
        target.is_some(),
    ) {
        (true, _, _) => "Bd Pp Li",
        (false, true, true) => "Bd Pp\n  Bd-indent",
        (false, true, false) => "Bd Pp Bd-indent",
        (false, false, _) => "Bd Pp",
    };
    append(output, &format!("<div class=\"{class}\""), maximum)?;
    if let Some(target) = &target {
        append(output, &format!(" id=\"{}\"", escape_html(target)), maximum)?;
    }
    append(output, ">", maximum)?;
    if node.literal_display() {
        render_html_mdoc_literal_display_body(body, limits, target, output, maximum)?;
    } else {
        render_html_mdoc_display_body(body, limits, state, target, output, maximum)?;
    }
    append(output, "</div>\n", maximum)
}

/// Literal displays stay in one `pre` element even when an mdoc paragraph
/// marker occurs inside them.  Such a marker contributes an empty HTML target
/// followed by the linked literal phrase; it never opens an HTML paragraph.
fn render_html_mdoc_literal_display_body(
    body: NodeRef<'_>,
    limits: &Limits,
    mut target: Option<String>,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let mut content = String::new();
    let mut previous: Option<NodeRef<'_>> = None;
    for child in body.children() {
        if child.macro_name() == Some("Pp") {
            if let Some(tag) = child.tag().filter(|tag| !tag.is_empty()) {
                if !content.is_empty() && !content.ends_with('\n') {
                    content.push('\n');
                }
                content.push_str("<mark id=\"");
                content.push_str(&escape_html(tag));
                content.push_str("\"></mark>\n");
                target = Some(tag.to_owned());
                previous = None;
            }
            continue;
        }
        let fragment = render_html_display_fragment(child, limits, &mut target);
        if fragment.is_empty() {
            continue;
        }
        if let Some(previous) = previous {
            if child.flags().line_start {
                content.push('\n');
            } else if !previous.flags().delimiter_open && !child.flags().delimiter_close {
                content.push(' ');
            }
        }
        content.push_str(&fragment);
        previous = Some(child);
    }
    if content.is_empty() {
        return Ok(());
    }
    append(output, "\n<pre>", maximum)?;
    append(output, &content, maximum)?;
    append(output, "</pre>\n", maximum)
}

/// Preserve a filled or `-unfilled` display's local flow.  Paragraph markers
/// select an HTML paragraph only when the display is filled; direct phrases
/// and following nested blocks remain raw display flow.
fn render_html_mdoc_display_body(
    body: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    mut target: Option<String>,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let mut inline: Vec<NodeRef<'_>> = Vec::new();
    let mut paragraph_tag = None;
    let mut first_flow = true;
    for child in body.children() {
        if child.macro_name() != Some("Pp") && html_is_mdoc_display_inline(child) {
            if inline
                .last()
                .is_some_and(|previous| previous.flags().no_fill != child.flags().no_fill)
            {
                render_html_mdoc_display_flow(
                    std::mem::take(&mut inline),
                    limits,
                    paragraph_tag.take(),
                    &mut target,
                    first_flow,
                    true,
                    output,
                    maximum,
                )?;
                first_flow = false;
            }
            inline.push(child);
            continue;
        }
        if child.macro_name() == Some("Pp") {
            render_html_mdoc_display_flow(
                std::mem::take(&mut inline),
                limits,
                paragraph_tag.take(),
                &mut target,
                first_flow,
                true,
                output,
                maximum,
            )?;
            first_flow = false;
            paragraph_tag = child.tag().filter(|tag| !tag.is_empty()).map(str::to_owned);
            continue;
        }
        render_html_mdoc_display_flow(
            std::mem::take(&mut inline),
            limits,
            paragraph_tag.take(),
            &mut target,
            first_flow,
            true,
            output,
            maximum,
        )?;
        first_flow = false;
        render_html_node(child, limits, state, output, maximum)?;
    }
    render_html_mdoc_display_flow(
        inline,
        limits,
        paragraph_tag,
        &mut target,
        first_flow,
        false,
        output,
        maximum,
    )
}

#[allow(clippy::too_many_arguments)] // Flow state mirrors mdoc's distinct device boundaries.
fn render_html_mdoc_display_flow(
    nodes: Vec<NodeRef<'_>>,
    limits: &Limits,
    paragraph_tag: Option<String>,
    target: &mut Option<String>,
    first_flow: bool,
    terminate_line: bool,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if nodes.is_empty() {
        return Ok(());
    }
    if let Some(tag) = paragraph_tag {
        let content = render_html_display_inline_nodes(nodes, limits, target, "    ", false);
        let tag = escape_html(&tag);
        append(output, &format!("<p class=\"Pp\" id=\"{tag}\">"), maximum)?;
        append(output, &content, maximum)?;
        return append(output, "</p>\n", maximum);
    }
    if nodes.iter().any(|node| node.flags().no_fill) {
        if first_flow {
            append(output, "\n", maximum)?;
        }
        let content = render_html_display_inline_nodes(nodes, limits, target, "", true);
        append(output, "<pre>", maximum)?;
        append(output, &content, maximum)?;
        return append(output, "</pre>\n", maximum);
    }
    let content = render_html_display_inline_nodes(nodes, limits, target, "  ", false);
    append(output, &content, maximum)?;
    if terminate_line {
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Render one display phrase with mandoc's source-line continuation geometry.
/// The normal inline renderer deliberately collapses that geometry for prose;
/// `Bd` keeps it for raw display flow and makes its leading `.Tg` link local.
fn render_html_display_inline_nodes(
    nodes: Vec<NodeRef<'_>>,
    limits: &Limits,
    target: &mut Option<String>,
    continuation: &str,
    preserve_text_lines: bool,
) -> String {
    let mut output = String::new();
    let mut previous: Option<NodeRef<'_>> = None;
    let mut previous_was_target = false;
    for node in nodes {
        let wraps_target = target.is_some();
        let mut fragment = render_html_display_fragment(node, limits, target);
        if fragment.is_empty() {
            continue;
        }
        if let Some(previous) = previous {
            if (previous_was_target || previous.flags().permalink) && node.flags().line_start {
                if continuation == "    "
                    && let Some(split) = fragment.find(' ')
                {
                    output.push(' ');
                    output.push_str(&fragment[..split]);
                    output.push('\n');
                    output.push_str(continuation);
                    fragment.replace_range(..=split, "");
                } else {
                    output.push('\n');
                    output.push_str(continuation);
                }
            } else if (preserve_text_lines || node.kind() != NodeKind::Text)
                && node.flags().line_start
            {
                output.push('\n');
                output.push_str(continuation);
            } else if !previous.flags().delimiter_open && !node.flags().delimiter_close {
                output.push(' ');
            }
        }
        output.push_str(&fragment);
        previous = Some(node);
        previous_was_target = wraps_target;
    }
    output
}

fn html_is_mdoc_display_inline(node: NodeRef<'_>) -> bool {
    matches!(node.macro_name(), Some("Pq"))
        || matches!(
            node.kind(),
            NodeKind::Text | NodeKind::Equation | NodeKind::Element
        ) && !matches!(node.macro_name(), Some("sp"))
}

fn render_html_display_fragment(
    node: NodeRef<'_>,
    limits: &Limits,
    target: &mut Option<String>,
) -> String {
    let mut content = render_html_inline_nodes(vec![node], limits);
    if content.is_empty() {
        return content;
    }
    if let Some(tag) = target.take() {
        if content.contains("class=\"permalink\"") {
            content = html_retarget_permalink(content, &tag);
        } else {
            let tag = escape_html(&tag);
            content = format!("<a class=\"permalink\" href=\"#{tag}\">{content}</a>");
        }
    }
    if node
        .parent()
        .is_some_and(|parent| parent.flags().deep_link_target)
        && !node.flags().permalink
    {
        content = content.replace(" (", "\n  (");
    }
    content
}

/// Render an mdoc font block without leaking its configuration Head into the
/// DOM.  The normalized font belongs to the whole Body, including explicit
/// paragraphs nested below it, so the wrapper must outlive those child blocks.
fn render_html_font_block(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let class = match node.font() {
        Some(NormalizedFont::Emphasis) => "Bf Em",
        Some(NormalizedFont::Literal) => "Bf Li",
        Some(NormalizedFont::Symbolic) => "Bf Sy",
        None => "Bf",
    };
    append(output, &format!("<div class=\"{class}\">"), maximum)?;

    let mut inline: Vec<NodeRef<'_>> = Vec::new();
    let mut next_inline_is_paragraph = false;
    for child in body.children() {
        if matches!(child.kind(), NodeKind::Text | NodeKind::Equation) {
            inline.push(child);
            continue;
        }
        if !inline.is_empty() {
            let inline = std::mem::take(&mut inline);
            if next_inline_is_paragraph {
                render_html_paragraph(inline, limits, None, output, maximum)?;
            } else {
                append(output, &render_html_inline_nodes(inline, limits), maximum)?;
                append(output, "\n", maximum)?;
            }
            next_inline_is_paragraph = false;
        }
        if matches!(child.macro_name(), Some("PP" | "LP" | "Pp")) {
            if let Some(paragraph) = child.children().find(|node| node.kind() == NodeKind::Body) {
                render_html_paragraph(
                    paragraph.children().collect::<Vec<_>>(),
                    limits,
                    None,
                    output,
                    maximum,
                )?;
            } else {
                next_inline_is_paragraph = true;
            }
        } else {
            render_html_node(child, limits, state, output, maximum)?;
        }
    }
    if !inline.is_empty() {
        if next_inline_is_paragraph {
            render_html_paragraph(inline, limits, None, output, maximum)?;
        } else {
            append(output, &render_html_inline_nodes(inline, limits), maximum)?;
            append(output, "\n", maximum)?;
        }
    }
    append(output, "</div>\n", maximum)
}

/// Render mdoc's `D1` and `Dl` as their one-line display DOM forms.  The
/// parser retains the first doubled argument separator on the following
/// phrase node, which is exactly the point where the HTML device breaks and
/// indents the display continuation.
fn render_html_one_line_display(
    node: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let children = body.children().collect::<Vec<_>>();
    let mut content = String::new();
    let literal = node.macro_name() == Some("Dl");
    for child in children {
        let mut fragment = render_html_inline_nodes(vec![child], limits);
        if !literal
            && !child.flags().permalink
            && child.separator_width() > 1
            && let Some(index) = fragment.find(' ')
        {
            fragment.replace_range(index..=index, "\n  ");
        }
        if !fragment.is_empty() {
            if !content.is_empty() {
                if literal {
                    content.push_str("\n  ");
                } else {
                    content.push(' ');
                }
            }
            content.push_str(&fragment);
        }
    }
    let class = if content.is_empty() {
        "Bd Bd-indent"
    } else {
        "Bd\n  Bd-indent"
    };
    append(output, &format!("<div class=\"{class}\""), maximum)?;
    if let Some(tag) = body.tag().filter(|tag| !tag.is_empty()) {
        append(output, &format!(" id=\"{}\"", escape_html(tag)), maximum)?;
    }
    append(output, ">", maximum)?;
    if literal {
        append(output, "<code class=\"Li\">", maximum)?;
        append(output, &content, maximum)?;
        append(output, "</code>", maximum)?;
    } else {
        append(output, &content, maximum)?;
    }
    append(output, "</div>\n", maximum)
}

/// Render a man paragraph's Body in source-order so `.nf` and `.fi` retain
/// their HTML preformatted boundaries.  The public AST stores both controls
/// among the Body children rather than turning them into independent blocks.
fn render_html_man_paragraph_block(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let mut inline: Vec<NodeRef<'_>> = Vec::new();
    let mut raw_after_literal = false;
    for child in body.children() {
        if matches!(child.kind(), NodeKind::Text | NodeKind::Equation)
            || child.macro_name() == Some("br")
            || child.macro_name() == Some("sp") && child.flags().no_fill
        {
            if inline
                .last()
                .is_some_and(|previous| previous.flags().no_fill != child.flags().no_fill)
            {
                let was_literal = inline.iter().any(|node| node.flags().no_fill);
                render_html_inline_flow(
                    std::mem::take(&mut inline),
                    limits,
                    None,
                    raw_after_literal,
                    output,
                    maximum,
                )?;
                raw_after_literal = was_literal;
            }
            inline.push(child);
            continue;
        }
        if matches!(child.macro_name(), Some("nf" | "fi")) {
            let was_literal = inline.iter().any(|node| node.flags().no_fill);
            render_html_inline_flow(
                std::mem::take(&mut inline),
                limits,
                None,
                raw_after_literal,
                output,
                maximum,
            )?;
            raw_after_literal = raw_after_literal || was_literal;
            continue;
        }
        render_html_inline_flow(
            std::mem::take(&mut inline),
            limits,
            None,
            raw_after_literal,
            output,
            maximum,
        )?;
        raw_after_literal = false;
        render_html_node(child, limits, state, output, maximum)?;
    }
    render_html_inline_flow(inline, limits, None, raw_after_literal, output, maximum)
}

/// Render man(7)'s tagged paragraphs through their Head/Body ownership.  A
/// `TP` Head retains its same-line width request in the compatible tree, so
/// only its following physical-line terms are visible in the HTML `dt`.
fn render_html_man_tagged_paragraph(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(fields) = html_man_tagged_paragraph_group(node) else {
        return Ok(());
    };
    append(output, "<dl class=\"Bl-tag\">\n", maximum)?;
    for field in fields {
        render_html_man_tagged_item(field, limits, state, output, maximum)?;
    }
    append(output, "</dl>\n", maximum)
}

fn html_man_tagged_paragraph_group(node: NodeRef<'_>) -> Option<Vec<NodeRef<'_>>> {
    let parent = node.parent()?;
    if node
        .previous_sibling()
        .is_some_and(|sibling| matches!(sibling.macro_name(), Some("TP" | "TQ")))
    {
        return None;
    }
    Some(
        parent
            .children()
            .skip_while(|sibling| sibling.id() != node.id())
            .take_while(|sibling| matches!(sibling.macro_name(), Some("TP" | "TQ")))
            .collect(),
    )
}

fn render_html_man_tagged_item(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(head) = node.children().find(|child| child.kind() == NodeKind::Head) else {
        return Ok(());
    };
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let term_nodes = head
        .children()
        .filter(|child| child.flags().line_start)
        .collect::<Vec<_>>();
    let mut term = render_html_inline_nodes(term_nodes, limits);
    let tag = html_unique_man_target(head, state);
    if let Some(tag) = &tag
        && !term.contains("class=\"permalink\"")
    {
        let escaped = escape_html(tag);
        term = format!("<a class=\"permalink\" href=\"#{escaped}\">{term}</a>");
    }
    append(output, "  <dt", maximum)?;
    if let Some(tag) = tag {
        append(output, &format!(" id=\"{}\"", escape_html(&tag)), maximum)?;
    }
    append(output, ">", maximum)?;
    append(output, &term, maximum)?;
    append(output, "</dt>\n", maximum)?;
    render_html_man_definition_body(body, limits, output, maximum)
}

/// A man field Body can transition between regular and no-fill text.  Each
/// transition is a distinct HTML preformatted boundary; rendering the entire
/// `dd` as one `pre` loses later ordinary phrase flow, and doing the opposite
/// loses the literal source lines.
fn render_html_man_definition_body(
    body: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let mut segments: Vec<(bool, Vec<NodeRef<'_>>)> = Vec::new();
    for child in body.children() {
        let no_fill = child.flags().no_fill;
        if segments
            .last()
            .is_some_and(|(previous, _)| *previous == no_fill)
        {
            segments.last_mut().expect("checked segment").1.push(child);
        } else {
            segments.push((no_fill, vec![child]));
        }
    }
    if segments.is_empty() {
        return append(output, "  <dd></dd>\n", maximum);
    }
    append(output, "  <dd>", maximum)?;
    for (index, (no_fill, nodes)) in segments.iter().enumerate() {
        if *no_fill {
            append(output, "\n    <pre>", maximum)?;
            append(
                output,
                &render_html_inline_nodes(nodes.clone(), limits),
                maximum,
            )?;
            append(output, "</pre>", maximum)?;
        } else {
            if index > 0 {
                append(output, "\n    ", maximum)?;
            }
            append(
                output,
                &render_html_inline_nodes(nodes.clone(), limits),
                maximum,
            )?;
        }
    }
    if segments.last().is_some_and(|(no_fill, _)| *no_fill) {
        append(output, "\n  ", maximum)?;
    }
    append(output, "</dd>\n", maximum)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum HtmlManIpKind {
    Tag,
    AsteriskBullet,
    DotBullet,
    Dash,
}

/// Render adjacent man `IP` fields in their shared DOM container.  mandoc
/// starts a new list when the authored marker changes, even when two markers
/// share the bullet class, so the marker spelling remains part of this small
/// renderer-only grouping key.
fn render_html_man_indented_paragraph(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some((kind, fields)) = html_man_ip_group(node, limits) else {
        return Ok(());
    };
    match kind {
        HtmlManIpKind::Tag => {
            append(output, "<dl class=\"Bl-tag\">\n", maximum)?;
            for field in fields {
                render_html_man_ip_tag_item(field, limits, state, output, maximum)?;
            }
            append(output, "</dl>\n", maximum)
        }
        HtmlManIpKind::AsteriskBullet | HtmlManIpKind::DotBullet | HtmlManIpKind::Dash => {
            let class = if kind == HtmlManIpKind::Dash {
                "Bl-dash"
            } else {
                "Bl-bullet"
            };
            append(output, &format!("<ul class=\"{class}\">\n"), maximum)?;
            for field in fields {
                let body = field
                    .children()
                    .find(|child| child.kind() == NodeKind::Body);
                let content = body.map_or_else(String::new, |body| {
                    render_html_inline_nodes(body.children().collect::<Vec<_>>(), limits)
                });
                append(output, "  <li>", maximum)?;
                append(output, &content, maximum)?;
                append(output, "</li>\n", maximum)?;
            }
            append(output, "</ul>\n", maximum)
        }
    }
}

fn html_man_ip_group<'document>(
    node: NodeRef<'document>,
    limits: &Limits,
) -> Option<(HtmlManIpKind, Vec<NodeRef<'document>>)> {
    let kind = html_man_ip_kind(node, limits)?;
    let parent = node.parent()?;
    if node.previous_sibling().is_some_and(|sibling| {
        sibling.macro_name() == Some("IP") && html_man_ip_kind(sibling, limits) == Some(kind)
    }) {
        return None;
    }
    Some((
        kind,
        parent
            .children()
            .skip_while(|sibling| sibling.id() != node.id())
            .take_while(|sibling| {
                sibling.macro_name() == Some("IP")
                    && html_man_ip_kind(*sibling, limits) == Some(kind)
            })
            .collect(),
    ))
}

fn html_man_ip_kind(node: NodeRef<'_>, limits: &Limits) -> Option<HtmlManIpKind> {
    let head = node
        .children()
        .find(|child| child.kind() == NodeKind::Head)?;
    let Some(marker) = head.children().next() else {
        return Some(HtmlManIpKind::Tag);
    };
    match render_html_inline_nodes(vec![marker], limits).as_str() {
        "*" => Some(HtmlManIpKind::AsteriskBullet),
        "&#x2022;" => Some(HtmlManIpKind::DotBullet),
        "-" => Some(HtmlManIpKind::Dash),
        _ => Some(HtmlManIpKind::Tag),
    }
}

fn render_html_man_ip_tag_item(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(head) = node.children().find(|child| child.kind() == NodeKind::Head) else {
        return Ok(());
    };
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let term = head.children().next().map_or_else(String::new, |marker| {
        render_html_inline_nodes(vec![marker], limits)
    });
    let tag = html_unique_man_target(head, state);
    let term = if let Some(tag) = &tag {
        if term.contains("class=\"permalink\"") {
            term
        } else {
            let escaped = escape_html(tag);
            format!("<a class=\"permalink\" href=\"#{escaped}\">{term}</a>")
        }
    } else {
        term
    };
    append(output, "  <dt", maximum)?;
    if let Some(tag) = tag {
        append(output, &format!(" id=\"{}\"", escape_html(&tag)), maximum)?;
    }
    append(output, ">", maximum)?;
    append(output, &term, maximum)?;
    append(output, "</dt>\n", maximum)?;
    render_html_man_definition_body(body, limits, output, maximum)
}

/// Render man(7)'s `HP` as an owned paragraph rather than leaking its width
/// Head.  A no-fill Body is the device's literal block and therefore has no
/// Pp wrapper.
fn render_html_man_hanging_paragraph(
    node: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let body_nodes = body.children().collect::<Vec<_>>();
    if body_nodes.iter().any(|child| child.flags().no_fill) {
        return render_html_preformatted(body_nodes, limits, output, maximum);
    }
    let content = render_html_inline_nodes(body_nodes, limits);
    if content.is_empty() {
        return Ok(());
    }
    append(output, "<p class=\"Pp HP\">", maximum)?;
    append(output, &content, maximum)?;
    append(output, "</p>\n", maximum)
}

/// Render a man `RS` Body as a structural indented region.  Its initial raw
/// phrase is not a paragraph, while nested PP/LP blocks retain their Pp DOM
/// ownership inside the indent.
fn render_html_man_indent(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    append(output, "<div class=\"Bd-indent\">", maximum)?;
    let mut inline = Vec::new();
    for child in body.children() {
        if matches!(child.kind(), NodeKind::Text | NodeKind::Equation)
            || child.macro_name() == Some("br")
        {
            inline.push(child);
            continue;
        }
        render_html_man_indent_inline(std::mem::take(&mut inline), limits, output, maximum)?;
        if output.ends_with("<div class=\"Bd-indent\">") {
            append(output, "\n", maximum)?;
        }
        render_html_node(child, limits, state, output, maximum)?;
    }
    render_html_man_indent_inline(inline, limits, output, maximum)?;
    append(output, "</div>\n", maximum)
}

fn render_html_man_indent_inline(
    nodes: Vec<NodeRef<'_>>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if nodes.is_empty() {
        return Ok(());
    }
    if nodes.iter().any(|node| node.flags().no_fill) {
        append(output, "\n", maximum)?;
        return render_html_preformatted(nodes, limits, output, maximum);
    }
    let content = render_html_inline_nodes(nodes, limits);
    if content.is_empty() {
        return Ok(());
    }
    append(output, &content, maximum)?;
    append(output, "\n", maximum)
}

/// Render a man synopsis as the two-column semantic device table.  A no-fill
/// argument lives in an inner preformatted field, preserving `SY`'s distinct
/// continuation geometry.
fn render_html_man_synopsis(
    node: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(head) = node.children().find(|child| child.kind() == NodeKind::Head) else {
        return Ok(());
    };
    let body = node.children().find(|child| child.kind() == NodeKind::Body);
    let command = render_html_inline_nodes(head.children().collect::<Vec<_>>(), limits);
    append(
        output,
        "<table class=\"Nm\">\n  <tr>\n    <td><code class=\"Nm\">",
        maximum,
    )?;
    append(output, &command, maximum)?;
    append(output, "</code></td>\n", maximum)?;
    if let Some(body) = body {
        let body_nodes = body.children().collect::<Vec<_>>();
        if body_nodes.iter().any(|child| child.flags().no_fill) {
            append(output, "    <td>\n    ", maximum)?;
            render_html_preformatted(body_nodes, limits, output, maximum)?;
            append(output, "    </td>\n", maximum)?;
        } else {
            append(output, "    <td>", maximum)?;
            append(
                output,
                &render_html_inline_nodes(body_nodes, limits),
                maximum,
            )?;
            append(output, "</td>\n", maximum)?;
        }
    }
    append(output, "  </tr>\n</table>\n", maximum)
}

fn render_html_mdoc_marker_list(
    node: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let class = match node.list_marker() {
        Some(MdocListMarker::Dash) => "Bl-dash",
        Some(MdocListMarker::Hyphen) => "Bl-hyphen",
        Some(MdocListMarker::Enum) => "Bl-enum",
        _ => "Bl-bullet",
    };
    append(output, &format!("<ul class=\"{class}\""), maximum)?;
    if let Some(tag) = html_list_direct_target_tag(node) {
        append(output, &format!(" id=\"{}\"", escape_html(&tag)), maximum)?;
    }
    append(output, ">\n", maximum)?;
    for item in body
        .children()
        .filter(|child| child.kind() == NodeKind::Block && child.macro_name() == Some("It"))
    {
        let item_body = item.children().find(|child| child.kind() == NodeKind::Body);
        let mut content = item_body.map_or_else(String::new, |body| {
            render_html_inline_nodes(body.children().collect::<Vec<_>>(), limits)
        });
        let tag = html_node_target_tag(item);
        if let Some(tag) = &tag
            && !content.contains("class=\"permalink\"")
        {
            let escaped = escape_html(tag);
            content = format!("<a class=\"permalink\" href=\"#{escaped}\">{content}</a>");
        }
        append(output, "  <li", maximum)?;
        if let Some(tag) = tag {
            append(output, &format!(" id=\"{}\"", escape_html(&tag)), maximum)?;
        }
        append(output, ">", maximum)?;
        append(output, &content, maximum)?;
        append(output, "</li>\n", maximum)?;
    }
    append(output, "</ul>\n", maximum)
}

fn render_html_mdoc_column_list(
    node: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    append(output, "<table class=\"Bl-column\"", maximum)?;
    if let Some(tag) = html_list_direct_target_tag(node) {
        append(output, &format!(" id=\"{}\"", escape_html(&tag)), maximum)?;
    }
    append(output, ">\n", maximum)?;
    for item in body
        .children()
        .filter(|child| child.kind() == NodeKind::Block && child.macro_name() == Some("It"))
    {
        append(output, "  <tr", maximum)?;
        if let Some(tag) = html_node_target_tag(item) {
            append(output, &format!(" id=\"{}\"", escape_html(&tag)), maximum)?;
        }
        append(output, ">\n", maximum)?;
        for cell in item
            .children()
            .filter(|child| child.kind() == NodeKind::Body)
        {
            append(output, "    <td>", maximum)?;
            append(
                output,
                &render_html_inline_nodes(cell.children().collect::<Vec<_>>(), limits),
                maximum,
            )?;
            append(output, "</td>\n", maximum)?;
        }
        append(output, "  </tr>\n", maximum)?;
    }
    append(output, "</table>\n", maximum)
}

fn html_node_target_tag(node: NodeRef<'_>) -> Option<String> {
    if (node.flags().deep_link_target || node.flags().permalink)
        && let Some(tag) = node.tag().filter(|tag| !tag.is_empty())
    {
        return Some(tag.to_owned());
    }
    let mut pending = node.children().collect::<Vec<_>>();
    while let Some(node) = pending.pop() {
        if node.flags().deep_link_target || node.flags().permalink {
            if let Some(tag) = node.tag().filter(|tag| !tag.is_empty()) {
                return Some(tag.to_owned());
            }
            if let Some(text) = html_first_visible_text(node) {
                let text = text.strip_prefix('-').unwrap_or(text);
                let end = text.find(char::is_whitespace).unwrap_or(text.len());
                if end > 0 {
                    return Some(text[..end].to_owned());
                }
            }
        }
        pending.extend(node.children());
    }
    None
}

fn html_list_direct_target_tag(node: NodeRef<'_>) -> Option<String> {
    [
        Some(node),
        node.children().find(|child| child.kind() == NodeKind::Body),
    ]
    .into_iter()
    .flatten()
    .find_map(|node| {
        (node.flags().deep_link_target || node.flags().permalink)
            .then(|| node.tag().filter(|tag| !tag.is_empty()).map(str::to_owned))
            .flatten()
    })
}

/// Render the common mdoc `Bl -tag` shape.  The terminal-only selectors
/// (`-hang`, `-diag`, and friends) intentionally keep their distinct terminal
/// path; this DOM form is for the normalized definition-list contract.
fn render_html_mdoc_tag_list(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    append(output, "<dl class=\"Bl-tag\"", maximum)?;
    if let Some(tag) = html_list_direct_target_tag(node) {
        append(output, &format!(" id=\"{}\"", escape_html(&tag)), maximum)?;
    }
    append(output, ">\n", maximum)?;
    for item in body
        .children()
        .filter(|child| child.kind() == NodeKind::Block && child.macro_name() == Some("It"))
    {
        let head = item.children().find(|child| child.kind() == NodeKind::Head);
        let item_body = item.children().find(|child| child.kind() == NodeKind::Body);
        let mut head_content = if let Some(head) = head {
            let content = render_html_inline_nodes(head.children().collect::<Vec<_>>(), limits);
            let first_macro = head.children().find_map(NodeRef::macro_name);
            if matches!(first_macro, Some("Fl" | "Em" | "Sy")) {
                content
            } else {
                content.replace(" |\n    ", "\n    |\n    ")
            }
        } else {
            String::new()
        };
        let tag = head
            .filter(|head| head.flags().deep_link_target)
            .and_then(|head| html_unique_definition_target(head, state));
        if let Some(tag) = &tag {
            head_content = html_retarget_permalink(head_content, tag);
        }
        append(output, "  <dt", maximum)?;
        if let Some(tag) = tag {
            append(output, &format!(" id=\"{}\"", escape_html(&tag)), maximum)?;
        }
        append(output, ">", maximum)?;
        append(output, &head_content, maximum)?;
        append(output, "</dt>\n", maximum)?;
        if let Some(item_body) = item_body {
            append(output, "  <dd>", maximum)?;
            if item_body
                .children()
                .any(|child| child.kind() == NodeKind::Block && child.macro_name() == Some("Bd"))
            {
                render_html_mdoc_definition_body(item_body, limits, state, output, maximum)?;
            } else {
                let body_content =
                    render_html_inline_nodes(item_body.children().collect::<Vec<_>>(), limits);
                append(output, &body_content, maximum)?;
            }
            append(output, "</dd>\n", maximum)?;
        }
    }
    append(output, "</dl>\n", maximum)
}

/// A definition item's body may contain a nested display.  Keep direct prose
/// inside its `dd`, but indent the embedded block exactly as mandoc's HTML
/// device does instead of flattening the display into the definition text.
fn render_html_mdoc_definition_body(
    body: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let mut inline: Vec<NodeRef<'_>> = Vec::new();
    let mut emitted_block = false;
    for child in body.children() {
        if child.kind() != NodeKind::Block || child.macro_name() != Some("Bd") {
            inline.push(child);
            continue;
        }
        if !inline.is_empty() {
            if emitted_block {
                append(output, "\n    ", maximum)?;
            }
            append(
                output,
                &render_html_inline_nodes(std::mem::take(&mut inline), limits),
                maximum,
            )?;
        }
        append(output, "\n", maximum)?;
        let mut rendered = String::new();
        render_html_mdoc_display(child, limits, state, &mut rendered, maximum)?;
        let rendered = rendered.trim_end();
        for (index, line) in rendered.lines().enumerate() {
            if index > 0 {
                append(output, "\n", maximum)?;
            }
            append(output, "    ", maximum)?;
            append(output, line, maximum)?;
        }
        emitted_block = true;
    }
    if !inline.is_empty() {
        if emitted_block {
            append(output, "\n    ", maximum)?;
        }
        append(output, &render_html_inline_nodes(inline, limits), maximum)?;
    }
    Ok(())
}

fn html_retarget_permalink(mut content: String, tag: &str) -> String {
    if !content.contains("class=\"permalink\"") {
        let escaped = escape_html(tag);
        return format!("<a class=\"permalink\" href=\"#{escaped}\">{content}</a>");
    }
    let Some(prefix) = content.find("href=\"#") else {
        return content;
    };
    let start = prefix + "href=\"#".len();
    let Some(relative_end) = content[start..].find('"') else {
        return content;
    };
    content.replace_range(start..start + relative_end, &escape_html(tag));
    content
}

fn html_definition_head_tag(head: NodeRef<'_>) -> Option<String> {
    head.tag()
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            (head.flags().deep_link_target || head.flags().permalink)
                .then(|| html_first_visible_text_in_source_order(head))
                .flatten()
                .and_then(html_automatic_target)
        })
        .or_else(|| {
            let mut pending = head.children().collect::<Vec<_>>();
            while let Some(node) = pending.pop() {
                if node.flags().deep_link_target || node.flags().permalink {
                    if let Some(tag) = node.tag().filter(|tag| !tag.is_empty()) {
                        return Some(tag.to_owned());
                    }
                    if let Some(text) = html_first_visible_text(node) {
                        let text = text.strip_prefix('-').unwrap_or(text);
                        let end = text.find(char::is_whitespace).unwrap_or(text.len());
                        if end > 0 {
                            return Some(text[..end].to_owned());
                        }
                    }
                }
                pending.extend(node.children());
            }
            None
        })
}

fn html_first_visible_text_in_source_order(node: NodeRef<'_>) -> Option<&str> {
    if node.flags().no_print {
        return None;
    }
    if let Some(text) = node.text().filter(|text| !text.is_empty()) {
        return Some(text);
    }
    node.children()
        .find_map(html_first_visible_text_in_source_order)
}

fn html_unique_man_target(head: NodeRef<'_>, state: &mut HtmlState) -> Option<String> {
    let target = html_definition_head_tag(head)?;
    if target.contains('~') {
        return Some(target);
    }
    let count = state.man_targets.entry(target.clone()).or_insert(0);
    *count += 1;
    (*count > 1)
        .then(|| format!("{target}~{count}"))
        .or(Some(target))
}

fn html_unique_definition_target(head: NodeRef<'_>, state: &mut HtmlState) -> Option<String> {
    let target = html_definition_head_tag(head)?;
    if target.contains('~') {
        return Some(target);
    }
    let count = state.definition_targets.entry(target.clone()).or_insert(0);
    *count += 1;
    (*count > 1)
        .then(|| format!("{target}~{count}"))
        .or(Some(target))
}

fn html_unique_display_target(target: String, state: &mut HtmlState) -> String {
    if target.contains('~') {
        return target;
    }
    let count = state.display_targets.entry(target.clone()).or_insert(0);
    *count += 1;
    if *count > 1 {
        format!("{target}~{count}")
    } else {
        target
    }
}

fn html_automatic_target(text: &str) -> Option<String> {
    let text = text.strip_prefix('-').unwrap_or(text);
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    (end > 0).then(|| text[..end].to_owned())
}

fn html_first_visible_text(node: NodeRef<'_>) -> Option<&str> {
    let mut pending = vec![node];
    while let Some(node) = pending.pop() {
        if !node.flags().no_print {
            if let Some(text) = node.text().filter(|text| !text.is_empty()) {
                return Some(text);
            }
            pending.extend(node.children());
        }
    }
    None
}

/// Retain the historical fragment behavior for raw roff text outside a
/// semantic Body. Structured section paths use paragraph ownership instead.
fn render_html_flat_node(
    node: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if node.kind() == NodeKind::Text {
        if node.flags().line_start
            && !output.is_empty()
            && !output.ends_with('>')
            && !output.ends_with('\n')
        {
            if output.ends_with(' ') {
                let _ = output.pop();
            }
            append(output, "\n", maximum)?;
        }
        if let Some(text) = node.text() {
            append(
                output,
                &render_html_visible_text_with_font(
                    text,
                    limits,
                    html_request_font_before(node).current,
                ),
                maximum,
            )?;
            append(output, " ", maximum)?;
        }
        return Ok(());
    }
    if let Some(value) = node.equation() {
        let mathml = node.equation_terminal().map_or_else(
            || escape_html(value),
            |equation| render_html_equation(equation, limits),
        );
        append(output, "<math class=\"eqn\">", maximum)?;
        append(output, &mathml, maximum)?;
        append(output, "</math>", maximum)?;
    }
    Ok(())
}

fn render_html_section(
    node: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let subsection = matches!(node.macro_name(), Some("SS" | "Ss"));
    let class = if subsection { "Ss" } else { "Sh" };
    let level = if subsection { "h2" } else { "h1" };
    let head = node.children().find(|child| child.kind() == NodeKind::Head);
    let body = node.children().find(|child| child.kind() == NodeKind::Body);
    append(output, &format!("<section class=\"{class}\">\n"), maximum)?;
    if let Some(head) = head {
        let title = render_html_inline_nodes(head.children().collect::<Vec<_>>(), limits);
        if !title.is_empty() {
            let tag = head
                .tag()
                .filter(|tag| !tag.is_empty())
                .map_or_else(|| html_heading_identifier(&title), str::to_owned);
            let empty_heading = title == "&#x00A0;";
            let title = if title.starts_with(char::is_whitespace) || subsection {
                title.replacen(' ', "\n  ", 1)
            } else {
                title
            };
            if empty_heading {
                append(
                    output,
                    &format!("<{level} class=\"{class}\">{title}</{level}>\n"),
                    maximum,
                )?;
            } else {
                let tag = state.unique_heading_tag(tag);
                let escaped_tag = escape_html(&tag);
                let opening = format!(
                    "<{level} class=\"{class}\" id=\"{escaped_tag}\"><a class=\"permalink\" href=\"#{escaped_tag}\">"
                );
                // `Rs` switches SEE ALSO into standalone citation flow.  The
                // upstream HTML writer consequently folds this particular
                // heading in its device field; ordinary headings retain the
                // source-compatible layout path below.
                let title = if title == "SEE ALSO"
                    && body.is_some_and(|body| {
                        body.children().any(|child| {
                            child.kind() == NodeKind::Block && child.macro_name() == Some("Rs")
                        })
                    }) {
                    wrap_html_heading(&title, opening.len())
                } else {
                    title
                };
                append(
                    output,
                    &format!("{opening}{title}</a></{level}>\n"),
                    maximum,
                )?;
            }
        }
    }
    if let Some(body) = body {
        render_html_body(body, limits, state, output, maximum)?;
    }
    append(output, "</section>\n", maximum)
}

impl HtmlState {
    fn unique_heading_tag(&mut self, tag: String) -> String {
        let count = self.headings.entry(tag.clone()).or_default();
        *count += 1;
        if *count == 1 {
            tag
        } else {
            format!("{tag}~{count}")
        }
    }
}

fn html_heading_identifier(title: &str) -> String {
    title
        .chars()
        .map(|character| {
            if character.is_whitespace() {
                '_'
            } else {
                character
            }
        })
        .collect()
}

fn render_html_body(
    body: NodeRef<'_>,
    limits: &Limits,
    state: &mut HtmlState,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let mut inline: Vec<NodeRef<'_>> = Vec::new();
    let mut paragraph_tag = None;
    let mut direct_semantic_count = 0_usize;
    // `D1` and `Dl` terminate a paragraph, but their immediately following
    // ordinary phrase is device-level flow rather than a fresh HTML Pp.
    // Keep that narrow exception until a later structural request consumes it.
    let mut raw_after_one_line_display = false;
    for child in body.children() {
        if matches!(child.kind(), NodeKind::Text | NodeKind::Equation) {
            if inline
                .last()
                .is_some_and(|previous| previous.flags().no_fill != child.flags().no_fill)
            {
                render_html_inline_flow(
                    std::mem::take(&mut inline),
                    limits,
                    paragraph_tag.take(),
                    raw_after_one_line_display,
                    output,
                    maximum,
                )?;
                raw_after_one_line_display = false;
            }
            inline.push(child);
            direct_semantic_count = 0;
            continue;
        }
        if child.macro_name() == Some("br") {
            inline.push(child);
            continue;
        }
        if child.macro_name() == Some("sp") && child.flags().no_fill {
            inline.push(child);
            continue;
        }
        if child.macro_name() == Some("Tg") {
            if child.flags().deep_link_target {
                inline.push(child);
            }
            continue;
        }
        if child.macro_name() == Some("ft") {
            inline.push(child);
            continue;
        }
        if child.kind() == NodeKind::Block && child.macro_name() == Some("Rs") {
            // Reference blocks are normally an inline bibliography phrase.
            // SEE ALSO is the one mdoc section where the HTML device closes
            // the preceding paragraph and gives the citation its own Pp.
            if terminal_mdoc_section_named(body, "SEE ALSO") {
                render_html_inline_flow(
                    std::mem::take(&mut inline),
                    limits,
                    paragraph_tag.take(),
                    raw_after_one_line_display,
                    output,
                    maximum,
                )?;
                raw_after_one_line_display = false;
                render_html_reference_paragraph(child, limits, output, maximum)?;
            } else {
                inline.push(child);
            }
            direct_semantic_count = 0;
            continue;
        }
        if child.macro_name() == Some("Fo") && !inline.is_empty() {
            inline.push(child);
            direct_semantic_count = 0;
            continue;
        }
        if child.macro_name() == Some("Fn") {
            inline.push(child);
            direct_semantic_count = 0;
            continue;
        }
        if html_is_semantic_inline_macro(child) && !inline.is_empty() {
            inline.push(child);
            direct_semantic_count = 0;
            continue;
        }
        // `YS` only closes the already-rendered synopsis block.  It must not
        // consume the raw outer-flow boundary that the block owns.
        if child.macro_name() == Some("YS") {
            continue;
        }
        let standalone_semantic = inline.is_empty()
            && child.kind() == NodeKind::Element
            && html_is_semantic_inline_macro(child);
        render_html_inline_flow(
            std::mem::take(&mut inline),
            limits,
            paragraph_tag.take(),
            raw_after_one_line_display,
            output,
            maximum,
        )?;
        raw_after_one_line_display = false;
        if child.macro_name() == Some("Pp") {
            paragraph_tag = child
                .flags()
                .deep_link_target
                .then(|| child.tag().map(str::to_owned))
                .flatten();
            direct_semantic_count = 0;
            continue;
        }
        if child.macro_name() == Some("sp") {
            direct_semantic_count = 0;
            continue;
        }
        if standalone_semantic {
            if direct_semantic_count > 0 {
                append(output, "  ", maximum)?;
            }
            append(
                output,
                &render_html_inline_nodes(vec![child], limits),
                maximum,
            )?;
            append(output, "\n", maximum)?;
            direct_semantic_count += 1;
            continue;
        }
        direct_semantic_count = 0;
        render_html_node(child, limits, state, output, maximum)?;
        raw_after_one_line_display =
            matches!(child.macro_name(), Some("Bd" | "D1" | "Dl" | "RS" | "SY"));
    }
    render_html_inline_flow(
        inline,
        limits,
        paragraph_tag,
        raw_after_one_line_display,
        output,
        maximum,
    )
}

fn render_html_inline_flow(
    nodes: Vec<NodeRef<'_>>,
    limits: &Limits,
    tag: Option<String>,
    raw: bool,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if nodes.iter().any(|node| node.flags().no_fill) {
        return render_html_preformatted(nodes, limits, output, maximum);
    }
    if !raw {
        return render_html_paragraph(nodes, limits, tag, output, maximum);
    }
    let content = render_html_inline_nodes(nodes, limits)
        // `br` is indented when it belongs to a Pp.  This narrow raw-flow
        // path has no paragraph envelope, so keep the device line flush.
        .replace("\n  <br/>\n  ", "\n<br/>\n");
    if content.is_empty() {
        return Ok(());
    }
    append(output, &content, maximum)?;
    append(output, "\n", maximum)
}

fn render_html_preformatted(
    nodes: Vec<NodeRef<'_>>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let content = render_html_inline_nodes(nodes, limits);
    if content.is_empty() {
        return Ok(());
    }
    append(output, "<pre>", maximum)?;
    append(output, &content, maximum)?;
    append(output, "</pre>\n", maximum)
}

fn render_html_paragraph(
    nodes: Vec<NodeRef<'_>>,
    limits: &Limits,
    tag: Option<String>,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let has_font_request = nodes
        .iter()
        .any(|node| node.kind() == NodeKind::Element && node.macro_name() == Some("ft"));
    let mut content = render_html_inline_nodes(nodes, limits);
    if content.is_empty() {
        return Ok(());
    }
    if content.starts_with("<a class=\"permalink\"") && content.contains("<code class=\"Fn\"") {
        content = content.replacen("</a>() and", "</a>()\n    and", 1);
        content = content.replacen("and\n    <code class=\"Fn\">", "and <code class=\"Fn\">", 1);
    }
    let opening = if let Some(tag) = tag {
        let tag = escape_html(&tag);
        format!("<p class=\"Pp\" id=\"{tag}\">")
    } else {
        "<p class=\"Pp\">".to_owned()
    };
    append(output, &opening, maximum)?;
    let content = if content.contains("class=\"Rs\"") || has_font_request {
        wrap_html_reference_paragraph(&content, opening.len())
    } else {
        wrap_html_plain_paragraph(&content, opening.len())
    };
    append(output, &content, maximum)?;
    append(output, "</p>\n", maximum)
}

/// mandoc's HTML writer folds ordinary ASCII paragraph prose at its 80-column
/// output field.  Semantic markup and non-ASCII/device-escaped content keep
/// their dedicated paths: this narrow helper only owns plain text, where
/// splitting at source-independent word boundaries is lossless.
pub(super) fn wrap_html_plain_paragraph(content: &str, opening_width: usize) -> String {
    if content.contains('<') || !content.is_ascii() || opening_width >= 80 {
        return content.to_owned();
    }
    let mut output = String::with_capacity(content.len());
    let mut column = opening_width;
    for word in content.split(' ') {
        if word.is_empty() {
            continue;
        }
        let separator = usize::from(!output.is_empty());
        if column.saturating_add(separator).saturating_add(word.len()) > 80 {
            output.push_str("\n    ");
            output.push_str(word);
            column = 4 + word.len();
        } else {
            if separator != 0 {
                output.push(' ');
                column += 1;
            }
            output.push_str(word);
            column += word.len();
        }
    }
    output
}

/// Headings share the HTML device's narrow output field, but use its
/// two-column continuation indentation rather than paragraph indentation.
fn wrap_html_heading(content: &str, opening_width: usize) -> String {
    const WIDTH: usize = 72;
    if content.contains('<') || !content.is_ascii() || opening_width >= WIDTH {
        return content.to_owned();
    }
    let mut output = String::with_capacity(content.len());
    let mut column = opening_width;
    for word in content.split(' ') {
        if word.is_empty() {
            continue;
        }
        let separator = usize::from(!output.is_empty());
        if column.saturating_add(separator).saturating_add(word.len()) > WIDTH {
            output.push_str("\n  ");
            output.push_str(word);
            column = 2 + word.len();
        } else {
            if separator != 0 {
                output.push(' ');
                column += 1;
            }
            output.push_str(word);
            column += word.len();
        }
    }
    output
}

/// The historical HTML writer formats mdoc bibliography markup as device
/// output, not DOM pretty-printing: it folds markup tokens at column 78 and
/// uses a four-column continuation.  Keep that narrow behavior local to
/// `Rs`; ordinary semantic HTML intentionally retains its authored flow.
fn wrap_html_reference_paragraph(content: &str, opening_width: usize) -> String {
    const WIDTH: usize = 78;
    let mut output = String::with_capacity(content.len());
    let mut column = opening_width;
    let mut pending_space = false;
    let mut cursor = 0_usize;

    while cursor < content.len() {
        let remainder = &content[cursor..];
        if let Some(character) = remainder.chars().next()
            && character.is_whitespace()
        {
            let whitespace_end = cursor
                + remainder
                    .char_indices()
                    .take_while(|(_, character)| character.is_whitespace())
                    .last()
                    .map_or_else(
                        || character.len_utf8(),
                        |(index, character)| index + character.len_utf8(),
                    );
            let whitespace = &content[cursor..whitespace_end];
            if whitespace.contains('\n') {
                output.push_str(whitespace);
                column = whitespace
                    .rsplit_once('\n')
                    .map_or(column + whitespace.len(), |(_, tail)| tail.len());
                pending_space = false;
            } else {
                pending_space = true;
            }
            cursor = whitespace_end;
            continue;
        }

        let token_end = if remainder.starts_with('<') {
            remainder
                .find('>')
                .map_or(content.len(), |index| cursor + index + 1)
        } else {
            cursor
                + remainder
                    .char_indices()
                    .take_while(|(_, character)| !character.is_whitespace() && *character != '<')
                    .last()
                    .map_or_else(
                        || remainder.chars().next().map_or(0, char::len_utf8),
                        |(index, character)| index + character.len_utf8(),
                    )
        };
        let token_end = html_compact_element_end(content, cursor).unwrap_or(token_end);
        let token = &content[cursor..token_end];
        let separator = usize::from(pending_space && !output.is_empty());
        if separator != 0 && column.saturating_add(separator + token.len()) > WIDTH {
            output.push_str("\n    ");
            column = 4;
        } else if separator != 0 {
            output.push(' ');
            column += 1;
        }
        output.push_str(token);
        column += token.len();
        pending_space = false;
        cursor = token_end;
    }
    output
}

/// Keep a no-space semantic HTML element together while applying the device
/// output-field fold.  The C writer opens and closes these wrappers around
/// one word atomically; treating their opening tag separately would allow a
/// long literal wrapper to overflow before its visible word is considered.
fn html_compact_element_end(content: &str, start: usize) -> Option<usize> {
    let remainder = content.get(start..)?;
    let opening_end = remainder.find('>')?;
    let opening = &remainder[..=opening_end];
    if !opening.starts_with('<') || opening.starts_with("</") || opening.ends_with("/>") {
        return None;
    }
    let name_end =
        opening[1..].find(|character: char| character.is_whitespace() || character == '>')? + 1;
    let name = &opening[1..name_end];
    let closing = format!("</{name}>");
    let content_start = start + opening_end + 1;
    let closing_start = content.get(content_start..)?.find(&closing)? + content_start;
    (!content[content_start..closing_start]
        .chars()
        .any(char::is_whitespace))
    .then_some(closing_start + closing.len())
}

/// Render an `Rs` block as the HTML device's inline citation.  The parser
/// has already imposed libmandoc's field order; presentation supplies field
/// classes, author conjunctions, field separators, and its final period.
fn render_html_reference_block(node: NodeRef<'_>, limits: &Limits) -> String {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return String::new();
    };
    let fields = body
        .children()
        .filter(|child| !child.flags().no_print)
        .collect::<Vec<_>>();
    let mut phrases = Vec::new();
    let mut index = 0_usize;
    while index < fields.len() {
        if fields[index].macro_name() == Some("%A") {
            let mut authors = Vec::new();
            while index < fields.len() && fields[index].macro_name() == Some("%A") {
                if let Some(author) = render_html_reference_field(fields[index], limits) {
                    authors.push(author);
                }
                index += 1;
            }
            let authors = match authors.as_slice() {
                [] => String::new(),
                [author] => author.clone(),
                [first, second] => format!("{first} and {second}"),
                _ => {
                    let mut value = authors[..authors.len() - 1].join(", ");
                    value.push_str(", and ");
                    value.push_str(authors.last().expect("authors is nonempty"));
                    value
                }
            };
            if !authors.is_empty() {
                phrases.push(authors);
            }
            continue;
        }
        if let Some(phrase) = render_html_reference_field(fields[index], limits) {
            phrases.push(phrase);
        }
        index += 1;
    }
    if phrases.is_empty() {
        return String::new();
    }
    format!("<cite class=\"Rs\">{}.</cite>", phrases.join(", "))
}

fn render_html_reference_field(field: NodeRef<'_>, limits: &Limits) -> Option<String> {
    let name = field.macro_name()?;
    if !matches!(
        name,
        "%A" | "%B"
            | "%C"
            | "%D"
            | "%I"
            | "%J"
            | "%N"
            | "%O"
            | "%P"
            | "%Q"
            | "%R"
            | "%T"
            | "%U"
            | "%V"
    ) {
        return None;
    }
    let value = render_html_inline_nodes(field.children().collect::<Vec<_>>(), limits);
    if value.is_empty() {
        return None;
    }
    let class = &name[1..];
    if name == "%U" {
        let href = html_first_visible_text_in_source_order(field)?;
        return Some(format!(
            "<a class=\"Rs{class}\" href=\"{}\">{value}</a>",
            escape_html(href)
        ));
    }
    let element = if matches!(name, "%B" | "%I" | "%J") {
        "i"
    } else {
        "span"
    };
    Some(format!(
        "<{element} class=\"Rs{class}\">{value}</{element}>"
    ))
}

fn render_html_reference_paragraph(
    node: NodeRef<'_>,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let citation = render_html_reference_block(node, limits);
    if citation.is_empty() {
        return Ok(());
    }
    let opening = "<p class=\"Pp\">";
    append(output, opening, maximum)?;
    append(
        output,
        &wrap_html_reference_paragraph(&citation, opening.len()),
        maximum,
    )?;
    append(output, "</p>\n", maximum)
}

fn render_html_inline_nodes(nodes: Vec<NodeRef<'_>>, limits: &Limits) -> String {
    let mut output = String::new();
    let mut previous: Option<NodeRef<'_>> = None;
    for node in nodes {
        if node.flags().no_print || node.ancestors().any(|ancestor| ancestor.flags().no_print) {
            continue;
        }
        let content = match node.kind() {
            NodeKind::Text => node.text().map(|text| {
                render_html_visible_text_with_font(
                    text,
                    limits,
                    html_request_font_before(node).current,
                )
            }),
            NodeKind::Equation => node.equation().map(|value| {
                // Keep the device's equation envelope even when an eqn block
                // appears in a semantic paragraph.  In particular, the
                // upstream regression harness locates MathML through this
                // exact marker rather than through surrounding block layout.
                let mathml = node.equation_terminal().map_or_else(
                    || escape_html(value),
                    |equation| render_html_equation(equation, limits),
                );
                format!("<math class=\"eqn\">{mathml}</math>")
            }),
            NodeKind::Block if node.macro_name() == Some("Fo") => {
                Some(render_html_function_declaration(node, limits))
            }
            NodeKind::Block if node.macro_name() == Some("Rs") => {
                Some(render_html_reference_block(node, limits))
            }
            NodeKind::Element if node.macro_name() == Some("ft") => None,
            _ => {
                let nested = render_html_inline_nodes(node.children().collect::<Vec<_>>(), limits);
                match node.macro_name() {
                    Some("br") => Some("\n  <br/>\n  ".to_owned()),
                    Some("sp") if node.flags().no_fill => Some("\n".to_owned()),
                    Some("Pp" | "sp") => None,
                    Some("Tg") if node.flags().deep_link_target && !nested.is_empty() => {
                        Some(format!("<mark id=\"{}\"></mark>", escape_html(&nested)))
                    }
                    _ if nested.is_empty() => None,
                    Some("Pq") if node.kind() == NodeKind::Block => Some(format!("({nested})")),
                    Some("Bq") if nested.starts_with('[') && nested.ends_with(']') => Some(nested),
                    Some("Bq") => Some(format!("[{nested}]")),
                    _ => {
                        if let Some(enclosure) = node.enclosure() {
                            let closing = enclosure.closing.as_deref().unwrap_or_default();
                            Some(format!(
                                "{}{}{}",
                                escape_html(&enclosure.opening),
                                nested,
                                escape_html(closing)
                            ))
                        } else {
                            Some(nested)
                        }
                    }
                }
            }
        };
        let Some(content) = content.filter(|content| !content.is_empty()) else {
            continue;
        };
        let tag = html_inline_tag(node, &content);
        let content = render_html_inline_semantics(
            node,
            &content,
            node.flags().deep_link_target,
            tag.as_deref(),
        );
        if let Some(previous) = previous {
            if node.flags().line_start && node.flags().no_fill && node.macro_name() != Some("sp") {
                output.push('\n');
            } else if previous.flags().permalink
                && node.flags().line_start
                && node.macro_name() != Some("br")
            {
                if previous.macro_name() == Some("Fn") {
                    output.push(' ');
                } else if previous.flags().deep_link_target {
                    output.push_str("\n    ");
                } else {
                    output.push_str("\n  ");
                }
            } else if matches!(node.macro_name(), Some("Fn" | "Fo")) && node.flags().line_start {
                output.push_str("\n    ");
            } else if node.macro_name() == Some("Tg") && node.flags().deep_link_target {
                output.push(' ');
            } else if node.flags().deep_link_target {
                output.push_str("\n    ");
            } else if previous.macro_name() != Some("br")
                && node.macro_name() != Some("br")
                && node.macro_name() != Some("sp")
                && !previous.flags().delimiter_open
                && !node.flags().delimiter_close
            {
                output.push(' ');
            }
        }
        if node.flags().permalink {
            if let Some(tag) = tag {
                let tag = escape_html(&tag);
                output.push_str("<a class=\"permalink\" href=\"#");
                output.push_str(&tag);
                output.push_str("\">");
                output.push_str(&content);
                output.push_str("</a>");
                if node.macro_name() == Some("Fn") {
                    output.push_str("()");
                }
            } else {
                output.push_str(&content);
            }
        } else {
            output.push_str(&content);
            if node.macro_name() == Some("Fn") {
                output.push_str("()");
            }
        }
        previous = Some(node);
    }
    output
}

/// Render an mdoc `Fo` declaration as one callable function phrase.  Its
/// Head owns the function destination and its Body owns the parenthesized
/// argument sequence; the terminating `Fc` contributes the declaration's
/// semicolon without surviving as a public AST node.
fn render_html_function_declaration(node: NodeRef<'_>, limits: &Limits) -> String {
    let Some(head) = node.children().find(|child| child.kind() == NodeKind::Head) else {
        return String::new();
    };
    let name = render_html_inline_nodes(head.children().collect::<Vec<_>>(), limits);
    if name.is_empty() {
        return String::new();
    }
    let tag = head
        .tag()
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .or_else(|| html_automatic_target(&name));
    let id = head
        .flags()
        .deep_link_target
        .then(|| tag.as_deref().map(escape_html))
        .flatten()
        .map_or_else(String::new, |tag| format!(" id=\"{tag}\""));
    let code = format!("<code class=\"Fn\"{id}>{name}</code>");
    let name = if head.flags().permalink {
        tag.map_or_else(
            || code.clone(),
            |tag| {
                let tag = escape_html(&tag);
                format!("<a class=\"permalink\" href=\"#{tag}\">{code}</a>")
            },
        )
    } else {
        code
    };
    let arguments = node
        .children()
        .find(|child| child.kind() == NodeKind::Body)
        .map(|body| render_html_inline_nodes(body.children().collect::<Vec<_>>(), limits))
        .unwrap_or_default();
    format!("{name}({arguments});")
}

/// Return the destination spelling that mandoc derives for an inline target.
/// Explicit `.Tg` values win; automatic mdoc names omit one leading option
/// dash and end at the first source-space boundary.
fn html_inline_tag(node: NodeRef<'_>, content: &str) -> Option<String> {
    if node.macro_name() == Some("Fn") {
        return node
            .tag()
            .filter(|tag| !tag.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                html_first_visible_text_in_source_order(node).and_then(html_automatic_target)
            });
    }
    node.tag()
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            (node.flags().deep_link_target || node.flags().permalink).then(|| {
                let content = content.strip_prefix('-').unwrap_or(content);
                let end = content.find(char::is_whitespace).unwrap_or(content.len());
                content[..end].to_owned()
            })
        })
        .filter(|tag| !tag.is_empty())
}

/// Map the normalized mdoc inline families to their stable HTML device tags.
/// The public arena intentionally retains the generic macro spelling, so this
/// remains a renderer-only mapping rather than an AST widening.
fn render_html_inline_semantics(
    node: NodeRef<'_>,
    content: &str,
    id: bool,
    tag: Option<&str>,
) -> String {
    let (element, class, prefix) = match node.macro_name() {
        Some("B") => return format!("<b>{content}</b>"),
        Some("I") => return format!("<i>{content}</i>"),
        Some("Fl") => ("code", "Fl", (!content.starts_with("--")).then_some("-")),
        Some("Cm" | "Dv" | "Er" | "Ev" | "Ic" | "Li") => {
            ("code", node.macro_name().unwrap_or_default(), None)
        }
        Some("Em") => ("i", "Em", None),
        Some("Sy") => ("b", "Sy", None),
        Some("Fa") => ("var", "Fa", None),
        Some("Fn") => ("code", "Fn", None),
        Some("No" | "Ms") => ("span", node.macro_name().unwrap_or_default(), None),
        _ => return content.to_owned(),
    };
    let id = id
        .then(|| tag.map(escape_html))
        .flatten()
        .map_or_else(String::new, |tag| format!(" id=\"{tag}\""));
    format!(
        "<{element} class=\"{class}\"{id}>{prefix}{content}</{element}>",
        prefix = prefix.unwrap_or_default()
    )
}

fn html_is_semantic_inline_macro(node: NodeRef<'_>) -> bool {
    matches!(
        node.macro_name(),
        Some(
            "Fl" | "Cm"
                | "Dv"
                | "Er"
                | "Ev"
                | "Ic"
                | "Li"
                | "Em"
                | "Sy"
                | "Fa"
                | "Fn"
                | "No"
                | "Ms"
        )
    )
}
