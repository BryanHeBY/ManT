use super::{
    HtmlFont, HtmlFontChange, HtmlRequestFontState, Limits, NodeKind, NodeRef,
    RENDER_LITERAL_BACKSLASH_MARKER, RenderError, RenderErrorKind, RenderFormat,
    TERMINAL_NONBREAKING_SPACE_MARKER, TERMINAL_PENDING_LINE_BREAK_MARKER,
    TERMINAL_ZERO_WIDTH_BACKSPACE_MARKER, TerminalFont, TerminalFontChange,
    ascii_terminal_named_scalar_is_known, display_width, render_terminal_font, render_visible_text,
    terminal_signed_roff_en_prefix,
};

/// Apply the 1.14.6 device's whitespace-escape recovery before generic
/// named-character normalization. Bracketed control spellings are silently
/// zero-width, while malformed names with a leading blank lose only their
/// introducer and keep the remaining authored bytes.
pub(super) fn render_terminal_whitespace_escapes(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        // Bracketed acute/grave spellings are source-visible invalid forms.
        // Package parsing keeps them for diagnostics, whereas the renderer
        // consumes them as zero-width controls. Their one-byte counterparts
        // remain ordinary visible accents.
        if bytes.get(cursor..cursor.saturating_add(4)) == Some(b"\\[']")
            || bytes.get(cursor..cursor.saturating_add(4)) == Some(b"\\[`]")
        {
            cursor += 4;
            continue;
        }
        if bytes.get(cursor..cursor.saturating_add(4)) == Some(b"\\[_]")
            || bytes.get(cursor..cursor.saturating_add(4)) == Some(b"\\[~]")
            || bytes.get(cursor..cursor.saturating_add(4)) == Some(b"\\[0]")
        {
            cursor += 4;
            continue;
        }
        if bytes.get(cursor..cursor.saturating_add(2)) == Some(b"\\~")
            || bytes.get(cursor..cursor.saturating_add(2)) == Some(b"\\0")
        {
            output.push(' ');
            cursor += 2;
            continue;
        }
        if bytes.get(cursor..cursor.saturating_add(3)) == Some(b"\\[ ") {
            cursor += 3;
            continue;
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor remains within a valid UTF-8 string");
        output.push(character);
        cursor += character.len_utf8();
    }
    output
}

/// Resolve terminal-visible text, including the inline bold form emitted by
/// man and mdoc sources. The public AST keeps font escapes verbatim, while
/// terminal output uses the same deterministic overstrike convention as
/// structural headings and `.B` elements.
pub(super) fn render_terminal_visible_text(
    text: &str,
    format: RenderFormat,
    limits: &Limits,
) -> String {
    render_terminal_visible_text_with_font(text, format, limits, TerminalFont::Roman)
}

pub(super) fn render_terminal_visible_text_with_font(
    text: &str,
    format: RenderFormat,
    limits: &Limits,
    initial_font: TerminalFont,
) -> String {
    // The library catalogue synthesizes the traditional two-character quote
    // names. Resolve them here with ordinary terminal text, so generated
    // unknown-library prose follows the same delimiter joining as authored
    // quotation marks.
    let text = match format {
        RenderFormat::Utf8 => text.replace(r"\(lq", "“").replace(r"\(rq", "”"),
        RenderFormat::Ascii | RenderFormat::Html => {
            text.replace(r"\(lq", "\"").replace(r"\(rq", "\"")
        }
    }
    // These traditional guillemet names use a two-cell ASCII terminal
    // fallback in mandoc rather than the generic non-ASCII replacement.
    .replace(
        r"\(Fo",
        if matches!(format, RenderFormat::Ascii) {
            "<<"
        } else {
            "«"
        },
    )
    .replace(
        r"\(Fc",
        if matches!(format, RenderFormat::Ascii) {
            ">>"
        } else {
            "»"
        },
    )
    .replace(r"\:", "\u{1a}");
    let text = render_terminal_roff_controls(&text, format, limits);
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut fragment = String::new();
    let mut cursor = 0_usize;
    let mut font = initial_font;
    let mut previous_font = initial_font;
    while cursor < bytes.len() {
        if let Some((next_cursor, change)) = terminal_font_escape(bytes, cursor) {
            let visible = render_terminal_visible_fragment(&fragment, format, limits);
            output.push_str(&render_terminal_font(&visible, font));
            fragment.clear();
            match change {
                TerminalFontChange::Set(next_font) => {
                    previous_font = font;
                    font = next_font;
                }
                TerminalFontChange::Restore => std::mem::swap(&mut font, &mut previous_font),
            }
            cursor = next_cursor;
            continue;
        }
        if text[cursor..].starts_with(TERMINAL_ZERO_WIDTH_BACKSPACE_MARKER) {
            let visible = render_terminal_visible_fragment(&fragment, format, limits);
            output.push_str(&render_terminal_font(&visible, font));
            fragment.clear();
            output.push('\u{8}');
            cursor += TERMINAL_ZERO_WIDTH_BACKSPACE_MARKER.len_utf8();
            continue;
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor remains within a valid UTF-8 string");
        fragment.push(character);
        cursor += character.len_utf8();
    }
    let visible = render_terminal_visible_fragment(&fragment, format, limits);
    output.push_str(&render_terminal_font(&visible, font));
    output
}

/// Decode the terminal device's accepted roff font selectors. Mandoc maps
/// bold-italic selectors to its underline/italic terminal convention, while
/// fixed-width aliases only select a style for the following fragment.
pub(super) fn terminal_font_escape(
    bytes: &[u8],
    cursor: usize,
) -> Option<(usize, TerminalFontChange)> {
    if bytes.get(cursor..cursor.saturating_add(2)) != Some(b"\\f") {
        return None;
    }
    let selector_start = cursor.saturating_add(2);
    let selector = *bytes.get(selector_start)?;
    let (name, next_cursor) = match selector {
        b'(' => {
            let name =
                bytes.get(selector_start.saturating_add(1)..selector_start.saturating_add(3))?;
            (name, selector_start.saturating_add(3))
        }
        b'[' => {
            let closing = bytes[selector_start.saturating_add(1)..]
                .iter()
                .position(|byte| *byte == b']')?;
            let name_end = selector_start.saturating_add(1).saturating_add(closing);
            (
                bytes.get(selector_start.saturating_add(1)..name_end)?,
                name_end.saturating_add(1),
            )
        }
        _ => (
            &bytes[selector_start..selector_start.saturating_add(1)],
            selector_start.saturating_add(1),
        ),
    };
    let change = match name {
        b"B" | b"3" | b"CB" => TerminalFontChange::Set(TerminalFont::Bold),
        b"I" | b"2" | b"CI" => TerminalFontChange::Set(TerminalFont::Italic),
        b"4" | b"BI" => TerminalFontChange::Set(TerminalFont::BoldItalic),
        b"R" | b"1" | b"" | b"CW" | b"CR" => TerminalFontChange::Set(TerminalFont::Roman),
        b"P" => TerminalFontChange::Restore,
        _ => return None,
    };
    Some((next_cursor, change))
}

/// Reconstruct the `.ft` register immediately before a text node.  The
/// renderer stays re-entrant because this walks immutable ancestry and prior
/// siblings rather than storing document-global device state.
pub(super) fn html_request_font_before(node: NodeRef<'_>) -> HtmlRequestFontState {
    let mut lineage = vec![node];
    let mut cursor = node;
    while let Some(parent) = cursor.parent() {
        lineage.push(parent);
        cursor = parent;
    }
    lineage.reverse();

    let mut state = HtmlRequestFontState::default();
    for current in lineage.into_iter().skip(1) {
        let Some(parent) = current.parent() else {
            continue;
        };
        for sibling in parent.children() {
            if sibling.id() == current.id() {
                break;
            }
            html_apply_font_requests(sibling, &mut state);
        }
    }
    state
}

pub(super) fn html_apply_font_requests(node: NodeRef<'_>, state: &mut HtmlRequestFontState) {
    if node.kind() == NodeKind::Element && node.macro_name() == Some("ft") {
        html_apply_font_request(node.children().find_map(NodeRef::text), state);
        return;
    }
    for child in node.children() {
        html_apply_font_requests(child, state);
    }
}

pub(super) fn html_apply_font_request(selector: Option<&str>, state: &mut HtmlRequestFontState) {
    let next = match selector.unwrap_or_default() {
        "B" => Some(HtmlFont::Bold),
        "I" => Some(HtmlFont::Italic),
        "BI" => Some(HtmlFont::BoldItalic),
        "CR" | "CW" => Some(HtmlFont::LiteralRoman),
        "CB" => Some(HtmlFont::LiteralBold),
        "CI" => Some(HtmlFont::LiteralItalic),
        "R" => Some(HtmlFont::Roman),
        // roff's HTML device accepts `.ft P` and an empty `.ft` but keeps
        // its already-open HTML font wrapper.  Inline `\fP` remains a real
        // swap in `html_font_escape`; this is request-specific behaviour.
        "" | "P" => None,
        _ => None,
    };
    if let Some(next) = next {
        state.previous = state.current;
        state.current = next;
    }
}

/// Render HTML-visible text while retaining roff's inline font changes and
/// the preceding `.ft` device selection.
///
/// The parser deliberately keeps `\\f` spellings in compatible text.  The
/// generic escape normalizer correctly removes their controls, but HTML must
/// first turn the known device selections into the reference's semantic
/// inline elements.  Literal (`C*`) selections use the same `Li` wrapper as
/// structural literal mdoc nodes.
pub(super) fn render_html_visible_text_with_font(
    text: &str,
    limits: &Limits,
    initial_font: HtmlFont,
) -> String {
    let bytes = text.as_bytes();
    let mut output = String::new();
    let mut fragment = String::new();
    let mut cursor = 0_usize;
    let mut font = initial_font;
    let mut previous_font = font;
    while cursor < bytes.len() {
        if let Some((next_cursor, change)) = html_font_escape(bytes, cursor) {
            append_html_font_fragment(&fragment, font, limits, &mut output);
            fragment.clear();
            match change {
                HtmlFontChange::Set(next_font) => {
                    previous_font = font;
                    font = next_font;
                }
                HtmlFontChange::Restore => std::mem::swap(&mut font, &mut previous_font),
            }
            cursor = next_cursor;
            continue;
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor remains within a valid UTF-8 string");
        fragment.push(character);
        cursor += character.len_utf8();
    }
    append_html_font_fragment(&fragment, font, limits, &mut output);
    output
}

pub(super) fn append_html_font_fragment(
    fragment: &str,
    font: HtmlFont,
    limits: &Limits,
    output: &mut String,
) {
    if fragment.is_empty() {
        return;
    }
    let visible = escape_html(&render_visible_text(fragment, RenderFormat::Html, limits));
    let (prefix, suffix) = match font {
        HtmlFont::Roman => ("", ""),
        HtmlFont::Bold => ("<b>", "</b>"),
        HtmlFont::Italic => ("<i>", "</i>"),
        HtmlFont::BoldItalic => ("<b><i>", "</i></b>"),
        HtmlFont::LiteralRoman => ("<span class=\"Li\">", "</span>"),
        HtmlFont::LiteralBold => ("<span class=\"Li\"><b>", "</b></span>"),
        HtmlFont::LiteralItalic => ("<span class=\"Li\"><i>", "</i></span>"),
    };
    output.push_str(prefix);
    output.push_str(&visible);
    output.push_str(suffix);
}

pub(super) fn html_font_escape(bytes: &[u8], cursor: usize) -> Option<(usize, HtmlFontChange)> {
    if bytes.get(cursor..cursor.saturating_add(2)) != Some(b"\\f") {
        return None;
    }
    let selector_start = cursor.saturating_add(2);
    let selector = *bytes.get(selector_start)?;
    let (name, next_cursor) = match selector {
        b'(' => {
            let name =
                bytes.get(selector_start.saturating_add(1)..selector_start.saturating_add(3))?;
            (name, selector_start.saturating_add(3))
        }
        b'[' => {
            let closing = bytes[selector_start.saturating_add(1)..]
                .iter()
                .position(|byte| *byte == b']')?;
            let name_end = selector_start.saturating_add(1).saturating_add(closing);
            (
                bytes.get(selector_start.saturating_add(1)..name_end)?,
                name_end.saturating_add(1),
            )
        }
        _ => (
            &bytes[selector_start..selector_start.saturating_add(1)],
            selector_start.saturating_add(1),
        ),
    };
    let change = match name {
        b"B" | b"3" => HtmlFontChange::Set(HtmlFont::Bold),
        b"I" | b"2" => HtmlFontChange::Set(HtmlFont::Italic),
        b"4" | b"BI" => HtmlFontChange::Set(HtmlFont::BoldItalic),
        b"CW" | b"CR" => HtmlFontChange::Set(HtmlFont::LiteralRoman),
        b"CB" => HtmlFontChange::Set(HtmlFont::LiteralBold),
        b"CI" => HtmlFontChange::Set(HtmlFont::LiteralItalic),
        b"R" | b"1" | b"" => HtmlFontChange::Set(HtmlFont::Roman),
        b"P" => HtmlFontChange::Restore,
        _ => return None,
    };
    Some((next_cursor, change))
}

/// Resolve one terminal text fragment while retaining non-breaking roff
/// spaces until the width pass.  The public AST preserves their source
/// spelling, so this renderer-only conversion cannot alter parser or engine
/// semantics.
pub(super) fn render_terminal_visible_fragment(
    text: &str,
    format: RenderFormat,
    limits: &Limits,
) -> String {
    let bytes = text.as_bytes();
    let mut marked = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if matches!(
            bytes.get(cursor..cursor.saturating_add(2)),
            Some(b"\\~" | b"\\0" | b"\\ ")
        ) {
            marked.push(TERMINAL_NONBREAKING_SPACE_MARKER);
            cursor += 2;
            continue;
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor remains within a valid UTF-8 string");
        marked.push(character);
        cursor += character.len_utf8();
    }
    render_visible_text(&marked, format, limits)
}

/// Resolve terminal-only roff controls that deliberately remain authored in
/// the compatible AST. These controls change device motion or presentation,
/// not document text: `\O` suppresses output, `\o` overstrikes its payload,
/// `\l` draws a terminal rule, and `\h` advances the current field.
pub(super) fn render_terminal_roff_controls(
    text: &str,
    format: RenderFormat,
    limits: &Limits,
) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes.get(cursor) != Some(&b'\\') {
            let character = text[cursor..]
                .chars()
                .next()
                .expect("cursor remains within a valid UTF-8 string");
            output.push(character);
            cursor += character.len_utf8();
            continue;
        }
        match bytes.get(cursor + 1) {
            Some(b'k') => {
                // Position-register interpolation only records the current
                // device column.  Its traditional, two-character, and
                // bracketed names are all presentation-invisible.
                cursor = terminal_named_roff_argument_end(bytes, cursor);
            }
            Some(b'R' | b'A') => {
                // Number-register and numeric-expression escapes are
                // terminal state.  Nested quoted controls are legal inside
                // their payload, so consume the complete quoted form rather
                // than stopping at the first inner quote.
                cursor = terminal_quoted_roff_control_end(text, cursor)
                    .unwrap_or_else(|| cursor.saturating_add(2).min(bytes.len()));
            }
            Some(b's')
                if matches!(
                    bytes.get(cursor + 2),
                    Some(b'+' | b'-' | b'0'..=b'9' | b'(' | b'[' | b'\'')
                ) =>
            {
                // Font-size requests alter the device but never emit their
                // selector.  They accept the same compact, parenthesized,
                // bracketed, and quoted forms as the stable formatter.
                let argument = cursor.saturating_add(2);
                cursor = if matches!(bytes.get(argument), Some(b'+' | b'-')) {
                    let size = argument.saturating_add(1);
                    if bytes.get(size) == Some(&b'\'') {
                        terminal_quoted_roff_argument_end(text, size)
                            .unwrap_or_else(|| size.min(bytes.len()))
                    } else {
                        terminal_roff_argument_end(bytes, size)
                    }
                } else if bytes.get(argument) == Some(&b'\'') {
                    terminal_quoted_roff_control_end(text, cursor)
                        .unwrap_or_else(|| cursor.saturating_add(2).min(bytes.len()))
                } else {
                    terminal_named_roff_argument_end(bytes, cursor)
                };
            }
            Some(b'O') => {
                // The terminal capability escape has no printable payload.
                // Its argument is one byte, a two-byte `\O(..)` name, or a
                // bracketed name; all variants are ignored by mandoc's
                // standard terminal device.
                cursor = match bytes.get(cursor + 2) {
                    Some(b'(') if cursor + 5 <= bytes.len() => cursor + 5,
                    Some(b'[') => bytes[cursor + 3..]
                        .iter()
                        .position(|byte| *byte == b']')
                        .map_or(bytes.len(), |offset| cursor + 4 + offset),
                    Some(_) => cursor.saturating_add(3).min(bytes.len()),
                    None => bytes.len(),
                };
            }
            Some(b'o') => {
                let Some((payload, next)) = terminal_quoted_roff_control(text, cursor) else {
                    output.push('\\');
                    cursor += 1;
                    continue;
                };
                terminal_append_overstrike(payload, &mut output);
                cursor = next;
            }
            Some(b'l') => {
                let Some((payload, next)) = terminal_quoted_roff_control(text, cursor) else {
                    cursor = cursor.saturating_add(3).min(bytes.len());
                    continue;
                };
                let (scale, fill) = terminal_roff_rule_parts(payload);
                if let Some(width) = terminal_signed_roff_en_prefix(scale) {
                    let fill = if fill.is_empty() { "_" } else { fill };
                    let fill_width =
                        display_width(&render_visible_text(fill, format, limits)).max(1);
                    for _ in 0..width.max(0).unsigned_abs() / fill_width {
                        output.push_str(fill);
                    }
                }
                cursor = next;
            }
            Some(b'z') => {
                let (atom, next, zero_width) = terminal_zero_width_roff_atom(text, cursor);
                output.push_str(&atom);
                if zero_width {
                    output.push(TERMINAL_ZERO_WIDTH_BACKSPACE_MARKER);
                }
                cursor = next;
            }
            Some(b'h') => {
                let Some((payload, next)) = terminal_quoted_roff_control(text, cursor) else {
                    cursor = cursor.saturating_add(3).min(bytes.len());
                    continue;
                };
                if let Some(target) = payload.strip_prefix('|') {
                    if let Some(target) = terminal_signed_roff_en_prefix(target) {
                        output.push_str(
                            &" ".repeat(
                                target
                                    .max(0)
                                    .unsigned_abs()
                                    .saturating_sub(display_width(&output)),
                            ),
                        );
                    }
                } else if let Some(delta) = terminal_signed_roff_en_prefix(payload)
                    && delta.is_positive()
                {
                    output.push_str(&" ".repeat(delta.unsigned_abs()));
                }
                cursor = next;
            }
            Some(b'p') => {
                // `\p` takes no argument.  If it is followed by source
                // whitespace it breaks immediately; otherwise it attaches
                // the next word to its left neighbor and breaks before the
                // following word.  Defer the actual newline until the text
                // layout path knows the active field indentation.
                cursor = cursor.saturating_add(2).min(bytes.len());
                if bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                        cursor += 1;
                    }
                    output.push(TERMINAL_PENDING_LINE_BREAK_MARKER);
                } else {
                    while let Some(byte) = bytes.get(cursor) {
                        if byte.is_ascii_whitespace() {
                            break;
                        }
                        let character = text[cursor..]
                            .chars()
                            .next()
                            .expect("cursor remains within a valid UTF-8 string");
                        output.push(character);
                        cursor += character.len_utf8();
                    }
                    output.push(TERMINAL_PENDING_LINE_BREAK_MARKER);
                    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                        cursor += 1;
                    }
                }
            }
            Some(b'!' | b'?') => {
                // Mandoc recognizes these as unsupported formatter controls:
                // retain their diagnostics in the AST, but emit no terminal
                // glyph or authored backslash.
                cursor = cursor.saturating_add(2).min(bytes.len());
            }
            Some(
                code @ (b'+' | b';' | b'<' | b'=' | b'>' | b'@' | b']' | b'1' | b'G' | b'I' | b'i'
                | b'J' | b'j' | b'K' | b'P' | b'Q' | b'q' | b'T' | b'U' | b'W' | b'y'),
            ) => {
                // Invalid one-byte escapes preserve their spelling's payload
                // while the terminal device consumes only the introducer.
                output.push(char::from(*code));
                cursor = cursor.saturating_add(2).min(bytes.len());
            }
            _ => {
                output.push('\\');
                cursor += 1;
            }
        }
    }
    output
}

pub(super) fn terminal_append_overstrike(payload: &str, output: &mut String) {
    for (index, character) in payload.chars().enumerate() {
        if index > 0 {
            output.push('\u{8}');
        }
        output.push(character);
    }
}

/// Return the first byte after a traditional roff name/argument atom.
pub(super) fn terminal_named_roff_argument_end(bytes: &[u8], cursor: usize) -> usize {
    terminal_roff_argument_end(bytes, cursor.saturating_add(2))
}

/// Return the first byte after a roff name/argument beginning at `start`.
pub(super) fn terminal_roff_argument_end(bytes: &[u8], start: usize) -> usize {
    match bytes.get(start) {
        Some(b'(') => start.saturating_add(3).min(bytes.len()),
        Some(b'[') => bytes[start.saturating_add(1)..]
            .iter()
            .position(|byte| *byte == b']')
            .map_or(bytes.len(), |offset| {
                start.saturating_add(2).saturating_add(offset)
            }),
        Some(_) => start.saturating_add(1).min(bytes.len()),
        None => bytes.len(),
    }
}

/// Consume one quoted roff control, including nested quoted escapes.
pub(super) fn terminal_quoted_roff_control_end(text: &str, cursor: usize) -> Option<usize> {
    terminal_quoted_roff_argument_end(text, cursor.saturating_add(2))
}

/// Consume a quoted roff argument whose delimiter sits at `delimiter_index`.
pub(super) fn terminal_quoted_roff_argument_end(
    text: &str,
    delimiter_index: usize,
) -> Option<usize> {
    let bytes = text.as_bytes();
    let delimiter = *bytes.get(delimiter_index)?;
    let mut position = delimiter_index.saturating_add(1);
    while position < bytes.len() {
        if bytes[position] == delimiter {
            return Some(position.saturating_add(1));
        }
        if bytes[position] == b'\\'
            && matches!(
                bytes.get(position.saturating_add(1)),
                Some(b'R' | b'A' | b'w' | b's')
            )
            && bytes.get(position.saturating_add(2)).is_some()
        {
            position = terminal_quoted_roff_control_end(text, position)?;
            continue;
        }
        let character = text[position..].chars().next()?;
        position = position.saturating_add(character.len_utf8());
    }
    None
}

/// Consume the one roff atom owned by `\z`. Unlike ordinary source text the
/// atom returns the terminal cursor to its original column, represented by a
/// trailing backspace after its printable projection. A nested `\z` is left
/// for the next scanner iteration so repeated zero-width controls do not
/// manufacture extra motion.
pub(super) fn terminal_zero_width_roff_atom(text: &str, cursor: usize) -> (String, usize, bool) {
    let bytes = text.as_bytes();
    let start = cursor.saturating_add(2);
    let Some(&first) = bytes.get(start) else {
        return (String::new(), bytes.len(), false);
    };
    if first != b'\\' {
        let character = text[start..]
            .chars()
            .next()
            .expect("cursor remains within a valid UTF-8 string");
        let next = start + character.len_utf8();
        return (
            character.to_string(),
            next,
            !character.is_whitespace() && next < bytes.len(),
        );
    }
    match bytes.get(start + 1) {
        Some(b'z') => (String::new(), start, false),
        Some(b'c' | b'&') => (
            String::new(),
            start.saturating_add(2).min(bytes.len()),
            false,
        ),
        Some(b'f') => {
            let Some((font_end, _)) = terminal_font_escape(bytes, start) else {
                return (
                    String::new(),
                    start.saturating_add(2).min(bytes.len()),
                    false,
                );
            };
            let Some(character) = text[font_end..].chars().next() else {
                return (text[start..font_end].to_owned(), font_end, false);
            };
            let next = font_end + character.len_utf8();
            (
                format!("{}{}", &text[start..font_end], character),
                next,
                !character.is_whitespace() && next < bytes.len(),
            )
        }
        Some(b'(') if start + 4 <= bytes.len() => {
            let next = start + 4;
            (text[start..next].to_owned(), next, next < bytes.len())
        }
        Some(b'[') => {
            let next = bytes[start + 2..]
                .iter()
                .position(|byte| *byte == b']')
                .map_or(bytes.len(), |offset| start + 3 + offset);
            (text[start..next].to_owned(), next, next < bytes.len())
        }
        Some(b'o') => {
            let Some((payload, next)) = terminal_quoted_roff_control(text, start) else {
                return (
                    String::new(),
                    start.saturating_add(2).min(bytes.len()),
                    false,
                );
            };
            let mut overstrike = String::new();
            terminal_append_overstrike(payload, &mut overstrike);
            (overstrike, next, !payload.is_empty() && next < bytes.len())
        }
        Some(_) | None => (
            String::new(),
            start.saturating_add(2).min(bytes.len()),
            false,
        ),
    }
}

/// Decode the quoted argument common to roff's `\o`, `\l`, and `\h`
/// controls. An unterminated argument remains authored, so callers can leave
/// normal escape normalization and diagnostics unchanged.
pub(super) fn terminal_quoted_roff_control(text: &str, cursor: usize) -> Option<(&str, usize)> {
    let bytes = text.as_bytes();
    let delimiter = *bytes.get(cursor + 2)?;
    let payload_start = cursor + 3;
    let end = bytes[payload_start..]
        .iter()
        .position(|byte| *byte == delimiter)?
        + payload_start;
    Some((&text[payload_start..end], end + 1))
}

/// Split `\l`'s scale prefix from its optional fill character.  Roff's
/// default scale unit is `n`; only known explicit unit letters consume one
/// byte before the fill spelling begins.
pub(super) fn terminal_roff_rule_parts(payload: &str) -> (&str, &str) {
    let bytes = payload.as_bytes();
    let mut end = 0_usize;
    while matches!(bytes.get(end), Some(b'+' | b'-' | b'.' | b'0'..=b'9')) {
        end += 1;
    }
    if matches!(
        bytes.get(end),
        Some(b'c' | b'i' | b'f' | b'M' | b'm' | b'n' | b'P' | b'v' | b'p' | b'u')
    ) {
        end += 1;
    }
    payload.split_at(end)
}

/// Resolve the two quoted Unicode forms consumed by mandoc's terminal
/// device. They remain renderer-only because the public AST deliberately
/// retains their authored spelling for diagnostics and lowering fidelity.
pub(super) fn render_unicode_character_escapes(text: &str, format: RenderFormat) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes.get(cursor..cursor.saturating_add(2)) == Some(b"\\[")
            && let Some(close) = bytes[cursor + 2..]
                .iter()
                .position(|byte| *byte == b']')
                .map(|offset| cursor + 2 + offset)
            && let Some(name) = text.get(cursor + 2..close)
            && let Some(value) = name.strip_prefix('u')
            && let Some(character) = canonical_unicode_scalar(value)
        {
            if character <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&character) {
                push_renderer_device_character(&mut output, character, format);
            } else if format == RenderFormat::Ascii
                && !character.is_ascii()
                && !ascii_terminal_named_scalar_is_known(character)
            {
                // Numeric Unicode names use mandoc's explicit unknown-glyph
                // notation, which differs from an arbitrary UTF-8 scalar in
                // authored terminal prose.
                output.push_str("<?>");
            } else {
                push_renderer_resolved_character(&mut output, character);
            }
            cursor = close + 1;
            continue;
        }
        let escaped =
            bytes.get(cursor) == Some(&b'\\') && matches!(bytes.get(cursor + 1), Some(b'U' | b'C'));
        let Some(&quote) = bytes
            .get(cursor + 2)
            .filter(|quote| matches!(quote, b'\'' | b'"'))
        else {
            let character = text[cursor..]
                .chars()
                .next()
                .expect("cursor remains within a valid UTF-8 string");
            output.push(character);
            cursor += character.len_utf8();
            continue;
        };
        if !escaped {
            let character = text[cursor..]
                .chars()
                .next()
                .expect("cursor remains within a valid UTF-8 string");
            output.push(character);
            cursor += character.len_utf8();
            continue;
        }
        let value_start = cursor + 3;
        let Some(close) = bytes[value_start..]
            .iter()
            .position(|byte| *byte == quote)
            .map(|offset| value_start + offset)
        else {
            output.push('\\');
            cursor += 1;
            continue;
        };
        let value = &text[value_start..close];
        if bytes[cursor + 1] == b'U' {
            // The pinned 1.14.6 terminal device has no `\U` scalar escape:
            // it drops the escape introducer and leaves the authored `U…`
            // spelling visible. Preserve that compatibility rather than
            // projecting a newer roff extension into the reference output.
            output.push_str(&text[cursor + 1..=close]);
            cursor = close + 1;
            continue;
        }
        let character = named_unicode_scalar(value);
        if let Some(character) = character {
            push_renderer_device_character(&mut output, character, format);
            cursor = close + 1;
        } else {
            output.push_str(&text[cursor..=close]);
            cursor = close + 1;
        }
    }
    output
}

pub(super) fn unicode_scalar(value: &str) -> Option<char> {
    (4..=6)
        .contains(&value.len())
        .then(|| u32::from_str_radix(value, 16).ok())
        .flatten()
        .and_then(char::from_u32)
}

pub(super) fn canonical_unicode_scalar(value: &str) -> Option<char> {
    let character = unicode_scalar(value)?;
    let scalar = u32::from(character);
    let canonical_length = if scalar <= 0xffff {
        4
    } else {
        format!("{scalar:X}").len()
    };
    (value.len() == canonical_length).then_some(character)
}

pub(super) fn named_unicode_scalar(value: &str) -> Option<char> {
    value
        .strip_prefix('u')
        .and_then(unicode_scalar)
        .or_else(|| match crate::special_character(value) {
            Some(crate::SpecialCharacter::Visible(character)) => Some(character),
            Some(crate::SpecialCharacter::ZeroWidth) | None => None,
        })
}

/// Convert valid single-byte `\\N'number'` escapes before generic escape
/// normalization. Invalid and out-of-device-range numbers are suppressed, as
/// the stable terminal device does; malformed spellings stay available to the
/// generic normalizer for conservative recovery.
pub(super) fn render_numeric_character_escapes(text: &str, format: RenderFormat) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes.get(cursor..cursor.saturating_add(2)) == Some(b"\\N") {
            let quote = bytes.get(cursor + 2).copied();
            if matches!(quote, Some(b'\'' | b'\"')) {
                let number_start = cursor + 3;
                let digits = bytes[number_start..]
                    .iter()
                    .take_while(|byte| byte.is_ascii_digit())
                    .count();
                let number_end = number_start + digits;
                if bytes.get(number_end).is_some() {
                    if let Ok(number) = std::str::from_utf8(&bytes[number_start..number_end])
                        && let Ok(number) = number.parse::<u8>()
                        && let Some(character) = char::from_u32(u32::from(number))
                    {
                        push_renderer_device_character(&mut output, character, format);
                    }
                    // The legacy device accepts only an immediate matching
                    // quote. On a malformed spelling it still consumes the
                    // first non-numeric byte before returning the remaining
                    // source to ordinary text flow.
                    cursor = number_end + 1;
                    continue;
                }
            } else if quote.is_some_and(|byte| !byte.is_ascii_digit()) {
                let number_start = cursor + 3;
                let digits = bytes[number_start..]
                    .iter()
                    .take_while(|byte| byte.is_ascii_digit())
                    .count();
                let number_end = number_start + digits;
                if digits > 0 && bytes.get(number_end).is_some() {
                    if let Ok(number) = std::str::from_utf8(&bytes[number_start..number_end])
                        && let Ok(number) = number.parse::<u8>()
                        && let Some(character) = char::from_u32(u32::from(number))
                    {
                        push_renderer_device_character(&mut output, character, format);
                    }
                    cursor = number_end + 1;
                    continue;
                }
                // With no valid quoted-like number, consume the introducer
                // and its first argument byte as the stable recovery does.
                cursor += 3;
                continue;
            } else {
                // The escape introducer is consumed even without its required
                // quote; its first following byte becomes the malformed
                // delimiter and the remaining bytes stay visible.
                cursor = cursor.saturating_add(3).min(bytes.len());
                continue;
            }
        }
        if bytes.get(cursor..cursor.saturating_add(2)) == Some(b"\\-") {
            output.push('-');
            cursor += 2;
            continue;
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor always points at a UTF-8 character boundary");
        output.push(character);
        cursor += character.len_utf8();
    }
    output
}

/// Keep renderer-produced backslashes inert until the authored escape stream
/// has been normalized exactly once.
pub(super) fn push_renderer_resolved_character(output: &mut String, character: char) {
    output.push(if character == '\\' {
        RENDER_LITERAL_BACKSLASH_MARKER
    } else {
        character
    });
}

/// The formatter does not send C0/C1 controls to its output device.  A named
/// or numeric roff escape representing one is rendered as the device's
/// printable control notation in ASCII, and as U+FFFD everywhere else.
/// Keeping this at escape resolution time also prevents literal newline and
/// tab scalars from changing the renderer's structural layout.
pub(super) fn push_renderer_device_character(
    output: &mut String,
    character: char,
    format: RenderFormat,
) {
    if character == '\t' {
        // Horizontal tabs retain their device tab-stop semantics in every
        // output format; unlike other controls, they are layout, not a
        // replacement glyph.
        output.push('\t');
    } else if character <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&character) {
        match format {
            RenderFormat::Ascii => output.push_str(ascii_terminal_control_name(character)),
            RenderFormat::Utf8 | RenderFormat::Html => output.push('\u{fffd}'),
        }
    } else {
        push_renderer_resolved_character(output, character);
    }
}

pub(super) fn ascii_terminal_control_name(character: char) -> &'static str {
    match character {
        '\0' => "<NUL>",
        '\u{1}' => "<SOH>",
        '\u{2}' => "<STX>",
        '\u{3}' => "<ETX>",
        '\u{4}' => "<EOT>",
        '\u{5}' => "<ENQ>",
        '\u{6}' => "<ACK>",
        '\u{7}' => "<BEL>",
        '\u{8}' => "<BS>",
        '\t' => "\t",
        '\n' => "<LF>",
        '\u{b}' => "<VT>",
        '\u{c}' => "<FF>",
        '\r' => "<CR>",
        '\u{e}' => "<SO>",
        '\u{f}' => "<SI>",
        '\u{10}' => "<DLE>",
        '\u{11}' => "<DC1>",
        '\u{12}' => "<DC2>",
        '\u{13}' => "<DC3>",
        '\u{14}' => "<DC4>",
        '\u{15}' => "<NAK>",
        '\u{16}' => "<SYN>",
        '\u{17}' => "<ETB>",
        '\u{18}' => "<CAN>",
        '\u{19}' => "<EM>",
        '\u{1a}' => "<SUB>",
        '\u{1b}' => "<ESC>",
        '\u{1c}' => "<FS>",
        '\u{1d}' => "<GS>",
        '\u{1e}' => "<RS>",
        '\u{1f}' => "<US>",
        '\u{7f}' => "<DEL>",
        '\u{80}' => "<80>",
        '\u{81}' => "<81>",
        '\u{82}' => "<82>",
        '\u{83}' => "<83>",
        '\u{84}' => "<84>",
        '\u{85}' => "<85>",
        '\u{86}' => "<86>",
        '\u{87}' => "<87>",
        '\u{88}' => "<88>",
        '\u{89}' => "<89>",
        '\u{8a}' => "<8A>",
        '\u{8b}' => "<8B>",
        '\u{8c}' => "<8C>",
        '\u{8d}' => "<8D>",
        '\u{8e}' => "<8E>",
        '\u{8f}' => "<8F>",
        '\u{90}' => "<90>",
        '\u{91}' => "<91>",
        '\u{92}' => "<92>",
        '\u{93}' => "<93>",
        '\u{94}' => "<94>",
        '\u{95}' => "<95>",
        '\u{96}' => "<96>",
        '\u{97}' => "<97>",
        '\u{98}' => "<98>",
        '\u{99}' => "<99>",
        '\u{9a}' => "<9A>",
        '\u{9b}' => "<9B>",
        '\u{9c}' => "<9C>",
        '\u{9d}' => "<9D>",
        '\u{9e}' => "<9E>",
        '\u{9f}' => "<9F>",
        _ => "<?>",
    }
}

pub(super) fn append(output: &mut String, value: &str, maximum: usize) -> Result<(), RenderError> {
    if output.len().saturating_add(value.len()) > maximum {
        return Err(RenderError {
            kind: RenderErrorKind::OutputLimit,
            message: format!("rendered output exceeds {maximum} bytes").into(),
        });
    }
    output.push_str(value);
    Ok(())
}

pub(super) fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            character if character.is_ascii() => escaped.push(character),
            character => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "&#x{:04X};", u32::from(character));
            }
        }
    }
    escaped
}
