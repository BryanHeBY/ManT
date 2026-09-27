//! One visible HEAD pass for native literal declaration boundaries.
//!
//! `mdoc_macro.c::blk_full/in_line` retain each Cm/Ic and Ar instance, while
//! `mdoc_term.c::termp_it_pre/termp_bold_pre/termp_under_pre` only execute
//! their display. A new macro instance is not, by itself, a new declaration:
//! `ManT` must keep parameter enclosures across those instances. These rules
//! select semantic candidates without changing the native display.

use super::{OwnerHeadComponent, OwnerHeadRole};
use std::ops::Range;

#[derive(Default)]
struct VisibleScope {
    quote: Option<char>,
    closers: Vec<char>,
    uncertain: bool,
}

impl VisibleScope {
    fn closed(&self) -> bool {
        self.quote.is_none() && self.closers.is_empty() && !self.uncertain
    }

    fn advance(&mut self, character: char, previous: Option<char>) {
        if let Some(close) = self.quote {
            if character == close {
                self.quote = None;
            }
            return;
        }
        let quote = match character {
            '"' => Some('"'),
            '\'' if previous.is_none_or(|before| {
                before.is_whitespace() || matches!(before, '=' | ',' | '|' | '[' | '{' | '(')
            }) =>
            {
                Some('\'')
            }
            '“' => Some('”'),
            '‘' => Some('’'),
            _ => None,
        };
        if let Some(close) = quote {
            self.quote = Some(close);
            return;
        }
        if let Some(close) = match character {
            '[' => Some(']'),
            '(' => Some(')'),
            '{' => Some('}'),
            '<' => Some('>'),
            _ => None,
        } {
            if self.closers.len() == 64 {
                self.uncertain = true;
            } else {
                self.closers.push(close);
            }
        } else if matches!(character, ']' | ')' | '}' | '>')
            && self.closers.pop() != Some(character)
        {
            self.uncertain = true;
        }
    }
}

/// Mark only literal instances that begin a name after a *visible*,
/// parameter-external separator. Every glyph is visited once; component
/// ranges and returned flags are in the same checked byte coordinate space.
pub(super) fn literal_name_starts(
    form: &str,
    components: &[OwnerHeadComponent],
    ranges: &[Range<usize>],
) -> Option<Vec<bool>> {
    if components.len() != ranges.len() {
        return None;
    }
    let mut previous_end = 0;
    for range in ranges {
        if range.start < previous_end
            || range.start >= range.end
            || range.end > form.len()
            || form.get(range.clone()).is_none()
        {
            return None;
        }
        previous_end = range.end;
    }

    let mut starts = vec![false; ranges.len()];
    let mut component_index = 0;
    let mut scope = VisibleScope::default();
    let mut previous = None;
    let mut seen_literal = false;
    let mut in_argument = false;
    let mut separator = false;
    for (offset, character) in form.char_indices() {
        while ranges
            .get(component_index)
            .is_some_and(|range| range.end <= offset)
        {
            component_index += 1;
        }
        let active = ranges
            .get(component_index)
            .is_some_and(|range| range.start <= offset && offset < range.end);
        if ranges
            .get(component_index)
            .is_some_and(|range| range.start == offset)
        {
            match components[component_index].role {
                OwnerHeadRole::Literal if !seen_literal => {
                    if !scope.closed() || !form.get(..offset)?.trim().is_empty() {
                        return None;
                    }
                    starts[component_index] = true;
                    seen_literal = true;
                    in_argument = false;
                }
                OwnerHeadRole::Literal if scope.closed() && separator => {
                    starts[component_index] = true;
                    in_argument = false;
                }
                OwnerHeadRole::Literal if in_argument || !scope.closed() => {}
                OwnerHeadRole::Argument if seen_literal => in_argument = true,
                _ => return None,
            }
            separator = false;
        }
        // Punctuation inside an Ar or a suppressed Cm is still parameter
        // content. Only an external separator after the complete enclosure
        // can restart a declaration; whitespace or font changes cannot.
        if !active && scope.closed() {
            if matches!(character, ',' | '|') {
                separator = true;
            } else if !character.is_whitespace() {
                separator = false;
            }
        }
        scope.advance(character, previous);
        previous = Some(character);
    }
    seen_literal.then_some(starts)
}
