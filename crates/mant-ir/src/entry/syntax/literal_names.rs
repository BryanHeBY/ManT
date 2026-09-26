//! Source-neutral option names in one independently established literal head.
//!
//! The owner and its final visible form are producer evidence. This grammar
//! selects byte ranges within that form; it never creates an owner or divides
//! its display. In particular, punctuation after `=` belongs to an argument
//! unless another complete option starts after an authored separator.

use super::{NameRange, lexical_option_token, literal_option_aliases_checked, option_prefix};
use std::ops::Range;

/// Select bounded option spellings from one complete native declaration head.
/// The caller must bind every returned byte range to its original display.
#[must_use]
pub fn literal_option_names(form: &str) -> Vec<(String, Range<usize>)> {
    // The shared alias grammar admits whitespace-separated short/long names
    // but keeps a slash after whitespace with a path-like operand.
    match literal_option_aliases_checked(form) {
        Ok(Some(aliases)) => return aliases,
        Err(()) => return Vec::new(),
        Ok(None) => {}
    }
    let (names, over_limit) = declaration_scan(
        form,
        &[],
        &[],
        &[],
        &[],
        &[],
        StyledBoundaryRule::SingleTextOperand,
    )
    .names(form);
    if over_limit { Vec::new() } else { names }
}

/// A native BI/BR operand boundary is known independently of its final font.
/// A single IP label has no such boundary; text punctuation cannot prove that
/// an already active styled argument has ended.
#[derive(Clone, Copy)]
pub(crate) enum StyledBoundaryRule {
    NativeComponents,
    SingleTextOperand,
}

/// The one text/state pass retains both complete declaration ranges and the
/// part of each range that was still eligible to contain names. In particular,
/// a styled parameter cannot be reparsed later as a fresh text-only head.
#[doc(hidden)]
#[derive(Debug)]
pub struct DeclarationScan {
    pub(crate) ranges: Vec<Range<usize>>,
    name_prefixes: Vec<Range<usize>>,
    numeric_name_starts: Vec<usize>,
}

impl DeclarationScan {
    /// Complete, untrimmed declaration intervals in visible UTF-8 bytes.
    /// The separating comma or pipe is outside both adjacent intervals.
    #[must_use]
    pub fn ranges(&self) -> &[Range<usize>] {
        &self.ranges
    }

    fn push(&mut self, start: usize, end: usize, prefix_end: Option<usize>) {
        self.ranges.push(start..end);
        self.name_prefixes
            .push(start..prefix_end.unwrap_or(end).min(end));
    }

    /// Parse only disjoint, already eligible prefixes. The stateful scan has
    /// excluded every argument byte before this small spelling pass begins.
    #[must_use]
    pub fn names(&self, form: &str) -> (Vec<(String, Range<usize>)>, bool) {
        let mut names = Vec::new();
        for prefix in &self.name_prefixes {
            let Some(value) = form.get(prefix.clone()) else {
                continue;
            };
            let leading = value.len() - value.trim_start().len();
            let group = value.trim();
            if group.is_empty() {
                continue;
            }
            let offset = prefix.start + leading;
            let selected = match literal_option_aliases_checked(group) {
                Err(()) => return (names, true),
                Ok(Some(aliases)) => aliases
                    .into_iter()
                    .map(|(name, range)| (name, offset + range.start..offset + range.end))
                    .collect(),
                Ok(None) => match slash_names(group, offset) {
                    Err(()) => return (names, true),
                    Ok(Some(slash)) => slash,
                    Ok(None) => match pattern_names(group, offset) {
                        Err(()) => return (names, true),
                        Ok(Some(pattern)) => pattern,
                        Ok(None) => {
                            leading_name_with_numeric(group, offset, &self.numeric_name_starts)
                                .into_iter()
                                .collect()
                        }
                    },
                },
            };
            if names.len() + selected.len() > 64 {
                return (names, true);
            }
            names.extend(selected);
        }
        (names, false)
    }
}

/// Bounded declaration intervals in one final visible head. A delimiter in
/// a quoted or bracketed argument is not a new declaration. Both native
/// component evidence and source-neutral literal spelling use these ranges.
#[cfg(test)]
pub(crate) fn literal_declaration_ranges(form: &str) -> Vec<Range<usize>> {
    declaration_scan(
        form,
        &[],
        &[],
        &[],
        &[],
        &[],
        StyledBoundaryRule::SingleTextOperand,
    )
    .ranges
}

/// Compatibility helper for tests that place independent operands exactly at
/// their first visible name. Production paths pass complete operand ranges.
#[cfg(test)]
pub(crate) fn literal_declaration_scan_with_starts(
    form: &str,
    independent_starts: &[usize],
    argument_starts: &[usize],
    boundary_rule: StyledBoundaryRule,
) -> DeclarationScan {
    let operands = independent_starts
        .iter()
        .enumerate()
        .map(|(index, &start)| {
            start
                ..independent_starts
                    .get(index + 1)
                    .copied()
                    .unwrap_or(form.len())
        })
        .collect::<Vec<_>>();
    declaration_scan(
        form,
        &operands,
        argument_starts,
        &[],
        &[],
        &[],
        boundary_rule,
    )
}

/// Scan one complete visible option head with native operand and final-style
/// evidence. All coordinates are UTF-8 byte offsets into `form`; they are
/// never source positions or terminal cells. Invalid or overlapping evidence
/// is rejected instead of being used to manufacture a declaration.
///
/// A native operand is the full visible interval of one alternating-font
/// macro child, not a font run. A font escape inside that child cannot create
/// an independent declaration. With no operands, the existing single-text
/// grammar remains in effect for Markdown and `.IP` labels.
#[doc(hidden)]
#[must_use]
pub fn scan_option_declarations(
    form: &str,
    native_operands: &[Range<usize>],
    argument_starts: &[usize],
) -> Option<DeclarationScan> {
    scan_option_declarations_with_numeric(form, native_operands, argument_starts, &[])
}

/// Scan a complete native head with independently proved numeric short names.
/// `numeric_name_starts` contains exact visible byte starts of bold `-[0-9]`
/// names. Each start must be the first nonblank glyph of a native operand, or
/// of the whole head when the native macro has only one text operand. Callers
/// must establish the native declaration role and final bold style separately;
/// a numeric-looking word in an existing argument is not such evidence.
#[doc(hidden)]
#[must_use]
pub fn scan_option_declarations_with_numeric(
    form: &str,
    native_operands: &[Range<usize>],
    argument_starts: &[usize],
    numeric_name_starts: &[usize],
) -> Option<DeclarationScan> {
    scan_option_declarations_with_style(
        form,
        native_operands,
        argument_starts,
        &[],
        numeric_name_starts,
    )
}

/// Scan one native head with final font evidence. A bold-underlined run is an
/// argument unless its first glyph is the head's independently witnessed
/// initial name or a name after a proved native-operand delimiter. A font
/// switch by itself does not terminate a parameter or an authored quote.
/// All offsets are UTF-8 byte positions in the final visible `form`.
#[doc(hidden)]
#[must_use]
pub fn scan_option_declarations_with_style(
    form: &str,
    native_operands: &[Range<usize>],
    plain_italic_starts: &[usize],
    bold_underline_starts: &[usize],
    numeric_name_starts: &[usize],
) -> Option<DeclarationScan> {
    scan_option_declarations_with_style_core(
        form,
        native_operands,
        plain_italic_starts,
        &[],
        bold_underline_starts,
        numeric_name_starts,
    )
}

/// Scan final visible styling with complete plain-italic byte intervals.
/// An interval can prove that a complete italic metavariable ended before a
/// delimiter; a font-start offset alone cannot distinguish that case from a
/// comma inside an ongoing parameter. Coordinates are checked UTF-8 bytes.
#[doc(hidden)]
#[must_use]
pub fn scan_option_declarations_with_style_ranges(
    form: &str,
    native_operands: &[Range<usize>],
    plain_italic_ranges: &[Range<usize>],
    bold_underline_starts: &[usize],
    numeric_name_starts: &[usize],
) -> Option<DeclarationScan> {
    let starts =
        checked_plain_italic_starts(form, plain_italic_ranges, ItalicStartMode::FirstVisible)?;
    scan_option_declarations_with_style_core(
        form,
        native_operands,
        &starts,
        plain_italic_ranges,
        bold_underline_starts,
        numeric_name_starts,
    )
}

/// Which existing scanner boundary an italic interval contributes.
#[derive(Clone, Copy)]
pub(super) enum ItalicStartMode {
    /// Final-style evidence starts at the first non-whitespace glyph.
    FirstVisible,
    /// The legacy textless native-component rule uses the exact range start.
    ExactRangeStart,
}

/// Validate final-style intervals and derive the scanner's boundary once.
/// This walks only the supplied intervals, never the complete HEAD per range.
pub(super) fn checked_plain_italic_starts(
    form: &str,
    plain_italic_ranges: &[Range<usize>],
    mode: ItalicStartMode,
) -> Option<Vec<usize>> {
    let mut previous_end = 0;
    let mut starts = Vec::with_capacity(plain_italic_ranges.len());
    for range in plain_italic_ranges {
        if range.start < previous_end
            || range.start >= range.end
            || range.end > form.len()
            || form.get(range.clone()).is_none()
        {
            return None;
        }
        match mode {
            ItalicStartMode::FirstVisible => {
                let visible = &form[range.clone()];
                if let Some(first) = visible.find(|character: char| !character.is_whitespace()) {
                    starts.push(range.start + first);
                }
            }
            ItalicStartMode::ExactRangeStart => starts.push(range.start),
        }
        previous_end = range.end;
    }
    Some(starts)
}

fn scan_option_declarations_with_style_core(
    form: &str,
    native_operands: &[Range<usize>],
    plain_italic_starts: &[usize],
    plain_italic_ranges: &[Range<usize>],
    bold_underline_starts: &[usize],
    numeric_name_starts: &[usize],
) -> Option<DeclarationScan> {
    if !valid_evidence(
        form,
        native_operands,
        plain_italic_starts,
        bold_underline_starts,
        numeric_name_starts,
    ) {
        return None;
    }
    let boundary_rule = if native_operands.is_empty() {
        StyledBoundaryRule::SingleTextOperand
    } else {
        StyledBoundaryRule::NativeComponents
    };
    Some(declaration_scan(
        form,
        native_operands,
        plain_italic_starts,
        bold_underline_starts,
        numeric_name_starts,
        plain_italic_ranges,
        boundary_rule,
    ))
}

pub(super) fn valid_evidence(
    form: &str,
    native_operands: &[Range<usize>],
    argument_starts: &[usize],
    bold_underline_starts: &[usize],
    numeric_name_starts: &[usize],
) -> bool {
    let mut end = 0;
    for operand in native_operands {
        if operand.start < end
            || operand.start >= operand.end
            || operand.end > form.len()
            || !form.is_char_boundary(operand.start)
            || !form.is_char_boundary(operand.end)
        {
            return false;
        }
        end = operand.end;
    }
    let mut previous = 0;
    for &start in argument_starts {
        if start < previous || start >= form.len() || !form.is_char_boundary(start) {
            return false;
        }
        previous = start;
    }
    if bold_underline_starts.len() > 64 {
        return false;
    }
    let mut previous_bold = None;
    for &start in bold_underline_starts {
        if previous_bold.is_some_and(|previous| start <= previous)
            || start >= form.len()
            || !form.is_char_boundary(start)
        {
            return false;
        }
        previous_bold = Some(start);
    }
    if numeric_name_starts.len() > 64 {
        return false;
    }
    let head_start =
        (!numeric_name_starts.is_empty()).then(|| form.len() - form.trim_start().len());
    let mut operand = 0;
    let mut previous_numeric = None;
    for &start in numeric_name_starts {
        if previous_numeric.is_some_and(|previous| start <= previous)
            || form.get(start..start.saturating_add(2)).is_none_or(|name| {
                let bytes = name.as_bytes();
                bytes.len() != 2 || bytes[0] != b'-' || !bytes[1].is_ascii_digit()
            })
        {
            return false;
        }
        previous_numeric = Some(start);
        if native_operands.is_empty() {
            if Some(start) != head_start {
                return false;
            }
            continue;
        }
        while native_operands
            .get(operand)
            .is_some_and(|range| range.end <= start)
        {
            operand += 1;
        }
        let Some(range) = native_operands.get(operand) else {
            return false;
        };
        let Some(visible) = form.get(range.clone()) else {
            return false;
        };
        if start != range.start + visible.len() - visible.trim_start().len() {
            return false;
        }
    }
    true
}

pub(crate) fn literal_declaration_scan_with_operands(
    form: &str,
    native_operands: &[Range<usize>],
    argument_starts: &[usize],
    boundary_rule: StyledBoundaryRule,
) -> DeclarationScan {
    declaration_scan(
        form,
        native_operands,
        argument_starts,
        &[],
        &[],
        &[],
        boundary_rule,
    )
}

#[cfg(test)]
pub(crate) fn literal_declaration_ranges_with_starts(
    form: &str,
    independent_starts: &[usize],
    argument_starts: &[usize],
) -> Vec<Range<usize>> {
    literal_declaration_scan_with_starts(
        form,
        independent_starts,
        argument_starts,
        StyledBoundaryRule::NativeComponents,
    )
    .ranges
}

/// Only a separate native operand can end an already active parameter. The
/// operand may begin at the name itself, with whitespace before the name, or
/// with the terminal delimiter. A font run inside the same operand has no
/// such authority (pinned `man_term.c::pre_alternate()` and `term.c::term_word()`).
fn independent_operand_after_separator(
    operands: &[Range<usize>],
    leading_ends: &[usize],
    cursor: &mut usize,
    separator: usize,
    candidate: usize,
) -> bool {
    while operands
        .get(*cursor)
        .is_some_and(|operand| operand.end <= candidate)
    {
        *cursor += 1;
    }
    let Some(operand) = operands.get(*cursor) else {
        return false;
    };
    if !(operand.start <= candidate && candidate < operand.end) {
        return false;
    }
    // `candidate` was found by skipping only whitespace after `separator`.
    // The delimiter either precedes this operand, or is its first nonblank
    // glyph. Cache that first nonblank offset once per operand: repeatedly
    // rescanning a long blank prefix at each comma would be quadratic.
    operand.start > separator
        || leading_ends
            .get(*cursor)
            .is_some_and(|&leading_end| separator == leading_end)
}

/// A punctuation-delimited spelling is not a complete new declaration when
/// its apparent name is immediately followed by another bare parameter
/// fragment: `first, --fake,last` is still one argument. Looking ahead only
/// to that next token keeps the cumulative scan linear. A single-text head
/// ending in `first, --fake` remains inherently ambiguous; the established
/// comma-plus-space convention still treats it as a declaration.
fn complete_candidate(form: &str, name_end: usize) -> bool {
    let Some(suffix) = form.get(name_end..) else {
        return false;
    };
    let Some(punctuation @ (',' | '|')) = suffix.chars().next() else {
        return true;
    };
    let next = name_end + punctuation.len_utf8();
    form[next..].trim_start().is_empty() || declaration_name_end(form, next).is_some()
}

/// In an already active ordinary parameter, comma-plus-space alone cannot
/// prove two adjacent option-looking fragments inside the same native
/// operand. A terminal candidate remains the established textual convention;
/// a separate operand can independently prove a nonterminal declaration.
fn terminal_parameter_candidate(form: &str, name_end: usize) -> bool {
    let Some(suffix) = form.get(name_end..) else {
        return false;
    };
    match suffix.chars().next() {
        Some(punctuation @ (',' | '|')) => form[name_end + punctuation.len_utf8()..]
            .trim_start()
            .is_empty(),
        _ => true,
    }
}

/// A complete italic metavariable is a bounded argument, not an option name.
/// A leading `-<name>` is also a provisional option template: it can license
/// the next declaration boundary without itself becoming a concrete name.
fn complete_italic_metavariable(value: &str, leading_option_pattern: bool) -> bool {
    // man pages also write complete metavariables as a single italic
    // all-caps word: `-g GLOB, --glob=GLOB` in rg(1). The comma after the
    // final italic glyph can delimit a new declaration; punctuation inside
    // the italic span cannot. A lower-case prose argument is not equivalent
    // evidence, and a leading dash is an option spelling rather than a
    // metavariable.
    if !leading_option_pattern
        && value.starts_with(|character: char| character.is_ascii_uppercase())
        && value.chars().all(|character| {
            character.is_ascii_uppercase()
                || character.is_ascii_digit()
                || matches!(character, '_' | '-')
        })
    {
        return true;
    }
    let inner = if leading_option_pattern {
        value.strip_prefix("-<")
    } else {
        value.strip_prefix('<')
    }
    .and_then(|value| value.strip_suffix('>'));
    inner.is_some_and(|inner| {
        !inner.is_empty()
            && inner.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
            })
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "keep the bounded, single-pass declaration state transitions together"
)]
fn declaration_scan(
    form: &str,
    native_operands: &[Range<usize>],
    argument_starts: &[usize],
    bold_underline_starts: &[usize],
    numeric_name_starts: &[usize],
    plain_italic_ranges: &[Range<usize>],
    boundary_rule: StyledBoundaryRule,
) -> DeclarationScan {
    let mut scan = DeclarationScan {
        ranges: Vec::new(),
        name_prefixes: Vec::new(),
        numeric_name_starts: numeric_name_starts.to_vec(),
    };
    let mut start = 0;
    let mut prefix_end = None;
    let mut phase = Phase::Name;
    let mut name_end = declaration_name_end_with_numeric(form, start, numeric_name_starts);
    let mut after_name_space = false;
    let mut closers = Vec::new();
    let mut quote = None;
    let mut uncertain = false;
    let operand_leading_ends = native_operands
        .iter()
        .map(|range| {
            form.get(range.clone()).map_or(range.start, |visible| {
                range.start + visible.len() - visible.trim_start().len()
            })
        })
        .collect::<Vec<_>>();
    let head_visible_start = form.len() - form.trim_start().len();
    let italic_templates = plain_italic_ranges
        .iter()
        .filter_map(|range| {
            let visible = form.get(range.clone())?;
            let leading = visible.len() - visible.trim_start().len();
            let start = range.start + leading;
            let spelling = visible.trim();
            (complete_italic_metavariable(spelling, false)
                || start == head_visible_start && complete_italic_metavariable(spelling, true))
            .then_some((start, range.end))
        })
        .collect::<Vec<_>>();
    let mut proved_bi_start = (name_end.is_some()
        && native_operands
            .iter()
            .zip(&operand_leading_ends)
            .any(|(range, &leading)| leading == head_visible_start && leading < range.end))
    .then_some(head_visible_start);
    let mut operand = 0usize;
    let mut argument = 0usize;
    let mut italic_template = 0usize;
    let mut active_template_end = None;
    let mut candidate_italic = 0usize;
    let mut bold_underline = 0usize;
    for (offset, character) in form.char_indices() {
        while argument_starts
            .get(argument)
            .is_some_and(|&evidence| evidence < offset)
        {
            argument += 1;
        }
        while bold_underline_starts
            .get(bold_underline)
            .is_some_and(|&evidence| evidence < offset)
        {
            bold_underline += 1;
        }
        let plain_argument = argument_starts.get(argument) == Some(&offset);
        let bold_underlined_argument = bold_underline_starts.get(bold_underline) == Some(&offset)
            && (proved_bi_start != Some(offset)
                || quote.is_some()
                || !closers.is_empty()
                || uncertain);
        if plain_argument || bold_underlined_argument {
            if plain_argument {
                while italic_templates
                    .get(italic_template)
                    .is_some_and(|(start, _)| *start < offset)
                {
                    italic_template += 1;
                }
                active_template_end = italic_templates
                    .get(italic_template)
                    .and_then(|(start, end)| (*start == offset).then_some(*end));
            } else {
                active_template_end = None;
            }
            // A nonempty final underlined operand is already an argument.
            // Without this transition, a glued `-Lfirst` looks like one
            // option token and its following comma could manufacture names
            // before the style check has a chance to reject them.
            // An opening enclosure can already have moved us to Argument
            // without ending the eligible name prefix. The first executed
            // styled parameter still closes that prefix, including before
            // a slash-adjacent operand such as `{-n/` + italic `-NUM`.
            prefix_end.get_or_insert(offset);
            phase = Phase::StyledArgument;
            name_end = None;
            after_name_space = false;
        }
        // A complete name followed by a separated non-option begins an
        // ordinary parameter. An alternating-font operand cannot reset this
        // state by itself; a closed parameter plus delimiter and a distinct
        // native operand may restart it. Single-text heads keep their
        // established comma-and-space inference.
        if phase == Phase::Name && name_end.is_some_and(|end| offset >= end) {
            if character.is_whitespace() {
                after_name_space = true;
            } else if after_name_space {
                if declaration_name_end(form, offset).is_none()
                    && !is_short_long_connector(form, start, offset)
                {
                    prefix_end.get_or_insert(offset);
                    phase = Phase::Argument;
                }
                after_name_space = false;
            }
        }
        // An independent native/styled declaration can restart only after
        // the complete authored quote/bracket scope has ended. Alternating
        // font operands are not parameter boundaries in pinned CVS.
        let mut proved_start = None;
        if matches!(character, ',' | '|') {
            let next_offset = offset + character.len_utf8();
            let remainder = &form[next_offset..];
            let candidate_start = next_offset + remainder.len() - remainder.trim_start().len();
            let candidate_name_end =
                declaration_name_end_with_numeric(form, next_offset, numeric_name_starts);
            let state_closed = quote.is_none() && closers.is_empty() && !uncertain;
            if matches!(boundary_rule, StyledBoundaryRule::NativeComponents)
                && state_closed
                && candidate_name_end.is_some()
                && independent_operand_after_separator(
                    native_operands,
                    &operand_leading_ends,
                    &mut operand,
                    offset,
                    candidate_start,
                )
            {
                proved_start = candidate_name_end.map(|end| (candidate_start, end));
            }
        }
        if let Some((candidate_start, next_name_end)) = proved_start {
            scan.push(start, offset, prefix_end);
            start = offset + character.len_utf8();
            prefix_end = None;
            phase = Phase::Name;
            name_end = Some(next_name_end);
            proved_bi_start = Some(candidate_start);
            after_name_space = false;
            continue;
        }
        if let Some(close) = quote {
            if character == close {
                quote = None;
            }
            continue;
        }
        let previous = form[..offset].chars().next_back();
        if let Some(close) = match character {
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
        } {
            quote = Some(close);
            // An opening display quote can wrap the declaration itself,
            // e.g. `“--foo”`.  `declaration_name_end` has already selected
            // that visible spelling.  A quote after the name instead opens
            // an argument and must keep its punctuation opaque across
            // native font operands (pre_alternate/term_word in pinned CVS).
            let wraps_name = phase == Phase::Name && name_end.is_some_and(|end| end > offset);
            if phase != Phase::StyledArgument && !wraps_name {
                if phase == Phase::Name {
                    prefix_end.get_or_insert(offset);
                }
                phase = Phase::Argument;
            }
            continue;
        }
        if matches!(character, ',' | '|') && !uncertain && closers.is_empty() {
            // Looking past a separator may scan whitespace. Do this only at
            // a separator, never for every scalar in a long literal head.
            let next_offset = offset + character.len_utf8();
            let remainder = &form[next_offset..];
            let candidate_start = next_offset + remainder.len() - remainder.trim_start().len();
            let next_name_end = declaration_name_end(form, next_offset)
                .filter(|&name_end| complete_candidate(form, name_end));
            let fresh_option = remainder.starts_with(char::is_whitespace)
                && next_name_end.is_some_and(|name_end| {
                    !matches!(boundary_rule, StyledBoundaryRule::NativeComponents)
                        || phase != Phase::Argument
                        || terminal_parameter_candidate(form, name_end)
                });
            while plain_italic_ranges
                .get(candidate_italic)
                .is_some_and(|range| range.end <= candidate_start)
            {
                candidate_italic += 1;
            }
            let candidate_still_italic = plain_italic_ranges
                .get(candidate_italic)
                .is_some_and(|range| range.start <= candidate_start && candidate_start < range.end);
            let completed_template = phase == Phase::StyledArgument
                && active_template_end.is_some_and(|end| {
                    end <= offset
                        && form
                            .get(end..offset)
                            .is_some_and(|gap| gap.chars().all(char::is_whitespace))
                })
                && !candidate_still_italic;
            // A styled argument stays opaque even if its internal punctuation
            // is followed by whitespace and a bold run. A plain text head
            // still permits comma+space declaration syntax before entering a
            // styled parameter; only native components can restart afterward.
            if next_name_end.is_some()
                && (phase == Phase::Name && name_end.is_some()
                    || phase != Phase::StyledArgument && fresh_option
                    || completed_template && fresh_option)
            {
                scan.push(start, offset, prefix_end);
                start = next_offset;
                prefix_end = None;
                phase = if next_name_end.is_some() {
                    Phase::Name
                } else {
                    Phase::Argument
                };
                name_end = next_name_end;
                proved_bi_start = None;
                active_template_end = None;
                after_name_space = false;
                continue;
            }
            if next_name_end.is_none()
                && phase == Phase::Name
                && name_end.is_some_and(|end| end <= offset)
            {
                // `.B "-a, text"` still proves its leading `-a`. The comma
                // does not create another declaration, but leaving it in the
                // name prefix would make the already complete name fail the
                // spelling check.
                prefix_end.get_or_insert(offset);
                phase = Phase::Argument;
            }
        }
        match character {
            '=' if phase != Phase::StyledArgument => {
                if phase == Phase::Name {
                    prefix_end.get_or_insert(offset);
                }
                phase = Phase::Argument;
            }
            '[' | '{' | '(' | '<' => {
                if phase != Phase::StyledArgument {
                    // An opening enclosure before a visible option (for
                    // example `[-n/--number]`) belongs to the declaration
                    // prefix. A bracket after the name begins its value.
                    if phase == Phase::Name && name_end.is_some_and(|end| offset >= end) {
                        prefix_end.get_or_insert(offset);
                    }
                    phase = Phase::Argument;
                }
                if closers.len() == 64 {
                    uncertain = true;
                } else {
                    closers.push(match character {
                        '[' => ']',
                        '{' => '}',
                        '(' => ')',
                        '<' => '>',
                        _ => unreachable!(),
                    });
                }
            }
            ']' | '}' | ')' | '>' => {
                if closers.last() == Some(&character) {
                    closers.pop();
                } else {
                    uncertain = true;
                }
            }
            _ => {}
        }
    }
    scan.push(start, form.len(), prefix_end);
    scan
}

/// End of the leading option spelling, before any attached value or visible
/// declaration separator. This is a syntax boundary, not a source coordinate
/// or proof that the native owner represents an option.
fn declaration_name_end(form: &str, start: usize) -> Option<usize> {
    declaration_name_end_with_numeric(form, start, &[])
}

fn declaration_name_end_with_numeric(
    form: &str,
    start: usize,
    numeric_name_starts: &[usize],
) -> Option<usize> {
    let remainder = form.get(start..)?;
    let leading = remainder.len() - remainder.trim_start().len();
    let head = remainder.trim_start();
    let token_end = head
        .find(|character: char| character.is_whitespace() || matches!(character, ',' | '|'))
        .unwrap_or(head.len());
    let token = &head[..token_end];
    if pattern_start(token) {
        // A pattern proves a provisional declaration boundary, not a name.
        // Subsequent ordinary operands must still enter Argument phase so a
        // comma inside one cannot restart at a fake option.
        return Some(start + leading + token.len());
    }
    leading_name_with_numeric(token, start + leading, numeric_name_starts)
        .map(|(_, range)| range.end)
}

/// The shared alias grammar admits exactly `-q or --quiet`; `or` is not an
/// ordinary argument when it bridges one short option and one long option.
/// Look ahead only at this one candidate connector, never at every glyph.
fn is_short_long_connector(form: &str, segment_start: usize, offset: usize) -> bool {
    let Some(before) = form.get(segment_start..offset) else {
        return false;
    };
    let first = before.trim();
    if first.len() != 2 || !first.starts_with('-') || !lexical_option_token(first) {
        return false;
    }
    let Some(after_or) = form
        .get(offset..)
        .and_then(|value| value.strip_prefix("or"))
    else {
        return false;
    };
    let skipped = after_or.len() - after_or.trim_start().len();
    if skipped == 0 {
        return false;
    }
    let next = offset + 2 + skipped;
    declaration_name_end(form, next)
        .and_then(|end| form.get(next..end))
        .is_some_and(|candidate| candidate.starts_with("--"))
}

/// Require an inferred PP/RS head to be complete declaration syntax, not
/// merely to start with an option-looking word. The native continuation
/// proves presentation ownership; this source-neutral rule prevents ordinary
/// prose in that same paragraph from becoming an entry. It follows the
/// bounded bare-argument rule of Flow's `is_option_head` without treating
/// visual adjacency as an alias or reconstructing roff markup.
#[must_use]
pub fn is_complete_hanging_option_head(form: &str) -> bool {
    is_complete_hanging_option_head_with_provisional(form, |_, _| false)
}

/// A one-glyph short spelling can be a provisional, non-name prefix when
/// native final-display evidence independently proves it bold. Keep that
/// evidence outside the source-neutral public spelling rule: only the checked
/// later long name is published, never the provisional spelling itself.
pub(crate) fn is_complete_hanging_option_head_with_provisional(
    form: &str,
    bold_names: impl FnOnce(Range<usize>, &[Range<usize>]) -> bool,
) -> bool {
    let names = literal_option_names(form);
    let Some((first_name, first)) = names.first() else {
        return false;
    };
    let leading = &form[..first.start];
    let neutral_leading = leading
        .chars()
        .all(|character| character.is_whitespace() || matches!(character, '[' | '{' | '('));
    let provisional_leading = leading
        .trim()
        .strip_suffix([',', '|'])
        .is_some_and(|pattern| complete_italic_metavariable(pattern.trim_end(), true));
    let native_short_leading = first_name.starts_with("--")
        && hanging_short_prefix(leading).is_some_and(|prefix| {
            // One native short prefix cannot promote a later roman argument
            // or a second unstyled token into a name. The caller checks
            // these exact ranges against final native glyph styling.
            let ranges = names
                .iter()
                .map(|(_, range)| range.clone())
                .collect::<Vec<_>>();
            bold_names(prefix, &ranges)
        });
    if !neutral_leading && !provisional_leading && !native_short_leading {
        return false;
    }
    for pair in names.windows(2) {
        let [(previous, previous_range), (next, next_range)] = pair else {
            unreachable!()
        };
        let Some(gap) = form.get(previous_range.end..next_range.start) else {
            return false;
        };
        let gap = gap.trim();
        if gap.is_empty() || gap == "or" {
            if previous.len() != 2 || !previous.starts_with('-') || !next.starts_with("--") {
                return false;
            }
        } else if let Some(prefix) = gap.strip_suffix([',', '|', '/']) {
            if !hanging_argument_tail(prefix) {
                return false;
            }
        } else {
            return false;
        }
    }
    names
        .last()
        .and_then(|(_, range)| form.get(range.end..))
        .is_some_and(hanging_argument_tail)
}

/// Exact `-<one glyph>, ` prefix of a complete hanging declaration. A longer
/// negative argument, an unseparated comma or quoted punctuation is not a
/// provisional short option; the caller must still prove final bold glyphs.
fn hanging_short_prefix(leading: &str) -> Option<Range<usize>> {
    let start = leading.len() - leading.trim_start().len();
    let rest = leading.get(start..)?.strip_prefix('-')?;
    let glyph = rest.chars().next()?;
    if !glyph.is_ascii_graphic()
        || matches!(
            glyph,
            '-' | ','
                | '|'
                | '/'
                | '='
                | '"'
                | '\''
                | '<'
                | '>'
                | '['
                | ']'
                | '{'
                | '}'
                | '('
                | ')'
        )
    {
        return None;
    }
    let end = start + 1 + glyph.len_utf8();
    let suffix = leading.get(end..)?.strip_prefix(',')?;
    (!suffix.is_empty() && suffix.chars().all(char::is_whitespace)).then_some(start..end)
}

fn hanging_argument_tail(value: &str) -> bool {
    let mut tail = value.trim();
    if !tail.is_empty()
        && tail
            .chars()
            .all(|character| matches!(character, ']' | '}' | ')'))
    {
        return true;
    }
    if tail.starts_with([',', '|', '/']) {
        return false;
    }
    if let Some(attached) = tail.strip_prefix('=') {
        let Some(token) = attached.split_whitespace().next() else {
            return false;
        };
        if token.is_empty() || token.chars().any(char::is_control) {
            return false;
        }
        tail = attached[token.len()..].trim_start();
    }
    let mut bare = 0usize;
    let mut closers = Vec::new();
    for token in tail.split_whitespace() {
        let inside = !closers.is_empty();
        for character in token.chars() {
            if let Some(closer) = match character {
                '[' => Some(']'),
                '{' => Some('}'),
                '<' => Some('>'),
                '(' => Some(')'),
                _ => None,
            } {
                if closers.len() == 64 {
                    return false;
                }
                closers.push(closer);
            } else if matches!(character, ']' | '}' | '>' | ')') && closers.pop() != Some(character)
            {
                return false;
            }
        }
        if inside || token.starts_with(['[', '{', '<', '(']) || token == "..." {
            continue;
        }
        if token.starts_with('/')
            || token
                .chars()
                .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-'))
                && !token.starts_with('-')
        {
            bare += 1;
        } else {
            return false;
        }
    }
    closers.is_empty() && bare <= 1
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Name,
    Argument,
    StyledArgument,
}

fn leading_name(group: &str, offset: usize) -> Option<(String, Range<usize>)> {
    leading_name_with_numeric(group, offset, &[])
}

fn leading_name_with_numeric(
    group: &str,
    offset: usize,
    numeric_name_starts: &[usize],
) -> Option<(String, Range<usize>)> {
    let token = group.split_whitespace().next()?;
    let token = token.trim_matches(['[', ']', '(', ')', '{', '}', '“', '”', '‘', '’']);
    let start = offset + token.as_ptr() as usize - group.as_ptr() as usize;
    let name = option_prefix(token)?;
    let proved_numeric_short = name.len() == 2
        && name.as_bytes()[0] == b'-'
        && name.as_bytes()[1].is_ascii_digit()
        && numeric_name_starts.binary_search(&start).is_ok();
    if !lexical_option_token(name) && !proved_numeric_short {
        return None;
    }
    let suffix = &token[name.len()..];
    if !suffix.is_empty() && !suffix.starts_with(['=', '[', '{', '<', '(', '/']) {
        return None;
    }
    Some((name.to_owned(), start..start + name.len()))
}

fn slash_names(group: &str, offset: usize) -> Result<Option<Vec<NameRange>>, ()> {
    let Some(token) = group.split_whitespace().next() else {
        return Ok(None);
    };
    let token = token.trim_matches(['[', ']', '(', ')', '{', '}', '“', '”', '‘', '’']);
    if !token.contains('/') {
        return Ok(None);
    }
    // Keep the temporary split bounded by the same 64-name ceiling as the
    // final result. An overlong candidate cannot become a checked name.
    let parts = token.split('/').take(66).collect::<Vec<_>>();
    if parts.len() > 65 {
        return Err(());
    }
    if parts.len() < 2
        || !parts[..parts.len() - 1]
            .iter()
            .all(|part| lexical_option_token(part))
    {
        return Ok(None);
    }
    let mut result = Vec::with_capacity(parts.len());
    let mut position = offset + token.as_ptr() as usize - group.as_ptr() as usize;
    for (index, part) in parts.iter().enumerate() {
        let Some(name) = option_prefix(part) else {
            return Ok(None);
        };
        if !lexical_option_token(name)
            || index + 1 != parts.len() && name != *part
            || index + 1 == parts.len() && name != *part && !part[name.len()..].starts_with('=')
        {
            return Ok(None);
        }
        result.push((name.to_owned(), position..position + name.len()));
        position += part.len() + 1;
    }
    if result.len() > 64 {
        return Err(());
    }
    Ok(Some(result))
}

fn pattern_names(group: &str, offset: usize) -> Result<Option<Vec<NameRange>>, ()> {
    let mut tokens = group.split_whitespace();
    if !tokens.next().is_some_and(pattern_start) {
        return Ok(None);
    }
    let mut names = Vec::new();
    for token in tokens {
        let start = offset + token.as_ptr() as usize - group.as_ptr() as usize;
        let Some((name, range)) = leading_name(token, start) else {
            break;
        };
        if !name.starts_with("--") {
            break;
        }
        if names.len() == 64 {
            // A provisional pattern may precede many complete long names.
            // The 65th valid spelling invalidates the whole head, whereas a
            // following ordinary parameter leaves the first 64 intact.
            return Err(());
        }
        let attached = range.end < start + token.len();
        names.push((name, range));
        // An assignment or bracketed value ends the provisional name group.
        // Later option-looking words need their own proved declaration edge.
        if attached {
            break;
        }
    }
    Ok((!names.is_empty()).then_some(names))
}

fn pattern_start(token: &str) -> bool {
    token.starts_with('-')
        && token.contains('#')
        && token
            .chars()
            .all(|character| matches!(character, '-' | '#'))
}

#[cfg(test)]
#[expect(
    clippy::single_range_in_vec_init,
    reason = "single Range values are explicit byte-interval evidence, not iterators"
)]
mod tests;
