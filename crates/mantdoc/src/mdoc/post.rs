use super::{
    DocumentBuilder, NodeId, NodeKind, Recovery, StructureOutcome, emphasis_fallback_elements,
    first_mdoc_content_node, mark_definition_item_xo_head_targets, mark_emphasis_targets,
    mark_section_targets, mark_synopsis_pretty, mark_unique_function_targets, node_kind_name,
    normalize_inline_paragraph_controls, normalize_list_trailing_paragraph_controls,
    normalize_section_paragraph_boundaries, normalize_trailing_no_space_in_implicit_blocks,
    paragraph_layout_recovery_offset, rebase_option_expansion_locations,
};

/// Merge syntax findings discovered by nested structure passes back into
/// their source-order position without sorting semantic validation findings.
pub(super) fn merge_syntax_recoveries(
    builder: &DocumentBuilder,
    outcome: &mut StructureOutcome,
    recoveries: Vec<Recovery>,
) {
    for recovery in recoveries {
        let line = match &recovery {
            Recovery::BadlyNestedBlock { location, .. } => location
                .as_ref()
                .and_then(|span| builder.source_position(span))
                .map_or(u32::MAX, |position| position.line),
            _ => unreachable!("syntax-stage findings are crossed blocks"),
        };
        let index = outcome
            .recoveries
            .iter()
            .enumerate()
            .find_map(|(index, existing)| match existing {
                Recovery::BadlyNestedBlock { location, .. }
                    if location
                        .as_ref()
                        .and_then(|span| builder.source_position(span))
                        .is_some_and(|position| position.line > line) =>
                {
                    Some(index)
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                outcome
                    .recoveries
                    .iter()
                    .rposition(|existing| matches!(existing, Recovery::BadlyNestedBlock { .. }))
                    .map_or(0, |index| index + 1)
            });
        outcome.recoveries.insert(index, recovery);
    }
}

/// Inputs consumed by the root-level mdoc post-validation pass.
pub(super) struct PostValidation<'a> {
    pub(super) builder: &'a mut DocumentBuilder,
    pub(super) root: NodeId,
    pub(super) root_children: &'a [NodeId],
    pub(super) outcome: &'a mut StructureOutcome,
    pub(super) synopsis_bodies: &'a [NodeId],
    pub(super) target_heads: &'a [NodeId],
    pub(super) automatic_function_targets: &'a [(NodeId, String, bool)],
    pub(super) automatic_function_tag_occurrences: &'a [String],
    pub(super) prologue: PrologueStatus,
    pub(super) netbsd: NetBsdValidation,
}

#[derive(Clone, Copy)]
pub(super) struct PrologueStatus {
    pub(super) saw_title: bool,
    pub(super) saw_date: bool,
    pub(super) saw_operating_system_request: bool,
}

#[derive(Clone, Copy)]
pub(super) struct NetBsdValidation {
    pub(super) enabled: bool,
    pub(super) saw_rcs_id: bool,
}

impl PostValidation<'_> {
    /// Freeze root topology, normalize paragraph boundaries, assign targets,
    /// and emit document-wide metadata findings in legacy order.
    pub(super) fn run(self) {
        let Self {
            builder,
            root,
            root_children,
            outcome,
            synopsis_bodies,
            target_heads,
            automatic_function_targets,
            automatic_function_tag_occurrences,
            prologue,
            netbsd,
        } = self;

        let _ = builder.replace_children(root, root_children);
        normalize_trailing_no_space_in_implicit_blocks(builder, root);
        let mut paragraph_layout_recoveries = Vec::new();
        normalize_list_trailing_paragraph_controls(builder, root, &mut paragraph_layout_recoveries);
        normalize_inline_paragraph_controls(builder, root, &mut paragraph_layout_recoveries);
        paragraph_layout_recoveries.sort_by_key(paragraph_layout_recovery_offset);
        outcome.recoveries.extend(paragraph_layout_recoveries);

        let emphasis_elements = emphasis_fallback_elements(builder);
        mark_emphasis_targets(builder, &emphasis_elements);
        let mut section_paragraph_recoveries = Vec::new();
        normalize_section_paragraph_boundaries(builder, root, &mut section_paragraph_recoveries);

        builder.metadata_mut().has_body = root_children
            .iter()
            .copied()
            .any(|node| builder.node_kind(node) != Some(NodeKind::Comment));
        if !prologue.saw_title {
            let metadata = builder.metadata_mut();
            metadata.title = Some("UNTITLED".into());
            if metadata.volume.is_none() {
                metadata.volume = Some("LOCAL".into());
            }
            outcome.recoveries.push(Recovery::MissingTitle);
        }
        let final_root_children = builder.children(root).unwrap_or_default();
        match first_mdoc_content_node(builder, final_root_children) {
            Some(node) if builder.node_macro_name(node) != Some("Sh") => {
                let content = builder
                    .node_macro_name(node)
                    .unwrap_or_else(|| node_kind_name(builder.node_kind(node)))
                    .into();
                outcome
                    .recoveries
                    .push(Recovery::ContentBeforeFirstSection {
                        content,
                        location: builder.node_location(node),
                    });
            }
            None => outcome.recoveries.push(Recovery::NoDocumentBody),
            Some(_) => {}
        }

        rebase_option_expansion_locations(builder, root);
        for body in synopsis_bodies {
            mark_synopsis_pretty(builder, *body);
        }
        mark_definition_item_xo_head_targets(builder);
        mark_section_targets(builder, target_heads);
        mark_unique_function_targets(
            builder,
            automatic_function_targets,
            automatic_function_tag_occurrences,
        );
        outcome.recoveries.extend(section_paragraph_recoveries);
        if !prologue.saw_operating_system_request && (prologue.saw_date || prologue.saw_title) {
            outcome.recoveries.push(Recovery::MissingOperatingSystem);
            builder.operating_system("");
        }
        if netbsd.enabled && !netbsd.saw_rcs_id {
            outcome
                .recoveries
                .push(Recovery::RcsIdMissing { flavour: "NetBSD" });
        }
    }
}
