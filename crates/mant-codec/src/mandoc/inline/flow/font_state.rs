//! Native font slots, previous-font registers and scoped presentation.
//!
//! CVS `term_fontrepl` changes the active slot; `term_fontpopq` restores
//! depth while leaving the independent previous-font register intact.

use super::Font;

/// Roff remembers the previous selection independently of the current font.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::mandoc) struct FontState {
    /// Terminal execution registers used by `\\fP` and `.ft P`.
    pub(in crate::mandoc::inline) current: Font,
    pub(in crate::mandoc::inline) previous: Font,
    /// The HTML/IR spelling of a font can retain constant width even when
    /// the terminal renderer executes CR as roman, CB as bold, and CI as
    /// italic. Keep that presentation out of the execution registers.
    display_current: Font,
    display_previous: Font,
    /// `tbl_html.c` has an independent display selection for CR cells, but
    /// `tbl_term.c` does not push a terminal font for them. Keep the temporary
    /// HTML pair separate from both persistent register pairs.
    table_presentation: Option<TablePresentationFont>,
    heading_bold_italic: bool,
    /// CVS term.c keeps mdoc fonts in a stack.  An ordinary `.ft` changes
    /// the current slot without pushing it, so BODY unwinding must restore
    /// only scopes pushed after that BODY was entered.
    // Saved lower slots; `current` is the mutable active slot. A `.ft`
    // replaces that slot, and term_fontpopq() only changes the active depth.
    font_stack: Vec<(Font, Font)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TablePresentationFont {
    current: Font,
    previous: Font,
}

#[derive(Clone, Copy)]
pub(in crate::mandoc) struct FontScope {
    depth: usize,
    table_presentation_current: Option<Font>,
}

impl FontState {
    pub(in crate::mandoc) fn new() -> Self {
        Self {
            current: Font::Regular,
            previous: Font::Regular,
            display_current: Font::Regular,
            display_previous: Font::Regular,
            table_presentation: None,
            heading_bold_italic: false,
            font_stack: Vec::new(),
        }
    }

    pub(in crate::mandoc) const fn display_current(&self) -> Font {
        match self.table_presentation {
            Some(presentation) => presentation.current,
            None => self.display_current,
        }
    }

    pub(in crate::mandoc) fn select(&mut self, font: Font) {
        let display = if self.heading_bold_italic && font == Font::Emphasis {
            Font::StrongEmphasis
        } else {
            font
        };
        self.previous = self.current;
        let terminal = match font {
            Font::Code => Font::Regular,
            Font::CodeStrong => Font::Strong,
            Font::CodeEmphasis => Font::Emphasis,
            font => font,
        };
        self.current = if self.heading_bold_italic && terminal == Font::Emphasis {
            Font::StrongEmphasis
        } else {
            terminal
        };
        self.display_previous = self.display_current;
        self.display_current = display;
        if let Some(presentation) = &mut self.table_presentation {
            presentation.previous = presentation.current;
            presentation.current = display;
        }
    }

    /// Every printable man(7) node enters and leaves through the generic
    /// `print_man_node()` font replacement, even when its macro has no post
    /// handler. Replacing Roman also updates the previous-font register.
    pub(in crate::mandoc) fn man_text_boundary(&mut self) {
        self.select(Font::Regular);
    }

    pub(in crate::mandoc::inline) fn restore(&mut self) {
        std::mem::swap(&mut self.current, &mut self.previous);
        std::mem::swap(&mut self.display_current, &mut self.display_previous);
        if let Some(presentation) = &mut self.table_presentation {
            std::mem::swap(&mut presentation.current, &mut presentation.previous);
        }
    }

    /// mdoc font scopes push a slot. The previous-font register is separate.
    pub(in crate::mandoc) fn push_scope(&mut self, font: Font) -> FontScope {
        let saved = self.checkpoint();
        self.font_stack.push((self.current, self.display_current));
        self.select(font);
        saved
    }

    pub(in crate::mandoc) fn checkpoint(&self) -> FontScope {
        FontScope {
            depth: self.font_stack.len(),
            table_presentation_current: self.table_presentation.map(|font| font.current),
        }
    }

    pub(in crate::mandoc) fn pop_scope(&mut self, saved: FontScope) {
        // term.c::term_fontpopq() is a no-op if a crossed BODY has already
        // unwound this font depth.  A later return must not revive it.
        if self.font_stack.len() <= saved.depth {
            return;
        }
        (self.current, self.display_current) = self.font_stack[saved.depth];
        self.font_stack.truncate(saved.depth);
        if let (Some(presentation), Some(current)) = (
            &mut self.table_presentation,
            saved.table_presentation_current,
        ) {
            presentation.current = current;
        }
    }

    /// CVS `tbl_term.c::tbl_word()` leaves terminal registers untouched for
    /// a CR layout cell, while `tbl_html.c::print_tbl()` still displays it in
    /// constant width. The subsequent in-cell `\\f` commands update both
    /// projections independently.
    pub(in crate::mandoc) fn begin_code_table_cell(&mut self) {
        debug_assert!(self.table_presentation.is_none());
        self.table_presentation = Some(TablePresentationFont {
            current: Font::Code,
            previous: self.display_current,
        });
    }

    pub(in crate::mandoc) fn end_code_table_cell(&mut self) {
        debug_assert!(self.table_presentation.is_some());
        self.table_presentation = None;
    }

    /// Enter CVS `termp_sh_pre()`/`termp_ss_pre()` font execution.
    ///
    /// Heading presentation is structural in the IR, but the terminal
    /// formatter still pushes bold and enables `fontibi`: an inner `\fI`
    /// selects bold-emphasis and updates the independent previous-font
    /// register.  The returned pair restores only the current stack and the
    /// mode flag; `previous` deliberately survives the scope.
    pub(in crate::mandoc) fn push_heading_scope(&mut self) -> (FontScope, bool) {
        let saved_mode = self.heading_bold_italic;
        let saved_font = self.push_scope(Font::Strong);
        self.heading_bold_italic = true;
        (saved_font, saved_mode)
    }

    pub(in crate::mandoc) fn pop_heading_scope(&mut self, saved: (FontScope, bool)) {
        self.pop_scope(saved.0);
        self.heading_bold_italic = saved.1;
    }

    /// Execute the replacement-font lifecycle used by man(7) SH/SS nodes.
    ///
    /// Unlike mdoc's font stack, `print_man_node()` replaces the font before
    /// and after each structural block/head/body node.  The two final resets
    /// leave both the current and previous-font registers at regular.
    pub(in crate::mandoc) fn begin_man_heading(&mut self) {
        self.select(Font::Regular);
        self.select(Font::Strong);
        self.heading_bold_italic = true;
    }

    pub(in crate::mandoc) fn end_man_heading(&mut self) {
        self.heading_bold_italic = false;
        self.select(Font::Regular);
        self.select(Font::Regular);
    }
}
