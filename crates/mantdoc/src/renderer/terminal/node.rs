use super::{
    AuthorMode, DEFAULT_RENDER_WIDTH, DisplayKind, Limits, NodeKind, NodeRef, NormalizedListKind,
    RenderError, RenderFormat, TERMINAL_ATTACH_NEXT_MARKER, TERMINAL_CONTINUE_SOURCE_LINE_MARKER,
    TERMINAL_NO_HYPHEN_BREAK_MARKER, TERMINAL_NONBREAKING_SPACE_MARKER, TerminalFont, TerminalJoin,
    TerminalTextLayout, append, append_blank_line, append_terminal_centered_lines,
    append_terminal_following_vertical_slot, append_terminal_hanging_indent,
    append_terminal_tab_stops_control, append_terminal_tab_stops_request,
    append_terminal_temporary_indent, append_terminal_text, append_terminal_vertical_space,
    collect_terminal_inline_text, collect_terminal_mdoc_heading,
    collect_terminal_mdoc_synopsis_name_head, collect_terminal_quote_contents,
    collect_terminal_semantic_text, collect_terminal_text, display_width, is_first_nested_section,
    is_mdoc_description_block, is_section_block, mark_terminal_attach_next,
    mark_terminal_force_separator, mark_terminal_force_separator_after_sentence,
    mark_terminal_hanging_indent, render_terminal_adjusted_input_lines, render_terminal_bold,
    render_terminal_column_list, render_terminal_definition_list, render_terminal_equation,
    render_terminal_equation_text, render_terminal_explicit_enclosure, render_terminal_font,
    render_terminal_man_hanging_paragraph, render_terminal_man_tagged_paragraph,
    render_terminal_marked_list, render_terminal_mdoc_function_block,
    render_terminal_mdoc_function_element, render_terminal_mdoc_include_declaration,
    render_terminal_mdoc_include_file, render_terminal_plain_list,
    render_terminal_quote_with_display, render_terminal_reference_block, render_terminal_table,
    render_terminal_text_node, take_terminal_table_vertical_skip,
    take_terminal_table_vertical_skips, terminal_apply_mdoc_spacing, terminal_author_mode,
    terminal_author_starts_line, terminal_body_starts_with_break,
    terminal_contains_closed_bf_scope, terminal_contains_embedded_display_quote_close,
    terminal_cross_reference, terminal_embedded_quote_closing,
    terminal_empty_man_section_starts_plain_flow, terminal_follows_empty_section_paragraph,
    terminal_has_pd_control, terminal_has_visible_output, terminal_has_visible_predecessor,
    terminal_has_visible_text, terminal_inherited_font, terminal_link,
    terminal_man_alternating_fonts, terminal_man_field_sibling_break, terminal_man_field_width,
    terminal_man_ip_is_in_rs_body, terminal_man_option, terminal_man_paragraph_density,
    terminal_man_rs_follows_empty_hanging_paragraph, terminal_mdoc_bk_continuation_indent,
    terminal_mdoc_display_indentation, terminal_mdoc_element_font,
    terminal_mdoc_empty_fl_attaches_to_following_macro,
    terminal_mdoc_inline_punctuation_is_literal, terminal_mdoc_list_is_empty,
    terminal_mdoc_long_name_field, terminal_mdoc_prefix_attaches_to_following_token,
    terminal_mdoc_section_named, terminal_mdoc_sm_relinked_argument_precedes,
    terminal_mdoc_sm_relinked_invalid_argument, terminal_mdoc_sm_relinked_valid_argument,
    terminal_mdoc_sm_starts_new_source_phrase, terminal_mdoc_synopsis,
    terminal_mdoc_synopsis_name_paragraph, terminal_mdoc_synopsis_option_body,
    terminal_mdoc_synopsis_spacing, terminal_mdoc_system_macro, terminal_mdoc_system_word_keep,
    terminal_mdoc_word_keep, terminal_next_visible_sibling, terminal_plain_field_width,
    terminal_previous_empty_section, terminal_quote_body_contains_display,
    terminal_quote_delimiters, terminal_quote_has_embedded_closer, terminal_section_body_indent,
    terminal_section_heading_indent, terminal_signed_layout_units, terminal_spacing_disabled,
    terminal_temporary_indent_target, terminal_vertical_span,
};

#[allow(clippy::too_many_lines)] // Terminal macro presentation remains an explicit ordered dispatcher.
pub(super) fn render_terminal_node(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if node.flags().no_print || node.ancestors().any(|ancestor| ancestor.flags().no_print) {
        return Ok(());
    }
    // PD is a stateful man formatter request. Depending on recovery shape it
    // may be represented as an Element or a partial Block.  A partial Block
    // owns a Body containing the following next-line scope, which remains
    // visible; its Head is the private spacing argument.
    if node.macro_name() == Some("PD") {
        if node.kind() == NodeKind::Block {
            for body in node
                .children()
                .filter(|child| child.kind() == NodeKind::Body)
            {
                for child in body.children() {
                    render_terminal_node(child, format, limits, indentation, output, maximum)?;
                }
            }
        }
        return Ok(());
    }
    // `Tg` establishes navigation metadata only. It never contributes a
    // terminal glyph, including when recovery leaves its tag spelling in an
    // otherwise visible compatible-AST element.
    if node.macro_name() == Some("Es") {
        // `Es` is terminal-invisible, but it consumes the same-line slot
        // that a preceding empty `Fl` would otherwise use for attachment.
        // Its next visible sibling therefore resumes with a normal space.
        if output.ends_with(TERMINAL_ATTACH_NEXT_MARKER) {
            mark_terminal_force_separator(output, maximum)?;
        }
        return Ok(());
    }
    if node.macro_name() == Some("Sm") {
        terminal_apply_mdoc_spacing(node, output, maximum)?;
        return Ok(());
    }
    if node.macro_name() == Some("ta") {
        // `.ta` owns terminal formatter state only.  Keep its arguments out
        // of visible flow and defer the state transition to the final width
        // pass, where source tabs are expanded.
        append_terminal_tab_stops_request(node, output, maximum)?;
        return Ok(());
    }
    if node.macro_name() == Some("Tg") {
        return Ok(());
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
        append(output, &render_terminal_font(closing, font), maximum)?;
        append(
            output,
            &TERMINAL_CONTINUE_SOURCE_LINE_MARKER.to_string(),
            maximum,
        )?;
        return Ok(());
    }
    if terminal_mdoc_sm_relinked_invalid_argument(node)
        && terminal_mdoc_sm_relinked_argument_precedes(node)
        && terminal_has_visible_output(output)
    {
        // Recovery turns the remaining words of `.Sm bad ...` into ordinary
        // sibling flow. They keep their normal internal spacing even when
        // the preceding valid request had disabled global mdoc spacing.
        mark_terminal_force_separator(output, maximum)?;
    }
    if matches!(node.macro_name(), Some("ce" | "rj")) {
        render_terminal_adjusted_input_lines(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if terminal_spacing_disabled(output)
        && terminal_has_visible_output(output)
        && terminal_mdoc_sm_starts_new_source_phrase(node)
        && !terminal_mdoc_sm_relinked_valid_argument(node)
        && !terminal_mdoc_sm_relinked_argument_precedes(node)
    {
        // `.Sm off` suppresses in-line argument separation, but a new
        // physical macro/text line still begins an ordinary filled phrase.
        mark_terminal_force_separator_after_sentence(output, maximum)?;
    }
    if is_mdoc_description_block(node) {
        if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
            let children = body.children().collect::<Vec<_>>();
            let paragraph = children
                .iter()
                .position(|child| child.macro_name() == Some("Pp"));
            // A recovered description can contain more than one physical
            // source line. Its body remains description flow until a Pp
            // restores ordinary structural rendering.
            let description_end = paragraph.unwrap_or(children.len());
            let mut description = String::new();
            for child in &children[..description_end] {
                collect_terminal_text(*child, format, limits, &mut description);
            }
            let prefix = if matches!(format, RenderFormat::Utf8) {
                "–"
            } else {
                "-"
            };
            let phrase = if description.is_empty() {
                prefix.to_owned()
            } else {
                format!("{prefix} {description}")
            };
            append_terminal_text(
                output,
                &phrase,
                TerminalTextLayout::default(),
                indentation,
                maximum,
            )?;
            for child in &children[description_end..] {
                render_terminal_node(*child, format, limits, indentation, output, maximum)?;
            }
        }
        return Ok(());
    }
    if is_section_block(node) {
        let mut heading = String::new();
        let mut body = None;
        let mdoc_heading = matches!(node.macro_name(), Some("Sh" | "Ss"));
        for child in node.children() {
            match child.kind() {
                NodeKind::Head if mdoc_heading => {
                    collect_terminal_mdoc_heading(child, format, limits, &mut heading);
                }
                NodeKind::Head => collect_terminal_text(child, format, limits, &mut heading),
                NodeKind::Body => body = Some(child),
                _ => {}
            }
        }
        if heading.chars().all(|character| {
            character.is_whitespace() || character == TERMINAL_NONBREAKING_SPACE_MARKER
        }) {
            // An mdoc section title consisting only of escaped horizontal
            // space is a recovered empty heading. It owns no device glyph;
            // retaining it as one literal blank would leave a visible-space
            // line between the surrounding paragraphs.
            heading.clear();
        }
        let empty_mdoc_heading = mdoc_heading && heading.is_empty();
        if !heading.is_empty() {
            if !is_first_nested_section(node) {
                // A section heading owns its normal separator below a table.
                // Do not weaken genuine negative `.sp` recovery here.
                let _ = take_terminal_table_vertical_skip(output);
                if terminal_previous_empty_section(node, format, limits) {
                    if !output.is_empty() && !output.ends_with('\n') {
                        append(output, "\n", maximum)?;
                    }
                } else if matches!(node.macro_name(), Some("SH" | "SS")) {
                    // `PD` is terminal presentation state, including at a
                    // following man section boundary.  A zero request merely
                    // completes the preceding line, while larger values add
                    // that many vertical slots before the next heading.
                    let density = terminal_man_paragraph_density(node).unwrap_or(1);
                    if density == 0 {
                        if !output.is_empty() && !output.ends_with('\n') {
                            append(output, "\n", maximum)?;
                        }
                    } else {
                        append_blank_line(output, maximum)?;
                        for _ in 1..density {
                            append(output, "\n", maximum)?;
                        }
                    }
                } else {
                    append_blank_line(output, maximum)?;
                }
            }
            if matches!(node.macro_name(), Some("SH" | "SS")) {
                // A long man heading begins at the section's heading column,
                // while each wrapped terminal continuation enters the Body
                // field. Keep this device-only hanging geometry out of the
                // compatible AST.
                append_terminal_hanging_indent(
                    output,
                    terminal_section_body_indent(node),
                    maximum,
                )?;
            }
            append(
                output,
                &" ".repeat(terminal_section_heading_indent(node)),
                maximum,
            )?;
            if mdoc_heading {
                append(output, &heading, maximum)?;
            } else {
                append(output, &render_terminal_bold(&heading, format), maximum)?;
            }
        } else if empty_mdoc_heading && !output.is_empty() {
            // A visibly empty mdoc section still transitions through the
            // heading field: its absent title leaves the normal section gap
            // plus the heading's own empty device line before Body prose.
            append_blank_line(output, maximum)?;
            append(output, "\n", maximum)?;
        }
        if let Some(body) = body {
            if heading.is_empty()
                && terminal_empty_man_section_starts_plain_flow(node, body)
                && !output.is_empty()
            {
                // An argumentless man section retains its Body after
                // validation. term.c treats that otherwise invisible section
                // opener as the ordinary paragraph boundary before prose or
                // a fill-mode transition. Structural paragraph/list blocks
                // own their own gap, so restrict this to plain body flow.
                append_blank_line(output, maximum)?;
            } else if !heading.is_empty()
                && (terminal_has_visible_text(body, format, limits)
                    || terminal_has_pd_control(body))
            {
                append(output, "\n", maximum)?;
            }
            let body_indentation = terminal_section_body_indent(node);
            for child in body.children() {
                render_terminal_node(child, format, limits, body_indentation, output, maximum)?;
            }
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("Rs") {
        render_terminal_reference_block(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("PP") {
        let density = terminal_man_paragraph_density(node);
        // A PD immediately after a section heading changes the following PP
        // before it emits any visible material, so it must not manufacture a
        // blank line before that first Body phrase.  Later paragraphs retain
        // the normal blank plus PD's additional vertical slots.
        if terminal_has_visible_predecessor(node) {
            append_terminal_following_vertical_slot(node, output, maximum)?;
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
        if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
            for child in body.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("TP") {
        render_terminal_man_tagged_paragraph(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("HP") {
        render_terminal_man_hanging_paragraph(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("Fo") {
        render_terminal_mdoc_function_block(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("Bf") {
        // `Bf`'s Head is validation/configuration input only; its retained
        // extra arguments remain observable in the public AST but the
        // terminal device skips that Head and applies the normalized font to
        // Body flow alone.
        if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
            for child in body.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("Bl")
        && terminal_mdoc_list_is_empty(node)
    {
        // An empty mdoc list is presentation-transparent except that its
        // block boundary completes the preceding physical source phrase.
        if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("Bl")
        && node.list_kind() == Some(NormalizedListKind::Plain)
    {
        render_terminal_plain_list(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("Bl")
        && node.list_kind() == Some(NormalizedListKind::Column)
    {
        return render_terminal_column_list(node, format, limits, indentation, output, maximum);
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("Bl")
        && matches!(
            node.list_kind(),
            Some(NormalizedListKind::Bullet | NormalizedListKind::Ordered)
        )
    {
        render_terminal_marked_list(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("Bl")
        && node.list_kind() == Some(NormalizedListKind::Definition)
    {
        render_terminal_definition_list(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("Eo") {
        render_terminal_explicit_enclosure(node, format, limits, indentation, output, maximum)?;
        return Ok(());
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("Op")
        && terminal_mdoc_synopsis(node)
    {
        // In SYNOPSIS each optional form is one keepable declaration field.
        // Collect its nested brackets and typography first, then protect its
        // internal separators so the width pass moves the whole option to
        // the continuation line rather than splitting after its opener.
        let mut optional = String::new();
        collect_terminal_text(node, format, limits, &mut optional);
        if !optional.is_empty() {
            // A kept optional form is one terminal word.  In particular, a
            // short option such as `-s` must not become the final hyphen of
            // one device line plus its letter on the next line.
            let optional = optional.replace(' ', &TERMINAL_NONBREAKING_SPACE_MARKER.to_string());
            let optional = optional.replace('-', &format!("-{TERMINAL_NO_HYPHEN_BREAK_MARKER}"));
            append_terminal_text(
                output,
                &optional,
                TerminalTextLayout::default(),
                indentation,
                maximum,
            )?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block
        && node.macro_name() == Some("En")
        && node.enclosure().is_some()
    {
        let mut contents = String::new();
        collect_terminal_text(node, format, limits, &mut contents);
        if !contents.is_empty() {
            append_terminal_text(
                output,
                &contents,
                TerminalTextLayout::default(),
                indentation,
                maximum,
            )?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block
        && node
            .children()
            .find(|child| child.kind() == NodeKind::Body)
            .is_some_and(terminal_quote_body_contains_display)
        && terminal_quote_delimiters(node, None, format).is_some()
    {
        return render_terminal_quote_with_display(
            node,
            format,
            limits,
            indentation,
            output,
            maximum,
        );
    }
    if node.kind() == NodeKind::Block && terminal_quote_delimiters(node, None, format).is_some() {
        let mut leading = String::new();
        let mut contents = String::new();
        let mut trailing = String::new();
        for head in node
            .children()
            .filter(|child| child.kind() == NodeKind::Head || child.flags().delimiter_open)
        {
            collect_terminal_text(head, format, limits, &mut leading);
        }
        let body = node.children().find(|child| child.kind() == NodeKind::Body);
        if let Some(body) = body {
            collect_terminal_quote_contents(body, format, limits, indentation, &mut contents);
        }
        for tail in node
            .children()
            .filter(|child| child.kind() == NodeKind::Tail || child.flags().delimiter_close)
        {
            collect_terminal_text(tail, format, limits, &mut trailing);
        }
        if let Some((opening, closing)) = terminal_quote_delimiters(node, body, format) {
            // Delimiters are generated presentation text rather than AST
            // words.  Give them the same inherited font as their opening
            // scope, then account for an empty Bf Body inserted by mdoc
            // recovery when `.Ef` closes inside this still-open enclosure.
            let opening = render_terminal_font(opening, terminal_inherited_font(node));
            let closing_font = if body.is_some_and(terminal_contains_closed_bf_scope) {
                TerminalFont::Roman
            } else {
                terminal_inherited_font(node)
            };
            let closing = if body
                .is_some_and(|body| terminal_quote_has_embedded_closer(body, node.macro_name()))
            {
                String::new()
            } else {
                render_terminal_font(closing, closing_font)
            };
            append_terminal_text(
                output,
                &format!("{leading}{opening}{contents}{closing}{trailing}"),
                TerminalTextLayout::default(),
                indentation,
                maximum,
            )?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block && matches!(node.macro_name(), Some("D1" | "Dl")) {
        // The one-line mdoc displays are independent terminal fields. They
        // always complete the preceding device line, use one extra display
        // indent, and leave the next ordinary phrase on its own line.
        if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        let mut contents = String::new();
        if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
            collect_terminal_text(body, format, limits, &mut contents);
        }
        if !contents.is_empty() {
            append_terminal_text(
                output,
                &contents,
                TerminalTextLayout {
                    line_start: true,
                    ..TerminalTextLayout::default()
                },
                indentation.saturating_add(6),
                maximum,
            )?;
        }
        if !contents.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("Bd") {
        // A display is an independent vertical region. Its optional mdoc
        // offset applies in addition to the enclosing section indentation.
        // An unoffset unfilled display directly below a section heading
        // starts in the heading's normal body field; it does not insert a
        // phantom vertical gap. The literal/unfilled distinction only changes
        // tab stops; it does not add a vertical slot before the first display.
        // Offsets and all displays following visible flow retain their
        // independent device boundary.
        if terminal_has_visible_predecessor(node) {
            if node.compact() {
                if !output.is_empty() && !output.ends_with('\n') {
                    append(output, "\n", maximum)?;
                }
            } else {
                append_blank_line(output, maximum)?;
            }
        } else if !node.compact()
            && !node.literal_display()
            && (node.display_kind() != Some(DisplayKind::Literal) || node.offset().is_some())
        {
            // A first filled display, or an offset `-unfilled` display,
            // owns a device gap below a section heading. An unoffset
            // `-unfilled` display begins in that heading field like a
            // literal display; the public normalized kind alone cannot
            // distinguish its tab-stop behavior from `-literal`.
            append_blank_line(output, maximum)?;
        }
        if node.literal_display() {
            // `termp_bd_pre()` resets the device tabs to the literal
            // display's eight-column periodic field.  This state survives
            // the display until a later roff `.ta` request changes it.
            append_terminal_tab_stops_control(output, "T\u{1f}8n", maximum)?;
        }
        if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
            let display_indentation = terminal_mdoc_display_indentation(node, indentation);
            if node.centered_display() {
                let mut centered = String::new();
                for child in body.children() {
                    render_terminal_node(
                        child,
                        format,
                        limits,
                        display_indentation,
                        &mut centered,
                        maximum,
                    )?;
                }
                append_terminal_centered_lines(output, &centered, maximum)?;
            } else {
                for child in body.children() {
                    render_terminal_node(
                        child,
                        format,
                        limits,
                        display_indentation,
                        output,
                        maximum,
                    )?;
                }
            }
        }
        // A following display or paragraph introduces its own vertical
        // boundary. Do not manufacture another one here: ordinary prose that
        // follows `.Ed` remains in its source paragraph.
        if terminal_contains_embedded_display_quote_close(node) {
            append(
                output,
                &TERMINAL_CONTINUE_SOURCE_LINE_MARKER.to_string(),
                maximum,
            )?;
        } else if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("IP") {
        // The first IP argument is its tag; an optional final `n` width
        // belongs to the following body indentation rather than visible
        // terminal content.
        // An empty recovered paragraph directly under a section heading is
        // retained as the first Body child of the field, but term.c consumes
        // it before placing that field. A field directly after the heading
        // still owns its ordinary paragraph boundary.
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
        let mut body = None;
        let mut tag_nodes = Vec::new();
        // Man's IP device field is seven `n` units by default.  A short
        // tag shares that physical line with the first body phrase; a tag
        // that reaches the field width leaves the body on the next line at
        // the same field boundary.
        let tag_field_width = terminal_man_field_width(node);
        for child in node.children() {
            match child.kind() {
                NodeKind::Head => tag_nodes.extend(child.children()),
                NodeKind::Body => body = Some(child),
                _ => {}
            }
        }
        // man(7) accepts exactly one tag argument and one optional width.
        // The compatible AST retains later malformed arguments for source
        // diagnostics, but the terminal device neither prints nor interprets
        // them as tag words.
        if tag_nodes.len() > 1 {
            tag_nodes.truncate(1);
        }
        let body_indentation = if tag_field_width.is_negative() {
            indentation.saturating_sub(tag_field_width.unsigned_abs())
        } else {
            indentation.saturating_add(tag_field_width.unsigned_abs())
        };
        let mut tag = String::new();
        for child in tag_nodes {
            collect_terminal_text(child, format, limits, &mut tag);
        }
        // Man IP tags are a field, not literal display text: trailing input
        // blanks consume no extra field width and must not force the body
        // onto a continuation line.
        let tag = tag.trim_end().to_owned();
        if !tag.is_empty() {
            append_terminal_text(
                output,
                &tag,
                TerminalTextLayout {
                    line_start: true,
                    // A tag normally remains ordinary wrappable terminal
                    // prose. Preserve only authored internal spacing; field
                    // padding is protected independently below so a long tag
                    // can still wrap at the device margin. A field that
                    // itself begins beyond the standard right margin is a
                    // terminal overflow field, for which term.c suppresses
                    // normal reflow entirely.
                    keep_spacing: tag.contains('\t')
                        || tag.contains("  ")
                        || body_indentation > DEFAULT_RENDER_WIDTH,
                    ..TerminalTextLayout::default()
                },
                indentation,
                maximum,
            )?;
        }
        if let Some(body) = body {
            // A tagged IP whose Body was closed immediately by another
            // field is a visible tag-only line. `term.c` does not materialise
            // its unused field padding; doing so would leak trailing blanks
            // into the public ASCII stream.
            let body_has_visible_text = terminal_has_visible_text(body, format, limits);
            let body_starts_with_terminal_break = terminal_body_starts_with_break(body);
            if !tag.is_empty()
                && body_has_visible_text
                && !body_starts_with_terminal_break
                && tag_field_width > 0
                && display_width(&tag) < tag_field_width.unsigned_abs()
            {
                let gap = tag_field_width
                    .unsigned_abs()
                    .saturating_sub(display_width(&tag));
                append(
                    output,
                    &TERMINAL_NONBREAKING_SPACE_MARKER
                        .to_string()
                        .repeat(gap.saturating_sub(1)),
                    maximum,
                )?;
            } else if !tag.is_empty() && body_has_visible_text && !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
            let mut inline_first_no_fill_text = !tag.is_empty()
                && body_has_visible_text
                && !body_starts_with_terminal_break
                && tag_field_width > 0
                && display_width(&tag) < tag_field_width.unsigned_abs();
            for child in body.children() {
                if inline_first_no_fill_text
                    && child.kind() == NodeKind::Text
                    && child.flags().no_fill
                    && child.flags().line_start
                {
                    render_terminal_text_node(
                        child,
                        format,
                        limits,
                        body_indentation,
                        output,
                        maximum,
                        true,
                    )?;
                    inline_first_no_fill_text = false;
                } else {
                    render_terminal_node(child, format, limits, body_indentation, output, maximum)?;
                    if !child.flags().no_print {
                        inline_first_no_fill_text = false;
                    }
                }
            }
        }
        // `post_IP()` closes the field with only one physical line. Outside
        // an explicit RS Body, the following paragraph/block owns the usual
        // vertical separation. Inside RS, adding a second line here leaks a
        // blank between the indented field and immediately resumed prose.
        if terminal_man_ip_is_in_rs_body(node) {
            if !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
        } else if density == Some(0) {
            // A zero PD keeps the following field or paragraph adjacent to
            // this IP. `post_IP()` still completes its physical line, but
            // must not manufacture the default vertical slot.
            if !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
        } else {
            append_blank_line(output, maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block && matches!(node.macro_name(), Some("UR" | "MT")) {
        // The terminal device presents URI and mailto blocks' visible Body
        // first and places their Head resource in angle brackets after it.
        // MT's Tail (the optional `.ME` arguments) attaches immediately to
        // that closing resource. The semantic tree keeps all three regions
        // separate for navigation and diagnostics.
        let mut resource = String::new();
        let mut contents = String::new();
        let mut trailing = String::new();
        for child in node.children() {
            match child.kind() {
                NodeKind::Head => collect_terminal_text(child, format, limits, &mut resource),
                NodeKind::Body => collect_terminal_text(child, format, limits, &mut contents),
                NodeKind::Tail => collect_terminal_text(child, format, limits, &mut trailing),
                _ => {}
            }
        }
        if !contents.is_empty() {
            append_terminal_text(
                output,
                &contents,
                TerminalTextLayout::default(),
                indentation,
                maximum,
            )?;
        }
        // An empty URI or mailto request is still an explicit link boundary:
        // term.c emits `<>` after any Body text.
        append_terminal_text(
            output,
            &format!("<{resource}>"),
            TerminalTextLayout::default(),
            indentation,
            maximum,
        )?;
        if !trailing.is_empty() {
            append_terminal_text(
                output,
                &trailing,
                TerminalTextLayout {
                    join: TerminalJoin::Attach,
                    ..TerminalTextLayout::default()
                },
                indentation,
                maximum,
            )?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("SY") {
        // A man(7) synopsis block is a device field rather than ordinary
        // nested prose.  Its command head is bold and owns a terminal line;
        // a body inside `.nf` starts in the indented synopsis continuation
        // field, while a filled body stays beside the command.
        append_blank_line(output, maximum)?;
        let head = node.children().find(|child| child.kind() == NodeKind::Head);
        let body = node.children().find(|child| child.kind() == NodeKind::Body);
        if let Some(head) = head {
            let mut command = String::new();
            collect_terminal_semantic_text(head, format, limits, TerminalFont::Bold, &mut command);
            if !command.is_empty() {
                append_terminal_text(
                    output,
                    &command,
                    TerminalTextLayout::default(),
                    indentation,
                    maximum,
                )?;
            }
        }
        let body_is_no_fill =
            body.is_some_and(|body| body.children().any(|child| child.flags().no_fill));
        if body_is_no_fill && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        if let Some(body) = body {
            let body_indentation = if body_is_no_fill {
                indentation.saturating_add(8)
            } else {
                indentation
            };
            for child in body.children() {
                render_terminal_node(child, format, limits, body_indentation, output, maximum)?;
            }
        }
        if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Block && node.macro_name() == Some("RS") {
        let explicit_width = node
            .children()
            .find(|child| child.kind() == NodeKind::Head)
            .and_then(|head| head.children().find_map(NodeRef::text))
            .and_then(|value| {
                terminal_signed_layout_units(value).or_else(|| {
                    // RS accepts an unsuffixed roff number as a terminal
                    // field width. The device truncates its fractional part
                    // to whole cells (`3.5` therefore contributes three).
                    terminal_plain_field_width(value)
                })
            });
        // A widthless RS restores the most recent TP/IP/HP field margin in
        // its current man body, even when ordinary prose intervenes. A PP
        // resets that register; a nested RS body has no such sibling field
        // and therefore resumes the ordinary seven-cell default.
        let saved_field_width = explicit_width
            .is_none()
            .then(|| {
                let parent = node.parent()?;
                parent
                    .children()
                    .take_while(|sibling| sibling.id() != node.id())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .take_while(|sibling| sibling.macro_name() != Some("PP"))
                    .any(|sibling| matches!(sibling.macro_name(), Some("TP" | "IP" | "HP")))
                    .then(|| terminal_man_field_width(node))
            })
            .flatten();
        let restores_field_margin = saved_field_width.is_some()
            || (explicit_width.is_none()
                && node.parent().is_some_and(|parent| {
                    parent.kind() == NodeKind::Body
                        && parent.parent().is_some_and(|field| {
                            matches!(field.macro_name(), Some("TP" | "IP" | "HP"))
                        })
                }));
        let width = explicit_width.unwrap_or(7);
        let body_indentation = if let Some(saved) = saved_field_width {
            if saved.is_negative() {
                indentation.saturating_sub(saved.unsigned_abs())
            } else {
                indentation.saturating_add(saved.unsigned_abs())
            }
        } else if restores_field_margin {
            indentation
        } else if width.is_negative() {
            indentation.saturating_sub(width.unsigned_abs())
        } else {
            indentation.saturating_add(width.unsigned_abs())
        };
        if restores_field_margin
            && !terminal_man_rs_follows_empty_hanging_paragraph(node)
            && output.ends_with("\n\n")
        {
            output.pop();
        }
        if terminal_man_rs_follows_empty_hanging_paragraph(node) && output.ends_with('\n') {
            // A zero-body HP is still a completed field boundary.  Its
            // following sibling RS starts a fresh region rather than
            // attaching to the preceding prose line.
            append(output, "\n", maximum)?;
        }
        if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        if let Some(body) = node.children().find(|child| child.kind() == NodeKind::Body) {
            for child in body.children() {
                render_terminal_node(child, format, limits, body_indentation, output, maximum)?;
            }
        }
        if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Element && node.macro_name() == Some("ti") {
        let target = node
            .children()
            .find_map(NodeRef::text)
            .and_then(|value| terminal_temporary_indent_target(value, indentation));
        if let Some(target) = target {
            append_terminal_temporary_indent(output, target, maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Element && matches!(node.macro_name(), Some("EX" | "EE")) {
        // `pre_literal()` always starts a terminal line and never prints the
        // request's recovered arguments. The surrounding no-fill text nodes
        // retain their own physical line boundaries.
        if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Element && matches!(node.macro_name(), Some("nf" | "fi")) {
        // `nf` and `fi` are terminal line controls even though their public
        // AST elements contain no printable payload. Consecutive controls do
        // not create a blank line, but a transition after visible flow flushes
        // that flow before the following fill mode writes its first word.
        if !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    if node.kind() == NodeKind::Element
        && matches!(node.macro_name(), Some("ft" | "po" | "ll" | "in"))
    {
        // `.ft` changes the terminal device's current font and `.po` changes
        // its page offset, `.ll` changes its line length, and `.in` changes
        // its physical field. Their
        // compatible AST children are request arguments, not printable prose;
        // subsequent text reconstructs each state from prior requests.
        // The terminal device also completes the preceding physical field
        // before a standalone indentation update; otherwise the new absolute
        // column could not take effect until a later paragraph boundary.
        if node.macro_name() == Some("in") && !output.is_empty() && !output.ends_with('\n') {
            append(output, "\n", maximum)?;
        }
        return Ok(());
    }
    match node.kind() {
        NodeKind::Comment => {}
        NodeKind::Text => {
            render_terminal_text_node(node, format, limits, indentation, output, maximum, false)?;
        }
        NodeKind::Element if node.macro_name() == Some("Nm") => {
            let mut name = String::new();
            // Nm establishes a bold base font, but its children can switch
            // to italic/roman with `\\f` and later restore the base. Applying
            // bold after generic collection would overstrike an already
            // styled fragment a second time.
            collect_terminal_semantic_text(node, format, limits, TerminalFont::Bold, &mut name);
            append_terminal_text(
                output,
                &name,
                TerminalTextLayout {
                    // Like other mdoc inline macros, Nm's physical request
                    // line remains ordinary filled prose.
                    line_start: false,
                    ..TerminalTextLayout::default()
                },
                indentation,
                maximum,
            )?;
        }
        NodeKind::Element if node.macro_name() == Some("Xr") => {
            if let Some(reference) = terminal_cross_reference(node, format, limits) {
                append_terminal_text(
                    output,
                    &reference,
                    TerminalTextLayout::default(),
                    indentation,
                    maximum,
                )?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("Lk") => {
            if let Some(link) = terminal_link(node, format, limits) {
                append_terminal_text(
                    output,
                    &link,
                    TerminalTextLayout::default(),
                    indentation,
                    maximum,
                )?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("Db") => {
            // `Db` is an obsolete debugging request.  Its syntax remains in
            // the compatible AST (and emits a parser diagnostic), but the
            // terminal device's `termp_skip_pre()` suppresses both it and
            // its recovered arguments.
        }
        NodeKind::Element if node.macro_name() == Some("Lb") => {
            for child in node.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
            // Library macros are ordinary inline content outside LIBRARY.
            // Inside that conventional section, a request that begins a
            // physical source line completes its device line after rendering.
            if node.flags().line_start
                && terminal_mdoc_section_named(node, "LIBRARY")
                && !output.ends_with('\n')
            {
                append(output, "\n", maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("Fn") => {
            render_terminal_mdoc_function_element(
                node,
                format,
                limits,
                indentation,
                output,
                maximum,
            )?;
        }
        NodeKind::Element if node.macro_name() == Some("Fd") => {
            render_terminal_mdoc_include_declaration(
                node,
                format,
                limits,
                indentation,
                output,
                maximum,
            )?;
        }
        NodeKind::Element if node.macro_name() == Some("In") => {
            render_terminal_mdoc_include_file(node, format, limits, indentation, output, maximum)?;
        }
        NodeKind::Element if node.macro_name() == Some("Ns") => {
            // `Ns` only removes a separator when it occurs in the middle of
            // a physical macro line. At its own line start, term.c leaves
            // the following phrase's ordinary separation intact.
            if !node.flags().line_start {
                append(output, &TERMINAL_ATTACH_NEXT_MARKER.to_string(), maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("No") => {
            // A line-start normal-text macro follows a preceding source-line
            // delimiter, not the delimiter's syntactic argument. Restore
            // the ordinary device separator before descending into its
            // Roman/no-hyphen text children.
            if node.flags().line_start
                && !node
                    .ancestors()
                    .any(|ancestor| ancestor.macro_name() == Some("Eo"))
                && output.ends_with([TERMINAL_ATTACH_NEXT_MARKER, '(', '[', '{', '<'])
            {
                mark_terminal_force_separator(output, maximum)?;
            }
            for child in node.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
        }
        NodeKind::Element if terminal_mdoc_system_macro(node.macro_name()) => {
            // A system-name macro and its optional version form one device
            // word.  Keeping the generated name and following version
            // together lets the width pass break before `OpenBSD 6.1`, not
            // between those two source arguments.
            let mut system = String::new();
            collect_terminal_text(node, format, limits, &mut system);
            append_terminal_text(
                output,
                &system.replace(' ', &TERMINAL_NONBREAKING_SPACE_MARKER.to_string()),
                TerminalTextLayout::default(),
                indentation,
                maximum,
            )?;
        }
        NodeKind::Block if node.macro_name() == Some("Bk") => {
            if let Some(phrase) = terminal_mdoc_system_word_keep(node, format, limits) {
                append_terminal_text(
                    output,
                    &phrase,
                    TerminalTextLayout::default(),
                    indentation,
                    maximum,
                )?;
            } else if let Some(phrase) = terminal_mdoc_word_keep(node, format, limits) {
                // A synopsis Bk leaves the first line in its enclosing field
                // but gives a wrapped kept phrase the device's ten-cell
                // continuation field. Prose keeps retain the paragraph's
                // ordinary wrap field.
                if terminal_mdoc_synopsis(node) {
                    mark_terminal_hanging_indent(
                        output,
                        terminal_mdoc_bk_continuation_indent(node, format, limits, indentation),
                    );
                }
                append_terminal_text(
                    output,
                    &phrase,
                    TerminalTextLayout::default(),
                    indentation,
                    maximum,
                )?;
            } else {
                for child in node.children() {
                    render_terminal_node(child, format, limits, indentation, output, maximum)?;
                }
            }
        }
        NodeKind::Block if node.macro_name() == Some("Nm") && terminal_mdoc_synopsis(node) => {
            // `termp_nm_pre()` enters synopsis layout from the Nm block's
            // Head.  Consequently consecutive name declarations are
            // distinct device lines even though their source blocks contain
            // only otherwise-inline text.
            terminal_mdoc_synopsis_spacing(node, output, maximum)?;
            for child in node.children() {
                if child.kind() == NodeKind::Head {
                    let mut name = String::new();
                    collect_terminal_mdoc_synopsis_name_head(child, format, limits, &mut name);
                    append_terminal_text(
                        output,
                        &name,
                        TerminalTextLayout::default(),
                        indentation,
                        maximum,
                    )?;
                    // A long implicit synopsis name establishes its Body's
                    // field one cell past the complete name, even when the
                    // name itself has already wrapped at the device margin.
                    // Short names keep the ordinary synopsis field; their
                    // option blocks have distinct mdoc layout semantics.
                    let name_width = display_width(&name);
                    if name_width > 70
                        && node.children().any(|part| {
                            part.kind() == NodeKind::Body
                                && part.children().any(|nested| !nested.flags().no_print)
                        })
                    {
                        mark_terminal_hanging_indent(
                            output,
                            indentation.saturating_add(name_width).saturating_add(1),
                        );
                    }
                } else {
                    // A synopsis name followed directly by optional forms
                    // owns the conventional nine-column continuation field.
                    // The field is independent of the name's visible width:
                    // the terminal moves a whole later option there when the
                    // current declaration line is full.
                    if child.kind() == NodeKind::Body && terminal_mdoc_synopsis_option_body(child) {
                        mark_terminal_hanging_indent(output, indentation.saturating_add(4));
                    }
                    render_terminal_node(child, format, limits, indentation, output, maximum)?;
                }
            }
        }
        NodeKind::Block if node.macro_name() == Some("Vt") && terminal_mdoc_synopsis(node) => {
            // In SYNOPSIS each variable declaration owns one device line;
            // the same macro remains an inline italic phrase in prose.
            if !output.is_empty() && !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
            for child in node.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("Ap") => {
            // `Ap` is an apostrophe punctuation macro.  Its optional child
            // belongs to the preceding word (for example `Ingo Ap s`), so
            // retain the same next-token attachment state used by `Ns`.
            append_terminal_text(
                output,
                "'",
                TerminalTextLayout {
                    join: TerminalJoin::Attach,
                    ..TerminalTextLayout::default()
                },
                indentation,
                maximum,
            )?;
            append(output, &TERMINAL_ATTACH_NEXT_MARKER.to_string(), maximum)?;
            for child in node.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("Pf") => {
            // `Pf` presents its one literal argument as a prefix for the
            // next visible token on the same source line.  In particular,
            // the prefix need not itself be parsed punctuation: `.Pf pre
            // fixed` becomes `prefixed`, while `.Pf . right` becomes
            // `.right`.  An incomplete prefix must not capture a later
            // physical source line.
            for child in node.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
            if terminal_mdoc_prefix_attaches_to_following_token(node) {
                mark_terminal_attach_next(output, maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("OP") => {
            append_terminal_text(
                output,
                &terminal_man_option(node, format, limits),
                TerminalTextLayout::default(),
                indentation,
                maximum,
            )?;
        }
        NodeKind::Element if terminal_man_alternating_fonts(node.macro_name()).is_some() => {
            let fonts = terminal_man_alternating_fonts(node.macro_name()).expect("guarded above");
            let mut contents = String::new();
            for (index, child) in node.children().enumerate() {
                let mut fragment = String::new();
                collect_terminal_semantic_text(
                    child,
                    format,
                    limits,
                    fonts[index % fonts.len()],
                    &mut fragment,
                );
                contents.push_str(&fragment);
            }
            let no_fill = node.flags().no_fill;
            append_terminal_text(
                output,
                &contents,
                TerminalTextLayout {
                    // man(7)'s alternating font requests deliberately join
                    // consecutive arguments without an inter-word device
                    // space; the styled child fragments retain their own
                    // formatter escapes.
                    line_start: no_fill && node.flags().line_start,
                    no_fill,
                    keep_spacing: contents.contains('\t'),
                    ..TerminalTextLayout::default()
                },
                indentation,
                maximum,
            )?;
        }
        NodeKind::Element if node.macro_name() == Some("B") => {
            let mut bold = String::new();
            collect_terminal_inline_text(node, format, limits, &mut bold);
            let no_fill = node.flags().no_fill;
            append_terminal_text(
                output,
                &render_terminal_bold(&bold, format),
                TerminalTextLayout {
                    // Font macros are inline even when their request begins
                    // a new source line; paragraph and display requests own
                    // terminal physical boundaries.
                    line_start: no_fill && node.flags().line_start,
                    no_fill,
                    keep_spacing: bold.contains('\t'),
                    ..TerminalTextLayout::default()
                },
                indentation,
                maximum,
            )?;
        }
        NodeKind::Element if node.macro_name() == Some("I") => {
            let mut italic = String::new();
            collect_terminal_semantic_text(node, format, limits, TerminalFont::Italic, &mut italic);
            let no_fill = node.flags().no_fill;
            append_terminal_text(
                output,
                &italic,
                TerminalTextLayout {
                    // Font macros remain inline in filled prose, but a
                    // literal source-line request retains its field start.
                    line_start: no_fill && node.flags().line_start,
                    no_fill,
                    keep_spacing: italic.contains('\t'),
                    ..TerminalTextLayout::default()
                },
                indentation,
                maximum,
            )?;
        }
        NodeKind::Element if node.macro_name() == Some("An") => {
            // `An -split` and `An -nosplit` are terminal-device state: the
            // directive itself (including validator-retained excess words)
            // does not print.  A following ordinary `An` begins its own
            // physical line in split mode.  The state is scoped to the
            // current mdoc body, where the parser publishes the resolved
            // option on its directive node.
            if node.author_mode().is_some() {
                return Ok(());
            }
            if terminal_author_mode(node) == AuthorMode::Split
                && terminal_author_starts_line(node)
                && !output.is_empty()
                && !output.ends_with('\n')
            {
                append(output, "\n", maximum)?;
            }
            for child in node.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("Ft") && terminal_mdoc_synopsis(node) => {
            terminal_mdoc_synopsis_spacing(node, output, maximum)?;
            let mut contents = String::new();
            collect_terminal_semantic_text(
                node,
                format,
                limits,
                TerminalFont::Italic,
                &mut contents,
            );
            append_terminal_text(
                output,
                &contents,
                TerminalTextLayout::default(),
                indentation,
                maximum,
            )?;
        }
        NodeKind::Element if terminal_mdoc_element_font(node).is_some() => {
            let mut contents = String::new();
            let font = terminal_mdoc_element_font(node).expect("guarded above");
            let trailing_open_delimiter = node
                .children()
                .next_back()
                .is_some_and(|child| child.flags().delimiter_open);
            collect_terminal_semantic_text(node, format, limits, font, &mut contents);
            let empty_flag = node.macro_name() == Some("Fl") && node.children().next().is_none();
            if node.macro_name() == Some("Fl")
                && (contents.is_empty() || node.children().next().is_some())
            {
                // `.Fl` owns its leading dash. An authored escaped hyphen
                // is its argument, so `Fl \\-long` intentionally renders as
                // the GNU-style `--long` rather than suppressing the macro's
                // own prefix after escape expansion.
                contents.insert_str(0, &render_terminal_font("-", font));
            }
            if terminal_mdoc_long_name_field(node, format, limits) {
                contents = contents.replace(' ', &TERMINAL_NONBREAKING_SPACE_MARKER.to_string());
            }
            if node.flags().line_start
                && output.ends_with([TERMINAL_ATTACH_NEXT_MARKER, '(', '[', '{', '<'])
            {
                mark_terminal_force_separator(output, maximum)?;
            }
            append_terminal_text(
                output,
                &contents,
                TerminalTextLayout {
                    // mdoc inline macros do not turn their physical source
                    // line into a terminal boundary. Structural requests
                    // (sections, displays, `br`, and paragraphs) have
                    // already produced one when required.
                    line_start: false,
                    literal_punctuation: terminal_mdoc_inline_punctuation_is_literal(node),
                    ..TerminalTextLayout::default()
                },
                indentation,
                maximum,
            )?;
            if empty_flag && terminal_mdoc_empty_fl_attaches_to_following_macro(node) {
                mark_terminal_attach_next(output, maximum)?;
            }
            if trailing_open_delimiter {
                // A delimiter at the end of one semantic macro does not
                // pull the first argument of a later macro across that
                // macro boundary. Preserve the following ordinary space
                // without leaking layout state into the public AST.
                mark_terminal_force_separator(output, maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("Nd") => {
            let mut description = String::new();
            collect_terminal_text(node, format, limits, &mut description);
            if !description.is_empty() {
                if !output.is_empty() && !output.ends_with([' ', '\n']) {
                    append(output, " ", maximum)?;
                }
                append(output, "- ", maximum)?;
                append(output, &description, maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("PD") => {
            // Paragraph density is a presentation request. Its scoped value
            // is queried by following PP blocks, not emitted as prose.
        }
        NodeKind::Element if matches!(node.macro_name(), Some("Ex" | "Rv")) => {
            // The standard exit/return-value expansions begin a fresh device
            // line below a preceding label such as `one argument:`. Their
            // generated phrases remain ordinary wrapped prose afterwards.
            if !output.is_empty() && !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
            for child in node.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
        }
        NodeKind::Element if node.macro_name() == Some("Pp") => {
            append_terminal_following_vertical_slot(node, output, maximum)?;
            append_blank_line(output, maximum)?;
            if terminal_mdoc_synopsis_name_paragraph(node)
                && terminal_next_visible_sibling(node)
                    .is_none_or(|next| next.macro_name() != Some("Nm"))
            {
                // In a synopsis-pretty mdoc scope, `Pp` starts the next
                // declaration phrase below the preceding `Nm` field. The
                // public node only carries the pretty flag; preserve the
                // device's twelve-column continuation privately until the
                // final width pass.
                append_terminal_temporary_indent(output, indentation.saturating_add(7), maximum)?;
            }
        }
        NodeKind::Element if matches!(node.macro_name(), Some("PP" | "LP")) => {
            append_blank_line(output, maximum)?;
        }
        NodeKind::Element if node.macro_name() == Some("sp") => {
            // A boxed tbl's trailing device border already occupies one
            // vertical slot (two for `doublebox`).  Its first following
            // positive `.sp` consumes exactly those border slots before
            // requesting any additional blank lines.  Borderless tables do
            // not manufacture a slot, so a following `.sp` remains visible.
            // Negative requests keep their independent deferred semantics.
            let table_slots = take_terminal_table_vertical_skips(output);
            let span = node
                .children()
                .find_map(NodeRef::text)
                .and_then(terminal_vertical_span)
                .unwrap_or(1);
            let span = if span.is_positive() {
                span.saturating_sub(isize::try_from(table_slots).unwrap_or(isize::MAX))
            } else {
                span
            };
            append_terminal_vertical_space(output, span, maximum)?;
        }
        NodeKind::Element if node.macro_name() == Some("br") => {
            // A stray man `.RE` is recovered as a line-breaking `br` beside
            // the field it tried to close.  `post_IP()` has already left its
            // ordinary paragraph slot in the output, whereas term.c lets the
            // recovered close resume directly on the following device line.
            // Real `.br` requests remain below the active field Body, so the
            // sibling relationship is the required narrow discriminator.
            if terminal_man_field_sibling_break(node) && output.ends_with("\n\n") {
                output.pop();
            }
            if !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
        }
        NodeKind::Equation => {
            if let Some(value) = node.equation() {
                // Equation lowering uses the same portable special-character
                // spellings as text nodes (for example `\\[*a]`).  They are
                // deliberately retained in the public AST, but the terminal
                // devices resolve them to their glyph (or ASCII fallback)
                // before the normal line-wrapping pass.
                let rendered = node
                    .equation_terminal()
                    .map(|equation| render_terminal_equation(equation, format, limits))
                    .filter(|rendered| !rendered.is_empty())
                    .unwrap_or_else(|| render_terminal_equation_text(value, format, limits));
                append_terminal_text(
                    output,
                    &rendered,
                    TerminalTextLayout::default(),
                    indentation,
                    maximum,
                )?;
            }
        }
        NodeKind::Table => {
            render_terminal_table(node, format, limits, indentation, output, maximum)?;
        }
        _ => {
            for child in node.children() {
                render_terminal_node(child, format, limits, indentation, output, maximum)?;
            }
        }
    }
    Ok(())
}
