//! The single event cursor owns source offsets and shared nesting accounting.
#[cfg(test)]
mod tests;
use pulldown_cmark::{Event, Options, Tag};
use std::ops::Range;
pub(super) type SpannedEvent<'a> = (Event<'a>, Range<usize>);

pub(super) fn markdown_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
        | Options::ENABLE_MATH
        | Options::ENABLE_GFM
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_WIKILINKS
}

pub(super) struct EventCursor<'a> {
    events: Vec<SpannedEvent<'a>>,
    position: usize,
    depth: usize,
}

/// Recursion budget shared by nested block containers and inline spans.
///
/// Parsing recurses once per nesting level, so unbounded input depth would
/// overflow the stack before any allocation limit applies. Subtrees beyond
/// this depth are preserved as unsupported source text with a diagnostic.
const MAX_NESTING_DEPTH: usize = 64;

impl<'a> EventCursor<'a> {
    pub(super) fn new(events: Vec<SpannedEvent<'a>>) -> Self {
        Self {
            events,
            position: 0,
            depth: 0,
        }
    }

    /// Reserve one nesting level; callers must pair with [`Self::ascend`].
    pub(super) fn try_descend(&mut self) -> bool {
        if self.depth >= MAX_NESTING_DEPTH {
            return false;
        }
        self.depth += 1;
        true
    }

    pub(super) fn ascend(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    pub(super) fn peek(&self) -> Option<&SpannedEvent<'a>> {
        self.events.get(self.position)
    }

    /// The parser wraps direct item paragraphs only in loose lists. Nested
    /// containers have their own tightness and must not influence this list.
    pub(super) fn item_has_direct_paragraph(&self) -> bool {
        let mut depth = 0usize;
        for (event, _) in &self.events[self.position..] {
            match event {
                Event::Start(Tag::Paragraph) if depth == 0 => return true,
                Event::Start(_) => depth += 1,
                Event::End(_) if depth == 0 => break,
                Event::End(_) => depth -= 1,
                _ => {}
            }
        }
        false
    }

    pub(super) fn next(&mut self) -> Option<SpannedEvent<'a>> {
        let event = self.events.get(self.position)?.clone();
        self.position += 1;
        Some(event)
    }

    /// Consume the remainder of a just-opened tag, including nested tags.
    pub(super) fn consume_balanced(&mut self, start: Range<usize>) -> Range<usize> {
        let mut depth = 1usize;
        let mut end = start.end;
        while let Some((event, range)) = self.next() {
            end = range.end;
            match event {
                Event::Start(_) => depth = depth.saturating_add(1),
                Event::End(_) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        start.start..end
    }

    pub(super) fn subtree_contains_task_marker(&self) -> bool {
        let mut depth = 1usize;
        for (event, _) in &self.events[self.position..] {
            match event {
                Event::TaskListMarker(_) => return true,
                Event::Start(_) => depth = depth.saturating_add(1),
                Event::End(_) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return false;
                    }
                }
                _ => {}
            }
        }
        false
    }
}
