use super::super::WordProjectionPolicy;
use super::super::field_buffer::FieldCell;
use super::super::{Inline, InlineBuilder, PendingBoundary};
use super::native_passes::{RecordedRowSplit, split_recorded_native_passes};
use super::source_blanks::take_retreated_source_blank;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct WordPassProjection {
    pub(super) closes_before: Vec<usize>,
    pub(super) leading_padding: RestartedPadding,
    pub(super) native_separator: usize,
    pub(super) preserve_retreated_separator: bool,
    pub(super) consumed_initial_separator: bool,
    pub(super) prints_padding: bool,
    pub(super) authored_separator: Option<Inline>,
    pub(super) split_word: Option<Vec<Inline>>,
}

#[derive(Clone, Copy)]
pub(super) enum RestartedPadding {
    CurrentWord(usize),
    WithEarlierOwners { total: usize, current: usize },
}

impl Default for RestartedPadding {
    fn default() -> Self {
        Self::CurrentWord(0)
    }
}

impl RestartedPadding {
    fn before_word(
        buffer: &super::super::field_buffer::FieldBuffer,
        boundary: usize,
        word_start: usize,
        word_first_cell: usize,
        borrowed: Option<&BorrowedPrefix>,
    ) -> Self {
        let current = buffer.projection_length(boundary.max(word_start), word_first_cell);
        if let Some(prefix) = borrowed
            && prefix.offset(buffer, boundary) == 0
        {
            Self::WithEarlierOwners {
                total: buffer
                    .projection_length(boundary, word_first_cell)
                    .saturating_sub(prefix.count),
                current,
            }
        } else {
            Self::CurrentWord(current)
        }
    }

    pub(super) fn remaining(self, earlier_cells: usize) -> usize {
        match self {
            Self::CurrentWord(cells) => cells,
            Self::WithEarlierOwners { total, .. } => total.saturating_sub(earlier_cells),
        }
    }
    pub(super) const fn current_word_cells(self) -> usize {
        match self {
            Self::CurrentWord(cells) => cells,
            Self::WithEarlierOwners { current, .. } => current,
        }
    }
}

impl WordPassProjection {
    fn record_separator(
        &mut self,
        buffer: &super::super::field_buffer::FieldBuffer,
        word_start: usize,
        written: usize,
        surviving: usize,
    ) {
        self.native_separator = surviving;
        self.preserve_retreated_separator = written == 2 && surviving == 1;
        let content = word_start.saturating_add(surviving);
        self.consumed_initial_separator = surviving > 0
            && buffer
                .cells()
                .get(content..buffer.resume_offset())
                .is_some_and(|prefix| {
                    prefix.iter().all(|cell| {
                        matches!(cell, FieldCell::BreakableBlank | FieldCell::TabReference)
                    })
                });
    }
}

impl InlineBuilder {
    pub(super) fn record_hang_word(
        &mut self,
        incoming: &mut [Inline],
        boundary: PendingBoundary,
        projection: WordProjectionPolicy,
    ) -> WordPassProjection {
        if self.execution.native_word_owner.is_none() {
            self.begin_native_word_owner();
        }
        let native_writes = self.execution.native_word_writes.take();
        let native_boundary = self
            .execution
            .native_word_boundary
            .take()
            .unwrap_or(boundary);
        let sentence = std::mem::take(&mut self.execution.native_word_sentence);
        let projected_keep = std::mem::take(&mut self.execution.projected_keep_separator);
        self.execution.observe_table_word_writes(
            native_writes.as_deref().unwrap_or_default(),
            !native_boundary.is_native_tight(),
        );
        if !self.execution.uses_definition_word_buffer() {
            // Plain flow shares the field's ordered cell consumer: the
            // paragraph is a degenerate flush unit (term.c runs one
            // term_fill() over `tcol->buf` regardless of authorship), so
            // its words take the same marker/pass/rejection arithmetic.
            return self.record_plain_unit_word(
                incoming,
                native_boundary,
                native_writes,
                sentence,
                projected_keep,
                projection,
            );
        }
        let materialized_field_padding = self.projected_native_field_padding();
        let Some(definition) = &mut self.execution.definition else {
            return WordPassProjection::default();
        };
        // term_word() buffers its separator and glyph in the native field.
        // Generated IR padding is excluded from that field.
        let native_word_start = definition.field_buffer.cells().len();
        let separator = definition.field_buffer.begin_word_with_spacing(
            native_boundary.is_native_tight(),
            native_boundary == PendingBoundary::Kept,
            1 + usize::from(sentence),
        );
        if let Some(boundary) = definition.run_in_boundaries.last_mut() {
            boundary.observe_following_word(separator);
        }
        // The text executor supplied native writes before IR projection.
        // A stable owner marker follows wrappers and compact presentation;
        // it never contributes a native cell.
        let anchor_ir_start = self
            .execution
            .native_word_owner
            .take()
            .expect("native word owner");
        // Every source and generated formatter-word entry supplies writes.
        // Empty operands supply an explicit empty write list, preserving the
        // word event without recovering graph facts from semantic output.
        // Register the anchor only AFTER execution, from the receipt: a
        // BACKBEFORE retreat (term.c:901-908) can pop the separator blank
        // recorded above, and a pre-execution content start would put
        // already-accepted cells into a later rejected interval.
        let receipt = definition
            .field_buffer
            .apply_writes(&native_writes.expect("formatter word native writes"));
        let native_separator = definition
            .field_buffer
            .surviving_word_separator(native_word_start, separator);
        let authored_separator =
            take_retreated_source_blank(incoming, &anchor_ir_start, receipt.leading_retreat);
        definition
            .field_word_anchors
            .push(super::super::NativeWordAnchor {
                start: native_word_start,
                owner: anchor_ir_start,
                content: if native_separator > 0
                    && self.execution.pending_field_spaces == 0
                    && (!boundary.is_tight()
                        || self.execution.zero_advance_joined
                        || receipt.leading_retreat.is_some()
                        || projected_keep)
                {
                    native_word_start
                } else {
                    receipt.first_content_cell
                },
                projected_device_padding: if receipt.prints_padding {
                    self.execution.pending_field_spaces
                } else {
                    0
                }
                .max(materialized_field_padding),
                projected_field_prefix: false,
                projection,
            });
        observe_definition_word(definition, incoming, separator);
        let mut projection =
            self.record_authored_native_passes(incoming, receipt.first_content_cell);
        let (buffer, _) = self.execution.word_input();
        projection.record_separator(buffer, native_word_start, separator, native_separator);
        projection.preserve_retreated_separator |=
            receipt.leading_retreat.is_some() || projected_keep;
        projection.prints_padding = receipt.prints_padding;
        projection.authored_separator = authored_separator;
        projection
    }

    /// Plain-flow word recording: the paragraph's flush unit takes the same
    /// `term_word()` cell sequence (separator, writes, anchor) as a field.
    /// It has no hang-row geometry; only the marker pass loop can act.
    fn record_plain_unit_word(
        &mut self,
        incoming: &mut [Inline],
        native_boundary: PendingBoundary,
        native_writes: Option<Vec<super::super::field_buffer::FieldWrite>>,
        sentence: bool,
        projected_keep: bool,
        projection: WordProjectionPolicy,
    ) -> WordPassProjection {
        // term_flushln() retired the old native unit before this word.
        // The IR destination may since have changed (D1/Dl, heading, link
        // label). Establish its new output interval here, before writing
        // the word; a Vec offset from the former owner is not transferable.
        self.execution.plain_input.begin_output_at(self.nodes.len());
        let native_word_start = self.execution.plain_input.buffer.cells().len();
        let separator = self.execution.plain_input.buffer.begin_word_with_spacing(
            native_boundary.is_native_tight(),
            native_boundary == PendingBoundary::Kept,
            1 + usize::from(sentence),
        );
        let anchor_ir_start = self
            .execution
            .native_word_owner
            .take()
            .expect("native word owner");
        let receipt = self
            .execution
            .plain_input
            .buffer
            .apply_writes(&native_writes.unwrap_or_default());
        let native_separator = self
            .execution
            .plain_input
            .buffer
            .surviving_word_separator(native_word_start, separator);
        let authored_separator =
            take_retreated_source_blank(incoming, &anchor_ir_start, receipt.leading_retreat);
        self.execution
            .plain_input
            .anchors
            .push(super::super::NativeWordAnchor {
                start: native_word_start,
                owner: anchor_ir_start,
                content: if native_separator > 0
                    && !native_boundary.is_tight()
                    && (!self.execution.boundary.is_tight()
                        || self.execution.zero_advance_joined
                        || receipt.leading_retreat.is_some()
                        || projected_keep)
                {
                    native_word_start
                } else {
                    receipt.first_content_cell
                },
                projected_device_padding: 0,
                projected_field_prefix: false,
                projection,
            });
        // The plain unit is a BRNEVER-shaped degenerate field (term.c:134,
        // 143-144): responsive reflow owns width, so passes only ever end
        // at authored markers and never at a device-width guess.
        let targets = super::super::field_buffer::FillTargets {
            first: usize::MAX / 2,
            rest: usize::MAX / 2,
            unbounded: true,
        };
        let cached_glyph = self
            .execution
            .zero_advance
            .buffered_glyph_belongs_to_current_word();
        let unit = &mut self.execution.plain_input;
        let mut result = Self::native_unit_passes(
            incoming,
            &mut unit.buffer,
            &mut unit.anchors,
            targets,
            receipt.first_content_cell,
            cached_glyph,
        );
        result.record_separator(&unit.buffer, native_word_start, separator, native_separator);
        result.preserve_retreated_separator |= receipt.leading_retreat.is_some() || projected_keep;
        result.prints_padding = receipt.prints_padding;
        result.authored_separator = authored_separator;
        result
    }

    fn record_authored_native_passes(
        &mut self,
        incoming: &[Inline],
        word_first_cell: usize,
    ) -> WordPassProjection {
        // Width-dependent passes stay provisional until term_flushln(): a
        // later .ta may reinterpret every buffered tab. Only authored break
        // markers can settle a word's existing hard-break execution here.
        let Some(targets) = self.native_field_targets(false, None) else {
            return WordPassProjection::default();
        };
        let Some(definition) = self.execution.definition.as_mut() else {
            return WordPassProjection::default();
        };
        let mut buffer = std::mem::take(&mut definition.field_buffer);
        let mut anchors = std::mem::take(&mut definition.field_word_anchors);
        let cached_glyph = self
            .execution
            .zero_advance
            .buffered_glyph_belongs_to_current_word();
        let result = Self::native_unit_passes(
            incoming,
            &mut buffer,
            &mut anchors,
            targets,
            word_first_cell,
            cached_glyph,
        );
        let Some(definition) = self.execution.definition.as_mut() else {
            return result;
        };
        definition.field_buffer = buffer;
        definition.field_word_anchors = anchors;
        result
    }

    /// The shared incremental pass loop (definition fields and plain flush
    /// units alike): commit only authored-marker passes, and report the row
    /// boundary positions relative to the word just recorded.
    fn native_unit_passes(
        incoming: &[Inline],
        buffer: &mut super::super::field_buffer::FieldBuffer,
        anchors: &mut [super::super::NativeWordAnchor],
        targets: super::super::field_buffer::FillTargets,
        word_first_cell: usize,
        cached_glyph: bool,
    ) -> WordPassProjection {
        let anchor_count = anchors.len();
        let mut inside_splits: BTreeMap<String, Vec<RecordedRowSplit>> = BTreeMap::new();
        let mut closes_before = Vec::new();
        let mut leading_padding = RestartedPadding::default();
        let starts_earlier_owner = starts_with_borrowed_output(incoming, anchors.last());
        // Borrowed output need not precede this pass: a projected glyph's
        // held cells can follow its breakpoint. Recover their exact native
        // interval only when such a prefix exists, searching recent owners
        // backwards rather than scanning the growing anchor history per word.
        let borrowed = starts_earlier_owner
            .then(|| BorrowedPrefix::capture(incoming, buffer, anchors, word_first_cell))
            .flatten();
        if anchor_count > 0 && !buffer.word_scan_deferred() && buffer.has_pending_break_markers() {
            let word_end = buffer.cells().len();
            let mut first_pass = !buffer.has_committed_pass();
            while let Some(pass) = buffer.fill_pass_units(targets.scan(first_pass)) {
                if !buffer.has_non_ignorable_after(pass.end, false) {
                    break;
                }
                let pass_start = buffer.resume_offset();
                let mut boundary = pass.end;
                while matches!(
                    buffer.cells().get(boundary),
                    Some(super::super::field_buffer::FieldCell::BreakableBlank)
                ) {
                    boundary += 1;
                }
                // An unbounded target can never stop at a device-width
                // guess (term.c:134,143-144): every accepted pass there is
                // marker-driven, even when the marker sits exactly at the
                // resumed boundary (term.c:294-295 restarts at it).
                let authored = targets.unbounded
                    || buffer.cells()[pass_start..boundary].iter().any(|cell| {
                        matches!(cell, super::super::field_buffer::FieldCell::BreakMarker)
                    });
                if !authored {
                    // This pass only guessed a device width break. Do not
                    // commit its cursor or IR; the real flush owns it.
                    break;
                }
                let represented_in_ir = buffer.has_projected_pass(pass.end);
                buffer.commit_pass(pass, targets.actual(first_pass));
                let scalar_offset =
                    restarted_word_offset(buffer, anchors, boundary, word_first_cell);
                if boundary <= word_first_cell || (scalar_offset == 0 && !starts_earlier_owner) {
                    if !represented_in_ir {
                        // The incoming projection may first settle an earlier
                        // word's cached BACKBEFORE glyph. Its native graph
                        // precedes this accepted boundary, so put the row
                        // event at the current word's stable owner, not before
                        // the entire Rust fragment (term.c:901-927).
                        if starts_earlier_owner {
                            if let Some(prefix) = &borrowed {
                                let offset = prefix.offset(buffer, boundary);
                                if offset == 0 {
                                    // Every borrowed scalar follows this
                                    // native boundary. Earlier empty words
                                    // may already have projected its pending
                                    // blanks into nodes: let the same row
                                    // close transfer those cells once.
                                    closes_before.push(pass.end);
                                } else {
                                    record_row_split(
                                        &mut inside_splits,
                                        &prefix.owner,
                                        pass.end,
                                        offset,
                                        false,
                                    );
                                }
                            } else {
                                record_row_split(
                                    &mut inside_splits,
                                    &anchors[anchor_count - 1].owner,
                                    pass.end,
                                    0,
                                    false,
                                );
                            }
                        } else {
                            closes_before.push(pass.end);
                        }
                    }
                    // term_flushln() consumes blanks only until the first
                    // non-blank byte (term.c:205-207). A NODE_LINE TABREF
                    // stops that sweep, leaving this word's real separator
                    // at the new row origin even after an empty operand.
                    leading_padding = RestartedPadding::before_word(
                        buffer,
                        boundary,
                        anchors[anchor_count - 1].start,
                        word_first_cell,
                        borrowed.as_ref(),
                    );
                    restore_restarted_separator(
                        anchors,
                        boundary,
                        word_first_cell,
                        !closes_before.is_empty() && leading_padding.current_word_cells() > 0,
                    );
                } else if boundary < word_end && !represented_in_ir {
                    record_row_split(
                        &mut inside_splits,
                        &anchors[anchor_count - 1].owner,
                        pass.end,
                        scalar_offset,
                        cached_glyph && buffer.projection_length(boundary, word_end) > 0,
                    );
                }
                if boundary >= word_end {
                    break;
                }
                first_pass = false;
            }
        }
        let split_word = split_recorded_word(incoming, anchors.last(), inside_splits, buffer);
        WordPassProjection {
            closes_before,
            leading_padding,
            split_word,
            ..WordPassProjection::default()
        }
    }
}

fn starts_with_borrowed_output(
    incoming: &[Inline],
    current: Option<&super::super::NativeWordAnchor>,
) -> bool {
    current.is_some_and(|anchor| {
        matches!(incoming.first(), Some(Inline::Anchor { id, .. })
            if id.as_str().starts_with(super::INTERNAL_FIELD_WORD) && id.as_str() != anchor.owner)
    })
}

fn observe_definition_word(
    definition: &mut super::super::definition::DefinitionFieldState,
    incoming: &[Inline],
    separator: usize,
) {
    let row = &mut definition.hang_row;
    let projected = super::super::super::plain_text(incoming);
    let trimmed = projected.trim_end_matches(' ');
    let trailing_spaces = projected.len().saturating_sub(trimmed.len());
    let width = mant_ir::geometry::text_width(trimmed);
    row.field_breakable |= trimmed.contains(' ');
    row.field_unproven_break |= trimmed.contains('\n');
    // term_fill() discards a field containing only ordinary breakable
    // blanks. A nonbreaking blank from \~ or \0 is a printable cell.
    let printable = trimmed
        .chars()
        .any(|ch| !super::super::super::is_formatter_word_blank(ch) && ch != '\n');
    row.word(separator, width, trailing_spaces, printable);
    if printable {
        row.field_last_unbreakable_width = trimmed
            .rsplit([' ', '\n'])
            .find(|part| !part.is_empty())
            .map_or(0, mant_ir::geometry::text_width);
    }
}

struct BorrowedPrefix {
    owner: String,
    content: usize,
    emitted: usize,
    count: usize,
}

impl BorrowedPrefix {
    fn capture(
        incoming: &[Inline],
        buffer: &super::super::field_buffer::FieldBuffer,
        anchors: &[super::super::NativeWordAnchor],
        word_first_cell: usize,
    ) -> Option<Self> {
        let Inline::Anchor { id, .. } = incoming.first()? else {
            return None;
        };
        let index = anchors[..anchors.len() - 1]
            .iter()
            .rposition(|anchor| anchor.owner == id.as_str())?;
        let content = anchors[index].content;
        let end = anchors[index + 1].start.min(word_first_cell);
        let count = borrowed_prefix_scalars(incoming, id.as_str());
        let emitted = buffer.projection_length(content, end).saturating_sub(count);
        Some(Self {
            owner: id.as_str().to_owned(),
            content,
            emitted,
            count,
        })
    }

    fn offset(&self, buffer: &super::super::field_buffer::FieldBuffer, boundary: usize) -> usize {
        buffer
            .projection_length(self.content, boundary)
            .saturating_sub(self.emitted)
            .min(self.count)
    }
}

fn split_recorded_word(
    incoming: &[Inline],
    anchor: Option<&super::super::NativeWordAnchor>,
    boundaries: BTreeMap<String, Vec<RecordedRowSplit>>,
    buffer: &mut super::super::field_buffer::FieldBuffer,
) -> Option<Vec<Inline>> {
    if boundaries.is_empty() {
        return None;
    }
    let owner = &anchor?.owner;
    let mut nodes = incoming.to_vec();
    let starts_owned = nodes.first().is_some_and(|node| {
        matches!(node,
        Inline::Anchor { id, .. } if id.as_str().starts_with(super::INTERNAL_FIELD_WORD))
    });
    if !starts_owned {
        nodes.insert(0, Inline::anchor(owner.clone()));
    }
    Some(split_recorded_native_passes(&nodes, boundaries, buffer))
}

fn record_row_split(
    splits: &mut BTreeMap<String, Vec<RecordedRowSplit>>,
    owner: &str,
    pass_end: usize,
    offset: usize,
    before_cached_glyph: bool,
) {
    splits
        .entry(owner.to_owned())
        .or_default()
        .push(RecordedRowSplit {
            offset,
            pass_end,
            before_cached_glyph,
        });
}

fn borrowed_prefix_scalars(nodes: &[Inline], owner: &str) -> usize {
    fn count(nodes: &[Inline], owner: &str, scalars: &mut usize) -> bool {
        for node in nodes {
            match node {
                Inline::Anchor { id, .. }
                    if id.as_str().starts_with(super::INTERNAL_FIELD_WORD) =>
                {
                    if id.as_str() != owner {
                        return false;
                    }
                }
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    *scalars += value.chars().count();
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => {
                    if !count(children, owner, scalars) {
                        return false;
                    }
                }
                Inline::Anchor { .. } | Inline::LineBreak { .. } => {}
            }
        }
        true
    }
    let mut scalars = 0;
    count(nodes, owner, &mut scalars);
    scalars
}

// Only the blanks skipped by term_flushln() are outside the restarted
// range. The word owns any remaining separator projected ahead of its text.
fn restarted_word_offset(
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &mut [super::super::NativeWordAnchor],
    boundary: usize,
    word_first_cell: usize,
) -> usize {
    let offset = buffer.projection_length(word_first_cell, boundary);
    if offset == 0 {
        // term_flushln() consumes the automatic blank before a word's first
        // projected scalar (term.c:205-207). NBRZW and markers can place the
        // boundary inside that word without accepting any of its glyphs.
        let anchor = anchors.last_mut().expect("current native word");
        anchor.content = anchor.content.max(word_first_cell);
    }
    offset
}

fn restore_restarted_separator(
    anchors: &mut [super::super::NativeWordAnchor],
    boundary: usize,
    word_first_cell: usize,
    projected: bool,
) {
    if projected {
        let anchor = anchors.last_mut().expect("current native word");
        anchor.content = boundary.max(anchor.start).min(word_first_cell);
    }
}
