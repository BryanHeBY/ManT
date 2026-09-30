//! Readable plain-text projection of already decoded roff events.
use super::{RoffInlineEvent, ZeroAdvanceMachine, decode, is_formatter_word_blank};

struct PlainTextProjection {
    output: String,
    zero_advance: ZeroAdvanceMachine<String>,
    pending_word_end_break: bool,
    suppress_break_whitespace: bool,
}

impl PlainTextProjection {
    const fn new() -> Self {
        Self {
            output: String::new(),
            zero_advance: ZeroAdvanceMachine::new(),
            pending_word_end_break: false,
            suppress_break_whitespace: false,
        }
    }

    fn append_text(&mut self, value: &str) {
        for character in value.chars() {
            if self.pending_word_end_break && is_formatter_word_blank(character) {
                if let Some(glyph) = self.zero_advance.resolve_word_boundary() {
                    self.output.extend(glyph);
                    self.suppress_break_whitespace = true;
                    continue;
                }
                self.output.push('\n');
                self.pending_word_end_break = false;
                self.suppress_break_whitespace = true;
                continue;
            }
            if self.suppress_break_whitespace && is_formatter_word_blank(character) {
                continue;
            }
            self.suppress_break_whitespace = false;
            if matches!(character, '\n' | '\r') {
                if let Some(glyph) = self.zero_advance.take_pending() {
                    self.output.extend(glyph);
                }
                self.output.push(character);
            } else if self.zero_advance.is_armed()
                && (is_formatter_word_blank(character) || character == '\t')
            {
                self.output.push(character);
            } else if self.zero_advance.is_armed() {
                let _ = self.zero_advance.project_glyph(character.to_string());
            } else if self.zero_advance.has_pending() {
                if is_formatter_word_blank(character) || character == '\t' {
                    if let Some(glyph) = self.zero_advance.take_pending() {
                        self.output.extend(glyph);
                    }
                    continue;
                }
                let Some((character, _)) = self.zero_advance.project_glyph(character.to_string())
                else {
                    continue;
                };
                self.output.extend(self.zero_advance.take_recoveries());
                self.output.push_str(&character);
            } else if let Some((character, _)) =
                self.zero_advance.project_glyph(character.to_string())
            {
                self.output.extend(self.zero_advance.take_recoveries());
                self.output.push_str(&character);
            }
        }
    }

    fn append_glyph(&mut self, value: String) {
        self.suppress_break_whitespace = false;
        if let Some((value, _)) = self.zero_advance.project_glyph(value) {
            self.output.extend(self.zero_advance.take_recoveries());
            self.output.push_str(&value);
        }
    }

    fn append_event(&mut self, event: RoffInlineEvent) {
        match event {
            RoffInlineEvent::Text(value) => self.append_text(&value),
            RoffInlineEvent::Glyph(value)
            | RoffInlineEvent::Overstrike {
                terminal: Some(value),
                ..
            } => self.append_glyph(value),
            RoffInlineEvent::FallbackGlyph(value) => {
                if let Some((value, _)) = self.zero_advance.project_fallback(value) {
                    self.suppress_break_whitespace = false;
                    self.output.extend(self.zero_advance.take_recoveries());
                    self.output.push_str(&value);
                }
            }
            RoffInlineEvent::DeviceName => self.append_text("utf8"),
            RoffInlineEvent::ZeroAdvance => self.zero_advance.arm(),
            RoffInlineEvent::EmptyDestination => {
                self.suppress_break_whitespace = false;
                self.output.push_str("<>");
            }
            RoffInlineEvent::LineBreak => self.pending_word_end_break = true,
            RoffInlineEvent::NoSpace => {
                self.zero_advance.cancel_armed();
            }
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::Link(_)
            | RoffInlineEvent::Overstrike { terminal: None, .. }
            | RoffInlineEvent::Presentation { .. } => {}
        }
    }

    fn finish(mut self) -> String {
        if let Some(glyph) = self.zero_advance.take_pending() {
            self.output.extend(glyph);
        }
        self.output
    }
}

/// Return only the visible characters of a roff-encoded identifier or label.
pub(in crate::mandoc) fn visible_text(source: &str) -> String {
    let mut projection = PlainTextProjection::new();
    for event in decode(source) {
        projection.append_event(event);
    }
    projection.finish()
}
