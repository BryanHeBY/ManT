use super::{
    AuthorMode, DEFAULT_RENDER_WIDTH, DisplayKind, Document, Limits, MacroSet, MdocListMarker,
    NodeKind, NodeRef, NormalizedFont, NormalizedListKind, RENDER_LITERAL_BACKSLASH_MARKER,
    RenderError, RenderErrorKind, RenderFormat, TERMINAL_ATTACH_NEXT_MARKER,
    TERMINAL_CENTER_MARKER, TERMINAL_CONTINUE_SOURCE_LINE_MARKER, TERMINAL_EMPTY_WORD_MARKER,
    TERMINAL_FORCE_SEPARATOR_MARKER, TERMINAL_HANGING_INDENT_MARKER, TERMINAL_KEEP_SPACING_MARKER,
    TERMINAL_LINE_LENGTH_MARKER, TERMINAL_LITERAL_PUNCTUATION_MARKER, TERMINAL_LITERAL_TAB_MARKER,
    TERMINAL_NO_HYPHEN_BREAK_MARKER, TERMINAL_NO_SPACE_MARKER, TERMINAL_NO_WRAP_MARKER,
    TERMINAL_NONBREAKING_SPACE_MARKER, TERMINAL_OPTIONAL_BREAK_MARKER,
    TERMINAL_PENDING_LINE_BREAK_MARKER, TERMINAL_RIGHT_MARKER, TERMINAL_SENTENCE_PENDING_MARKER,
    TERMINAL_SENTENCE_SPACE_MARKER, TERMINAL_TAB_STOPS_MARKER, TERMINAL_TABLE_VERTICAL_SKIP_MARKER,
    TERMINAL_TEMPORARY_INDENT_MARKER, TERMINAL_VERTICAL_SKIP_MARKER,
    TERMINAL_ZERO_WIDTH_BACKSPACE_MARKER, TableAlignment, TableTerminalBorder, TableTerminalFont,
    TableTerminalRow, TerminalFont, TerminalJoin, TerminalLineLength, TerminalMdocSmRelink,
    TerminalPageOffsetState, TerminalRequestFontState, TerminalRequestIndentState,
    TerminalTabLayout, TerminalTabStops, TerminalTextLayout, UnicodeWidthChar, append,
    ascii_terminal_character, render_numeric_character_escapes, render_terminal_equation,
    render_terminal_equation_text, render_terminal_visible_text,
    render_terminal_visible_text_with_font, render_terminal_whitespace_escapes,
    render_unicode_character_escapes,
};

/// Render terminal formats from semantic section blocks rather than a flat
/// preorder stream. This retains a section's Head/Body boundary even though a
/// Head's text often has no independent line-start flag.
pub(super) fn render_terminal_document(
    document: &Document,
    format: RenderFormat,
    width: usize,
    maximum: usize,
    limits: &Limits,
) -> Result<String, RenderError> {
    let mut output = String::new();
    let protected_header_lines =
        append_terminal_header(document, format, width, limits, &mut output, maximum)?;
    let Some(root) = document.node(document.root()) else {
        return Ok(output);
    };
    for node in root.children() {
        // A malformed, argumentless `.SH`/`.SS` is deliberately absent from
        // the compatible tree: subsequent man nodes are therefore attached
        // directly to Root.  term.c keeps the current man body field in that
        // recovery shape, though, rather than resetting it to column zero.
        // Root-level section blocks still own their distinct heading/body
        // geometry; every other direct man child resumes in the ordinary
        // seven-column body field.
        let indentation = if document.macro_set() == MacroSet::Man && !is_section_block(node) {
            7
        } else {
            0
        };
        render_terminal_node(node, format, limits, indentation, &mut output, maximum)?;
    }
    let protected_footer_lines =
        append_terminal_footer(document, format, width, limits, &mut output, maximum)?;
    let mut rendered = wrap_terminal_output(
        output.trim_end(),
        width,
        maximum,
        protected_header_lines,
        protected_footer_lines,
    )?;
    if !rendered.is_empty() {
        append(&mut rendered, "\n", maximum)?;
    }
    Ok(rendered)
}

/// Emit the shared terminal-page heading from normalized metadata.
///
/// The stable terminal device reserves the first line for the manual
/// identifier at both margins and the collection name in the centre.  This is
/// deliberately independent from the package-specific body walkers: man and
/// mdoc produce the same three-field geometry once parsing has normalised
/// their control macros into [`crate::Metadata`].  Pages without a title or a
/// section (for example a raw roff fragment) remain headerless.
pub(super) fn append_terminal_header(
    document: &Document,
    format: RenderFormat,
    width: usize,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<usize, RenderError> {
    let metadata = document.metadata();
    let Some(title) = metadata.title.as_deref() else {
        return Ok(0);
    };
    let section = metadata.section.as_deref();
    if document.macro_set() == MacroSet::Man && section.is_none() {
        return Ok(0);
    }
    let identifier =
        section.map_or_else(|| title.to_owned(), |section| format!("{title}({section})"));
    let mut volume = metadata.volume.as_deref().map_or_else(
        || {
            if document.macro_set() == MacroSet::Mdoc && section.is_none() {
                "LOCAL".to_owned()
            } else {
                terminal_default_volume(section.unwrap_or_default()).to_owned()
            }
        },
        str::to_owned,
    );
    if let Some(architecture) = metadata.arch.as_deref()
        && !architecture.is_empty()
    {
        volume.push_str(" (");
        volume.push_str(architecture);
        volume.push(')');
    }
    let identifier = render_visible_text(&identifier, format, limits);
    let volume = render_visible_text(&volume, format, limits);
    let identifier_width = display_width(&identifier);
    let volume_width = display_width(&volume);
    // `print_mdoc_head()`/`print_man_head()` first reserve the middle field.
    // The strict comparison is intentional: when exactly full, the C device
    // uses the wide middle-volume field and omits the duplicated right title
    // instead of concatenating all three header words.
    let centre = if identifier_width
        .saturating_add(1)
        .saturating_mul(2)
        .saturating_add(volume_width)
        < width
    {
        width.saturating_sub(volume_width).saturating_add(1) / 2
    } else if volume_width < width {
        width.saturating_sub(volume_width)
    } else {
        0
    };
    // An identifier wider than its initially reserved field makes the C
    // terminal flush it on a line of its own before rendering the volume.
    // Preserve that overflow path for intentionally tiny custom widths too.
    if identifier_width > centre {
        append(output, &identifier, maximum)?;
        append(output, "\n", maximum)?;
        // A title wider than the device line owns its line outright.  The
        // terminal device still right-justifies the otherwise fitting manual
        // volume on the following physical line; treating both fields as
        // unpositioned fallbacks loses that stable page-heading geometry.
        if volume_width <= width {
            append(
                output,
                &" ".repeat(width.saturating_sub(volume_width)),
                maximum,
            )?;
        }
        append(output, &volume, maximum)?;
        append(output, "\n\n", maximum)?;
        return Ok(2);
    }
    let left_padding = centre.saturating_sub(identifier_width);
    let right_start = if centre
        .saturating_add(volume_width)
        .saturating_add(identifier_width)
        < width
    {
        width.saturating_sub(identifier_width)
    } else {
        width
    };
    let right_padding = right_start.saturating_sub(centre.saturating_add(volume_width));
    append(output, &identifier, maximum)?;
    append(output, &" ".repeat(left_padding), maximum)?;
    append(output, &volume, maximum)?;
    append(output, &" ".repeat(right_padding), maximum)?;
    if right_start.saturating_add(identifier_width) <= width {
        append(output, &identifier, maximum)?;
    }
    append(output, "\n\n", maximum)?;
    Ok(1)
}

pub(super) fn terminal_default_volume(section: &str) -> &'static str {
    // `msec.in` is part of the pinned terminal-device contract. Match the
    // whole section rather than its first character: `3p`, for example, has
    // a distinct Perl volume rather than section 3's library heading.
    match section {
        "1" => "General Commands Manual",
        "2" => "System Calls Manual",
        "3" => "Library Functions Manual",
        "3p" => "Perl Library Manual",
        "4" => "Device Drivers Manual",
        "5" => "File Formats Manual",
        "6" => "Games Manual",
        "7" => "Miscellaneous Information Manual",
        "8" => "System Manager's Manual",
        "9" => "Kernel Developer's Manual",
        _ => "",
    }
}

/// Emit the metadata footer using the terminal device's fixed three-column
/// layout. Man pages end with `system / date / identifier`; mdoc pages use the
/// declared system at both margins. Like the header, this is metadata-only and
/// never depends on the host locale or clock.
pub(super) fn append_terminal_footer(
    document: &Document,
    format: RenderFormat,
    width: usize,
    limits: &Limits,
    output: &mut String,
    maximum: usize,
) -> Result<usize, RenderError> {
    let metadata = document.metadata();
    let Some(title) = metadata.title.as_deref() else {
        return Ok(0);
    };
    let section = metadata.section.as_deref();
    if document.macro_set() == MacroSet::Man && section.is_none() {
        return Ok(0);
    }
    // A syntactically present man `.TH` always opens the terminal page. Its
    // date and system fields may both be empty or recovered independently;
    // they still produce the three-column footer, just like an argument-less
    // `.TH`. Documents without a title request returned above remain
    // footerless.
    let date = metadata.date.as_deref().unwrap_or("");
    let system = metadata.os.as_deref().unwrap_or("OpenBSD");
    let right = if document.macro_set() == MacroSet::Man {
        format!("{title}({})", section.unwrap_or_default())
    } else {
        system.to_owned()
    };
    let system = render_visible_text(system, format, limits);
    let date = render_visible_text(date, format, limits);
    let right = render_visible_text(&right, format, limits);
    if document_ends_with_terminal_spacing(document) {
        append_terminal_footer_space(output, maximum)?;
    } else {
        append_blank_line(output, maximum)?;
    }
    append_terminal_three_column_line(output, &system, &date, &right, width, maximum)
}

/// Reserve the terminal device's final vertical slot before its page footer.
///
/// This intentionally differs from [`append_blank_line`]: after a document's
/// final `.sp`, `term_vspace()` used by libmandoc's footer is cumulative.  The
/// request has already completed one empty line, but the footer still requests
/// another one.  Boxed tables and negative spacing retain their private skip
/// markers, which are consumed by that same request instead of manufacturing a
/// blank line.
pub(super) fn append_terminal_footer_space(
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if output.ends_with(TERMINAL_SENTENCE_PENDING_MARKER) {
        let _ = output.pop();
    }
    if take_terminal_vertical_skip(output) || take_terminal_table_vertical_skip(output) {
        return Ok(());
    }
    if output.is_empty() {
        return Ok(());
    }
    if output.ends_with('\n') {
        append(output, "\n", maximum)
    } else {
        append(output, "\n\n", maximum)
    }
}

/// Whether the last terminal-affecting request in the document is `.sp`.
///
/// The terminal backend only exposes its current field state to the footer,
/// while this renderer deliberately keeps that state local to each semantic
/// node.  Recover the one observable cross-boundary fact by following the
/// normal recursive rendering order.  Text nested below `.sp` is its numeric
/// argument, not trailing prose; otherwise the last text/table node resets the
/// marker.  Structural wrapper nodes have no terminal effect of their own.
pub(super) fn document_ends_with_terminal_spacing(document: &Document) -> bool {
    fn visit(node: NodeRef<'_>, inside_spacing: bool, last_is_spacing: &mut bool) {
        if node.flags().no_print {
            return;
        }
        let is_spacing = node.macro_name() == Some("sp");
        if matches!(node.kind(), NodeKind::Text | NodeKind::Table) && !inside_spacing {
            *last_is_spacing = false;
        }
        for child in node.children() {
            visit(child, inside_spacing || is_spacing, last_is_spacing);
        }
        if is_spacing {
            *last_is_spacing = true;
        }
    }

    let Some(root) = document.node(document.root()) else {
        return false;
    };
    let mut last_is_spacing = false;
    for child in root.children() {
        visit(child, false, &mut last_is_spacing);
    }
    last_is_spacing
}

pub(super) fn append_terminal_three_column_line(
    output: &mut String,
    left: &str,
    centre: &str,
    right: &str,
    width: usize,
    maximum: usize,
) -> Result<usize, RenderError> {
    let left_width = display_width(left);
    let centre_width = display_width(centre);
    let right_width = display_width(right);
    if left_width
        .saturating_add(centre_width)
        .saturating_add(right_width)
        > width
    {
        append(output, left, maximum)?;
        // Keep the left and centre fields together whenever that pair fits:
        // `term_end()` uses the ordinary centre column even if the right
        // identifier must spill to its own line.  Conversely an oversized
        // centre field gets its own line, while a fitting right field remains
        // right-justified on the final one.
        let left_and_centre_fit = left_width.saturating_add(centre_width) <= width;
        if left_and_centre_fit {
            let centre_start = width.saturating_sub(centre_width).saturating_add(1) / 2;
            append(
                output,
                &" ".repeat(centre_start.saturating_sub(left_width)),
                maximum,
            )?;
            append(output, centre, maximum)?;
        }
        append(output, "\n", maximum)?;
        if !left_and_centre_fit {
            let centre_start = width.saturating_sub(centre_width).saturating_add(1) / 2;
            append(output, &" ".repeat(centre_start), maximum)?;
            append(output, centre, maximum)?;
            append(output, "\n", maximum)?;
        }
        if right_width <= width {
            append(
                output,
                &" ".repeat(width.saturating_sub(right_width)),
                maximum,
            )?;
        }
        append(output, right, maximum)?;
        return Ok(if left_and_centre_fit { 2 } else { 3 });
    }
    let centre_start = width.saturating_sub(centre_width).saturating_add(1) / 2;
    let left_padding = centre_start.saturating_sub(left_width);
    let right_start = width.saturating_sub(right_width);
    let right_padding = right_start.saturating_sub(centre_start.saturating_add(centre_width));
    append(output, left, maximum)?;
    append(output, &" ".repeat(left_padding), maximum)?;
    append(output, centre, maximum)?;
    append(output, &" ".repeat(right_padding), maximum)?;
    append(output, right, maximum)?;
    Ok(1)
}

mod node;
use node::render_terminal_node;

/// Render a text node, optionally retaining an IP field's no-fill first line
/// after the field padding instead of treating its source line as a new
/// terminal line. This is a device-layout override; the public node flags
/// remain untouched.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_terminal_text_node(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
    inline_no_fill_line_start: bool,
) -> Result<(), RenderError> {
    let Some(text) = node.text() else {
        return Ok(());
    };
    let mut rendered =
        render_terminal_visible_text_with_font(text, format, limits, terminal_text_font(node));
    if node
        .ancestors()
        .any(|ancestor| ancestor.macro_name() == Some("No"))
    {
        rendered = rendered.replace('-', "-\u{19}");
    }
    let source_no_fill = node.flags().no_fill;
    let no_fill = source_no_fill && node.flags().line_start;
    let inline_conditional_body = node.terminal_inline_conditional();
    if inline_conditional_body && rendered.starts_with(' ') {
        // The ordinary fill separator belongs to the preceding body node;
        // this leading blank is an additional authored cell after `\}`. Keep
        // it with its suffix while still allowing later prose to wrap.
        rendered.replace_range(
            ..' '.len_utf8(),
            &TERMINAL_NONBREAKING_SPACE_MARKER.to_string(),
        );
    }
    if !no_fill && rendered.contains("  ") {
        rendered = terminal_internal_spaces_to_nonbreaking(&rendered);
    }
    let indentation = terminal_text_indentation(node, indentation);
    // The terminal device preserves a no-fill line's word and tab layout,
    // but still discards trailing source whitespace. In particular, an empty
    // macro argument must not leave one visible blank after a final colon.
    let rendered = if no_fill {
        rendered.trim_end()
    } else {
        rendered.as_str()
    };
    // The public AST intentionally normalizes ordinary argument separation,
    // but the arena retains its width for package restructuring. The terminal
    // device observes a run of adjacent spaces, including `\\ ` escapes
    // normalized inside one visible text node, so preserve it before the
    // final prose reflow would collapse it.
    let separator_width = node.separator_width() as usize;
    let preserve_spacing = separator_width > 1;
    let keep_spacing = rendered.contains('\t') || preserve_spacing;
    let literal_tabs = no_fill && node.ancestors().any(NodeRef::literal_display);
    // A detached mdoc punctuation token can be syntactically adjacent to the
    // following inline macro. Only the parser's sentence flag distinguishes
    // `Cd . z` from an actual prose sentence boundary.
    let detached_mdoc_punctuation = matches!(text, "." | "!" | "?")
        && !node.flags().sentence_end
        && node
            .ancestors()
            .any(|ancestor| matches!(ancestor.macro_name(), Some("Sh" | "Ss")));
    mark_terminal_line_length(output, terminal_line_length_before(node), maximum)?;
    append_terminal_text(
        output,
        rendered,
        TerminalTextLayout {
            // mdoc source newlines normally remain fillable whitespace. Man
            // likewise fills ordinary source lines; only no-fill text and a
            // leading tab field/source space retain a physical boundary.
            line_start: !inline_no_fill_line_start
                && node.flags().line_start
                && !inline_conditional_body
                && (no_fill || rendered.starts_with(['\t', ' '])),
            join: if node.flags().delimiter_close {
                TerminalJoin::Attach
            } else {
                TerminalJoin::Separate
            },
            no_fill: no_fill && !rendered.trim().is_empty(),
            no_fill_continuation: source_no_fill && !node.flags().line_start,
            keep_spacing,
            // A plain mdoc text node retains the same terminal sentence
            // boundary as prose in a man paragraph. Semantic mdoc macros
            // retain their distinct delimiter and inline-spacing rules.
            sentence_end: node.flags().sentence_end
                && terminal_sentence_terminator(rendered)
                && !no_fill
                && (node
                    .ancestors()
                    .any(|ancestor| matches!(ancestor.macro_name(), Some("SH" | "SS")))
                    || terminal_mdoc_plain_text_sentence(node)),
            literal_punctuation: node
                .ancestors()
                .any(terminal_mdoc_inline_punctuation_is_literal)
                || detached_mdoc_punctuation,
            tabs: if literal_tabs {
                TerminalTabLayout::PhysicalLiteral
            } else {
                TerminalTabLayout::Relative
            },
        },
        indentation,
        maximum,
    )?;
    if separator_width > 1 {
        append(output, &" ".repeat(separator_width - 1), maximum)?;
    }
    if node.flags().delimiter_open {
        append(output, &TERMINAL_ATTACH_NEXT_MARKER.to_string(), maximum)?;
    }
    if node.flags().line_continuation && !text.ends_with("\\z\\c") {
        // `\c` is already normalized out of the public text while its
        // scanner flag records that the next physical source phrase loses
        // the usual fill/no-fill boundary and separator. A trailing `\c`
        // *inside* `\z` remains the zero-width operand rather than a
        // physical-line continuation.
        append(output, &TERMINAL_ATTACH_NEXT_MARKER.to_string(), maximum)?;
    }
    Ok(())
}

pub(super) fn terminal_quote_delimiters(
    node: NodeRef<'_>,
    body: Option<NodeRef<'_>>,
    format: RenderFormat,
) -> Option<(&'static str, &'static str)> {
    match node.macro_name() {
        Some("Ao" | "Aq") if body.is_some_and(terminal_quote_is_mail_target) => Some(("<", ">")),
        Some("Ao" | "Aq") if matches!(format, RenderFormat::Utf8) => Some(("⟨", "⟩")),
        Some("Ao" | "Aq") => Some(("<", ">")),
        Some("Bo" | "Bq" | "Oo" | "Op") => Some(("[", "]")),
        Some("Bro" | "Brq") => Some(("{", "}")),
        Some("Do" | "Dq") if matches!(format, RenderFormat::Utf8) => Some(("“", "”")),
        Some("Do" | "Dq" | "Qo" | "Qq") => Some(("\"", "\"")),
        Some("Po" | "Pq") => Some(("(", ")")),
        Some("Ql" | "So" | "Sq") if matches!(format, RenderFormat::Utf8) => Some(("‘", "’")),
        Some("Ql" | "So" | "Sq") => Some(("`", "'")),
        _ => None,
    }
}

pub(super) fn terminal_quote_is_mail_target(body: NodeRef<'_>) -> bool {
    let mut children = body.children();
    children
        .next()
        .is_some_and(|child| child.macro_name() == Some("Mt"))
        && children.next().is_none()
}

/// A recovered explicit closer is represented by an empty Body bearing the
/// opening macro's name.  It must be emitted where it appears in source order
/// rather than deferred to the outer Block's normal terminal post-hook.
pub(super) fn terminal_embedded_quote_closing(
    node: NodeRef<'_>,
    format: RenderFormat,
) -> Option<&'static str> {
    (node.kind() == NodeKind::Body && node.children().next().is_none())
        .then(|| terminal_quote_delimiters(node, None, format))
        .flatten()
        .map(|(_, closing)| closing)
}

pub(super) fn terminal_quote_has_embedded_closer(
    body: NodeRef<'_>,
    macro_name: Option<&str>,
) -> bool {
    body.children().any(|child| {
        (child.kind() == NodeKind::Body
            && child.macro_name() == macro_name
            && child.children().next().is_none())
            || terminal_quote_has_embedded_closer(child, macro_name)
    })
}

pub(super) fn terminal_quote_body_contains_display(body: NodeRef<'_>) -> bool {
    body.children().any(|child| {
        child.kind() == NodeKind::Block && matches!(child.macro_name(), Some("Bd" | "Bl"))
    })
}

/// Render an explicit enclosure whose Body contains a vertical layout block.
/// Flattening a display or list would erase its terminal field boundaries;
/// walking it structurally retains them while a recovered empty quote Body
/// still closes at its authored source position.
pub(super) fn render_terminal_quote_with_display(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let Some((opening, closing)) = terminal_quote_delimiters(node, Some(body), format) else {
        return Ok(());
    };
    let mut leading = String::new();
    for head in node
        .children()
        .filter(|child| child.kind() == NodeKind::Head || child.flags().delimiter_open)
    {
        collect_terminal_text(head, format, limits, &mut leading);
    }
    let opening = render_terminal_font(opening, terminal_inherited_font(node));
    append_terminal_text(
        output,
        &format!("{leading}{opening}"),
        TerminalTextLayout::default(),
        indentation,
        maximum,
    )?;
    append(output, &TERMINAL_ATTACH_NEXT_MARKER.to_string(), maximum)?;
    for child in body.children() {
        render_terminal_node(child, format, limits, indentation, output, maximum)?;
    }
    if !terminal_quote_has_embedded_closer(body, node.macro_name()) {
        append_terminal_text(
            output,
            &render_terminal_font(closing, terminal_inherited_font(node)),
            TerminalTextLayout {
                join: TerminalJoin::Attach,
                ..TerminalTextLayout::default()
            },
            indentation,
            maximum,
        )?;
    }
    for tail in node
        .children()
        .filter(|child| child.kind() == NodeKind::Tail || child.flags().delimiter_close)
    {
        render_terminal_node(tail, format, limits, indentation, output, maximum)?;
    }
    Ok(())
}

/// An `Ed` that terminates while an explicit quote is still open is retained
/// as an empty `Bd` Body below that quote.  The next phrase resumes at the
/// display's enclosing field, not its display offset.
pub(super) fn terminal_embedded_display_closing_indentation(
    node: NodeRef<'_>,
    current_indentation: usize,
) -> Option<usize> {
    if !terminal_is_embedded_display_closer(node) {
        return None;
    }
    let display = node.ancestors().find(|ancestor| {
        ancestor.kind() == NodeKind::Block && ancestor.macro_name() == Some("Bd")
    })?;
    let offset = terminal_mdoc_display_offset(display);
    Some(if offset.is_negative() {
        current_indentation.saturating_add(offset.unsigned_abs())
    } else {
        current_indentation.saturating_sub(offset.unsigned_abs())
    })
}

pub(super) fn terminal_is_embedded_display_closer(node: NodeRef<'_>) -> bool {
    node.kind() == NodeKind::Body
        && node.macro_name() == Some("Bd")
        && node.children().next().is_none()
}

pub(super) fn terminal_embedded_display_closes_quote(node: NodeRef<'_>) -> bool {
    terminal_is_embedded_display_closer(node)
        && node.parent().is_some_and(|body| {
            body.kind() == NodeKind::Body
                && body.parent().is_some_and(|block| {
                    block.kind() == NodeKind::Block
                        && terminal_quote_delimiters(block, None, RenderFormat::Ascii).is_some()
                })
        })
}

pub(super) fn terminal_contains_embedded_display_quote_close(node: NodeRef<'_>) -> bool {
    terminal_embedded_display_closes_quote(node)
        || node
            .children()
            .any(terminal_contains_embedded_display_quote_close)
}

/// Collect an explicit quote Body without flattening a synopsis-pretty `Pp`
/// boundary.  The public AST deliberately exposes the paragraph as an inline
/// mdoc element, while the terminal device starts its next phrase in the
/// name-field continuation column even when an `nS` reset occurs inside the
/// still-open optional enclosure.
pub(super) fn collect_terminal_quote_contents(
    body: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
) {
    for child in body.children() {
        if child.kind() == NodeKind::Block
            && child.macro_name() == Some("Nm")
            && terminal_mdoc_synopsis(child)
        {
            // `Nm` remains a declaration field even below an open optional
            // enclosure. The generic quote collector would otherwise flatten
            // it into the opener's preceding source line and lose both its
            // bold font and SYNOPSIS column.
            output.push('\n');
            output.push(TERMINAL_TEMPORARY_INDENT_MARKER);
            output.push_str(&indentation.to_string());
            output.push(TERMINAL_TEMPORARY_INDENT_MARKER);
            if let Some(head) = child.children().find(|part| part.kind() == NodeKind::Head) {
                collect_terminal_mdoc_synopsis_name_head(head, format, limits, output);
            }
            // Validation can nest the remaining source tail below the Nm
            // Body (for example a `Bk` before an `Oc`). It is still part of
            // the same synopsis declaration field after the bold name.
            for nested_body in child
                .children()
                .filter(|part| part.kind() == NodeKind::Body)
            {
                collect_terminal_text(nested_body, format, limits, output);
            }
        } else if child.macro_name() == Some("Pp") && terminal_mdoc_synopsis_paragraph(child) {
            output.push('\n');
            output.push('\n');
            output.push(TERMINAL_TEMPORARY_INDENT_MARKER);
            output.push_str(&indentation.saturating_add(7).to_string());
            output.push(TERMINAL_TEMPORARY_INDENT_MARKER);
        } else if child.macro_name() == Some("br") {
            // A recovered list closer can survive inside an otherwise-open
            // quote Body as a terminal `br` (for example `Bo … El` followed
            // by a stray `It`).  It resets to the enclosing list Body field;
            // flattening it loses that boundary and joins the stray item to
            // the bracket phrase.
            output.push('\n');
            output.push(TERMINAL_TEMPORARY_INDENT_MARKER);
            output.push_str(&indentation.to_string());
            output.push(TERMINAL_TEMPORARY_INDENT_MARKER);
        } else if let Some(target) =
            terminal_embedded_display_closing_indentation(child, indentation)
        {
            output.push('\n');
            output.push(TERMINAL_TEMPORARY_INDENT_MARKER);
            output.push_str(&target.to_string());
            output.push(TERMINAL_TEMPORARY_INDENT_MARKER);
        } else {
            collect_terminal_text(child, format, limits, output);
        }
    }
}

pub(super) fn is_section_block(node: NodeRef<'_>) -> bool {
    node.kind() == NodeKind::Block && matches!(node.macro_name(), Some("SH" | "SS" | "Sh" | "Ss"))
}

pub(super) fn is_mdoc_description_block(node: NodeRef<'_>) -> bool {
    node.kind() == NodeKind::Block && node.macro_name() == Some("Nd")
}

/// Collect an mdoc section heading with its ordinary words bold, while
/// preserving any explicit semantic font macro as an independent device
/// fragment. Rendering the whole collected phrase bold would apply a second
/// overstrike to an `Em`/`Li`/`Sy` child.
pub(super) fn collect_terminal_mdoc_heading(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    output: &mut String,
) {
    if node.flags().no_print || node.ancestors().any(|ancestor| ancestor.flags().no_print) {
        return;
    }
    if node.kind() == NodeKind::Element
        && let Some(font) = terminal_mdoc_element_font(node)
    {
        let mut phrase = String::new();
        collect_terminal_semantic_text(node, format, limits, font, &mut phrase);
        if !phrase.is_empty() {
            terminal_append_heading_separator(output, &phrase);
            output.push_str(&phrase);
        }
        return;
    }
    if let Some(text) = node.text() {
        let phrase = render_terminal_bold(
            &render_terminal_visible_text_with_font(text, format, limits, TerminalFont::Roman),
            format,
        );
        terminal_append_heading_separator(output, &phrase);
        output.push_str(&phrase);
    }
    for child in node.children() {
        collect_terminal_mdoc_heading(child, format, limits, output);
    }
}

pub(super) fn terminal_append_heading_separator(output: &mut String, phrase: &str) {
    if !output.is_empty()
        && !output.ends_with([' ', '(', '[', '{', '<'])
        && !phrase.starts_with([')', ']', '}', '>', ',', '.', ';', ':', '!', '?'])
    {
        output.push(' ');
    }
}

/// Empty mdoc sections are a terminal heading transition rather than a full
/// vertical region.  The next heading follows on the immediately next line;
/// its ordinary section gap would incorrectly introduce a blank line.
pub(super) fn terminal_previous_empty_section(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let previous = parent
        .children()
        .take_while(|child| child.id() != node.id())
        .last();
    previous.is_some_and(|previous| {
        is_section_block(previous)
            && previous
                .children()
                .find(|child| child.kind() == NodeKind::Body)
                .is_some_and(|body| {
                    !terminal_has_visible_text(body, format, limits)
                        && !terminal_has_visible_table(body)
                })
    })
}

pub(super) fn terminal_has_visible_table(node: NodeRef<'_>) -> bool {
    (node.kind() == NodeKind::Table && !node.table_cells().is_empty())
        || node.children().any(terminal_has_visible_table)
}

pub(super) fn terminal_man_paragraph_density(node: NodeRef<'_>) -> Option<usize> {
    let mut density = None;
    let mut root = node;
    while let Some(parent) = root.parent() {
        root = parent;
    }
    terminal_last_pd_before(root, node.id(), &mut density);
    density
}

/// Visit source-ordered syntax up to `target`, retaining man(7)'s most recent
/// paragraph-distance request.  The structural pass may attach `PD` to the
/// preceding paragraph Body, a pending next-line Head, or the surrounding
/// Body, so direct-sibling lookup is not sufficient.
pub(super) fn terminal_last_pd_before(
    node: NodeRef<'_>,
    target: crate::NodeId,
    density: &mut Option<usize>,
) -> bool {
    if node.id() == target {
        return true;
    }
    if node.macro_name() == Some("PD") {
        match terminal_first_text(node) {
            None => *density = Some(1),
            Some(value) => {
                if let Some(value) = terminal_vertical_span(value) {
                    *density = Some(value.max(0).unsigned_abs());
                }
            }
        }
    }
    node.children()
        .any(|child| terminal_last_pd_before(child, target, density))
}

/// Return the first textual scanner argument below a stateful man request.
///
/// Recoverable blocks such as `PD` retain their argument below a Head rather
/// than directly on the Block, while their well-formed Element counterpart
/// can expose text one level sooner.  The terminal state machine consumes the
/// first argument in either shape.
pub(super) fn terminal_first_text(node: NodeRef<'_>) -> Option<&str> {
    node.text()
        .or_else(|| node.children().find_map(terminal_first_text))
}

pub(super) fn terminal_has_visible_predecessor(node: NodeRef<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    parent
        .children()
        .take_while(|sibling| sibling.id() != node.id())
        .any(|sibling| {
            // `PD` selects future paragraph density and an initial `.sp`
            // has no device effect before any visible field. Neither is a
            // predecessor capable of making a following section-leading
            // `PP` manufacture a vertical gap.
            !matches!(sibling.macro_name(), Some("PD" | "sp")) && !sibling.flags().no_print
        })
}

/// `term_vspace()` is additive, even when a transparent anchor separates the
/// two source requests.  Parser structure can put the preceding `.sp` either
/// beside the next request or at the end of the previous paragraph Body, so
/// recover that device-level predecessor before an mdoc `Pp` or man `PP`
/// asks for its own vertical slot.
pub(super) fn append_terminal_following_vertical_slot(
    node: NodeRef<'_>,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if terminal_follows_vertical_space(node) && output.ends_with("\n\n") {
        append(output, "\n", maximum)?;
    }
    Ok(())
}

pub(super) fn terminal_follows_vertical_space(node: NodeRef<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    parent
        .children()
        .take_while(|sibling| sibling.id() != node.id())
        // `Tg` creates an anchor but has no terminal-device presentation, so
        // it cannot consume an adjacent vertical slot.
        .filter(|sibling| sibling.macro_name() != Some("Tg") && !sibling.flags().no_print)
        .last()
        .is_some_and(terminal_ends_with_vertical_space)
}

pub(super) fn terminal_ends_with_vertical_space(node: NodeRef<'_>) -> bool {
    if node.macro_name() == Some("sp") {
        return true;
    }
    node.children()
        .rfind(|child| !child.flags().no_print)
        .is_some_and(terminal_ends_with_vertical_space)
}

/// Identify the source blank which man validation consumes before an initial
/// field block below a section heading. The parser retains it only as private
/// terminal provenance so public canonical ASTs stay legacy-compatible.
pub(super) fn terminal_follows_empty_section_paragraph(node: NodeRef<'_>) -> bool {
    node.terminal_suppressed_leading_blank()
}

pub(super) fn terminal_man_ip_is_in_rs_body(node: NodeRef<'_>) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == NodeKind::Body && parent.macro_name() == Some("RS"))
}

pub(super) fn terminal_man_rs_follows_empty_hanging_paragraph(node: NodeRef<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    parent
        .children()
        .take_while(|sibling| sibling.id() != node.id())
        .last()
        .is_some_and(|previous| {
            previous.kind() == NodeKind::Block
                && previous.macro_name() == Some("HP")
                && previous
                    .children()
                    .find(|child| child.kind() == NodeKind::Body)
                    .is_some_and(|body| body.children().all(|child| child.flags().no_print))
        })
}

/// Whether a recovered line break immediately follows a completed man field.
///
/// Valid `.br` requests occurring inside an `IP`/`TP`/`HP` remain children of
/// that field's Body.  In contrast, a `.RE` with no open `RS` closes the
/// field at the structural layer and becomes this direct sibling.  The latter
/// consumes the field's trailing paragraph slot in the terminal device.
pub(super) fn terminal_man_field_sibling_break(node: NodeRef<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if !matches!(parent.macro_name(), Some("SH" | "SS")) {
        return false;
    }
    parent
        .children()
        .take_while(|sibling| sibling.id() != node.id())
        .last()
        .is_some_and(|previous| {
            previous.kind() == NodeKind::Block
                && matches!(previous.macro_name(), Some("IP" | "TP" | "HP"))
        })
}

/// Recover man(7)'s shared `lmargin` register for field macros.
///
/// `IP`, `TP`, and `HP` all read and (when supplied a valid dimensional
/// argument) update the same terminal register.  A `PP` block and section
/// boundaries reset it to the device default; the latter naturally receives a
/// different Body parent, so this source-order sibling walk only needs the
/// former explicit reset marker.
pub(super) fn terminal_man_field_width(node: NodeRef<'_>) -> isize {
    if let Some(width) = terminal_man_explicit_field_width(node) {
        return width;
    }
    let Some(parent) = node.parent() else {
        return 7;
    };
    let preceding = parent
        .children()
        .take_while(|sibling| sibling.id() != node.id())
        .collect::<Vec<_>>();
    for sibling in preceding.into_iter().rev() {
        if sibling.macro_name() == Some("PP") {
            break;
        }
        if matches!(sibling.macro_name(), Some("IP" | "TP" | "HP"))
            && let Some(width) = terminal_man_explicit_field_width(sibling)
        {
            return width;
        }
    }
    7
}

pub(super) fn terminal_man_explicit_field_width(node: NodeRef<'_>) -> Option<isize> {
    let head = node
        .children()
        .find(|child| child.kind() == NodeKind::Head)?;
    match node.macro_name() {
        // IP has a visible tag before its optional width.
        Some("IP") => head
            .children()
            .nth(1)
            .and_then(NodeRef::text)
            .and_then(terminal_signed_roff_en_prefix),
        // TP and HP take their layout width as the Head's first same-line
        // scanner argument.  A next-line TP term such as `20n` is visible
        // text, not an update to the field register.
        Some("TP" | "HP") => head
            .children()
            .next()
            .filter(|argument| !argument.flags().line_start)
            .and_then(NodeRef::text)
            .and_then(terminal_signed_layout_units),
        _ => None,
    }
}

/// Render mdoc's `Bl -item` form from its `It` bodies.  Unlike definition and
/// tagged lists, the `It` head is syntactic input rather than visible content.
/// Its compact flag controls only the boundary between sibling items.
pub(super) fn render_terminal_plain_list(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let compact = node.compact();
    let list_indentation = terminal_mdoc_list_indentation(node, indentation);
    if terminal_has_visible_predecessor(node) && !compact {
        append_blank_line(output, maximum)?;
    } else if !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    let mut first = true;
    for item in body
        .children()
        .filter(|child| child.kind() == NodeKind::Block && child.macro_name() == Some("It"))
    {
        let Some(item_body) = item.children().find(|child| child.kind() == NodeKind::Body) else {
            continue;
        };
        if !first {
            if compact {
                if !output.is_empty() && !output.ends_with('\n') {
                    append(output, "\n", maximum)?;
                }
            } else {
                append_blank_line(output, maximum)?;
            }
        }
        for child in item_body.children() {
            render_terminal_node(child, format, limits, list_indentation, output, maximum)?;
        }
        first = false;
    }
    // A populated item list is a terminal field.  Its following outer-flow
    // sibling therefore begins a new device line even when the final item
    // consists solely of recovery-visible text after a bare `Ta`.
    if !first && !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Render mdoc's `Bl -column` rows as fixed terminal fields.
///
/// `Bl -column` is neither an ordinary hanging list nor a tbl node: each
/// `It` owns one Body per `Ta`-delimited cell, while the list declaration
/// phrases determine the field widths.  Those phrases are private arena
/// provenance because the legacy public AST discards them.  Mandoc leaves four
/// cells between declared fields and appends excess cells directly after the
/// final declared field; keeping the resulting line spacing protected avoids
/// ordinary prose wrapping collapsing the table geometry.
pub(super) fn render_terminal_column_list(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let declared_widths = node
        .column_widths()
        .map(|declaration| {
            let rendered = render_terminal_visible_text_with_font(
                declaration,
                format,
                limits,
                terminal_inherited_font(node),
            );
            display_width(&rendered)
        })
        .collect::<Vec<_>>();
    let list_indentation = terminal_mdoc_list_indentation(node, indentation);
    if terminal_has_visible_predecessor(node) && !node.compact() {
        append_blank_line(output, maximum)?;
    } else if !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    let mut table_precedes_next_item = false;
    for child in body.children() {
        if child.kind() == NodeKind::Table && !child.table_cells().is_empty() {
            // tbl rows are direct Body siblings when they occur between mdoc
            // column-list items.  They must stay structural: flattening them
            // through the column-cell collector erases every generated row.
            render_terminal_table(child, format, limits, list_indentation, output, maximum)?;
            table_precedes_next_item = true;
            continue;
        }
        if child.kind() != NodeKind::Block || child.macro_name() != Some("It") {
            continue;
        }
        if table_precedes_next_item {
            append_blank_line(output, maximum)?;
        }
        let table_rows = child
            .children()
            .filter(|cell| cell.kind() == NodeKind::Body)
            .flat_map(NodeRef::children)
            .filter(|row| row.kind() == NodeKind::Table && !row.table_cells().is_empty())
            .collect::<Vec<_>>();
        if !table_rows.is_empty() {
            // The mdoc parser wraps a tbl range in an otherwise empty `It`
            // when it occurs between ordinary column-list rows.  The public
            // compatible tree keeps that wrapper, but terminal layout must
            // render its Table children as a contiguous tbl range.
            for row in table_rows {
                render_terminal_table(row, format, limits, list_indentation, output, maximum)?;
            }
            table_precedes_next_item = true;
            continue;
        }
        let mut structural_tail = Vec::new();
        let cells = child
            .children()
            .filter(|cell| cell.kind() == NodeKind::Body)
            .map(|cell| {
                let children = cell.children().collect::<Vec<_>>();
                let structural_start = terminal_definition_body_structural_tail_start(&children)
                    .unwrap_or(children.len());
                if structural_tail.is_empty() && structural_start < children.len() {
                    structural_tail.extend_from_slice(&children[structural_start..]);
                }
                let mut text = String::new();
                for child in &children[..structural_start] {
                    collect_terminal_column_cell_text(*child, format, limits, &mut text);
                }
                text
            })
            .collect::<Vec<_>>();
        if cells.iter().all(String::is_empty) {
            continue;
        }
        if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        append(output, &TERMINAL_KEEP_SPACING_MARKER.to_string(), maximum)?;
        append(output, &" ".repeat(list_indentation), maximum)?;
        for (index, cell) in cells.iter().enumerate() {
            let visible = cell.trim_end();
            append(output, visible, maximum)?;
            if index + 1 < cells.len()
                && let Some(width) = declared_widths.get(index)
            {
                // `term.c` leaves four device cells between up to four
                // columns. Its five-column layout reserves one of those
                // cells for the extra field, leaving a three-cell gap.
                let column_gap = if declared_widths.len() >= 5 { 3 } else { 4 };
                // Compute against the complete next-field target rather
                // than saturating the declaration width first: a source
                // phrase one cell wider than its label still consumes one of
                // the four inter-column cells instead of shifting every
                // following column right.
                let padding = width
                    .saturating_add(column_gap)
                    .saturating_sub(display_width(visible));
                append(output, &" ".repeat(padding), maximum)?;
            }
        }
        append(output, "\n", maximum)?;
        // A column cell can recover into a nested display/list after its
        // visible field text.  The compatible AST deliberately keeps both
        // beneath the same It Body, but treating the structural tail as cell
        // prose flattens its vertical field and loses its display offset.
        // Render it only after committing the row, at the column-list field.
        for tail in &structural_tail {
            render_terminal_node(*tail, format, limits, list_indentation, output, maximum)?;
        }
        // Each empty `Body(Bl)` retained below the structural tail is a
        // scanner-recovered list closer.  The native device finishes that
        // recovered field after the enclosing display has emitted its own
        // source tail, rather than flattening the closer where it appears in
        // the compatibility tree.  Keep those vertical slots cumulative: a
        // pair of nested closers is observably two slots before the following
        // outer section.
        for _ in 0..terminal_recovered_list_closer_count(&structural_tail) {
            if !output.is_empty() {
                append(output, "\n", maximum)?;
            }
        }
        table_precedes_next_item = false;
    }
    Ok(())
}

/// Collect one column-list cell node without discarding a scanner-retained
/// empty phrase. A tab followed by source whitespace can become an empty Text
/// node before a semantic mdoc macro; the terminal still advances one cell
/// before that macro's visible expansion. Ordinary prose deliberately
/// suppresses such placeholders, so keep this narrowly within `Bl -column`
/// layout.
pub(super) fn collect_terminal_column_cell_text(
    child: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    output: &mut String,
) {
    if child.kind() == NodeKind::Text && child.text() == Some("") {
        output.push(' ');
    } else if child.kind() == NodeKind::Text && child.text() == Some(r"\&") {
        // A zero-width no-break escape at the end of a tab-created cell
        // carries the following physical source phrase in the same cell.
        // `term.c` retains its one-cell field separation there even though
        // the escape itself has no glyph.
        output.push(' ');
    } else {
        collect_terminal_text(child, format, limits, output);
    }
}

pub(super) fn terminal_recovered_list_closer_count(nodes: &[NodeRef<'_>]) -> usize {
    fn count(node: NodeRef<'_>) -> usize {
        usize::from(
            node.kind() == NodeKind::Body
                && node.macro_name() == Some("Bl")
                && node.children().next().is_none(),
        ) + node.children().map(count).sum::<usize>()
    }

    nodes.iter().copied().map(count).sum()
}

pub(super) mod table;
use table::render_terminal_table;

/// Whether an mdoc list has no semantic item Blocks.
pub(super) fn terminal_mdoc_list_is_empty(node: NodeRef<'_>) -> bool {
    node.children()
        .find(|child| child.kind() == NodeKind::Body)
        .is_none_or(|body| {
            !body
                .children()
                .any(|child| child.kind() == NodeKind::Block && child.macro_name() == Some("It"))
        })
}

/// Render mdoc's marker-bearing `Bl` variants without widening the legacy
/// normalized list API. The parser retains their source spelling privately:
/// bullet, dash, and hyphen markers are bold, while enum counts from one.
/// All reserve the terminal device's five-cell marker field.
pub(super) fn render_terminal_marked_list(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let Some(marker) = node.list_marker() else {
        // A recovery-created list can be normalized without a source
        // selector. Its legacy-compatible fallback is marker-free flow.
        return render_terminal_plain_list(node, format, limits, indentation, output, maximum);
    };
    let compact = node.compact();
    let marker_indentation = terminal_mdoc_list_indentation(node, indentation);
    // `termp_it_pre()` starts marker-list Bodies at the declared width plus
    // groff's two-cell buffer.  Negative and narrow widths still leave one
    // separator after the marker but make wrapped lines outdent accordingly.
    let explicit_body_field_width = node
        .width()
        .and_then(terminal_signed_layout_units)
        .map(|width| width.saturating_add(2));
    let body_field_width = explicit_body_field_width.unwrap_or(5);
    let body_indentation = if body_field_width.is_negative() {
        marker_indentation.saturating_sub(body_field_width.unsigned_abs())
    } else {
        marker_indentation.saturating_add(body_field_width.unsigned_abs())
    };
    if terminal_has_visible_predecessor(node) && !compact {
        append_blank_line(output, maximum)?;
    } else if !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    let mut ordinal = 1_usize;
    let mut first = true;
    for item in body
        .children()
        .filter(|child| child.kind() == NodeKind::Block && child.macro_name() == Some("It"))
    {
        if !first {
            if compact {
                if !output.is_empty() && !output.ends_with('\n') {
                    append(output, "\n", maximum)?;
                }
            } else {
                append_blank_line(output, maximum)?;
            }
        }
        let visible_marker = match marker {
            // The historical terminal device draws the bullet as a plus and
            // circle overstruck at the same column, not as two separately
            // bold glyphs.  Keep its byte-for-byte backspace sequence.
            MdocListMarker::Bullet => "+\u{8}+\u{8}o\u{8}o".to_owned(),
            MdocListMarker::Dash | MdocListMarker::Hyphen => render_terminal_bold("-", format),
            MdocListMarker::Enum => format!("{ordinal}."),
        };
        append_terminal_hanging_indent(output, body_indentation, maximum)?;
        append_terminal_text(
            output,
            &visible_marker,
            TerminalTextLayout {
                line_start: true,
                // An enum's dot is a terminal list marker, not prose that
                // should request the next word's double sentence spacing.
                literal_punctuation: matches!(marker, MdocListMarker::Enum),
                ..TerminalTextLayout::default()
            },
            marker_indentation,
            maximum,
        )?;
        if let Some(item_body) = item.children().find(|child| child.kind() == NodeKind::Body)
            && item_body.children().any(|child| !child.flags().no_print)
        {
            let leading_list = item_body
                .children()
                .find(|child| !child.flags().no_print)
                .filter(|child| child.macro_name() == Some("Bl"));
            if let Some(list) = leading_list {
                // A marker whose Body opens directly with another list owns
                // its own otherwise-empty device field.  Do not leave the
                // ordinary marker-to-prose padding behind it: a non-compact
                // nested list starts after the field's vertical slot, while
                // a compact one merely starts on the next physical line.
                if list.compact() {
                    append(output, "\n", maximum)?;
                } else {
                    append_blank_line(output, maximum)?;
                }
            } else {
                let field_gap = explicit_body_field_width.map_or(3, |width| {
                    width
                        .saturating_sub_unsigned(display_width(&visible_marker))
                        .max(1)
                        .unsigned_abs()
                });
                // Keep all but the final field separator non-breaking until the
                // width pass.  It can then wrap prose at the Body field without
                // collapsing the marker's explicitly padded first line.
                let protected_padding = TERMINAL_NONBREAKING_SPACE_MARKER
                    .to_string()
                    .repeat(field_gap.saturating_sub(1));
                append(output, &protected_padding, maximum)?;
            }
            for child in item_body.children() {
                render_terminal_node(child, format, limits, body_indentation, output, maximum)?;
            }
        }
        ordinal = ordinal.saturating_add(1);
        first = false;
    }
    if !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Render man(7)'s `TP` as a tagged paragraph.  The leading `n`/`i` width is
/// scanner input kept below the public tag, while the following physical line
/// is the visible term.  The body position is relative to its containing
/// section and deliberately accepts negative widths, matching the terminal
/// device's leftward outdent behaviour.
pub(super) fn render_terminal_man_tagged_paragraph(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(head) = node.children().find(|child| child.kind() == NodeKind::Head) else {
        return Ok(());
    };
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };

    // A bare TP starts at the terminal's seven-cell default. A width applies
    // only when it is on this TP request's source line, so it remains below
    // the public tag rather than a persistent semantic AST property.
    let mut width = terminal_man_field_width(node);
    let mut tag = String::new();
    let mut tag_indentation = indentation;
    let mut children = head.children();
    if let Some(first) = children.next() {
        if !first.flags().line_start
            && let Some(parsed_width) = first.text().and_then(terminal_signed_layout_units)
        {
            width = parsed_width;
        } else if first.flags().line_start {
            // With no same-line width argument, the first Head child is the
            // physical next-line term. Invalid same-line widths are not.
            collect_terminal_text(first, format, limits, &mut tag);
        }
    }
    // `pre_TP()` consumes one same-line width argument only.  Subsequent
    // malformed scanner arguments remain in the public recovery tree, but
    // term.c skips them while looking for the next physical-line tag.  An
    // `.in` request can appear while that Head is open; it changes only the
    // tag's left edge, not the Body field established by TP's width.
    for child in children.filter(|child| child.flags().line_start) {
        if child.macro_name() == Some("in")
            && let Some(value) = terminal_first_text(child)
            && let Some(next) = terminal_man_in_target(value, tag_indentation)
        {
            tag_indentation = next;
        } else {
            collect_terminal_text(child, format, limits, &mut tag);
        }
    }
    if tag.is_empty() {
        let Some(raw_tag) = head.tag() else {
            return Ok(());
        };
        tag = render_terminal_visible_text(raw_tag, format, limits);
    }
    // Like IP, TP's Head is a terminal field rather than literal display
    // text. Escaped trailing blanks reserve field cells (and may move the
    // Body below a long term) but must not themselves print at line end.
    let logical_tag_end = tag_indentation.saturating_add(display_width(&tag));
    tag = tag
        .trim_end_matches(|character: char| {
            character.is_whitespace() || character == TERMINAL_NONBREAKING_SPACE_MARKER
        })
        .to_owned();
    let visible_tag_end = tag_indentation.saturating_add(display_width(&tag));

    let body_indentation = if width.is_negative() {
        indentation.saturating_sub(width.unsigned_abs())
    } else {
        indentation.saturating_add(width.unsigned_abs())
    };
    let body_has_visible_text = terminal_has_visible_text(body, format, limits);
    let body_starts_with_terminal_break = terminal_body_starts_with_break(body);
    let first_body = body.children().find(|child| !child.flags().no_print);
    let first_body_is_no_fill =
        first_body.is_some_and(|child| child.flags().no_fill && child.flags().line_start);

    let density = terminal_man_paragraph_density(node);
    if !terminal_follows_empty_section_paragraph(node)
        && (density.is_none() || terminal_has_visible_predecessor(node))
    {
        if density == Some(0) {
            if !output.is_empty() && !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
        } else {
            append_blank_line(output, maximum)?;
            for _ in 1..density.unwrap_or(1) {
                append(output, "\n", maximum)?;
            }
        }
    }
    let inline_body = !tag.is_empty()
        && body_has_visible_text
        && !body_starts_with_terminal_break
        && body_indentation > logical_tag_end;
    if inline_body {
        // Once a short term shares its field with filled Body text, every
        // wrap continuation belongs to the Body column. A long term starts
        // its Body on a fresh field line instead, so it deliberately retains
        // the tag's own indentation while wrapping.
        append_terminal_hanging_indent(output, body_indentation, maximum)?;
    }
    append_terminal_text(
        output,
        &tag,
        TerminalTextLayout {
            line_start: true,
            // A TP tag is normal fill-mode text. Preserve authored internal
            // spacing only; forcing all tags to no-fill leaves long terms
            // beyond the terminal margin instead of wrapping them.
            keep_spacing: tag.contains('\t')
                || tag.contains("  ")
                || body_indentation > DEFAULT_RENDER_WIDTH,
            ..TerminalTextLayout::default()
        },
        tag_indentation,
        maximum,
    )?;

    // The first body line shares the term's field even when it is no-fill;
    // only *subsequent* no-fill source lines own new physical lines.  This is
    // why a `.TP` opened inside `.nf` displays `term     first line` rather
    // than an empty term line followed by an indented body.
    if inline_body {
        append(
            output,
            &TERMINAL_NONBREAKING_SPACE_MARKER
                .to_string()
                // `append_terminal_text()` contributes the field's ordinary
                // joining cell before the first Body phrase. Protect the
                // remaining padding so fill-mode wrapping cannot collapse
                // the TP column to a single blank.
                .repeat((body_indentation - visible_tag_end).saturating_sub(1)),
            maximum,
        )?;
    } else {
        append(output, "\n", maximum)?;
    }
    let mut consumed_first_no_fill = None;
    if first_body_is_no_fill
        && let Some(first) = first_body
        && let Some(text) = first.text()
    {
        let rendered = render_terminal_visible_text_with_font(
            text.trim_end(),
            format,
            limits,
            terminal_inherited_font(first),
        );
        append_terminal_text(
            output,
            &rendered,
            TerminalTextLayout {
                // The tagged field has already supplied the first line's
                // physical placement; retain no-fill only for wrapping and
                // for subsequent source-line boundaries.
                no_fill: !rendered.is_empty(),
                keep_spacing: first.separator_width() > 1 || rendered.contains("  "),
                ..TerminalTextLayout::default()
            },
            body_indentation,
            maximum,
        )?;
        consumed_first_no_fill = Some(first.id());
    }
    for child in body.children() {
        if Some(child.id()) == consumed_first_no_fill {
            continue;
        }
        render_terminal_node(child, format, limits, body_indentation, output, maximum)?;
    }
    Ok(())
}

/// Render man(7)'s `HP` as a hanging paragraph: its first terminal line keeps
/// the enclosing section field and all wraps/explicit body breaks use the
/// signed Head width. The Head is a layout request, never visible prose.
pub(super) fn render_terminal_man_hanging_paragraph(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let width = terminal_man_field_width(node);
    let continuation_indentation = if width.is_negative() {
        indentation.saturating_sub(width.unsigned_abs())
    } else {
        indentation.saturating_add(width.unsigned_abs())
    };
    let density = terminal_man_paragraph_density(node);
    // A first HP immediately below a section Head owns no extra paragraph
    // gap. Once visible filled flow has begun, it follows normal man
    // paragraph-density spacing.
    if !output.is_empty() && !output.ends_with('\n') {
        if density == Some(0) {
            append(output, "\n", maximum)?;
        } else {
            append_blank_line(output, maximum)?;
            for _ in 1..density.unwrap_or(1) {
                append(output, "\n", maximum)?;
            }
        }
    }
    let mut children = body.children().filter(|child| !child.flags().no_print);
    let Some(first) = children.next() else {
        return Ok(());
    };
    append_terminal_hanging_indent(output, continuation_indentation, maximum)?;
    render_terminal_node(first, format, limits, indentation, output, maximum)?;
    for child in children {
        if child.macro_name() == Some("fi") && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        render_terminal_node(
            child,
            format,
            limits,
            continuation_indentation,
            output,
            maximum,
        )?;
    }
    Ok(())
}

/// Render mdoc's `Fo`/`Fc` declaration block without flattening the Head and
/// Body in the public tree.  The terminal device makes the Head bold, formats
/// each `Fa` argument in italic with an attached comma, and gives SYNOPSIS
/// declarations their own completed line and trailing semicolon.
pub(super) fn render_terminal_mdoc_function_block(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let mut function = String::new();
    if let Some(head) = node.children().find(|child| child.kind() == NodeKind::Head) {
        collect_terminal_semantic_text(head, format, limits, TerminalFont::Bold, &mut function);
    }
    let mut arguments = Vec::new();
    if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
        for child in body.children().filter(|child| !child.flags().no_print) {
            // `Tg` contributes a navigation target to the AST but is
            // transparent to the terminal prototype.  Its text otherwise
            // duplicates the adjacent `Fa` argument.
            if child.macro_name() == Some("Tg") {
                continue;
            }
            if child.macro_name() == Some("Fa") {
                for argument in child.children() {
                    let mut rendered = String::new();
                    collect_terminal_semantic_text(
                        argument,
                        format,
                        limits,
                        TerminalFont::Italic,
                        &mut rendered,
                    );
                    if !rendered.is_empty() {
                        // `termp_fa_pre()` sets `TERMP_NBRWORD` for every
                        // `Fa` argument, not just in SYNOPSIS.  A multiword
                        // type/name phrase therefore moves as one field
                        // after its comma instead of splitting at its
                        // internal authored space.
                        arguments.push(
                            rendered.replace(' ', &TERMINAL_NONBREAKING_SPACE_MARKER.to_string()),
                        );
                    }
                }
            } else if child.macro_name() == Some("Nm") {
                // A recovered synopsis `Nm` can occur as the only argument
                // of a still-open `Fo`. It retains its normal bold device
                // presentation rather than becoming a generic italic
                // function argument.
                let mut rendered = String::new();
                collect_terminal_semantic_text(
                    child,
                    format,
                    limits,
                    TerminalFont::Bold,
                    &mut rendered,
                );
                if !rendered.is_empty() {
                    arguments.push(rendered);
                }
            } else {
                let mut rendered = String::new();
                collect_terminal_semantic_text(
                    child,
                    format,
                    limits,
                    TerminalFont::Italic,
                    &mut rendered,
                );
                if !rendered.is_empty() {
                    arguments.push(rendered);
                }
            }
        }
    }
    render_terminal_mdoc_function_signature(
        node,
        &function,
        &arguments,
        indentation,
        output,
        maximum,
    )
}

/// Render mdoc's one-request function form (`Fn`) using the same terminal
/// semantics as an `Fo` block: first argument is the bold function name and
/// the rest are italic comma-separated prototype arguments.
pub(super) fn render_terminal_mdoc_function_element(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let mut children = node.children();
    let mut function = String::new();
    if let Some(name) = children.next() {
        collect_terminal_semantic_text(name, format, limits, TerminalFont::Bold, &mut function);
    }
    let mut arguments = Vec::new();
    for argument in children {
        let mut rendered = String::new();
        collect_terminal_semantic_text(
            argument,
            format,
            limits,
            TerminalFont::Italic,
            &mut rendered,
        );
        if !rendered.is_empty() {
            arguments.push(rendered);
        }
    }
    render_terminal_mdoc_function_signature(
        node,
        &function,
        &arguments,
        indentation,
        output,
        maximum,
    )
}

pub(super) fn render_terminal_mdoc_function_signature(
    node: NodeRef<'_>,
    function: &str,
    arguments: &[String],
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let synopsis = terminal_mdoc_synopsis(node);
    if synopsis {
        terminal_mdoc_synopsis_spacing(node, output, maximum)?;
    }
    // A function argument is parsed as one mdoc argument phrase even when it
    // contains several visible words. The terminal device therefore moves a
    // whole phrase after a comma to its hanging field instead of splitting a
    // type from its parameter name.
    let nonbreaking_space = TERMINAL_NONBREAKING_SPACE_MARKER.to_string();
    let arguments = arguments
        .iter()
        .map(|argument| {
            if synopsis {
                argument.replace(' ', &nonbreaking_space)
            } else {
                argument.clone()
            }
        })
        .collect::<Vec<_>>();
    // Within a Bk body the terminal has entered `TERMP_KEEP` immediately
    // after emitting the function name.  Thus the separator from one
    // comma-terminated argument to the next is nonbreaking, while spaces
    // authored inside a plain Fn argument retain their ordinary break point.
    // This lets an overfull signature backtrack to the last authored space
    // instead of peeling a later argument onto its own line.
    let argument_separator = if terminal_mdoc_word_keep_scope(node) {
        format!(",{TERMINAL_NONBREAKING_SPACE_MARKER}")
    } else {
        ", ".to_owned()
    };
    let signature = format!(
        "{function}({}){}",
        arguments.join(&argument_separator),
        if synopsis { ";" } else { "" }
    );
    // `termp_fn_pre()` retains a four-cell continuation field below a
    // function starting a device line. Inline description prototypes retain
    // their surrounding field instead, so their marker cannot be injected
    // halfway through an existing output line.
    if synopsis && (output.is_empty() || output.ends_with('\n')) {
        append_terminal_hanging_indent(output, indentation.saturating_add(4), maximum)?;
    }
    append_terminal_text(
        output,
        &signature,
        TerminalTextLayout {
            join: if function.is_empty() {
                TerminalJoin::Attach
            } else {
                TerminalJoin::Separate
            },
            ..TerminalTextLayout::default()
        },
        indentation,
        maximum,
    )?;
    if synopsis && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Render the old-style mdoc header declaration.  Unlike the other bold
/// inline macros, `Fd` always completes a terminal line; in SYNOPSIS it also
/// participates in the declaration-group spacing rule shared with functions
/// and types.
pub(super) fn render_terminal_mdoc_include_declaration(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if terminal_mdoc_synopsis(node) {
        terminal_mdoc_synopsis_spacing(node, output, maximum)?;
    }
    let mut contents = String::new();
    collect_terminal_semantic_text(node, format, limits, TerminalFont::Bold, &mut contents);
    append_terminal_text(
        output,
        &contents,
        TerminalTextLayout::default(),
        indentation,
        maximum,
    )?;
    if !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Render mdoc's semantic include-file macro.  It is a bold complete C
/// include phrase in SYNOPSIS, but a roman-bracketed italic file name in
/// prose.  Like the terminal device, only adjacent SYNOPSIS `In` elements
/// introduce a physical line boundary; the macro itself does not.
pub(super) fn render_terminal_mdoc_include_file(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let synopsis = terminal_mdoc_synopsis(node);
    if synopsis {
        terminal_mdoc_synopsis_spacing(node, output, maximum)?;
    }
    let mut contents = String::new();
    let font = if synopsis {
        TerminalFont::Bold
    } else {
        TerminalFont::Italic
    };
    collect_terminal_semantic_text(node, format, limits, font, &mut contents);
    let rendered = if synopsis {
        format!(
            "{}{}{}",
            render_terminal_bold("#include <", format),
            contents,
            render_terminal_bold(">", format)
        )
    } else {
        format!("<{contents}>")
    };
    append_terminal_text(
        output,
        &rendered,
        TerminalTextLayout {
            join: TerminalJoin::Separate,
            ..TerminalTextLayout::default()
        },
        indentation,
        maximum,
    )
}

/// Render mdoc's exceptional explicit enclosure (`Eo`/`Ec`). Unlike the
/// other quote blocks it carries authored Head and Tail delimiters, and an
/// entirely empty pair still counts as a zero-width terminal word.
pub(super) fn render_terminal_explicit_enclosure(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let mut tail = None;
    let mut has_head_or_body = false;
    for child in node.children() {
        match child.kind() {
            NodeKind::Head => {
                has_head_or_body |= terminal_has_visible_text(child, format, limits);
                for nested in child.children() {
                    render_terminal_node(nested, format, limits, indentation, output, maximum)?;
                }
                // The Head supplies Eo's opening delimiter.  It is an
                // explicit enclosure boundary, so attach the following Body
                // rather than allowing normal prose layout to insert a space.
                if terminal_has_visible_text(child, format, limits) {
                    mark_terminal_attach_next(output, maximum)?;
                }
            }
            NodeKind::Body => {
                has_head_or_body |= terminal_has_visible_text(child, format, limits);
                for nested in child.children() {
                    render_terminal_node(nested, format, limits, indentation, output, maximum)?;
                }
            }
            NodeKind::Tail => tail = Some(child),
            _ => {}
        }
    }
    let has_tail = tail.is_some_and(|tail| terminal_has_visible_text(tail, format, limits));
    if let Some(tail) = tail.filter(|_| has_tail) {
        if has_head_or_body {
            mark_terminal_attach_next(output, maximum)?;
        }
        for nested in tail.children() {
            render_terminal_node(nested, format, limits, indentation, output, maximum)?;
        }
    } else if has_head_or_body {
        // An opening-only Eo must not leak the opening delimiter's parser
        // attachment into the first normal sibling after the block.
        if output.ends_with(TERMINAL_ATTACH_NEXT_MARKER) {
            let _ = output.pop();
        }
        append(
            output,
            &TERMINAL_FORCE_SEPARATOR_MARKER.to_string(),
            maximum,
        )?;
    } else {
        append_terminal_empty_word(output, indentation, maximum)?;
    }
    Ok(())
}

/// Render an mdoc `Bl -tag` list from the semantic `It` Head/Body pairs.
///
/// Macro-name widths arrive normalized to fixed terminal `n` units, while an
/// authored roff scale is retained for the public AST.  The formatter turns
/// both forms into the terminal field geometry used by `a2width(3)`.
pub(super) fn render_terminal_definition_list(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let list_indentation = terminal_mdoc_list_indentation(node, indentation);
    // `termp_it_pre()` uses the declared signed `-width` plus its two-cell
    // terminal buffer.  A negative field deliberately outdents the Body;
    // treating it as an unsigned fallback loses the first half of the
    // mdoc tag-list geometry.
    let field_width = node
        .width()
        .map_or(8, |width| terminal_mdoc_a2width(width).saturating_add(2));
    let hanging_list = node.terminal_hanging_list();
    let overhanging_list = node.terminal_overhanging_list();
    let inset_list = node.terminal_inset_list();
    let diagnostic_list = node.terminal_diagnostic_list();
    let body_indentation = if field_width.is_negative() {
        list_indentation.saturating_sub(field_width.unsigned_abs())
    } else {
        list_indentation.saturating_add(field_width.unsigned_abs())
    };
    if terminal_has_visible_predecessor(node) && !node.compact() {
        append_blank_line(output, maximum)?;
    } else if !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    let mut first = true;
    for item in body
        .children()
        .filter(|child| child.kind() == NodeKind::Block && child.macro_name() == Some("It"))
    {
        let mut tag = String::new();
        let mut contents = String::new();
        let mut structural_tail = Vec::new();
        for child in item.children() {
            match child.kind() {
                NodeKind::Head if diagnostic_list => collect_terminal_semantic_text(
                    child,
                    format,
                    limits,
                    TerminalFont::Bold,
                    &mut tag,
                ),
                NodeKind::Head => collect_terminal_text(child, format, limits, &mut tag),
                NodeKind::Body => {
                    let children = child.children().collect::<Vec<_>>();
                    if let Some(tail_start) =
                        terminal_definition_body_structural_tail_start(&children)
                    {
                        for child in &children[..tail_start] {
                            collect_terminal_text(*child, format, limits, &mut contents);
                        }
                        structural_tail = children[tail_start..].to_vec();
                    } else {
                        collect_terminal_text(child, format, limits, &mut contents);
                    }
                }
                _ => {}
            }
        }
        // A quoted trailing term blank participates in the `Bl -tag` width
        // threshold, but it is not rendered before the Body field.  Preserve
        // the original width for the inline-versus-next-line decision below,
        // then remove it from the emitted fixed-field term.  `-inset` has no
        // fixed field and deliberately retains authored spacing.
        let tag_field_width = display_width(&tag);
        if !inset_list {
            tag = tag
                .trim_end_matches(|character| {
                    character == ' ' || character == TERMINAL_NONBREAKING_SPACE_MARKER
                })
                .to_owned();
        }
        if !first {
            if node.compact() {
                if !output.is_empty() && !output.ends_with('\n') {
                    append(output, "\n", maximum)?;
                }
            } else {
                append_blank_line(output, maximum)?;
            }
        }
        if tag.is_empty() {
            if !contents.is_empty() {
                // Empty item heads do not use the normal fixed definition
                // field for the list forms whose term is itself a block:
                // `-ohang` and `-inset` restart at the list margin, while
                // `-diag` retains its two-cell diagnostic lead-in.  Hanging
                // and tag lists, in contrast, still align an empty term's
                // body with their normal definition field.
                let contents_indentation = if diagnostic_list {
                    list_indentation.saturating_add(2)
                } else if overhanging_list || inset_list {
                    list_indentation
                } else {
                    body_indentation
                };
                append_terminal_text(
                    output,
                    &contents,
                    TerminalTextLayout {
                        line_start: true,
                        ..TerminalTextLayout::default()
                    },
                    contents_indentation,
                    maximum,
                )?;
            }
            render_terminal_definition_tail(
                &structural_tail,
                format,
                limits,
                body_indentation,
                output,
                maximum,
            )?;
            first = false;
            continue;
        }
        if !overhanging_list && !inset_list && !diagnostic_list {
            append_terminal_hanging_indent(output, body_indentation, maximum)?;
        }
        append_terminal_text(
            output,
            &tag,
            TerminalTextLayout {
                line_start: true,
                // `Bl -inset` has no fixed field: quoted trailing term
                // whitespace remains observable before its one-cell Body
                // separator instead of being normalized by filled layout.
                keep_spacing: inset_list && tag.contains("  "),
                ..TerminalTextLayout::default()
            },
            list_indentation,
            maximum,
        )?;
        if overhanging_list {
            if !contents.is_empty() {
                append(output, "\n", maximum)?;
                append_terminal_text(
                    output,
                    &contents,
                    TerminalTextLayout {
                        line_start: true,
                        ..TerminalTextLayout::default()
                    },
                    list_indentation,
                    maximum,
                )?;
            }
            render_terminal_definition_tail(
                &structural_tail,
                format,
                limits,
                list_indentation,
                output,
                maximum,
            )?;
            first = false;
            continue;
        }
        if inset_list || diagnostic_list {
            if !contents.is_empty() {
                let trailing_term_space = tag.ends_with(' ');
                if diagnostic_list || trailing_term_space {
                    append(
                        output,
                        &TERMINAL_NONBREAKING_SPACE_MARKER.to_string(),
                        maximum,
                    )?;
                }
                append_terminal_text(
                    output,
                    &contents,
                    TerminalTextLayout {
                        join: if inset_list && trailing_term_space {
                            TerminalJoin::Attach
                        } else {
                            TerminalJoin::Separate
                        },
                        ..TerminalTextLayout::default()
                    },
                    list_indentation,
                    maximum,
                )?;
            }
            render_terminal_definition_tail(
                &structural_tail,
                format,
                limits,
                list_indentation,
                output,
                maximum,
            )?;
            first = false;
            continue;
        }
        if !contents.is_empty() {
            // `Bl -tag` uses the declared width as its term threshold, then
            // reserves two extra cells before an inline definition. A term
            // that reaches the declared width moves its body to the next
            // line at the wider body indentation. `Bl -hang` shares the
            // normalized definition topology, but it always keeps the first
            // Body phrase on the term line; its width controls continuations
            // only, including negative/zero values.
            if hanging_list
                || (field_width > 0
                    && tag_field_width.saturating_add(2) <= field_width.unsigned_abs())
            {
                // Hanging-list widths are an optional continuation field:
                // when it reaches past the term, align the first Body phrase
                // to that same field; otherwise retain the one ordinary
                // separator that keeps the phrase on the term line.
                let field_gap = field_width
                    .saturating_sub_unsigned(display_width(&tag))
                    .max(1)
                    .unsigned_abs();
                let protected_padding = TERMINAL_NONBREAKING_SPACE_MARKER
                    .to_string()
                    .repeat(field_gap.saturating_sub(1));
                append(output, &protected_padding, maximum)?;
                if body_indentation > DEFAULT_RENDER_WIDTH {
                    // An overflow tag field still accepts its first body
                    // word on the same device line.  Subsequent filled
                    // words resume at the (also overflow) body field;
                    // treating the protected padding as an ordinary break
                    // point would instead leave a padding-only line.
                    let (first_word, remaining) = contents
                        .split_once(' ')
                        .map_or((contents.as_str(), None), |(first, rest)| {
                            (first, Some(rest))
                        });
                    // The ordinary field path below adds its final visible
                    // separator in `append_terminal_text()`.  This overflow
                    // path attaches the first word instead, so retain that
                    // one cell as protected padding.
                    append(
                        output,
                        &TERMINAL_NONBREAKING_SPACE_MARKER.to_string(),
                        maximum,
                    )?;
                    append_terminal_text(
                        output,
                        first_word,
                        TerminalTextLayout {
                            join: TerminalJoin::Attach,
                            ..TerminalTextLayout::default()
                        },
                        body_indentation,
                        maximum,
                    )?;
                    if let Some(remaining) = remaining.filter(|remaining| !remaining.is_empty()) {
                        append(output, "\n", maximum)?;
                        append_terminal_text(
                            output,
                            remaining,
                            TerminalTextLayout {
                                line_start: true,
                                ..TerminalTextLayout::default()
                            },
                            body_indentation,
                            maximum,
                        )?;
                    }
                } else {
                    append_terminal_text(
                        output,
                        &contents,
                        TerminalTextLayout::default(),
                        body_indentation,
                        maximum,
                    )?;
                }
            } else {
                append(output, "\n", maximum)?;
                append_terminal_text(
                    output,
                    &contents,
                    TerminalTextLayout {
                        line_start: true,
                        ..TerminalTextLayout::default()
                    },
                    body_indentation,
                    maximum,
                )?;
            }
        }
        render_terminal_definition_tail(
            &structural_tail,
            format,
            limits,
            body_indentation,
            output,
            maximum,
        )?;
        first = false;
    }
    if !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Find the first Body child which switches a definition item from its inline
/// term phrase to independent device flow.  The text collector deliberately
/// flattens ordinary inline macros, so letting it consume a vertical request
/// or nested display/list would discard the boundary and attach later text to
/// the tag field.
pub(super) fn terminal_definition_body_structural_tail_start(
    children: &[NodeRef<'_>],
) -> Option<usize> {
    children.iter().position(|child| {
        matches!(child.macro_name(), Some("Pp" | "PP" | "LP" | "sp" | "br"))
            || matches!(child.kind(), NodeKind::Table)
            || matches!(child.macro_name(), Some("Bd" | "Bl" | "D1" | "Dl"))
    })
}

pub(super) fn render_terminal_definition_tail(
    tail: &[NodeRef<'_>],
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let first = tail.first().copied();
    if first.is_some_and(|node| matches!(node.macro_name(), Some("Bd" | "D1" | "Dl")))
        && !output.is_empty()
        && !output.ends_with('\n')
    {
        // A list term's inline field must complete before a compact display
        // begins.  Non-compact displays already own their blank slot, while
        // a compact `Bd` only owns this physical line break.
        append(output, "\n", maximum)?;
    }
    if first.is_some_and(|node| {
        node.macro_name() == Some("Bl") && !terminal_has_visible_predecessor(node)
    }) {
        // A nested list that is the only Body child starts a fresh device
        // field. Unlike a display, the list has no preceding prose of its
        // own to claim that vertical slot, so preserve it here.
        append_blank_line(output, maximum)?;
    }
    for child in tail {
        render_terminal_node(*child, format, limits, indentation, output, maximum)?;
    }
    Ok(())
}

pub(super) fn is_first_nested_section(node: NodeRef<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() != NodeKind::Body
        || !matches!(parent.macro_name(), Some("SH" | "SS" | "Sh" | "Ss"))
    {
        return false;
    }
    let predecessors = parent
        .children()
        .take_while(|child| child.id() != node.id())
        .collect::<Vec<_>>();
    if predecessors
        .iter()
        .all(|child| child.flags().no_print || child.macro_name() == Some("PD"))
    {
        return true;
    }
    // Consecutive man subsections with only a PD control in the first Body
    // do not make an empty vertical paragraph between their headings.
    node.macro_name() == Some("SS")
        && predecessors.last().is_some_and(|previous| {
            previous.kind() == NodeKind::Block
                && previous.macro_name() == Some("SS")
                && previous
                    .children()
                    .find(|child| child.kind() == NodeKind::Body)
                    .is_some_and(|body| {
                        body.children()
                            .all(|child| child.flags().no_print || child.macro_name() == Some("PD"))
                    })
        })
}

pub(super) fn terminal_section_body_indent(node: NodeRef<'_>) -> usize {
    match node.macro_name() {
        Some("SH" | "SS") => 7,
        Some("Sh" | "Ss") => 5,
        _ => 0,
    }
}

pub(super) fn terminal_empty_man_section_starts_plain_flow(
    node: NodeRef<'_>,
    body: NodeRef<'_>,
) -> bool {
    matches!(node.macro_name(), Some("SH" | "SS"))
        && body
            .children()
            .find(|child| !child.flags().no_print)
            .is_some_and(|child| {
                child.kind() == NodeKind::Text || matches!(child.macro_name(), Some("nf" | "fi"))
            })
}

pub(super) fn terminal_section_heading_indent(node: NodeRef<'_>) -> usize {
    match node.macro_name() {
        Some("SS" | "Ss") => 3,
        _ => 0,
    }
}

pub(super) fn terminal_mdoc_element_font(node: NodeRef<'_>) -> Option<TerminalFont> {
    match node.macro_name() {
        // The 1.14.6 terminal device presents these mdoc argument families
        // in bold, including their formatter-control escapes.
        Some("Cd" | "Cm" | "Fd" | "Fl" | "Ic" | "Ms" | "Sy") => Some(TerminalFont::Bold),
        Some("Ad" | "Ar" | "Em" | "Fa" | "Fr" | "Ft" | "Mt" | "Pa" | "Sx" | "Va") => {
            Some(TerminalFont::Italic)
        }
        // `Li` establishes an explicit literal/roman scope.  In particular,
        // it must override the surrounding `Vt` italic presentation rather
        // than inheriting that variable-type scope into its children.
        Some("Li") => Some(TerminalFont::Roman),
        _ => None,
    }
}

/// Mdoc inline semantic macros leave sentence separation to their enclosing
/// prose state. A terminal period in their rendered argument is not by itself
/// a request for the device's automatic double-sentence gap.
pub(super) fn terminal_mdoc_inline_punctuation_is_literal(node: NodeRef<'_>) -> bool {
    match node.macro_name() {
        // Cd's punctuation can be either a direct argument or a separately
        // parsed sentence delimiter. Only the former suppresses automatic
        // sentence spacing (`Cd pciide?`); an outer `Cd options INSECURE .`
        // must leave the following sentence-ending delimiter observable.
        Some("Cd") => node
            .children()
            .filter_map(NodeRef::text)
            .next_back()
            .is_some_and(terminal_sentence_terminator),
        Some("Ad" | "Dv" | "Er" | "Ev" | "Ic" | "Ms" | "Va" | "Vt") => true,
        _ => false,
    }
}

/// A text node directly in mdoc's ordinary block flow can end a terminal
/// sentence. Text nested inside a semantic mdoc inline macro deliberately
/// does not: those macros have their own punctuation and spacing contracts.
pub(super) fn terminal_mdoc_plain_text_sentence(node: NodeRef<'_>) -> bool {
    node.ancestors()
        .any(|ancestor| matches!(ancestor.macro_name(), Some("Sh" | "Ss")))
        && !node
            .ancestors()
            .any(|ancestor| terminal_mdoc_element_font(ancestor).is_some())
}

/// A childless mdoc `Fl` still prints its own dash.  When the next visible
/// same-line node is another macro, `termp_fl_pre()` keeps that macro attached
/// to the dash (`Fl Cm help` → `-help`); ordinary text deliberately retains a
/// separator.  Transparent nodes do not decide the boundary themselves.
pub(super) fn terminal_mdoc_empty_fl_attaches_to_following_macro(node: NodeRef<'_>) -> bool {
    if node.macro_name() != Some("Fl") || node.children().next().is_some() {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    parent
        .children()
        .skip_while(|sibling| sibling.id() != node.id())
        .skip(1)
        .find(|sibling| !sibling.flags().no_print)
        .is_some_and(|next| {
            next.macro_name().is_some()
                && node
                    .source_position()
                    .zip(next.source_position())
                    .is_some_and(|(current, following)| current.line == following.line)
        })
}

/// `.Pf` owns one literal prefix and attaches exactly to the next visible
/// same-line token.  Unlike an empty `.Fl`, the following token may be either
/// a macro or ordinary text.  Parser validation reports an incomplete prefix,
/// but rendering also checks this relationship so recovery cannot join it to
/// a later physical source line.
pub(super) fn terminal_mdoc_prefix_attaches_to_following_token(node: NodeRef<'_>) -> bool {
    if node.macro_name() != Some("Pf") {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    parent
        .children()
        .skip_while(|sibling| sibling.id() != node.id())
        .skip(1)
        .find(|sibling| !sibling.flags().no_print)
        .is_some_and(|next| {
            node.source_position()
                .zip(next.source_position())
                .is_some_and(|(current, following)| current.line == following.line)
        })
}

/// Render man-ext `OP` as its terminal option synopsis.
///
/// The parser keeps all recovered arguments for diagnostics, but the device
/// consumes at most two: the option in bold and its operand in italic.
pub(super) fn terminal_man_option(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
) -> String {
    let mut arguments = node.children().filter(|child| !child.flags().no_print);
    let Some(option) = arguments.next() else {
        return "[]".to_owned();
    };
    let mut contents = String::from("[");
    let mut option_text = String::new();
    collect_terminal_semantic_text(option, format, limits, TerminalFont::Bold, &mut option_text);
    contents.push_str(&option_text);
    if let Some(argument) = arguments.next() {
        let mut value = String::new();
        collect_terminal_semantic_text(argument, format, limits, TerminalFont::Italic, &mut value);
        if !value.is_empty() {
            contents.push(' ');
            contents.push_str(&value);
        }
    }
    contents.push(']');
    contents
}

/// Terminal fonts for man(7)'s two-argument alternating requests.
///
/// `man_term.c:pre_alternate()` toggles the device font after every argument
/// and sets `TERMP_NOSPACE` between them.  Font-size-only `SB`/`SM` requests
/// are handled as ordinary bold/roman text elsewhere; these six names are the
/// complete terminal alternating family.
pub(super) fn terminal_man_alternating_fonts(name: Option<&str>) -> Option<[TerminalFont; 2]> {
    match name {
        Some("BI") => Some([TerminalFont::Bold, TerminalFont::Italic]),
        Some("IB") => Some([TerminalFont::Italic, TerminalFont::Bold]),
        Some("BR") => Some([TerminalFont::Bold, TerminalFont::Roman]),
        Some("RB") => Some([TerminalFont::Roman, TerminalFont::Bold]),
        Some("IR") => Some([TerminalFont::Italic, TerminalFont::Roman]),
        Some("RI") => Some([TerminalFont::Roman, TerminalFont::Italic]),
        _ => None,
    }
}

pub(super) fn terminal_inherited_font(node: NodeRef<'_>) -> TerminalFont {
    terminal_scope_font(node).unwrap_or_default()
}

/// Return a structural mdoc font when one owns this node.  A plain roff text
/// node has no such scope and consequently inherits the document-order `.ft`
/// state instead of being reset to Roman.
pub(super) fn terminal_scope_font(node: NodeRef<'_>) -> Option<TerminalFont> {
    if terminal_bf_scope_closed_before(node) {
        return Some(TerminalFont::Roman);
    }
    node.ancestors().find_map(|ancestor| {
        // A `Bf` without a recognized font argument resets its nested
        // scope to Roman.  The normalized AST represents both missing
        // and unknown arguments with `font == None`, which is precisely
        // the terminal device's shared fallback behavior.
        if ancestor.kind() == NodeKind::Block
            && ancestor.macro_name() == Some("Bf")
            && ancestor.font().is_none()
        {
            return Some(TerminalFont::Roman);
        }
        let font = ancestor.font().map(|font| match font {
            NormalizedFont::Emphasis => TerminalFont::Italic,
            NormalizedFont::Literal => TerminalFont::Roman,
            NormalizedFont::Symbolic => TerminalFont::Bold,
        });
        font.or_else(|| {
            // `Vt` italicizes its direct text arguments, but it does
            // not flatten nested semantic macro children: a nested `Sy`
            // must still render bold.  Inheritance preserves that
            // source-level boundary while covering both inline and
            // SYNOPSIS partial-block forms.
            (ancestor.macro_name() == Some("Vt")).then_some(TerminalFont::Italic)
        })
    })
}

/// Resolve the effective device font for one ordinary text node.  Structural
/// mdoc scopes deliberately take precedence over roff's process-like `.ft`
/// register; outside those scopes the request state remains in effect across
/// ordinary sibling blocks just as it does in the terminal device.
pub(super) fn terminal_text_font(node: NodeRef<'_>) -> TerminalFont {
    terminal_scope_font(node).unwrap_or_else(|| terminal_request_font_before(node).current)
}

/// Reconstruct the `.ft` register immediately before `node` in document
/// order. Each level contributes every prior sibling subtree before advancing
/// down the path to the target, which handles requests nested inside a roff
/// body without relying on arena IDs or mutable global state.
pub(super) fn terminal_request_font_before(node: NodeRef<'_>) -> TerminalRequestFontState {
    let mut lineage = vec![node];
    let mut cursor = node;
    while let Some(parent) = cursor.parent() {
        lineage.push(parent);
        cursor = parent;
    }
    lineage.reverse();

    let mut state = TerminalRequestFontState::default();
    for current in lineage.into_iter().skip(1) {
        let Some(parent) = current.parent() else {
            continue;
        };
        for sibling in parent.children() {
            if sibling.id() == current.id() {
                break;
            }
            terminal_apply_font_requests(sibling, &mut state);
        }
    }
    state
}

pub(super) fn terminal_apply_font_requests(
    node: NodeRef<'_>,
    state: &mut TerminalRequestFontState,
) {
    if node.kind() == NodeKind::Element && node.macro_name() == Some("ft") {
        let selector = node.children().find_map(NodeRef::text);
        terminal_apply_font_request(selector, state);
        return;
    }
    for child in node.children() {
        terminal_apply_font_requests(child, state);
    }
}

pub(super) fn terminal_apply_font_request(
    selector: Option<&str>,
    state: &mut TerminalRequestFontState,
) {
    let next = match selector.unwrap_or_default() {
        "B" | "CB" => Some(TerminalFont::Bold),
        "I" | "CI" => Some(TerminalFont::Italic),
        "BI" => Some(TerminalFont::BoldItalic),
        "R" | "CR" => Some(TerminalFont::Roman),
        "" | "P" => {
            std::mem::swap(&mut state.current, &mut state.previous);
            None
        }
        _ => None,
    };
    if let Some(next) = next {
        state.previous = state.current;
        state.current = next;
    }
}

/// Apply the cumulative `.po` device offset to one text node's enclosing
/// field. The raw offset can extend beyond the visible page; mandoc retains
/// that value for a later relative request, then clamps only the rendered
/// field to the terminal's `[-offset, 60]` range.
pub(super) fn terminal_text_indentation(node: NodeRef<'_>, indentation: usize) -> usize {
    // A source tail released by `Fc` resumes one cell into the SYNOPSIS
    // field. The public AST correctly exposes it as the next text sibling of
    // `Fo`, but not the terminal-only continuation column.
    let indentation = if terminal_mdoc_function_tail(node) {
        indentation.saturating_add(1)
    } else {
        indentation
    };
    let indentation = terminal_request_indent_before(node, indentation).unwrap_or(indentation);
    let state = terminal_page_offset_before(node);
    let lower = -isize::try_from(indentation).unwrap_or(isize::MIN);
    let applied = state.current.clamp(lower, 60);
    if applied.is_negative() {
        indentation.saturating_sub(applied.unsigned_abs())
    } else {
        indentation.saturating_add(applied.unsigned_abs())
    }
}

pub(super) fn terminal_mdoc_function_tail(node: NodeRef<'_>) -> bool {
    node.kind() == NodeKind::Text
        && terminal_previous_sibling(node).is_some_and(|previous| {
            previous.kind() == NodeKind::Block
                && previous.macro_name() == Some("Fo")
                && terminal_mdoc_synopsis(previous)
        })
}

pub(super) fn terminal_page_offset_before(node: NodeRef<'_>) -> TerminalPageOffsetState {
    let mut lineage = vec![node];
    let mut cursor = node;
    while let Some(parent) = cursor.parent() {
        lineage.push(parent);
        cursor = parent;
    }
    lineage.reverse();

    let mut state = TerminalPageOffsetState::default();
    for current in lineage.into_iter().skip(1) {
        let Some(parent) = current.parent() else {
            continue;
        };
        for sibling in parent.children() {
            if sibling.id() == current.id() {
                break;
            }
            terminal_apply_page_offset_requests(sibling, &mut state);
        }
    }
    state
}

pub(super) fn terminal_apply_page_offset_requests(
    node: NodeRef<'_>,
    state: &mut TerminalPageOffsetState,
) {
    if node.kind() == NodeKind::Element && node.macro_name() == Some("po") {
        let requested = node.children().find_map(NodeRef::text);
        terminal_apply_page_offset_request(requested, state);
        return;
    }
    for child in node.children() {
        terminal_apply_page_offset_requests(child, state);
    }
}

pub(super) fn terminal_apply_page_offset_request(
    requested: Option<&str>,
    state: &mut TerminalPageOffsetState,
) {
    let relative = requested.is_some_and(|value| value.trim_start().starts_with(['+', '-']));
    let next = requested
        .and_then(terminal_page_offset_units)
        .map_or(state.previous, |value| {
            if relative {
                state.current.saturating_add(value)
            } else {
                value
            }
        });
    state.previous = state.current;
    state.current = next;
}

pub(super) fn terminal_page_offset_units(value: &str) -> Option<isize> {
    terminal_signed_layout_units(value).or_else(|| value.trim().parse().ok())
}

/// Resolve the most recent roff `.in` request before a text node.  Its
/// absolute device column wins over the structural field passed by the AST;
/// a first relative request uses that structural field as its base.
pub(super) fn terminal_request_indent_before(node: NodeRef<'_>, base: usize) -> Option<usize> {
    let mut lineage = vec![node];
    let mut cursor = node;
    while let Some(parent) = cursor.parent() {
        lineage.push(parent);
        cursor = parent;
    }
    lineage.reverse();

    let mut state = TerminalRequestIndentState::default();
    for current in lineage.into_iter().skip(1) {
        let Some(parent) = current.parent() else {
            continue;
        };
        for sibling in parent.children() {
            if sibling.id() == current.id() {
                break;
            }
            terminal_apply_indent_requests(sibling, base, &mut state);
        }
    }
    state.current.map(|value| value.max(0).unsigned_abs())
}

pub(super) fn terminal_apply_indent_requests(
    node: NodeRef<'_>,
    base: usize,
    state: &mut TerminalRequestIndentState,
) {
    if matches!(node.macro_name(), Some("Pp" | "PP" | "LP")) {
        // Paragraph macros re-enter their package-managed body field, which
        // supersedes a preceding raw roff indentation request.
        state.current = None;
        return;
    }
    if node.kind() == NodeKind::Element
        && node.macro_name() == Some("in")
        && !terminal_man_tp_head_indent_request(node)
    {
        terminal_apply_indent_request(node.children().find_map(NodeRef::text), base, state);
        return;
    }
    for child in node.children() {
        terminal_apply_indent_requests(child, base, state);
    }
}

/// A man `TP` keeps an `.in` request inside its Head as a tag-only layout
/// adjustment. `render_terminal_man_tp` consumes that private meaning while
/// placing the tag; it must not update the ordinary roff field register seen
/// by the following Body.
pub(super) fn terminal_man_tp_head_indent_request(node: NodeRef<'_>) -> bool {
    node.ancestors().any(|ancestor| {
        ancestor.kind() == NodeKind::Head
            && ancestor
                .parent()
                .is_some_and(|parent| parent.macro_name() == Some("TP"))
    })
}

pub(super) fn terminal_apply_indent_request(
    requested: Option<&str>,
    base: usize,
    state: &mut TerminalRequestIndentState,
) {
    let Some(value) = requested.map(str::trim).filter(|value| !value.is_empty()) else {
        state.current = None;
        return;
    };
    let Some(units) = terminal_signed_roff_en_prefix(value) else {
        state.current = None;
        return;
    };
    if value.starts_with(['+', '-']) {
        let base = state
            .current
            .unwrap_or_else(|| isize::try_from(base).unwrap_or(isize::MAX));
        state.current = Some(base.saturating_add(units));
    } else {
        state.current = Some(units);
    }
}

/// Reconstruct the `.ll` register before one text node.  As with font and
/// page-offset requests, every prior sibling subtree along the ancestor path
/// contributes state, while the request's own AST argument stays public.
pub(super) fn terminal_line_length_before(node: NodeRef<'_>) -> TerminalLineLength {
    let mut lineage = vec![node];
    let mut cursor = node;
    while let Some(parent) = cursor.parent() {
        lineage.push(parent);
        cursor = parent;
    }
    lineage.reverse();

    let mut state = TerminalLineLength::Default;
    for current in lineage.into_iter().skip(1) {
        let Some(parent) = current.parent() else {
            continue;
        };
        for sibling in parent.children() {
            if sibling.id() == current.id() {
                break;
            }
            terminal_apply_line_length_requests(sibling, &mut state);
        }
    }
    state
}

pub(super) fn terminal_apply_line_length_requests(
    node: NodeRef<'_>,
    state: &mut TerminalLineLength,
) {
    if node.kind() == NodeKind::Element && node.macro_name() == Some("ll") {
        terminal_apply_line_length_request(node.children().find_map(NodeRef::text), state);
        return;
    }
    for child in node.children() {
        terminal_apply_line_length_requests(child, state);
    }
}

/// Apply the subset of `.ll` requests that changes a terminal field. Bare or
/// malformed requests restore the renderer's configured default; a signed
/// valid request remains symbolic when based on that default so a caller's
/// nonstandard `Renderer::with_width()` is honoured at the final width pass.
pub(super) fn terminal_apply_line_length_request(
    requested: Option<&str>,
    state: &mut TerminalLineLength,
) {
    let Some(value) = requested.map(str::trim).filter(|value| !value.is_empty()) else {
        *state = TerminalLineLength::Default;
        return;
    };
    let Some(units) = terminal_signed_layout_units(value) else {
        *state = TerminalLineLength::Default;
        return;
    };
    if value.starts_with(['+', '-']) {
        *state = match *state {
            TerminalLineLength::Default => TerminalLineLength::Relative(units),
            TerminalLineLength::Relative(prior) => {
                TerminalLineLength::Relative(prior.saturating_add(units))
            }
            TerminalLineLength::Absolute(prior) => {
                TerminalLineLength::Absolute(prior.saturating_add_signed(units))
            }
        };
    } else {
        *state = TerminalLineLength::Absolute(units.max(0).unsigned_abs());
    }
}

/// Whether an mdoc `Ef` was preserved as an otherwise empty `Bf` Body before
/// this node inside an outer syntactic scope.  The canonical AST must retain
/// that recovery node for source compatibility; terminal presentation uses it
/// as a state transition from the enclosing Bf font back to Roman.
pub(super) fn terminal_bf_scope_closed_before(node: NodeRef<'_>) -> bool {
    let closes_bf = node
        .ancestors()
        .any(|ancestor| ancestor.macro_name() == Some("Bf"));
    let mut current = node;
    while let Some(parent) = current.parent() {
        for sibling in parent.children() {
            if sibling.id() == current.id() {
                break;
            }
            if terminal_is_closed_bf_scope(sibling)
                || (closes_bf
                    && terminal_embedded_quote_closing(sibling, RenderFormat::Ascii).is_some())
            {
                return true;
            }
        }
        current = parent;
    }
    false
}

pub(super) fn terminal_contains_closed_bf_scope(node: NodeRef<'_>) -> bool {
    terminal_is_closed_bf_scope(node) || node.children().any(terminal_contains_closed_bf_scope)
}

pub(super) fn terminal_is_closed_bf_scope(node: NodeRef<'_>) -> bool {
    node.kind() == NodeKind::Body
        && node.macro_name() == Some("Bf")
        && node.font().is_some()
        && node.children().next().is_none()
}

pub(super) fn terminal_mdoc_display_indentation(node: NodeRef<'_>, indentation: usize) -> usize {
    let offset = terminal_mdoc_display_offset(node);
    if offset.is_negative() {
        indentation.saturating_sub(offset.unsigned_abs())
    } else {
        indentation.saturating_add(offset.unsigned_abs())
    }
}

pub(super) fn terminal_mdoc_display_offset(node: NodeRef<'_>) -> isize {
    match node.offset() {
        None | Some("left") => 0,
        Some("indent") => 6,
        Some("indent-two") => 12,
        Some(value) => terminal_signed_layout_units(value)
            .unwrap_or_else(|| isize::try_from(display_width(value)).unwrap_or(isize::MAX)),
    }
}

pub(super) fn terminal_mdoc_list_indentation(node: NodeRef<'_>, indentation: usize) -> usize {
    let offset = match node.offset() {
        None => 0,
        // These mdoc layout keywords name terminal fields rather than source
        // strings. Unknown names fall back to their visible-cell width.
        Some("left") => 4,
        Some("indent") => 6,
        Some("indent-two") => 10,
        Some(value) => terminal_signed_layout_units(value)
            .unwrap_or_else(|| isize::try_from(display_width(value)).unwrap_or(isize::MAX)),
    };
    if offset.is_negative() {
        indentation.saturating_sub(offset.unsigned_abs())
    } else {
        indentation.saturating_add(offset.unsigned_abs())
    }
}

pub(super) fn terminal_authors_section(node: NodeRef<'_>) -> bool {
    terminal_mdoc_section_named(node, "AUTHORS")
}

/// Return the compact mdoc system-name forms with one optional version
/// argument.  `St` is deliberately excluded: its expanded standard name is
/// ordinary prose, not a single device word.
pub(super) fn terminal_mdoc_system_macro(name: Option<&str>) -> bool {
    matches!(name, Some("Bsx" | "Dx" | "Fx" | "Nx" | "Ox" | "Ux"))
}

/// Render the stable system-name case of mdoc's short-lived `Bk` word keep.
/// The full macro keeps inter-node word boundaries by source line; use this
/// narrow renderer-private projection only once a system macro is present,
/// leaving complex `Bk` bodies on their established structural path.
pub(super) fn terminal_mdoc_system_word_keep(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
) -> Option<String> {
    let body = node
        .children()
        .find(|child| child.kind() == NodeKind::Body)?;
    let children = body
        .children()
        .filter(|child| !child.flags().no_print)
        .collect::<Vec<_>>();
    if !children
        .iter()
        .any(|child| terminal_mdoc_system_macro(child.macro_name()))
    {
        return None;
    }
    let mut output = String::new();
    for child in children {
        let mut fragment = if child.macro_name() == Some("Xr") {
            terminal_cross_reference(child, format, limits).unwrap_or_default()
        } else {
            let mut fragment = String::new();
            collect_terminal_text(child, format, limits, &mut fragment);
            fragment
        };
        if terminal_mdoc_system_macro(child.macro_name()) {
            fragment = fragment.replace(' ', &TERMINAL_NONBREAKING_SPACE_MARKER.to_string());
        }
        if fragment.is_empty() {
            continue;
        }
        if !output.is_empty() {
            output.push(if child.flags().line_start {
                ' '
            } else {
                TERMINAL_NONBREAKING_SPACE_MARKER
            });
        }
        output.push_str(&fragment);
    }
    Some(output)
}

/// Collect an ordinary `Bk` Body into one unbreakable device phrase.
///
/// `Bk`'s Head contains layout selectors (and, after recovery, invalid
/// selector tail words) rather than display content.  Its Body is the only
/// phrase that participates in the keep request.  Keep this narrow to inline
/// content so block-level layouts retain their established structural paths.
pub(super) fn terminal_mdoc_word_keep(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
) -> Option<String> {
    let body = node
        .children()
        .find(|child| child.kind() == NodeKind::Body)?;
    let children = body
        .children()
        .filter(|child| !child.flags().no_print)
        .collect::<Vec<_>>();
    if children.is_empty()
        // A word keep around ordinary free-form text is intentionally inert:
        // only a macro-owned phrase activates the device keep state.
        || children.iter().all(|child| child.kind() == NodeKind::Text)
        || children.iter().any(|child| {
            matches!(child.kind(), NodeKind::Table | NodeKind::Equation)
                || matches!(child.macro_name(), Some("Bd" | "Bl" | "D1" | "Dl" | "Fn" | "Fo"))
        })
    {
        return None;
    }
    let line_started_fragments = terminal_mdoc_bk_line_started_fragments(body, format, limits);
    let mut output = String::new();
    for child in children {
        let fragment = if child.macro_name() == Some("Xr") {
            terminal_cross_reference(child, format, limits).unwrap_or_default()
        } else {
            let mut fragment = String::new();
            collect_terminal_text(child, format, limits, &mut fragment);
            fragment
        };
        if fragment.is_empty() {
            continue;
        }
        if !output.is_empty() {
            output.push(if child.flags().line_start {
                ' '
            } else {
                TERMINAL_NONBREAKING_SPACE_MARKER
            });
        }
        output.push_str(&fragment.replace(' ', &TERMINAL_NONBREAKING_SPACE_MARKER.to_string()));
    }
    for punctuation in ['.', ',', ';', ':', '!', '?', ')', ']'] {
        output = output.replace(
            &format!("{TERMINAL_NONBREAKING_SPACE_MARKER}{punctuation}"),
            &punctuation.to_string(),
        );
    }
    // `Bk` keeps words only after the first rendered word on a physical
    // source line.  A nested optional or plain `No` word that starts a later
    // line after its preceding sibling has closed therefore retains an
    // ordinary breakable separator. The arena has already normalized the
    // literal `Oc`, but the nested Body sibling boundary still distinguishes
    // this from a line containing only a new `Oo` opener.
    for fragment in line_started_fragments {
        output = output.replace(
            &format!("{TERMINAL_NONBREAKING_SPACE_MARKER}{fragment}"),
            &format!(" {fragment}"),
        );
    }
    (!output.is_empty()).then_some(output)
}

pub(super) fn terminal_mdoc_bk_line_started_fragments(
    body: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
) -> Vec<String> {
    fn visit(node: NodeRef<'_>, format: RenderFormat, limits: &Limits, output: &mut Vec<String>) {
        let is_optional = node.kind() == NodeKind::Block && node.macro_name() == Some("Oo");
        let is_plain_no = node.kind() == NodeKind::Element && node.macro_name() == Some("No");
        if (is_optional || is_plain_no)
            && node.flags().line_start
            && node
                .parent()
                .is_some_and(|parent| parent.macro_name() == Some("Oo"))
            && terminal_previous_sibling(node).is_some()
        {
            let mut optional = String::new();
            collect_terminal_text(node, format, limits, &mut optional);
            if !optional.is_empty() {
                output.push(optional);
            }
        }
        for child in node.children() {
            visit(child, format, limits, output);
        }
    }

    let mut optionals = Vec::new();
    for child in body.children() {
        visit(child, format, limits, &mut optionals);
    }
    optionals
}

/// Select the synopsis continuation field for a kept phrase.
///
/// `Bk` continues below the owning declaration name, not at a fixed global
/// offset.  The compatible tree keeps that declaration as an ancestor, so
/// recover its display width only for this renderer-private field decision.
pub(super) fn terminal_mdoc_bk_continuation_indent(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
) -> usize {
    let Some(name) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == NodeKind::Block && ancestor.macro_name() == Some("Nm"))
        .and_then(|name| name.children().find(|child| child.kind() == NodeKind::Head))
    else {
        return indentation.saturating_add(10);
    };
    let mut rendered = String::new();
    collect_terminal_mdoc_synopsis_name_head(name, format, limits, &mut rendered);
    if rendered.is_empty() {
        indentation.saturating_add(10)
    } else {
        indentation
            .saturating_add(display_width(&rendered))
            .saturating_add(1)
    }
}

/// A synopsis declaration whose implicit `Nm` Head exceeds the device width
/// retains each later mdoc macro argument as one field phrase.  Otherwise the
/// width pass would split the synthesized default of a bare `Ar` into two
/// impossible columns beyond the name field.
pub(super) fn terminal_mdoc_long_name_field(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
) -> bool {
    let Some(head) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == NodeKind::Block && ancestor.macro_name() == Some("Nm"))
        .and_then(|name| name.children().find(|child| child.kind() == NodeKind::Head))
    else {
        return false;
    };
    let mut rendered = String::new();
    collect_terminal_mdoc_synopsis_name_head(head, format, limits, &mut rendered);
    display_width(&rendered) > 70
}

/// Resolve the persistent mdoc `An` layout mode in one containing body.
///
/// The parser keeps an option directive as a public `An` element so AST
/// consumers can observe it, but the terminal device treats that element as
/// a state update and consumes all its remaining words.  `An` siblings are
/// emitted in source order under a single mdoc body, so a bounded sibling
/// scan exactly matches the device's state without adding renderer state to
/// the public arena.
pub(super) fn terminal_author_mode(node: NodeRef<'_>) -> AuthorMode {
    let mut mode = if terminal_authors_section(node) {
        AuthorMode::Split
    } else {
        AuthorMode::NoSplit
    };
    let Some(parent) = node.parent() else {
        return mode;
    };
    for sibling in parent.children() {
        if sibling.id() == node.id() {
            break;
        }
        if sibling.macro_name() == Some("An")
            && let Some(updated) = sibling.author_mode()
        {
            mode = updated;
        }
    }
    mode
}

/// A split author begins a fresh terminal line after an earlier `An` sibling.
/// The AUTHORS section's implicit initial split mode deliberately leaves its
/// first author attached to preceding prose; an explicit `-split` directive
/// counts as the earlier sibling and therefore starts the next author line.
pub(super) fn terminal_author_starts_line(node: NodeRef<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        parent
            .children()
            .take_while(|sibling| sibling.id() != node.id())
            .any(|sibling| sibling.macro_name() == Some("An"))
    })
}

pub(super) fn terminal_mdoc_section_named(node: NodeRef<'_>, name: &str) -> bool {
    node.ancestors().any(|ancestor| {
        if ancestor.kind() != NodeKind::Block || ancestor.macro_name() != Some("Sh") {
            return false;
        }
        let Some(head) = ancestor
            .children()
            .find(|child| child.kind() == NodeKind::Head)
        else {
            return false;
        };
        let mut title = String::new();
        collect_terminal_plain_words(head, &mut title);
        title.eq_ignore_ascii_case(name)
    })
}

pub(super) fn collect_terminal_plain_words(node: NodeRef<'_>, output: &mut String) {
    if let Some(text) = node.text()
        && !text.is_empty()
    {
        if !output.is_empty() {
            output.push(' ');
        }
        output.push_str(text);
    }
    for child in node.children() {
        collect_terminal_plain_words(child, output);
    }
}

pub(super) fn terminal_mdoc_synopsis(node: NodeRef<'_>) -> bool {
    node.flags().synopsis_pretty || terminal_mdoc_section_named(node, "SYNOPSIS")
}

/// A paragraph can be parsed after an inline `nS` reset while still sitting
/// inside an already-open synopsis-pretty `Nm` block.  The device retains the
/// declaration field through that nested recovery shape, so the paragraph's
/// own flag is not sufficient to select its continuation column.
pub(super) fn terminal_mdoc_synopsis_paragraph(node: NodeRef<'_>) -> bool {
    node.flags().synopsis_pretty
        || node.ancestors().any(|ancestor| {
            ancestor.kind() == NodeKind::Block
                && ancestor.macro_name() == Some("Nm")
                && ancestor.flags().synopsis_pretty
        })
}

/// A synopsis paragraph inherits the name continuation field only while it
/// remains structurally inside the owning `Nm` block. Section-level synopsis
/// prose and function declarations use the ordinary five-cell field even
/// though their parser flags also carry synopsis provenance.
pub(super) fn terminal_mdoc_synopsis_name_paragraph(node: NodeRef<'_>) -> bool {
    node.ancestors().any(|ancestor| {
        ancestor.kind() == NodeKind::Block
            && ancestor.macro_name() == Some("Nm")
            && ancestor.flags().synopsis_pretty
    })
}

/// True for the compact `Nm` synopsis grammar consisting solely of optional
/// argument forms.  Its body uses the device's standard five-plus-four-cell
/// continuation field, unlike an arbitrary mixed synopsis body (and unlike a
/// nested `Bk`, which calculates its own field from the preceding argument).
pub(super) fn terminal_mdoc_synopsis_option_body(node: NodeRef<'_>) -> bool {
    let mut found = false;
    node.children()
        .filter(|child| !child.flags().no_print)
        .all(|child| {
            found = true;
            child.kind() == NodeKind::Block && child.macro_name() == Some("Op")
        })
        && found
}

/// Whether `node` is being formatted inside an mdoc `Bk` body.  The public
/// compatible AST intentionally discards the validator-only `-words` option,
/// but every retained Bk block represents the terminal keep scope introduced
/// by that request.
pub(super) fn terminal_mdoc_word_keep_scope(node: NodeRef<'_>) -> bool {
    node.ancestors()
        .any(|ancestor| ancestor.kind() == NodeKind::Block && ancestor.macro_name() == Some("Bk"))
}

/// Mirror the terminal device's `synopsis_pre()` vertical spacing for the
/// declaration families currently rendered structurally.  `Ft` followed by a
/// function starts the next declaration line; a later `Ft` after a completed
/// function starts a new vertical group.
pub(super) fn terminal_mdoc_synopsis_spacing(
    node: NodeRef<'_>,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let Some(previous) = terminal_previous_sibling(node) else {
        return Ok(());
    };
    if previous.macro_name() == node.macro_name()
        && !matches!(node.macro_name(), Some("Ft" | "Fo" | "Fn"))
    {
        if !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    match previous.macro_name() {
        Some("Fd" | "Fn" | "Fo" | "In" | "Vt") => append_blank_line(output, maximum),
        Some("Ft") if node.macro_name() == Some("Ft") => append_blank_line(output, maximum),
        _ if !output.is_empty() && !output.ends_with('\n') => append(output, "\n", maximum),
        _ => Ok(()),
    }
}

pub(super) fn terminal_previous_sibling(node: NodeRef<'_>) -> Option<NodeRef<'_>> {
    node.parent()?
        .children()
        .take_while(|child| child.id() != node.id())
        .last()
}

pub(super) fn terminal_next_visible_sibling(node: NodeRef<'_>) -> Option<NodeRef<'_>> {
    node.parent()?
        .children()
        .skip_while(|child| child.id() != node.id())
        .skip(1)
        .find(|child| !child.flags().no_print)
}

pub(super) fn terminal_signed_layout_units(value: &str) -> Option<isize> {
    if let Some(value) = value.strip_suffix('n') {
        return value.parse().ok();
    }
    let value = value.strip_suffix('i')?.parse::<f64>().ok()?;
    // The terminal device rounds scaled inch values to the nearest `n` unit.
    (value * 10.0).round().to_string().parse().ok()
}

/// Parse the bare numeric field width accepted by a man `RS` request.
///
/// The caller has already tried all scaled forms.  The terminal device
/// truncates a finite bare decimal toward zero and accepts only values an
/// `isize` can represent.
#[allow(clippy::cast_precision_loss)] // Bounds only compare the f64 parser domain with the target integer range.
pub(super) fn terminal_plain_field_width(value: &str) -> Option<isize> {
    let value = value.parse::<f64>().ok()?;
    if !value.is_finite() || value < isize::MIN as f64 || value > isize::MAX as f64 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some(value as isize)
}

/// Apply man(7)'s persistent `.in` request to a terminal field.  The parser
/// normalizes a request captured below an open `TP` Head to a signed relative
/// value, while an ordinary unsigned request names an absolute column.
pub(super) fn terminal_man_in_target(value: &str, indentation: usize) -> Option<usize> {
    let value = value.trim();
    let units = terminal_signed_layout_units(value)?;
    if value.starts_with(['+', '-']) {
        return Some(if units.is_negative() {
            indentation.saturating_sub(units.unsigned_abs())
        } else {
            indentation.saturating_add(units.unsigned_abs())
        });
    }
    Some(units.max(0).unsigned_abs())
}

/// Parse the prefix accepted by `a2roffsu(value, SCALE_EN)`, then resolve it
/// to terminal cells. Unlike mdoc's `a2width()`, the man formatter accepts a
/// numeric prefix even when a trailing byte remains; an unrecognised suffix
/// keeps the default `n` unit.
pub(super) fn terminal_signed_roff_en_prefix(value: &str) -> Option<isize> {
    let mut numeric = None;
    for end in value
        .char_indices()
        .map(|(index, _)| index)
        .skip(1)
        .chain(std::iter::once(value.len()))
    {
        if let Ok(scale) = value[..end].parse::<f64>()
            && scale.is_finite()
        {
            numeric = Some((end, scale));
        }
    }
    let (end, scale) = numeric?;
    let unit = value[end..].chars().next();
    let multiplier = match unit {
        Some('c') => 240.0 / 2.54,
        Some('i') => 240.0,
        Some('f') => 65_536.0,
        Some('M') => 0.24,
        Some('m' | 'n') => 24.0,
        Some('P' | 'v') => 40.0,
        Some('p') => 10.0 / 3.0,
        Some('u') => 1.0,
        Some(_) | None => 24.0,
    };
    terminal_hen(scale, multiplier)
}

pub(super) fn terminal_hen(scale: f64, multiplier: f64) -> Option<isize> {
    let basic = (scale * multiplier).trunc();
    if !basic.is_finite() {
        return None;
    }
    // Finite values are clamped to the target range before reproducing C's
    // truncating conversion from scaled layout units.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let basic = basic.clamp(isize::MIN as f64, isize::MAX as f64) as isize;
    Some(if basic >= 0 {
        basic.saturating_add(11) / 24
    } else {
        -(basic.saturating_abs().saturating_add(11) / 24)
    })
}

/// Convert an mdoc `Bl` layout field the same way libmandoc's terminal
/// `a2width()` does.  It accepts a complete roff scale, rounds it in the
/// terminal's 24-basic-unit grid, and deliberately falls back to the visible
/// width of malformed or suffix-bearing input such as `1cx` and `xxx`.
pub(super) fn terminal_mdoc_a2width(value: &str) -> isize {
    let Some(unit) = value.chars().last() else {
        return 0;
    };
    let number = &value[..value.len().saturating_sub(unit.len_utf8())];
    let Some(multiplier) = (match unit {
        'c' => Some(240.0 / 2.54),
        'i' => Some(240.0),
        'f' => Some(65_536.0),
        'M' => Some(0.24),
        'm' | 'n' => Some(24.0),
        'P' | 'v' => Some(40.0),
        'p' => Some(10.0 / 3.0),
        'u' => Some(1.0),
        _ => None,
    }) else {
        return isize::try_from(display_width(value)).unwrap_or(isize::MAX);
    };
    let Ok(scale) = number.parse::<f64>() else {
        return isize::try_from(display_width(value)).unwrap_or(isize::MAX);
    };
    terminal_hen(scale, multiplier)
        .unwrap_or_else(|| isize::try_from(display_width(value)).unwrap_or(isize::MAX))
}

/// Resolve roff's one-line temporary indentation. Signed forms are relative
/// to the current structural field; an unsigned value is an absolute terminal
/// column. The device clamps a request at column 72, except that an already
/// wider enclosing structural field is never pulled back to the clamp.
pub(super) fn terminal_temporary_indent_target(value: &str, indentation: usize) -> Option<usize> {
    let value = value.trim();
    let units = terminal_signed_layout_units(value)?;
    let relative = value.starts_with(['+', '-']);
    let target = if relative {
        if units.is_negative() {
            indentation.saturating_sub(units.unsigned_abs())
        } else {
            indentation.saturating_add(units.unsigned_abs())
        }
    } else {
        units.max(0).unsigned_abs()
    };
    Some(target.min(indentation.max(72)))
}

/// Convert roff's vertical scaled units to terminal line spans. This mirrors
/// libmandoc's `term_vspan()`: the terminal's basic unit is one fortieth of a
/// line, while centimetres, inches, picas, points, ens, and ems retain the
/// device's fixed conversion factors.
#[allow(clippy::cast_possible_truncation)] // Match C's deliberate cast after the 0.4995 rounding offset.
pub(super) fn terminal_vertical_span(value: &str) -> Option<isize> {
    let value = value.trim();
    let numeric_end = value
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(value.len()))
        .filter_map(|end| value[..end].parse::<f64>().ok().map(|number| (end, number)))
        .next_back()?;
    let (numeric_end, number) = numeric_end;
    let factor = match value[numeric_end..].chars().next() {
        Some('u') => 1.0 / 40.0,
        Some('c') => 6.0 / 2.54,
        Some('f') => 65_536.0 / 40.0,
        Some('i') => 6.0,
        Some('M') => 0.006,
        Some('m' | 'n') => 0.6,
        Some('P' | 'v') => 1.0,
        Some('p') => 1.0 / 12.0,
        _ => 1.0,
    };
    let scaled = number * factor;
    let rounded = if scaled.is_sign_positive() {
        (scaled + 0.4995) as isize
    } else {
        (scaled - 0.4995) as isize
    };
    Some(if rounded < 66 { rounded } else { 1 })
}

pub(super) fn append_terminal_indentation(
    output: &mut String,
    indentation: usize,
    maximum: usize,
) -> Result<(), RenderError> {
    if indentation > 0 {
        append(output, &" ".repeat(indentation), maximum)?;
    }
    Ok(())
}

/// Emit the physical lines requested by roff's `.sp`, including its final
/// line break. A zero-height scaled span still owns that one break; positive
/// spans add one blank line per terminal vertical unit. Negative spans defer
/// their effect: the reference renderer suppresses the next vertical spaces
/// rather than retracting output that has already been flushed.
pub(super) fn append_terminal_vertical_space(
    output: &mut String,
    span: isize,
    maximum: usize,
) -> Result<(), RenderError> {
    if output.is_empty() {
        return Ok(());
    }
    if span.is_negative() {
        for _ in 0..span.unsigned_abs() {
            mark_terminal_vertical_skip(output);
        }
        if !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    let requested = span.unsigned_abs();
    let emitted = (0..requested)
        .filter(|_| !take_terminal_vertical_skip(output))
        .count();
    let required = emitted.saturating_add(1);
    let trailing = output
        .chars()
        .rev()
        .take_while(|character| *character == '\n')
        .count();
    // `term_vspace()` is cumulative once an earlier vertical request has
    // completed the current physical line.  In particular, two adjacent
    // `.sp` requests produce two blank device lines rather than sharing one
    // already-present separator.  The first request still owns its terminal
    // line break below, which is why a text line starts at two newlines.
    if trailing >= 2 {
        for _ in 0..emitted {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    for _ in trailing..required {
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Consume one pending negative `.sp` adjustment, if any.  The markers live
/// immediately before the pending physical line break, leaving all ordinary
/// terminal layout predicates (`ends_with('\\n')`) unchanged.
pub(super) fn take_terminal_vertical_skip(output: &mut String) -> bool {
    let newline_start = output.trim_end_matches('\n').len();
    let prefix = &output[..newline_start];
    if prefix.ends_with(TERMINAL_VERTICAL_SKIP_MARKER) {
        let marker_start = newline_start - TERMINAL_VERTICAL_SKIP_MARKER.len_utf8();
        output.drain(marker_start..newline_start);
        true
    } else {
        false
    }
}

pub(super) fn mark_terminal_vertical_skip(output: &mut String) {
    let newline_start = output.trim_end_matches('\n').len();
    output.insert(newline_start, TERMINAL_VERTICAL_SKIP_MARKER);
}

pub(super) fn mark_terminal_table_vertical_skip(output: &mut String) {
    let newline_start = output.trim_end_matches('\n').len();
    output.insert(newline_start, TERMINAL_TABLE_VERTICAL_SKIP_MARKER);
}

pub(super) fn take_terminal_table_vertical_skip(output: &mut String) -> bool {
    take_terminal_table_vertical_skips(output) != 0
}

pub(super) fn take_terminal_table_vertical_skips(output: &mut String) -> usize {
    let newline_start = output.trim_end_matches('\n').len();
    let marker_width = TERMINAL_TABLE_VERTICAL_SKIP_MARKER.len_utf8();
    let mut marker_start = newline_start;
    let mut count = 0_usize;
    while marker_start >= marker_width
        && output[..marker_start].ends_with(TERMINAL_TABLE_VERTICAL_SKIP_MARKER)
    {
        marker_start -= marker_width;
        count += 1;
    }
    output.drain(marker_start..newline_start);
    count
}

/// Start the next rendered phrase on a roff `.ti` temporary column. The
/// marker remains private until `wrap_terminal_output`, where only that
/// phrase's first visual line receives the requested column.
pub(super) fn append_terminal_temporary_indent(
    output: &mut String,
    target: usize,
    maximum: usize,
) -> Result<(), RenderError> {
    if !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    append(
        output,
        &TERMINAL_TEMPORARY_INDENT_MARKER.to_string(),
        maximum,
    )?;
    append(output, &target.to_string(), maximum)?;
    append(
        output,
        &TERMINAL_TEMPORARY_INDENT_MARKER.to_string(),
        maximum,
    )
}

/// Start the next rendered phrase in a man hanging-paragraph field. Unlike
/// `.ti`, the current line retains its normal structural indentation while
/// every wrapped continuation uses the encoded target column.
pub(super) fn append_terminal_hanging_indent(
    output: &mut String,
    continuation: usize,
    maximum: usize,
) -> Result<(), RenderError> {
    append(output, &TERMINAL_HANGING_INDENT_MARKER.to_string(), maximum)?;
    append(output, &continuation.to_string(), maximum)?;
    append(output, &TERMINAL_HANGING_INDENT_MARKER.to_string(), maximum)
}

/// Give the current device line a distinct wrap continuation field.
///
/// The marker parser intentionally accepts hanging fields only at a physical
/// line's beginning. A `Bk` begins after already-rendered synopsis words, so
/// prepend rather than append the private marker before the line is wrapped.
pub(super) fn mark_terminal_hanging_indent(output: &mut String, continuation: usize) {
    let line_start = output.rfind('\n').map_or(0, |index| index + 1);
    output.insert_str(
        line_start,
        &format!("{TERMINAL_HANGING_INDENT_MARKER}{continuation}{TERMINAL_HANGING_INDENT_MARKER}"),
    );
}

/// Prefix each visible source line of a centered display with the renderer's
/// private centering marker.  Rendering the Body into its own buffer first
/// lets ordinary inline and block rules remain unchanged while the final
/// width pass sees the same device state on every physical display line.
pub(super) fn append_terminal_centered_lines(
    output: &mut String,
    centered: &str,
    maximum: usize,
) -> Result<(), RenderError> {
    for (index, line) in centered.split('\n').enumerate() {
        if index > 0 {
            append(output, "\n", maximum)?;
        }
        if !line.is_empty() {
            append(output, &TERMINAL_CENTER_MARKER.to_string(), maximum)?;
        }
        append(output, line, maximum)?;
    }
    Ok(())
}

/// Render the text children structurally attached to roff's `.ce` and `.rj`
/// requests.  They are presentation-only requests: the first child is their
/// line count, each remaining child is a physical no-fill line, and ordinary
/// prose resumes after the requested count.  The parser intentionally retains
/// both the request argument and the owned source texts for AST compatibility.
pub(super) fn render_terminal_adjusted_input_lines(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if terminal_has_visible_output(output) && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    let marker = if node.macro_name() == Some("rj") {
        TERMINAL_RIGHT_MARKER
    } else {
        TERMINAL_CENTER_MARKER
    };
    // `man.rs` already bounds attached text to the normalized positive count,
    // so skipping the count child here also correctly handles a recovered
    // empty request without producing a phantom terminal line.
    for child in node.children().skip(1) {
        let Some(text) = child.text() else {
            continue;
        };
        append(output, &marker.to_string(), maximum)?;
        append(output, &TERMINAL_NO_WRAP_MARKER.to_string(), maximum)?;
        // `rj` moves text to the device margin.  Centered input remains in
        // the enclosing field, matching term.c's distinct offset behavior.
        if node.macro_name() != Some("rj") {
            append(output, &" ".repeat(indentation), maximum)?;
        }
        let rendered =
            render_terminal_visible_text_with_font(text, format, limits, terminal_text_font(child));
        append(output, rendered.trim_end(), maximum)?;
        append(output, "\n", maximum)?;
    }
    Ok(())
}

pub(super) fn append_terminal_text(
    output: &mut String,
    text: &str,
    layout: TerminalTextLayout,
    indentation: usize,
    maximum: usize,
) -> Result<(), RenderError> {
    let break_replacement;
    let text = if text.contains(TERMINAL_PENDING_LINE_BREAK_MARKER) {
        break_replacement = format!("\n{}", " ".repeat(indentation));
        text.replace(TERMINAL_PENDING_LINE_BREAK_MARKER, &break_replacement)
    } else {
        text.to_owned()
    };
    let spacing_disabled = terminal_spacing_disabled(output);
    let visible_output = terminal_has_visible_output(output);
    let pending_special_indentation = output.ends_with([
        TERMINAL_TEMPORARY_INDENT_MARKER,
        TERMINAL_HANGING_INDENT_MARKER,
        TERMINAL_LINE_LENGTH_MARKER,
    ]);
    let empty_word = output.ends_with(TERMINAL_EMPTY_WORD_MARKER);
    if empty_word {
        let _ = output.pop();
    }
    let force_separator = output.ends_with(TERMINAL_FORCE_SEPARATOR_MARKER);
    if force_separator {
        let _ = output.pop();
    }
    let continue_source_line = output.ends_with(TERMINAL_CONTINUE_SOURCE_LINE_MARKER);
    if continue_source_line {
        let _ = output.pop();
    }
    let follows_no_fill_line = output
        .rsplit('\n')
        .next()
        .is_some_and(|line| line.starts_with(TERMINAL_NO_WRAP_MARKER));
    let attach_previous = output.ends_with(TERMINAL_ATTACH_NEXT_MARKER);
    if attach_previous {
        let _ = output.pop();
    }
    let mut pending_sentence = output.ends_with(TERMINAL_SENTENCE_PENDING_MARKER);
    if pending_sentence {
        let _ = output.pop();
    }
    let literal_punctuation = output.ends_with(TERMINAL_LITERAL_PUNCTUATION_MARKER);
    if literal_punctuation {
        let _ = output.pop();
    }
    if !attach_previous
        && !continue_source_line
        && !pending_special_indentation
        && (layout.line_start
            || (follows_no_fill_line && !layout.no_fill && !layout.no_fill_continuation))
        && visible_output
        && !output.ends_with('\n')
    {
        pending_sentence = false;
        append(output, "\n", maximum)?;
    } else if attach_previous || matches!(layout.join, TerminalJoin::Attach) {
        if output.ends_with(' ') {
            let _ = output.pop();
        }
    } else if empty_word && !output.is_empty() && !output.ends_with('\n') {
        // `Eo`/`Ec` can be a zero-width word. Its preceding separator was
        // already emitted, but the following visible word must receive its
        // own separator as well.
        append(
            output,
            &format!(" {TERMINAL_SENTENCE_SPACE_MARKER} "),
            maximum,
        )?;
    } else if (force_separator || continue_source_line)
        && !output.is_empty()
        && !output.ends_with('\n')
    {
        let separator = if pending_sentence {
            format!(" {TERMINAL_SENTENCE_SPACE_MARKER} ")
        } else {
            " ".to_owned()
        };
        append(output, &separator, maximum)?;
    } else if spacing_disabled {
        // `.Sm off` suppresses only ordinary word separation; explicit line
        // starts, parsed attachments, and structural field breaks above keep
        // their own terminal semantics.
    } else if !pending_special_indentation
        && visible_output
        // Literal punctuation is not layout state.  In particular, a roff
        // translation can leave visible `<<` at the end of a text node; the
        // next source phrase still needs its ordinary fill separator.  Only
        // the parser-informed private attachment marker denotes an opening
        // delimiter that owns the next word.
        && !output.ends_with([' ', '\n'])
    {
        let separator = if pending_sentence
            || output.chars().next_back().is_some_and(|character| {
                !literal_punctuation && matches!(character, '.' | '!' | '?')
            }) {
            " \u{1b} "
        } else {
            " "
        };
        append(output, separator, maximum)?;
    }
    let at_line_start = pending_special_indentation || !visible_output || output.ends_with('\n');
    if matches!(layout.tabs, TerminalTabLayout::PhysicalLiteral) {
        mark_terminal_line(output, TERMINAL_LITERAL_TAB_MARKER);
    }
    if layout.no_fill {
        mark_terminal_line(output, TERMINAL_NO_WRAP_MARKER);
    } else if layout.keep_spacing {
        mark_terminal_line(output, TERMINAL_KEEP_SPACING_MARKER);
    }
    if at_line_start {
        append_terminal_indentation(output, indentation, maximum)?;
    }
    append(output, &text, maximum)?;
    if layout.sentence_end || (pending_sentence && matches!(layout.join, TerminalJoin::Attach)) {
        append(
            output,
            &TERMINAL_SENTENCE_PENDING_MARKER.to_string(),
            maximum,
        )?;
    }
    if layout.literal_punctuation
        || (literal_punctuation && matches!(layout.join, TerminalJoin::Attach))
    {
        append(
            output,
            &TERMINAL_LITERAL_PUNCTUATION_MARKER.to_string(),
            maximum,
        )?;
    }
    Ok(())
}

pub(super) fn terminal_has_visible_text(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
) -> bool {
    let mut text = String::new();
    collect_terminal_text(node, format, limits, &mut text);
    !text.is_empty()
}

/// A tagged man field cannot share its tag line with Body content after an
/// explicit terminal break.  The Body can still contain visible prose later,
/// but its initial `.sp`/`.br` has already completed the tag's device line.
pub(super) fn terminal_body_starts_with_break(body: NodeRef<'_>) -> bool {
    body.children()
        .find(|child| !child.flags().no_print)
        .is_some_and(|child| matches!(child.macro_name(), Some("sp" | "br" | "PP" | "LP" | "Pp")))
}

/// `PD` owns no terminal glyphs, but it is still a physical body boundary
/// when it appears between a section heading and the next nested section.
pub(super) fn terminal_has_pd_control(node: NodeRef<'_>) -> bool {
    node.macro_name() == Some("PD") || node.children().any(terminal_has_pd_control)
}

pub(super) fn mark_terminal_attach_next(
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if !output.ends_with(TERMINAL_ATTACH_NEXT_MARKER) {
        append(output, &TERMINAL_ATTACH_NEXT_MARKER.to_string(), maximum)?;
    }
    Ok(())
}

/// Record a source-order `.ta` request on its own private terminal line.
///
/// Roff requests arrive at physical-line boundaries.  Keeping the state
/// marker standalone makes the next source line begin normally while the
/// width pass can remove the marker without manufacturing a blank output
/// line.  Individual arguments are already scanner-normalized AST text and
/// cannot contain the unit separator used by this bounded private encoding.
pub(super) fn append_terminal_tab_stops_request(
    node: NodeRef<'_>,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let request = node
        .children()
        .filter_map(NodeRef::text)
        .collect::<Vec<_>>()
        .join("\u{1f}");
    append_terminal_tab_stops_control(output, &request, maximum)
}

pub(super) fn append_terminal_tab_stops_control(
    output: &mut String,
    request: &str,
    maximum: usize,
) -> Result<(), RenderError> {
    if !output.is_empty() && !output.ends_with('\n') {
        append(output, "\n", maximum)?;
    }
    append(output, &TERMINAL_TAB_STOPS_MARKER.to_string(), maximum)?;
    append(output, request, maximum)?;
    append(output, &TERMINAL_TAB_STOPS_MARKER.to_string(), maximum)?;
    append(output, "\n", maximum)
}

pub(super) fn terminal_tab_stop_request(line: &str) -> Option<&str> {
    line.strip_prefix(TERMINAL_TAB_STOPS_MARKER)?
        .strip_suffix(TERMINAL_TAB_STOPS_MARKER)
}

pub(super) fn terminal_apply_tab_stop_request(tab_stops: &mut TerminalTabStops, request: &str) {
    *tab_stops = TerminalTabStops {
        configured: true,
        ..TerminalTabStops::default()
    };
    let mut periodic = false;
    for argument in request.split('\u{1f}') {
        if argument == "T" {
            periodic = true;
            continue;
        }
        let Some(width) = terminal_signed_roff_en_prefix(argument) else {
            continue;
        };
        let width = width.max(0).unsigned_abs();
        let positions = if periodic {
            &mut tab_stops.periodic
        } else {
            &mut tab_stops.absolute
        };
        let position = if argument.starts_with('+') {
            positions.last().copied().unwrap_or(0).saturating_add(width)
        } else {
            width
        };
        positions.push(position);
    }
}

pub(super) fn terminal_tab_next(tab_stops: &TerminalTabStops, previous: usize) -> usize {
    if let Some(position) = tab_stops
        .absolute
        .iter()
        .copied()
        .find(|position| previous < *position)
    {
        return position;
    }
    if tab_stops.periodic.is_empty() {
        return previous;
    }
    let cycle = *tab_stops.absolute.last().unwrap_or(&0);
    let period = *tab_stops.periodic.last().unwrap_or(&0);
    if period == 0 {
        return previous;
    }
    let mut base = cycle;
    while base.saturating_add(period) <= previous {
        base = base.saturating_add(period);
    }
    for position in &tab_stops.periodic {
        let position = base.saturating_add(*position);
        if previous < position {
            return position;
        }
    }
    previous
}

pub(super) fn mark_terminal_force_separator(
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if output.ends_with(TERMINAL_ATTACH_NEXT_MARKER) {
        let _ = output.pop();
    }
    if !output.ends_with(TERMINAL_FORCE_SEPARATOR_MARKER) {
        append(
            output,
            &TERMINAL_FORCE_SEPARATOR_MARKER.to_string(),
            maximum,
        )?;
    }
    Ok(())
}

/// `.Sm off` suppresses ordinary mdoc word spacing, but a later physical
/// phrase still observes the preceding sentence boundary.  Preserve that
/// narrow terminal state before forcing the source-line separator.
pub(super) fn mark_terminal_force_separator_after_sentence(
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if output.ends_with(TERMINAL_ATTACH_NEXT_MARKER) {
        let _ = output.pop();
    }
    let literal_punctuation = output.ends_with(TERMINAL_LITERAL_PUNCTUATION_MARKER);
    if !literal_punctuation
        && !output.ends_with(TERMINAL_SENTENCE_PENDING_MARKER)
        && output.ends_with(['.', '!', '?'])
    {
        append(
            output,
            &TERMINAL_SENTENCE_PENDING_MARKER.to_string(),
            maximum,
        )?;
    }
    if !output.ends_with(TERMINAL_FORCE_SEPARATOR_MARKER) {
        append(
            output,
            &TERMINAL_FORCE_SEPARATOR_MARKER.to_string(),
            maximum,
        )?;
    }
    Ok(())
}

pub(super) fn append_terminal_empty_word(
    output: &mut String,
    indentation: usize,
    maximum: usize,
) -> Result<(), RenderError> {
    let attached = output.ends_with(TERMINAL_ATTACH_NEXT_MARKER);
    append_terminal_text(
        output,
        "",
        TerminalTextLayout::default(),
        indentation,
        maximum,
    )?;
    let marker = if attached {
        TERMINAL_FORCE_SEPARATOR_MARKER
    } else {
        TERMINAL_EMPTY_WORD_MARKER
    };
    append(output, &marker.to_string(), maximum)
}

pub(super) fn terminal_sentence_terminator(text: &str) -> bool {
    text.trim_end()
        .chars()
        .next_back()
        .is_some_and(|character| {
            matches!(character, '.' | '!' | '?' | '"' | '\'' | ')' | ']' | '}')
        })
}

pub(super) fn mark_terminal_line(output: &mut String, marker: char) {
    let line_start = output.rfind('\n').map_or(0, |index| index + 1);
    // No-fill literal text needs both the no-wrap and literal-tab markers.
    // They are prepended in a fixed order, so a later text node continuing
    // that same physical line must recognise either marker rather than
    // inserting a duplicate behind the first one.
    let already_marked = output[line_start..]
        .chars()
        .take_while(|character| {
            matches!(
                *character,
                TERMINAL_NO_WRAP_MARKER
                    | TERMINAL_LITERAL_TAB_MARKER
                    | TERMINAL_KEEP_SPACING_MARKER
            )
        })
        .any(|character| character == marker);
    if !already_marked {
        output.insert(line_start, marker);
    }
}

/// Prefix the pending raw terminal line with its non-default `.ll` state.
/// The paired encoding makes the state unambiguous beside other one-byte
/// layout markers and is removed before caller-visible output is returned.
pub(super) fn mark_terminal_line_length(
    output: &mut String,
    state: TerminalLineLength,
    maximum: usize,
) -> Result<(), RenderError> {
    let encoded = match state {
        TerminalLineLength::Default => {
            format!("{TERMINAL_LINE_LENGTH_MARKER}D{TERMINAL_LINE_LENGTH_MARKER}")
        }
        TerminalLineLength::Absolute(value) => {
            format!("{TERMINAL_LINE_LENGTH_MARKER}A{value}{TERMINAL_LINE_LENGTH_MARKER}")
        }
        TerminalLineLength::Relative(value) => {
            format!("{TERMINAL_LINE_LENGTH_MARKER}R{value}{TERMINAL_LINE_LENGTH_MARKER}")
        }
    };
    let line_start = output.rfind('\n').map_or(0, |index| index + 1);
    let Some(relative_start) = output[line_start..].find(TERMINAL_LINE_LENGTH_MARKER) else {
        if matches!(state, TerminalLineLength::Default) {
            return Ok(());
        }
        if output.len().saturating_add(encoded.len()) > maximum {
            return Err(RenderError {
                kind: RenderErrorKind::OutputLimit,
                message: format!("rendered output exceeds {maximum} bytes").into(),
            });
        }
        output.insert_str(line_start, &encoded);
        return Ok(());
    };
    let marker_start = line_start + relative_start;
    let payload_start = marker_start + TERMINAL_LINE_LENGTH_MARKER.len_utf8();
    let Some(relative_end) = output[payload_start..].find(TERMINAL_LINE_LENGTH_MARKER) else {
        // An incomplete private marker can only arise while handling a
        // bounded-output error. It is discarded rather than leaked.
        output.truncate(marker_start);
        return Ok(());
    };
    let marker_end = payload_start + relative_end + TERMINAL_LINE_LENGTH_MARKER.len_utf8();
    let replaced = marker_end.saturating_sub(marker_start);
    let next_len = output
        .len()
        .saturating_sub(replaced)
        .saturating_add(encoded.len());
    if next_len > maximum {
        return Err(RenderError {
            kind: RenderErrorKind::OutputLimit,
            message: format!("rendered output exceeds {maximum} bytes").into(),
        });
    }
    output.replace_range(marker_start..marker_end, &encoded);
    Ok(())
}

pub(super) fn terminal_spacing_disabled(output: &str) -> bool {
    output.starts_with(TERMINAL_NO_SPACE_MARKER)
}

pub(super) fn terminal_has_visible_output(output: &str) -> bool {
    !output.is_empty() && !output.chars().eq(std::iter::once(TERMINAL_NO_SPACE_MARKER))
}

/// Apply mdoc's stateful spacing request. Valid `on` and `off` selectors are
/// retained as the Element's sole child. An argument-less request toggles the
/// state; parser recovery relinks an invalid same-line word after an empty
/// Element, which deliberately leaves the current state unchanged.
pub(super) fn terminal_apply_mdoc_spacing(
    node: NodeRef<'_>,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let requested = node
        .children()
        .find_map(NodeRef::text)
        .and_then(|value| match value {
            "on" => Some(true),
            "off" => Some(false),
            _ => None,
        });
    let invalid_argument = terminal_mdoc_sm_has_relinked_invalid_argument(node);
    // Both a bare request and a recovered invalid selector take the device's
    // toggle path. The invalid spelling itself is relinked as ordinary text;
    // it does not leave a separate renderer-only spacing mode behind.
    let enabled = requested.unwrap_or_else(|| terminal_spacing_disabled(output));
    if enabled {
        if terminal_spacing_disabled(output) {
            output.drain(..TERMINAL_NO_SPACE_MARKER.len_utf8());
        }
    } else if !terminal_spacing_disabled(output) {
        if output
            .len()
            .saturating_add(TERMINAL_NO_SPACE_MARKER.len_utf8())
            > maximum
        {
            return Err(RenderError {
                kind: RenderErrorKind::OutputLimit,
                message: format!("rendered output exceeds {maximum} bytes").into(),
            });
        }
        output.insert(0, TERMINAL_NO_SPACE_MARKER);
    }
    // Recovery leaves an invalid `.Sm bad` argument as the request's
    // immediate text sibling.  The request itself is invisible, but it still
    // closes the preceding filled phrase; keep the recovered word separate.
    if invalid_argument && terminal_has_visible_output(output) {
        mark_terminal_force_separator(output, maximum)?;
    } else if requested.is_some()
        && terminal_mdoc_sm_has_relinked_valid_argument(node)
        && terminal_has_visible_output(output)
    {
        // A valid selector's surplus words are relinked after the invisible
        // request. The first one starts the request's visible phrase, while
        // its following source line remains subject to `.Sm off`.
        mark_terminal_force_separator(output, maximum)?;
    }
    Ok(())
}

pub(super) fn terminal_mdoc_sm_has_relinked_invalid_argument(node: NodeRef<'_>) -> bool {
    if node
        .children()
        .find_map(NodeRef::text)
        .is_some_and(|argument| matches!(argument, "on" | "off"))
    {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    let Some(next) = parent
        .children()
        .skip_while(|sibling| sibling.id() != node.id())
        .nth(1)
    else {
        return false;
    };
    next.text().is_some()
        && node
            .source_position()
            .zip(next.source_position())
            .is_some_and(|(request, argument)| request.line == argument.line)
}

pub(super) fn terminal_mdoc_sm_has_relinked_valid_argument(node: NodeRef<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let Some(next) = parent
        .children()
        .skip_while(|sibling| sibling.id() != node.id())
        .nth(1)
    else {
        return false;
    };
    next.text().is_some()
        && node
            .source_position()
            .zip(next.source_position())
            .is_some_and(|(request, argument)| request.line == argument.line)
}

pub(super) fn terminal_mdoc_sm_relinked_valid_argument(node: NodeRef<'_>) -> bool {
    terminal_mdoc_sm_relink_before(node) == Some(TerminalMdocSmRelink::Valid)
}

pub(super) fn terminal_mdoc_sm_relinked_invalid_argument(node: NodeRef<'_>) -> bool {
    terminal_mdoc_sm_relink_before(node) == Some(TerminalMdocSmRelink::Invalid)
}

pub(super) fn terminal_mdoc_sm_relinked_argument_precedes(node: NodeRef<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    parent
        .children()
        .take_while(|sibling| sibling.id() != node.id())
        .last()
        .is_some_and(|previous| terminal_mdoc_sm_relink_before(previous).is_some())
}

/// Classify a word the parser detached from a same-line `.Sm` request.
///
/// The valid and invalid paths look similar in the public AST, but their
/// terminal spacing differs: valid `off two` retains `two` as the first
/// no-space phrase, while recovery for `bad two` resumes ordinary word flow.
pub(super) fn terminal_mdoc_sm_relink_before(node: NodeRef<'_>) -> Option<TerminalMdocSmRelink> {
    node.text()?;
    let target = node.source_position()?;
    let parent = node.parent()?;
    let preceding = parent
        .children()
        .take_while(|sibling| sibling.id() != node.id())
        .collect::<Vec<_>>();
    for sibling in preceding.into_iter().rev() {
        let Some(position) = sibling.source_position() else {
            continue;
        };
        if position.line != target.line {
            break;
        }
        if sibling.kind() != NodeKind::Element || sibling.macro_name() != Some("Sm") {
            continue;
        }
        return Some(
            if sibling
                .children()
                .find_map(NodeRef::text)
                .is_some_and(|argument| matches!(argument, "on" | "off"))
            {
                TerminalMdocSmRelink::Valid
            } else {
                TerminalMdocSmRelink::Invalid
            },
        );
    }
    None
}

pub(super) fn terminal_mdoc_sm_starts_new_source_phrase(node: NodeRef<'_>) -> bool {
    if !node.flags().line_start {
        return false;
    }
    match node.kind() {
        NodeKind::Text => true,
        NodeKind::Element => !matches!(
            node.macro_name(),
            Some("Pp" | "PP" | "LP" | "sp" | "br" | "Sm" | "Tg" | "Es" | "ft" | "po" | "ll" | "in")
        ),
        // An `Op` block at an input-line boundary begins a visible optional
        // phrase. Under `.Sm off` its opening bracket still receives the
        // one source-phrase separator, while nested same-line options do
        // not manufacture one.
        NodeKind::Block => node.macro_name() == Some("Op"),
        _ => false,
    }
}

/// Return the mdoc word-spacing state effective at `node`'s source position.
///
/// Terminal rendering normally carries `.Sm` state in its private output
/// buffer.  Some presentation paths first collect an enclosure or a styled
/// macro into a separate string, though, so that buffer is deliberately not
/// available there. Replaying the tiny state machine from the immutable tree
/// keeps those nested phrases faithful without making the public AST carry a
/// renderer-only control bit.
pub(super) fn terminal_mdoc_spacing_disabled_before(node: NodeRef<'_>) -> bool {
    let Some(target) = node.source_position() else {
        return false;
    };
    let mut root = node;
    while let Some(parent) = root.parent() {
        root = parent;
    }

    let mut spacing_enabled = true;
    let mut pending = vec![root];
    while let Some(current) = pending.pop() {
        if current.kind() == NodeKind::Element
            && current.macro_name() == Some("Sm")
            && current
                .source_position()
                .is_some_and(|position| terminal_source_position_precedes(position, target))
        {
            match current.children().find_map(NodeRef::text) {
                Some("on") => {
                    spacing_enabled = true;
                }
                Some("off") => {
                    spacing_enabled = false;
                }
                None => {
                    spacing_enabled = !spacing_enabled;
                }
                Some(_) => {}
            }
        }
        let children = current.children().collect::<Vec<_>>();
        pending.extend(children.into_iter().rev());
    }
    !spacing_enabled
}

pub(super) fn terminal_source_position_precedes(
    position: crate::SourcePosition,
    target: crate::SourcePosition,
) -> bool {
    (position.line, position.column) < (target.line, target.column)
}

pub(super) fn collect_terminal_text(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    output: &mut String,
) {
    if node.flags().no_print || node.ancestors().any(|ancestor| ancestor.flags().no_print) {
        return;
    }
    if node.macro_name() == Some("PD") {
        if node.kind() == NodeKind::Block {
            for body in node
                .children()
                .filter(|child| child.kind() == NodeKind::Body)
            {
                collect_terminal_text(body, format, limits, output);
            }
        }
        return;
    }
    if let Some(closing) = terminal_embedded_quote_closing(node, format) {
        let font = if node
            .ancestors()
            .any(|ancestor| ancestor.macro_name() == Some("Bf"))
        {
            TerminalFont::Roman
        } else {
            terminal_inherited_font(node)
        };
        output.push_str(&render_terminal_font(closing, font));
        return;
    }
    if matches!(
        node.macro_name(),
        Some("Es" | "Sm" | "Tg" | "ft" | "po" | "ll" | "in" | "sp" | "br" | "ta")
    ) {
        return;
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("Bf") {
        // A Bf Head is formatter configuration, not phrase text.  This
        // collector is reached from explicit enclosures and list terms as
        // well as the top-level walker, so mirror the terminal dispatch here
        // instead of leaking its normalized `Em`/`Li`/`Sy` argument.
        if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
            collect_terminal_text(body, format, limits, output);
        }
        return;
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("Eo") {
        // Collection is used inside a surrounding quote/list phrase, where
        // the top-level Eo dispatcher is intentionally bypassed.  Preserve
        // the explicit Head/Body/Tail attachment here as well: Eo's opening
        // delimiter attaches to its Body, and its recovered Ec Tail attaches
        // back to that Body without also swallowing the following phrase.
        let mut tail = None;
        let mut has_head_or_body = false;
        let mut embedded_outer_closer = false;
        let has_visible_tail = node.children().any(|child| {
            child.kind() == NodeKind::Tail && terminal_has_visible_text(child, format, limits)
        });
        for child in node.children() {
            match child.kind() {
                NodeKind::Head => {
                    let visible = terminal_has_visible_text(child, format, limits);
                    has_head_or_body |= visible;
                    for nested in child.children() {
                        collect_terminal_text(nested, format, limits, output);
                    }
                    if visible {
                        output.push(TERMINAL_ATTACH_NEXT_MARKER);
                    }
                }
                NodeKind::Body => {
                    // A recovered `Bc` nested in Eo is represented as an
                    // empty `Body(Bo)` child. It emits the *outer* quote's
                    // closing bracket at this source point, but does not
                    // make an otherwise empty Eo own content for Ec-tail
                    // attachment purposes.
                    let has_embedded_closer = child
                        .children()
                        .any(|nested| terminal_embedded_quote_closing(nested, format).is_some());
                    let has_own_content = child.children().any(|nested| {
                        terminal_embedded_quote_closing(nested, format).is_none()
                            && terminal_has_visible_text(nested, format, limits)
                    });
                    if has_embedded_closer
                        && !has_head_or_body
                        && !has_own_content
                        && has_visible_tail
                    {
                        // An empty Eo survives only to close after the
                        // surrounding partial block. Preserve the blank
                        // before that outer closer, then attach Ec to it.
                        if !output.ends_with(' ') {
                            output.push(' ');
                        }
                        embedded_outer_closer = true;
                    }
                    has_head_or_body |= has_own_content;
                    for nested in child.children() {
                        collect_terminal_text(nested, format, limits, output);
                    }
                }
                NodeKind::Tail => tail = Some(child),
                _ => {}
            }
        }
        let has_tail = tail.is_some_and(|tail| terminal_has_visible_text(tail, format, limits));
        if let Some(tail) = tail.filter(|_| has_tail) {
            if has_head_or_body || embedded_outer_closer {
                output.push(TERMINAL_ATTACH_NEXT_MARKER);
            } else {
                // Eo may survive only as the owner of a late Ec after an
                // enclosing quote has already closed.  That closer starts a
                // new phrase; never inherit the enclosing quote's old
                // attachment marker.
                if output.ends_with(TERMINAL_ATTACH_NEXT_MARKER) {
                    let _ = output.pop();
                }
                if !output.ends_with(' ') {
                    output.push(' ');
                }
            }
            for nested in tail.children() {
                collect_terminal_text(nested, format, limits, output);
            }
        } else if has_head_or_body {
            if output.ends_with(TERMINAL_ATTACH_NEXT_MARKER) {
                let _ = output.pop();
            }
            // Unlike the top-level renderer, this collector returns one
            // already-assembled phrase.  Emit the separator directly so the
            // following collected text cannot consume Eo's old attachment.
            if !output.ends_with(' ') {
                output.push(' ');
            }
        } else {
            output.push(TERMINAL_EMPTY_WORD_MARKER);
        }
        return;
    }
    if node.kind() == NodeKind::Body
        && node.macro_name() == Some("Eo")
        && node
            .parent()
            .is_some_and(|parent| parent.macro_name() != Some("Eo"))
    {
        // When an Eo closes while another partial block owns the active
        // body, mandoc retains Ec as an Eo Body nested at that exact source
        // position rather than as the outer block's Tail.  It is still a
        // closing delimiter: attach it to the preceding phrase, but let the
        // following sibling receive its ordinary separator.
        if node
            .children()
            .any(|child| terminal_has_visible_text(child, format, limits))
        {
            output.push(TERMINAL_ATTACH_NEXT_MARKER);
            for child in node.children() {
                collect_terminal_text(child, format, limits, output);
            }
        } else {
            // A bare Ec has no delimiter payload. It closes Eo's attachment,
            // but it must not attach the next word to the preceding phrase.
            if output.ends_with(TERMINAL_ATTACH_NEXT_MARKER) {
                let _ = output.pop();
            }
            if !output.ends_with(' ') {
                output.push(' ');
            }
        }
        return;
    }
    if is_mdoc_description_block(node) {
        // A broken or explicitly enclosed `.Nd` can be collected through a
        // surrounding quote/list phrase instead of the normal top-level
        // block dispatcher.  Its Body alone omits the device's description
        // separator, so reproduce the small inline form here.
        let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
            return;
        };
        let mut description = String::new();
        collect_terminal_text(body, format, limits, &mut description);
        if !output.is_empty()
            && !output.ends_with([' ', TERMINAL_ATTACH_NEXT_MARKER, '(', '[', '{', '<'])
        {
            output.push(' ');
        }
        if matches!(format, RenderFormat::Utf8) {
            output.push('–');
        } else {
            output.push('-');
        }
        if !description.is_empty() {
            output.push(' ');
            output.push_str(&description);
        }
        return;
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("Op")
        && let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body)
        && let Some((opening, closing)) = terminal_quote_delimiters(node, Some(body), format)
    {
        // A nested optional phrase is collected into its enclosing terminal
        // quote Body rather than walked through the top-level dispatcher.
        // Preserve its own brackets here; `.Sm off` still controls only the
        // gap before this source phrase, not the nested macro contents.
        if !terminal_mdoc_spacing_disabled_before(node)
            && !output.is_empty()
            && !output.ends_with([' ', TERMINAL_ATTACH_NEXT_MARKER, '(', '[', '{', '<'])
        {
            output.push(' ');
        }
        let opening = render_terminal_font(opening, terminal_inherited_font(node));
        let closing = if terminal_quote_has_embedded_closer(body, node.macro_name()) {
            String::new()
        } else {
            render_terminal_font(closing, terminal_inherited_font(node))
        };
        output.push_str(&opening);
        collect_terminal_text(body, format, limits, output);
        output.push_str(&closing);
        return;
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("En")
        && let Some(enclosure) = node.enclosure()
    {
        // The obsolete `Es` request stores its resolved delimiters on each
        // later `En` block.  These blocks are often collected as a phrase,
        // where walking only their Body would silently discard that state.
        if !terminal_mdoc_spacing_disabled_before(node)
            && !output.is_empty()
            && !output.ends_with([' ', TERMINAL_ATTACH_NEXT_MARKER, '(', '[', '{', '<'])
        {
            output.push(' ');
        }
        for leading in node
            .children()
            .filter(|child| child.kind() == NodeKind::Head || child.flags().delimiter_open)
        {
            collect_terminal_text(leading, format, limits, output);
        }
        output.push_str(&enclosure.opening);
        if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
            collect_terminal_text(body, format, limits, output);
        }
        if let Some(closing) = &enclosure.closing {
            output.push_str(closing);
        }
        return;
    }
    if node.kind() == NodeKind::Block
        && let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body)
        && let Some((opening, closing)) = terminal_quote_delimiters(node, Some(body), format)
    {
        // Collection is also used for list terms and other partial syntax
        // regions that bypass the top-level block dispatcher.  Retain an
        // ordinary explicit quote scope here rather than flattening it to
        // its Body words; otherwise an `Ao` extended item head, for example,
        // loses its visible angle brackets.
        if !terminal_mdoc_spacing_disabled_before(node)
            && !output.is_empty()
            && !output.ends_with([' ', TERMINAL_ATTACH_NEXT_MARKER, '(', '[', '{', '<'])
        {
            output.push(' ');
        }
        let opening = render_terminal_font(opening, terminal_inherited_font(node));
        let closing = if terminal_quote_has_embedded_closer(body, node.macro_name()) {
            String::new()
        } else {
            render_terminal_font(closing, terminal_inherited_font(node))
        };
        output.push_str(&opening);
        collect_terminal_text(body, format, limits, output);
        output.push_str(&closing);
        for tail in node
            .children()
            .filter(|child| child.kind() == NodeKind::Tail)
        {
            collect_terminal_text(tail, format, limits, output);
        }
        return;
    }
    if node.kind() == NodeKind::Element && node.macro_name() == Some("OP") {
        output.push_str(&terminal_man_option(node, format, limits));
        return;
    }
    if node.kind() == NodeKind::Element && node.macro_name() == Some("Pf") {
        for child in node.children() {
            collect_terminal_text(child, format, limits, output);
        }
        if terminal_mdoc_prefix_attaches_to_following_token(node) {
            output.push(TERMINAL_ATTACH_NEXT_MARKER);
        }
        return;
    }
    if node.kind() == NodeKind::Element
        && let Some(fonts) = terminal_man_alternating_fonts(node.macro_name())
    {
        for (index, child) in node.children().enumerate() {
            let mut fragment = String::new();
            collect_terminal_semantic_text(
                child,
                format,
                limits,
                fonts[index % fonts.len()],
                &mut fragment,
            );
            output.push_str(&fragment);
        }
        return;
    }
    if node.kind() == NodeKind::Element
        && let Some(font) = match node.macro_name() {
            Some("B") => Some(TerminalFont::Bold),
            Some("I") => Some(TerminalFont::Italic),
            Some("R") => Some(TerminalFont::Roman),
            _ => None,
        }
    {
        collect_terminal_semantic_text(node, format, limits, font, output);
        return;
    }
    if node.kind() == NodeKind::Element && node.macro_name() == Some("Nm") {
        // Collection paths (notably an `Nd` nested in an explicit quote)
        // bypass the top-level Nm dispatcher.  Preserve Nm's bold base font
        // here instead of flattening its child text to ordinary prose.
        let mut phrase = String::new();
        collect_terminal_semantic_text(node, format, limits, TerminalFont::Bold, &mut phrase);
        if !phrase.is_empty() {
            if !terminal_mdoc_spacing_disabled_before(node)
                && !output.is_empty()
                && !output.ends_with([' ', TERMINAL_ATTACH_NEXT_MARKER, '(', '[', '{', '<'])
            {
                output.push(' ');
            }
            output.push_str(&phrase);
        }
        return;
    }
    if node.kind() == NodeKind::Element
        && let Some(font) = terminal_mdoc_element_font(node)
    {
        let mut phrase = String::new();
        collect_terminal_semantic_text(node, format, limits, font, &mut phrase);
        let empty_flag = node.macro_name() == Some("Fl") && node.children().next().is_none();
        if node.macro_name() == Some("Fl")
            && (phrase.is_empty() || node.children().next().is_some())
        {
            phrase.insert_str(0, &render_terminal_font("-", font));
        }
        if !phrase.is_empty() {
            if !terminal_mdoc_spacing_disabled_before(node)
                && !output.is_empty()
                && !output.ends_with([' ', TERMINAL_ATTACH_NEXT_MARKER, '(', '[', '{', '<'])
            {
                output.push(' ');
            }
            output.push_str(&phrase);
            if empty_flag && terminal_mdoc_empty_fl_attaches_to_following_macro(node) {
                output.push(TERMINAL_ATTACH_NEXT_MARKER);
            }
        }
        return;
    }
    if let Some(text) = node.text() {
        let sentence_boundary = node.flags().sentence_end
            && terminal_sentence_terminator(text)
            && terminal_mdoc_plain_text_sentence(node)
            && !node.flags().delimiter_close
            && terminal_next_visible_sibling(node).is_some_and(|next| {
                // A later explicit enclosure is still ordinary terminal
                // prose from the preceding plain sentence's perspective.
                // The collector otherwise flattens it before the final
                // layout call can see that transition.
                next.kind() == NodeKind::Text || next.macro_name() == Some("Ao")
            });
        if node.flags().delimiter_close
            || (terminal_closing_punctuation(text)
                && !node
                    .ancestors()
                    .any(|ancestor| ancestor.macro_name() == Some("Pf")))
        {
            if output.ends_with(' ') {
                let _ = output.pop();
            }
        } else if !output.is_empty()
            && !output.ends_with([' ', TERMINAL_ATTACH_NEXT_MARKER])
            && !terminal_mdoc_spacing_disabled_before(node)
            // A parsed opening delimiter owns the following phrase's
            // adjacency. The collector is used for partial-block bodies,
            // where that parser flag has already been consumed into the
            // visible punctuation spelling.
            && !output.ends_with(['(', '[', '{', '<'])
        {
            output.push(' ');
        }
        output.push_str(&render_terminal_visible_text_with_font(
            text,
            format,
            limits,
            terminal_inherited_font(node),
        ));
        if node.flags().line_continuation && !text.ends_with("\\z\\c") {
            output.push(TERMINAL_ATTACH_NEXT_MARKER);
        }
        if sentence_boundary {
            // This collector supplies one assembled phrase to tag/list and
            // enclosure renderers. Preserve the device's sentence token
            // explicitly, since the final `append_terminal_text()` call no
            // longer sees the original node boundary.
            output.push(' ');
            output.push(TERMINAL_SENTENCE_SPACE_MARKER);
            output.push(' ');
        }
    }
    for child in node.children() {
        collect_terminal_text(child, format, limits, output);
    }
}

pub(super) fn terminal_closing_punctuation(text: &str) -> bool {
    matches!(text, "." | "," | ";" | ":" | "!" | "?" | ")" | "]" | "}")
}

/// Render mdoc's fixed two-argument cross-reference form.  The parser keeps
/// its target and section as individual children for navigation; the terminal
/// device presents them as one `name(section)` phrase.
pub(super) fn terminal_cross_reference(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
) -> Option<String> {
    let mut arguments = node.children().filter(|child| !child.flags().no_print);
    let mut name = String::new();
    collect_terminal_text(arguments.next()?, format, limits, &mut name);
    if name.is_empty() {
        return None;
    }
    let mut section = String::new();
    let Some(section_argument) = arguments.next() else {
        return Some(name);
    };
    collect_terminal_text(section_argument, format, limits, &mut section);
    if section.is_empty() {
        Some(name)
    } else {
        Some(format!("{name}({section})"))
    }
}

/// Collect a SYNOPSIS `.Nm` Head.  Most of the head is a bold semantic name,
/// but a partial quote block can be nested inside it when the parser closes
/// the implicit Nm block around another mdoc macro.  The normal semantic
/// collector intentionally flattens syntax blocks; this presentation-only
/// path preserves the nested terminal delimiters without changing that AST.
pub(super) fn collect_terminal_mdoc_synopsis_name_head(
    head: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    output: &mut String,
) {
    for child in head.children() {
        if child.kind() == NodeKind::Block
            && let Some(body) = child
                .children()
                .find(|nested| nested.kind() == NodeKind::Body)
            && let Some((opening, closing)) = terminal_quote_delimiters(child, Some(body), format)
        {
            if !output.is_empty() && !output.ends_with(' ') {
                output.push(' ');
            }
            output.push_str(&render_terminal_font(opening, TerminalFont::Bold));
            let mut contents = String::new();
            collect_terminal_semantic_text(body, format, limits, TerminalFont::Bold, &mut contents);
            output.push_str(&contents);
            output.push_str(&render_terminal_font(closing, TerminalFont::Bold));
        } else {
            collect_terminal_semantic_text(child, format, limits, TerminalFont::Bold, output);
        }
    }
}

/// Render mdoc's hyperlink form.  Its first argument is the URL; remaining
/// arguments are a human-readable label.  The terminal device displays the
/// label first in italic, followed by a Roman colon and the URL in bold.  A
/// delimiter parsed as a separate final label child belongs after the URL.
pub(super) fn terminal_link(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
) -> Option<String> {
    let mut arguments = node.children().filter(|child| !child.flags().no_print);
    let target = arguments.next()?;
    let mut target_text = String::new();
    collect_terminal_semantic_text(target, format, limits, TerminalFont::Bold, &mut target_text);
    if target_text.is_empty() {
        return None;
    }

    let mut label_arguments = arguments.collect::<Vec<_>>();
    if label_arguments.is_empty() {
        return Some(target_text);
    }

    // `.Lk url label ,` tokenizes the comma as a delimiter child of the
    // label. `term.c` moves that delimiter after its rendered URL, while a
    // comma authored directly in a word (for example `label,`) stays within
    // the italic label as parsed.
    let delimiter = if label_arguments
        .last()
        .is_some_and(|argument| argument.flags().delimiter_close)
    {
        label_arguments.pop()
    } else {
        None
    };

    let mut label = String::new();
    for argument in label_arguments {
        collect_terminal_semantic_text(argument, format, limits, TerminalFont::Italic, &mut label);
    }
    if label.is_empty() {
        if let Some(delimiter) = delimiter {
            let mut trailing = String::new();
            collect_terminal_semantic_text(
                delimiter,
                format,
                limits,
                TerminalFont::Roman,
                &mut trailing,
            );
            target_text.push_str(&trailing);
        }
        return Some(target_text);
    }

    let mut rendered = format!("{label}: {target_text}");
    if let Some(delimiter) = delimiter {
        let mut trailing = String::new();
        collect_terminal_semantic_text(
            delimiter,
            format,
            limits,
            TerminalFont::Roman,
            &mut trailing,
        );
        rendered.push_str(&trailing);
    }
    Some(rendered)
}

/// Render an mdoc `Rs` bibliography block as one terminal reference.  The
/// parser has already normalized direct `%` fields into the reference order;
/// terminal presentation adds the package-specific author conjunction,
/// typography, separators, and final sentence punctuation.
pub(super) fn render_terminal_reference_block(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if terminal_mdoc_section_named(node, "SEE ALSO")
        && terminal_has_visible_preceding_sibling(node, format, limits)
    {
        append_blank_line(output, maximum)?;
    }
    let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) else {
        return Ok(());
    };
    let fields = body
        .children()
        .filter(|child| !child.flags().no_print)
        .collect::<Vec<_>>();
    let has_journal = fields.iter().any(|field| field.macro_name() == Some("%J"));
    let mut fields_after_authors = Vec::new();
    let mut authors = Vec::new();
    let mut direct_prefix = Vec::new();
    for field in &fields {
        if field.macro_name() == Some("%A") {
            let mut author = String::new();
            collect_terminal_text(*field, format, limits, &mut author);
            if !author.is_empty() {
                authors.push(author);
            }
        } else if let Some(phrase) = terminal_reference_field(*field, format, limits, has_journal) {
            fields_after_authors.push(phrase);
        } else {
            let mut direct = String::new();
            if let Some(font) = terminal_mdoc_element_font(*field) {
                collect_terminal_semantic_text(*field, format, limits, font, &mut direct);
            } else {
                collect_terminal_text(*field, format, limits, &mut direct);
            }
            if !direct.is_empty() {
                direct_prefix.push(direct);
            }
        }
    }
    if direct_prefix.is_empty() && authors.is_empty() && fields_after_authors.is_empty() {
        return Ok(());
    }
    let mut reference = direct_prefix.join(" ");
    if !authors.is_empty() {
        if !reference.is_empty() {
            reference.push(' ');
        }
        reference.push_str(&terminal_reference_authors(&authors));
    }
    for phrase in &fields_after_authors {
        if !reference.is_empty() {
            reference.push_str(", ");
        }
        reference.push_str(phrase);
    }
    reference.push('.');
    append_terminal_text(
        output,
        &reference,
        TerminalTextLayout {
            sentence_end: true,
            ..TerminalTextLayout::default()
        },
        indentation,
        maximum,
    )
}

pub(super) fn terminal_has_visible_preceding_sibling(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
) -> bool {
    node.parent().is_some_and(|parent| {
        parent
            .children()
            .take_while(|sibling| sibling.id() != node.id())
            .any(|sibling| terminal_has_visible_text(sibling, format, limits))
    })
}

pub(super) fn terminal_reference_authors(authors: &[String]) -> String {
    match authors {
        [] => String::new(),
        [author] => author.clone(),
        [first, second] => format!("{first} and {second}"),
        _ => {
            let mut output = authors[..authors.len() - 1].join(", ");
            output.push_str(", and ");
            output.push_str(authors.last().expect("nonempty author list"));
            output
        }
    }
}

pub(super) fn terminal_reference_field(
    field: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    has_journal: bool,
) -> Option<String> {
    let macro_name = field.macro_name()?;
    if !matches!(
        macro_name,
        "%B" | "%C" | "%D" | "%I" | "%J" | "%N" | "%O" | "%P" | "%Q" | "%R" | "%T" | "%U" | "%V"
    ) {
        return None;
    }
    let mut value = String::new();
    let font = match macro_name {
        "%B" | "%I" | "%J" => Some(TerminalFont::Italic),
        "%T" if !has_journal => Some(TerminalFont::Italic),
        _ => None,
    };
    if let Some(font) = font {
        collect_terminal_semantic_text(field, format, limits, font, &mut value);
    } else {
        collect_terminal_text(field, format, limits, &mut value);
    }
    if value.is_empty() {
        return None;
    }
    if macro_name == "%T" && has_journal {
        let (open, close) = if matches!(format, RenderFormat::Utf8) {
            ("“", "”")
        } else {
            ("\"", "\"")
        };
        value = format!("{open}{value}{close}");
    }
    Some(value)
}

pub(super) fn collect_terminal_inline_text(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    output: &mut String,
) {
    let children = node.children().collect::<Vec<_>>();
    for (index, child) in children.iter().copied().enumerate() {
        // `\c` is collected as the same private attachment marker used by
        // ordinary terminal flow.  A man font element applies its style only
        // after collecting all of its arguments, so consume that marker here
        // before introducing this helper's otherwise-normal inter-argument
        // separator; rendering the marker in bold would turn it into a
        // visible overstrike space.
        let attach_previous = output.ends_with(TERMINAL_ATTACH_NEXT_MARKER);
        if attach_previous {
            let _ = output.pop();
        }
        if index > 0 && !output.is_empty() && !attach_previous {
            let separator = if children[index - 1].separator_after() == Some(b'\t') {
                "\t"
            } else {
                " "
            };
            output.push_str(separator);
        }
        let mut fragment = String::new();
        collect_terminal_text(child, format, limits, &mut fragment);
        output.push_str(&fragment);
    }
}

/// Collect mdoc macro arguments with the macro's semantic font as their
/// initial terminal state. Source `\f` controls then switch away from and
/// back to that state, rather than being discarded or overriding the whole
/// phrase. This is distinct from ordinary prose, whose initial state is Roman.
pub(super) fn collect_terminal_semantic_text(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    font: TerminalFont,
    output: &mut String,
) {
    if node.flags().no_print || node.ancestors().any(|ancestor| ancestor.flags().no_print) {
        return;
    }
    if node.macro_name() == Some("PD") {
        if node.kind() == NodeKind::Block {
            for body in node
                .children()
                .filter(|child| child.kind() == NodeKind::Body)
            {
                collect_terminal_semantic_text(body, format, limits, font, output);
            }
        }
        return;
    }
    if matches!(node.macro_name(), Some("Es" | "Sm" | "Tg")) {
        return;
    }
    if node.kind() == NodeKind::Element && node.macro_name() == Some("Pf") {
        for child in node.children() {
            collect_terminal_semantic_text(child, format, limits, font, output);
        }
        if terminal_mdoc_prefix_attaches_to_following_token(node) {
            output.push(TERMINAL_ATTACH_NEXT_MARKER);
        }
        return;
    }
    // A man font request can remain open across following request lines. The
    // AST therefore nests the later request below the opener (for example a
    // blank `.B` followed by `.I next-line` in a TP Head). Descendant font
    // requests override that inherited device state rather than receiving the
    // outer font a second time.
    let font = match (node.kind(), node.macro_name()) {
        (NodeKind::Element, Some("B")) => TerminalFont::Bold,
        (NodeKind::Element, Some("I")) => TerminalFont::Italic,
        (NodeKind::Element, Some("R")) => TerminalFont::Roman,
        _ => font,
    };
    if let Some(text) = node.text() {
        if !terminal_mdoc_spacing_disabled_before(node)
            && !output.is_empty()
            && !output.ends_with([' ', TERMINAL_ATTACH_NEXT_MARKER])
        {
            output.push(' ');
        }
        let rendered = render_terminal_visible_text_with_font(text, format, limits, font);
        output.push_str(&terminal_quoted_trailing_spaces(node, rendered));
        if node.flags().line_continuation && !text.ends_with("\\z\\c") {
            output.push(TERMINAL_ATTACH_NEXT_MARKER);
        }
    }
    for child in node.children() {
        collect_terminal_semantic_text(child, format, limits, font, output);
    }
}

/// Keep blanks that belong to a quoted mdoc macro argument through the filled
/// width pass.  Ordinary whitespace splitting is correct for source layout,
/// but would collapse the public-AST spelling of `.Fl "one " "two "`.
/// A private nonbreaking marker remains one terminal cell and is converted
/// back to a literal blank only after wrapping has completed.
pub(super) fn terminal_quoted_trailing_spaces(node: NodeRef<'_>, mut rendered: String) -> String {
    if !node.argument_quoted() {
        return rendered;
    }
    let trailing_start = rendered.trim_end_matches(' ').len();
    if trailing_start < rendered.len() {
        let count = rendered[trailing_start..].chars().count();
        rendered.replace_range(
            trailing_start..,
            &TERMINAL_NONBREAKING_SPACE_MARKER.to_string().repeat(count),
        );
    }
    rendered
}

/// Retain an authored interior run of spaces without turning the complete
/// terminal line into a no-wrap line.  The width pass treats the private
/// marker as one visible cell and restores it to a blank after choosing its
/// normal line breaks.
pub(super) fn terminal_internal_spaces_to_nonbreaking(rendered: &str) -> String {
    let mut output = String::with_capacity(rendered.len());
    let mut previous_was_space = false;
    for character in rendered.chars() {
        if character == ' ' && previous_was_space {
            output.push(TERMINAL_NONBREAKING_SPACE_MARKER);
        } else {
            output.push(character);
        }
        previous_was_space = character == ' ';
    }
    output
}

/// Encode the stable terminal-device bold convention. Both upstream ASCII and
/// UTF-8 terminal outputs use overstriking (`X\\bX`), while HTML follows its
/// independent DOM path. It needs no terminal-capability probing and remains
/// deterministic in a library call.
pub(super) fn render_terminal_bold(value: &str, _format: RenderFormat) -> String {
    render_terminal_font(value, TerminalFont::Bold)
}

pub(super) fn render_terminal_font(value: &str, font: TerminalFont) -> String {
    if matches!(font, TerminalFont::Roman) {
        return value.replace(TERMINAL_ZERO_WIDTH_BACKSPACE_MARKER, "\u{8}");
    }
    let mut output = String::with_capacity(value.len().saturating_mul(3));
    for character in value.chars() {
        if character.is_whitespace()
            || character == '\u{8}'
            || character == TERMINAL_NONBREAKING_SPACE_MARKER
            || character == TERMINAL_PENDING_LINE_BREAK_MARKER
        {
            output.push(character);
        } else if character == TERMINAL_ZERO_WIDTH_BACKSPACE_MARKER {
            output.push('\u{8}');
        } else {
            match font {
                TerminalFont::Roman => output.push(character),
                TerminalFont::Bold => {
                    output.push(character);
                    output.push('\u{8}');
                    output.push(character);
                }
                TerminalFont::Italic => {
                    output.push('_');
                    output.push('\u{8}');
                    output.push(character);
                }
                TerminalFont::BoldItalic => {
                    output.push('_');
                    output.push('\u{8}');
                    output.push(character);
                    output.push('\u{8}');
                    output.push(character);
                }
            }
        }
    }
    output
}

pub(super) mod layout;
use layout::{
    append_blank_line, display_width, render_visible_text, terminal_line_length_value,
    wrap_terminal_output,
};
