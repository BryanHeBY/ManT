use super::{
    Limits, RENDER_LITERAL_BACKSLASH_MARKER, RenderError, RenderFormat,
    TERMINAL_ATTACH_NEXT_MARKER, TERMINAL_CENTER_MARKER, TERMINAL_CONTINUE_SOURCE_LINE_MARKER,
    TERMINAL_FORCE_SEPARATOR_MARKER, TERMINAL_HANGING_INDENT_MARKER, TERMINAL_KEEP_SPACING_MARKER,
    TERMINAL_LINE_LENGTH_MARKER, TERMINAL_LITERAL_PUNCTUATION_MARKER, TERMINAL_LITERAL_TAB_MARKER,
    TERMINAL_NO_HYPHEN_BREAK_MARKER, TERMINAL_NO_SPACE_MARKER, TERMINAL_NO_WRAP_MARKER,
    TERMINAL_NONBREAKING_SPACE_MARKER, TERMINAL_OPTIONAL_BREAK_MARKER,
    TERMINAL_PENDING_LINE_BREAK_MARKER, TERMINAL_RIGHT_MARKER, TERMINAL_SENTENCE_PENDING_MARKER,
    TERMINAL_SENTENCE_SPACE_MARKER, TERMINAL_TABLE_VERTICAL_SKIP_MARKER,
    TERMINAL_TEMPORARY_INDENT_MARKER, TERMINAL_VERTICAL_SKIP_MARKER,
    TERMINAL_ZERO_WIDTH_BACKSPACE_MARKER, TerminalLineLength, TerminalTabStops, UnicodeWidthChar,
    append, ascii_terminal_character, render_numeric_character_escapes,
    render_terminal_whitespace_escapes, render_unicode_character_escapes,
    take_terminal_table_vertical_skip, take_terminal_vertical_skip,
    terminal_apply_tab_stop_request, terminal_tab_next, terminal_tab_stop_request,
};

pub(in crate::renderer) fn append_blank_line(
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    if output.ends_with(TERMINAL_SENTENCE_PENDING_MARKER) {
        let _ = output.pop();
    }
    // `term_vspace()` consumes a deferred negative `.sp` request before it
    // emits anything.  In particular, `.sp -1v` followed by `.PP` leaves one
    // ordinary line break rather than a blank paragraph gap.
    if take_terminal_vertical_skip(output) {
        return Ok(());
    }
    if take_terminal_table_vertical_skip(output) {
        return Ok(());
    }
    if output.is_empty() || output.ends_with("\n\n") {
        return Ok(());
    }
    if output.ends_with('\n') {
        append(output, "\n", maximum)
    } else {
        append(output, "\n\n", maximum)
    }
}

/// Decode a private `.ti` marker at the start of one pending rendered line.
/// An incomplete marker is deliberately consumed as no temporary indentation:
/// it can only arise from a bounded-output truncation and must never leak to
/// caller-visible terminal text.
pub(in crate::renderer) fn terminal_temporary_indent(line: &str) -> (Option<usize>, &str) {
    let Some(encoded) = line.strip_prefix(TERMINAL_TEMPORARY_INDENT_MARKER) else {
        return (None, line);
    };
    let Some(end) = encoded.find(TERMINAL_TEMPORARY_INDENT_MARKER) else {
        return (None, "");
    };
    let value = encoded[..end].parse().ok();
    let remainder = &encoded[end + TERMINAL_TEMPORARY_INDENT_MARKER.len_utf8()..];
    (value, remainder)
}

/// Decode a private man `.HP` continuation marker at the start of a pending
/// rendered line. It shares the paired marker encoding used by `.ti`, but
/// affects wrapped lines rather than the first line.
pub(in crate::renderer) fn terminal_hanging_indent(line: &str) -> (Option<usize>, &str) {
    let Some(encoded) = line.strip_prefix(TERMINAL_HANGING_INDENT_MARKER) else {
        return (None, line);
    };
    let Some(end) = encoded.find(TERMINAL_HANGING_INDENT_MARKER) else {
        return (None, "");
    };
    let value = encoded[..end].parse().ok();
    let remainder = &encoded[end + TERMINAL_HANGING_INDENT_MARKER.len_utf8()..];
    (value, remainder)
}

/// Decode one pending roff `.ll` field width. Invalid private encodings use
/// the caller-configured width, keeping output bounded and never exposing a
/// layout marker to public terminal text.
pub(in crate::renderer) fn terminal_line_length(line: &str, default: usize) -> (usize, &str) {
    let Some(encoded) = line.strip_prefix(TERMINAL_LINE_LENGTH_MARKER) else {
        return (default, line);
    };
    let Some(end) = encoded.find(TERMINAL_LINE_LENGTH_MARKER) else {
        return (default, "");
    };
    let value = &encoded[..end];
    let state = if value == "D" {
        TerminalLineLength::Default
    } else if let Some(value) = value
        .strip_prefix('A')
        .and_then(|value| value.parse::<usize>().ok())
    {
        TerminalLineLength::Absolute(value)
    } else if let Some(value) = value
        .strip_prefix('R')
        .and_then(|value| value.parse::<isize>().ok())
    {
        TerminalLineLength::Relative(value)
    } else {
        TerminalLineLength::Default
    };
    let remainder = &encoded[end + TERMINAL_LINE_LENGTH_MARKER.len_utf8()..];
    (terminal_line_length_value(state, default), remainder)
}

/// Resolve a reconstructed `.ll` register for one terminal device field.
/// Keeping this separate from marker decoding lets layout primitives (notably
/// tbl's eager `x` width calculation) use the same state before a raw line
/// exists to carry a private marker.
pub(in crate::renderer) fn terminal_line_length_value(
    state: TerminalLineLength,
    default: usize,
) -> usize {
    match state {
        TerminalLineLength::Default => default,
        TerminalLineLength::Absolute(value) => value,
        TerminalLineLength::Relative(delta) => default.saturating_add_signed(delta),
    }
    .max(1)
}

#[derive(Clone, Copy, Default)]
enum TerminalAlignment {
    #[default]
    Left,
    Center,
    Right,
}

struct TerminalLayoutLine<'a> {
    alignment: TerminalAlignment,
    no_wrap: bool,
    literal_tabs: bool,
    keep_spacing: bool,
    width: usize,
    temporary_indent: Option<usize>,
    hanging_indent: Option<usize>,
    text: &'a str,
}

impl<'a> TerminalLayoutLine<'a> {
    fn decode(raw: &'a str, default_width: usize) -> Self {
        let (alignment, raw) = if let Some(line) = raw.strip_prefix(TERMINAL_CENTER_MARKER) {
            (TerminalAlignment::Center, line)
        } else if let Some(line) = raw.strip_prefix(TERMINAL_RIGHT_MARKER) {
            (TerminalAlignment::Right, line)
        } else {
            (TerminalAlignment::Left, raw)
        };
        let (no_wrap, raw) = take_flag(raw, TERMINAL_NO_WRAP_MARKER);
        let (literal_tabs, raw) = take_flag(raw, TERMINAL_LITERAL_TAB_MARKER);
        let (keep_spacing, raw) = take_flag(raw, TERMINAL_KEEP_SPACING_MARKER);
        let (width, raw) = terminal_line_length(raw, default_width);
        let (temporary_indent, raw) = terminal_temporary_indent(raw);
        let (hanging_indent, text) = terminal_hanging_indent(raw);
        Self {
            alignment,
            no_wrap,
            literal_tabs,
            keep_spacing,
            width,
            temporary_indent,
            hanging_indent,
            text,
        }
    }

    fn align(self, output: &mut String, start: usize, maximum: usize) -> Result<(), RenderError> {
        match self.alignment {
            TerminalAlignment::Left => Ok(()),
            TerminalAlignment::Center => {
                center_terminal_output_segment(output, start, self.width, maximum)
            }
            TerminalAlignment::Right => {
                right_adjust_terminal_output_segment(output, start, self.width, maximum)
            }
        }
    }
}

fn take_flag(input: &str, marker: char) -> (bool, &str) {
    input
        .strip_prefix(marker)
        .map_or((false, input), |remainder| (true, remainder))
}

struct TerminalLayoutProgram<'a> {
    encoded: &'a str,
    visible_lines: usize,
}

impl<'a> TerminalLayoutProgram<'a> {
    fn decode(encoded: &'a str) -> Self {
        let visible_lines = encoded
            .split('\n')
            .filter(|line| terminal_tab_stop_request(line).is_none())
            .count();
        Self {
            encoded,
            visible_lines,
        }
    }
}

/// Wrap filled terminal prose with Unicode display-width accounting.
///
/// Explicit line breaks and table tabs remain structural boundaries. The
/// parser already records literal/no-fill layout separately; this conservative
/// first terminal pass therefore wraps only ordinary whitespace-separated
/// prose and never truncates a single long token.
#[allow(clippy::too_many_lines)] // Terminal wrapping keeps all width and marker state in one ordered pass.
pub(in crate::renderer) fn wrap_terminal_output(
    input: &str,
    width: usize,
    maximum: usize,
    protected_header_lines: usize,
    protected_footer_lines: usize,
) -> Result<String, RenderError> {
    let input = input.replace(
        [
            TERMINAL_ATTACH_NEXT_MARKER,
            TERMINAL_LITERAL_PUNCTUATION_MARKER,
            TERMINAL_FORCE_SEPARATOR_MARKER,
            TERMINAL_CONTINUE_SOURCE_LINE_MARKER,
            TERMINAL_VERTICAL_SKIP_MARKER,
            TERMINAL_TABLE_VERTICAL_SKIP_MARKER,
            TERMINAL_NO_SPACE_MARKER,
        ],
        "",
    );
    // `.ta` state commands occupy private source-order lines.  Consume them
    // before counting device lines so they neither create blank output nor
    // perturb the header/footer protection indexes.
    let mut tab_stops = TerminalTabStops {
        periodic: vec![5],
        ..TerminalTabStops::default()
    };
    let program = TerminalLayoutProgram::decode(&input);
    let mut output = String::new();
    let mut line_index = 0_usize;
    for raw_line in program.encoded.split('\n') {
        if let Some(request) = terminal_tab_stop_request(raw_line) {
            terminal_apply_tab_stop_request(&mut tab_stops, request);
            continue;
        }
        if line_index > 0 {
            append(&mut output, "\n", maximum)?;
        }
        let output_start = output.len();
        let layout = TerminalLayoutLine::decode(raw_line, width);
        let line = layout.text;
        // The default `T`/`.5i` tab policy starts at the fifth column of the
        // text field, then advances in five-column fields. The distinct
        // `Bd -literal` device state uses eight-column stops unless an
        // authored `.ta` request has supplied an explicit configuration.
        let expanded = line.contains('\t').then(|| {
            if tab_stops.configured {
                expand_terminal_tabs(line, &tab_stops)
            } else if layout.literal_tabs {
                expand_literal_terminal_tabs(line)
            } else {
                expand_filled_terminal_tabs(line)
            }
        });
        let line = expanded.as_deref().unwrap_or(line);
        if line_index < protected_header_lines
            || line_index >= program.visible_lines.saturating_sub(protected_footer_lines)
            || layout.no_wrap
            || layout.keep_spacing
            || line.is_empty()
            || line.contains('\t')
        {
            let temporary_line = layout.temporary_indent.map(|target| {
                let indentation = line.bytes().take_while(|byte| *byte == b' ').count();
                format!("{}{}", " ".repeat(target), &line[indentation..])
            });
            let line = temporary_line.as_deref().unwrap_or(line);
            let line = line
                .replace(
                    [
                        TERMINAL_SENTENCE_SPACE_MARKER,
                        TERMINAL_OPTIONAL_BREAK_MARKER,
                        TERMINAL_NO_HYPHEN_BREAK_MARKER,
                        TERMINAL_SENTENCE_PENDING_MARKER,
                    ],
                    "",
                )
                .replace(TERMINAL_NONBREAKING_SPACE_MARKER, " ");
            append(&mut output, &line, maximum)?;
            layout.align(&mut output, output_start, maximum)?;
            line_index += 1;
            continue;
        }
        let indent_width = line.bytes().take_while(|byte| *byte == b' ').count();
        let (indent, content) = line.split_at(indent_width);
        let initial_indent_width = layout.temporary_indent.unwrap_or(indent_width);
        let initial_indent = layout
            .temporary_indent
            .map_or_else(|| indent.to_owned(), |target| " ".repeat(target));
        let continuation_indent_width = layout.hanging_indent.unwrap_or(indent_width);
        let continuation_indent = layout
            .hanging_indent
            .map_or_else(|| indent.to_owned(), |target| " ".repeat(target));
        let mut current_width = 0_usize;
        let mut first_word = true;
        let mut initial_line = true;
        let mut sentence_spacing = false;
        for raw_word in content.split_whitespace() {
            if raw_word == "\u{1b}" {
                sentence_spacing = true;
                continue;
            }
            let no_hyphen_break = raw_word.contains(TERMINAL_NO_HYPHEN_BREAK_MARKER);
            let word = raw_word.replace(
                [
                    TERMINAL_OPTIONAL_BREAK_MARKER,
                    TERMINAL_NO_HYPHEN_BREAK_MARKER,
                    TERMINAL_SENTENCE_PENDING_MARKER,
                ],
                "",
            );
            let word_width = display_width(&word);
            let separator = if first_word {
                0
            } else if sentence_spacing {
                2
            } else {
                1
            };
            if first_word
                && raw_word.contains(TERMINAL_OPTIONAL_BREAK_MARKER)
                && initial_indent_width.saturating_add(word_width) > layout.width
                && let Some((prefix, suffix)) = terminal_optional_break(
                    raw_word,
                    layout.width.saturating_sub(initial_indent_width),
                )
            {
                let prefix = prefix.replace(TERMINAL_OPTIONAL_BREAK_MARKER, "");
                let suffix = suffix.replace(TERMINAL_OPTIONAL_BREAK_MARKER, "");
                append(&mut output, &initial_indent, maximum)?;
                append(&mut output, &prefix, maximum)?;
                append(&mut output, "\n", maximum)?;
                append(&mut output, &continuation_indent, maximum)?;
                append(&mut output, &suffix, maximum)?;
                current_width = continuation_indent_width.saturating_add(display_width(&suffix));
                first_word = false;
                initial_line = false;
                sentence_spacing = false;
                continue;
            }
            if !first_word
                && current_width > 0
                && current_width
                    .saturating_add(separator)
                    .saturating_add(word_width)
                    > layout.width
            {
                let available = layout
                    .width
                    .saturating_sub(current_width.saturating_add(separator));
                if let Some((prefix, suffix)) = terminal_optional_break(raw_word, available) {
                    let prefix = prefix.replace(TERMINAL_OPTIONAL_BREAK_MARKER, "");
                    let suffix = suffix.replace(TERMINAL_OPTIONAL_BREAK_MARKER, "");
                    append(&mut output, &" ".repeat(separator), maximum)?;
                    append(&mut output, &prefix, maximum)?;
                    append(&mut output, "\n", maximum)?;
                    append(&mut output, &continuation_indent, maximum)?;
                    append(&mut output, &suffix, maximum)?;
                    current_width =
                        continuation_indent_width.saturating_add(display_width(&suffix));
                    first_word = false;
                    initial_line = false;
                    sentence_spacing = false;
                    continue;
                }
                if !no_hyphen_break
                    && let Some((prefix, suffix)) = terminal_hyphen_break(&word, available)
                {
                    append(&mut output, &" ".repeat(separator), maximum)?;
                    append(&mut output, prefix, maximum)?;
                    append(&mut output, "\n", maximum)?;
                    append(&mut output, &continuation_indent, maximum)?;
                    append(&mut output, suffix, maximum)?;
                    current_width = continuation_indent_width.saturating_add(display_width(suffix));
                    first_word = false;
                    initial_line = false;
                    sentence_spacing = false;
                    continue;
                }
                append(&mut output, "\n", maximum)?;
                append(&mut output, &continuation_indent, maximum)?;
                current_width = continuation_indent_width;
                first_word = true;
                initial_line = false;
            }
            if first_word {
                if initial_line && current_width == 0 {
                    append(&mut output, &initial_indent, maximum)?;
                    current_width = initial_indent_width;
                }
            } else {
                append(&mut output, &" ".repeat(separator), maximum)?;
                current_width = current_width.saturating_add(separator);
            }
            append(&mut output, &word, maximum)?;
            current_width = current_width.saturating_add(word_width);
            first_word = false;
            sentence_spacing = false;
        }
        layout.align(&mut output, output_start, maximum)?;
        line_index += 1;
    }
    Ok(output
        .replace(TERMINAL_NONBREAKING_SPACE_MARKER, " ")
        .replace(TERMINAL_SENTENCE_PENDING_MARKER, ""))
}

/// Center a just-emitted display fragment inside the visible field already
/// represented by its leading indentation.  The fragment is limited to one
/// source display line, but normal wrapping may have introduced additional
/// physical lines; each receives its own centering calculation.
pub(in crate::renderer) fn center_terminal_output_segment(
    output: &mut String,
    start: usize,
    width: usize,
    maximum: usize,
) -> Result<(), RenderError> {
    let fragment = output.split_off(start);
    for (index, line) in fragment.split('\n').enumerate() {
        if index > 0 {
            append(output, "\n", maximum)?;
        }
        if line.is_empty() {
            continue;
        }
        let indentation = line.bytes().take_while(|byte| *byte == b' ').count();
        let content_width = display_width(&line[indentation..]);
        let leading = width
            .saturating_sub(indentation)
            .saturating_sub(content_width)
            / 2;
        append(output, &" ".repeat(leading), maximum)?;
        append(output, line, maximum)?;
    }
    Ok(())
}

/// Right-align a completed no-fill roff request at the device margin.
/// `.rj` is distinct from a section or display field: it uses the page's
/// current right column, so the marker's payload begins with no field prefix.
pub(in crate::renderer) fn right_adjust_terminal_output_segment(
    output: &mut String,
    start: usize,
    width: usize,
    maximum: usize,
) -> Result<(), RenderError> {
    let fragment = output.split_off(start);
    for (index, line) in fragment.split('\n').enumerate() {
        if index > 0 {
            append(output, "\n", maximum)?;
        }
        if line.is_empty() {
            continue;
        }
        append(
            output,
            &" ".repeat(width.saturating_sub(display_width(line))),
            maximum,
        )?;
        append(output, line, maximum)?;
    }
    Ok(())
}

pub(in crate::renderer) fn terminal_hyphen_break(
    word: &str,
    available: usize,
) -> Option<(&str, &str)> {
    let hyphen = word.rfind('-')?;
    let (prefix, suffix) = word.split_at(hyphen + 1);
    (!suffix.is_empty() && display_width(prefix) <= available).then_some((prefix, suffix))
}

pub(in crate::renderer) fn terminal_optional_break(
    word: &str,
    available: usize,
) -> Option<(&str, &str)> {
    word.match_indices(TERMINAL_OPTIONAL_BREAK_MARKER)
        .filter_map(|(offset, _)| {
            let prefix = &word[..offset];
            let suffix = &word[offset + TERMINAL_OPTIONAL_BREAK_MARKER.len_utf8()..];
            (!suffix.is_empty()
                && display_width(&prefix.replace(TERMINAL_OPTIONAL_BREAK_MARKER, "")) <= available)
                .then_some((prefix, suffix))
        })
        .next_back()
}

pub(in crate::renderer) fn expand_filled_terminal_tabs(line: &str) -> String {
    expand_terminal_tabs(
        line,
        &TerminalTabStops {
            periodic: vec![5],
            ..TerminalTabStops::default()
        },
    )
}

pub(in crate::renderer) fn expand_terminal_tabs(
    line: &str,
    tab_stops: &TerminalTabStops,
) -> String {
    let indentation = line.bytes().take_while(|byte| *byte == b' ').count();
    let (prefix, content) = line.split_at(indentation);
    let mut output = String::with_capacity(line.len().saturating_add(8));
    output.push_str(prefix);
    let mut column = 0_usize;
    let mut characters = content.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\t' {
            let next = terminal_tab_next(tab_stops, column);
            let spaces = next.saturating_sub(column);
            output.push_str(&" ".repeat(spaces));
            column = next;
            continue;
        }
        output.push(character);
        column = column.saturating_add(terminal_character_width(character));
        if characters.peek() == Some(&'\u{8}') {
            output.push(characters.next().expect("peeked overstrike is present"));
            if let Some(overstrike) = characters.next() {
                output.push(overstrike);
            }
        }
    }
    output
}

pub(in crate::renderer) fn expand_literal_terminal_tabs(line: &str) -> String {
    expand_terminal_tabs(
        line,
        &TerminalTabStops {
            periodic: vec![8],
            ..TerminalTabStops::default()
        },
    )
}

pub(in crate::renderer) fn display_width(value: &str) -> usize {
    let mut column = 0_usize;
    let mut maximum = 0_usize;
    for character in value.chars() {
        match character {
            '\n' => column = 0,
            // The terminal's historical emphasis streams can contain more
            // than one consecutive overstrike (the bullet is
            // `+\b+\bo\bo`).  Track the actual cursor rather than assuming
            // each backspace occurs only in a two-glyph pair.
            '\u{8}' => column = column.saturating_sub(1),
            character => {
                let width = terminal_character_width(character);
                column = column.saturating_add(width);
                maximum = maximum.max(column);
            }
        }
    }
    maximum
}

/// Width of one terminal device character.
///
/// mandoc's pinned UTF-8 regressions use the platform `wcwidth()` contract.
/// Hangul Jamo Extended-B is double-width there, while `unicode-width` treats
/// it as a zero-width combining range.  Keep that device distinction local to
/// terminal geometry so source text and the public AST retain their Unicode
/// spelling unchanged.
pub(in crate::renderer) fn terminal_character_width(character: char) -> usize {
    if character == TERMINAL_NONBREAKING_SPACE_MARKER {
        return 1;
    }
    let scalar = u32::from(character);
    // The reference terminal coerces negative `wcwidth()` results to zero.
    // These stable-regression scalars are unassigned or noncharacters in the
    // pinned device table, while `unicode-width` reports one cell for them.
    // Keep that distinction local to terminal geometry: source text and the
    // public AST retain their authored spelling unchanged.
    if matches!(
        scalar,
        0x0fff | 0xd7ff | 0x3ffff | 0x40000 | 0xc0000 | 0xeffff | 0xfffff
    ) || matches!(scalar & 0xffff, 0xfffe | 0xffff)
    {
        return 0;
    }
    if matches!(character, '\u{d7b0}'..='\u{d7fb}') {
        return 2;
    }
    UnicodeWidthChar::width(character).unwrap_or(0)
}

/// Interpret presentation-only roff escapes after parsing has preserved their
/// authored spelling in the public AST.
///
/// Parsing deliberately retains several formatter controls because source
/// fidelity, diagnostics, and downstream lowering need that spelling. A
/// reference renderer instead consumes the zero-width controls and resolves
/// named characters. Numeric `\\N'…'` escapes are a renderer concern too: the
/// stable mandoc ASCII device accepts only its one-byte character domain.
pub(in crate::renderer) fn render_visible_text(
    text: &str,
    format: RenderFormat,
    limits: &Limits,
) -> String {
    let device_strings = render_default_device_string_escapes(text, format);
    let whitespace = render_terminal_whitespace_escapes(&device_strings);
    // Mandoc's two-character `~=` and `~~` names share U+2248 in the
    // character catalogue but use distinct ASCII-device spellings. Preserve
    // the only ambiguous source form before scalar normalization erases that
    // distinction; all other formats intentionally use the common scalar.
    let whitespace = if format == RenderFormat::Ascii {
        whitespace.replace(r"\(~=", "~=")
    } else {
        whitespace
    };
    let unicode = render_unicode_character_escapes(&whitespace, format);
    let numeric = render_numeric_character_escapes(&unicode, format);
    let normalized = crate::escape::normalize_escapes(numeric.as_bytes(), b'\\', limits)
        .text
        .replace(RENDER_LITERAL_BACKSLASH_MARKER, "\\");
    if format == RenderFormat::Ascii {
        ascii_terminal_text(&normalized)
    } else {
        normalized
    }
}

/// Resolve the formatter's default `.T` string only in presentation.
///
/// The parser intentionally retains `\*(.T` and `\*[.T]` in the compatible
/// public AST until a user `.ds .T` override exists.  The terminal and HTML
/// formatters, however, expose their own device name at render time.  Treat a
/// doubled escape as literal input so this renderer-only substitution cannot
/// reinterpret an explicitly escaped spelling.
pub(in crate::renderer) fn render_default_device_string_escapes(
    text: &str,
    format: RenderFormat,
) -> String {
    let device = match format {
        RenderFormat::Ascii => "ascii",
        RenderFormat::Utf8 => "utf8",
        RenderFormat::Html => "html",
    };
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes.get(cursor..cursor.saturating_add(2)) == Some(b"\\\\") {
            output.push_str("\\\\");
            cursor = cursor.saturating_add(2);
            continue;
        }
        if bytes.get(cursor..cursor.saturating_add(5)) == Some(b"\\*(.T") {
            output.push_str(device);
            cursor = cursor.saturating_add(5);
            continue;
        }
        if bytes.get(cursor..cursor.saturating_add(6)) == Some(b"\\*[.T]") {
            output.push_str(device);
            cursor = cursor.saturating_add(6);
            continue;
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor remains within a valid UTF-8 string");
        output.push(character);
        cursor = cursor.saturating_add(character.len_utf8());
    }
    output
}

pub(in crate::renderer) fn ascii_terminal_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(
            character,
            TERMINAL_ZERO_WIDTH_BACKSPACE_MARKER | TERMINAL_PENDING_LINE_BREAK_MARKER
        ) {
            output.push(character);
            continue;
        }
        match character {
            // The ASCII device encodes these arrows as overstruck glyphs.
            '\u{2191}' => output.push_str("|\u{8}^"),
            '\u{21d1}' => output.push_str("=\u{8}^"),
            // The named combining-accent fallbacks occupy one terminal
            // column in mandoc's ASCII device rather than becoming `?`.
            '\u{00b4}' => output.push('\''),
            '\u{02dd}' | '\u{00a8}' => output.push('"'),
            '\u{00b8}' | '\u{02db}' => output.push(','),
            '\u{02c7}' => output.push('v'),
            '\u{02da}' => output.push('o'),
            // Punctuation names use printable device fallbacks in ASCII.
            '\u{2010}' | '\u{2013}' => output.push('-'),
            '\u{2014}' => output.push_str("--"),
            '\u{2018}' => output.push('`'),
            '\u{2019}' => output.push('\''),
            '\u{201a}' => output.push(','),
            '\u{201c}' | '\u{201d}' => output.push('"'),
            '\u{201e}' => output.push_str(",,"),
            '\u{226a}' => output.push_str("<<"),
            '\u{226b}' => output.push_str(">>"),
            // The terminal table draws extensible delimiters with their
            // portable ASCII pieces rather than treating each Unicode scalar
            // as an unsupported glyph.
            '\u{203e}' => output.push('-'),
            '\u{210f}' => output.push_str("/h"),
            '\u{2195}' => output.push_str("^v"),
            '\u{21d5}' => output.push_str("^=v"),
            '\u{239b}' | '\u{23a0}' => output.push('/'),
            '\u{239c}' | '\u{239f}' | '\u{23a1}' | '\u{23a2}' | '\u{23a3}' | '\u{23a4}'
            | '\u{23a5}' | '\u{23a6}' | '\u{23aa}' => output.push('|'),
            '\u{239d}' | '\u{239e}' => output.push('\\'),
            '\u{23a7}' => output.push_str(",-"),
            '\u{23a8}' => output.push('{'),
            '\u{23a9}' => output.push_str("`-"),
            '\u{23ab}' => output.push_str("-."),
            '\u{23ac}' => output.push('}'),
            '\u{23ad}' => output.push_str("-'"),
            _ => {
                if let Some(fallback) = ascii_terminal_catalog_fallback(character) {
                    output.push_str(fallback);
                } else {
                    output.push(ascii_terminal_character(character));
                }
            }
        }
    }
    output
}

pub(in crate::renderer) fn ascii_terminal_named_scalar_is_known(character: char) -> bool {
    ascii_terminal_catalog_fallback(character).is_some()
        || matches!(
            character,
            '\u{2191}'
                | '\u{21d1}'
                | '\u{00b4}'
                | '\u{02dd}'
                | '\u{00a8}'
                | '\u{00b8}'
                | '\u{02db}'
                | '\u{02c7}'
                | '\u{02da}'
                | '\u{2010}'
                | '\u{2013}'
                | '\u{2014}'
                | '\u{2212}'
                | '\u{2018}'
                | '\u{2019}'
                | '\u{201a}'
                | '\u{201c}'
                | '\u{201d}'
                | '\u{201e}'
                | '\u{226a}'
                | '\u{226b}'
                | '\u{203e}'
                | '\u{210f}'
                | '\u{2195}'
                | '\u{21d5}'
                | '\u{239b}'
                | '\u{23a0}'
                | '\u{239c}'
                | '\u{239f}'
                | '\u{23a1}'
                | '\u{23a2}'
                | '\u{23a3}'
                | '\u{23a4}'
                | '\u{23a5}'
                | '\u{23a6}'
                | '\u{23aa}'
                | '\u{239d}'
                | '\u{239e}'
                | '\u{23a7}'
                | '\u{23a8}'
                | '\u{23a9}'
                | '\u{23ab}'
                | '\u{23ac}'
                | '\u{23ad}'
        )
}

/// ASCII device spellings for catalog scalars that cannot occupy one
/// printable ASCII cell. The table is pinned to mandoc 1.14.6.
pub(in crate::renderer) fn ascii_terminal_catalog_fallback(
    character: char,
) -> Option<&'static str> {
    let fallback = match character {
        // Latin-1 symbols and letters are emitted through the same catalog
        // as `\\[u00xx]` escapes.  Preserve mandoc's terminal-device
        // spellings, including its backspace overstrikes for diacritics.
        '\u{00a1}' => "!",
        '\u{00a2}' => "/\x08c",
        '\u{00a3}' => "-\x08L",
        '\u{00a4}' => "o\x08x",
        '\u{00a5}' => "=\x08Y",
        '\u{00a6}' => "|",
        '\u{00a7}' => "<section>",
        '\u{00a9}' => "(C)",
        '\u{00aa}' => "_\x08a",
        '\u{00ab}' => "<<",
        '\u{00ac}' => "~",
        '\u{00ad}' => "",
        '\u{00ae}' => "(R)",
        '\u{00af}' => "-",
        '\u{00b0}' => "<degree>",
        '\u{00b1}' => "+-",
        '\u{00b2}' => "^2",
        '\u{00b3}' => "^3",
        '\u{00b5}' => "<micro>",
        '\u{00b6}' => "<paragraph>",
        '\u{00b7}' => ".",
        '\u{00b9}' => "^1",
        '\u{00ba}' => "_\x08o",
        '\u{00bb}' => ">>",
        '\u{00bc}' => "1/4",
        '\u{00bd}' => "1/2",
        '\u{00be}' => "3/4",
        '\u{00bf}' => "?",
        '\u{00c0}' => "\x60\x08A",
        '\u{00c1}' => "\x27\x08A",
        '\u{00c2}' => "^\x08A",
        '\u{00c3}' => "~\x08A",
        '\u{00c4}' => "\"\x08A",
        '\u{00c5}' => "o\x08A",
        '\u{00c6}' => "AE",
        '\u{00c7}' => ",\x08C",
        '\u{00c8}' => "\x60\x08E",
        '\u{00c9}' => "\x27\x08E",
        '\u{00ca}' => "^\x08E",
        '\u{00cb}' => "\"\x08E",
        '\u{00cc}' => "\x60\x08I",
        '\u{00cd}' => "\x27\x08I",
        '\u{00ce}' => "^\x08I",
        '\u{00cf}' => "\"\x08I",
        '\u{00d0}' => "Dh",
        '\u{00d1}' => "~\x08N",
        '\u{00d2}' => "\x60\x08O",
        '\u{00d3}' => "\x27\x08O",
        '\u{00d4}' => "^\x08O",
        '\u{00d5}' => "~\x08O",
        '\u{00d6}' => "\"\x08O",
        '\u{00d7}' => "x",
        '\u{00d8}' => "/\x08O",
        '\u{00d9}' => "\x60\x08U",
        '\u{00da}' => "\x27\x08U",
        '\u{00db}' => "^\x08U",
        '\u{00dc}' => "\"\x08U",
        '\u{00dd}' => "\x27\x08Y",
        '\u{00de}' => "Th",
        '\u{00df}' => "ss",
        '\u{00e0}' => "\x60\x08a",
        '\u{00e1}' => "\x27\x08a",
        '\u{00e2}' => "^\x08a",
        '\u{00e3}' => "~\x08a",
        '\u{00e4}' => "\"\x08a",
        '\u{00e5}' => "o\x08a",
        '\u{00e6}' => "ae",
        '\u{00e7}' => ",\x08c",
        '\u{00e8}' => "\x60\x08e",
        '\u{00e9}' => "\x27\x08e",
        '\u{00ea}' => "^\x08e",
        '\u{00eb}' => "\"\x08e",
        '\u{00ec}' => "\x60\x08i",
        '\u{00ed}' => "\x27\x08i",
        '\u{00ee}' => "^\x08i",
        '\u{00ef}' => "\"\x08i",
        '\u{00f0}' => "dh",
        '\u{00f1}' => "~\x08n",
        '\u{00f2}' => "\x60\x08o",
        '\u{00f3}' => "\x27\x08o",
        '\u{00f4}' => "^\x08o",
        '\u{00f5}' => "~\x08o",
        '\u{00f6}' => "\"\x08o",
        '\u{00f7}' => "/",
        '\u{00f8}' => "/\x08o",
        '\u{00f9}' => "\x60\x08u",
        '\u{00fa}' => "\x27\x08u",
        '\u{00fb}' => "^\x08u",
        '\u{00fc}' => "\"\x08u",
        '\u{00fd}' => "\x27\x08y",
        '\u{00fe}' => "th",
        '\u{00ff}' => "\"\x08y",
        '\u{02d8}' => "\x27\x08\x60",
        '\u{02d9}' => ".",
        // The stable Unicode-name regression also exercises the portable
        // punctuation, mathematical, and symbol catalogue.  These strings
        // are the ASCII column of mandoc 1.14.6's `chars.c` table.
        '\u{2020}' => "<*>",
        '\u{2021}' => "<**>",
        '\u{2022}' => "+\x08o",
        '\u{2030}' => "<permille>",
        '\u{2032}' => "'",
        '\u{2033}' => "''",
        '\u{2039}' => "<",
        '\u{203a}' => ">",
        '\u{2044}' => "/",
        '\u{20ac}' => "EUR",
        '\u{2111}' => "<Im>",
        '\u{2118}' => "p",
        '\u{211c}' => "<Re>",
        '\u{2122}' => "tm",
        '\u{2135}' => "<Aleph>",
        '\u{215b}' => "1/8",
        '\u{215c}' => "3/8",
        '\u{215d}' => "5/8",
        '\u{215e}' => "7/8",
        '\u{2190}' => "<-",
        '\u{2192}' => "->",
        '\u{2193}' => "|\x08v",
        '\u{2194}' => "<->",
        '\u{21b5}' => "<cr>",
        '\u{21d0}' => "<=",
        '\u{21d2}' => "=>",
        '\u{21d3}' => "=\x08v",
        '\u{21d4}' => "<=>",
        '\u{2200}' => "<for all>",
        '\u{2202}' => "<del>",
        '\u{2203}' => "<there exists>",
        '\u{2205}' => "{}",
        '\u{2207}' => "<nabla>",
        '\u{2208}' => "<element of>",
        '\u{2209}' => "<not element of>",
        '\u{220b}' => "<such that>",
        '\u{220f}' => "<product>",
        '\u{2210}' => "<coproduct>",
        '\u{2211}' => "<sum>",
        '\u{2213}' => "-+",
        '\u{2217}' => "*",
        '\u{221a}' => "<sqrt>",
        '\u{221d}' => "<proportional to>",
        '\u{221e}' => "<infinity>",
        '\u{2220}' => "<angle>",
        '\u{2227}' => "^",
        '\u{2228}' => "v",
        '\u{2229}' => "<intersection>",
        '\u{222a}' => "<union>",
        '\u{222b}' => "<integral>",
        '\u{2234}' => "<therefore>",
        '\u{223c}' => "~",
        '\u{2243}' => "-~",
        '\u{2245}' => "=~",
        '\u{2248}' => "~~",
        '\u{2260}' => "!=",
        '\u{2261}' => "==",
        '\u{2262}' => "!==",
        '\u{2264}' => "<=",
        '\u{2265}' => ">=",
        '\u{2282}' => "<proper subset>",
        '\u{2283}' => "<proper superset>",
        '\u{2284}' => "<not subset>",
        '\u{2285}' => "<not superset>",
        '\u{2286}' => "<subset or equal>",
        '\u{2287}' => "<superset or equal>",
        '\u{2295}' => "O\x08+",
        '\u{2297}' => "O\x08x",
        '\u{22a5}' => "<perpendicular>",
        '\u{22c5}' => ".",
        '\u{2308}' => "|~",
        '\u{2309}' => "~|",
        '\u{230a}' => "|_",
        '\u{230b}' => "_|",
        '\u{23af}' => "-",
        '\u{2502}' => "|",
        '\u{25a1}' => "[]",
        '\u{25ca}' => "<>",
        '\u{25cb}' => "O",
        '\u{261c}' => "<=",
        '\u{261e}' => "=>",
        '\u{2660}' => "S",
        '\u{2663}' => "C",
        '\u{2665}' => "H",
        '\u{2666}' => "D",
        '\u{27e8}' => "<",
        '\u{27e9}' => ">",
        '\u{0131}' => "i",
        '\u{0132}' => "IJ",
        '\u{0133}' => "ij",
        '\u{0141}' => "/\x08L",
        '\u{0142}' => "/\x08l",
        '\u{0152}' => "OE",
        '\u{0153}' => "oe",
        '\u{0192}' => ",\x08f",
        '\u{0237}' => "j",
        '\u{0391}' => "A",
        '\u{0392}' => "B",
        '\u{0393}' => "<Gamma>",
        '\u{0394}' => "<Delta>",
        '\u{0395}' => "E",
        '\u{0396}' => "Z",
        '\u{0397}' => "H",
        '\u{0398}' => "<Theta>",
        '\u{0399}' => "I",
        '\u{039a}' => "K",
        '\u{039b}' => "<Lambda>",
        '\u{039c}' => "M",
        '\u{039d}' => "N",
        '\u{039e}' => "<Xi>",
        '\u{039f}' => "O",
        '\u{03a0}' => "<Pi>",
        '\u{03a1}' => "P",
        '\u{03a3}' => "<Sigma>",
        '\u{03a4}' => "T",
        '\u{03a5}' => "Y",
        '\u{03a6}' => "<Phi>",
        '\u{03a7}' => "X",
        '\u{03a8}' => "<Psi>",
        '\u{03a9}' => "<Omega>",
        '\u{03b1}' => "<alpha>",
        '\u{03b2}' => "<beta>",
        '\u{03b3}' => "<gamma>",
        '\u{03b4}' => "<delta>",
        '\u{03b5}' => "<epsilon>",
        '\u{03b6}' => "<zeta>",
        '\u{03b7}' => "<eta>",
        '\u{03b8}' => "<theta>",
        '\u{03b9}' => "<iota>",
        '\u{03ba}' => "<kappa>",
        '\u{03bb}' => "<lambda>",
        '\u{03bc}' => "<mu>",
        '\u{03bd}' => "<nu>",
        '\u{03be}' => "<xi>",
        '\u{03bf}' => "o",
        '\u{03c0}' => "<pi>",
        '\u{03c1}' => "<rho>",
        '\u{03c2}' | '\u{03c3}' => "<sigma>",
        '\u{03c4}' => "<tau>",
        '\u{03c5}' => "<upsilon>",
        '\u{03c6}' => "<phi>",
        '\u{03c7}' => "<chi>",
        '\u{03c8}' => "<psi>",
        '\u{03c9}' => "<omega>",
        '\u{03d1}' => "<theta>",
        '\u{03d5}' => "<phi>",
        '\u{03d6}' => "<pi>",
        '\u{03f5}' => "<epsilon>",
        '\u{fb00}' => "ff",
        '\u{fb01}' => "fi",
        '\u{fb02}' => "fl",
        '\u{fb03}' => "ffi",
        '\u{fb04}' => "ffl",
        _ => return None,
    };
    Some(fallback)
}
