//! Independent, bidirectional source-run / final-group accounting.
//! This profiler does not assign names or repair IR. Unexplained rejected runs
//! are review candidates; an IR group without a compatible source run is a
//! violation. A source run may also contain layout-preserved, unaddressable
//! template heads; those do not invalidate a contiguous semantic sub-run.
use libmandoc_rs::{Node, NodeKind};
use mant_ir::Document;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[path = "declarations/owners.rs"]
mod owners;
#[path = "declarations/source.rs"]
mod source;
#[cfg(test)]
#[path = "declarations/tests.rs"]
mod tests;

use owners::{NativeOwners, ObservedOwners, OwnerKey};
use source::{
    body, bracket_head, native_mdoc_search_template, paragraph_boundary, source_presentation_head,
    unsigned_numeric_head,
};

struct Audit<'a> {
    observed: ObservedOwners<'a>,
    rows: Vec<Value>,
    matched: BTreeSet<usize>,
    group_index: BTreeMap<Vec<OwnerKey>, usize>,
    native: NativeOwners<'a>,
}
impl Audit<'_> {
    fn contains_native_search_template(&self, sources: &[OwnerKey]) -> bool {
        sources.iter().any(|source| {
            self.native
                .node(source)
                .zip(self.observed.owners.get(source))
                .is_some_and(|(node, item)| native_mdoc_search_template(node, item))
        })
    }

    fn retained_group(&self, sources: &[OwnerKey]) -> Option<(usize, &'static str)> {
        if let Some(index) = self.group_index.get(sources).copied()
            && !self.contains_native_search_template(sources)
        {
            return Some((index, "source-run-retained"));
        }

        // mdoc command references often collect several `.It Xo` forms under
        // one description. Search forms containing `Ar` are presentation
        // templates. A later literal command can still have a name and
        // share the description with an adjacent command. The final declaration
        // group therefore represents a contiguous *semantic* portion of the
        // physical native run. Accept that precise subset only when every
        // omitted owner is an immutable native template form; missing,
        // reordered, or merely damaged IR owners remain audit obligations.
        self.observed
            .groups
            .iter()
            .enumerate()
            .filter(|(_, group)| !group.is_empty() && group.len() < sources.len())
            .find_map(|(index, group)| {
                sources
                    .windows(group.len())
                    .position(|window| window == group)
                    .filter(|&start| {
                        !self.contains_native_search_template(group)
                            && sources[..start]
                                .iter()
                                .chain(&sources[start + group.len()..])
                                .all(|source| {
                                    self.observed
                                        .owners
                                        .get(source)
                                        .zip(self.native.node(source))
                                        .is_some_and(|(item, node)| {
                                            source_presentation_head(node, item)
                                        })
                                })
                    })
                    .map(|_| (index, "source-run-semantic-subset"))
            })
    }

    fn classify(&mut self, run: &[(&Node, Vec<usize>)], boundary: &str) {
        if run.is_empty() {
            return;
        }
        let mut sources = Vec::new();
        for (node, _) in run {
            let Some(source) = self.native.key(node) else {
                continue;
            };
            // Multiple explicit TQ heads can share one logical owner. Do not
            // confuse their source coordinate with the next independent TP.
            if sources.last() == Some(&source) {
                continue;
            }
            sources.push(source);
        }
        let retained = self.retained_group(&sources);
        let observed_group = retained.map(|(index, _)| index);
        let reason = if let Some((index, reason)) = retained {
            self.matched.insert(index);
            reason
        } else if boundary != "description" {
            boundary
        } else if sources.len() < 2 {
            "single-physical-owner"
        } else if sources
            .iter()
            .any(|s| !self.observed.owners.contains_key(s))
        {
            "not-a-final-definition-owner"
        } else {
            // Without an actual final group, even a fully named native run
            // does not establish that all heads share one description. Keep
            // this source fact in the ledger, but do not promote it to a
            // product failure on inference alone.
            "named-source-run-without-group"
        };
        self.rows.push(json!({
            "status": if retained.is_some() { "retained" } else { "rejected" }, "reason": reason,
            "sourceOwners": run.iter().map(|(n,path)| json!({"astPath":path,"line":n.line,"column":n.column,"macro":n.macro_token.as_deref(),"flowEpoch":n.flow_epoch,"ownerOccurrence":self.native.key(n).map(|key|key.2)})).collect::<Vec<_>>(),
            "physicalSources":sources,"observedGroup":observed_group,
        }));
    }
    fn walk(&mut self, node: &Node, path: &mut Vec<usize>) {
        let mut run: Vec<(&Node, Vec<usize>)> = Vec::new();
        for (index, child) in node.children.iter().enumerate() {
            path.push(index);
            if child.kind == NodeKind::Block
                && matches!(
                    child.macro_token.as_deref(),
                    Some("IP" | "TP" | "TQ" | "It")
                )
            {
                if run
                    .last()
                    .is_some_and(|(previous, _)| previous.flow_epoch != child.flow_epoch)
                {
                    self.classify(&run, "executed-flow-boundary");
                    run.clear();
                }
                // A nameless unsigned numeric label is not a declaration head
                // and can precede a valid named suffix (ffmpeg's 422/high/ss).
                // Do not generalize to every nameless owner: e.g. -1 has an
                // option-shaped source witness even when its final kind is Term.
                // Require an independent native numeric label, not merely a
                // damaged observed head with its names removed. Never infer
                // boundaries from the observed group's start/end. Missing owners are NOT
                // treated as non-declarations: keep their source obligation.
                let boundary = self
                    .native
                    .key(child)
                    .and_then(|key| self.observed.owners.get(&key))
                    .filter(|item| {
                        item.entry
                            .as_ref()
                            .is_none_or(|facts| facts.names.is_empty())
                    })
                    .and_then(|item| {
                        if bracket_head(child, item) {
                            Some("parameter-only-head")
                        } else if unsigned_numeric_head(child, item) {
                            Some("unsigned-numeric-head")
                        } else {
                            None
                        }
                    });
                if let Some(boundary) = boundary {
                    self.classify(&run, &format!("{boundary}-boundary"));
                    run.clear();
                    self.classify(&[(child, path.clone())], boundary);
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
            } else if !matches!(
                child.macro_token.as_deref(),
                Some("PD" | "Sm" | "Tg" | "ft")
            ) {
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
    let observed = ObservedOwners::collect(document);
    let native = NativeOwners::collect(root, &observed.list_owners);
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
        group_index,
        native,
    };
    audit.walk(root, &mut Vec::new());
    let ungrouped_runs = audit
        .rows
        .iter()
        .filter(|row| row["reason"] == "named-source-run-without-group")
        .count();
    let unexpected = audit.observed.groups.iter().enumerate().filter(|(i,_)| !audit.matched.contains(i))
        .map(|(index,sources)| json!({"group":index,"sources":sources,"reason":"no-compatible-source-run"})).collect::<Vec<_>>();
    json!({"sourceRuns":audit.rows,"observedGroups":audit.observed.groups,
        "unexpectedGroups":unexpected,"invalidGroups":audit.observed.invalid,
        // Kept for historical ledger readers. Ungrouped source runs are now
        // separately counted census evidence, never unresolved failures.
        "unresolvedRuns":0,"ungroupedRuns":ungrouped_runs})
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
    errors
}
