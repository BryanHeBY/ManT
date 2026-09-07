//! Definition identity policy; coordinated by the parent discovery passes.
use super::{
    context::DefinitionContext,
    syntax::{environment_variable_body, infer_identity},
};
use crate::inline::plain_text;
use mant_ir::{
    Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, EntryOwner, Inline,
    ListItem, Section, ValueDomain,
    visit::{self, Visit},
};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
};

pub(super) fn document_anchor_ids(blocks: &[Block], sections: &[Section]) -> HashSet<String> {
    struct Collector(HashSet<String>);

    impl<'ir> Visit<'ir> for Collector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Anchor { id, .. } = inline {
                self.0.insert(id.to_string());
            }
            visit::walk_inline(self, inline);
        }
    }

    let mut collector = Collector(HashSet::new());
    for block in blocks {
        collector.visit_block(block);
    }
    for section in sections {
        collector.visit_section(section);
    }
    collector.0
}

pub(super) struct IdentityPlan {
    pub(super) role: DefinitionRole,
    pub(super) case: DefinitionCase,
    pub(super) names: Vec<String>,
    pub(super) value_domain: Option<ValueDomain>,
    pub(super) preferred: String,
}

pub(super) fn identity_plan(item: &DefinitionItem, context: DefinitionContext) -> IdentityPlan {
    let (role, case, names, value_domain) = item.identity.as_ref().map_or_else(
        || {
            let (role, case, names) = infer_identity(item, context);
            (role, case, names, None)
        },
        |identity| {
            (
                identity.role,
                identity.case,
                identity.names.clone(),
                identity.value_domain.clone(),
            )
        },
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
        value_domain,
        preferred,
    }
}

pub(super) fn list_identity_base(item: &ListItem) -> Option<String> {
    let facts = item.entry.as_ref()?;
    let name = facts.names.first().map_or("entry", String::as_str);
    Some(format!(
        "{}-{}",
        role_id_prefix(facts.role),
        role_name_slug(facts.role, name)
    ))
}

pub(super) fn identify_list_item(
    item: &mut ListItem,
    used: &mut HashSet<String>,
    reserved: &HashSet<String>,
    retained: &mut HashSet<String>,
    preferred_counts: &HashMap<String, usize>,
) -> Option<DefinitionRole> {
    let mut preferred = list_identity_base(item)?;
    let facts = item.entry.as_ref()?;
    if preferred_counts
        .get(&preferred)
        .copied()
        .unwrap_or_default()
        > 1
        || reserved.contains(&preferred)
    {
        preferred = format!(
            "{preferred}-{}",
            semantic_fingerprint(EntryOwner::List(item), facts.role, facts.case, &facts.names)
        );
    }
    let id = unique_id(&preferred, used, reserved);
    retained.insert(id.clone());
    let facts = item.entry.as_mut()?;
    facts.id = id.into();
    Some(facts.role)
}

pub(super) fn identify_item(
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
        value_domain,
        mut preferred,
    } = identity_plan(item, context);

    let mut anchors = Vec::new();
    for term in &item.terms {
        collect_anchor_ids(term, &mut anchors);
    }
    retained.extend(anchors.iter().cloned());

    // A target-only definition is a navigation placement artifact, not a
    // semantic concept. Keep its native anchors but do not manufacture an
    // empty `term-entry-*` whose outline label is merely its generated ID.
    if names.is_empty()
        && !item
            .terms
            .iter()
            .any(|term| !plain_text(term).trim().is_empty())
    {
        item.identity = None;
        return DefinitionRole::Term;
    }

    if preferred_counts
        .get(&preferred)
        .copied()
        .unwrap_or_default()
        > 1
        || reserved.contains(&preferred)
    {
        preferred = format!(
            "{preferred}-{}",
            semantic_fingerprint(EntryOwner::Definition(item), role, case, &names)
        );
    }
    let id = unique_id(&preferred, used, reserved);
    if !anchors.iter().any(|anchor| anchor == &id)
        && let Some(term) = item.terms.first_mut()
    {
        term.insert(0, Inline::anchor(id.clone()));
    }
    retained.insert(id.clone());
    item.identity = Some(DefinitionIdentity {
        name_bindings: Vec::new(),
        alias_groups: Vec::new(),
        alias_of: None,
        forms: Vec::new(),
        id: id.into(),
        role,
        case,
        names,
        value_domain,
    });
    role
}

pub(super) fn has_semantic_spelling(item: &DefinitionItem, plan: &IdentityPlan) -> bool {
    !plan.names.is_empty()
        || item
            .terms
            .iter()
            .any(|term| !plain_text(term).trim().is_empty())
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
        return document_id_slug(body);
    }
    let slug = document_id_slug(name);
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

fn collect_anchor_ids(nodes: &[Inline], output: &mut Vec<String>) {
    struct Collector<'a>(&'a mut Vec<String>);

    impl<'ir> Visit<'ir> for Collector<'_> {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Anchor { id, .. } = inline {
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
    item: EntryOwner<'_>,
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
    if let EntryOwner::Definition(item) = item {
        for term in &item.terms {
            content.field("term");
            for inline in term {
                content.visit_inline(inline);
            }
        }
    }
    for block in item.blocks() {
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

/// Normalize a visible semantic name into a document-local identity base.
///
/// Source-specific producers use this same boundary for generated navigation
/// anchors so validation, entry discovery, and native permalink identities
/// cannot drift into separate slug dialects.
pub(crate) fn document_id_slug(value: &str) -> String {
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
