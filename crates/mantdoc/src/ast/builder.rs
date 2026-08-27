use super::{
    AuthorMode, BTreeMap, DisplayKind, Document, EquationTerminal, InputUnicodeProvenance,
    MacroSet, MdocListMarker, Metadata, NodeFlags, NodeId, NodeKind, NodeRecord,
    NormalizedEnclosure, NormalizedFont, NormalizedListKind, Source, SourceId, SourcePosition,
    SourceRecord, SourceSpan, StringId, TableCell, TableTerminalRow,
};

pub(crate) struct DocumentBuilder {
    document: Document,
    children: Vec<Vec<NodeId>>,
    /// Only source lines with non-ASCII or malformed input retain this
    /// temporary tbl projection. It is consumed during preprocessing and is
    /// never copied into the completed public document.
    table_input_text: BTreeMap<NodeId, Box<str>>,
    /// Semantic preprocessor opener attached to the first normalized output
    /// event. Package restructuring consumes it as an otherwise invisible
    /// scope boundary; it is never copied into the completed public document.
    preprocessor_openers: BTreeMap<NodeId, &'static str>,
    /// Scanner-owned mdoc `nS` state changes.  They deliberately do not
    /// become public AST nodes: mdoc's parser observes the register as
    /// presentation state, not as a roff request in the syntax tree.
    ///
    /// Each boundary is the number of root source events already emitted when
    /// the change took effect.  The mdoc pass consumes it before that indexed
    /// flat source event, retaining source order even though `.nr` itself is
    /// transparent in the final tree.
    mdoc_synopsis_events: Vec<(usize, bool)>,
}

impl DocumentBuilder {
    pub(crate) fn new(macro_set: MacroSet, root_source: Source<'_>) -> Self {
        Self {
            document: Document::empty(macro_set, root_source),
            children: vec![Vec::new()],
            table_input_text: BTreeMap::new(),
            preprocessor_openers: BTreeMap::new(),
            mdoc_synopsis_events: Vec::new(),
        }
    }

    /// Return the macro package selected for this in-progress document.
    pub(crate) const fn macro_set(&self) -> MacroSet {
        self.document.macro_set
    }

    /// Record the resolved mdoc operating-system label.
    ///
    /// This is deliberately parser-internal: public metadata becomes
    /// immutable only when [`Self::finish`] returns the completed document.
    pub(crate) fn operating_system(&mut self, value: impl Into<Box<str>>) {
        self.document.metadata.os = Some(value.into());
    }

    /// Mutably borrow parser-owned metadata before the document is frozen.
    pub(crate) fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.document.metadata
    }

    #[allow(dead_code)] // M2 scanner starts constructing syntax nodes through this builder.
    pub(crate) const fn root() -> NodeId {
        NodeId(0)
    }

    #[allow(dead_code)] // M2 scanner starts spans at the parser-owned root source.
    pub(crate) const fn root_source() -> SourceId {
        SourceId(0)
    }

    /// Register one resolver-owned source in the document-local source map.
    ///
    /// The parser validates byte and line budgets before calling this method;
    /// this builder only rejects an identity that cannot be represented by the
    /// opaque public [`SourceId`].
    pub(crate) fn add_source(&mut self, source: Source<'_>) -> Option<SourceId> {
        let index = u32::try_from(self.document.sources.len()).ok()?;
        self.document
            .sources
            .push(SourceRecord::from_source(source));
        Some(SourceId(index))
    }

    #[allow(dead_code)] // M2 scanner enforces the public AST node budget.
    pub(crate) fn node_count(&self) -> usize {
        self.document.nodes.len()
    }

    /// Record the scanner-observed value of mdoc's private `nS` register.
    pub(crate) fn record_mdoc_synopsis_state(&mut self, active: bool) {
        self.mdoc_synopsis_events
            .push((self.children[Self::root().0 as usize].len(), active));
    }

    /// Take the private `nS` state stream for one mdoc restructuring pass.
    pub(crate) fn take_mdoc_synopsis_events(&mut self) -> Vec<(usize, bool)> {
        std::mem::take(&mut self.mdoc_synopsis_events)
    }

    /// Return a parser-owned node role for semantic restructuring.
    pub(crate) fn node_kind(&self, node: NodeId) -> Option<NodeKind> {
        self.document
            .nodes
            .get(node.0 as usize)
            .map(|record| record.kind)
    }

    /// Return a parser-owned parent while semantic postprocessing still owns
    /// the arena topology.
    pub(crate) fn node_parent(&self, node: NodeId) -> Option<NodeId> {
        self.document.nodes.get(node.0 as usize)?.parent
    }

    /// Change a parser-owned node role before immutable edges are frozen.
    pub(crate) fn set_node_kind(&mut self, node: NodeId, kind: NodeKind) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.kind = kind;
        true
    }

    /// Read one parser-owned macro name without leaking string-table IDs.
    pub(crate) fn node_macro_name(&self, node: NodeId) -> Option<&str> {
        let record = self.document.nodes.get(node.0 as usize)?;
        record.macro_name.map(|id| self.document.string(id))
    }

    /// Read parser-owned visible text without leaking string-table IDs.
    pub(crate) fn node_text(&self, node: NodeId) -> Option<&str> {
        let record = self.document.nodes.get(node.0 as usize)?;
        record.text.map(|id| self.document.string(id))
    }

    /// Read a parser-owned validated destination without leaking string-table IDs.
    pub(crate) fn node_tag(&self, node: NodeId) -> Option<&str> {
        let record = self.document.nodes.get(node.0 as usize)?;
        record.tag.map(|id| self.document.string(id))
    }

    /// Replace provisional visible text during a semantic normalization pass.
    pub(crate) fn set_node_text(&mut self, node: NodeId, value: impl Into<Box<str>>) -> bool {
        if self.document.nodes.get(node.0 as usize).is_none() {
            return false;
        }
        let Ok(index) = u32::try_from(self.document.strings.len()) else {
            return false;
        };
        self.document.strings.push(value.into());
        self.document.nodes[node.0 as usize].text = Some(StringId(index));
        true
    }

    /// Return the scanner-owned byte immediately following one argument.
    ///
    /// This is intentionally private parser metadata, not source text or a
    /// public AST property.  It is consumed by mdoc phrase reconstruction
    /// before [`Self::finish`] freezes the arena.
    pub(crate) fn node_separator_after(&self, node: NodeId) -> Option<u8> {
        self.document.nodes.get(node.0 as usize)?.separator_after
    }

    /// Whether the scanner-owned separator after an argument contains a tab.
    pub(crate) fn node_separator_contains_tab(&self, node: NodeId) -> bool {
        self.document
            .nodes
            .get(node.0 as usize)
            .is_some_and(|record| record.separator_contains_tab)
    }

    /// Return the scanner-owned count of literal tabs within an argument.
    pub(crate) fn node_embedded_tab_count(&self, node: NodeId) -> u32 {
        self.document
            .nodes
            .get(node.0 as usize)
            .map_or(0, |record| record.embedded_tab_count)
    }

    /// Return the scanner-owned horizontal-whitespace run after one argument.
    pub(crate) fn node_separator_width(&self, node: NodeId) -> u32 {
        self.document
            .nodes
            .get(node.0 as usize)
            .map_or(0, |record| record.separator_width)
    }

    /// Retain one scanner-owned argument delimiter for package restructuring.
    pub(crate) fn set_node_separator_after(&mut self, node: NodeId, value: Option<u8>) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.separator_after = value;
        true
    }

    /// Retain whether a scanner-owned separator contains a tab.
    pub(crate) fn set_node_separator_contains_tab(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.separator_contains_tab = value;
        true
    }

    /// Retain the scanner-owned number of literal tabs within an argument.
    pub(crate) fn set_node_embedded_tab_count(&mut self, node: NodeId, value: usize) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.embedded_tab_count = u32::try_from(value).unwrap_or(u32::MAX);
        true
    }

    /// Retain the width of one scanner-owned argument delimiter.
    pub(crate) fn set_node_separator_width(&mut self, node: NodeId, value: usize) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.separator_width = u32::try_from(value).unwrap_or(u32::MAX);
        true
    }

    /// Record that a copy-mode argument contained an authored escaped
    /// tabulation escape.  This is scanner provenance, not public AST state.
    pub(crate) fn set_node_protected_tabulation_escape(
        &mut self,
        node: NodeId,
        value: bool,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.protected_tabulation_escape = value;
        true
    }

    /// Read the temporary copy-mode provenance for package restructuring.
    pub(crate) fn node_has_protected_tabulation_escape(&self, node: NodeId) -> bool {
        self.document
            .nodes
            .get(node.0 as usize)
            .is_some_and(|record| record.protected_tabulation_escape)
    }

    /// Record byte-encoding provenance until semantic preprocessing has
    /// consumed source-relative text offsets.
    pub(crate) fn set_node_input_unicode_provenance(
        &mut self,
        node: NodeId,
        has_invalid_input_bytes: bool,
        has_valid_utf8_non_ascii: bool,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.input_unicode_provenance =
            InputUnicodeProvenance::new(has_invalid_input_bytes, has_valid_utf8_non_ascii);
        true
    }

    /// Read malformed-byte provenance during semantic preprocessing.
    pub(crate) fn node_has_invalid_input_bytes(&self, node: NodeId) -> bool {
        self.document
            .nodes
            .get(node.0 as usize)
            .is_some_and(|record| record.input_unicode_provenance.has_invalid_input_bytes())
    }

    /// Read valid UTF-8 provenance during semantic preprocessing.
    pub(crate) fn node_has_valid_utf8_non_ascii(&self, node: NodeId) -> bool {
        self.document
            .nodes
            .get(node.0 as usize)
            .is_some_and(|record| record.input_unicode_provenance.has_valid_utf8_non_ascii())
    }

    /// Retain a byte-faithful tbl projection for one exceptional source line.
    pub(crate) fn set_node_table_input_text(
        &mut self,
        node: NodeId,
        value: impl Into<Box<str>>,
    ) -> bool {
        if self.document.nodes.get(node.0 as usize).is_none() {
            return false;
        }
        self.table_input_text.insert(node, value.into());
        true
    }

    /// Read the transient byte-faithful tbl projection during preprocessing.
    pub(crate) fn node_table_input_text(&self, node: NodeId) -> Option<&str> {
        self.table_input_text.get(&node).map(Box::as_ref)
    }

    /// Mark one normalized preprocessing event as the first public result of
    /// an otherwise-elided roff preprocessor opener.
    pub(crate) fn set_node_preprocessor_opener(
        &mut self,
        node: NodeId,
        opener: &'static str,
    ) -> bool {
        if self.document.nodes.get(node.0 as usize).is_none() {
            return false;
        }
        self.preprocessor_openers.insert(node, opener);
        true
    }

    /// Read the private preprocessor opener during package restructuring.
    pub(crate) fn node_preprocessor_opener(&self, node: NodeId) -> Option<&'static str> {
        self.preprocessor_openers.get(&node).copied()
    }

    /// Record the private post-expansion width adjustment for one argument.
    pub(crate) fn set_node_argument_expansion_width_delta(
        &mut self,
        node: NodeId,
        value: i32,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.argument_expansion_width_delta = value;
        true
    }

    /// Read the private post-expansion width adjustment for package validation.
    pub(crate) fn node_argument_expansion_width_delta(&self, node: NodeId) -> i32 {
        self.document
            .nodes
            .get(node.0 as usize)
            .map_or(0, |record| record.argument_expansion_width_delta)
    }

    /// Retain whether a scanner argument had an outer quote for package
    /// validators that need legacy suffix source positions.
    pub(crate) fn set_node_argument_quoted(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.argument_quoted = value;
        true
    }

    /// Read private outer-quote provenance during package validation.
    pub(crate) fn node_argument_quoted(&self, node: NodeId) -> bool {
        self.document
            .nodes
            .get(node.0 as usize)
            .is_some_and(|record| record.argument_quoted)
    }

    /// Read the source span attached to a provisional node.
    pub(crate) fn node_location(&self, node: NodeId) -> Option<SourceSpan> {
        self.document.nodes.get(node.0 as usize)?.location.clone()
    }

    /// Resolve a provisional node's current source location for package
    /// validators that need an explicit logical diagnostic column.
    pub(crate) fn node_source_position(&self, node: NodeId) -> Option<SourcePosition> {
        let location = self.node_location(node)?;
        self.document.source_position(&location)
    }

    /// Resolve an arbitrary provisional diagnostic span to its logical source
    /// position without exposing arena storage details to package validators.
    pub(crate) fn source_position(&self, span: &SourceSpan) -> Option<SourcePosition> {
        self.document.source_position(span)
    }

    /// Set a source span on a synthesized semantic node.
    pub(crate) fn set_node_location(&mut self, node: NodeId, value: Option<SourceSpan>) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.location = value;
        true
    }

    /// Rebase a continued control-line node onto the final physical line while
    /// preserving its original logical column.  mandoc's package parsers use
    /// this provenance for an argument list joined with a trailing escape.
    pub(crate) fn rebase_node_location_to_final_line(&mut self, node: NodeId) -> bool {
        let Some(location) = self
            .document
            .nodes
            .get(node.0 as usize)
            .and_then(|record| record.location.clone())
        else {
            return false;
        };
        let Some(source) = self.document.sources.get(location.source.0 as usize) else {
            return false;
        };
        let line_start_for = |offset: u32| {
            source
                .line_starts
                .get(
                    source
                        .line_starts
                        .partition_point(|start| *start <= offset)
                        .saturating_sub(1),
                )
                .copied()
        };
        let Some(initial_line_start) = line_start_for(location.start) else {
            return false;
        };
        let Some(final_line_start) = line_start_for(location.end.saturating_sub(1)) else {
            return false;
        };
        let Some(start) = final_line_start.checked_add(location.start - initial_line_start) else {
            return false;
        };
        if start > location.end {
            return false;
        }
        let final_line = source
            .position(location.end.saturating_sub(1))
            .map_or(1, |position| position.line);
        let logical_column = location
            .start
            .checked_sub(initial_line_start)
            .and_then(|column| column.checked_add(1))
            .unwrap_or(1);
        self.document.nodes[node.0 as usize].location =
            SourceSpan::new(location.source, start, location.end)
                .ok()
                .map(|span| {
                    span.with_logical_start(SourcePosition {
                        line: final_line,
                        column: logical_column,
                    })
                });
        true
    }

    /// Override the presentation location of a node while preserving its byte
    /// range.  This is restricted to parser lowering because it represents
    /// legacy logical-line provenance rather than a source edit.
    pub(crate) fn set_node_logical_start(
        &mut self,
        node: NodeId,
        position: SourcePosition,
    ) -> bool {
        let Some(location) = self
            .document
            .nodes
            .get_mut(node.0 as usize)
            .and_then(|record| record.location.as_mut())
        else {
            return false;
        };
        location.logical_start = Some(position);
        true
    }

    /// Read parser-owned flags while semantic passes still own the arena.
    pub(crate) fn node_flags(&self, node: NodeId) -> Option<NodeFlags> {
        self.document
            .nodes
            .get(node.0 as usize)
            .map(|record| record.flags)
    }

    /// Read provisional list semantics while mdoc postprocessing still owns
    /// the arena topology.
    pub(crate) fn node_list_kind(&self, node: NodeId) -> Option<NormalizedListKind> {
        self.document
            .nodes
            .get(node.0 as usize)
            .and_then(|record| record.list_kind)
    }

    /// Read provisional compact layout state during package postprocessing.
    pub(crate) fn node_compact(&self, node: NodeId) -> Option<bool> {
        self.document
            .nodes
            .get(node.0 as usize)
            .map(|record| record.compact)
    }

    /// Replace parser-owned flags before the document is frozen.
    pub(crate) fn set_node_flags(&mut self, node: NodeId, flags: NodeFlags) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.flags = flags;
        true
    }

    /// Attach a parser-validated same-document destination spelling.
    pub(crate) fn set_node_tag(&mut self, node: NodeId, value: impl Into<Box<str>>) -> bool {
        if self.document.nodes.get(node.0 as usize).is_none() {
            return false;
        }
        let Ok(index) = u32::try_from(self.document.strings.len()) else {
            return false;
        };
        self.document.strings.push(value.into());
        self.document.nodes[node.0 as usize].tag = Some(StringId(index));
        true
    }

    /// Remove a provisional tag superseded by a duplicate fallback heading.
    pub(crate) fn clear_node_tag(&mut self, node: NodeId) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.tag = None;
        true
    }

    /// Set the normalized list semantics for one provisional node.
    pub(crate) fn set_node_list_kind(
        &mut self,
        node: NodeId,
        value: Option<NormalizedListKind>,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.list_kind = value;
        true
    }

    /// Retain the exact mdoc list marker selected by validation for renderers.
    pub(crate) fn set_node_list_marker(
        &mut self,
        node: NodeId,
        value: Option<MdocListMarker>,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.list_marker = value;
        true
    }

    /// Retain renderer-only mdoc column declaration phrases.
    pub(crate) fn set_node_column_widths(
        &mut self,
        node: NodeId,
        values: impl IntoIterator<Item = String>,
    ) -> bool {
        let values = values.into_iter().collect::<Vec<_>>();
        let Some(total) = self.document.strings.len().checked_add(values.len()) else {
            return false;
        };
        if self.document.nodes.get(node.0 as usize).is_none() || total > u32::MAX as usize {
            return false;
        }
        let start = self.document.strings.len();
        self.document
            .strings
            .extend(values.into_iter().map(Into::into));
        let widths = (start..self.document.strings.len())
            .map(|index| {
                // `total` above is bounded by `u32::MAX`, and this half-open
                // range consequently cannot produce an out-of-range id.
                StringId(u32::try_from(index).expect("checked string index fits u32"))
            })
            .collect();
        self.document.nodes[node.0 as usize].column_widths = widths;
        true
    }

    /// Retain mdoc `Bl -hang` provenance for terminal layout only.
    pub(crate) fn set_node_terminal_hanging_list(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.terminal_hanging_list = value;
        true
    }

    /// Retain mdoc `Bl -ohang` provenance for terminal layout only.
    pub(crate) fn set_node_terminal_overhanging_list(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.terminal_overhanging_list = value;
        true
    }

    /// Retain mdoc `Bl -inset` provenance for terminal layout only.
    pub(crate) fn set_node_terminal_inset_list(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.terminal_inset_list = value;
        true
    }

    /// Retain mdoc `Bl -diag` provenance for terminal layout only.
    pub(crate) fn set_node_terminal_diagnostic_list(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.terminal_diagnostic_list = value;
        true
    }

    /// Retain a man validation-only blank-line suppression for terminal
    /// presentation without exposing it through the public AST schema.
    pub(crate) fn set_node_terminal_suppressed_leading_blank(
        &mut self,
        node: NodeId,
        value: bool,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.terminal_suppressed_leading_blank = value;
        true
    }

    /// Retain same-line conditional renderer provenance without exposing it
    /// in the public compatible AST schema.
    pub(crate) fn set_node_terminal_inline_conditional(
        &mut self,
        node: NodeId,
        value: bool,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.terminal_inline_conditional = value;
        true
    }

    /// Set the normalized display behavior for one provisional node.
    pub(crate) fn set_node_display_kind(
        &mut self,
        node: NodeId,
        value: Option<DisplayKind>,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.display_kind = value;
        true
    }

    /// Retain whether an mdoc display used the `-literal` spelling.
    pub(crate) fn set_node_literal_display(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.literal_display = value;
        true
    }

    /// Retain whether an mdoc display used the `-centered` spelling.
    pub(crate) fn set_node_centered_display(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.centered_display = value;
        true
    }

    /// Set the normalized font behavior for one provisional node.
    pub(crate) fn set_node_font(&mut self, node: NodeId, value: Option<NormalizedFont>) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.font = value;
        true
    }

    /// Set the normalized mdoc author layout behavior for one provisional node.
    pub(crate) fn set_node_author_mode(&mut self, node: NodeId, value: Option<AuthorMode>) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.author_mode = value;
        true
    }

    /// Set the mdoc `Es`/`En` delimiters resolved for one provisional node.
    pub(crate) fn set_node_enclosure(
        &mut self,
        node: NodeId,
        value: Option<NormalizedEnclosure>,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.enclosure = value;
        true
    }

    /// Set compact layout behavior for one provisional node.
    pub(crate) fn set_node_compact(&mut self, node: NodeId, value: bool) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.compact = value;
        true
    }

    /// Set a normalized roff offset string for one provisional node.
    pub(crate) fn set_node_offset(&mut self, node: NodeId, value: impl Into<Box<str>>) -> bool {
        if self.document.nodes.get(node.0 as usize).is_none() {
            return false;
        }
        let Ok(index) = u32::try_from(self.document.strings.len()) else {
            return false;
        };
        self.document.strings.push(value.into());
        self.document.nodes[node.0 as usize].offset = Some(StringId(index));
        true
    }

    /// Set a normalized width string for one provisional node.
    pub(crate) fn set_node_width(&mut self, node: NodeId, value: impl Into<Box<str>>) -> bool {
        if self.document.nodes.get(node.0 as usize).is_none() {
            return false;
        }
        let Ok(index) = u32::try_from(self.document.strings.len()) else {
            return false;
        };
        self.document.strings.push(value.into());
        self.document.nodes[node.0 as usize].width = Some(StringId(index));
        true
    }

    /// Copy the normalized layout fields of one provisional node to another.
    ///
    /// Semantic recovery sometimes materializes a closer-owned Body node;
    /// its source span and flags belong to the closer, while its layout is
    /// inherited from the interrupted block.
    pub(crate) fn copy_node_layout(&mut self, source: NodeId, target: NodeId) -> bool {
        let Some(source) = self.document.nodes.get(source.0 as usize) else {
            return false;
        };
        let layout = (
            source.list_kind,
            source.list_marker,
            source.column_widths.clone(),
            source.display_kind,
            source.literal_display,
            source.centered_display,
            source.font,
            source.author_mode,
            source.enclosure.clone(),
            source.compact,
            source.offset,
            source.width,
        );
        let Some(target) = self.document.nodes.get_mut(target.0 as usize) else {
            return false;
        };
        (
            target.list_kind,
            target.list_marker,
            target.column_widths,
            target.display_kind,
            target.literal_display,
            target.centered_display,
            target.font,
            target.author_mode,
            target.enclosure,
            target.compact,
            target.offset,
            target.width,
        ) = layout;
        true
    }

    /// Set normalized tbl cells on a synthesized table-row node.
    pub(crate) fn set_node_table_cells(&mut self, node: NodeId, value: Vec<TableCell>) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.table_cells = value;
        true
    }

    /// Set private terminal tbl layout metadata on a generated row.
    pub(crate) fn set_node_table_terminal(
        &mut self,
        node: NodeId,
        value: TableTerminalRow,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.table_terminal = Some(value);
        true
    }

    /// Set a normalized eqn expression on a synthesized equation node.
    pub(crate) fn set_node_equation(&mut self, node: NodeId, value: impl Into<Box<str>>) -> bool {
        if self.document.nodes.get(node.0 as usize).is_none() {
            return false;
        }
        let Ok(index) = u32::try_from(self.document.strings.len()) else {
            return false;
        };
        self.document.strings.push(value.into());
        self.document.nodes[node.0 as usize].equation = Some(StringId(index));
        true
    }

    /// Set private device eqn metadata without exposing it through the AST.
    pub(crate) fn set_node_equation_terminal(
        &mut self,
        node: NodeId,
        value: EquationTerminal,
    ) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.equation_terminal = Some(value);
        true
    }

    /// Copy the provisional direct children of an in-progress node.
    pub(crate) fn children(&self, parent: NodeId) -> Option<&[NodeId]> {
        self.children.get(parent.0 as usize).map(Vec::as_slice)
    }

    /// Replace an in-progress node's direct children in source order.
    ///
    /// This primitive intentionally does not retain the children at their
    /// previous parent. Semantic restructurers call it only on nodes taken
    /// from the provisional flat scanner tree.
    pub(crate) fn replace_children(&mut self, parent: NodeId, children: &[NodeId]) -> bool {
        if parent.0 as usize >= self.document.nodes.len()
            || children
                .iter()
                .any(|child| child.0 as usize >= self.document.nodes.len())
        {
            return false;
        }
        for child in children {
            self.document.nodes[child.0 as usize].parent = Some(parent);
        }
        self.children[parent.0 as usize].clear();
        self.children[parent.0 as usize].extend_from_slice(children);
        true
    }

    /// Append an existing provisional node under a new semantic parent.
    pub(crate) fn append_existing_child(&mut self, parent: NodeId, child: NodeId) -> bool {
        if parent.0 as usize >= self.document.nodes.len()
            || child.0 as usize >= self.document.nodes.len()
        {
            return false;
        }
        self.document.nodes[child.0 as usize].parent = Some(parent);
        self.children[parent.0 as usize].push(child);
        true
    }

    /// Retain only the finite prefix reachable through `max_depth` node
    /// levels, counting the synthetic root as level one.
    ///
    /// The old FFI adapter copied its root at recursive depth zero and did
    /// not descend from a node at depth 255.  Keeping 256 levels here gives
    /// callers the same observable prefix without reintroducing a recursive
    /// owned-tree copy.  The semantic passes have not exposed any [`NodeId`]
    /// yet, so discarded arena entries can be compacted immediately.
    pub(crate) fn truncate_descendants_at_depth(&mut self, max_depth: usize) -> bool {
        debug_assert!(max_depth > 0);
        let mut truncated = false;
        let mut pending = vec![(Self::root(), 1_usize)];

        while let Some((node, depth)) = pending.pop() {
            let Some(children) = self.children.get(node.0 as usize) else {
                continue;
            };
            if depth >= max_depth {
                if !children.is_empty() {
                    self.children[node.0 as usize].clear();
                    truncated = true;
                }
                continue;
            }
            pending.extend(
                children
                    .iter()
                    .rev()
                    .copied()
                    .map(|child| (child, depth + 1)),
            );
        }

        if truncated {
            self.compact_reachable_nodes();
        }
        truncated
    }

    /// Remove arena entries no longer reachable from the synthetic root.
    ///
    /// Restructuring may replace provisional child lists.  That is harmless
    /// while the builder is private, but a finite-prefix result must not keep
    /// detached nodes observable through `Document::node_count`.  Node IDs
    /// are intentionally opaque and no public view exists until `finish`, so
    /// rebuilding the private arena is safe.
    fn compact_reachable_nodes(&mut self) {
        let old_node_count = self.document.nodes.len();
        let mut mapping = vec![None; old_node_count];
        let mut order = Vec::new();
        let mut pending = vec![Self::root()];

        while let Some(node) = pending.pop() {
            let index = node.0 as usize;
            if mapping.get(index).is_none() || mapping[index].is_some() {
                continue;
            }
            let next = NodeId(
                u32::try_from(order.len()).expect("reachable node count fits opaque NodeId"),
            );
            mapping[index] = Some(next);
            order.push(node);
            if let Some(children) = self.children.get(index) {
                pending.extend(children.iter().rev().copied());
            }
        }

        let mut nodes = Vec::with_capacity(order.len());
        let mut children = Vec::with_capacity(order.len());
        for old in &order {
            let mut record = self.document.nodes[old.0 as usize].clone();
            record.parent = None;
            record.child_start = 0;
            record.child_len = 0;
            nodes.push(record);
            children.push(Vec::new());
        }
        for old in order {
            let new = mapping[old.0 as usize].expect("reachable node has a new ID");
            let new_children = self.children[old.0 as usize]
                .iter()
                .filter_map(|child| mapping[child.0 as usize])
                .collect::<Vec<_>>();
            for child in &new_children {
                nodes[child.0 as usize].parent = Some(new);
            }
            children[new.0 as usize] = new_children;
        }

        self.document.nodes = nodes;
        self.children = children;
    }

    #[allow(dead_code)] // M2 scanner starts constructing syntax nodes through this builder.
    pub(crate) fn push(&mut self, parent: NodeId, kind: NodeKind) -> Option<NodeId> {
        if parent.0 as usize >= self.document.nodes.len()
            || self.document.nodes.len() >= u32::MAX as usize
        {
            return None;
        }
        let id = NodeId(u32::try_from(self.document.nodes.len()).ok()?);
        let mut record = NodeRecord::root();
        record.kind = kind;
        record.parent = Some(parent);
        self.document.nodes.push(record);
        self.children.push(Vec::new());
        self.children[parent.0 as usize].push(id);
        Some(id)
    }

    #[allow(dead_code)] // M2 scanner starts constructing syntax nodes through this builder.
    pub(crate) fn text(&mut self, node: NodeId, value: impl Into<Box<str>>) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        let Ok(index) = u32::try_from(self.document.strings.len()) else {
            return false;
        };
        let id = StringId(index);
        self.document.strings.push(value.into());
        record.text = Some(id);
        true
    }

    /// Clear temporary scanner text when a token is reclassified as an mdoc
    /// inline macro during private semantic restructuring.
    pub(crate) fn clear_node_text(&mut self, node: NodeId) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.text = None;
        true
    }

    #[allow(dead_code)] // M2 scanner retains control names before macro parsing starts.
    pub(crate) fn macro_name(&mut self, node: NodeId, value: impl Into<Box<str>>) -> bool {
        if self.document.nodes.get(node.0 as usize).is_none() {
            return false;
        }
        let Ok(index) = u32::try_from(self.document.strings.len()) else {
            return false;
        };
        self.document.strings.push(value.into());
        self.document.nodes[node.0 as usize].macro_name = Some(StringId(index));
        true
    }

    #[allow(dead_code)] // M2 scanner supplies physical-line and continuation flags.
    pub(crate) fn flags(&mut self, node: NodeId, flags: NodeFlags) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        record.flags = flags;
        true
    }

    #[allow(dead_code)] // M2 scanner associates every emitted node with a source span.
    pub(crate) fn location(&mut self, node: NodeId, span: SourceSpan) -> bool {
        let Some(record) = self.document.nodes.get_mut(node.0 as usize) else {
            return false;
        };
        let Some(source) = self.document.sources.get(span.source.0 as usize) else {
            return false;
        };
        if span.end > source.byte_len {
            return false;
        }
        record.location = Some(span);
        true
    }

    pub(crate) fn finish(mut self) -> Document {
        for (index, children) in self.children.into_iter().enumerate() {
            let start = self.document.child_edges.len();
            self.document.child_edges.extend(children);
            self.document.nodes[index].child_start =
                u32::try_from(start).expect("node count bounds the number of child edges");
            self.document.nodes[index].child_len =
                u32::try_from(self.document.child_edges.len() - start)
                    .expect("node count bounds each node's child count");
        }
        // Builders grow geometrically, but a completed document is immutable.
        // Reclaim that transient capacity before it becomes observable memory.
        self.document.nodes.shrink_to_fit();
        self.document.child_edges.shrink_to_fit();
        self.document.strings.shrink_to_fit();
        self.document.sources.shrink_to_fit();
        for source in &mut self.document.sources {
            source.line_starts.shrink_to_fit();
        }
        self.document
    }
}
