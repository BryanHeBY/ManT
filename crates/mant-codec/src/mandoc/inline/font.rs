//! Stateful roff font decoding shared by prose, macro operands and table cells.
use super::super::reference::trailing_sphinx_manual_reference;
use super::{
    Font, FontState, Inline, InlineBuilder, Node, RoffInlineEvent, append_inline_nodes, decode,
};

/// Bounded semantic projection of CVS mandoc's `TERMP_BACKAFTER`/
/// `TERMP_BACKBEFORE` state for `\\z`.
///
/// The terminal formatter carries that state across `term_word()` calls.  A
/// pending glyph therefore belongs to the surrounding inline stream rather
/// than to the one text node that happened to contain the escape.
pub(in crate::mandoc) struct ZeroAdvanceState {
    armed: bool,
    pending: Option<Inline>,
    fragment_started_pending: bool,
    resolved_preexisting: bool,
}

impl ZeroAdvanceState {
    pub(in crate::mandoc) const fn new() -> Self {
        Self {
            armed: false,
            pending: None,
            fragment_started_pending: false,
            resolved_preexisting: false,
        }
    }

    fn begin_fragment(&mut self) {
        self.fragment_started_pending = self.pending.is_some();
        self.resolved_preexisting = false;
    }

    fn arm(&mut self) {
        if self.pending.take().is_some() && self.fragment_started_pending {
            self.resolved_preexisting = true;
        }
        self.armed = true;
    }

    fn append_text(
        &mut self,
        value: &str,
        output: &mut Vec<Inline>,
        buffer: &mut String,
        font: Font,
        link: Option<&str>,
    ) {
        for character in value.chars() {
            if matches!(character, '\n' | '\r') {
                self.flush(output, buffer, font, link);
                buffer.push(character);
                continue;
            }
            if self.armed {
                self.armed = false;
                self.pending = Some(styled_link(character.to_string(), font, link));
                continue;
            }
            if self.pending.is_some() {
                if character.is_whitespace() {
                    // The first intervening formatter blank consumes the
                    // backtracking position but does not become document
                    // content.  This is why `TOKEN\\zX END` renders as
                    // `TOKENXEND` in both pinned reference formatters.
                    self.flush(output, buffer, font, link);
                    continue;
                }
                self.pending = None;
                if self.fragment_started_pending {
                    self.resolved_preexisting = true;
                }
            }
            buffer.push(character);
        }
    }

    fn append_glyph(&mut self, value: String, buffer: &mut String, font: Font, link: Option<&str>) {
        if self.armed {
            self.armed = false;
            self.pending = Some(styled_link(value, font, link));
            return;
        }
        if self.pending.take().is_some() && self.fragment_started_pending {
            self.resolved_preexisting = true;
        }
        buffer.push_str(&value);
        // A fallback spelling is one roff glyph even though it takes several
        // Unicode scalar values to present. Do not let a following source
        // character overstrike its interior.
    }

    fn append_fallback_glyph(&mut self, value: &str, buffer: &mut String) {
        if self.armed {
            self.armed = false;
        } else {
            if self.pending.take().is_some() && self.fragment_started_pending {
                self.resolved_preexisting = true;
            }
            buffer.push_str(value);
        }
    }

    fn flush(
        &mut self,
        output: &mut Vec<Inline>,
        buffer: &mut String,
        font: Font,
        link: Option<&str>,
    ) {
        flush_segment(output, buffer, font, link);
        if let Some(glyph) = self.pending.take() {
            if self.fragment_started_pending {
                self.resolved_preexisting = true;
            }
            output.push(glyph);
        }
    }

    pub(in crate::mandoc) fn finish_into(&mut self, output: &mut Vec<Inline>) {
        if let Some(glyph) = self.pending.take() {
            output.push(glyph);
        }
        self.armed = false;
    }

    fn take_preceding_join(&mut self) -> bool {
        let joined = self.fragment_started_pending && self.resolved_preexisting;
        self.fragment_started_pending = false;
        self.resolved_preexisting = false;
        joined
    }
}

impl Default for ZeroAdvanceState {
    fn default() -> Self {
        Self::new()
    }
}

pub(in crate::mandoc) fn parse_roff_text(source: &str) -> Vec<Inline> {
    parse_roff_text_with_font(source, Font::Regular, true)
}

/// Decode one roff text run using the font selected by its enclosing macro.
/// Explicit `\\f` escapes change `font` while the run is scanned, so a reset
/// to regular text remains visible even inside an alternating `.BI` argument.
pub(super) fn parse_roff_text_with_font(
    source: &str,
    initial_font: Font,
    recognize_generated_references: bool,
) -> Vec<Inline> {
    let mut state = FontState::new();
    state.select(initial_font);
    parse_roff_text_with_state(source, &mut state, recognize_generated_references)
}

pub(super) fn lower_man_font_scope(
    node: &Node,
    default_name: Option<&str>,
    spacing: bool,
    state: &mut FontState,
    zero_advance: &mut ZeroAdvanceState,
) -> (Vec<Inline>, bool) {
    state.select(Font::Regular);
    if let Some((first, second)) = super::alternating_font_pair(node.macro_name.as_deref()) {
        let mut output = builder_with_zero_advance(spacing, *state, zero_advance);
        for (index, child) in node.children.iter().enumerate() {
            state.select(if index % 2 == 0 { first } else { second });
            output.font = *state;
            // Alternating man macro operands are emitted as one formatter
            // word.  This is also the scope in which CVS preserves the
            // TERMP_BACK* state from one argument to the next.
            if index > 0 {
                output.tighten_next_boundary();
            }
            super::append_inline_node_with_next(
                &mut output,
                child,
                node.children.get(index + 1),
                default_name,
            );
            *state = output.font;
        }
        state.select(Font::Regular);
        return finish_with_zero_advance(output, zero_advance);
    }
    if node.macro_name.as_deref() == Some("OP") {
        let mut output = builder_with_zero_advance(spacing, *state, zero_advance);
        for (index, child) in node.children.iter().enumerate() {
            state.select(if index == 0 {
                Font::Strong
            } else {
                Font::Emphasis
            });
            output.font = *state;
            super::append_inline_node_with_next(
                &mut output,
                child,
                node.children.get(index + 1),
                default_name,
            );
            *state = output.font;
        }
        // OP resets for its closing bracket, then the man macro scope resets
        // again. Consequently a following fP selects regular, not its operand.
        state.select(Font::Regular);
        state.select(Font::Regular);
        let (output, joined) = finish_with_zero_advance(output, zero_advance);
        return (super::surround("[", output, "]"), joined);
    }
    match node.macro_name.as_deref() {
        Some("B" | "SB") => state.select(Font::Strong),
        Some("I") => state.select(Font::Emphasis),
        _ => {}
    }
    let (result, joined) = lower_inline_nodes_with_font_state_and_zero_advance(
        &node.children,
        default_name,
        spacing,
        state,
        zero_advance,
    );
    state.select(Font::Regular);
    (result, joined)
}

fn builder_with_zero_advance(
    spacing: bool,
    state: FontState,
    zero_advance: &mut ZeroAdvanceState,
) -> InlineBuilder {
    let mut builder = InlineBuilder::with_spacing(spacing);
    builder.font = state;
    builder.zero_advance = std::mem::take(zero_advance);
    builder
}

fn finish_with_zero_advance(
    builder: InlineBuilder,
    zero_advance: &mut ZeroAdvanceState,
) -> (Vec<Inline>, bool) {
    let (output, next_zero_advance, joined) = builder.finish_preserving_zero_advance();
    *zero_advance = next_zero_advance;
    (output, joined)
}

pub(in crate::mandoc) fn lower_inline_nodes_with_font_state(
    nodes: &[Node],
    default_name: Option<&str>,
    spacing: bool,
    state: &mut FontState,
) -> Vec<Inline> {
    let mut zero_advance = ZeroAdvanceState::new();
    let (mut output, _) = lower_inline_nodes_with_font_state_and_zero_advance(
        nodes,
        default_name,
        spacing,
        state,
        &mut zero_advance,
    );
    zero_advance.finish_into(&mut output);
    output
}

fn lower_inline_nodes_with_font_state_and_zero_advance(
    nodes: &[Node],
    default_name: Option<&str>,
    spacing: bool,
    state: &mut FontState,
    zero_advance: &mut ZeroAdvanceState,
) -> (Vec<Inline>, bool) {
    let mut builder = builder_with_zero_advance(spacing, *state, zero_advance);
    append_inline_nodes(&mut builder, nodes, default_name);
    *state = builder.font;
    finish_with_zero_advance(builder, zero_advance)
}

pub(in crate::mandoc) fn parse_roff_text_with_state(
    source: &str,
    state: &mut FontState,
    recognize_generated_references: bool,
) -> Vec<Inline> {
    let mut zero_advance = ZeroAdvanceState::default();
    let (mut output, _) = parse_roff_text_with_zero_advance(
        source,
        state,
        recognize_generated_references,
        &mut zero_advance,
    );
    zero_advance.finish_into(&mut output);
    output
}

/// Decode a text node while retaining a pending `\\z` glyph in its caller's
/// inline stream.  The boolean reports that a glyph from an earlier text node
/// resolved here, so the caller must not invent a word boundary before it.
pub(in crate::mandoc) fn parse_roff_text_with_zero_advance(
    source: &str,
    state: &mut FontState,
    recognize_generated_references: bool,
    zero_advance: &mut ZeroAdvanceState,
) -> (Vec<Inline>, bool) {
    let mut output = Vec::new();
    let mut buffer = String::new();
    let mut font = state.current;
    let mut link: Option<String> = None;
    zero_advance.begin_fragment();

    for event in decode(source) {
        match event {
            RoffInlineEvent::Text(value) => {
                // The decoder has already classified controls. A backslash
                // produced by \e or \[rs] is literal author content.
                zero_advance.append_text(&value, &mut output, &mut buffer, font, link.as_deref());
            }
            RoffInlineEvent::Glyph(value) => {
                zero_advance.append_glyph(value, &mut buffer, font, link.as_deref());
            }
            RoffInlineEvent::FallbackGlyph(value) => {
                zero_advance.append_fallback_glyph(&value, &mut buffer);
            }
            RoffInlineEvent::ZeroAdvance => zero_advance.arm(),
            RoffInlineEvent::Font(next_font) => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                state.select(next_font);
                font = state.current;
            }
            RoffInlineEvent::PreviousFont => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                state.restore();
                font = state.current;
            }
            RoffInlineEvent::Link(target) => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                link = target;
            }
            RoffInlineEvent::EmptyDestination => {
                if !recognize_generated_references
                    || !promote_sphinx_manual_reference(
                        &mut output,
                        &mut buffer,
                        font,
                        link.as_deref(),
                    )
                {
                    buffer.push_str("<>");
                }
            }
            RoffInlineEvent::LineBreak => {
                zero_advance.flush(&mut output, &mut buffer, font, link.as_deref());
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                if !matches!(output.last(), Some(Inline::LineBreak)) {
                    output.push(Inline::LineBreak);
                }
            }
            RoffInlineEvent::Presentation { .. } | RoffInlineEvent::ZeroWidthGlyph => {}
        }
    }
    flush_segment(&mut output, &mut buffer, font, link.as_deref());
    (output, zero_advance.take_preceding_join())
}

fn promote_sphinx_manual_reference(
    output: &mut Vec<Inline>,
    buffer: &mut String,
    font: Font,
    external_link: Option<&str>,
) -> bool {
    if external_link.is_some() || matches!(font, Font::Code | Font::CodeStrong | Font::CodeEmphasis)
    {
        return false;
    }
    let Some(reference) = trailing_sphinx_manual_reference(buffer) else {
        return false;
    };
    let prefix = reference.prefix.to_owned();
    let display = reference.display.to_owned();
    let name = reference.name.to_owned();
    let manual_section = reference.manual_section.to_owned();
    *buffer = prefix;
    flush_segment(output, buffer, font, None);
    output.push(Inline::Link {
        target: mant_ir::LinkTarget::Manual {
            name,
            manual_section: Some(manual_section),
        },
        title: None,
        children: vec![styled_segment(display, font)],
    });
    true
}

fn flush_segment(output: &mut Vec<Inline>, buffer: &mut String, font: Font, link: Option<&str>) {
    if buffer.is_empty() {
        return;
    }
    let value = std::mem::take(buffer);
    output.push(styled_link(value, font, link));
}

fn styled_link(value: String, font: Font, link: Option<&str>) -> Inline {
    let styled = styled_segment(value, font);
    if let Some(target) = link {
        Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: target.to_owned(),
            },
            title: None,
            children: vec![styled],
        }
    } else {
        styled
    }
}

pub(super) fn styled_segment(value: String, font: Font) -> Inline {
    match font {
        Font::Regular => Inline::Text { value },
        Font::Strong => Inline::Strong {
            children: vec![Inline::Text { value }],
        },
        Font::Emphasis => Inline::Emphasis {
            children: vec![Inline::Text { value }],
        },
        Font::StrongEmphasis => Inline::Strong {
            children: vec![Inline::Emphasis {
                children: vec![Inline::Text { value }],
            }],
        },
        Font::Code => Inline::Code { value },
        Font::CodeStrong => Inline::Strong {
            children: vec![Inline::Code { value }],
        },
        Font::CodeEmphasis => Inline::Emphasis {
            children: vec![Inline::Code { value }],
        },
    }
}

/// Keep a generated prefix and equally styled operands in a single visible
/// run, without wrapping font overrides in an additional, additive style.
pub(super) fn coalesce_font_runs(nodes: Vec<Inline>) -> Vec<Inline> {
    let mut output: Vec<Inline> = Vec::new();
    for node in nodes {
        match (output.last_mut(), node) {
            (Some(Inline::Strong { children: previous }), Inline::Strong { mut children })
            | (Some(Inline::Emphasis { children: previous }), Inline::Emphasis { mut children }) => {
                previous.append(&mut children);
            }
            (Some(Inline::Text { value: previous }), Inline::Text { value }) => {
                previous.push_str(&value);
            }
            (_, node) => output.push(node),
        }
    }
    output
}
