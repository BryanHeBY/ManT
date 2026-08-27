//! Identifies addressable semantic entries after source-specific lowering.
//!
//! Both libmandoc macro sets and Markdown produce definition lists. This
//! pass assigns one canonical option identity without leaking source macros
//! into the stable document contract.

use std::{
    collections::{HashSet, VecDeque},
    mem,
};

use mant_ir::{
    Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, Inline, LayoutHint,
    Section, SourceSpan,
    visit::{self, Visit},
};

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
    let mut used = HashSet::new();
    collect_section_ids(sections, &mut used);
    let mut retained = HashSet::new();
    let root_context = document_name.map_or(DefinitionContext::Generic, |name| {
        let name = name.to_ascii_lowercase();
        if name.ends_with("_config") || name.ends_with("-config") {
            DefinitionContext::ConfigurationKeys
        } else {
            DefinitionContext::Generic
        }
    });
    identify_blocks(
        blocks,
        root_context,
        &mut used,
        reserved_targets,
        &mut retained,
    );
    for section in sections {
        let context = DefinitionContext::for_section(&section.title, root_context);
        identify_blocks(
            &mut section.blocks,
            context,
            &mut used,
            reserved_targets,
            &mut retained,
        );
        identify_sections(
            &mut section.children,
            context,
            &mut used,
            reserved_targets,
            &mut retained,
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

fn collect_section_ids(sections: &[Section], output: &mut HashSet<String>) {
    struct Collector<'a>(&'a mut HashSet<String>);

    impl<'ir> Visit<'ir> for Collector<'_> {
        fn visit_section(&mut self, section: &'ir Section) {
            self.0.insert(section.id.to_string());
            visit::walk_section(self, section);
        }
    }

    let mut collector = Collector(output);
    for section in sections {
        collector.visit_section(section);
    }
}

fn identify_sections(
    sections: &mut [Section],
    parent_context: DefinitionContext,
    used: &mut HashSet<String>,
    reserved: &HashSet<String>,
    retained: &mut HashSet<String>,
) {
    for section in sections {
        let context = DefinitionContext::for_section(&section.title, parent_context);
        identify_blocks(&mut section.blocks, context, used, reserved, retained);
        identify_sections(&mut section.children, context, used, reserved, retained);
    }
}

fn identify_blocks(
    blocks: &mut Vec<Block>,
    context: DefinitionContext,
    used: &mut HashSet<String>,
    reserved: &HashSet<String>,
    retained: &mut HashSet<String>,
) {
    normalize_definition_nesting(blocks);
    normalize_hanging_definitions(blocks);
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    identify_blocks(&mut item.blocks, context, used, reserved, retained);
                }
            }
            Block::DefinitionList { items, layout, .. } => {
                let item_context =
                    if context == DefinitionContext::Commands && layout.indent_columns > 0 {
                        DefinitionContext::Parameters
                    } else {
                        context
                    };
                for item in items {
                    let role = identify_item(item, item_context, used, reserved, retained);
                    let child_context = match role {
                        DefinitionRole::Command => DefinitionContext::Parameters,
                        DefinitionRole::Option
                        | DefinitionRole::Marker
                        | DefinitionRole::Operand
                        | DefinitionRole::ConfigurationKey => DefinitionContext::Values,
                        DefinitionRole::EnvironmentVariable
                        | DefinitionRole::Variable
                        | DefinitionRole::Value
                        | DefinitionRole::Term => item_context,
                    };
                    identify_blocks(
                        &mut item.description,
                        child_context,
                        used,
                        reserved,
                        retained,
                    );
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        identify_blocks(&mut cell.blocks, context, used, reserved, retained);
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
        if words.contains(&"ENVIRONMENT") {
            return Self::EnvironmentVariables;
        }
        if words.contains(&"VARIABLES") || words.contains(&"VARIABLE") {
            return Self::Variables;
        }
        if words.contains(&"OPTIONS")
            || words.contains(&"OPTION")
            || words.contains(&"SWITCHES")
            || words.contains(&"FLAGS")
        {
            return Self::Parameters;
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
/// option entries. Native parsers correctly retain that layout, but
/// neither representation is a definition list on its own. Recognising the
/// shared visible shape here keeps option identity independent of the source
/// macro set or source parser used by the query pipeline.
fn normalize_hanging_definitions(blocks: &mut Vec<Block>) {
    let mut pending: VecDeque<Block> = mem::take(blocks).into();
    let mut normalized = Vec::with_capacity(pending.len());

    while let Some(block) = pending.pop_front() {
        let Some(term_indent) = option_term_indent(&block) else {
            normalized.push(block);
            continue;
        };

        let mut description = Vec::new();
        while let Some(next) = pending.front() {
            if option_term_indent(next) == Some(term_indent) {
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

fn option_term_indent(block: &Block) -> Option<u16> {
    let Block::Paragraph {
        children, layout, ..
    } = block
    else {
        return None;
    };
    let text = plain_text(children);
    let trimmed = text.trim_start();
    (trimmed.starts_with('-')
        && !option_names_from_terms(std::slice::from_ref(children)).is_empty())
    .then_some(layout.indent_columns)
}

fn block_indent(block: &Block) -> u16 {
    block_layout(block).map_or(0, |layout| layout.indent_columns)
}

fn shift_block_indent(block: &mut Block, origin: u16) {
    if let Some(layout) = block_layout_mut(block) {
        layout.indent_columns = layout.indent_columns.saturating_sub(origin);
    }
}

fn identify_item(
    item: &mut DefinitionItem,
    context: DefinitionContext,
    used: &mut HashSet<String>,
    reserved: &HashSet<String>,
    retained: &mut HashSet<String>,
) -> DefinitionRole {
    let (role, case, names) = item.identity.as_ref().map_or_else(
        || infer_identity(item, context),
        |identity| (identity.role, identity.case, identity.names.clone()),
    );

    let mut anchors = Vec::new();
    for term in &item.terms {
        collect_anchor_ids(term, &mut anchors);
    }
    if role != DefinitionRole::Term {
        retained.extend(anchors.iter().cloned());
    }

    let existing = anchors.first().cloned();
    let preferred = existing.clone().unwrap_or_else(|| {
        let name = names.first().cloned().unwrap_or_else(|| {
            item.terms
                .first()
                .map_or_else(|| "entry".to_owned(), |term| plain_text(term))
        });
        format!("{}-{}", role_id_prefix(role), role_name_slug(role, &name))
    });
    // A copied libmandoc anchor may itself be an explicit `.Tg` destination,
    // so it is allowed to match the reserved set. Generated IDs are not.
    let id = if existing.is_some() && !used.contains(&preferred) {
        used.insert(preferred.clone());
        preferred
    } else {
        unique_id(&preferred, used, reserved)
    };
    if role != DefinitionRole::Term
        && !anchors.iter().any(|anchor| anchor == &id)
        && let Some(term) = item.terms.first_mut()
    {
        term.insert(
            0,
            Inline::Anchor {
                id: id.clone().into(),
            },
        );
    }
    if role != DefinitionRole::Term {
        retained.insert(id.clone());
    }
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
        DefinitionContext::EnvironmentVariables => (
            DefinitionRole::EnvironmentVariable,
            DefinitionCase::Sensitive,
            named_term(item, is_variable_term),
        ),
        DefinitionContext::Variables => (
            DefinitionRole::Variable,
            DefinitionCase::Sensitive,
            named_term(item, is_variable_term),
        ),
        DefinitionContext::ConfigurationKeys => (
            DefinitionRole::ConfigurationKey,
            DefinitionCase::Insensitive,
            named_term(item, is_configuration_key),
        ),
        DefinitionContext::Values => (
            DefinitionRole::Value,
            DefinitionCase::Sensitive,
            named_term(item, is_value_name),
        ),
        DefinitionContext::Parameters => parameter_identity(item, trimmed),
        DefinitionContext::Generic if trimmed.starts_with('-') => parameter_identity(item, trimmed),
        DefinitionContext::Generic => (DefinitionRole::Term, DefinitionCase::Sensitive, Vec::new()),
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
        .filter_map(|term| {
            let text = plain_text(term);
            let name = text.split_whitespace().next()?.trim();
            (!name.is_empty() && !name.starts_with(['-', '+', '/'])).then(|| name.to_owned())
        })
        .fold(Vec::new(), |mut names, name| {
            if !names.contains(&name) {
                names.push(name);
            }
            names
        })
}

fn named_term(item: &DefinitionItem, validate: fn(&str) -> bool) -> Vec<String> {
    item.terms
        .iter()
        .flat_map(|term| {
            let text = plain_text(term);
            text.split(',')
                .filter_map(|part| {
                    let name = part.split_whitespace().next()?;
                    validate(name).then(|| name.to_owned())
                })
                .collect::<Vec<_>>()
        })
        .fold(Vec::new(), |mut names, name| {
            if !names.contains(&name) {
                names.push(name);
            }
            names
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
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
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
    use std::collections::HashSet;

    use mant_ir::{Block, DefinitionItem, DefinitionRole, Inline, LayoutHint, Section};

    use super::{identify_definitions, option_names, option_prefix};

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
