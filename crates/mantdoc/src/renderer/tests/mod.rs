use crate::ast::{EquationTerminal, EquationTerminalToken, NodeKind};
use crate::{Limits, Parser, Source, SourceName};

use super::{
    DEFAULT_RENDER_OUTPUT_BYTES, RenderErrorKind, RenderFormat, Renderer,
    TERMINAL_HANGING_INDENT_MARKER, TERMINAL_NONBREAKING_SPACE_MARKER, TerminalFont, display_width,
    escape_html, expand_filled_terminal_tabs, expand_literal_terminal_tabs, render_html_equation,
    render_terminal_bold, render_terminal_equation, render_terminal_equation_text,
    render_terminal_visible_text, render_terminal_visible_text_with_font, render_visible_text,
    terminal_character_width, terminal_default_volume, terminal_mdoc_plain_text_sentence,
    terminal_table_text_block_lines, wrap_html_plain_paragraph, wrap_terminal_output,
};

mod fields;
mod html;
mod mdoc_layout;
mod misc;
mod terminal_core;
mod terminal_recovery;
