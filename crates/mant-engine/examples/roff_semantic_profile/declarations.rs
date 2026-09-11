//! Independent, bidirectional source-run / final-group accounting.
//! This profiler does not assign names or repair IR. Unexplained rejected runs
//! are review candidates; an IR group without a compatible source run is a
//! violation. A source run may also contain layout-preserved, unaddressable
//! template heads; those do not invalidate a contiguous semantic sub-run.
use libmandoc_rs::{Node, NodeKind};
use mant_ir::{
    Block, DefinitionItem, Document, SourceSpan,
    visit::{self, Visit},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

type OwnerKey = (u32, u32, usize);

#[derive(Default)]
struct Observed<'a> {
    owners: BTreeMap<OwnerKey, &'a DefinitionItem>,
    pointers: BTreeMap<usize, OwnerKey>,
    occurrences: BTreeMap<(u32, u32), usize>,
    list_owners: BTreeSet<OwnerKey>,
    group_pointers: Vec<Vec<usize>>,
    groups: Vec<Vec<OwnerKey>>,
    invalid: usize,
}
impl<'a> Visit<'a> for Observed<'a> {
    fn visit_list_item(&mut self, item: &'a mant_ir::ListItem) {
        // Ordinal/bullet conversion changes the block kind, not its place in
        // a repeated source-coordinate stream.
        if let Some(source) = item.source {
            let coordinate = key(source);
            let occurrence = self.occurrences.entry(coordinate).or_default();
            self.list_owners
                .insert((coordinate.0, coordinate.1, *occurrence));
            *occurrence += 1;
        }
        visit::walk_list_item(self, item);
    }
    fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
        if let Some(source) = item.source {
            let coordinate = key(source);
            let occurrence = self.occurrences.entry(coordinate).or_default();
            let key = (coordinate.0, coordinate.1, *occurrence);
            *occurrence += 1;
            self.owners.insert(key, item);
            self.pointers.insert(std::ptr::from_ref(item) as usize, key);
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
                        .map(|i| i.source.map(|_| std::ptr::from_ref(i) as usize))
                        .collect::<Option<Vec<_>>>()
                });
                if let Some(sources) = sources {
                    self.group_pointers.push(sources);
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

fn bracket_head(item: &DefinitionItem) -> bool {
    // Native text can still contain font escapes before `[`. Inspect the
    // source-correlated visible head, not raw roff bytes or a derived ID.
    item.terms
        .first()
        .is_some_and(|term| super::inline_text(term).trim_start().starts_with('['))
}

fn native_numeric_label(node: &Node) -> Option<String> {
    // IP's first HEAD child is the label; subsequent children are width
    // operands. Other macro shapes need their own independent source witness.
    if node.macro_name.as_deref() != Some("IP") {
        return None;
    }
    let head = node.children.iter().find(|n| n.kind == NodeKind::Head)?;
    let label = head.children.first()?;
    if label.kind != NodeKind::Text || label.flags.no_print || !label.children.is_empty() {
        return None;
    }
    literal_numeric_label(label.text.as_deref()?)
}

fn literal_numeric_label(text: &str) -> Option<String> {
    // This is deliberately not a roff evaluator. Only literal digits and
    // known one-character font switches prove a numeric label; every other
    // escape, request, or internal space leaves the source obligation intact.
    let mut bytes = text.trim_matches([' ', '\t']).bytes();
    let mut label = String::new();
    while let Some(byte) = bytes.next() {
        if byte.is_ascii_digit() {
            label.push(char::from(byte));
        } else if byte != b'\\'
            || bytes.next() != Some(b'f')
            || !matches!(bytes.next(), Some(b'B' | b'I' | b'R' | b'P' | b'1'..=b'4'))
        {
            return None;
        }
    }
    (!label.is_empty()).then_some(label)
}

fn unsigned_numeric_head(node: &Node, item: &DefinitionItem) -> bool {
    let [term] = item.terms.as_slice() else {
        return false;
    };
    let text = super::inline_text(term);
    native_numeric_label(node).is_some_and(|label| label == text.trim_matches([' ', '\t']))
}

fn native_template_head(node: &Node) -> bool {
    // This deliberately recognizes only a source-level mdoc command template
    // such as `.Pf / Ns Ar RE`, not every anonymous final IR owner. The AST
    // remains immutable during corruption tests, so a later lost name cannot
    // turn a real declaration into a permitted audit gap.
    let Some(head) = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Head)
    else {
        return false;
    };
    let mut text = String::new();
    let mut has_argument = false;
    fn collect(node: &Node, text: &mut String, has_argument: &mut bool) {
        if node.flags.no_print {
            return;
        }
        if node.macro_name.as_deref() == Some("Ar") {
            *has_argument = true;
        }
        if let Some(value) = node.text.as_deref() {
            text.push_str(value);
        }
        for child in &node.children {
            collect(child, text, has_argument);
        }
    }
    collect(head, &mut text, &mut has_argument);
    has_argument && text.trim_start().starts_with('/')
}

struct Audit<'a> {
    observed: Observed<'a>,
    rows: Vec<Value>,
    matched: BTreeSet<usize>,
    review: usize,
    group_index: BTreeMap<Vec<OwnerKey>, usize>,
    native_owners: BTreeMap<usize, OwnerKey>,
    native_source_nodes: BTreeMap<OwnerKey, &'a Node>,
}
impl Audit<'_> {
    fn retained_group(&self, sources: &[OwnerKey]) -> Option<(usize, &'static str)> {
        if let Some(index) = self.group_index.get(sources).copied() {
            return Some((index, "source-run-retained"));
        }

        // mdoc command references often collect several `.It Xo` forms under
        // one description. A leading `/RE` template is deliberately retained
        // as presentation because it has no literal selector, while a later
        // `?RE` or `n` can be a real semantic entry. The final declaration
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
                        sources[..start]
                            .iter()
                            .chain(&sources[start + group.len()..])
                            .all(|source| {
                                self.native_source_nodes
                                    .get(source)
                                    .is_some_and(|node| native_template_head(node))
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
            let Some(&source) = self
                .native_owners
                .get(&(std::ptr::from_ref(*node) as usize))
            else {
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
            "sourceOwners": run.iter().map(|(n,path)| json!({"astPath":path,"line":n.line,"column":n.column,"macro":n.macro_name,"flowEpoch":n.flow_epoch,"ownerOccurrence":self.native_owners.get(&(std::ptr::from_ref(*n) as usize)).map(|key|key.2)})).collect::<Vec<_>>(),
            "physicalSources":sources,"observedGroup":observed_group,
        }));
    }
    fn walk(&mut self, node: &Node, path: &mut Vec<usize>) {
        let mut run: Vec<(&Node, Vec<usize>)> = Vec::new();
        for (index, child) in node.children.iter().enumerate() {
            path.push(index);
            if child.kind == NodeKind::Block
                && matches!(child.macro_name.as_deref(), Some("IP" | "TP" | "TQ" | "It"))
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
                    .observed
                    .owners
                    .get(
                        self.native_owners
                            .get(&(std::ptr::from_ref(child) as usize))
                            .unwrap_or(&(0, 0, usize::MAX)),
                    )
                    .filter(|item| {
                        item.entry
                            .as_ref()
                            .is_none_or(|facts| facts.names.is_empty())
                    })
                    .and_then(|item| {
                        if bracket_head(item) {
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
    // Physical source coordinates can repeat during macro expansion. Preserve
    // every occurrence in syntax/IR traversal order instead of overwriting a
    // coordinate bucket. Deleted/reordered owners leave unmatched obligations.
    fn native_keys<'a>(
        node: &'a Node,
        counts: &mut BTreeMap<(u32, u32), usize>,
        keys: &mut BTreeMap<usize, OwnerKey>,
        source_nodes: &mut BTreeMap<OwnerKey, &'a Node>,
        list_owners: &BTreeSet<OwnerKey>,
    ) {
        let mut previous: Option<(&Node, OwnerKey)> = None;
        for child in &node.children {
            if child.kind == NodeKind::Block
                && matches!(child.macro_name.as_deref(), Some("IP" | "TP" | "It" | "TQ"))
            {
                // IP's first argument is its tag; later HEAD children are
                // layout widths, not term text (`.IP "" 4`).
                let headless = child
                    .children
                    .iter()
                    .find(|n| n.kind == NodeKind::Head)
                    .and_then(|head| head.children.first())
                    .is_none_or(|tag| !readable(tag));
                let continued =
                    previous.filter(|(before, _)| before.flow_epoch == child.flow_epoch);
                let merged = child.macro_name.as_deref() == Some("TQ")
                    && continued.is_some_and(|(before, _)| !body(before));
                let continuation = child.macro_name.as_deref() == Some("IP")
                    && headless
                    && continued
                        .is_some_and(|(before, key)| body(before) && !list_owners.contains(&key));
                if continuation {
                    // Headless IP is another paragraph of the previous owner,
                    // not a declaration in a source run.
                    native_keys(child, counts, keys, source_nodes, list_owners);
                    continue;
                }
                let source = if merged {
                    continued.expect("checked preceding owner").1
                } else {
                    let coordinate = (child.line, child.column);
                    let occurrence = counts.entry(coordinate).or_default();
                    let source = (coordinate.0, coordinate.1, *occurrence);
                    *occurrence += 1;
                    source
                };
                keys.insert(std::ptr::from_ref(child) as usize, source);
                source_nodes.insert(source, child);
                previous = Some((child, source));
            } else if !matches!(child.macro_name.as_deref(), Some("PD" | "Sm" | "Tg" | "ft")) {
                previous = None;
            }
            native_keys(child, counts, keys, source_nodes, list_owners);
        }
    }
    let mut observed = Observed::default();
    observed.visit_document(document);
    observed.groups = observed
        .group_pointers
        .iter()
        .map(|group| group.iter().map(|p| observed.pointers[p]).collect())
        .collect();
    let mut native_owners = BTreeMap::new();
    let mut native_source_nodes = BTreeMap::new();
    native_keys(
        root,
        &mut BTreeMap::new(),
        &mut native_owners,
        &mut native_source_nodes,
        &observed.list_owners,
    );
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
        native_owners,
        native_source_nodes,
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
    fn numeric_boundary_requires_independent_complete_native_evidence() {
        for (input, expected) in [
            ("422", Some("422")),
            (r"\fB422\fR", Some("422")),
            (r"\fB4\fI22\fP", Some("422")),
            ("foo", None),
            ("4 22", None),
            (r"\&422", None),
            (r"\n[digits]", None),
            (r"\f[unknown]422", None),
            (r"422\f", None),
            (r"\fB\fR", None),
        ] {
            assert_eq!(literal_numeric_label(input).as_deref(), expected, "{input}");
        }

        let source = b".TH PROBE 1\n.SH OPTIONS\n.IP \\fBfoo\\fR 4\n.PD 0\n.IP \\fBhigh\\fR 4\n.IP \\fBss\\fR 4\n.PD\nSpatially Scalable\n";
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source)
            .unwrap();
        let mut document =
            mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
        assert!(violations(&profile(&native.document.root, &document)).is_empty());
        let Block::DefinitionList {
            items,
            declaration_groups,
            ..
        } = &mut document.sections[0].blocks[0]
        else {
            panic!("definition list")
        };
        assert_eq!(declaration_groups[0].start_item, 0);
        assert_eq!(declaration_groups[0].end_item, 3);
        // Coupled corruption must not turn a real source declaration into a
        // numeric exception and then bless the shortened observed group.
        items[0].terms = vec![vec![mant_ir::Inline::Text {
            value: "422".into(),
        }]];
        items[0].entry.as_mut().unwrap().names.clear();
        declaration_groups[0].start_item = 1;
        let changed = profile(&native.document.root, &document);
        assert!(!violations(&changed).is_empty(), "{changed}");
        assert_eq!(changed["unexpectedGroups"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn unnamed_source_head_does_not_invalidate_or_hide_a_named_suffix_group() {
        // Reduced from ffmpeg-codecs(1)'s 422/high/ss profile list. A numeric
        // label has no exact discovered name; high/ss still form a contiguous
        // named source run ending at the independently authored description.
        let source = b".TH PROBE 1\n.SH OPTIONS\n.IP \\fB422\\fR 4\n.PD 0\n.IP \\fBhigh\\fR 4\n.IP \\fBss\\fR 4\n.PD\nSpatially Scalable\n";
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source)
            .unwrap();
        let document =
            mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
        let valid = profile(&native.document.root, &document);
        assert_eq!(valid["observedGroups"].as_array().unwrap().len(), 1);
        assert!(violations(&valid).is_empty(), "{valid}");
        assert!(valid["sourceRuns"].as_array().unwrap().iter().any(|row| {
            row["reason"] == "unsigned-numeric-head" && row["physicalSources"] == json!([[3, 2, 0]])
        }));

        for mutation in ["delete-group", "cross-unnamed-owner", "move-source-owner"] {
            let mut changed = document.clone();
            let Block::DefinitionList {
                items,
                declaration_groups,
                ..
            } = &mut changed.sections[0].blocks[0]
            else {
                panic!("definition list")
            };
            match mutation {
                "delete-group" => declaration_groups.clear(),
                "cross-unnamed-owner" => declaration_groups[0].start_item = 0,
                "move-source-owner" => {
                    let source = items[1].source;
                    items[1].source = items[0].source;
                    items[0].source = source;
                }
                _ => unreachable!(),
            }
            let observed = profile(&native.document.root, &changed);
            assert!(!violations(&observed).is_empty(), "{mutation}: {observed}");
        }
    }

    #[test]
    fn signed_option_shaped_head_remains_in_the_source_run() {
        let source = b".TH PROBE 1\n.SH OPTIONS\n.IP \\fBseq_disp_ext\\fR 4\nEncoder configuration.\n.RS 4\n.IP \\fB\\-1\\fR 4\n.PD 0\n.IP \\fBauto\\fR 4\n.PD\nDecide automatically.\n.RE\n";
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source)
            .unwrap();
        let document =
            mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
        let valid = profile(&native.document.root, &document);
        assert_eq!(valid["observedGroups"].as_array().unwrap().len(), 1);
        assert!(violations(&valid).is_empty(), "{valid}");
    }

    #[test]
    fn semantic_subrun_can_follow_unaddressable_mdoc_templates() {
        // Reduced from the vi(1) search commands. `/RE` forms use an authored
        // regular-expression placeholder and deliberately have no exact
        // selector; `?RE` and `n` are addressable command heads sharing the
        // same physical mdoc description run.
        let source = b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh COMMANDS\n.Bl -tag -width Ds\n.It Xo\n.Pf / Ns Ar RE\n.Xc\n.It Xo\n.Pf / Ns Ar RE Ns /\n.Xc\n.It Xo\n.Pf ? Ns Ar RE\n.Xc\n.It Cm n\nSearch.\n.El\n";
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source)
            .unwrap();
        let document =
            mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
        let profile = profile(&native.document.root, &document);

        assert!(violations(&profile).is_empty(), "{profile}");
        assert!(profile["sourceRuns"].as_array().unwrap().iter().any(|row| {
            row["reason"] == "source-run-semantic-subset"
                && row["physicalSources"] == json!([[6, 2, 0], [9, 2, 0], [12, 2, 0], [15, 2, 0]])
                && row["observedGroup"] == 0
        }));
    }

    #[test]
    fn headless_ip_layout_arguments_and_non_definition_predecessors_preserve_obligations() {
        for prefix in [
            ".TP\n.B -c\n.TP\n.B -d\nFirst body.\n.IP \"\" 4\nTail.\n",
            ".IP \\(bu\nBullet body.\n.IP\nTail.\n",
        ] {
            let source = format!(
                ".TH PROBE 1\n.SH OPTIONS\n.de PAIR\n{prefix}.TP\n.B -a\n.TP\n.B -b\nSecond body.\n..\n.PAIR\n"
            );
            let native = libmandoc_rs::Parser::default()
                .parse_bytes("probe.1", source.as_bytes())
                .unwrap();
            let mut document =
                mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source.as_bytes())
                    .unwrap();
            let original = profile(&native.document.root, &document);
            assert!(violations(&original).is_empty(), "{source}\n{original}");
            for block in &mut document.sections[0].blocks {
                if let Block::DefinitionList {
                    declaration_groups, ..
                } = block
                {
                    declaration_groups.clear();
                }
            }
            let removed = profile(&native.document.root, &document);
            assert!(
                !violations(&removed).is_empty(),
                "deleted groups were silently accepted: {source}\n{removed}"
            );
        }
    }
    #[test]
    fn same_coordinate_stream_accounts_for_explicit_heads_and_list_conversion() {
        for prefix in ["", ".IP 1.\nA numbered step.\n", ".IP \\(bu\nA bullet.\n"] {
            for continuation in ["", ".TQ\n.B --all\n"] {
                let source = format!(
                    ".TH PROBE 1\n.SH OPTIONS\n.de PAIR\n{prefix}.TP\n.B -a\n{continuation}.TP\n.B -b\nBody.\n.IP\nTail.\n..\n.PAIR\n.PAIR\n"
                );
                let native = libmandoc_rs::Parser::default()
                    .parse_bytes("probe.1", source.as_bytes())
                    .unwrap();
                let document = mant_loader::parse_manual_bytes(
                    std::path::Path::new("probe.1"),
                    source.as_bytes(),
                )
                .unwrap();
                let result = profile(&native.document.root, &document);
                assert_eq!(
                    result["observedGroups"].as_array().unwrap().len(),
                    2,
                    "{source}\n{result}"
                );
                assert!(violations(&result).is_empty(), "{source}\n{result}");
            }
        }
    }
    #[test]
    fn macro_expansion_retains_every_same_coordinate_owner_occurrence() {
        let source = b".TH PROBE 1\n.SH OPTIONS\n.de PAIR\n.TP\n.B -a\n.TP\n.B -b\nBody.\n..\n.PAIR\n.PAIR\n";
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source)
            .unwrap();
        let mut document =
            mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
        let original = profile(&native.document.root, &document);
        assert_eq!(original["observedGroups"].as_array().unwrap().len(), 2);
        assert!(violations(&original).is_empty(), "{original}");
        let Block::DefinitionList {
            declaration_groups, ..
        } = &mut document.sections[0].blocks[0]
        else {
            panic!("definitions")
        };
        declaration_groups.remove(0);
        assert!(!violations(&profile(&native.document.root, &document)).is_empty());
    }
    #[test]
    fn parameter_continuation_separates_runs_without_hiding_crossing_groups() {
        let source = b".TH PROBE 1\n.SH COMMANDS\n.TP\n.B first\n.TP\n\\fB      \\fP[ \\fIargument\\fP ]\n.TP\n.B second\n.TP\n.B third\nBody.\n";
        let parsed = libmandoc_rs::Parser::new(Default::default())
            .parse_bytes("probe.1", source)
            .unwrap();
        let mut document = mant_loader::load_roff_bytes(source)
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
        let mut document = mant_loader::load_roff_bytes(source)
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
