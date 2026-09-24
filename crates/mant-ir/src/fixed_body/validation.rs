//! Validation of the sole Fixed surface and its source-neutral mark relations.

use super::{
    AnchorMark, DisplayPoint, DisplayRole, DisplaySurface, FixedBody, HeadingMark, LinkTarget,
    MAX_FIXED_ROW_COLUMNS, MAX_FIXED_TOTAL_COLUMNS, MAX_FIXED_TOTAL_JOIN_BYTES, NonZeroU32,
    OutputSlice, SourceKey, SourceSpan, TextJoin, TextSelection, fmt,
};
use std::collections::BTreeMap;

/// A malformed reference or relationship in an owned Fixed body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedBodyError(&'static str);

impl fmt::Display for FixedBodyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for FixedBodyError {}

impl DisplaySurface {
    /// Check dense final keys, row/run ownership, UTF-8 boundaries and exact
    /// one-time byte-arena coverage. No geometry is recomputed here.
    ///
    /// # Errors
    /// Returns the first malformed surface relationship.
    pub fn validate(&self) -> Result<(), FixedBodyError> {
        if self
            .text
            .chars()
            .any(|scalar| scalar.is_control() || matches!(scalar, '\u{2028}' | '\u{2029}'))
        {
            return Err(FixedBodyError(
                "display text contains unsafe control scalar",
            ));
        }
        let mut next_run = 1usize;
        let mut total_columns = 0u64;
        for (index, row) in self.rows.iter().enumerate() {
            if row.column_count > MAX_FIXED_ROW_COLUMNS {
                return Err(FixedBodyError("display row exceeds column budget"));
            }
            total_columns = total_columns
                .checked_add(u64::from(row.column_count))
                .ok_or(FixedBodyError("display column budget overflow"))?;
            if total_columns > MAX_FIXED_TOTAL_COLUMNS {
                return Err(FixedBodyError("display exceeds total column budget"));
            }
            if row.key.get() as usize != index + 1
                || row.first_run.get() as usize != next_run
                || (index + 1 < self.rows.len() && !row.break_after)
            {
                return Err(FixedBodyError("invalid display row order or break"));
            }
            next_run = next_run
                .checked_add(row.run_count as usize)
                .ok_or(FixedBodyError("display run count overflow"))?;
            if next_run > self.runs.len() + 1 {
                return Err(FixedBodyError("display row references missing runs"));
            }
            let mut last_column_end = 0u32;
            for run in &self.runs[(row.first_run.get() - 1) as usize..next_run - 1] {
                let end = run
                    .column
                    .checked_add(run.width)
                    .ok_or(FixedBodyError("display column overflow"))?;
                if run.row != row.key || run.column < last_column_end || end > row.column_count {
                    return Err(FixedBodyError("display run has invalid row or columns"));
                }
                last_column_end = end;
            }
        }
        if next_run != self.runs.len() + 1 {
            return Err(FixedBodyError("unowned display runs"));
        }
        let mut byte_end = 0usize;
        for (index, run) in self.runs.iter().enumerate() {
            let start = usize::try_from(run.byte_start)
                .map_err(|_| FixedBodyError("display byte offset overflow"))?;
            let count = usize::try_from(run.byte_count)
                .map_err(|_| FixedBodyError("display byte length overflow"))?;
            let end = start
                .checked_add(count)
                .ok_or(FixedBodyError("display byte range overflow"))?;
            if run.key.get() as usize != index + 1
                || start != byte_end
                || count == 0
                || !self.text.is_char_boundary(end)
                || end > self.text.len()
            {
                return Err(FixedBodyError("invalid display run byte coverage"));
            }
            if run.label.role == DisplayRole::Layout
                && (run.label.owner.is_some()
                    || run.label.link.is_some()
                    || run.label.source.is_some()
                    || run.label.style.bold
                    || run.label.style.underline
                    || !self.text[start..end].bytes().all(|byte| byte == b' '))
            {
                return Err(FixedBodyError("invalid native layout run"));
            }
            byte_end = end;
        }
        if byte_end != self.text.len() {
            return Err(FixedBodyError("display text has uncovered bytes"));
        }
        Ok(())
    }

    /// Borrow one final run's visible text without copying it.
    #[must_use]
    pub fn run_text(&self, key: NonZeroU32) -> Option<&str> {
        let run = self.runs.get((key.get() - 1) as usize)?;
        (run.key == key).then_some(()).and_then(|()| {
            let start = usize::try_from(run.byte_start).ok()?;
            let end = start.checked_add(usize::try_from(run.byte_count).ok()?)?;
            self.text.get(start..end)
        })
    }

    fn non_layout_prefix(&self) -> Vec<u64> {
        let mut prefix = Vec::with_capacity(self.runs.len() + 1);
        prefix.push(0);
        for run in &self.runs {
            prefix.push(
                prefix.last().copied().unwrap_or(0)
                    + u64::from(run.label.role != DisplayRole::Layout),
            );
        }
        prefix
    }

    fn validate_selection_with_prefix(
        &self,
        selection: &TextSelection,
        non_layout_prefix: &[u64],
    ) -> Result<(), FixedBodyError> {
        if selection.joins.len() != selection.parts.len().saturating_sub(1) {
            return Err(FixedBodyError("display selection joins are not pairwise"));
        }
        let mut previous: Option<OutputSlice> = None;
        for (index, part) in selection.parts.iter().enumerate() {
            let text = self
                .run_text(part.run)
                .ok_or(FixedBodyError("display selection references missing run"))?;
            let start = usize::try_from(part.start_byte)
                .map_err(|_| FixedBodyError("display selection byte overflow"))?;
            let end = usize::try_from(part.end_byte)
                .map_err(|_| FixedBodyError("display selection byte overflow"))?;
            if start >= end || text.get(start..end).is_none() {
                return Err(FixedBodyError("display selection is not a UTF-8 slice"));
            }
            if let Some(prior) = previous
                && (part.run < prior.run
                    || (part.run == prior.run && part.start_byte < prior.end_byte))
            {
                return Err(FixedBodyError(
                    "display selection is unordered or overlapping",
                ));
            }
            if let Some(prior) = previous {
                let join = &selection.joins[index - 1];
                if let TextJoin::AuthoredSeparator(separator)
                | TextJoin::GeneratedSeparator(separator) = join
                    && (separator.is_empty() || !separator.bytes().all(|byte| byte == b' '))
                {
                    return Err(FixedBodyError("invalid native display separator"));
                }
                if !matches!(
                    join,
                    TextJoin::DirectContact
                        | TextJoin::AuthoredSeparator(_)
                        | TextJoin::GeneratedSeparator(_)
                ) {
                    previous = Some(*part);
                    continue;
                }
                if part.run == prior.run {
                    if *join != TextJoin::DirectContact || part.start_byte != prior.end_byte {
                        return Err(FixedBodyError(
                            "display join skips or repeats bytes in one run",
                        ));
                    }
                } else {
                    let prior_run = &self.runs[(prior.run.get() - 1) as usize];
                    let current_run = &self.runs[(part.run.get() - 1) as usize];
                    let prior_text = self
                        .run_text(prior.run)
                        .ok_or(FixedBodyError("display selection references missing run"))?;
                    // term_flushln() consumes a WRAP separator before
                    // term_field() emits the next row's indentation.  Those
                    // layout-only runs may sit between two native-connected
                    // selected runs, but visible authored runs may not.
                    let layout_only_gap = non_layout_prefix[prior.run.get() as usize]
                        == non_layout_prefix[(part.run.get() - 1) as usize];
                    let adjacent_rows = prior_run.row == current_run.row
                        || prior_run.row.get().checked_add(1) == Some(current_run.row.get());
                    // Native join evidence, not terminal column geometry,
                    // determines whether a soft wrap or indentation carries
                    // logical text. Still forbid skipping a visible run/row.
                    if !layout_only_gap
                        || !adjacent_rows
                        || prior.end_byte != prior_text.len() as u64
                        || part.start_byte != 0
                    {
                        return Err(FixedBodyError("display join skips visible output"));
                    }
                }
            }
            previous = Some(*part);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn validate_selection(
        &self,
        selection: &TextSelection,
    ) -> Result<(), FixedBodyError> {
        self.validate_selection_with_prefix(selection, &self.non_layout_prefix())
    }

    fn validate_point(&self, point: DisplayPoint) -> Result<(), FixedBodyError> {
        match point {
            DisplayPoint::RunBoundary { run, byte } => {
                let text = self
                    .run_text(run)
                    .ok_or(FixedBodyError("display point references missing run"))?;
                let byte = usize::try_from(byte)
                    .map_err(|_| FixedBodyError("display point byte overflow"))?;
                if !text.is_char_boundary(byte) {
                    return Err(FixedBodyError("display point is not a UTF-8 boundary"));
                }
            }
            DisplayPoint::RowColumn { row, column } => {
                let row = self
                    .rows
                    .get((row.get() - 1) as usize)
                    .ok_or(FixedBodyError("display point references missing row"))?;
                if column > row.column_count {
                    return Err(FixedBodyError("display point exceeds final row columns"));
                }
            }
            DisplayPoint::DocumentEnd { row_count } => {
                if row_count as usize != self.rows.len() {
                    return Err(FixedBodyError("document-end point has wrong row count"));
                }
            }
        }
        Ok(())
    }
}

impl FixedBody {
    /// Validate all display and mark references without consulting source bytes.
    /// The containing `Document` must additionally close all `SourceKey`s.
    ///
    /// # Errors
    /// Returns the first malformed source-neutral relationship.
    #[allow(clippy::too_many_lines)] // One pass closes all typed mark relations over one surface.
    pub fn validate(&self) -> Result<(), FixedBodyError> {
        self.surface.validate()?;
        let non_layout_prefix = self.surface.non_layout_prefix();
        let mut join_bytes = 0_u64;
        let mut validate_selection = |selection: &TextSelection| {
            self.surface
                .validate_selection_with_prefix(selection, &non_layout_prefix)?;
            for join in &selection.joins {
                if let TextJoin::AuthoredSeparator(separator)
                | TextJoin::GeneratedSeparator(separator) = join
                {
                    join_bytes = join_bytes
                        .checked_add(separator.len() as u64)
                        .ok_or(FixedBodyError("display join byte count overflows"))?;
                    if join_bytes > MAX_FIXED_TOTAL_JOIN_BYTES {
                        return Err(FixedBodyError("display join byte budget exceeded"));
                    }
                }
            }
            Ok::<(), FixedBodyError>(())
        };
        for (index, heading) in self.headings.iter().enumerate() {
            dense_key(heading.key, index)?;
            validate_heading_identity(heading)?;
            earlier(heading.parent, heading.key)?;
            self.surface.validate_point(heading.at)?;
            validate_selection(&heading.title)?;
            validate_selection(&heading.direct_body)?;
        }
        let mut latest_owner_by_scope = BTreeMap::new();
        for (index, owner) in self.owners.iter().enumerate() {
            dense_key(owner.key, index)?;
            if !crate::is_normalized_node_id(owner.id.as_str()) {
                return Err(FixedBodyError("fixed owner has invalid identity"));
            }
            earlier(owner.parent, owner.key)?;
            earlier(owner.preceding_owner, owner.key)?;
            reference(owner.section, self.headings.len())?;
            if let Some(preceding_key) = owner.preceding_owner {
                // Native C/FFI prove the IP or TP/TQ sibling family and
                // flow epoch. Typed IR retains only that asserted relation:
                // a TP/TQ lexical head need not carry an IP-style prefix.
                let preceding = self
                    .owners
                    .get((preceding_key.get() - 1) as usize)
                    .ok_or(FixedBodyError("fixed owner predecessor is missing"))?;
                if preceding.key != preceding_key
                    || latest_owner_by_scope.get(&(owner.parent, owner.section))
                        != Some(&preceding_key)
                    || preceding.parent != owner.parent
                    || preceding.section != owner.section
                    || preceding.role != super::OwnerRole::Definition
                    || owner.role != super::OwnerRole::Definition
                    || preceding.head_role != Some(super::OwnerHeadRole::Lexical)
                    || owner.head_role != Some(super::OwnerHeadRole::Lexical)
                {
                    return Err(FixedBodyError(
                        "fixed owner predecessor crosses a structural boundary",
                    ));
                }
            }
            if owner.head_role.is_some() && owner.role != super::OwnerRole::Definition {
                return Err(FixedBodyError("non-definition owner has a head role"));
            }
            if owner
                .head_role_prefix
                .as_ref()
                .is_some_and(String::is_empty)
                || owner.head_role_prefix.is_some()
                    && !matches!(
                        owner.head_role,
                        Some(
                            super::OwnerHeadRole::Option
                                | super::OwnerHeadRole::Environment
                                | super::OwnerHeadRole::Lexical
                        )
                    )
            {
                return Err(FixedBodyError("invalid native owner head prefix"));
            }
            validate_selection(&owner.head)?;
            validate_selection(&owner.direct_body)?;
            let mut previous_component: Option<&super::OwnerHeadComponent> = None;
            for component in &owner.head_components {
                validate_selection(&component.selection)?;
                if let Some(previous) = previous_component
                    && (selections_overlap(&previous.selection, &component.selection)
                        || previous
                            .selection
                            .parts
                            .last()
                            .zip(component.selection.parts.first())
                            .is_some_and(|(a, b)| a.run > b.run))
                {
                    return Err(FixedBodyError("head components overlap or reorder"));
                }
                previous_component = Some(component);
            }
            // The source-backed component must occupy a contiguous HEAD
            // interval with the same internal joins, not merely reuse some
            // later glyph parts or replace a generated separator.
            if super::component_part_ranges(&owner.head, &owner.head_components).is_none() {
                return Err(FixedBodyError("head component escapes owner head"));
            }
            if owner.entry.is_some() && self.validated_entry(owner).is_none() {
                return Err(FixedBodyError("invalid fixed entry facts"));
            }
            if selections_overlap(&owner.head, &owner.direct_body) {
                return Err(FixedBodyError("owner head and direct body overlap"));
            }
            for part in owner.head.parts.iter().chain(&owner.direct_body.parts) {
                let run = &self.surface.runs[(part.run.get() - 1) as usize];
                if run.label.owner != Some(owner.key) {
                    return Err(FixedBodyError("owner selection crosses owner labels"));
                }
            }
            if let Some(point) = owner.empty_point {
                self.surface.validate_point(point)?;
            }
            if !owner.head.parts.is_empty() || !owner.direct_body.parts.is_empty() {
                if owner.empty_point.is_some() {
                    return Err(FixedBodyError("nonempty owner has an empty point"));
                }
            } else if owner.empty_point.is_none() {
                return Err(FixedBodyError("empty owner has no display point"));
            }
            latest_owner_by_scope.insert((owner.parent, owner.section), owner.key);
        }
        let mut link_covered_bytes = vec![0u64; self.surface.runs.len()];
        for (index, link) in self.links.iter().enumerate() {
            dense_key(link.key, index)?;
            if let Some(target) = &link.target {
                validate_link_target(target)?;
            }
            validate_selection(&link.label)?;
            for part in &link.label.parts {
                let run_index = (part.run.get() - 1) as usize;
                let run = &self.surface.runs[run_index];
                if run.label.link != Some(link.key) {
                    return Err(FixedBodyError("link selection crosses occurrence labels"));
                }
                link_covered_bytes[run_index] = link_covered_bytes[run_index]
                    .checked_add(part.end_byte - part.start_byte)
                    .ok_or(FixedBodyError("link selection byte count overflows"))?;
            }
        }
        for (index, anchor) in self.anchors.iter().enumerate() {
            dense_key(anchor.key, index)?;
            validate_anchor_identity(anchor)?;
            reference(anchor.section, self.headings.len())?;
            self.surface.validate_point(anchor.at)?;
        }
        for (index, region) in self.regions.iter().enumerate() {
            dense_key(region.key, index)?;
            earlier(region.parent, region.key)?;
            reference(region.owner, self.owners.len())?;
            reference(region.section, self.headings.len())?;
            if let Some(owner) = region.owner
                && self.owners[(owner.get() - 1) as usize].section != region.section
            {
                return Err(FixedBodyError("region and owner have different sections"));
            }
            validate_selection(&region.selection)?;
            if let Some(point) = region.empty_point {
                self.surface.validate_point(point)?;
            }
            if region.selection.parts.is_empty() == region.empty_point.is_none() {
                return Err(FixedBodyError(
                    "region needs exactly one visible selection or empty point",
                ));
            }
        }
        for (index, run) in self.surface.runs.iter().enumerate() {
            reference(run.label.owner, self.owners.len())?;
            reference(run.label.link, self.links.len())?;
            if run.label.link.is_some() && link_covered_bytes[index] != run.byte_count {
                return Err(FixedBodyError("link label does not cover its labeled run"));
            }
        }
        Ok(())
    }

    /// Iterate every typed source identity retained in run labels and marks.
    pub fn source_keys(&self) -> impl Iterator<Item = SourceKey> + '_ {
        self.surface
            .runs
            .iter()
            .filter_map(|run| run.label.source)
            .chain(self.source_spans().map(|span| span.source))
    }

    /// Iterate authored mark spans for containing-document range validation.
    pub fn source_spans(&self) -> impl Iterator<Item = SourceSpan> + '_ {
        self.headings
            .iter()
            .filter_map(|mark| mark.source)
            .chain(self.owners.iter().filter_map(|mark| mark.source))
            .chain(self.owners.iter().flat_map(|owner| {
                owner
                    .head_components
                    .iter()
                    .filter_map(|component| component.source)
            }))
            .chain(self.links.iter().filter_map(|mark| mark.source))
            .chain(self.anchors.iter().filter_map(|mark| mark.source))
            .chain(self.regions.iter().filter_map(|mark| mark.source))
    }
}

fn validate_heading_identity(heading: &HeadingMark) -> Result<(), FixedBodyError> {
    if !crate::is_normalized_node_id(heading.id.as_str()) {
        return Err(FixedBodyError("fixed heading has invalid identity"));
    }
    if heading.rendered_fragment_aliases.len()
        != heading.fragment_aliases.len() + heading.generated_fragment_aliases.len()
        || heading
            .fragment_aliases
            .iter()
            .chain(&heading.generated_fragment_aliases)
            .chain(&heading.rendered_fragment_aliases)
            .any(|alias| !valid_alias(alias))
    {
        return Err(FixedBodyError("fixed heading has invalid fragment alias"));
    }
    Ok(())
}

fn validate_anchor_identity(anchor: &AnchorMark) -> Result<(), FixedBodyError> {
    if !crate::is_normalized_node_id(anchor.id.as_str())
        || !valid_alias(&anchor.name)
        || !valid_alias(&anchor.rendered_fragment)
    {
        return Err(FixedBodyError("fixed anchor has invalid identity or name"));
    }
    Ok(())
}

fn valid_alias(value: &str) -> bool {
    !value.is_empty()
        && !value
            .chars()
            .any(|scalar| scalar.is_control() || scalar.is_whitespace())
}

fn selections_overlap(left: &TextSelection, right: &TextSelection) -> bool {
    let (mut left_index, mut right_index) = (0, 0);
    while let (Some(a), Some(b)) = (left.parts.get(left_index), right.parts.get(right_index)) {
        if a.run < b.run || (a.run == b.run && a.end_byte <= b.start_byte) {
            left_index += 1;
        } else if b.run < a.run || (a.run == b.run && b.end_byte <= a.start_byte) {
            right_index += 1;
        } else {
            return true;
        }
    }
    false
}

fn validate_link_target(target: &LinkTarget) -> Result<(), FixedBodyError> {
    let valid = match target {
        LinkTarget::External { uri } => crate::is_valid_external_uri(uri),
        LinkTarget::Email { address } => crate::is_valid_email_address(address),
        LinkTarget::Document { name, fragment } => {
            !name.is_empty()
                && !name.starts_with('/')
                && !name.contains(['\\', '?', '#'])
                && !name.chars().any(char::is_control)
                && name.split('/').all(|component| !component.is_empty())
                && fragment.as_deref().is_none_or(|fragment| {
                    !fragment.is_empty() && !fragment.chars().any(char::is_control)
                })
        }
        LinkTarget::Manual {
            name,
            manual_section,
        } => {
            !name.is_empty()
                && !name.contains(['/', '\\'])
                && !name
                    .chars()
                    .any(|scalar| scalar.is_whitespace() || scalar.is_control())
                && manual_section
                    .as_deref()
                    .is_none_or(crate::is_manual_section)
        }
        LinkTarget::Section { id } => crate::is_normalized_node_id(id.as_str()),
    };
    valid
        .then_some(())
        .ok_or(FixedBodyError("invalid fixed link target"))
}

fn dense_key(key: NonZeroU32, index: usize) -> Result<(), FixedBodyError> {
    if key.get() as usize == index + 1 {
        Ok(())
    } else {
        Err(FixedBodyError("display mark key is not dense"))
    }
}

fn earlier(parent: Option<NonZeroU32>, child: NonZeroU32) -> Result<(), FixedBodyError> {
    if parent.is_none_or(|key| key < child) {
        Ok(())
    } else {
        Err(FixedBodyError("display mark parent must be earlier"))
    }
}

fn reference(key: Option<NonZeroU32>, len: usize) -> Result<(), FixedBodyError> {
    if key.is_none_or(|key| key.get() as usize <= len) {
        Ok(())
    } else {
        Err(FixedBodyError("display mark reference does not resolve"))
    }
}
