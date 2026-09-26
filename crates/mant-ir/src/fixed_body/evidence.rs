//! Read-time evidence closure for native owner heads and text selections.
//!
//! This module validates surviving glyph ranges, declaration syntax and
//! bindings against one Fixed surface. It neither formats roff nor owns the
//! native collector state; every proof is recomputed for mutable documents.

use super::{
    FixedBody, OutputSlice, OwnerHeadComponent, OwnerHeadRole, OwnerMark, OwnerRole, RegionKind,
    TextJoin, TextSelection, validation,
};
use crate::{EntryFacts, EntryKind, EntryNameEvidence, NameCase, ParameterKind};
use std::{collections::BTreeSet, num::NonZeroU32, ops::Range};

/// Map each native component to a contiguous HEAD part interval. A component
/// may leave unrelated HEAD glyphs outside its interval, but cannot skip a
/// part or substitute a different join inside its own visible spelling.
pub(super) fn component_part_ranges(
    head: &TextSelection,
    components: &[OwnerHeadComponent],
) -> Option<Vec<Range<usize>>> {
    let mut ranges = Vec::with_capacity(components.len());
    let mut cursor = 0;
    for component in components {
        let first = component.selection.parts.first()?;
        while head.parts.get(cursor).is_some_and(|part| part != first) {
            cursor += 1;
        }
        let end = cursor.checked_add(component.selection.parts.len())?;
        if head.parts.get(cursor..end)? != component.selection.parts
            || head.joins.get(cursor..end.saturating_sub(1))? != component.selection.joins
        {
            return None;
        }
        ranges.push(cursor..end);
        cursor = end;
    }
    Some(ranges)
}

fn group_name_occurrences(
    found: impl IntoIterator<Item = (String, TextSelection)>,
) -> Vec<(String, Vec<TextSelection>)> {
    let mut indices: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut grouped: Vec<(String, Vec<TextSelection>)> = Vec::new();
    for (name, selection) in found {
        if let Some(&index) = indices.get(&name) {
            grouped[index].1.push(selection);
        } else {
            indices.insert(name.clone(), grouped.len());
            grouped.push((name, vec![selection]));
        }
    }
    grouped
}

impl FixedBody {
    /// Check disjoint logical name ranges against final bold runs in one
    /// forward pass. A consumed separator has no glyph style and cannot be
    /// used as a name binding; no complete HEAD copy is made per name.
    pub(super) fn selection_ranges_bold(
        &self,
        selection: &TextSelection,
        form: &str,
        prefix: Range<usize>,
        names: &[Range<usize>],
    ) -> bool {
        if selection.joins.len() != selection.parts.len().saturating_sub(1) {
            return false;
        }
        let mut ranges = Vec::with_capacity(names.len() + 1);
        ranges.push(prefix);
        ranges.extend_from_slice(names);
        let mut previous_end = 0;
        for range in &ranges {
            if range.start < previous_end
                || range.start >= range.end
                || form.get(range.clone()).is_none()
            {
                return false;
            }
            previous_end = range.end;
        }
        let mut cursor = 0usize;
        let mut target = 0usize;
        let mut covered = ranges[0].start;
        for (index, part) in selection.parts.iter().enumerate() {
            if index != 0 {
                let separator = match &selection.joins[index - 1] {
                    TextJoin::DirectContact => None,
                    TextJoin::AuthoredSeparator(text) | TextJoin::GeneratedSeparator(text) => {
                        Some(text)
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return false,
                };
                if let Some(separator) = separator {
                    let Some(end) = cursor.checked_add(separator.len()) else {
                        return false;
                    };
                    if ranges
                        .get(target)
                        .is_some_and(|range| range.start < end && cursor < range.end)
                    {
                        return false;
                    }
                    cursor = end;
                }
            }
            let Some(start_byte) = usize::try_from(part.start_byte).ok() else {
                return false;
            };
            let Some(end_byte) = usize::try_from(part.end_byte).ok() else {
                return false;
            };
            let Some(run) = self.surface.runs.get((part.run.get() - 1) as usize) else {
                return false;
            };
            let Some(visible) = self
                .surface
                .run_text(part.run)
                .and_then(|text| text.get(start_byte..end_byte))
            else {
                return false;
            };
            let Some(end) = cursor.checked_add(visible.len()) else {
                return false;
            };
            while let Some(range) = ranges.get(target)
                && range.start < end
            {
                if range.end <= cursor {
                    return false;
                }
                let begin = range.start.max(cursor);
                let stop = range.end.min(end);
                if begin != covered || !run.label.style.bold {
                    return false;
                }
                covered = stop;
                if covered == range.end {
                    target += 1;
                    if let Some(next) = ranges.get(target) {
                        covered = next.start;
                    }
                } else {
                    break;
                }
            }
            cursor = end;
        }
        target == ranges.len() && cursor == form.len()
    }

    /// Check one PP/RS declaration against its direct native continuation,
    /// without scanning unrelated owners or regions. The paragraph head must
    /// be complete syntax; a textless RS qualifies only with a checked nested
    /// definition head, not merely a table or empty layout scope.
    #[must_use]
    pub fn hanging_declaration_ready(&self, owner: &OwnerMark) -> bool {
        let Some(region) = owner
            .hanging_continuation
            .and_then(|key| self.regions.get((key.get() - 1) as usize))
        else {
            return false;
        };
        let nested_head_ready = owner
            .hanging_nested_head
            .and_then(|key| self.regions.get((key.get() - 1) as usize))
            .is_some_and(|head| {
                head.kind == RegionKind::OwnerHead
                    && head.parent == Some(region.key)
                    && head.section == owner.section
                    && head.owner.is_some_and(|nested| {
                        nested != owner.key
                            && self
                                .owners
                                .get((nested.get() - 1) as usize)
                                .is_some_and(|child| {
                                    child.section == owner.section
                                        && child.parent == owner.parent
                                        && child.role == OwnerRole::Definition
                                        && child.head == head.selection
                                })
                    })
            });
        owner.hanging_candidate
            && owner.role == OwnerRole::Definition
            && owner.head_role == Some(OwnerHeadRole::Lexical)
            && region.kind == RegionKind::HangingContinuation
            && region.continuation_of == Some(owner.key)
            && region.owner.is_none()
            && region.section == owner.section
            && (owner.hanging_nested_head.is_none() || nested_head_ready)
            && (!region.selection.parts.is_empty() || nested_head_ready)
            && self.owner_complete_form(owner).is_some_and(|form| {
                crate::entry::is_complete_hanging_option_head_with_provisional(
                    &form,
                    |prefix, names| self.selection_ranges_bold(&owner.head, &form, prefix, names),
                )
            })
    }

    /// Select names from one complete native lexical head. Source-neutral
    /// spellings and native argument style pass the same checked intervals.
    pub(super) fn lexical_literal_names(
        &self,
        owner: &OwnerMark,
    ) -> Option<Vec<(String, TextSelection, std::ops::Range<usize>)>> {
        if owner.head_role != Some(OwnerHeadRole::Lexical) {
            return None;
        }
        let form = self.owner_complete_form(owner)?;
        let scan = self.lexical_declaration_scan(owner, &form)?;
        let segments = &scan.ranges;
        let (literal, over_limit) = scan.names(&form);
        if over_limit {
            return None;
        }
        // With no candidate there is no name-to-glyph mapping to perform.
        // checked_lexical_names also accepts this empty, non-rejected case.
        if literal.is_empty() {
            return Some(Vec::new());
        }
        let ranges = literal
            .iter()
            .map(|(_, range)| range.clone())
            .collect::<Vec<_>>();
        let selections = self.selection_subranges_from_form(&owner.head, &form, &ranges)?;
        let mut names = Vec::with_capacity(literal.len());
        for ((name, range), selection) in literal.into_iter().zip(selections) {
            if form.get(range.clone()) != Some(name.as_str()) {
                return None;
            }
            names.push((name, selection, range));
        }
        let names = self.checked_lexical_names(owner, names, segments)?;
        // The parser-alive prefix is a candidate for the first declaration,
        // not permission to ignore the rest of the native HEAD.  Keep it
        // tied to the same final glyphs when it was recorded.
        if !names.is_empty()
            && let Some(prefix) = owner.head_role_prefix.as_deref()
            && names.first().map(|(name, _, _)| name.as_str()) != Some(prefix)
        {
            return None;
        }
        Some(names)
    }

    /// One checked result for producer, validator, and query positions.
    /// Native operand components describe where glyphs came from; a prefix
    /// inside one component is not an independent name alongside the complete
    /// displayed HEAD (`man_term.c::pre_alternate` joins its operands). A
    /// nonempty result contains exact names; an empty result proves a checked
    /// non-option head; `None` denotes incomplete or contradictory evidence.
    #[must_use]
    pub fn lexical_names(
        &self,
        owner: &OwnerMark,
    ) -> Option<Vec<(String, TextSelection, std::ops::Range<usize>)>> {
        self.lexical_literal_names(owner)
    }

    /// A bold native operand begins another declaration only at its own
    /// final visible glyphs. In particular, punctuation inside the preceding
    /// italic parameter cannot manufacture such an operand. This is a
    /// bounded annotation of the complete displayed HEAD, not a second
    /// source-spelling parser or a requirement for authored coordinates.
    #[expect(
        clippy::too_many_lines,
        reason = "collect one owner's native component and final-style evidence in display order"
    )]
    pub(super) fn lexical_declaration_scan(
        &self,
        owner: &OwnerMark,
        form: &str,
    ) -> Option<crate::entry::DeclarationScan> {
        let component_ranges = component_part_ranges(&owner.head, &owner.head_components)
            .and_then(|parts| self.component_byte_ranges(&owner.head, &parts))
            .map(|ranges| owner.head_components.iter().zip(ranges).collect::<Vec<_>>())
            .unwrap_or_default();
        // Only native macro components are independent operands. A lone `.IP`
        // label is one text operand in man_term.c::pre_IP; `\fB` within it
        // may change the final run style without ending a parameter.
        let operands = component_ranges
            .iter()
            .filter_map(|(component, range)| {
                (component.role == OwnerHeadRole::Lexical && range.start < range.end)
                    .then_some(range.clone())
            })
            .collect::<Vec<_>>();
        // Build final-bold glyph intervals once in logical HEAD coordinates.
        // Calling selection_subrange for each numeric-looking native child
        // would repeatedly rebuild and scan the whole HEAD (quadratic for a
        // long alternating macro). Joins occupy logical bytes but no glyphs,
        // so a name crossing one cannot accidentally inherit a font style.
        if owner.head.joins.len() != owner.head.parts.len().saturating_sub(1) {
            return None;
        }
        let mut bold_spans: Vec<Range<usize>> = Vec::new();
        let mut glyph_cursor = 0usize;
        for (index, part) in owner.head.parts.iter().enumerate() {
            if index != 0 {
                glyph_cursor = glyph_cursor.checked_add(match &owner.head.joins[index - 1] {
                    TextJoin::DirectContact => 0,
                    TextJoin::AuthoredSeparator(text) | TextJoin::GeneratedSeparator(text) => {
                        text.len()
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                })?;
            }
            let start = glyph_cursor;
            let run = self.surface.runs.get((part.run.get() - 1) as usize)?;
            let visible = self.surface.run_text(part.run)?.get(
                usize::try_from(part.start_byte).ok()?..usize::try_from(part.end_byte).ok()?,
            )?;
            glyph_cursor = glyph_cursor.checked_add(visible.len())?;
            if run.label.style.bold {
                if let Some(last) = bold_spans.last_mut()
                    && last.end == start
                {
                    last.end = glyph_cursor;
                } else {
                    bold_spans.push(start..glyph_cursor);
                }
            }
        }
        if glyph_cursor != form.len() {
            return None;
        }
        // A signed number in an existing argument is not an option. The
        // exception for a short numeric flag requires this native lexical
        // component's first surviving glyphs, their authored source identity,
        // and the final bold display of both glyphs. pre_alternate() supplies
        // the operand boundary; term_word() supplies the executed glyph/style.
        let mut bold_cursor = 0usize;
        let numeric_name_starts = component_ranges
            .iter()
            .filter_map(|(component, range)| {
                if component.role != OwnerHeadRole::Lexical || !component.has_source_identity() {
                    return None;
                }
                let visible = form.get(range.clone())?;
                let start = range.start + visible.len() - visible.trim_start().len();
                let end = start.checked_add(2)?;
                let name = form.get(start..end)?;
                let bytes = name.as_bytes();
                if end > range.end
                    || bytes.len() != 2
                    || bytes[0] != b'-'
                    || !bytes[1].is_ascii_digit()
                {
                    return None;
                }
                while bold_spans
                    .get(bold_cursor)
                    .is_some_and(|span| span.end <= start)
                {
                    bold_cursor += 1;
                }
                bold_spans
                    .get(bold_cursor)
                    .is_some_and(|span| span.start <= start && end <= span.end)
                    .then_some(start)
            })
            .collect::<Vec<_>>();
        // Component intervals and display parts are both in visible order.
        // Walk their starts once; a tree lookup for every font fragment would
        // make a long alternating HEAD needlessly superlinear.
        let mut operand_boundary = 0;
        let mut plain_italic_ranges: Vec<Range<usize>> = Vec::new();
        let mut bold_underline_starts = Vec::new();
        let mut offset = 0usize;
        let mut previous_style = None;
        let mut underlined_group_started = false;
        for (index, part) in owner.head.parts.iter().enumerate() {
            if index != 0 {
                offset = offset.checked_add(match &owner.head.joins[index - 1] {
                    TextJoin::DirectContact => 0,
                    TextJoin::AuthoredSeparator(text) | TextJoin::GeneratedSeparator(text) => {
                        text.len()
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                })?;
            }
            let start = offset;
            let run = self.surface.runs.get((part.run.get() - 1) as usize)?;
            let visible = self.surface.run_text(part.run)?.get(
                usize::try_from(part.start_byte).ok()?..usize::try_from(part.end_byte).ok()?,
            )?;
            offset = offset.checked_add(visible.len())?;
            let style = (run.label.style.bold, run.label.style.underline);
            while operands
                .get(operand_boundary)
                .is_some_and(|operand| operand.start < start)
            {
                operand_boundary += 1;
            }
            let starts_operand = operands
                .get(operand_boundary)
                .is_some_and(|operand| operand.start == start);
            let same_style_group = index != 0
                && owner.head.joins[index - 1] == TextJoin::DirectContact
                && !starts_operand
                && previous_style == Some(style);
            if !same_style_group {
                underlined_group_started = false;
            }
            if run.label.style.underline {
                if underlined_group_started && same_style_group && !run.label.style.bold {
                    plain_italic_ranges.last_mut()?.end = offset;
                } else if !underlined_group_started
                    && let Some(first) = visible.find(|character: char| !character.is_whitespace())
                {
                    let visible_start = start.checked_add(first)?;
                    if run.label.style.bold {
                        bold_underline_starts.push(visible_start);
                    } else {
                        plain_italic_ranges.push(visible_start..offset);
                    }
                    underlined_group_started = true;
                }
            }
            previous_style = Some(style);
        }
        if offset != form.len() {
            return None;
        }
        if owner.head_components.is_empty() || !operands.is_empty() {
            crate::scan_option_declarations_with_style_ranges(
                form,
                &operands,
                &plain_italic_ranges,
                &bold_underline_starts,
                &numeric_name_starts,
            )
        } else {
            // Preserve the existing conservative rule for a lexical owner
            // whose native components are all non-lexical or textless.
            let mut plain_italic_starts = plain_italic_ranges
                .iter()
                .map(|range| range.start)
                .collect::<Vec<_>>();
            plain_italic_starts.extend(bold_underline_starts);
            plain_italic_starts.sort_unstable();
            plain_italic_starts.dedup();
            Some(crate::entry::literal_declaration_scan_with_operands(
                form,
                &operands,
                &plain_italic_starts,
                crate::entry::StyledBoundaryRule::NativeComponents,
            ))
        }
    }

    fn checked_lexical_names(
        &self,
        owner: &OwnerMark,
        candidates: Vec<(String, TextSelection, std::ops::Range<usize>)>,
        segments: &[Range<usize>],
    ) -> Option<Vec<(String, TextSelection, std::ops::Range<usize>)>> {
        let had_candidates = !candidates.is_empty();
        let mut names = Vec::new();
        let mut blocked_segment = None;
        let mut segment_cursor = 0;
        let mut style_rejected = false;
        for (name, selection, range) in candidates {
            while segments
                .get(segment_cursor)
                .is_some_and(|segment| segment.end < range.start)
            {
                segment_cursor += 1;
            }
            let segment = segments.get(segment_cursor)?;
            if segment.start > range.start || range.end > segment.end {
                return None;
            }
            if blocked_segment == Some(segment_cursor) {
                continue;
            }
            // Underline is final native display evidence, not a recovered
            // italic opcode. Conservatively treat an underlined nonbold name
            // as a parameter within this segment only; a later independently
            // delimited declaration remains eligible.
            if selection.parts.iter().any(|part| {
                self.surface
                    .runs
                    .get((part.run.get() - 1) as usize)
                    .is_some_and(|run| run.label.style.underline && !run.label.style.bold)
            }) {
                style_rejected = true;
                blocked_segment = Some(segment_cursor);
                continue;
            }
            // A plain IP label has no independent lexical component or TP/TQ
            // term witness. Bold `-a` glued to roman `foo` is only a styled
            // label, not a proved combined option. In contrast, pre_B() is
            // an explicit macro instance and term_word() may switch to roman
            // inside its one complete spelling without ending that name.
            if !owner.lexical_term_witness && owner.head_components.is_empty() {
                let mut prior_bold = None;
                let mut mixed_bold = false;
                for part in &selection.parts {
                    let current_bold = self
                        .surface
                        .runs
                        .get((part.run.get() - 1) as usize)?
                        .label
                        .style
                        .bold;
                    if prior_bold
                        .replace(current_bold)
                        .is_some_and(|before| before != current_bold)
                    {
                        mixed_bold = true;
                    }
                }
                if mixed_bold {
                    style_rejected = true;
                    blocked_segment = Some(segment_cursor);
                    continue;
                }
            }
            // A B operand can switch to roman in the middle of one visible
            // spelling: pre_B() chooses only its initial font and term_word()
            // executes \fR inline. The shared scan already found the name
            // interval; a boldness change is not a second parameter grammar.
            names.push((name, selection, range));
        }
        if names.len() > 64 || names.windows(2).any(|pair| pair[0].2.end > pair[1].2.start) {
            return None;
        }
        // An empty result is meaningful only when final italic styling
        // rejected a candidate: the producer must not reclassify that same
        // visible spelling through the generic identity fallback. A head
        // with no option candidate remains eligible as an ordinary Term.
        (!names.is_empty() || style_rejected || !had_candidates).then_some(names)
    }

    /// Bind each source-identified `Fl` macro to its own final glyphs without
    /// requiring those names to cover the entire `HEAD`.
    /// `mdoc_macro.c::blk_full()` keeps `Ar` operands in the same `It` `HEAD`,
    /// and `mdoc_term.c::termp_fl_pre()` prints every distinct `Fl` invocation.
    /// The complete `HEAD` remains one form; these components prove names, not
    /// separate forms or alias relationships.
    /// A sixty-fifth proved invocation invalidates the whole semantic group,
    /// even if its spelling repeats an earlier one.
    #[must_use]
    pub fn option_component_names(
        &self,
        owner: &OwnerMark,
    ) -> Option<Vec<(String, TextSelection, std::ops::Range<usize>)>> {
        if owner.head_role != Some(OwnerHeadRole::Option)
            || owner.head_components.len() < 2
            || self.owner_complete_form(owner).is_none()
        {
            return None;
        }
        let ranges = component_part_ranges(&owner.head, &owner.head_components)?;
        let byte_ranges = self.component_byte_ranges(&owner.head, &ranges)?;
        let mut names = Vec::with_capacity(owner.head_components.len().min(64));
        for (component, range) in owner.head_components.iter().zip(byte_ranges) {
            if component.selection.parts.is_empty() || !component.has_source_identity() {
                return None;
            }
            // A mixed It HEAD can contain Cm/Ic alongside distinct Fl
            // invocations. Only Fl is option-name evidence; a literal that
            // happens to spell like an option must not veto sibling Fl names
            // or itself become one.
            if component.role != OwnerHeadRole::Option {
                continue;
            }
            let text = self.selection_text(&component.selection)?;
            if !crate::native_option_token(&text) {
                // Each Fl is an independent native invocation. In particular,
                // mdoc_term.c::termp_fl_pre() emits a visible dash even when
                // the operand has no glyphs. That instance proves no name,
                // but must not erase names proved by sibling Fl instances.
                continue;
            }
            if names.len() == 64 {
                return None;
            }
            names.push((text, component.selection.clone(), range));
        }
        (!names.is_empty()).then_some(names)
    }

    /// Whether more than 64 independently visible `Fl` invocations prove
    /// option names in one head. The producer checks this before any lexical
    /// or first-component fallback; the IR validator applies the same gate to
    /// externally supplied entry facts. Neither may publish a truncated group.
    #[must_use]
    pub fn option_component_over_limit(&self, owner: &OwnerMark) -> bool {
        if owner.head_role != Some(OwnerHeadRole::Option) {
            return false;
        }
        let mut proved = 0;
        for component in &owner.head_components {
            if component.role != OwnerHeadRole::Option
                || component.selection.parts.is_empty()
                || !component.has_source_identity()
            {
                continue;
            }
            if self
                .selection_text(&component.selection)
                .is_some_and(|text| crate::native_option_token(&text))
            {
                proved += 1;
                if proved > 64 {
                    return true;
                }
            }
        }
        false
    }

    fn component_byte_ranges(
        &self,
        head: &TextSelection,
        part_ranges: &[std::ops::Range<usize>],
    ) -> Option<Vec<std::ops::Range<usize>>> {
        let mut output = Vec::with_capacity(part_ranges.len());
        let mut offset = 0usize;
        let mut component = 0usize;
        let mut start = 0usize;
        for (index, part) in head.parts.iter().enumerate() {
            if index != 0 {
                offset = offset.checked_add(match &head.joins[index - 1] {
                    TextJoin::DirectContact => 0,
                    TextJoin::AuthoredSeparator(text) | TextJoin::GeneratedSeparator(text) => {
                        text.len()
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                })?;
            }
            if part_ranges
                .get(component)
                .is_some_and(|range| range.start == index)
            {
                start = offset;
            }
            let run = self.surface.run_text(part.run)?;
            let text = run.get(
                usize::try_from(part.start_byte).ok()?..usize::try_from(part.end_byte).ok()?,
            )?;
            offset = offset.checked_add(text.len())?;
            if part_ranges
                .get(component)
                .is_some_and(|range| range.end == index + 1)
            {
                output.push(start..offset);
                component += 1;
            }
        }
        (component == part_ranges.len()).then_some(output)
    }

    /// Return multiple exact option declarations only when distinct native
    /// `Fl` macro instances cover every non-separator glyph in one complete
    /// definition HEAD. Typography or punctuation alone never creates names.
    #[must_use]
    pub fn option_component_forms(
        &self,
        owner: &OwnerMark,
    ) -> Option<Vec<(String, TextSelection)>> {
        if owner.role != OwnerRole::Definition || owner.head_components.len() < 2 {
            return None;
        }
        let mut forms = Vec::with_capacity(owner.head_components.len().min(64));
        let mut seen = BTreeSet::new();
        for component in &owner.head_components {
            if component.role != OwnerHeadRole::Option
                || component.selection.parts.is_empty()
                || !component.has_source_identity()
            {
                return None;
            }
            let text = self.selection_text(&component.selection)?;
            if !crate::native_option_token(&text) || !seen.insert(text.clone()) {
                return None;
            }
            if forms.len() == 64 {
                return None;
            }
            forms.push((text, component.selection.clone()));
        }
        let mut selected = owner
            .head_components
            .iter()
            .flat_map(|component| component.selection.parts.iter())
            .peekable();
        for part in &owner.head.parts {
            if selected.peek().is_some_and(|component| *component == part) {
                selected.next();
                continue;
            }
            let run = self.surface.run_text(part.run)?;
            let start = usize::try_from(part.start_byte).ok()?;
            let end = usize::try_from(part.end_byte).ok()?;
            if !run
                .get(start..end)?
                .bytes()
                .all(|byte| byte.is_ascii_whitespace() || matches!(byte, b',' | b'|' | b'/'))
            {
                return None;
            }
        }
        selected.next().is_none().then_some(forms)
    }

    /// Keys whose optional entry facts fail the same read-time proof used by
    /// consumers. Producers can retract only these facts before finalizing a
    /// document, without discarding the native body or valid sibling entries.
    #[must_use]
    pub fn invalid_entry_keys(&self) -> Vec<NonZeroU32> {
        self.owners
            .iter()
            .filter(|owner| owner.entry.is_some() && self.validated_entry(owner).is_none())
            .map(|owner| owner.key)
            .collect()
    }

    /// Native output remains readable when a link target fails semantic
    /// validation; only these occurrences must lose activation.
    #[must_use]
    pub fn invalid_link_target_keys(&self) -> Vec<NonZeroU32> {
        self.links
            .iter()
            .filter(|link| {
                link.target
                    .as_ref()
                    .is_some_and(|target| validation::validate_link_target(target).is_err())
            })
            .map(|link| link.key)
            .collect()
    }

    /// Borrow only the current conservative Fixed facts whose form, name and
    /// lexical binding close against this owner's surviving native head.
    /// Recheck at read time: an in-memory `Document` can be changed after its
    /// deserialization guard ran.
    #[allow(clippy::too_many_lines)] // One read-time closure of all entry fact variants.
    pub(crate) fn validated_entry<'a>(
        &self,
        owner: &'a OwnerMark,
    ) -> Option<&'a EntryFacts<TextSelection>> {
        let entry = owner.entry.as_ref()?;
        if self.option_component_over_limit(owner) {
            return None;
        }
        if owner.hanging_candidate {
            if !self.hanging_declaration_ready(owner) {
                return None;
            }
        } else if owner.hanging_continuation.is_some() || owner.hanging_nested_head.is_some() {
            return None;
        }
        if entry.forms.len() > 1 {
            let forms = self.option_component_forms(owner)?;
            let valid = entry.id == owner.id
                && entry.kind
                    == EntryKind::Parameter {
                        parameter_kind: ParameterKind::Option,
                    }
                && entry.case == NameCase::Sensitive
                && entry.alias_groups.is_empty()
                && entry.alias_of.is_none()
                && entry.value_domain.is_none()
                && entry.forms.len() == forms.len()
                && entry.names.len() == forms.len()
                && entry.name_bindings.len() == forms.len()
                && forms.iter().enumerate().all(|(index, (name, selection))| {
                    entry.names[index] == *name
                        && entry.forms[index] == *selection
                        && entry.name_bindings[index].name == index
                        && entry.name_bindings[index].evidence == EntryNameEvidence::NativeMarkup
                        && entry.name_bindings[index].occurrences.as_slice()
                            == std::slice::from_ref(selection)
                });
            return valid.then_some(entry);
        }
        let Some(form) = self.owner_complete_form(owner) else {
            // mdoc_macro.c::blk_full may keep a long Xo HEAD whose later
            // output joins are unknown. Its first Ic/Cm component can still
            // prove a complete command word; no other entry kind may borrow
            // this partial form or infer the rest of the HEAD.
            return self
                .validated_partial_literal_command(owner, entry)
                .then_some(entry);
        };
        let [only_form] = entry.forms.as_slice() else {
            return None;
        };
        if entry.kind == EntryKind::Term
            && (owner.head_role.is_none()
                || owner.head_role == Some(OwnerHeadRole::Lexical)
                    && !owner.lexical_term_witness
                    && self
                        .lexical_names(owner)
                        .is_some_and(|names| names.is_empty()))
        {
            let valid = entry.id == owner.id
                && entry.case == NameCase::Sensitive
                && entry.alias_groups.is_empty()
                && entry.alias_of.is_none()
                && entry.value_domain.is_none()
                && only_form == &owner.head
                && entry.names.is_empty()
                && entry.name_bindings.is_empty();
            return valid.then_some(entry);
        }
        if owner.head_role == Some(OwnerHeadRole::Option)
            && owner.head_components.len() > 1
            && self.option_component_names(owner).is_some()
        {
            return self
                .validated_component_names(owner, entry, only_form)
                .then_some(entry);
        }
        if entry.names.len() > 1 {
            return (self.validated_lexical_names(owner, entry, only_form)
                || self.validated_component_names(owner, entry, only_form))
            .then_some(entry);
        }
        let [only_name] = entry.names.as_slice() else {
            return None;
        };
        let [binding] = entry.name_bindings.as_slice() else {
            return None;
        };
        let binding_matches = match (owner.head_role, entry.kind, binding.evidence) {
            (_, EntryKind::Term, EntryNameEvidence::Lexical) => {
                only_name == &form
                    && binding.occurrences.as_slice() == std::slice::from_ref(&owner.head)
                    && (owner.head_role != Some(OwnerHeadRole::Lexical)
                        || owner.lexical_term_witness
                            && self
                                .lexical_names(owner)
                                .is_some_and(|names| names.is_empty()))
            }
            (
                Some(OwnerHeadRole::Lexical),
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                EntryNameEvidence::Lexical,
            ) => self.validated_lexical_names(owner, entry, only_form),
            (
                Some(OwnerHeadRole::Option),
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                EntryNameEvidence::NativeMarkup,
            )
            | (
                Some(OwnerHeadRole::Environment),
                EntryKind::EnvironmentVariable,
                EntryNameEvidence::NativeMarkup,
            )
            | (Some(OwnerHeadRole::Literal), EntryKind::Command, EntryNameEvidence::NativeMarkup) => {
                self.native_markup_name_matches(
                    owner,
                    entry.kind,
                    &form,
                    only_name,
                    &binding.occurrences,
                )
            }
            _ => false,
        };
        (entry.id == owner.id
            && binding_matches
            && entry.case == NameCase::Sensitive
            && only_form == &owner.head
            && binding.name == 0
            && entry.alias_groups.is_empty()
            && entry.alias_of.is_none()
            && entry.value_domain.is_none())
        .then_some(entry)
    }

    fn validated_partial_literal_command(
        &self,
        owner: &OwnerMark,
        entry: &EntryFacts<TextSelection>,
    ) -> bool {
        let Some((name, component)) = self.literal_command_component(owner) else {
            return false;
        };
        let ([only_form], [only_name], [binding]) = (
            entry.forms.as_slice(),
            entry.names.as_slice(),
            entry.name_bindings.as_slice(),
        ) else {
            return false;
        };
        entry.id == owner.id
            && entry.kind == EntryKind::Command
            && entry.case == NameCase::Sensitive
            && entry.alias_groups.is_empty()
            && entry.alias_of.is_none()
            && entry.value_domain.is_none()
            && only_form == component
            && only_name == &name
            && binding.name == 0
            && binding.evidence == EntryNameEvidence::NativeMarkup
            && binding.occurrences.as_slice() == std::slice::from_ref(component)
    }

    fn native_markup_name_matches(
        &self,
        owner: &OwnerMark,
        kind: EntryKind,
        form: &str,
        name: &str,
        occurrences: &[TextSelection],
    ) -> bool {
        let start = form.len() - form.trim_start().len();
        let Some(end) = start.checked_add(name.len()) else {
            return false;
        };
        if kind == EntryKind::Command && owner.head_role == Some(OwnerHeadRole::Literal) {
            let Some((native_name, component)) = self.literal_command_component(owner) else {
                return false;
            };
            return native_name == name
                && form.get(start..end) == Some(name)
                && form.get(end..).is_some_and(|suffix| {
                    suffix.is_empty() || suffix.starts_with(char::is_whitespace)
                })
                && self.selection_subrange(&owner.head, start..end).as_ref() == Some(component)
                && occurrences == std::slice::from_ref(component);
        }
        let Some(role_prefix) = owner.head_role_prefix.as_deref() else {
            return false;
        };
        let role_proves_name = match (kind, owner.head_role) {
            (EntryKind::EnvironmentVariable, _) => {
                crate::environment_variable_alias(role_prefix).as_deref() == Some(name)
            }
            _ => role_prefix == name && crate::native_option_token(name),
        };
        role_proves_name
            && form.trim_start().starts_with(role_prefix)
            && form.get(start..end) == Some(name)
            && self.selection_subrange(&owner.head, start..end).as_ref() == occurrences.first()
            && occurrences.len() == 1
    }

    /// A first native Ic/Cm component may prove one complete command token
    /// even when a later Xo HEAD join is unknown. The component must be the
    /// exact visible prefix and must end at a proved word boundary; layout
    /// adjacency alone never supplies that boundary.
    #[must_use]
    pub fn literal_command_component<'a>(
        &self,
        owner: &'a OwnerMark,
    ) -> Option<(String, &'a TextSelection)> {
        if owner.role != OwnerRole::Definition || owner.head_role != Some(OwnerHeadRole::Literal) {
            return None;
        }
        let component = owner.head_components.first()?;
        if component.role != OwnerHeadRole::Literal || !component.has_source_identity() {
            return None;
        }
        let name = self.selection_text(&component.selection)?;
        if !crate::native_command_token(&name) || component.selection.parts.is_empty() {
            return None;
        }
        let count = component.selection.parts.len();
        if count > owner.head.parts.len()
            || component.selection.joins.as_slice() != owner.head.joins.get(..count - 1)?
            || component.selection.parts.as_slice() != owner.head.parts.get(..count)?
        {
            return None;
        }
        let boundary = if count == owner.head.parts.len() {
            true
        } else {
            match owner.head.joins.get(count - 1)? {
                TextJoin::AuthoredSeparator(separator)
                | TextJoin::GeneratedSeparator(separator) => {
                    separator.starts_with(char::is_whitespace)
                }
                TextJoin::DirectContact => {
                    let next = &owner.head.parts[count];
                    let text = self.surface.run_text(next.run)?;
                    text.get(usize::try_from(next.start_byte).ok()?..)?
                        .starts_with(char::is_whitespace)
                }
                TextJoin::HardBoundary | TextJoin::Unknown => false,
            }
        };
        boundary.then_some((name, &component.selection))
    }

    /// Close every lexical alias against the same original HEAD and one
    /// exact, surviving display sub-selection. The syntax cannot stand in for
    /// the native role or for a missing glyph range.
    fn validated_lexical_names(
        &self,
        owner: &OwnerMark,
        entry: &EntryFacts<TextSelection>,
        only_form: &TextSelection,
    ) -> bool {
        if owner.head_role != Some(OwnerHeadRole::Lexical) {
            return false;
        }
        let Some(found) = self.lexical_names(owner) else {
            return false;
        };
        let grouped = group_name_occurrences(
            found
                .into_iter()
                .map(|(name, selection, _)| (name, selection)),
        );
        entry.id == owner.id
            && entry.kind
                == EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                }
            && entry.case == NameCase::Sensitive
            && entry.alias_groups.is_empty()
            && entry.alias_of.is_none()
            && entry.value_domain.is_none()
            && only_form == &owner.head
            && entry.names.len() == grouped.len()
            && entry.name_bindings.len() == grouped.len()
            && grouped
                .iter()
                .enumerate()
                .all(|(index, (name, occurrences))| {
                    entry.names[index] == *name
                        && entry.name_bindings[index].name == index
                        && entry.name_bindings[index].evidence == EntryNameEvidence::Lexical
                        && entry.name_bindings[index].occurrences == *occurrences
                })
    }

    fn validated_component_names(
        &self,
        owner: &OwnerMark,
        entry: &EntryFacts<TextSelection>,
        only_form: &TextSelection,
    ) -> bool {
        let Some(names) = self.option_component_names(owner) else {
            return false;
        };
        let grouped = group_name_occurrences(
            names
                .into_iter()
                .map(|(name, selection, _)| (name, selection)),
        );
        entry.id == owner.id
            && entry.kind
                == EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                }
            && entry.case == NameCase::Sensitive
            && entry.alias_groups.is_empty()
            && entry.alias_of.is_none()
            && entry.value_domain.is_none()
            && only_form == &owner.head
            && entry.names.len() == grouped.len()
            && entry.name_bindings.len() == grouped.len()
            && grouped
                .iter()
                .enumerate()
                .all(|(index, (name, occurrences))| {
                    entry.names[index] == *name
                        && entry.name_bindings[index].name == index
                        && entry.name_bindings[index].evidence == EntryNameEvidence::NativeMarkup
                        && entry.name_bindings[index].occurrences == *occurrences
                })
    }

    /// Project a checked final-display selection into logical text without
    /// copying the surface into a second stored body.
    #[must_use]
    pub fn selection_text(&self, selection: &TextSelection) -> Option<String> {
        if selection.joins.len() != selection.parts.len().saturating_sub(1) {
            return None;
        }
        let mut text = String::new();
        for (index, part) in selection.parts.iter().enumerate() {
            if index != 0 {
                match &selection.joins[index - 1] {
                    TextJoin::DirectContact => {}
                    TextJoin::AuthoredSeparator(separator)
                    | TextJoin::GeneratedSeparator(separator) => text.push_str(separator),
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                }
            }
            let run = self.surface.run_text(part.run)?;
            let start = usize::try_from(part.start_byte).ok()?;
            let end = usize::try_from(part.end_byte).ok()?;
            text.push_str(run.get(start..end)?);
        }
        Some(text)
    }

    /// Map a logical UTF-8 range back to surviving display slices. A range
    /// touching a consumed authored or generated separator has no final
    /// glyph to bind and is rejected; no byte or cell coordinate is inferred
    /// from layout.
    #[must_use]
    pub fn selection_subrange(
        &self,
        selection: &TextSelection,
        range: Range<usize>,
    ) -> Option<TextSelection> {
        let logical = self.selection_text(selection)?;
        if range.start >= range.end || logical.get(range.clone()).is_none() {
            return None;
        }
        let mut cursor = 0usize;
        let mut parts = Vec::new();
        let mut joins = Vec::new();
        for (index, part) in selection.parts.iter().enumerate() {
            if index != 0 {
                match &selection.joins[index - 1] {
                    TextJoin::DirectContact => {}
                    TextJoin::AuthoredSeparator(separator)
                    | TextJoin::GeneratedSeparator(separator) => {
                        let end = cursor.checked_add(separator.len())?;
                        if range.start < end && cursor < range.end {
                            return None;
                        }
                        cursor = end;
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                }
            }
            let length = usize::try_from(part.end_byte.checked_sub(part.start_byte)?).ok()?;
            let end = cursor.checked_add(length)?;
            let start_in_part = range.start.max(cursor);
            let end_in_part = range.end.min(end);
            if start_in_part < end_in_part {
                let start_byte = part
                    .start_byte
                    .checked_add(u64::try_from(start_in_part - cursor).ok()?)?;
                let end_byte = part
                    .start_byte
                    .checked_add(u64::try_from(end_in_part - cursor).ok()?)?;
                self.surface
                    .run_text(part.run)?
                    .get(usize::try_from(start_byte).ok()?..usize::try_from(end_byte).ok()?)?;
                if !parts.is_empty() {
                    joins.push(selection.joins[index - 1].clone());
                }
                parts.push(OutputSlice {
                    run: part.run,
                    start_byte,
                    end_byte,
                });
            }
            cursor = end;
        }
        (cursor == logical.len() && !parts.is_empty()).then_some(TextSelection { parts, joins })
    }

    /// Map disjoint declaration names against the already materialized HEAD.
    /// Unlike the public single-range helper, this private path walks the
    /// native selection once for all names. `form` is the unmodified result
    /// of `selection_text(selection)` in the same lexical proof operation.
    pub(super) fn selection_subranges_from_form(
        &self,
        selection: &TextSelection,
        form: &str,
        ranges: &[Range<usize>],
    ) -> Option<Vec<TextSelection>> {
        if selection.joins.len() != selection.parts.len().saturating_sub(1) {
            return None;
        }
        let mut previous_end = 0usize;
        for range in ranges {
            if range.start < previous_end
                || range.start >= range.end
                || form.get(range.clone()).is_none()
            {
                return None;
            }
            previous_end = range.end;
        }
        let mut found = ranges
            .iter()
            .map(|_| TextSelection {
                parts: Vec::new(),
                joins: Vec::new(),
            })
            .collect::<Vec<_>>();
        let mut current = 0usize;
        let mut cursor = 0usize;
        for (index, part) in selection.parts.iter().enumerate() {
            if index != 0 {
                match &selection.joins[index - 1] {
                    TextJoin::DirectContact => {}
                    TextJoin::AuthoredSeparator(separator)
                    | TextJoin::GeneratedSeparator(separator) => {
                        let end = cursor.checked_add(separator.len())?;
                        if ranges
                            .get(current)
                            .is_some_and(|range| range.start < end && cursor < range.end)
                        {
                            return None;
                        }
                        cursor = end;
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                }
            }
            let start_byte = usize::try_from(part.start_byte).ok()?;
            let end_byte = usize::try_from(part.end_byte).ok()?;
            let run_text = self.surface.run_text(part.run)?;
            let visible = run_text.get(start_byte..end_byte)?;
            let end = cursor.checked_add(visible.len())?;
            // The public mapper skips zero-width parts even in an otherwise
            // malformed mutable selection; never emit an empty output slice.
            if visible.is_empty() {
                continue;
            }
            while let Some(range) = ranges.get(current)
                && range.start < end
            {
                if range.end <= cursor {
                    return None;
                }
                let clip_start = range.start.max(cursor);
                let clip_end = range.end.min(end);
                let slice_start = start_byte.checked_add(clip_start - cursor)?;
                let slice_end = start_byte.checked_add(clip_end - cursor)?;
                run_text.get(slice_start..slice_end)?;
                let target = found.get_mut(current)?;
                if !target.parts.is_empty() {
                    target
                        .joins
                        .push(selection.joins.get(index.checked_sub(1)?)?.clone());
                }
                target.parts.push(OutputSlice {
                    run: part.run,
                    start_byte: u64::try_from(slice_start).ok()?,
                    end_byte: u64::try_from(slice_end).ok()?,
                });
                if range.end <= end {
                    current += 1;
                } else {
                    break;
                }
            }
            cursor = end;
        }
        (cursor == form.len()
            && current == ranges.len()
            && found.iter().all(|selection| !selection.parts.is_empty()))
        .then_some(found)
    }

    /// Read one complete surviving definition head without inferring bytes
    /// from neighboring rows, owners or unknown native joins.
    #[must_use]
    pub fn owner_complete_form(&self, owner: &OwnerMark) -> Option<String> {
        if !owner.has_complete_form() {
            return None;
        }
        let form = self.selection_text(&owner.head)?;
        (!form.trim().is_empty()).then_some(form)
    }
}
