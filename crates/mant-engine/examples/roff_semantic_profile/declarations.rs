//! Independent, bidirectional source-run / final-group accounting.
//! This profiler does not assign names or repair IR. Unexplained rejected runs
//! are review candidates; an IR group without a source run is a violation.
use libmandoc_rs::{Node, NodeKind};
use mant_ir::{
    Block, DefinitionItem, Document, SourceSpan,
    visit::{self, Visit},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Observed<'a> {
    owners: BTreeMap<(u32, u32), &'a DefinitionItem>,
    groups: Vec<Vec<(u32, u32)>>,
    invalid: usize,
}
impl<'a> Visit<'a> for Observed<'a> {
    fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
        if let Some(source) = item.source {
            self.owners.insert(key(source), item);
        }
        visit::walk_definition_item(self, item);
    }
    fn visit_block(&mut self, block: &'a Block) {
        if let Block::DefinitionList {
            items,
            declaration_groups,
            ..
        } = block
        {
            for group in declaration_groups {
                let sources = group.resolve(items).and_then(|members| {
                    members
                        .iter()
                        .map(|i| i.source.map(key))
                        .collect::<Option<Vec<_>>>()
                });
                if let Some(sources) = sources {
                    self.groups.push(sources);
                } else {
                    self.invalid += 1;
                }
            }
        }
        visit::walk_block(self, block);
    }
}
fn key(source: SourceSpan) -> (u32, u32) {
    (source.line, source.column)
}

fn readable(node: &Node) -> bool {
    if node.flags.no_print || matches!(node.macro_name.as_deref(), Some("Tg" | "PD" | "Sm" | "ft"))
    {
        return false;
    }
    node.text.as_ref().is_some_and(|t| !t.trim().is_empty()) || node.children.iter().any(readable)
}
fn body(node: &Node) -> bool {
    node.children
        .iter()
        .filter(|n| n.kind == NodeKind::Body)
        .any(readable)
}
fn paragraph_boundary(node: &Node) -> bool {
    matches!(
        node.macro_name.as_deref(),
        Some("PP" | "P" | "LP" | "Pp" | "sp" | "br")
    ) || node
        .children
        .iter()
        .filter(|n| n.kind == NodeKind::Body)
        .any(paragraph_boundary)
}

fn bracket_head(node: &Node) -> bool {
    fn first_text(node: &Node) -> Option<&str> {
        node.text
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .or_else(|| node.children.iter().find_map(first_text))
    }
    node.children
        .iter()
        .find(|n| n.kind == NodeKind::Head)
        .and_then(first_text)
        .is_some_and(|text| text.trim_start().starts_with('['))
}

struct Audit<'a> {
    observed: Observed<'a>,
    rows: Vec<Value>,
    matched: BTreeSet<usize>,
    review: usize,
    group_index: BTreeMap<Vec<(u32, u32)>, usize>,
}
impl Audit<'_> {
    fn classify(&mut self, run: &[(&Node, Vec<usize>)], boundary: &str) {
        if run.is_empty() {
            return;
        }
        let mut sources = Vec::new();
        for (node, _) in run {
            let source = (node.line, node.column);
            // TQ explicitly contributes another term to the preceding owner.
            if node.macro_name.as_deref() == Some("TQ")
                && !self.observed.owners.contains_key(&source)
            {
                continue;
            }
            sources.push(source);
        }
        let retained = self.group_index.get(&sources).copied();
        let reason = if let Some(index) = retained {
            self.matched.insert(index);
            "source-run-retained"
        } else if boundary != "description" {
            boundary
        } else if sources.len() < 2 {
            "single-physical-owner"
        } else if sources
            .iter()
            .any(|s| !self.observed.owners.contains_key(s))
        {
            "not-a-final-definition-owner"
        } else if sources.iter().any(|s| {
            self.observed.owners[s]
                .entry
                .as_ref()
                .is_none_or(|e| e.names.is_empty())
        }) {
            "no-exact-name-review-template-or-non-declaration"
        } else if sources[..sources.len() - 1]
            .iter()
            .any(|s| mant_ir::blocks_have_readable_content(&self.observed.owners[s].description))
        {
            "leading-owner-has-readable-content"
        } else {
            self.review += 1;
            "unresolved-recognized-source-run"
        };
        self.rows.push(json!({
            "status": if retained.is_some() { "retained" } else { "rejected" }, "reason": reason,
            "sourceOwners": run.iter().map(|(n,path)| json!({"astPath":path,"line":n.line,"column":n.column,"macro":n.macro_name})).collect::<Vec<_>>(),
            "physicalSources":sources,"observedGroup":retained,
        }));
    }
    fn walk(&mut self, node: &Node, path: &mut Vec<usize>) {
        let mut run = Vec::new();
        for (index, child) in node.children.iter().enumerate() {
            path.push(index);
            if child.kind == NodeKind::Block
                && matches!(child.macro_name.as_deref(), Some("IP" | "TP" | "TQ" | "It"))
            {
                // A source-only parameter continuation cannot connect the
                // declarations before and after it. A literal named `[` (test)
                // is protected by its actual name facts, not this punctuation.
                let parameter = bracket_head(child)
                    && self
                        .observed
                        .owners
                        .get(&(child.line, child.column))
                        .is_some_and(|item| {
                            item.entry
                                .as_ref()
                                .is_none_or(|facts| facts.names.is_empty())
                        });
                if parameter {
                    self.classify(&run, "parameter-only-head-boundary");
                    run.clear();
                    self.classify(&[(child, path.clone())], "parameter-only-head");
                    self.walk(child, path);
                    path.pop();
                    continue;
                }
                run.push((child, path.clone()));
                if body(child) {
                    if run.len() > 1 {
                        self.classify(&run, "description");
                    }
                    run.clear();
                } else if paragraph_boundary(child) {
                    self.classify(&run, "explicit-paragraph-boundary");
                    run.clear();
                }
            } else if !matches!(child.macro_name.as_deref(), Some("PD" | "Sm" | "Tg" | "ft")) {
                self.classify(&run, "source-container-or-content-boundary");
                run.clear();
            }
            self.walk(child, path);
            path.pop();
        }
        self.classify(&run, "unclosed-head-run");
    }
}

pub(super) fn profile(root: &Node, document: &Document) -> Value {
    let mut observed = Observed::default();
    observed.visit_document(document);
    let group_index = observed
        .groups
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, s)| (s, i))
        .collect();
    let mut audit = Audit {
        observed,
        rows: Vec::new(),
        matched: BTreeSet::new(),
        review: 0,
        group_index,
    };
    audit.walk(root, &mut Vec::new());
    let unexpected = audit.observed.groups.iter().enumerate().filter(|(i,_)| !audit.matched.contains(i))
        .map(|(index,sources)| json!({"group":index,"sources":sources,"reason":"no-compatible-source-run"})).collect::<Vec<_>>();
    json!({"sourceRuns":audit.rows,"observedGroups":audit.observed.groups,
        "unexpectedGroups":unexpected,"invalidGroups":audit.observed.invalid,"unresolvedRuns":audit.review})
}

pub(super) fn violations(profile: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    if profile["invalidGroups"].as_u64().unwrap_or(0) != 0
        || !profile["unexpectedGroups"]
            .as_array()
            .is_some_and(Vec::is_empty)
    {
        errors.push("declaration group lacks a valid source-owner run".into());
    }
    if let Some(count) = profile["unresolvedRuns"].as_u64().filter(|&n| n != 0) {
        errors.push(format!(
            "{count} recognized source declaration runs need review"
        ));
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parameter_continuation_separates_runs_without_hiding_crossing_groups() {
        let source = b".TH PROBE 1\n.SH COMMANDS\n.TP\n.B first\n.TP\n[ argument ]\n.TP\n.B second\n.TP\n.B third\nBody.\n";
        let parsed = libmandoc_rs::Parser::new(Default::default())
            .parse_bytes("probe.1", source)
            .unwrap();
        let mut document = mant_engine::query_roff_bytes(source)
            .unwrap()
            .document
            .unwrap();
        let valid = profile(&parsed.document.root, &document);
        assert_eq!(valid["unexpectedGroups"], json!([]));
        assert!(
            valid["sourceRuns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["reason"] == "parameter-only-head")
        );
        let Block::DefinitionList {
            declaration_groups, ..
        } = &mut document.sections[0].blocks[0]
        else {
            panic!()
        };
        declaration_groups[0].start_item = 0;
        assert_eq!(
            profile(&parsed.document.root, &document)["unexpectedGroups"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn accounting_detects_missing_and_unbacked_groups_in_both_directions() {
        let source = b".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --first\n.TP\n.B --second\nBody.\n";
        let parsed = libmandoc_rs::Parser::new(Default::default())
            .parse_bytes("probe.1", source)
            .unwrap();
        let mut document = mant_engine::query_roff_bytes(source)
            .unwrap()
            .document
            .unwrap();
        let valid = profile(&parsed.document.root, &document);
        assert_eq!(valid["sourceRuns"][0]["status"], "retained");
        assert_eq!(valid["unexpectedGroups"], json!([]));
        let Block::DefinitionList {
            declaration_groups, ..
        } = &mut document.sections[0].blocks[0]
        else {
            panic!()
        };
        declaration_groups.clear();
        assert_eq!(
            profile(&parsed.document.root, &document)["unresolvedRuns"],
            1
        );
        let Block::DefinitionList {
            declaration_groups, ..
        } = &mut document.sections[0].blocks[0]
        else {
            panic!()
        };
        declaration_groups.push(mant_ir::DeclarationGroup {
            start_item: 0,
            end_item: 2,
        });
        let mut wrong = parsed.document.root.clone();
        fn relocate(node: &mut Node) {
            node.line += 100;
            for child in &mut node.children {
                relocate(child);
            }
        }
        relocate(&mut wrong);
        assert_eq!(
            profile(&wrong, &document)["unexpectedGroups"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
