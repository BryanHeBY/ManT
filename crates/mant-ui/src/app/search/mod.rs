//! Search-surface facade; input, page jobs and outline jobs stay distinct.
mod input;
mod outline;
mod page;
mod repaint;

use super::App;
pub(super) use input::{SearchCommand, SearchInput, SearchMode, SearchOptions};
pub(super) use outline::{OutlineSearchState, SearchTarget};
pub(super) use page::SearchState;
#[cfg(test)]
pub(super) use repaint::SearchRepaint;

impl App {
    pub(super) fn active_search_input(&self) -> &SearchInput {
        match self.search_target {
            SearchTarget::Page => &self.search,
            SearchTarget::Outline => &self.outline_search.input,
        }
    }
    pub(super) fn active_search_input_mut(&mut self) -> &mut SearchInput {
        match self.search_target {
            SearchTarget::Page => &mut self.search,
            SearchTarget::Outline => &mut self.outline_search.input,
        }
    }
    pub(super) fn search_is_open(&self) -> bool {
        self.active_search_input().is_open()
    }
    pub(super) fn search_prompt(&self) -> String {
        self.search_prompt_for_width(self.geometry.status.width)
    }
    pub(super) fn search_prompt_for_width(&self, width: u16) -> String {
        let input = self.active_search_input();
        let direction = if input.direction > 0 { '/' } else { '?' };
        if width <= 20 {
            return format!("{direction} ");
        }
        if width < 70 {
            return format!(
                "{}{direction} [{} {}{}]: ",
                if self.search_target == SearchTarget::Page {
                    'P'
                } else {
                    'O'
                },
                if input.options.syntax == mant_protocol::SearchSyntax::Regex {
                    'R'
                } else {
                    'L'
                },
                match input.options.case {
                    mant_protocol::SearchCase::Sensitive => "Aa",
                    mant_protocol::SearchCase::Insensitive => "aa",
                    mant_protocol::SearchCase::Smart => "S",
                },
                if input.wrap { " W" } else { "" }
            );
        }
        format!(
            " Find {} {} [{} · {}{}]: ",
            if self.search_target == SearchTarget::Page {
                "Page"
            } else {
                "Outline"
            },
            direction,
            if input.options.syntax == mant_protocol::SearchSyntax::Regex {
                "Regex"
            } else {
                "Literal"
            },
            match input.options.case {
                mant_protocol::SearchCase::Sensitive => "Case",
                mant_protocol::SearchCase::Insensitive => "Ignore case",
                mant_protocol::SearchCase::Smart => "Smart case",
            },
            if input.wrap { " · Wrap" } else { "" }
        )
    }
}
