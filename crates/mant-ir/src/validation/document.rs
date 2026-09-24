//! Validation for invariants shared by every normalized document source.

#[cfg(test)]
use super::links::{email_address_from_mailto_uri, mailto_uri_for_email_address};
use super::{
    links::{is_valid_email_address, is_valid_external_uri},
    source::validate_source_span,
};
use crate::{
    Block, CoverageScope, DefinitionItem, Diagnostic, DiagnosticLevel, Document, DocumentBodyRef,
    DocumentIndex, DocumentReference, IndexedRole, Inline, LinkTarget, NodeId, Section, SourceSpan,
    ValueDomain,
    visit::{self, Visit},
};

/// Validate invariants that parsers must satisfy before consumers receive IR.
///
/// Findings are ordinary document diagnostics so best-effort parsing remains
/// possible, while every parser and consumer sees the same contract failures.
#[must_use]
pub fn validate_document(document: &Document) -> Vec<Diagnostic> {
    crate::DocumentValidation::new(document).into_diagnostics()
}

#[allow(clippy::too_many_lines)]
pub(super) fn validate_with_index(
    document: &Document,
    index: &DocumentIndex,
    relations: &[crate::EntryRelationIssue],
) -> Vec<Diagnostic> {
    let DocumentBodyRef::Flow(flow) = document.body() else {
        let DocumentBodyRef::Fixed(fixed) = document.body() else {
            unreachable!("all document body arms were matched")
        };
        return validate_fixed_document(document, fixed, index);
    };
    let mut diagnostics = Vec::new();

    for source in document
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.source)
    {
        validate_source_span(&mut diagnostics, source);
    }
    for diagnostic in &document.diagnostics {
        match diagnostic.coverage_scope {
            Some(CoverageScope::Source { key }) if document.source_record(key).is_none() => {
                diagnostics.push(invariant(
                    "ir.invalid-coverage-source",
                    format!(
                        "diagnostic coverage scope references unknown source {}",
                        key.get()
                    ),
                ));
            }
            Some(
                CoverageScope::Section { .. }
                | CoverageScope::Owner { .. }
                | CoverageScope::Region { .. },
            ) => diagnostics.push(invariant(
                "ir.invalid-coverage-scope",
                "native mark coverage scope cannot be attached to a Flow document".to_owned(),
            )),
            Some(CoverageScope::Document | CoverageScope::Source { .. }) | None => {}
        }
    }

    for (id, node) in index.iter() {
        if id.trim().is_empty() {
            for role in node.roles() {
                diagnostics.push(invariant(
                    "ir.empty-identity",
                    format!("{role:?} identity must not be empty"),
                ));
            }
        } else if !is_normalized_node_id(id) {
            diagnostics.push(invariant(
                "ir.invalid-identity",
                format!("identity '{id}' is not a normalized document-local ID"),
            ));
        }
        if node.roles().len() > 1
            && !(node.roles().len() == 2
                && node.has_role(IndexedRole::Entry)
                && node.has_role(IndexedRole::Anchor))
        {
            diagnostics.push(invariant(
                "ir.identity-role-collision",
                format!(
                    "identity '{id}' is shared by incompatible roles {:?}",
                    node.roles()
                ),
            ));
        }
    }

    for duplicate in index.duplicates() {
        diagnostics.push(invariant(
            "ir.duplicate-identity",
            format!("duplicate {:?} identity '{}'", duplicate.role, duplicate.id),
        ));
    }

    for alias in index.authored_fragments() {
        if alias.is_empty() {
            diagnostics.push(invariant(
                "ir.empty-fragment-alias",
                "source-authored fragment alias must not be empty".to_owned(),
            ));
        } else if alias
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        {
            diagnostics.push(invariant(
                "ir.invalid-fragment-alias",
                format!(
                    "source-authored fragment alias '{alias}' contains whitespace or control characters"
                ),
            ));
        }
    }
    for (alias, targets) in index.ambiguous_fragments() {
        diagnostics.push(invariant(
            "ir.ambiguous-fragment-alias",
            format!(
                "fragment '{alias}' resolves to multiple document-local IDs: {}",
                targets
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }

    let mut collector = InvariantCollector {
        content: document.content(),
        positions: document.content().inline_position_index(),
        section_targets: Vec::new(),
        diagnostics: Vec::new(),
        seen_atoms: vec![0; flow.content_store.atoms.len()],
        seen_fixed_views: vec![0; flow.content_store.fixed_views.len()],
        seen_table_points: vec![0; flow.content_store.points.len()],
        strong_depth: 0,
        emphasis_depth: 0,
        active_link: None,
        linked_leaf_count: 0,
    };
    collector.visit_document(document);
    for atom in &flow.content_store.atoms {
        let seen = usize::try_from(atom.key.get() - 1)
            .ok()
            .and_then(|index| collector.seen_atoms.get(index))
            .copied()
            .unwrap_or(0);
        let expected = usize::from(!matches!(
            atom.kind,
            crate::ContentAtomKind::BreakOpportunity {}
        ));
        if seen != expected {
            collector.diagnostics.push(invariant(
                "ir.invalid-content-coverage",
                format!(
                    "content atom {} must occur exactly {expected} time(s) in inline topology, found {seen}",
                    atom.key.get()
                ),
            ));
        }
    }
    for (index, seen) in collector.seen_fixed_views.iter().enumerate() {
        if *seen != 1 {
            collector.diagnostics.push(invariant(
                "ir.invalid-fixed-view-coverage",
                format!(
                    "fixed view {} must be used by exactly one block or table",
                    index + 1
                ),
            ));
        }
    }
    diagnostics.extend(collector.diagnostics);
    for id in collector.section_targets {
        if !index.contains(id.as_str()) {
            diagnostics.push(invariant(
                "ir.dangling-section-link",
                format!("section link target '{id}' does not exist"),
            ));
        }
    }

    diagnostics.extend(relations.iter().map(crate::EntryRelationIssue::diagnostic));
    diagnostics
}

fn validate_fixed_document(
    document: &Document,
    fixed: &crate::FixedBody,
    index: &DocumentIndex,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if let Err(error) = fixed.validate() {
        diagnostics.push(invariant("ir.invalid-fixed-body", error.to_string()));
    }
    if let Err(error) = crate::validate_document_sources(document) {
        diagnostics.push(invariant("ir.invalid-source-relation", error.to_string()));
    }
    for source in document
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.source)
        .chain(fixed.source_spans())
    {
        validate_source_span(&mut diagnostics, source);
    }
    for alias in &document.fragment_aliases {
        if alias.is_empty() || alias.chars().any(|c| c.is_control() || c.is_whitespace()) {
            diagnostics.push(invariant(
                "ir.invalid-fragment-alias",
                "document root fragment alias must not be empty or contain whitespace".to_owned(),
            ));
        }
    }
    for (id, node) in index.iter() {
        if !is_normalized_node_id(id) {
            diagnostics.push(invariant(
                "ir.invalid-identity",
                format!("identity '{id}' is not a normalized document-local ID"),
            ));
        }
        if node.roles().len() > 1 {
            diagnostics.push(invariant(
                "ir.identity-role-collision",
                format!("identity '{id}' is shared by incompatible roles"),
            ));
        }
    }
    for duplicate in index.duplicates() {
        diagnostics.push(invariant(
            "ir.duplicate-identity",
            format!("duplicate {:?} identity '{}'", duplicate.role, duplicate.id),
        ));
    }
    for (alias, targets) in index.ambiguous_fragments() {
        diagnostics.push(invariant(
            "ir.ambiguous-fragment-alias",
            format!(
                "fragment '{alias}' resolves to multiple document-local IDs: {}",
                targets
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    for link in &fixed.links {
        if let Some(LinkTarget::Section { id }) = &link.target
            && !index.contains(id.as_str())
        {
            diagnostics.push(invariant(
                "ir.dangling-section-link",
                format!("section link target '{id}' does not exist"),
            ));
        }
    }
    diagnostics
}

// Internal classification of this validator's own findings. Consumers read
// Diagnostic::impact, never these codes or another producer's private list.
fn is_semantic_completeness_diagnostic(code: &str) -> bool {
    matches!(
        code,
        "ir.empty-identity"
            | "ir.invalid-coverage-source"
            | "ir.invalid-coverage-scope"
            | "ir.invalid-declaration-group"
            | "ir.invalid-entry-content"
            | "ir.invalid-entry-name-binding"
            | "ir.invalid-entry-alias-groups"
            | "ir.invalid-entry-alias-of"
            | "ir.cyclic-entry-alias"
            | "ir.invalid-identity"
            | "ir.identity-role-collision"
            | "ir.duplicate-identity"
            | "ir.empty-fragment-alias"
            | "ir.invalid-fragment-alias"
            | "ir.ambiguous-fragment-alias"
            | "ir.empty-semantic-document-reference"
            | "ir.invalid-semantic-document-reference"
            | "ir.empty-entry-value-domain"
            | "ir.duplicate-entry-value-kind"
            | "ir.invalid-entry-choices"
    )
}

/// Whether an exact authored ID satisfies the canonical identity grammar.
/// This does not check document-local uniqueness or reserved selector names.
#[must_use]
pub fn is_normalized_node_id(id: &str) -> bool {
    let mut characters = id.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    let Some(last) = id.chars().next_back() else {
        return false;
    };
    (first.is_alphanumeric() || first == '_')
        && (last.is_alphanumeric() || last == '_')
        && id
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '-' | '_'))
        && id.chars().flat_map(char::to_lowercase).eq(id.chars())
}

fn invariant(code: &str, message: String) -> Diagnostic {
    Diagnostic {
        impact: invariant_impact(code),
        level: DiagnosticLevel::Warning,
        code: Some(code.to_owned()),
        message,
        source: None,
        source_key: None,
        coverage_scope: None,
    }
}

pub(super) fn invariant_at(code: &str, message: String, source: SourceSpan) -> Diagnostic {
    Diagnostic {
        impact: invariant_impact(code),
        level: DiagnosticLevel::Warning,
        code: Some(code.to_owned()),
        message,
        source: Some(source),
        source_key: None,
        coverage_scope: None,
    }
}

fn invariant_impact(code: &str) -> crate::DiagnosticImpact {
    if is_semantic_completeness_diagnostic(code) {
        crate::DiagnosticImpact::SemanticCoverage
    } else {
        crate::DiagnosticImpact::None
    }
}

struct InvariantCollector<'a> {
    content: crate::ContentContext<'a>,
    positions: Option<crate::InlinePositionIndex<'a>>,
    section_targets: Vec<NodeId>,
    diagnostics: Vec<Diagnostic>,
    seen_atoms: Vec<usize>,
    seen_fixed_views: Vec<usize>,
    seen_table_points: Vec<u8>,
    strong_depth: usize,
    emphasis_depth: usize,
    active_link: Option<crate::LinkOccurrenceKey>,
    linked_leaf_count: usize,
}

impl InvariantCollector<'_> {
    #[allow(clippy::too_many_lines)] // Keep row shape and cell-point relations in one pass.
    fn validate_table(
        &mut self,
        rows: &[crate::TableRow],
        fixed_view: Option<crate::FixedViewKey>,
    ) {
        let placed_points = fixed_view
            .and_then(|view| self.content.fixed_view(view))
            .map(|view| {
                view.lines
                    .iter()
                    .flat_map(|line| &line.placements)
                    .filter_map(|placement| match placement.target {
                        crate::PlacementTarget::Point(point) => Some(point),
                        crate::PlacementTarget::Content(_) => None,
                    })
                    .fold(std::collections::HashMap::new(), |mut counts, point| {
                        *counts.entry(point).or_insert(0_usize) += 1;
                        counts
                    })
            });
        for row in rows {
            if matches!(
                row.kind,
                crate::TableRowKind::HorizontalRule | crate::TableRowKind::DoubleHorizontalRule
            ) && !row.cells.is_empty()
            {
                self.diagnostics.push(invariant(
                    "ir.invalid-table-rule-cells",
                    "whole-row table rules must not contain cells".to_owned(),
                ));
            }
            if let crate::TableRowKind::LayoutRule { cells } = &row.kind {
                if cells.is_empty() {
                    self.diagnostics.push(invariant(
                        "ir.empty-table-layout-rule",
                        "layout-only table rules must contain at least one rule cell".to_owned(),
                    ));
                }
                // Historical v0.12 JSON stores strengths without cell
                // records. Native lowering retains real cells too; when
                // present, the two representations must agree exactly.
                if !row.cells.is_empty()
                    && (cells.len() != row.cells.len()
                        || cells
                            .iter()
                            .zip(&row.cells)
                            .any(|(strength, cell)| match cell.kind {
                                crate::TableCellKind::HorizontalRule
                                | crate::TableCellKind::IsolatedHorizontalRule => {
                                    *strength != crate::TableRuleCellKind::Horizontal
                                }
                                crate::TableCellKind::DoubleHorizontalRule
                                | crate::TableCellKind::IsolatedDoubleHorizontalRule => {
                                    *strength != crate::TableRuleCellKind::DoubleHorizontal
                                }
                                crate::TableCellKind::Text => true,
                            }))
                {
                    self.diagnostics.push(invariant(
                        "ir.invalid-table-layout-rule",
                        "layout-only rule cells must match the declared column strengths"
                            .to_owned(),
                    ));
                }
            }
        }
        for cell in rows.iter().flat_map(|row| &row.cells) {
            if let Some(key) = cell.point {
                let valid = self.content.point(key).is_some_and(|point| {
                    self.content.root(point.root).is_some_and(|root| {
                        root.kind == crate::ContentRootKind::Cell && root.owner == point.owner
                    }) && self
                        .content
                        .owner(point.owner)
                        .is_some_and(|owner| owner.kind == crate::ContentOwnerKind::TableCell)
                });
                if !valid {
                    self.diagnostics.push(invariant(
                        "ir.invalid-table-cell-point",
                        format!(
                            "table cell point {} must belong to a cell root and owner",
                            key.get()
                        ),
                    ));
                }
                let seen = usize::try_from(key.get() - 1)
                    .ok()
                    .and_then(|index| self.seen_table_points.get_mut(index));
                if let Some(seen) = seen {
                    *seen = seen.saturating_add(1);
                    if *seen != 1 {
                        self.diagnostics.push(invariant(
                            "ir.duplicate-table-cell-point",
                            format!("table cell point {} is used more than once", key.get()),
                        ));
                    }
                }
                if placed_points
                    .as_ref()
                    .is_some_and(|points| points.get(&key) != Some(&1))
                {
                    self.diagnostics.push(invariant(
                        "ir.unplaced-table-cell-point",
                        format!(
                            "fixed table cell point {} must have exactly one physical placement",
                            key.get()
                        ),
                    ));
                }
            }
            if let Some(source) = cell.source {
                validate_source_span(&mut self.diagnostics, source);
            }
            if cell.kind != crate::TableCellKind::Text && !cell.blocks.is_empty() {
                self.diagnostics.push(invariant(
                    "ir.invalid-table-rule-content",
                    "table rule cells must not contain ordinary block content".to_owned(),
                ));
            }
            if cell.column_span == 0 || cell.row_span == 0 {
                self.diagnostics.push(invariant(
                    "ir.invalid-table-span",
                    "table row and column spans must be at least one".to_owned(),
                ));
            }
        }
    }

    fn record_fixed_view(&mut self, key: crate::FixedViewKey) {
        let Some(seen) = usize::try_from(key.get() - 1)
            .ok()
            .and_then(|index| self.seen_fixed_views.get_mut(index))
        else {
            self.diagnostics.push(invariant(
                "ir.invalid-fixed-view-reference",
                format!("fixed view {} does not exist", key.get()),
            ));
            return;
        };
        *seen = seen.saturating_add(1);
    }

    fn validate_fixed_display_content(&mut self, children: &[Inline], key: crate::FixedViewKey) {
        fn collect_roots(
            nodes: &[Inline],
            content: crate::ContentContext<'_>,
            roots: &mut std::collections::HashSet<crate::ContentRootKey>,
        ) {
            for node in nodes {
                match node {
                    Inline::Text { content: reference } | Inline::Code { content: reference } => {
                        if let Some(atom) = content.atom(reference.atom) {
                            roots.insert(atom.root);
                        }
                    }
                    Inline::LineBreak { atom } => {
                        if let Some(atom) = content.atom(*atom) {
                            roots.insert(atom.root);
                        }
                    }
                    Inline::Anchor { point, .. } => {
                        if let Some(point) = content.point(*point) {
                            roots.insert(point.root);
                        }
                    }
                    Inline::Strong { children }
                    | Inline::Emphasis { children }
                    | Inline::Link { children, .. } => collect_roots(children, content, roots),
                }
            }
        }

        let Some(view) = self.content.fixed_view(key) else {
            return; // The missing view is reported by record_fixed_view.
        };
        let mut roots = std::collections::HashSet::new();
        collect_roots(children, self.content, &mut roots);
        for placement in view.lines.iter().flat_map(|line| &line.placements) {
            let root = match placement.target {
                crate::PlacementTarget::Content(reference) => {
                    self.content.atom(reference.atom).map(|atom| atom.root)
                }
                crate::PlacementTarget::Point(point) => {
                    self.content.point(point).map(|point| point.root)
                }
            };
            if root.is_some_and(|root| !roots.contains(&root)) {
                self.diagnostics.push(invariant(
                    "ir.invalid-fixed-view-content",
                    format!("fixed view {} places content outside its block", key.get()),
                ));
                break;
            }
        }
    }

    fn invalid_content(&mut self, detail: impl Into<String>) {
        self.diagnostics
            .push(invariant("ir.invalid-content-reference", detail.into()));
    }

    fn record_atom(&mut self, key: crate::ContentAtomKey) {
        let Some(index) = usize::try_from(key.get() - 1)
            .ok()
            .filter(|index| *index < self.seen_atoms.len())
        else {
            self.invalid_content("inline leaf references an unknown content atom");
            return;
        };
        self.seen_atoms[index] = self.seen_atoms[index].saturating_add(1);
        if self.active_link.is_some() {
            self.linked_leaf_count = self.linked_leaf_count.saturating_add(1);
        }
    }

    fn validate_inline_sequence(&mut self, nodes: &[Inline]) {
        if !self
            .positions
            .as_mut()
            .is_some_and(|positions| positions.is_positioned_in_document(nodes))
        {
            self.invalid_content(
                "inline sequence roots and zero-width points must match their logical positions",
            );
        }
    }

    fn validate_entry(&mut self, item: crate::EntryOwner<'_>) {
        if let Some(source) = item.source() {
            validate_source_span(&mut self.diagnostics, source);
        }
        if item.facts().is_some() && self.content.entry_forms(item).ok().flatten().is_none() {
            self.diagnostics.push(invariant(
                "ir.invalid-entry-content",
                "entry form references must address valid owner content".to_owned(),
            ));
        }
        if matches!(
            item.facts()
                .and_then(|identity| identity.value_domain.as_ref()),
            Some(ValueDomain::Choices { .. })
        ) && !item.has_value_choices()
        {
            self.diagnostics.push(invariant(
                "ir.invalid-entry-choices",
                "a choices domain requires nonempty direct semantic children of kind value"
                    .to_owned(),
            ));
        }
        if let Some(ValueDomain::EntrySet {
            reference,
            entry_kinds,
            source,
        }) = item
            .facts()
            .and_then(|identity| identity.value_domain.as_ref())
        {
            validate_semantic_document_reference(&mut self.diagnostics, reference);
            if let Some(source) = source {
                validate_source_span(&mut self.diagnostics, *source);
            }
            if entry_kinds.is_empty() {
                self.diagnostics.push(invariant(
                    "ir.empty-entry-value-domain",
                    "cross-document entry value domain must select at least one entry kind"
                        .to_owned(),
                ));
            }
            if entry_kinds
                .iter()
                .enumerate()
                .any(|(index, kind)| entry_kinds[..index].contains(kind))
            {
                self.diagnostics.push(invariant(
                    "ir.duplicate-entry-value-kind",
                    "cross-document entry value domain must not repeat entry kinds".to_owned(),
                ));
            }
        }
    }
}

impl<'ir> Visit<'ir> for InvariantCollector<'ir> {
    fn visit_heading(&mut self, heading: &'ir crate::Heading) {
        if let Some(source) = heading.source {
            validate_source_span(&mut self.diagnostics, source);
        }
        self.validate_inline_sequence(&heading.content);
        visit::walk_heading(self, heading);
    }
    fn visit_section(&mut self, section: &'ir Section) {
        if let Some(source) = section.source {
            validate_source_span(&mut self.diagnostics, source);
        }
        visit::walk_section(self, section);
    }

    fn visit_block(&mut self, block: &'ir Block) {
        match block {
            Block::FixedDisplay { view, .. }
            | Block::Table {
                fixed_view: Some(view),
                ..
            } => self.record_fixed_view(*view),
            _ => {}
        }
        if let Block::FixedDisplay { children, view, .. } = block {
            self.validate_fixed_display_content(children, *view);
        }
        if let Block::Paragraph { children, .. }
        | Block::Preformatted { children, .. }
        | Block::FixedDisplay { children, .. } = block
        {
            self.validate_inline_sequence(children);
        }
        if let Block::DefinitionList {
            items,
            declaration_groups,
            ..
        } = block
        {
            let mut end = 0;
            for group in declaration_groups {
                if group.start_item < end || group.resolve(self.content, items).is_none() {
                    self.diagnostics.push(invariant(
                        "ir.invalid-declaration-group",
                        "declaration groups must be ordered, disjoint, in bounds and end in readable context after empty heads".to_owned(),
                    ));
                }
                end = group.end_item;
            }
        }
        let source = match block {
            Block::Paragraph { source, .. }
            | Block::Preformatted { source, .. }
            | Block::FixedDisplay { source, .. }
            | Block::List { source, .. }
            | Block::DefinitionList { source, .. }
            | Block::Table { source, .. }
            | Block::Equation { source, .. }
            | Block::VerticalSpace { source, .. }
            | Block::ThematicBreak { source }
            | Block::Unsupported { source, .. } => *source,
        };
        if let Some(source) = source {
            validate_source_span(&mut self.diagnostics, source);
        }
        if let Block::Table {
            rows, fixed_view, ..
        } = block
        {
            self.validate_table(rows, *fixed_view);
        }
        visit::walk_block(self, block);
    }

    fn visit_definition_item(&mut self, item: &'ir DefinitionItem) {
        self.validate_entry(crate::EntryOwner::Definition(item));
        for term in &item.terms {
            self.validate_inline_sequence(term);
        }
        visit::walk_definition_item(self, item);
    }

    fn visit_list_item(&mut self, item: &'ir crate::ListItem) {
        self.validate_entry(crate::EntryOwner::List(item));
        visit::walk_list_item(self, item);
    }

    #[allow(clippy::too_many_lines)]
    fn visit_inline(&mut self, inline: &'ir Inline) {
        let Ok(view) = self.content.inline(inline) else {
            self.invalid_content("inline content does not resolve in the document content store");
            return;
        };
        let target = match view {
            crate::InlineView::Link(link) => Some(link.target()),
            _ => None,
        };
        match target {
            Some(LinkTarget::Section { id }) => self.section_targets.push(id.clone()),
            Some(LinkTarget::External { uri }) if !is_valid_external_uri(uri) => {
                self.diagnostics.push(invariant(
                    "ir.invalid-external-uri",
                    format!("external link target '{uri}' is not an absolute URI"),
                ));
            }
            Some(LinkTarget::Email { address }) if !is_valid_email_address(address) => {
                self.diagnostics.push(invariant(
                    "ir.invalid-email-address",
                    format!("email link target '{address}' is not a valid mailbox"),
                ));
            }
            _ => {}
        }
        match (inline, view) {
            (
                Inline::Text { content } | Inline::Code { content },
                crate::InlineView::Text(_) | crate::InlineView::Code(_),
            ) => {
                self.record_atom(content.atom);
                let Some(atom) = self.content.atom(content.atom) else {
                    return;
                };
                let Some(text) = atom.kind.text() else {
                    self.invalid_content("text leaf references a non-text content atom");
                    return;
                };
                if content.bytes.start != 0 || content.bytes.end as usize != text.len() {
                    self.invalid_content(
                        "structural inline leaves must cover their complete content atom",
                    );
                }
                if atom.link != self.active_link {
                    self.invalid_content(
                        "inline link wrapper does not agree with the atom occurrence",
                    );
                }
                if self.strong_depth > 0 && !atom.style.strong {
                    self.invalid_content("strong wrapper does not agree with atom style");
                }
                if self.emphasis_depth > 0 && !atom.style.emphasis {
                    self.invalid_content("emphasis wrapper does not agree with atom style");
                }
                if matches!(inline, Inline::Code { .. }) != atom.style.literal {
                    self.invalid_content("code leaf does not agree with atom literal style");
                }
            }
            (Inline::Strong { children }, crate::InlineView::Strong(_)) => {
                self.strong_depth = self.strong_depth.saturating_add(1);
                for child in children {
                    self.visit_inline(child);
                }
                self.strong_depth -= 1;
            }
            (Inline::Emphasis { children }, crate::InlineView::Emphasis(_)) => {
                self.emphasis_depth = self.emphasis_depth.saturating_add(1);
                for child in children {
                    self.visit_inline(child);
                }
                self.emphasis_depth -= 1;
            }
            (
                Inline::Link {
                    occurrence,
                    children,
                },
                crate::InlineView::Link(_),
            ) => {
                if self.active_link.is_some() {
                    self.invalid_content("link wrappers cannot be nested");
                }
                let previous = self.active_link.replace(*occurrence);
                let before = self.linked_leaf_count;
                for child in children {
                    self.visit_inline(child);
                }
                if self.linked_leaf_count == before
                    && self
                        .content
                        .occurrence(*occurrence)
                        .is_some_and(|link| !link.label.is_empty())
                {
                    self.invalid_content(
                        "link wrapper borrows an occurrence with visible label atoms elsewhere",
                    );
                }
                self.active_link = previous;
            }
            (Inline::Anchor { point, .. }, crate::InlineView::Anchor(_)) => {
                if self.content.point(*point).is_none() {
                    self.invalid_content("anchor references an unknown content point");
                }
            }
            (Inline::LineBreak { atom }, crate::InlineView::LineBreak) => {
                self.record_atom(*atom);
                let Some(record) = self.content.atom(*atom) else {
                    return;
                };
                if record.link != self.active_link {
                    self.invalid_content(
                        "linked hard break does not agree with its structural wrapper",
                    );
                }
            }
            _ => self.invalid_content("inline node resolved to a mismatched content view"),
        }
    }
}

fn validate_semantic_document_reference(
    diagnostics: &mut Vec<Diagnostic>,
    reference: &DocumentReference,
) {
    let empty = match reference {
        DocumentReference::Document { name, fragment } => {
            name.trim().is_empty()
                || fragment
                    .as_deref()
                    .is_some_and(|fragment| fragment.trim().is_empty())
        }
        DocumentReference::Manual {
            name,
            manual_section,
        } => {
            name.trim().is_empty()
                || manual_section
                    .as_deref()
                    .is_some_and(|section| section.trim().is_empty())
        }
    };
    if empty {
        diagnostics.push(invariant(
            "ir.empty-semantic-document-reference",
            "semantic document reference components must not be empty".to_owned(),
        ));
    } else if !reference.is_well_formed() {
        diagnostics.push(invariant(
            "ir.invalid-semantic-document-reference",
            "semantic document reference does not follow the document or manual grammar".to_owned(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Block, DefinitionItem, DocumentMeta, EntryFacts, EntryKind, LayoutHint, NameCase, Section,
        SourceCoordinates, SourceFormat, SourceIdentity, SourceKey, SourceRecord, TableCell,
        TableRow, TextRange, TextSize,
    };

    use super::*;

    #[test]
    fn table_cell_points_require_cell_ownership_and_unique_use() {
        let mut builder = crate::ContentStoreBuilder::new();
        let owner =
            builder.push_owner(crate::ContentOwnerKind::Content, crate::Provenance::Unknown);
        let root = builder.push_root(
            owner,
            crate::ContentRootKind::Body,
            crate::Provenance::Unknown,
        );
        let point = builder.push_point(
            root,
            crate::PointBoundary::BetweenAtoms { atom_boundary: 0 },
            0,
            crate::Provenance::Unknown,
        );
        let cell = TableCell {
            kind: crate::TableCellKind::Text,
            blocks: Vec::new(),
            point: Some(point),
            column_span: 1,
            row_span: 1,
            alignment: None,
            source: None,
        };
        let block = Block::Table {
            rows: vec![TableRow {
                kind: crate::TableRowKind::Data,
                cells: vec![cell.clone(), cell],
            }],
            fixed_view: None,
            layout: LayoutHint::default(),
            source: None,
        };
        let codes = validate_document(&document_with_store(
            builder.finish(),
            Vec::new(),
            vec![block],
        ))
        .into_iter()
        .filter_map(|diagnostic| diagnostic.code)
        .collect::<Vec<_>>();
        assert!(codes.contains(&"ir.invalid-table-cell-point".to_owned()));
        assert!(codes.contains(&"ir.duplicate-table-cell-point".to_owned()));
    }

    fn document(sections: Vec<Section>, blocks: Vec<Block>) -> Document {
        document_with_store(crate::ContentStore::default(), sections, blocks)
    }

    fn document_with_store(
        content_store: crate::ContentStore,
        sections: Vec<Section>,
        blocks: Vec<Block>,
    ) -> Document {
        Document {
            parser: None,
            sources: vec![SourceRecord {
                key: SourceKey::FIRST,
                identity: SourceIdentity::Anonymous {
                    name: "test".to_owned(),
                },
                format: SourceFormat::Markdown,
                decoded_byte_length: u64::MAX,
                content_sha256: None,
                coordinates: SourceCoordinates::DecodedUtf8Bytes,
            }],
            root_source: SourceKey::FIRST,
            body: crate::DocumentBody::Flow(crate::FlowBody {
                content_store,
                heading: None,
                blocks,
                sections,
            }),
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn invalid_fixed_target_is_a_document_invariant_not_trusted_content() {
        let mut document = document(Vec::new(), Vec::new());
        document.body = crate::DocumentBody::Fixed(crate::FixedBody {
            surface: crate::DisplaySurface {
                text: String::new(),
                rows: Vec::new(),
                runs: Vec::new(),
            },
            headings: Vec::new(),
            owners: Vec::new(),
            links: vec![crate::LinkMark {
                key: std::num::NonZeroU32::new(1).unwrap(),
                target: Some(LinkTarget::External {
                    uri: "https://unsafe host".to_owned(),
                }),
                label: crate::TextSelection {
                    parts: Vec::new(),
                    joins: Vec::new(),
                },
                source_key: None,
                source: None,
            }],
            anchors: Vec::new(),
            regions: Vec::new(),
        });
        assert!(
            validate_document(&document)
                .iter()
                .any(|diagnostic| { diagnostic.code.as_deref() == Some("ir.invalid-fixed-body") })
        );
        assert!(
            serde_json::from_value::<Document>(serde_json::to_value(document).unwrap()).is_err()
        );
    }

    #[test]
    fn fixed_display_cannot_place_a_later_same_owner_paragraph() {
        use crate::{
            CellMapKind, ContentByteRange, ContentOwnerKind, ContentRef, ContentRootKind,
            ContentStoreBuilder, ContentStyle, FixedLine, FixedLineKey, FixedView, FixedViewKey,
            Placement, PlacementKey, PlacementTarget, Provenance,
        };

        let mut builder = ContentStoreBuilder::new();
        let owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
        let fixed = builder.push_root(owner, ContentRootKind::FixedBody, Provenance::Unknown);
        let paragraph = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
        let first = builder.push_text(
            fixed,
            "a".to_owned(),
            None,
            ContentStyle::default(),
            None,
            None,
            Provenance::Unknown,
        );
        let later = builder.push_text(
            paragraph,
            "b".to_owned(),
            None,
            ContentStyle::default(),
            None,
            None,
            Provenance::Unknown,
        );
        let mut store = builder.finish();
        store.fixed_views.push(FixedView {
            key: FixedViewKey::FIRST,
            owner,
            lines: vec![FixedLine {
                key: FixedLineKey::FIRST,
                terminal_columns: 1,
                placements: vec![Placement {
                    key: PlacementKey::FIRST,
                    target: PlacementTarget::Content(ContentRef {
                        atom: later.atom,
                        bytes: ContentByteRange { start: 0, end: 1 },
                    }),
                    root_scalar_range: 0..1,
                    start_column: 0,
                    end_column: 1,
                    map: CellMapKind::Affine {
                        columns_per_scalar: 1,
                    },
                }],
                decorations: Vec::new(),
            }],
            provenance: Provenance::Unknown,
        });
        let blocks = vec![
            Block::FixedDisplay {
                children: vec![Inline::Text { content: first }],
                view: FixedViewKey::FIRST,
                layout: crate::LayoutHint::default(),
                source: None,
            },
            Block::Paragraph {
                children: vec![Inline::Text { content: later }],
                layout: crate::LayoutHint::default(),
                source: None,
            },
        ];
        let codes = validate_document(&document_with_store(store, Vec::new(), blocks))
            .into_iter()
            .filter_map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>();
        assert!(codes.contains(&"ir.invalid-fixed-view-content".to_owned()));
    }

    fn section(id: &str) -> Section {
        Section {
            id: id.into(),
            fragment_aliases: Vec::new(),
            heading: crate::Heading {
                content: Vec::new(),
                source: None,
            },
            spacing_before_lines: 0,
            blocks: Vec::new(),
            children: Vec::new(),
            source: None,
        }
    }

    #[test]
    fn reports_duplicate_and_empty_section_identities() {
        let diagnostics = validate_document(&document(
            vec![section(""), section("duplicate"), section("duplicate")],
            Vec::new(),
        ));
        let codes = diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.code.as_deref())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"ir.empty-identity"));
        assert!(codes.contains(&"ir.duplicate-identity"));
    }

    #[test]
    fn accepts_links_to_sections_and_inline_anchors() {
        let mut fixture = crate::test_support::ContentFixture::body();
        let anchor = fixture.anchor("anchor");
        let section_link = fixture.link_text(
            LinkTarget::Section {
                id: "section".into(),
            },
            None,
            "section",
            false,
        );
        let anchor_link = fixture.link_text(
            LinkTarget::Section {
                id: "anchor".into(),
            },
            None,
            "anchor",
            false,
        );
        let missing_link = fixture.link_text(
            LinkTarget::Section {
                id: "missing".into(),
            },
            None,
            "missing",
            false,
        );
        let blocks = vec![Block::Paragraph {
            children: vec![anchor, section_link, anchor_link, missing_link],
            layout: LayoutHint::default(),
            source: None,
        }];
        let diagnostics = validate_document(&document_with_store(
            fixture.finish(),
            vec![section("section")],
            blocks,
        ));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code.as_deref(),
            Some("ir.dangling-section-link")
        );
    }

    #[test]
    fn zero_width_target_must_stay_at_its_root_scalar_boundary() {
        let mut fixture = crate::test_support::ContentFixture::body();
        let anchor = fixture.anchor("target");
        let text = fixture.text("prefix");
        let store = fixture.finish();
        crate::validate_content_store(&store).unwrap();
        let blocks = vec![Block::Paragraph {
            children: vec![text, anchor],
            layout: LayoutHint::default(),
            source: None,
        }];
        let codes = validate_document(&document_with_store(store, Vec::new(), blocks))
            .into_iter()
            .filter_map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>();
        assert!(
            codes
                .iter()
                .any(|code| code == "ir.invalid-content-reference")
        );
    }

    #[test]
    fn point_only_root_can_precede_visible_content_in_one_inline_container() {
        let mut builder = crate::ContentStoreBuilder::new();
        let point_owner =
            builder.push_owner(crate::ContentOwnerKind::Content, crate::Provenance::Unknown);
        let point_root = builder.push_root(
            point_owner,
            crate::ContentRootKind::Body,
            crate::Provenance::Unknown,
        );
        let text_owner =
            builder.push_owner(crate::ContentOwnerKind::Content, crate::Provenance::Unknown);
        let text_root = builder.push_root(
            text_owner,
            crate::ContentRootKind::Body,
            crate::Provenance::Unknown,
        );
        let point = builder.push_point(
            point_root,
            crate::PointBoundary::BetweenAtoms { atom_boundary: 0 },
            0,
            crate::Provenance::Unknown,
        );
        let text = builder.push_text(
            text_root,
            "BODY".to_owned(),
            None,
            crate::ContentStyle::default(),
            None,
            None,
            crate::Provenance::Unknown,
        );
        let store = builder.finish();
        crate::validate_content_store(&store).unwrap();
        let blocks = vec![Block::Paragraph {
            children: vec![
                Inline::anchor(point, "target"),
                Inline::Text { content: text },
            ],
            layout: LayoutHint::default(),
            source: None,
        }];
        let diagnostics = validate_document(&document_with_store(store, Vec::new(), blocks));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn empty_link_wrapper_cannot_borrow_another_fragment_occurrence() {
        let mut fixture = crate::test_support::ContentFixture::body();
        let linked = fixture.link_text(
            LinkTarget::External {
                uri: "https://example.test/".into(),
            },
            None,
            "label",
            false,
        );
        let Inline::Link { occurrence, .. } = &linked else {
            panic!("fixture creates a link");
        };
        let borrowed = Inline::Link {
            occurrence: *occurrence,
            children: Vec::new(),
        };
        let blocks = vec![Block::Paragraph {
            children: vec![linked, borrowed],
            layout: LayoutHint::default(),
            source: None,
        }];
        let codes = validate_document(&document_with_store(fixture.finish(), Vec::new(), blocks))
            .into_iter()
            .filter_map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>();
        assert!(
            codes
                .iter()
                .any(|code| code == "ir.invalid-content-reference")
        );
    }

    #[test]
    fn fragment_aliases_keep_source_spelling_but_must_resolve_uniquely() {
        let mut first = section("first");
        first.fragment_aliases = vec!["Mixed.Target".into(), "--option".into()];
        let diagnostics = validate_document(&document(vec![first.clone()], Vec::new()));
        assert!(diagnostics.is_empty());

        let mut second = section("second");
        second.fragment_aliases = vec!["Mixed.Target".into(), "bad fragment".into()];
        let diagnostics = validate_document(&document(vec![first, second], Vec::new()));
        let codes = diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.code.as_deref())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"ir.invalid-fragment-alias"));
        assert!(codes.contains(&"ir.ambiguous-fragment-alias"));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn reports_invalid_ids_role_collisions_ranges_tables_and_uris() {
        let mut fixture = crate::test_support::ContentFixture::body();
        let source = SourceSpan {
            source: SourceKey::FIRST,
            byte_range: Some(TextRange {
                start: TextSize::new(9),
                end: TextSize::new(3),
            }),
            line: 0,
            column: 0,
            end_line: Some(0),
            end_column: Some(0),
        };
        let shared: NodeId = "Bad ID".into();
        let shared_anchor = fixture.anchor(shared.clone());
        let external = fixture.empty_link(
            LinkTarget::External {
                uri: "relative target".to_owned(),
            },
            None,
        );
        let email = fixture.empty_link(
            LinkTarget::Email {
                address: "missing-domain".to_owned(),
            },
            None,
        );
        let section = Section {
            id: shared.clone(),
            fragment_aliases: Vec::new(),
            heading: crate::Heading {
                content: Vec::new(),
                source: None,
            },
            spacing_before_lines: 0,
            blocks: vec![Block::DefinitionList {
                declaration_groups: Vec::new(),
                items: vec![DefinitionItem {
                    source: None,
                    entry: Some(EntryFacts {
                        name_bindings: Vec::new(),
                        alias_groups: Vec::new(),
                        alias_of: None,
                        forms: Vec::new(),
                        id: shared.clone(),
                        kind: EntryKind::Term,
                        case: NameCase::Sensitive,
                        names: vec!["term".to_owned()],
                        value_domain: None,
                    }),
                    terms: vec![vec![shared_anchor]],
                    description: Vec::new(),
                    layout: crate::DefinitionLayout {
                        inline_term: false,
                        spacing_before_lines: None,
                        ..Default::default()
                    },
                }],
                compact: true,
                layout: LayoutHint::default(),
                source: Some(source),
            }],
            children: Vec::new(),
            source: None,
        };
        let blocks = vec![
            Block::Paragraph {
                children: vec![external, email],
                layout: LayoutHint::default(),
                source: None,
            },
            Block::Table {
                fixed_view: None,
                rows: vec![TableRow {
                    kind: crate::TableRowKind::Data,
                    cells: vec![TableCell {
                        kind: crate::TableCellKind::Text,
                        blocks: Vec::new(),
                        point: None,
                        column_span: 0,
                        row_span: 0,
                        alignment: None,
                        source: None,
                    }],
                }],
                layout: LayoutHint::default(),
                source: None,
            },
        ];

        let diagnostics = validate_document(&document_with_store(
            fixture.finish(),
            vec![section],
            blocks,
        ));
        let codes = diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.code.as_deref())
            .collect::<Vec<_>>();
        for expected in [
            "ir.invalid-identity",
            "ir.identity-role-collision",
            "ir.invalid-source-position",
            "ir.reverse-source-range",
            "ir.invalid-table-span",
            "ir.invalid-external-uri",
            "ir.invalid-email-address",
        ] {
            assert!(codes.contains(&expected), "missing {expected}: {codes:?}");
        }
    }

    #[test]
    fn rejects_rule_rows_with_data_or_without_layout_strengths() {
        let blocks = vec![Block::Table {
            fixed_view: None,
            rows: vec![
                TableRow {
                    kind: crate::TableRowKind::HorizontalRule,
                    cells: vec![TableCell {
                        kind: crate::TableCellKind::Text,
                        blocks: Vec::new(),
                        point: None,
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                        source: None,
                    }],
                },
                TableRow {
                    kind: crate::TableRowKind::LayoutRule { cells: Vec::new() },
                    cells: Vec::new(),
                },
                TableRow {
                    kind: crate::TableRowKind::LayoutRule {
                        cells: vec![crate::TableRuleCellKind::Horizontal],
                    },
                    cells: vec![TableCell {
                        kind: crate::TableCellKind::DoubleHorizontalRule,
                        blocks: Vec::new(),
                        point: None,
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                        source: None,
                    }],
                },
                TableRow {
                    kind: crate::TableRowKind::Data,
                    cells: vec![TableCell {
                        kind: crate::TableCellKind::HorizontalRule,
                        blocks: vec![Block::Paragraph {
                            children: Vec::new(),
                            layout: LayoutHint::default(),
                            source: None,
                        }],
                        point: None,
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                        source: None,
                    }],
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        }];
        let diagnostics = validate_document(&document(Vec::new(), blocks));
        let codes = diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.code.as_deref())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"ir.invalid-table-rule-cells"), "{codes:?}");
        assert!(codes.contains(&"ir.empty-table-layout-rule"), "{codes:?}");
        assert!(codes.contains(&"ir.invalid-table-layout-rule"), "{codes:?}");
        assert!(
            codes.contains(&"ir.invalid-table-rule-content"),
            "{codes:?}"
        );
    }

    #[test]
    fn validates_external_uri_structure() {
        for uri in [
            "https:relative",
            "https:///missing-host",
            "https://example.test:",
            "https://[::1",
            "https://[::1]:invalid",
            "https://%ZZ@example.test/path",
            "https://example.test/%ZZ",
            "https://user]name@example.test/path",
            "https://example%ZZ.test/path",
            "https://example..test/path",
            "https://例.example/path",
            "https://example.test/path#one#two",
            "mailto:",
            "mailto:?subject=x",
            "mailto:a..b@example.test",
            "mailto:.a@example.test",
            "mailto:a.@example.test",
            "mailto:user%ZZ@example.test",
            "mailto:%2Euser@example.test",
            "mailto:user%2E%2Ename@example.test",
            "mailto:user%40evil@example.test",
            "mailto:user%2Csecond@example.test",
            "mailto:%80@example.test",
            "mailto:%2Euser@example.test?subject=x",
            "mailto:user%2E%2Ename@example.test?subject=x",
            "mailto:user%40evil@example.test?subject=x",
            "mailto:%2Euser@example.test#fragment",
        ] {
            assert!(!is_valid_external_uri(uri), "accepted invalid URI {uri}");
        }
        for uri in [
            "https://example.test/path",
            "https://user@example.test:443/path",
            "https://user%40name@example.test/path",
            "https://[::1]:8443/path",
            "https://[::1]:8443/path?q=x#part",
            "https://service_name.example.test/path",
            "https://example.test./path",
            "https://ex%41mple.test/path",
            "https://xn--fsq.example/path",
            "mailto:user@example.test",
            "mailto:user@example.test?subject=hello",
            "mailto:user%25tag@example.test",
            "mailto:a%2Fb@example.test",
            "mailto:user@example.test,second@example.test",
            "mailto:user%252Etag@example.test",
        ] {
            assert!(is_valid_external_uri(uri), "rejected valid URI {uri}");
        }
    }

    #[test]
    fn validates_email_and_mailto_round_trips() {
        for address in [
            "",
            "missing-domain",
            "@example.test",
            "docs@",
            ".docs@example.test",
            "docs.@example.test",
            "docs..team@example.test",
            "quoted\"name@example.test",
        ] {
            assert!(
                !is_valid_email_address(address),
                "accepted invalid email address {address}"
            );
        }
        for address in [
            "docs@example.test",
            "support@sub.example.test",
            "build+notifications@example.test",
        ] {
            assert!(
                is_valid_email_address(address),
                "rejected valid email address {address}"
            );
        }

        for (uri, address) in [
            ("mailto:docs@example.test", "docs@example.test"),
            ("MAILTO:user%25tag@example.test", "user%tag@example.test"),
            ("mailto:a%2Fb@example.test", "a/b@example.test"),
            (
                "mailto:user%252Etag@example.test",
                "user%2Etag@example.test",
            ),
        ] {
            let (_, remainder) = uri.split_once(':').expect("mailto URI has a scheme");
            let canonical_uri = format!("mailto:{remainder}");
            assert_eq!(
                email_address_from_mailto_uri(uri).as_deref(),
                Some(address),
                "failed to decode {uri}"
            );
            assert_eq!(
                mailto_uri_for_email_address(address).as_deref(),
                Some(canonical_uri.as_str()),
                "failed to serialize {address}"
            );
        }
        for uri in [
            "mailto:%2Euser@example.test",
            "mailto:user%2E%2Ename@example.test",
            "mailto:user%40evil@example.test",
            "mailto:user%2Csecond@example.test",
            "mailto:user@example.test,second@example.test",
            "mailto:user@example.test?subject=x",
            "mailto:user@example.test#fragment",
        ] {
            assert!(
                email_address_from_mailto_uri(uri).is_none(),
                "classified non-typed mailto URI {uri}"
            );
        }
    }

    #[test]
    fn validates_source_spans_owned_by_document_diagnostics() {
        let source = SourceSpan {
            source: SourceKey::FIRST,
            byte_range: Some(TextRange {
                start: TextSize::new(8),
                end: TextSize::new(3),
            }),
            line: 0,
            column: 0,
            end_line: Some(0),
            end_column: Some(0),
        };
        let mut document = document(Vec::new(), Vec::new());
        document.diagnostics.push(Diagnostic {
            impact: crate::DiagnosticImpact::None,
            level: DiagnosticLevel::Warning,
            code: Some("producer.finding".to_owned()),
            message: "producer finding".to_owned(),
            source: Some(source),
            source_key: None,
            coverage_scope: None,
        });

        let codes = validate_document(&document)
            .into_iter()
            .filter_map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>();
        assert!(
            codes
                .iter()
                .any(|code| code == "ir.invalid-source-position")
        );
        assert!(codes.iter().any(|code| code == "ir.reverse-source-range"));
    }

    #[test]
    fn source_only_diagnostic_identity_is_closed_without_an_authored_position() {
        let mut document = document(Vec::new(), Vec::new());
        document.diagnostics.push(Diagnostic {
            impact: crate::DiagnosticImpact::None,
            level: DiagnosticLevel::Warning,
            code: Some("producer.source-only".to_owned()),
            message: "expanded source coordinates are not authored".to_owned(),
            source: None,
            source_key: Some(SourceKey::FIRST),
            coverage_scope: None,
        });
        crate::validate_document_sources(&document).unwrap();
        let wire = serde_json::to_value(&document).unwrap();
        assert_eq!(wire["diagnostics"][0]["sourceKey"], 1);
        assert!(wire["diagnostics"][0].get("source").is_none());
        assert_eq!(serde_json::from_value::<Document>(wire).unwrap(), document);

        let diagnostic = &mut document.diagnostics[0];
        diagnostic.source = Some(SourceSpan {
            source: SourceKey::FIRST,
            byte_range: None,
            line: 1,
            column: 1,
            end_line: None,
            end_column: None,
        });
        assert!(crate::validate_document_sources(&document).is_err());
        document.diagnostics[0].source = None;
        document.diagnostics[0].source_key = SourceKey::new(2);
        assert!(crate::validate_document_sources(&document).is_err());
    }

    #[test]
    fn in_memory_flow_document_rejects_native_mark_coverage_scopes() {
        for scope in [
            CoverageScope::Section {
                key: std::num::NonZeroU32::MIN,
            },
            CoverageScope::Owner {
                key: std::num::NonZeroU32::MIN,
            },
            CoverageScope::Region {
                key: std::num::NonZeroU32::MIN,
            },
        ] {
            let mut document = document(Vec::new(), Vec::new());
            document.diagnostics.push(Diagnostic {
                impact: crate::DiagnosticImpact::None,
                level: DiagnosticLevel::Unsupported,
                code: Some("annotated.coverage.owner.unverified".to_owned()),
                message: "native mark unavailable".to_owned(),
                source: None,
                source_key: None,
                coverage_scope: Some(scope),
            });
            let findings = validate_document(&document);
            assert!(findings.iter().any(|finding| {
                finding.code.as_deref() == Some("ir.invalid-coverage-scope")
                    && finding.impact == crate::DiagnosticImpact::SemanticCoverage
            }));
            let mut merged = document.diagnostics;
            merged.extend(findings);
            assert!(!crate::semantics_complete(&merged));
        }
    }

    #[test]
    fn invalid_source_coverage_scope_cannot_leave_in_memory_semantics_complete() {
        let mut document = document(Vec::new(), Vec::new());
        document.diagnostics.push(Diagnostic {
            impact: crate::DiagnosticImpact::None,
            level: DiagnosticLevel::Unsupported,
            code: None,
            message: "source binding unavailable".to_owned(),
            source: None,
            source_key: None,
            coverage_scope: Some(CoverageScope::Source {
                key: SourceKey::new(2).unwrap(),
            }),
        });
        let findings = validate_document(&document);
        assert!(findings.iter().any(|finding| {
            finding.code.as_deref() == Some("ir.invalid-coverage-source")
                && finding.impact == crate::DiagnosticImpact::SemanticCoverage
        }));
        let mut merged = document.diagnostics;
        merged.extend(findings);
        assert!(!crate::semantics_complete(&merged));
    }

    #[test]
    fn reports_invalid_cross_document_entry_domains() {
        let mut fixture = crate::test_support::ContentFixture::body();
        let anchor = fixture.anchor("option-output");
        let store = fixture.finish();
        let mut definition = DefinitionItem {
            source: None,
            entry: Some(EntryFacts {
                name_bindings: Vec::new(),
                alias_groups: Vec::new(),
                alias_of: None,
                forms: Vec::new(),
                id: "option-output".into(),
                kind: EntryKind::Parameter {
                    parameter_kind: crate::ParameterKind::Option,
                },
                case: NameCase::Sensitive,
                names: vec!["--output".to_owned()],
                value_domain: Some(crate::ValueDomain::EntrySet {
                    reference: crate::DocumentReference::Manual {
                        name: String::new(),
                        manual_section: Some(String::new()),
                    },
                    entry_kinds: Vec::new(),
                    source: None,
                }),
            }),
            terms: vec![vec![anchor]],
            description: Vec::new(),
            layout: crate::DefinitionLayout {
                inline_term: false,
                spacing_before_lines: None,
                ..Default::default()
            },
        };
        let blocks = vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![definition.clone()],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }];
        let diagnostics =
            validate_document(&document_with_store(store.clone(), Vec::new(), blocks));
        let codes = diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.code.as_deref())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"ir.empty-semantic-document-reference"));
        assert!(codes.contains(&"ir.empty-entry-value-domain"));

        definition.entry.as_mut().expect("identity").value_domain =
            Some(crate::ValueDomain::EntrySet {
                reference: crate::DocumentReference::Manual {
                    name: "ssh_config".to_owned(),
                    manual_section: Some("qgroup".to_owned()),
                },
                entry_kinds: vec![
                    crate::EntryKind::ConfigurationKey,
                    crate::EntryKind::ConfigurationKey,
                ],
                source: None,
            });
        let diagnostics = validate_document(&document_with_store(
            store,
            Vec::new(),
            vec![Block::DefinitionList {
                declaration_groups: Vec::new(),
                items: vec![definition],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }],
        ));
        let codes = diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.code.as_deref())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"ir.invalid-semantic-document-reference"));
        assert!(codes.contains(&"ir.duplicate-entry-value-kind"));
    }

    #[test]
    fn classifies_only_semantic_invariant_diagnostics_as_incomplete() {
        for code in [
            "ir.invalid-identity",
            "ir.ambiguous-fragment-alias",
            "ir.invalid-semantic-document-reference",
            "ir.empty-entry-value-domain",
        ] {
            assert!(is_semantic_completeness_diagnostic(code), "{code}");
        }
        for code in [
            "ir.invalid-table-span",
            "ir.invalid-source-position",
            "ir.invalid-external-uri",
        ] {
            assert!(!is_semantic_completeness_diagnostic(code), "{code}");
        }
    }
}
