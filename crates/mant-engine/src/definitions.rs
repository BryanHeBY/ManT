//! Identifies addressable semantic entries after source-specific lowering.
//!
//! Both libmandoc macro sets and Markdown produce definition lists. This
//! pass assigns one canonical option identity without leaking source macros
//! into the stable document contract.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt::Write as _,
    mem,
};

use mant_ir::{
    Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, Inline, LayoutHint,
    Section, SourceSpan,
    visit::{self, Visit},
};
use sha2::{Digest, Sha256};

use crate::{
    block::{block_layout, block_layout_mut},
    inline::{DEFAULT_INLINE_TERM_MAX_WIDTH, plain_text, terms_fit_inline},
};

/// Annotate reliably recognizable command-line options and return every
/// inline anchor that the navigation resolver must retain.
pub(crate) fn identify_definitions(
    blocks: &mut Vec<Block>,
    sections: &mut [Section],
    reserved_targets: &HashSet<String>,
    document_name: Option<&str>,
) -> HashSet<String> {
    let mut preferred_counts = HashMap::new();
    let root_context = document_name.map_or(DefinitionContext::Generic, |name| {
        let name = name.to_ascii_lowercase();
        if name.ends_with("_config") || name.ends_with("-config") {
            DefinitionContext::ConfigurationKeys
        } else {
            DefinitionContext::Generic
        }
    });
    prepare_blocks(blocks, root_context, &mut preferred_counts);
    prepare_sections(sections, root_context, &mut preferred_counts);

    let mut used = HashSet::new();
    let mut retained = HashSet::new();
    identify_blocks(
        blocks,
        root_context,
        &mut used,
        reserved_targets,
        &mut retained,
        &preferred_counts,
    );
    for section in sections {
        let context = DefinitionContext::for_section(&section.title, root_context);
        identify_blocks(
            &mut section.blocks,
            context,
            &mut used,
            reserved_targets,
            &mut retained,
            &preferred_counts,
        );
        identify_sections(
            &mut section.children,
            context,
            &mut used,
            reserved_targets,
            &mut retained,
            &preferred_counts,
        );
    }
    retained
}

/// One identified definition together with its semantic coordinates.
///
/// `indices` contains the one-based position at every entry nesting level.
/// Keeping these coordinates beside the borrowed item gives outline,
/// excerpt, and addressable-Markdown projections one topology instead of
/// independently flattening definition descriptions.
pub(crate) struct DefinitionEntry<'a> {
    pub(crate) item: &'a DefinitionItem,
    pub(crate) source: Option<SourceSpan>,
    pub(crate) indices: Vec<usize>,
    pub(crate) ancestors: Vec<&'a DefinitionItem>,
}

/// Return identified definition items in semantic pre-order.
///
/// Definitions nested inside lists or table cells remain direct entries of
/// the surrounding scope. Definitions inside an entry description become
/// children of that entry, matching [`mant_ir::SemanticIndex`].
pub(crate) fn definition_entries(blocks: &[Block]) -> Vec<DefinitionEntry<'_>> {
    let mut entries = Vec::new();
    collect_definition_scope(blocks, &[], &mut Vec::new(), &mut entries);
    entries
}

/// Report definition-shaped native content that a semantic section could not
/// classify without guessing.
pub(crate) fn manual_discovery_diagnostics(sections: &[Section]) -> Vec<mant_ir::Diagnostic> {
    let mut diagnostics = Vec::new();
    visit_manual_discovery_sections(sections, DefinitionContext::Generic, &mut diagnostics);
    diagnostics
}

fn visit_manual_discovery_sections(
    sections: &[Section],
    parent_context: DefinitionContext,
    output: &mut Vec<mant_ir::Diagnostic>,
) {
    for section in sections {
        let context = DefinitionContext::for_section(&section.title, parent_context);
        visit_manual_discovery_blocks(&section.blocks, context, true, output);
        visit_manual_discovery_sections(&section.children, context, output);
    }
}

fn visit_manual_discovery_blocks(
    blocks: &[Block],
    context: DefinitionContext,
    report_unclassified: bool,
    output: &mut Vec<mant_ir::Diagnostic>,
) {
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    visit_manual_discovery_blocks(
                        &item.blocks,
                        context,
                        report_unclassified,
                        output,
                    );
                }
            }
            Block::DefinitionList {
                items,
                layout,
                source,
                ..
            } => visit_manual_definition_items(
                items,
                *layout,
                *source,
                context,
                report_unclassified,
                output,
            ),
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    visit_manual_discovery_blocks(
                        &cell.blocks,
                        context,
                        report_unclassified,
                        output,
                    );
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn visit_manual_definition_items(
    items: &[DefinitionItem],
    layout: LayoutHint,
    source: Option<SourceSpan>,
    context: DefinitionContext,
    report_unclassified: bool,
    output: &mut Vec<mant_ir::Diagnostic>,
) {
    let inferred_context =
        if context == DefinitionContext::Generic && is_key_binding_command_group(items) {
            DefinitionContext::Commands
        } else {
            context
        };
    let item_context =
        if inferred_context == DefinitionContext::Commands && layout.indent_columns > 0 {
            DefinitionContext::Parameters
        } else {
            inferred_context
        };
    for item in items {
        let identity = item.identity.as_ref();
        let role = identity.map_or(DefinitionRole::Term, |identity| identity.role);
        if report_unclassified
            && item_context != DefinitionContext::Generic
            && role == DefinitionRole::Term
            && identity.is_some_and(|identity| identity.names.is_empty())
        {
            report_unclassified_definition(item, item_context, source, output);
        }
        visit_manual_discovery_blocks(
            &item.description,
            child_definition_context(role, item_context),
            false,
            output,
        );
    }
}

fn report_unclassified_definition(
    item: &DefinitionItem,
    context: DefinitionContext,
    source: Option<SourceSpan>,
    output: &mut Vec<mant_ir::Diagnostic>,
) {
    let term = item
        .terms
        .first()
        .map_or_else(String::new, |term| plain_text(term));
    if term.trim().is_empty() {
        return;
    }
    output.push(mant_ir::Diagnostic {
        level: mant_ir::DiagnosticLevel::Warning,
        code: Some("manual.semantic-entry.unclassified-definition".to_owned()),
        message: format!(
            "definition term '{}' did not match the complete {} name grammar and remains an unclassified term",
            term.trim(),
            context.label()
        ),
        source,
    });
}

fn collect_definition_scope<'a>(
    blocks: &'a [Block],
    parent_indices: &[usize],
    ancestors: &mut Vec<&'a DefinitionItem>,
    output: &mut Vec<DefinitionEntry<'a>>,
) {
    let mut direct_index = 0;
    collect_direct_definitions(blocks, parent_indices, ancestors, &mut direct_index, output);
}

fn collect_direct_definitions<'a>(
    blocks: &'a [Block],
    parent_indices: &[usize],
    ancestors: &mut Vec<&'a DefinitionItem>,
    direct_index: &mut usize,
    output: &mut Vec<DefinitionEntry<'a>>,
) {
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    collect_direct_definitions(
                        &item.blocks,
                        parent_indices,
                        ancestors,
                        direct_index,
                        output,
                    );
                }
            }
            Block::DefinitionList { items, source, .. } => {
                for item in items {
                    if item.identity.is_none() {
                        continue;
                    }
                    *direct_index += 1;
                    let mut indices = parent_indices.to_vec();
                    indices.push(*direct_index);
                    output.push(DefinitionEntry {
                        item,
                        source: *source,
                        indices: indices.clone(),
                        ancestors: ancestors.clone(),
                    });
                    ancestors.push(item);
                    collect_definition_scope(&item.description, &indices, ancestors, output);
                    ancestors.pop();
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        collect_direct_definitions(
                            &cell.blocks,
                            parent_indices,
                            ancestors,
                            direct_index,
                            output,
                        );
                    }
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn prepare_sections(
    sections: &mut [Section],
    parent_context: DefinitionContext,
    preferred_counts: &mut HashMap<String, usize>,
) {
    for section in sections {
        let context = DefinitionContext::for_section(&section.title, parent_context);
        prepare_blocks(&mut section.blocks, context, preferred_counts);
        prepare_sections(&mut section.children, context, preferred_counts);
    }
}

fn prepare_blocks(
    blocks: &mut Vec<Block>,
    context: DefinitionContext,
    preferred_counts: &mut HashMap<String, usize>,
) {
    normalize_definition_nesting(blocks);
    normalize_hanging_definitions(blocks, context);
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    prepare_blocks(&mut item.blocks, context, preferred_counts);
                }
            }
            Block::DefinitionList { items, layout, .. } => {
                let inferred_context = if context == DefinitionContext::Generic
                    && is_key_binding_command_group(items)
                {
                    DefinitionContext::Commands
                } else {
                    context
                };
                let item_context = if inferred_context == DefinitionContext::Commands
                    && layout.indent_columns > 0
                {
                    DefinitionContext::Parameters
                } else {
                    inferred_context
                };
                for item in items {
                    let plan = identity_plan(item, item_context);
                    *preferred_counts.entry(plan.preferred).or_default() += 1;
                    let child_context = child_definition_context(plan.role, item_context);
                    prepare_blocks(&mut item.description, child_context, preferred_counts);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        prepare_blocks(&mut cell.blocks, context, preferred_counts);
                    }
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn child_definition_context(
    role: DefinitionRole,
    item_context: DefinitionContext,
) -> DefinitionContext {
    match role {
        DefinitionRole::Command => DefinitionContext::Parameters,
        DefinitionRole::Option
        | DefinitionRole::Marker
        | DefinitionRole::Operand
        | DefinitionRole::ConfigurationKey => DefinitionContext::Values,
        DefinitionRole::EnvironmentVariable
        | DefinitionRole::Variable
        | DefinitionRole::Value
        | DefinitionRole::Term => item_context,
    }
}

fn identify_sections(
    sections: &mut [Section],
    parent_context: DefinitionContext,
    used: &mut HashSet<String>,
    reserved: &HashSet<String>,
    retained: &mut HashSet<String>,
    preferred_counts: &HashMap<String, usize>,
) {
    for section in sections {
        let context = DefinitionContext::for_section(&section.title, parent_context);
        identify_blocks(
            &mut section.blocks,
            context,
            used,
            reserved,
            retained,
            preferred_counts,
        );
        identify_sections(
            &mut section.children,
            context,
            used,
            reserved,
            retained,
            preferred_counts,
        );
    }
}

fn identify_blocks(
    blocks: &mut Vec<Block>,
    context: DefinitionContext,
    used: &mut HashSet<String>,
    reserved: &HashSet<String>,
    retained: &mut HashSet<String>,
    preferred_counts: &HashMap<String, usize>,
) {
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    identify_blocks(
                        &mut item.blocks,
                        context,
                        used,
                        reserved,
                        retained,
                        preferred_counts,
                    );
                }
            }
            Block::DefinitionList { items, layout, .. } => {
                let inferred_context = if context == DefinitionContext::Generic
                    && is_key_binding_command_group(items)
                {
                    DefinitionContext::Commands
                } else {
                    context
                };
                let item_context = if inferred_context == DefinitionContext::Commands
                    && layout.indent_columns > 0
                {
                    DefinitionContext::Parameters
                } else {
                    inferred_context
                };
                for item in items {
                    let role = identify_item(
                        item,
                        item_context,
                        used,
                        reserved,
                        retained,
                        preferred_counts,
                    );
                    let child_context = child_definition_context(role, item_context);
                    identify_blocks(
                        &mut item.description,
                        child_context,
                        used,
                        reserved,
                        retained,
                        preferred_counts,
                    );
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        identify_blocks(
                            &mut cell.blocks,
                            context,
                            used,
                            reserved,
                            retained,
                            preferred_counts,
                        );
                    }
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

/// Recognize a definition group whose authored forms are editor commands and
/// optional key bindings.
///
/// Manuals such as Bash group Readline commands under topical headings like
/// "Killing and Yanking" or "Miscellaneous", so a heading-only classifier
/// cannot recover their executable names. Requiring a whole multi-item group
/// of command-name tokens plus at least one recognizable binding keeps this
/// inference narrower than treating arbitrary hyphenated glossary terms as
/// commands.
fn is_key_binding_command_group(items: &[DefinitionItem]) -> bool {
    items.len() > 1
        && items.iter().all(|item| {
            item.terms
                .first()
                .is_some_and(|term| key_binding_command_form(&plain_text(term)).is_some())
        })
        && items.iter().any(|item| {
            item.terms.first().is_some_and(|term| {
                key_binding_command_form(&plain_text(term))
                    .is_some_and(|(_, binding)| binding.is_some())
            })
        })
}

fn key_binding_command_form(value: &str) -> Option<(&str, Option<&str>)> {
    let value = value.trim();
    let split = value
        .char_indices()
        .find(|(_, character)| character.is_whitespace());
    let (name, suffix) = split.map_or((value, ""), |(index, _)| {
        (&value[..index], value[index..].trim())
    });
    let mut characters = name.chars();
    if !characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic())
        || !characters
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return None;
    }
    if suffix.is_empty() {
        return Some((name, None));
    }
    let binding = suffix.strip_prefix('(')?.strip_suffix(')')?.trim();
    (!binding.is_empty() && looks_like_key_binding(binding)).then_some((name, Some(binding)))
}

fn looks_like_key_binding(value: &str) -> bool {
    value
        .split([',', ' '])
        .filter(|part| !part.is_empty() && *part != "usually" && *part != "...")
        .any(|part| {
            part.starts_with("C-")
                || part.starts_with("M-")
                || matches!(
                    part,
                    "TAB" | "Return" | "Newline" | "Rubout" | "ESC" | "<space>"
                )
        })
}

/// Reattach source-neutral indented continuations to their owning definition.
///
/// libmandoc can retain man(7) `.RS` continuations as later sibling blocks
/// whose absolute indentation is greater than the preceding definition list.
/// They remain visually correct in that flat form, but the topology loses the
/// command → parameter → value relationship needed by semantic navigation.
/// Move the run under the last definition and translate its layout to the
/// description's relative coordinate system so rendering is unchanged.
fn normalize_definition_nesting(blocks: &mut Vec<Block>) {
    let mut pending: VecDeque<Block> = mem::take(blocks).into();
    let mut normalized = Vec::with_capacity(pending.len());

    while let Some(mut block) = pending.pop_front() {
        let Some(base_indent) = block_definition_indent(&block) else {
            normalized.push(block);
            continue;
        };
        let Some(last_item) = last_definition_mut(&mut block) else {
            normalized.push(block);
            continue;
        };
        let description_origin = base_indent.saturating_add(4);
        while pending
            .front()
            .is_some_and(|next| block_indent(next) > base_indent)
        {
            let mut nested = pending.pop_front().expect("front exists");
            shift_block_indent(&mut nested, description_origin);
            last_item.description.push(nested);
        }
        normalized.push(block);
    }

    *blocks = normalized;
}

fn block_definition_indent(block: &Block) -> Option<u16> {
    match block {
        Block::DefinitionList { layout, .. } => Some(layout.indent_columns),
        _ => None,
    }
}

fn last_definition_mut(block: &mut Block) -> Option<&mut DefinitionItem> {
    match block {
        Block::DefinitionList { items, .. } => items.last_mut(),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DefinitionContext {
    Generic,
    Parameters,
    Commands,
    EnvironmentVariables,
    Variables,
    ConfigurationKeys,
    Values,
}

impl DefinitionContext {
    const fn label(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Parameters => "parameter",
            Self::Commands => "command",
            Self::EnvironmentVariables => "environment-variable",
            Self::Variables => "variable",
            Self::ConfigurationKeys => "configuration-key",
            Self::Values => "value",
        }
    }

    fn for_section(title: &str, inherited: Self) -> Self {
        let normalized = title
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character.to_ascii_uppercase()
                } else {
                    ' '
                }
            })
            .collect::<String>();
        let words = normalized.split_whitespace().collect::<Vec<_>>();
        // Composite headings describe the more specific syntax family.  In
        // particular, "ENVIRONMENT OPTIONS" documents command-line options
        // whose defaults happen to come from the environment; it is not a
        // declaration list of environment-variable names.
        if words.contains(&"OPTIONS")
            || words.contains(&"OPTION")
            || words.contains(&"SWITCHES")
            || words.contains(&"FLAGS")
        {
            return Self::Parameters;
        }
        if words.contains(&"ENVIRONMENT") || words.contains(&"ENVIRONMENTS") {
            return Self::EnvironmentVariables;
        }
        if words.contains(&"VARIABLES") || words.contains(&"VARIABLE") {
            return Self::Variables;
        }
        if normalized.trim() == "COMMANDS"
            || normalized.contains("BUILTIN COMMANDS")
            || normalized.contains("SUBCOMMANDS")
        {
            return Self::Commands;
        }
        if normalized.contains("CONFIGURATION") || normalized.trim() == "KEYWORDS" {
            return Self::ConfigurationKeys;
        }
        inherited
    }
}

/// Turn renderer-neutral hanging-indent runs into semantic definitions.
///
/// Some man(7) generators use `.PP` followed by `.RS` instead of `.TP` for
/// options and environment variables. Native parsers correctly retain that
/// layout, but neither representation is a definition list on its own.
/// Recognising the shared visible shape here keeps identity independent of
/// the source macro set or source parser used by the query pipeline.
fn normalize_hanging_definitions(blocks: &mut Vec<Block>, context: DefinitionContext) {
    let mut pending: VecDeque<Block> = mem::take(blocks).into();
    let mut normalized = Vec::with_capacity(pending.len());

    while let Some(block) = pending.pop_front() {
        let Some(term_indent) = hanging_term_indent(&block, context) else {
            normalized.push(block);
            continue;
        };

        let mut description = Vec::new();
        while let Some(next) = pending.front() {
            if hanging_term_indent(next, context) == Some(term_indent) {
                break;
            }
            if matches!(next, Block::VerticalSpace { .. }) {
                if description.is_empty() {
                    break;
                }
                description.push(pending.pop_front().expect("front exists"));
                continue;
            }
            if block_indent(next) <= term_indent {
                break;
            }
            description.push(pending.pop_front().expect("front exists"));
        }

        if description.is_empty() {
            normalized.push(block);
            continue;
        }

        let Block::Paragraph {
            children,
            layout,
            source,
        } = block
        else {
            unreachable!("option_term_indent only accepts paragraphs");
        };
        let description_origin = term_indent.saturating_add(4);
        for child in &mut description {
            shift_block_indent(child, description_origin);
        }
        let terms = vec![children];
        normalized.push(Block::DefinitionList {
            items: vec![DefinitionItem {
                identity: None,
                inline_term: terms_fit_inline(&terms, DEFAULT_INLINE_TERM_MAX_WIDTH),
                terms,
                description,
                spacing_before_lines: Some(layout.spacing_before_lines),
            }],
            compact: true,
            layout: LayoutHint {
                indent_columns: term_indent,
                spacing_before_lines: 0,
            },
            source,
        });
    }

    *blocks = normalized;
}

fn hanging_term_indent(block: &Block, context: DefinitionContext) -> Option<u16> {
    let Block::Paragraph {
        children, layout, ..
    } = block
    else {
        return None;
    };
    let recognized = match context {
        DefinitionContext::EnvironmentVariables => {
            !environment_names_from_terms(std::slice::from_ref(children)).is_empty()
        }
        DefinitionContext::Generic | DefinitionContext::Parameters => {
            let text = plain_text(children);
            text.trim_start().starts_with('-')
                && !option_names_from_terms(std::slice::from_ref(children)).is_empty()
        }
        DefinitionContext::Commands
        | DefinitionContext::Variables
        | DefinitionContext::ConfigurationKeys
        | DefinitionContext::Values => false,
    };
    recognized.then_some(layout.indent_columns)
}

fn block_indent(block: &Block) -> u16 {
    block_layout(block).map_or(0, |layout| layout.indent_columns)
}

fn shift_block_indent(block: &mut Block, origin: u16) {
    if let Some(layout) = block_layout_mut(block) {
        layout.indent_columns = layout.indent_columns.saturating_sub(origin);
    }
}

struct IdentityPlan {
    role: DefinitionRole,
    case: DefinitionCase,
    names: Vec<String>,
    preferred: String,
}

fn identity_plan(item: &DefinitionItem, context: DefinitionContext) -> IdentityPlan {
    let (role, case, names) = item.identity.as_ref().map_or_else(
        || infer_identity(item, context),
        |identity| (identity.role, identity.case, identity.names.clone()),
    );
    let name = names.first().cloned().unwrap_or_else(|| {
        item.terms
            .first()
            .map_or_else(|| "entry".to_owned(), |term| plain_text(term))
    });
    let preferred = format!("{}-{}", role_id_prefix(role), role_name_slug(role, &name));
    IdentityPlan {
        role,
        case,
        names,
        preferred,
    }
}

fn identify_item(
    item: &mut DefinitionItem,
    context: DefinitionContext,
    used: &mut HashSet<String>,
    reserved: &HashSet<String>,
    retained: &mut HashSet<String>,
    preferred_counts: &HashMap<String, usize>,
) -> DefinitionRole {
    // Native parser anchors are navigation destinations whose formatter tags
    // may contain only the first word of a term. Keep those anchors
    // addressable, but never reuse them as semantic entry IDs: doing so turns
    // `set-mark` into the misleading semantic ID `set`. Markdown producers
    // likewise provide role/name evidence and leave allocation to this pass.
    let IdentityPlan {
        role,
        case,
        names,
        mut preferred,
    } = identity_plan(item, context);

    let mut anchors = Vec::new();
    for term in &item.terms {
        collect_anchor_ids(term, &mut anchors);
    }
    retained.extend(anchors.iter().cloned());

    if preferred_counts
        .get(&preferred)
        .copied()
        .unwrap_or_default()
        > 1
        || reserved.contains(&preferred)
    {
        preferred = format!(
            "{preferred}-{}",
            semantic_fingerprint(item, role, case, &names)
        );
    }
    let id = unique_id(&preferred, used, reserved);
    if !anchors.iter().any(|anchor| anchor == &id)
        && let Some(term) = item.terms.first_mut()
    {
        term.insert(
            0,
            Inline::Anchor {
                id: id.clone().into(),
            },
        );
    }
    retained.insert(id.clone());
    item.identity = Some(DefinitionIdentity {
        id: id.into(),
        role,
        case,
        names,
    });
    role
}

fn infer_identity(
    item: &DefinitionItem,
    context: DefinitionContext,
) -> (DefinitionRole, DefinitionCase, Vec<String>) {
    let first = item
        .terms
        .first()
        .map_or_else(String::new, |term| plain_text(term));
    let trimmed = first.trim();
    match context {
        DefinitionContext::Commands
            if trimmed.starts_with(['-', '+']) || trimmed.starts_with("[-+]") =>
        {
            parameter_identity(item, trimmed)
        }
        DefinitionContext::Commands => {
            let names = command_names(item);
            if names.is_empty() {
                (DefinitionRole::Term, DefinitionCase::Sensitive, Vec::new())
            } else {
                (DefinitionRole::Command, DefinitionCase::Sensitive, names)
            }
        }
        DefinitionContext::EnvironmentVariables => named_identity(
            DefinitionRole::EnvironmentVariable,
            DefinitionCase::Sensitive,
            environment_names(item),
        ),
        DefinitionContext::Variables => named_identity(
            DefinitionRole::Variable,
            DefinitionCase::Sensitive,
            named_term(item, is_variable_term),
        ),
        DefinitionContext::ConfigurationKeys => named_identity(
            DefinitionRole::ConfigurationKey,
            DefinitionCase::Insensitive,
            named_term(item, is_configuration_key),
        ),
        DefinitionContext::Values => named_identity(
            DefinitionRole::Value,
            DefinitionCase::Sensitive,
            named_term(item, is_value_name),
        ),
        DefinitionContext::Parameters => parameter_identity(item, trimmed),
        DefinitionContext::Generic if trimmed.starts_with('-') => parameter_identity(item, trimmed),
        DefinitionContext::Generic => (DefinitionRole::Term, DefinitionCase::Sensitive, Vec::new()),
    }
}

fn named_identity(
    role: DefinitionRole,
    case: DefinitionCase,
    names: Vec<String>,
) -> (DefinitionRole, DefinitionCase, Vec<String>) {
    if names.is_empty() {
        (DefinitionRole::Term, DefinitionCase::Sensitive, names)
    } else {
        (role, case, names)
    }
}

fn is_value_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().all(|character| {
            character.is_alphanumeric()
                || matches!(character, '-' | '_' | '.' | '/' | ':' | '+' | '?')
        })
}

fn parameter_identity(
    item: &DefinitionItem,
    first_term: &str,
) -> (DefinitionRole, DefinitionCase, Vec<String>) {
    if first_term == "--" || first_term == "--%" {
        return (
            DefinitionRole::Marker,
            DefinitionCase::Sensitive,
            vec![first_term.to_owned()],
        );
    }
    if first_term == "-" {
        return (
            DefinitionRole::Operand,
            DefinitionCase::Sensitive,
            vec![first_term.to_owned()],
        );
    }
    let names = parameter_names(item);
    if names.is_empty() {
        (DefinitionRole::Term, DefinitionCase::Sensitive, Vec::new())
    } else {
        (DefinitionRole::Option, DefinitionCase::Sensitive, names)
    }
}

fn parameter_names(item: &DefinitionItem) -> Vec<String> {
    let mut names = option_names(item);
    for term in &item.terms {
        let text = plain_text(term);
        let token = text.split_whitespace().next().unwrap_or_default();
        if let Some(body) = token.strip_prefix("[-+]")
            && is_option_name_body(body)
        {
            for prefix in ['-', '+'] {
                let name = format!("{prefix}{body}");
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        } else if let Some(body) = token.strip_prefix('+')
            && is_option_name_body(body)
        {
            let name = format!("+{body}");
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

fn command_names(item: &DefinitionItem) -> Vec<String> {
    item.terms
        .iter()
        .flat_map(|term| {
            let text = plain_text(term);
            if let Some((name, _)) = key_binding_command_form(&text) {
                return vec![name.to_owned()];
            }
            if let Some(name) = leading_styled_command_name(term) {
                return vec![name];
            }
            text.split([',', '|'])
                .filter_map(command_name_from_authored_form)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .fold(Vec::new(), |mut names, name| {
            if !names.contains(&name) {
                names.push(name);
            }
            names
        })
}

/// Extract the command token from an unstyled authored form.
///
/// A single token is unambiguous. Multiword prose is not promoted merely
/// because it occurs below a command-oriented section; the first word is
/// accepted only when the rest begins with explicit argument syntax.
fn command_name_from_authored_form(value: &str) -> Option<&str> {
    let value = value.trim();
    let Some((first, suffix)) = value.split_once(char::is_whitespace) else {
        return is_command_name(value).then_some(value);
    };
    let suffix = suffix.trim_start();
    (suffix.starts_with(['-', '+', '/', '[', '<', '{']) && is_command_name(first)).then_some(first)
}

/// Read a formatter-emphasized command name without flattening the adjacent
/// argument placeholders into it.
fn leading_styled_command_name(term: &[Inline]) -> Option<String> {
    let first = term.iter().find(|inline| match inline {
        Inline::Anchor { .. } => false,
        Inline::Text { value } => !value.trim().is_empty(),
        _ => true,
    })?;
    let Inline::Strong { children } = first else {
        return None;
    };
    let name = plain_text(children);
    let name = name.trim();
    is_command_name(name).then(|| name.to_owned())
}

fn named_term(item: &DefinitionItem, validate: fn(&str) -> bool) -> Vec<String> {
    item.terms
        .iter()
        .flat_map(|term| {
            let text = plain_text(term);
            text.split(',')
                .filter_map(|part| named_term_name(part, validate).map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .fold(Vec::new(), |mut names, name| {
            if !names.contains(&name) {
                names.push(name);
            }
            names
        })
}

/// Extract a complete semantic name, allowing one explicitly delimited
/// trailing annotation such as Readline's `(On)` default-value notation.
fn named_term_name(value: &str, validate: fn(&str) -> bool) -> Option<&str> {
    let value = value.trim();
    if validate(value) {
        return Some(value);
    }
    let (name, annotation) = value.rsplit_once(" (")?;
    (annotation.ends_with(')') && validate(name)).then_some(name)
}

fn environment_names(item: &DefinitionItem) -> Vec<String> {
    environment_names_from_terms(&item.terms)
}

fn environment_names_from_terms(terms: &[Vec<Inline>]) -> Vec<String> {
    terms
        .iter()
        .flat_map(|term| {
            let text = plain_text(term);
            text.split([',', '|'])
                .filter_map(environment_variable_alias)
                .collect::<Vec<_>>()
        })
        .fold(Vec::new(), |mut names, name| {
            if !names.contains(&name) {
                names.push(name);
            }
            names
        })
}

/// Return one exact environment-variable spelling without an authored value.
///
/// The semantic name grammar is shared by inferred native definitions and
/// explicitly declared Markdown entries. It accepts conventional POSIX,
/// shell, PowerShell provider, braced provider, and Windows `%NAME%` forms.
/// The complete term must match.  Leading prose such as `export FOO=bar`, a
/// shell label such as `Unix Bourne shell:`, and multiple assignments such as
/// `LC_ALL=C LANG=en_US` are deliberately rejected instead of being truncated
/// to a plausible-looking first token.
pub(crate) fn environment_variable_alias(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.contains(['\r', '\n']) {
        return None;
    }
    let (spelling, assignment) = value
        .split_once('=')
        .map_or((value, None), |(name, assignment)| {
            (name.trim_end(), Some(assignment.trim_start()))
        });
    if spelling.is_empty() || spelling.chars().any(char::is_whitespace) {
        return None;
    }
    if assignment.is_some_and(contains_additional_environment_assignment) {
        return None;
    }
    let body = environment_variable_body(spelling)?;
    is_environment_variable_body(body).then(|| spelling.to_owned())
}

fn contains_additional_environment_assignment(value: &str) -> bool {
    value.split_whitespace().any(|token| {
        token.split_once('=').is_some_and(|(name, _)| {
            environment_variable_body(name).is_some_and(is_environment_variable_body)
        })
    })
}

/// Remove a recognized shell/provider wrapper from an environment selector.
pub(crate) fn environment_variable_body(value: &str) -> Option<&str> {
    if let Some(body) = value
        .strip_prefix('%')
        .and_then(|body| body.strip_suffix('%'))
    {
        return Some(body);
    }
    if let Some(body) = value
        .strip_prefix("${")
        .and_then(|body| body.strip_suffix('}'))
    {
        return strip_environment_provider(body);
    }
    if let Some(body) = strip_environment_provider(value) {
        return Some(body);
    }
    Some(value.strip_prefix('$').unwrap_or(value))
}

fn strip_environment_provider(value: &str) -> Option<&str> {
    let (provider, body) = value.split_once(':')?;
    provider
        .trim_start_matches('$')
        .eq_ignore_ascii_case("env")
        .then_some(body)
}

fn is_environment_variable_body(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '(' | ')')
        })
}

fn is_variable_term(value: &str) -> bool {
    let value = value.strip_prefix('$').unwrap_or(value);
    let (head, index) = value
        .split_once('[')
        .map_or((value, None), |(head, tail)| (head, tail.strip_suffix(']')));
    !head.is_empty()
        && head
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        && head
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && index.is_none_or(|index| {
            !index.is_empty()
                && index
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        })
}

fn is_configuration_key(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
}

fn is_command_name(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(char::is_control)
        && !value.starts_with(['-', '+', '/'])
        && !value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
}

fn role_name_slug(role: DefinitionRole, name: &str) -> String {
    match (role, name) {
        (DefinitionRole::Marker, "--") => return "end-of-options".to_owned(),
        (DefinitionRole::Marker, "--%") => return "stop-parsing".to_owned(),
        (DefinitionRole::Operand, "-") => return "dash".to_owned(),
        _ => {}
    }
    if role == DefinitionRole::Variable {
        match name {
            "$?" => return "question-mark".to_owned(),
            "$$" => return "dollar-dollar".to_owned(),
            "$^" => return "caret".to_owned(),
            "$_" => return "underscore".to_owned(),
            _ => {}
        }
    }
    if role == DefinitionRole::EnvironmentVariable
        && let Some(body) = environment_variable_body(name)
    {
        return slug(body);
    }
    let slug = slug(name);
    if slug.is_empty() {
        "entry".to_owned()
    } else {
        slug
    }
}

const fn role_id_prefix(role: DefinitionRole) -> &'static str {
    match role {
        DefinitionRole::Option => "option",
        DefinitionRole::Marker => "marker",
        DefinitionRole::Operand => "operand",
        DefinitionRole::Command => "command",
        DefinitionRole::ConfigurationKey => "configuration",
        DefinitionRole::EnvironmentVariable => "environment",
        DefinitionRole::Variable => "variable",
        DefinitionRole::Value => "value",
        DefinitionRole::Term => "term",
    }
}

fn option_names(item: &DefinitionItem) -> Vec<String> {
    option_names_from_terms(&item.terms)
}

pub(crate) fn option_names_from_terms(terms: &[Vec<Inline>]) -> Vec<String> {
    let mut names = Vec::new();
    for term in terms {
        let text = plain_text(term);
        for token in text.split(|character: char| {
            character.is_whitespace() || matches!(character, ',' | '|' | '/' | ';')
        }) {
            let token = token.trim_matches(|character: char| {
                matches!(
                    character,
                    '[' | ']' | '(' | ')' | '{' | '}' | '“' | '”' | '‘' | '’'
                )
            });
            let Some(name) = option_prefix(token) else {
                continue;
            };
            if !names.iter().any(|existing| existing == name) {
                names.push(name.to_owned());
            }
        }
    }
    names
}

pub(crate) fn option_prefix(token: &str) -> Option<&str> {
    if !token.starts_with('-') || token == "-" {
        return None;
    }
    let end = token
        .char_indices()
        .skip(1)
        .take_while(|(_, character)| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '?' | '.')
        })
        .map(|(index, character)| index + character.len_utf8())
        .last()?;
    let candidate = &token[..end];
    let body = candidate.trim_start_matches('-');
    is_option_name_body(body).then_some(candidate)
}

fn is_option_name_body(value: &str) -> bool {
    value.split('.').all(|segment| {
        !segment.is_empty()
            && segment.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '?')
            })
            && segment
                .chars()
                .any(|character| character.is_ascii_alphanumeric() || character == '?')
    })
}

fn collect_anchor_ids(nodes: &[Inline], output: &mut Vec<String>) {
    struct Collector<'a>(&'a mut Vec<String>);

    impl<'ir> Visit<'ir> for Collector<'_> {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Anchor { id } = inline {
                self.0.push(id.to_string());
            }
            visit::walk_inline(self, inline);
        }
    }

    let mut collector = Collector(output);
    for node in nodes {
        collector.visit_inline(node);
    }
}

fn semantic_fingerprint(
    item: &DefinitionItem,
    role: DefinitionRole,
    case: DefinitionCase,
    names: &[String],
) -> String {
    struct VisibleFingerprint(Vec<u8>);

    impl VisibleFingerprint {
        fn field(&mut self, value: &str) {
            self.0
                .extend_from_slice(&u64::try_from(value.len()).unwrap_or(u64::MAX).to_le_bytes());
            self.0.extend_from_slice(value.as_bytes());
        }
    }

    impl<'ir> Visit<'ir> for VisibleFingerprint {
        fn visit_block(&mut self, block: &'ir Block) {
            let marker = match block {
                Block::Paragraph { .. } => "paragraph",
                Block::Preformatted { .. } => "preformatted",
                Block::List { .. } => "list",
                Block::DefinitionList { .. } => "definition-list",
                Block::Table { .. } => "table",
                Block::Equation { value, .. } => {
                    self.field("equation");
                    self.field(value);
                    return;
                }
                Block::VerticalSpace { lines, .. } => {
                    self.field("vertical-space");
                    self.field(&lines.to_string());
                    return;
                }
                Block::ThematicBreak { .. } => "thematic-break",
                Block::Unsupported { name, text, .. } => {
                    self.field("unsupported");
                    self.field(name.as_deref().unwrap_or_default());
                    self.field(text);
                    return;
                }
            };
            self.field(marker);
            visit::walk_block(self, block);
        }

        fn visit_inline(&mut self, inline: &'ir Inline) {
            match inline {
                Inline::Text { value } => {
                    self.field("text");
                    self.field(value);
                }
                Inline::Code { value } => {
                    self.field("code");
                    self.field(value);
                }
                Inline::Strong { .. } => self.field("strong"),
                Inline::Emphasis { .. } => self.field("emphasis"),
                Inline::Link { .. } => self.field("link"),
                Inline::LineBreak => self.field("line-break"),
                Inline::Anchor { .. } => return,
            }
            visit::walk_inline(self, inline);
        }
    }

    let mut content = VisibleFingerprint(Vec::new());
    content.field(role_id_prefix(role));
    content.field(match case {
        DefinitionCase::Sensitive => "sensitive",
        DefinitionCase::Insensitive => "insensitive",
    });
    for name in names {
        content.field(name);
    }
    for term in &item.terms {
        content.field("term");
        for inline in term {
            content.visit_inline(inline);
        }
    }
    for block in &item.description {
        content.visit_block(block);
    }
    let digest = Sha256::digest(content.0);
    digest[..6]
        .iter()
        .fold(String::with_capacity(12), |mut fingerprint, byte| {
            write!(fingerprint, "{byte:02x}").expect("writing to a String cannot fail");
            fingerprint
        })
}

fn slug(value: &str) -> String {
    if value.trim_start_matches(['-', '/']) == "?" {
        return "help".to_owned();
    }
    let slug = value
        .trim_start_matches(['-', '/'])
        .chars()
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "entry".to_owned()
    } else {
        slug
    }
}

fn unique_id(base: &str, used: &mut HashSet<String>, reserved: &HashSet<String>) -> String {
    let mut candidate = base.to_owned();
    let mut suffix = 2;
    while used.contains(&candidate) || reserved.contains(&candidate) {
        candidate = format!("{base}-{suffix}");
        suffix += 1;
    }
    used.insert(candidate.clone());
    candidate
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use mant_ir::{
        Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, Inline,
        LayoutHint, Section,
    };

    use super::{environment_variable_alias, identify_definitions, option_names, option_prefix};

    fn item(value: &str) -> DefinitionItem {
        DefinitionItem {
            identity: None,
            inline_term: false,
            terms: vec![vec![Inline::Text {
                value: value.into(),
            }]],
            description: Vec::new(),
            spacing_before_lines: None,
        }
    }

    fn strong_item(value: &str) -> DefinitionItem {
        DefinitionItem {
            identity: None,
            inline_term: false,
            terms: vec![vec![Inline::Strong {
                children: vec![Inline::Text {
                    value: value.into(),
                }],
            }]],
            description: Vec::new(),
            spacing_before_lines: None,
        }
    }

    #[test]
    fn extracts_aliases_without_argument_placeholders() {
        assert_eq!(
            option_names(&item("-g, --listed-incremental=FILE")),
            ["-g", "--listed-incremental"]
        );
        assert_eq!(option_names(&item("ordinary term")), Vec::<String>::new());
        assert_eq!(option_prefix("-ca.cert"), Some("-ca.cert"));
        assert_eq!(option_prefix("--foo.bar=VALUE"), Some("--foo.bar"));
        assert_eq!(option_prefix("--foo..bar"), None);
    }

    #[test]
    fn semantic_id_allocation_ignores_a_prefilled_producer_id() {
        let mut option = item("--verbose");
        option.identity = Some(DefinitionIdentity {
            id: "producer-specific-id".into(),
            role: DefinitionRole::Option,
            case: DefinitionCase::Sensitive,
            names: vec!["--verbose".to_owned()],
        });
        let mut sections = vec![Section {
            id: "options".into(),
            title: "OPTIONS".to_owned(),
            spacing_before_lines: 0,
            blocks: vec![Block::DefinitionList {
                items: vec![option],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }],
            children: Vec::new(),
            source: None,
        }];

        identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);

        let Block::DefinitionList { items, .. } = &sections[0].blocks[0] else {
            panic!("option list");
        };
        assert_eq!(
            items[0].identity.as_ref().expect("identity").id.as_str(),
            "option-verbose"
        );
    }

    #[test]
    fn environment_aliases_require_one_complete_semantic_name() {
        for (value, expected) in [
            ("HOME", Some("HOME")),
            ("$Env:Path = C:\\Tools", Some("$Env:Path")),
            (
                "%ProgramFiles(x86)%=C:\\Program Files (x86)",
                Some("%ProgramFiles(x86)%"),
            ),
            ("Unix Bourne shell:", None),
            ("export FOO=bar", None),
            ("LC_ALL=C LANG=en_US", None),
            ("FOO= LANG=en_US", None),
        ] {
            assert_eq!(
                environment_variable_alias(value).as_deref(),
                expected,
                "{value}"
            );
        }
    }

    #[test]
    fn composite_environment_options_use_parameter_semantics() {
        let mut sections = vec![Section {
            id: "environment-options".into(),
            title: "ENVIRONMENT OPTIONS".to_owned(),
            spacing_before_lines: 0,
            blocks: vec![Block::DefinitionList {
                items: vec![item("Unix Bourne shell:"), item("-q")],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }],
            children: Vec::new(),
            source: None,
        }];

        identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);

        let Block::DefinitionList { items, .. } = &sections[0].blocks[0] else {
            panic!("definition list");
        };
        assert_eq!(
            items[0].identity.as_ref().expect("term").role,
            DefinitionRole::Term
        );
        assert_eq!(
            items[1].identity.as_ref().expect("option").role,
            DefinitionRole::Option
        );
    }

    #[test]
    fn command_discovery_requires_a_structural_or_syntactic_boundary() {
        let definition_list = |items| Block::DefinitionList {
            items,
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        };
        let mut sections = vec![
            Section {
                id: "commands".into(),
                title: "COMMANDS".to_owned(),
                spacing_before_lines: 0,
                blocks: vec![definition_list(vec![
                    strong_item("Send Env"),
                    item("Send Buffer"),
                    item("bind [-m keymap]"),
                    item("set -o"),
                    item("0 arguments"),
                ])],
                children: Vec::new(),
                source: None,
            },
            Section {
                id: "variables".into(),
                title: "VARIABLES".to_owned(),
                spacing_before_lines: 0,
                blocks: vec![definition_list(vec![
                    item("real-name"),
                    item("bind-tty-special-chars (On)"),
                    item("name prose"),
                ])],
                children: Vec::new(),
                source: None,
            },
        ];

        identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);

        let Block::DefinitionList {
            items: commands, ..
        } = &sections[0].blocks[0]
        else {
            panic!("commands");
        };
        assert_eq!(
            commands[0].identity.as_ref().expect("command").names,
            ["Send Env"]
        );
        assert!(
            commands[1]
                .identity
                .as_ref()
                .expect("unstyled prose")
                .names
                .is_empty()
        );
        assert_eq!(
            commands[2].identity.as_ref().expect("command form").names,
            ["bind"]
        );
        assert_eq!(
            commands[3].identity.as_ref().expect("command form").names,
            ["set"]
        );
        assert!(
            commands[4]
                .identity
                .as_ref()
                .expect("numeric prose")
                .names
                .is_empty()
        );
        let Block::DefinitionList {
            items: variables, ..
        } = &sections[1].blocks[0]
        else {
            panic!("variables");
        };
        assert_eq!(
            variables[0].identity.as_ref().expect("variable").names,
            ["real-name"]
        );
        assert!(
            variables[1]
                .identity
                .as_ref()
                .is_some_and(|identity| identity.names == ["bind-tty-special-chars"])
        );
        assert!(
            variables[2]
                .identity
                .as_ref()
                .expect("unclassified term")
                .names
                .is_empty()
        );
    }

    #[test]
    fn colliding_generated_ids_follow_semantics_not_sibling_order() {
        fn ids(terms: &[&str], with_colliding_section: bool) -> HashMap<String, String> {
            let mut sections = Vec::new();
            if with_colliding_section {
                sections.push(Section {
                    id: "option-v".into(),
                    title: "Unrelated notes".to_owned(),
                    spacing_before_lines: 0,
                    blocks: Vec::new(),
                    children: Vec::new(),
                    source: None,
                });
            }
            sections.push(Section {
                id: "options".into(),
                title: "OPTIONS".to_owned(),
                spacing_before_lines: 0,
                blocks: vec![Block::DefinitionList {
                    items: terms.iter().map(|term| item(term)).collect(),
                    compact: true,
                    layout: LayoutHint::default(),
                    source: None,
                }],
                children: Vec::new(),
                source: None,
            });

            identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);
            let Block::DefinitionList { items, .. } = &sections.last().expect("options").blocks[0]
            else {
                panic!("definitions");
            };
            items
                .iter()
                .map(|item| {
                    let identity = item.identity.as_ref().expect("identity");
                    (identity.names[0].clone(), identity.id.to_string())
                })
                .collect()
        }

        let original = ids(&["-v", "-V"], false);
        let reordered = ids(&["-V", "-v"], false);
        let with_section = ids(&["-v", "-V"], true);
        assert_eq!(original, reordered);
        assert_eq!(original, with_section);
        assert_ne!(original["-v"], original["-V"]);
        assert!(original.values().all(|id| id.starts_with("option-v-")));
    }

    #[test]
    fn normalizes_hanging_option_layout_before_assigning_identity() {
        let paragraph = |value: &str, indent_columns, spacing_before_lines| Block::Paragraph {
            children: vec![Inline::Text {
                value: value.to_owned(),
            }],
            layout: LayoutHint {
                indent_columns,
                spacing_before_lines,
            },
            source: None,
        };
        let mut sections = vec![Section {
            id: "options".to_owned().into(),
            title: "OPTIONS".to_owned(),
            spacing_before_lines: 0,
            blocks: vec![
                paragraph("-v, --version", 0, 1),
                paragraph("Print version information.", 4, 0),
                paragraph("-C <path>", 0, 1),
                paragraph("Run from path.", 4, 0),
            ],
            children: Vec::new(),
            source: None,
        }];

        identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);

        assert_eq!(sections[0].blocks.len(), 2);
        let Block::DefinitionList { items, layout, .. } = &sections[0].blocks[0] else {
            panic!("hanging option should become a definition list");
        };
        assert_eq!(layout.indent_columns, 0);
        assert_eq!(
            items[0].identity.as_ref().expect("option identity").names,
            ["-v", "--version"]
        );
        assert!(matches!(
            &items[0].description[0],
            Block::Paragraph { layout, .. }
                if layout.indent_columns == 0 && layout.spacing_before_lines == 0
        ));
        assert_eq!(items[0].spacing_before_lines, Some(1));
        let Block::DefinitionList { items, .. } = &sections[0].blocks[1] else {
            panic!("second option should remain independently addressable");
        };
        assert_eq!(
            items[0].identity.as_ref().expect("option identity").names,
            ["-C"]
        );
    }

    #[test]
    fn normalizes_cross_platform_hanging_environment_definitions() {
        let paragraph = |value: &str, indent_columns| Block::Paragraph {
            children: vec![Inline::Text {
                value: value.to_owned(),
            }],
            layout: LayoutHint {
                indent_columns,
                spacing_before_lines: 0,
            },
            source: None,
        };
        let mut sections = vec![Section {
            id: "environment".into(),
            title: "ENVIRONMENT VARIABLES".to_owned(),
            spacing_before_lines: 0,
            blocks: vec![
                paragraph("HOME", 0),
                paragraph("User home.", 4),
                paragraph("$Env:Path = C:\\Tools", 0),
                paragraph("PowerShell provider form.", 4),
                paragraph("%ProgramFiles(x86)%=C:\\Program Files (x86)", 0),
                paragraph("Windows expansion form.", 4),
            ],
            children: Vec::new(),
            source: None,
        }];

        identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);

        let identities = sections[0]
            .blocks
            .iter()
            .map(|block| {
                let Block::DefinitionList { items, .. } = block else {
                    panic!("hanging environment entry should become a definition list");
                };
                items[0].identity.as_ref().expect("environment identity")
            })
            .collect::<Vec<_>>();
        assert_eq!(identities.len(), 3);
        assert!(
            identities
                .iter()
                .all(|identity| identity.role == DefinitionRole::EnvironmentVariable)
        );
        assert_eq!(identities[0].names, ["HOME"]);
        assert_eq!(identities[1].names, ["$Env:Path"]);
        assert_eq!(identities[2].names, ["%ProgramFiles(x86)%"]);
        assert_eq!(identities[1].id.as_str(), "environment-path");
        assert_eq!(identities[2].id.as_str(), "environment-programfiles-x86");
    }

    #[test]
    fn keeps_native_navigation_anchors_separate_from_semantic_ids() {
        let mut command = item("set-mark");
        command.terms[0].insert(0, Inline::Anchor { id: "set".into() });
        let mut sections = vec![Section {
            id: "commands".into(),
            title: "COMMANDS".to_owned(),
            spacing_before_lines: 0,
            blocks: vec![Block::DefinitionList {
                items: vec![command],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }],
            children: Vec::new(),
            source: None,
        }];

        let retained = identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);
        let Block::DefinitionList { items, .. } = &sections[0].blocks[0] else {
            panic!("command definition list");
        };
        let identity = items[0].identity.as_ref().expect("command identity");
        assert_eq!(identity.id.as_str(), "command-set-mark");
        assert_eq!(identity.names, ["set-mark"]);
        assert!(retained.contains("set"));
        assert!(retained.contains("command-set-mark"));
    }

    #[test]
    fn generic_terms_receive_the_anchor_their_projected_entry_advertises() {
        let mut sections = vec![Section {
            id: "glossary".into(),
            title: "GLOSSARY".to_owned(),
            spacing_before_lines: 0,
            blocks: vec![Block::DefinitionList {
                items: vec![item("widget")],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }],
            children: Vec::new(),
            source: None,
        }];

        let retained = identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);
        let Block::DefinitionList { items, .. } = &sections[0].blocks[0] else {
            panic!("term definition list");
        };
        let identity = items[0].identity.as_ref().expect("term identity");
        assert_eq!(identity.id.as_str(), "term-widget");
        assert!(matches!(
            items[0].terms[0].first(),
            Some(Inline::Anchor { id }) if id == "term-widget"
        ));
        assert!(retained.contains("term-widget"));
    }

    #[test]
    fn classifies_environment_configuration_and_nested_parameter_semantics() {
        fn identities(section: &Section) -> Vec<&mant_ir::DefinitionIdentity> {
            let Block::DefinitionList { items, .. } = &section.blocks[0] else {
                panic!("expected definition list");
            };
            items
                .iter()
                .map(|item| item.identity.as_ref().expect("semantic identity"))
                .collect()
        }

        let definition_list = |items| Block::DefinitionList {
            items,
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        };
        let section = |id: &str, title: &str, items| Section {
            id: id.into(),
            title: title.to_owned(),
            spacing_before_lines: 0,
            blocks: vec![definition_list(items)],
            children: Vec::new(),
            source: None,
        };
        let mut option = item("-o MODE");
        option
            .description
            .push(definition_list(vec![item("yes"), item("no")]));
        let mut sections = vec![
            section("environment", "ENVIRONMENT", vec![item("PATH")]),
            section(
                "configuration",
                "CONFIGURATION KEYWORDS",
                vec![item("HostKeyAlgorithms")],
            ),
            section("options", "OPTIONS", vec![item("--"), item("-"), option]),
        ];

        identify_definitions(&mut Vec::new(), &mut sections, &HashSet::new(), None);

        assert_eq!(
            identities(&sections[0])[0].role,
            DefinitionRole::EnvironmentVariable
        );
        assert_eq!(
            identities(&sections[1])[0].role,
            DefinitionRole::ConfigurationKey
        );
        let parameters = identities(&sections[2]);
        assert_eq!(parameters[0].role, DefinitionRole::Marker);
        assert_eq!(parameters[1].role, DefinitionRole::Operand);
        assert_eq!(parameters[2].role, DefinitionRole::Option);
        let Block::DefinitionList { items, .. } = &sections[2].blocks[0] else {
            panic!("expected option definitions");
        };
        let Block::DefinitionList { items: values, .. } = &items[2].description[0] else {
            panic!("expected nested values");
        };
        assert!(values.iter().all(|value| {
            value
                .identity
                .as_ref()
                .is_some_and(|identity| identity.role == DefinitionRole::Value)
        }));
    }
}
