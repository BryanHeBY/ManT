//! Keep declaration separators and parameter styling distinct until extraction.
use super::declaration::DeclarationState;
use mant_ir::{ContentContext, ContentRef, Inline, InlineView, literal_option_aliases};
use std::ops::Range;

use mant_ir::inline_plain_text as plain_text;

/// Borrowed authored syntax. Candidate generation never rewrites its complete
/// form; only the temporary selector candidates are split.
pub(super) struct AuthoredForm<'a> {
    content: ContentContext<'a>,
    inlines: &'a [Inline],
}

impl<'a> AuthoredForm<'a> {
    pub(super) const fn new(content: ContentContext<'a>, inlines: &'a [Inline]) -> Self {
        Self { content, inlines }
    }

    pub(super) fn option_candidates(&self) -> impl Iterator<Item = FormCandidate<'a>> {
        option_alias_groups(self.content, self.inlines)
            .into_iter()
            .scan(0, |offset, inlines| {
                let start = *offset;
                // All removed separators are one ASCII byte. Opaque argument
                // runs were not split, and remain part of this visible length.
                *offset += plain_text(self.content, &inlines).len() + 1;
                Some(FormCandidate {
                    content: self.content,
                    inlines,
                    start,
                    token: None,
                })
            })
            .flat_map(FormCandidate::paired_invocations)
    }
}

/// Own the styled candidate until the parameter decision is complete. Callers
/// receive a lexical token only through that decision, not a flattenable tree.
pub(super) struct FormCandidate<'a> {
    content: ContentContext<'a>,
    inlines: Vec<Inline>,
    start: usize,
    token: Option<(String, usize)>,
}

impl FormCandidate<'_> {
    pub(super) fn invocation_token(&self) -> Option<(String, usize)> {
        if let Some(token) = &self.token {
            return Some(token.clone());
        }
        if starts_with_parameter(self.content, &self.inlines) {
            return None;
        }
        let mut prefix = String::new();
        append_name_prefix(self.content, &self.inlines, &mut prefix);
        let token = invocation_token(&prefix);
        if token.is_empty() {
            return None;
        }
        // `token` is the exact subslice selected by the grammar, including
        // removal of whitespace and authored enclosing punctuation.
        let offset = token.as_ptr() as usize - prefix.as_ptr() as usize;
        Some((token.to_owned(), self.start + offset))
    }

    fn paired_invocations(self) -> Vec<Self> {
        if let Some(tokens) = pattern_declarations(self.content, &self.inlines) {
            return tokens
                .into_iter()
                .map(|(token, offset)| Self {
                    content: self.content,
                    inlines: Vec::new(),
                    start: self.start,
                    token: Some((token, self.start + offset)),
                })
                .collect();
        }
        let mut prefix = String::new();
        if !append_name_prefix(self.content, &self.inlines, &mut prefix) {
            return vec![self];
        }
        let Some(tokens) = literal_option_aliases(&prefix) else {
            return vec![self];
        };
        tokens
            .into_iter()
            .map(|(name, range)| Self {
                content: self.content,
                inlines: Vec::new(),
                start: self.start,
                token: Some((name, self.start + range.start)),
            })
            .collect()
    }
}

/// A complete whitespace-separated literal head may contain a pattern plus
/// independently spelled long options. Accept no operands, argument styling,
/// assignment suffixes or prose; a pattern itself never expands into names.
fn pattern_declarations(
    content: ContentContext<'_>,
    inlines: &[Inline],
) -> Option<Vec<(String, usize)>> {
    let mut literal = String::new();
    if !append_name_prefix(content, inlines, &mut literal) {
        return None;
    }
    pattern_declarations_text(&literal)
}

fn pattern_declarations_text(literal: &str) -> Option<Vec<(String, usize)>> {
    let tokens = literal.split_whitespace().collect::<Vec<_>>();
    if tokens.len() < 2
        || !tokens[0].starts_with('-')
        || !tokens[0].contains('#')
        || !tokens[0].chars().all(|c| matches!(c, '-' | '#'))
        || !tokens[1..].iter().all(|token| {
            token.starts_with("--") && super::options::option_prefix(token) == Some(*token)
        })
    {
        return None;
    }
    Some(
        tokens[1..]
            .iter()
            .map(|token| {
                (
                    (*token).to_owned(),
                    token.as_ptr() as usize - literal.as_ptr() as usize,
                )
            })
            .collect(),
    )
}

/// Read visible literal content only until an explicitly styled parameter.
/// Transparent wrappers do not erase that boundary, even without whitespace.
fn append_name_prefix(content: ContentContext<'_>, nodes: &[Inline], output: &mut String) -> bool {
    for node in nodes {
        match content.inline(node).expect("definition content resolves") {
            InlineView::Text(value) | InlineView::Code(value) => output.push_str(value),
            InlineView::Strong(children) => {
                if !append_name_prefix(content, children, output) {
                    return false;
                }
            }
            InlineView::Link(link) => {
                if !append_name_prefix(content, link.children(), output) {
                    return false;
                }
            }
            InlineView::Emphasis(children) => {
                if first_content_is_parameter(content, children).is_some() {
                    return false;
                }
                // Whitespace-only styling is not a parameter, but still
                // occupies bytes before a later recognized name.
                output.push_str(&plain_text(content, children));
            }
            InlineView::Anchor(_) => {}
            InlineView::LineBreak => output.push('\n'),
            _ => unreachable!("all inline views are handled"),
        }
    }
    true
}

pub(super) fn literal_prefix(content: ContentContext<'_>, inlines: &[Inline]) -> String {
    let mut prefix = String::new();
    append_name_prefix(content, inlines, &mut prefix);
    prefix
}

/// An adjacent placeholder is part of the variable name, not the boundary of
/// a shorter exact name. Separated operands and assignment values are different.
pub(super) fn environment_prefix(
    content: ContentContext<'_>,
    inlines: &[Inline],
) -> Option<String> {
    let mut prefix = String::new();
    let complete = append_name_prefix(content, inlines, &mut prefix);
    (complete || prefix.ends_with(char::is_whitespace) || prefix.contains('=')).then_some(prefix)
}

/// Split complete declarations without flattening parameter spans. Bracket
/// nesting and local argument phases survive strong/link wrapper boundaries;
/// punctuation inside an argument is not a fresh declaration.
pub(super) fn declaration_groups(content: ContentContext<'_>, term: &[Inline]) -> Vec<Vec<Inline>> {
    let mut ranges = SplitRanges::default();
    split_groups(
        content,
        term,
        &[',', '|'],
        &mut None,
        &mut DeclarationState::new(content, plain_text(content, term), term),
        &mut ranges,
        false,
    )
}

/// Return the exact visible byte ranges selected by the same declaration
/// state machine as [`declaration_groups`]. Callers can bind those decisions
/// back to the original styled tree without searching flattened text.
#[allow(dead_code)] // Source-neutral range grammar retained for later native evidence binding.
pub(in crate::definitions) fn declaration_group_ranges(
    content: ContentContext<'_>,
    term: &[Inline],
) -> Vec<Range<usize>> {
    let text = plain_text(content, term);
    let mut ranges = SplitRanges::tracking();
    let _ = split_groups(
        content,
        term,
        &[',', '|'],
        &mut None,
        &mut DeclarationState::new(content, text.clone(), term),
        &mut ranges,
        false,
    );
    ranges.finish(&text)
}

/// Slashes only separate the invocation token after its option grammar has
/// been validated. Keep candidate trees intact so each candidate still passes
/// the parameter check; never split argument paths later in the form.
fn option_alias_groups(content: ContentContext<'_>, term: &[Inline]) -> Vec<Vec<Inline>> {
    declaration_groups(content, term)
        .into_iter()
        .flat_map(|group| {
            let text = plain_text(content, &group);
            let token = invocation_token(&text);
            if super::slash_option_forms(token).is_none() {
                return vec![group];
            }
            // The token is a substring of text; the prefix can include blank
            // styling or opening brackets excluded by invocation_token.
            let Some(start) = text.find(token) else {
                return vec![group];
            };
            split_groups(
                content,
                &group,
                &['/'],
                &mut Some(start + token.len()),
                &mut DeclarationState::new(content, text, &group).within_validated_token(),
                &mut SplitRanges::default(),
                false,
            )
        })
        .collect()
}

fn invocation_token(text: &str) -> &str {
    text.split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(|character: char| {
            matches!(
                character,
                '[' | ']' | '(' | ')' | '{' | '}' | '“' | '”' | '‘' | '’'
            )
        })
}

/// Select option candidates from one complete literal leaf without building a
/// temporary string-owning inline tree.
pub(super) fn literal_option_tokens(value: &str) -> Vec<(String, usize)> {
    let mut state = DeclarationState::literal(value);
    let mut ranges = Vec::new();
    let mut start = 0;
    for (offset, character) in value.char_indices() {
        if state.separator(character, matches!(character, ',' | '|')) {
            ranges.push(start..offset);
            start = offset + character.len_utf8();
        }
    }
    ranges.push(start..value.len());

    ranges
        .into_iter()
        .filter_map(|range| trim_range(value, range))
        .flat_map(|range| {
            let group = &value[range.clone()];
            let token = invocation_token(group);
            let token_start = range.start + token.as_ptr() as usize - group.as_ptr() as usize;
            if let Some(parts) = super::slash_option_forms(token) {
                return parts
                    .into_iter()
                    .scan(token_start, |offset, part| {
                        let start = *offset;
                        *offset += part.len() + 1;
                        Some((part.to_owned(), start))
                    })
                    .collect::<Vec<_>>();
            }
            if let Some(tokens) = pattern_declarations_text(group) {
                return tokens
                    .into_iter()
                    .map(|(token, offset)| (token, range.start + offset))
                    .collect();
            }
            if let Some(tokens) = literal_option_aliases(group) {
                return tokens
                    .into_iter()
                    .map(|(token, offset)| (token, range.start + offset.start))
                    .collect();
            }
            if token.is_empty() {
                Vec::new()
            } else {
                vec![(token.to_owned(), token_start)]
            }
        })
        .collect()
}

/// One style-preserving splitter for alias punctuation. A bounded pass counts
/// visible bytes even in opaque arguments. Links preserve their wrapper while
/// exposing their visible children; their destination is never name evidence.
fn split_groups(
    content: ContentContext<'_>,
    term: &[Inline],
    separators: &[char],
    remaining: &mut Option<usize>,
    state: &mut DeclarationState,
    ranges: &mut SplitRanges,
    styled: bool,
) -> Vec<Vec<Inline>> {
    let mut groups = vec![Vec::new()];
    for inline in term {
        let parts = match inline {
            Inline::Text { content: reference } => split_leaf(
                content, *reference, false, separators, remaining, state, ranges, styled,
            ),
            Inline::Code { content: reference } => split_leaf(
                content, *reference, true, separators, remaining, state, ranges, styled,
            ),
            Inline::Strong { children } => split_groups(
                content, children, separators, remaining, state, ranges, styled,
            )
            .into_iter()
            .map(|children| vec![Inline::Strong { children }])
            .collect(),
            Inline::Link {
                occurrence,
                children,
            } => split_groups(
                content, children, separators, remaining, state, ranges, styled,
            )
            .into_iter()
            .map(|children| {
                vec![Inline::Link {
                    occurrence: *occurrence,
                    children,
                }]
            })
            .collect(),
            Inline::Emphasis { children } => split_groups(
                content, children, separators, remaining, state, ranges, true,
            )
            .into_iter()
            .map(|children| vec![Inline::Emphasis { children }])
            .collect(),
            _ => {
                let text = plain_text(content, std::slice::from_ref(inline));
                state.opaque(&text);
                ranges.opaque(text.len());
                if let Some(bytes) = remaining {
                    *bytes = bytes.saturating_sub(text.len());
                }
                vec![vec![inline.clone()]]
            }
        };
        for (index, part) in parts.into_iter().enumerate() {
            if index > 0 {
                groups.push(Vec::new());
            }
            groups.last_mut().expect("at least one group").extend(part);
        }
    }
    groups
}

#[allow(clippy::too_many_arguments)]
fn split_leaf(
    content: ContentContext<'_>,
    reference: ContentRef,
    code: bool,
    separators: &[char],
    remaining: &mut Option<usize>,
    state: &mut DeclarationState,
    ranges: &mut SplitRanges,
    styled: bool,
) -> Vec<Vec<Inline>> {
    let value = content
        .resolve_text(reference)
        .expect("definition leaf resolves through its content store");
    let mut output = Vec::new();
    let mut start = 0usize;
    for (offset, character) in value.char_indices() {
        let eligible = take_separator(character, separators, remaining);
        let separator = if styled {
            state.styled_character(character, eligible)
        } else {
            state.separator(character, eligible)
        };
        ranges.character(character, separator);
        if separator {
            output.push(vec![slice_leaf(reference, start, offset, code)]);
            start = offset + character.len_utf8();
        }
    }
    output.push(vec![slice_leaf(reference, start, value.len(), code)]);
    output
}

fn slice_leaf(reference: ContentRef, start: usize, end: usize, code: bool) -> Inline {
    let start = reference
        .bytes
        .start
        .checked_add(u32::try_from(start).expect("leaf slice start fits u32"))
        .expect("leaf slice start stays within its atom");
    let end = reference
        .bytes
        .start
        .checked_add(u32::try_from(end).expect("leaf slice end fits u32"))
        .expect("leaf slice end stays within its atom");
    let content = ContentRef {
        atom: reference.atom,
        bytes: mant_ir::ContentByteRange { start, end },
    };
    if code {
        Inline::Code { content }
    } else {
        Inline::Text { content }
    }
}

#[derive(Default)]
struct SplitRanges {
    enabled: bool,
    start: usize,
    offset: usize,
    groups: Vec<Range<usize>>,
}

#[allow(dead_code)] // Range tracking is retained for the next native semantic evidence unit.
impl SplitRanges {
    fn tracking() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }

    fn character(&mut self, character: char, separator: bool) {
        if !self.enabled {
            return;
        }
        if separator {
            self.groups.push(self.start..self.offset);
            self.offset += character.len_utf8();
            self.start = self.offset;
        } else {
            self.offset += character.len_utf8();
        }
    }

    fn opaque(&mut self, length: usize) {
        if self.enabled {
            self.offset += length;
        }
    }

    fn finish(mut self, text: &str) -> Vec<Range<usize>> {
        self.groups.push(self.start..self.offset);
        self.groups
            .into_iter()
            .filter_map(|range| trim_range(text, range))
            .collect()
    }
}

fn trim_range(text: &str, range: Range<usize>) -> Option<Range<usize>> {
    let value = text.get(range.clone())?;
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| {
        let start = trimmed.as_ptr() as usize - value.as_ptr() as usize;
        range.start + start..range.start + start + trimmed.len()
    })
}

fn take_separator(character: char, separators: &[char], remaining: &mut Option<usize>) -> bool {
    let eligible = remaining.is_none_or(|bytes| bytes >= character.len_utf8());
    if let Some(bytes) = remaining {
        *bytes = bytes.saturating_sub(character.len_utf8());
    }
    eligible && separators.contains(&character)
}

fn starts_with_parameter(content: ContentContext<'_>, term: &[Inline]) -> bool {
    first_content_is_parameter(content, term).unwrap_or(false)
}

/// Locate content, not merely a wrapper: separators may leave empty strong
/// runs and anchors before the argument. Preserve emphasis ancestry instead
/// of flattening text and losing the distinction between a name and a value.
fn first_content_is_parameter(content: ContentContext<'_>, term: &[Inline]) -> Option<bool> {
    term.iter().find_map(|inline| {
        match content.inline(inline).expect("definition content resolves") {
            InlineView::Anchor(_) | InlineView::LineBreak => None,
            InlineView::Text(value) | InlineView::Code(value) => {
                (!value.trim().is_empty()).then_some(false)
            }
            InlineView::Strong(children) => first_content_is_parameter(content, children),
            InlineView::Link(link) => first_content_is_parameter(content, link.children()),
            InlineView::Emphasis(children) => {
                first_content_is_parameter(content, children).map(|_| true)
            }
            _ => unreachable!("all inline views are handled"),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_content as fixture;

    fn declaration_groups(term: &[Inline]) -> Vec<Vec<Inline>> {
        super::declaration_groups(fixture::content(), term)
    }

    fn declaration_group_ranges(term: &[Inline]) -> Vec<Range<usize>> {
        super::declaration_group_ranges(fixture::content(), term)
    }

    fn plain_text(term: &[Inline]) -> String {
        super::plain_text(fixture::content(), term)
    }

    fn option_names_from_terms(terms: &[Vec<Inline>]) -> Vec<String> {
        super::super::option_names_from_terms(fixture::content(), terms)
    }

    #[test]
    fn declaration_state_crosses_wrappers_and_bounds_uncertain_nesting() {
        let text = fixture::text;
        let link = |children| {
            fixture::link(
                mant_ir::LinkTarget::External {
                    uri: "https://example.invalid/".into(),
                },
                None,
                children,
            )
        };
        for parameter in ["[a|b]", "{+|-}", "<日本,名前>", "[a|b", "a] | b"] {
            let term = vec![
                Inline::Strong {
                    children: vec![text("set ")],
                },
                link(vec![text(parameter)]),
            ];
            assert_eq!(declaration_groups(&term), vec![term], "{parameter}");
        }
        let deeply_nested = format!("set {}x{} | phantom", "[".repeat(65), "]".repeat(65));
        let term = vec![text(&deeply_nested)];
        assert_eq!(declaration_groups(&term), vec![term]);

        let term = vec![
            text("--界"),
            Inline::Emphasis {
                children: vec![link(vec![text("値,--FAKE")])],
            },
            text(", --other"),
        ];
        let names: Vec<_> = AuthoredForm::new(fixture::content(), &term)
            .option_candidates()
            .filter_map(|candidate| candidate.invocation_token())
            .collect();
        assert_eq!(
            names,
            [
                ("--界".into(), 0),
                ("--other".into(), "--界値,--FAKE, ".len())
            ]
        );

        let term = vec![text("--mode=[a|b], --other")];
        assert_eq!(declaration_groups(&term).len(), 2);
    }

    #[test]
    fn declaration_ranges_track_utf8_wrappers_opaque_arguments_and_repeated_text() {
        let text = fixture::text;
        let term = vec![
            Inline::Strong {
                children: vec![
                    text("--界"),
                    Inline::Emphasis {
                        children: vec![text("値,同")],
                    },
                ],
            },
            text(", "),
            fixture::link(
                mant_ir::LinkTarget::External {
                    uri: "https://example.invalid/".into(),
                },
                None,
                vec![Inline::Strong {
                    children: vec![text("--界")],
                }],
            ),
        ];
        let visible = plain_text(&term);
        let ranges = declaration_group_ranges(&term);
        assert_eq!(
            ranges
                .iter()
                .map(|range| &visible[range.clone()])
                .collect::<Vec<_>>(),
            ["--界値,同", "--界"]
        );
        assert_eq!(ranges, [0..12, 14..19]);

        for value in ["--set=KEY,VALUE", "--set=KEY,[a,b]"] {
            let term = vec![Inline::Strong {
                children: vec![text(value)],
            }];
            assert_eq!(
                declaration_group_ranges(&term).as_slice(),
                std::slice::from_ref(&(0..value.len()))
            );
        }

        let adjacent = vec![
            Inline::Strong {
                children: vec![text("--set=KEY")],
            },
            text(","),
            Inline::Strong {
                children: vec![text("VALUE")],
            },
        ];
        assert_eq!(
            declaration_groups(&adjacent).as_slice(),
            std::slice::from_ref(&adjacent)
        );

        let spaced = vec![
            Inline::Strong {
                children: vec![text("--set=KEY")],
            },
            text(", "),
            Inline::Strong {
                children: vec![text("VALUE")],
            },
        ];
        assert_eq!(declaration_groups(&spaced).len(), 2);
    }

    #[test]
    fn negative_number_after_a_name_is_an_argument_not_an_alias_source() {
        // All three exact .IP labels ran pinned CVS -Tutf8 first. man_term.c::
        // pre_IP keeps one HEAD, while term.c::term_word prints the roman
        // negative-number parameter after the bold name.
        let head = vec![
            Inline::Strong {
                children: vec![fixture::text("--number")],
            },
            fixture::text(" -10,--fake,20"),
        ];
        assert_eq!(option_names_from_terms(&[head]), ["--number"]);

        let followed = vec![
            Inline::Strong {
                children: vec![fixture::text("--number")],
            },
            fixture::text(" -10,--fake,20, "),
            Inline::Strong {
                children: vec![fixture::text("--all")],
            },
        ];
        assert_eq!(option_names_from_terms(&[followed]), ["--number", "--all"]);

        let after_comma = vec![
            Inline::Strong {
                children: vec![fixture::text("--number")],
            },
            fixture::text(", -10,--fake,20"),
        ];
        assert_eq!(option_names_from_terms(&[after_comma]), ["--number"]);
    }

    #[test]
    fn argument_and_quote_state_cross_style_slices_and_literal_heads() {
        // Pinned CVS man_term.c::pre_TP/pre_IP and term.c::term_word render
        // these complete HEADs before name inference. A parameter's commas
        // do not create another authored option declaration.
        for parameter in ["-10,--fake,20", "-10%,--fake,20", "-10:20,--fake,30"] {
            for head in [
                vec![Inline::Strong {
                    children: vec![fixture::text(format!("--number {parameter}"))],
                }],
                vec![
                    Inline::Strong {
                        children: vec![fixture::text("--number")],
                    },
                    fixture::text(format!(" {parameter}")),
                ],
            ] {
                assert_eq!(
                    option_names_from_terms(&[head]),
                    ["--number"],
                    "{parameter}"
                );
            }
            assert_eq!(
                super::super::option_names_from_literal(&format!("--number {parameter}")),
                ["--number"],
                "Markdown: {parameter}"
            );
        }
        let quoted = "--pattern \"one, --fake,two\"";
        assert_eq!(
            option_names_from_terms(&[vec![Inline::Strong {
                children: vec![fixture::text(quoted)],
            }]]),
            ["--pattern"]
        );
        assert_eq!(
            option_names_from_terms(&[vec![
                Inline::Strong {
                    children: vec![fixture::text("--pattern ")],
                },
                fixture::text("\"one, "),
                Inline::Strong {
                    children: vec![fixture::text("--fake")],
                },
                fixture::text(",two\""),
            ]]),
            ["--pattern"]
        );
        assert_eq!(
            super::super::option_names_from_literal(quoted),
            ["--pattern"]
        );
    }

    #[test]
    fn transparent_links_preserve_separators_and_parameter_ancestry() {
        let text = fixture::text;
        let link = |children| {
            fixture::link(
                mant_ir::LinkTarget::External {
                    uri: "https://example.invalid/--not-a-name".into(),
                },
                None,
                children,
            )
        };
        let source = vec![link(vec![Inline::Strong {
            children: vec![
                text("-L"),
                Inline::Emphasis {
                    children: vec![text("dir,--FAKE")],
                },
                text(", --library"),
            ],
        }])];
        let original = source.clone();
        assert_eq!(
            option_names_from_terms(std::slice::from_ref(&source)),
            ["-L", "--library"]
        );
        assert_eq!(source, original);
        let argument = vec![Inline::Emphasis {
            children: vec![link(vec![text("-n,--FAKE")])],
        }];
        assert!(option_names_from_terms(&[argument]).is_empty());
        let slash = vec![link(vec![text("-n/-NUM")])];
        assert_eq!(option_names_from_terms(&[slash]), ["-n", "-NUM"]);
    }

    #[test]
    fn slash_grouping_keeps_styles_and_excludes_later_argument_paths() {
        let code = |value: &str| fixture::code(value.to_owned());
        let argument = |value: &str| Inline::Emphasis {
            children: vec![code(value)],
        };
        for spacer in [
            code(""),
            fixture::anchor("invisible"),
            Inline::Strong { children: vec![] },
        ] {
            for name in [
                argument("-NUM"),
                Inline::Strong {
                    children: vec![argument("-NUM")],
                },
            ] {
                let term = vec![code("-n/"), spacer.clone(), name];
                assert_eq!(option_names_from_terms(&[term]), ["-n"]);
            }
        }
        for (term, names) in [
            (vec![code("-n"), argument("/-NUM")], vec!["-n"]),
            (vec![code("-n/--number /tmp/-NUM")], vec!["-n", "--number"]),
            (
                vec![code("[-n/--number] /tmp/-NUM")],
                vec!["-n", "--number"],
            ),
            (
                vec![code("-n/--number"), argument(" /日本/"), code("-NUM")],
                vec!["-n", "--number"],
            ),
            (vec![code("--output=dir/-NUM")], vec!["--output"]),
            (vec![code("-n/-NUM")], vec!["-n", "-NUM"]),
        ] {
            assert_eq!(
                option_names_from_terms(std::slice::from_ref(&term)),
                names,
                "{term:?}"
            );
            // Option-only slash rules must not leak into command grouping.
            assert_eq!(declaration_groups(&term), vec![term]);
        }
    }

    #[test]
    fn empty_styling_cannot_hide_the_first_parameter_or_alias() {
        for spacer in [
            fixture::text(" \t"),
            fixture::code(""),
            fixture::code(" \t"),
            Inline::Strong { children: vec![] },
            Inline::Strong {
                children: vec![fixture::text(" ")],
            },
            Inline::Strong {
                children: vec![fixture::code(" "), fixture::anchor("invisible")],
            },
            Inline::Emphasis {
                children: vec![fixture::text(" ")],
            },
            fixture::anchor("invisible"),
            fixture::line_break(),
        ] {
            for separator in [",", "|"] {
                for parameter in [false, true] {
                    let text = fixture::text("-NUM");
                    let name = if parameter {
                        Inline::Emphasis {
                            children: vec![text],
                        }
                    } else {
                        Inline::Strong {
                            children: vec![text],
                        }
                    };
                    let term = vec![Inline::Strong {
                        children: vec![
                            fixture::text(format!("-n{separator}")),
                            spacer.clone(),
                            name,
                        ],
                    }];
                    assert_eq!(
                        option_names_from_terms(&[term]),
                        if parameter {
                            vec!["-n"]
                        } else {
                            vec!["-n", "-NUM"]
                        },
                        "{spacer:?}: {separator}: parameter={parameter}"
                    );
                }
            }
        }
    }
}
