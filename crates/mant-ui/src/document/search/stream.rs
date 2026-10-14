//! Resumable scalar-folded literal matching, including arbitrarily long records.
//!
//! Work is charged per source byte. The prefix automaton and its byte-to-source
//! ring are bounded by the query, not the record, and survive slice boundaries.

use std::{collections::VecDeque, ops::Range};

#[derive(Debug, Clone)]
pub(crate) struct SearchStream {
    needle: Vec<u8>,
    prefixes: Vec<usize>,
    matched: usize,
    source: VecDeque<Range<usize>>,
    pub(crate) offset: usize,
}

impl SearchStream {
    pub(crate) fn new(query: &str) -> Self {
        let needle: Vec<u8> = query
            .chars()
            .flat_map(char::to_lowercase)
            .collect::<String>()
            .into_bytes();
        let mut prefixes = vec![0; needle.len()];
        for index in 1..needle.len() {
            let mut length = prefixes[index - 1];
            while length > 0 && needle[index] != needle[length] {
                length = prefixes[length - 1];
            }
            if needle[index] == needle[length] {
                length += 1;
            }
            prefixes[index] = length;
        }
        Self {
            needle,
            prefixes,
            matched: 0,
            source: VecDeque::new(),
            offset: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.offset = 0;
        self.matched = 0;
        self.source.clear();
    }

    /// Consume at most `budget` original bytes (plus one complete UTF-8 scalar).
    /// Output retains the synchronous matcher's non-overlapping semantics.
    pub(crate) fn advance(&mut self, text: &str, budget: &mut usize) -> Vec<Range<usize>> {
        let mut hits = Vec::new();
        if self.needle.is_empty() {
            self.offset = text.len();
            return hits;
        }
        while *budget > 0 && self.offset < text.len() {
            let character = text[self.offset..].chars().next().expect("scalar boundary");
            let start = self.offset;
            self.offset += character.len_utf8();
            *budget = budget.saturating_sub(character.len_utf8());
            for folded in character.to_lowercase() {
                let mut bytes = [0; 4];
                for &byte in folded.encode_utf8(&mut bytes).as_bytes() {
                    self.source.push_back(start..self.offset);
                    if self.source.len() > self.needle.len() {
                        self.source.pop_front();
                    }
                    while self.matched > 0 && byte != self.needle[self.matched] {
                        self.matched = self.prefixes[self.matched - 1];
                    }
                    if byte == self.needle[self.matched] {
                        self.matched += 1;
                    }
                    if self.matched == self.needle.len() {
                        hits.push(self.source.front().expect("matched source").start..self.offset);
                        self.matched = 0;
                    }
                }
            }
        }
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::LiteralSearch;

    #[test]
    fn every_chunk_size_preserves_unicode_and_nonoverlapping_literal_ranges() {
        for (text, query) in [
            ("aaaaa", "aa"),
            ("ababa", "aba"),
            ("ΟΣ οσ ος", "ΟΣ"),
            ("İstanbul İSTANBUL", "i\u{307}st"),
            ("é界e\u{301}界", "界"),
            ("prefix ababac suffix ababac", "ababac"),
            ("İ", "\u{307}"),
        ] {
            let expected = LiteralSearch::new(query).ranges(text, usize::MAX);
            for size in 1..=text.len() {
                let mut stream = SearchStream::new(query);
                let mut hits = Vec::new();
                while stream.offset < text.len() {
                    hits.extend(stream.advance(text, &mut size.clone()));
                }
                assert_eq!(hits, expected, "{text:?}/{query:?}, chunk {size}");
            }
        }
    }

    #[test]
    fn long_record_work_and_memory_are_bounded_and_resumable() {
        let text = format!("{}needle", "x".repeat(100_000));
        let mut stream = SearchStream::new("needle");
        assert_eq!(stream.advance(&text, &mut 1024), Vec::<Range<usize>>::new());
        assert_eq!(stream.offset, 1024);
        assert!(stream.source.len() <= stream.needle.len());
        let mut hits = Vec::new();
        while stream.offset < text.len() {
            hits.extend(stream.advance(&text, &mut 1024));
        }
        assert_eq!(hits, vec![100_000..100_006]);
        stream.reset();
        assert_eq!(stream.advance("needle", &mut 1024), vec![0..6]);
    }
}
