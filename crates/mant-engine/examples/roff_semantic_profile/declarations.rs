//! Independent, bidirectional source-run / final-group accounting.
//! This profiler does not assign names or repair IR. Unexplained rejected runs
//! are review candidates; an IR group without a compatible source run is a
//! violation. A source run may also contain layout-preserved, unaddressable
//! template heads; those do not invalidate a contiguous semantic sub-run.
use libmandoc_rs::{Node, NodeKind};
use mant_ir::{
    Block, ContentContext, DefinitionItem, Document, SourceSpan,
    visit::{self, Visit},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

type OwnerKey = (u32, u32, usize);

struct Observed<'a> {
    content: ContentContext<'a>,
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
                let sources = group.resolve(self.content, items).and_then(|members| {
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

fn single_visible_head(content: ContentContext<'_>, item: &DefinitionItem) -> Option<String> {
    let [term] = item.terms.as_slice() else {
        return None;
    };
    Some(super::inline_text(content, term))
}

fn source_parameter_head(node: &Node) -> Option<String> {
    // Share the production roff source-text projection for escapes; a second
    // audit decoder would drift on valid glyph, font, motion and spacing
    // controls. The AST role and owner boundary remain independently native.
    // Explicit mdoc Fl/Cm/Ic (or a man font macro) may declare a literal
    // bracketed name, so they must never be reinterpreted as presentation.
    // The projection must equal one complete final IR term to excuse a group.
    if !matches!(node.macro_name.as_deref(), Some("IP" | "TP" | "It")) {
        return None;
    }
    let label = source_head_node(node)?;
    if label.kind != NodeKind::Text || label.flags.no_print || !label.children.is_empty() {
        return None;
    }
    let visible = mant_codec::roff_source_visible_text_for_audit(label.text.as_deref()?);
    let label = visible.trim_matches([' ', '\t']);
    (label.starts_with('[')
        && label.ends_with(']')
        && label
            .chars()
            .any(|character| character.is_ascii_alphabetic()))
    .then_some(visible)
}

fn bracket_head(content: ContentContext<'_>, node: &Node, item: &DefinitionItem) -> bool {
    // CVS man_macro.c::blk_imp retains the actual next-line TP head. A
    // bracketed IR term alone is not independent source evidence: corrupting
    // the final head must not turn an unrelated declaration into an allowed
    // parameter boundary. Compare its *one complete term* with the native
    // text witness. Joining several IR terms would let a damaged owner fake
    // one source-proven parameter head.
    source_parameter_head(node).is_some_and(|source| {
        single_visible_head(content, item)
            .is_some_and(|head| source.trim_matches([' ', '\t']) == head.trim_matches([' ', '\t']))
    })
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

fn unsigned_numeric_head(content: ContentContext<'_>, node: &Node, item: &DefinitionItem) -> bool {
    native_numeric_label(node).is_some_and(|label| {
        single_visible_head(content, item)
            .is_some_and(|head| label == head.trim_matches([' ', '\t']))
    })
}

fn source_presentation_head(node: &Node) -> bool {
    // This deliberately recognizes only source-level presentation templates,
    // never an anonymous final IR owner. The AST remains immutable during
    // corruption tests, so a later lost name cannot turn a real declaration
    // into a permitted audit gap.
    native_mdoc_search_template(node)
        || native_man_option_template(node)
        || native_man_search_template(node)
        || native_title_head(node)
}

fn collect_mdoc_template_text(node: &Node, text: &mut String, has_argument: &mut bool) {
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
        collect_mdoc_template_text(child, text, has_argument);
    }
}

fn native_mdoc_search_template(node: &Node) -> bool {
    let Some(head) = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Head)
    else {
        return false;
    };
    let mut text = String::new();
    let mut has_argument = false;
    collect_mdoc_template_text(head, &mut text, &mut has_argument);
    has_argument
        && mant_codec::roff_source_visible_text_for_audit(&text)
            .trim_start()
            .starts_with('/')
}

fn source_head_node(node: &Node) -> Option<&Node> {
    let head = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Head)?;
    // CVS man_term.c::pre_TP ignores same-line width operands and renders
    // the first NODE_LINE child. IP/It retain their first-head contract.
    if node.macro_name.as_deref() == Some("TP") {
        head.children.iter().find(|child| child.flags.line_start)
    } else {
        head.children.first()
    }
}

fn source_head_text(node: &Node) -> Option<&str> {
    let mut current = source_head_node(node)?;
    loop {
        if current.kind == NodeKind::Text {
            return (!current.flags.no_print && current.children.is_empty())
                .then_some(current.text.as_deref()?);
        }
        if current.flags.no_print || current.text.is_some() || current.children.len() != 1 {
            return None;
        }
        current = &current.children[0];
    }
}

fn native_man_option_template(node: &Node) -> bool {
    // Perl-generated man pages use a historical spelling such as
    // `.IP \fB-\fR\fImin-len\fR`: it displays a dash followed by an italic
    // placeholder, rather than an option named `-min-len`. mandoc's man
    // formatter deliberately treats the head as visible layout text. Admit
    // this exact source witness as presentation-only so a later `-n`/`--long`
    // semantic group may still be proven, but never generalize it to a real
    // dash option or a damaged final owner.
    if !matches!(node.macro_name.as_deref(), Some("IP" | "TP")) {
        return false;
    }
    let Some(raw) = source_head_text(node) else {
        return false;
    };
    let Some((dash, parameter)) = raw.trim_matches([' ', '\t']).split_once(r"\fI") else {
        return false;
    };
    source_visible_dash(dash)
        && parameter
            .strip_suffix(r"\fR")
            .is_some_and(|name| !name.is_empty() && !name.contains(char::is_whitespace))
}

fn source_visible_dash(source: &str) -> bool {
    // Interpret only the historical two-byte font changes and escaped hyphen
    // emitted by generated man pages. This is intentionally not a roff
    // evaluator: an arbitrary escape cannot prove a presentation template.
    // The traditional bold/escaped-hyphen/reset and bold/hyphen/reset forms
    // both render exactly one dash.
    let mut visible = String::new();
    let mut characters = source.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            visible.push(character);
            continue;
        }
        match characters.next() {
            Some('f') if matches!(characters.next(), Some('B' | 'I' | 'R' | 'P' | '1'..='4')) => {}
            Some('-') => visible.push('-'),
            _ => return false,
        }
    }
    visible == "-"
}

fn native_man_search_template(node: &Node) -> bool {
    // Historic nvi/ex manuals encode search commands as a man TP head with an
    // RE placeholder and a carriage-return marker. It is visible syntax, not
    // a literal command that can be selected safely. Modern mdoc forms are
    // handled separately by native_mdoc_search_template.
    matches!(node.macro_name.as_deref(), Some("IP" | "TP"))
        && source_head_text(node).is_some_and(|text| {
            let text = text.trim_matches([' ', '\t']);
            if !matches!(text.as_bytes().first(), Some(b'/' | b'?')) {
                return false;
            }
            let rest = &text[1..];
            let Some(suffix) = rest.strip_prefix("RE") else {
                return false;
            };
            let Some(parameters) = suffix.strip_suffix("<carriage-return>") else {
                return false;
            };
            parameters.chars().all(|character| {
                character.is_ascii_alphabetic()
                    || matches!(character, '/' | '?' | '[' | ']' | ' ' | '-')
            })
        })
}

fn native_title_head(node: &Node) -> bool {
    // A plain, bodyless title in native list layout is presentation structure
    // rather than a selector. This covers generated contents/taxonomy labels
    // while preserving styled syntax, lower-case commands, option spelling,
    // and every label with readable owner content as audit obligations.
    if !matches!(node.macro_name.as_deref(), Some("IP" | "TP" | "It")) || body(node) {
        return false;
    }
    let Some(text) = source_head_text(node).map(str::trim) else {
        return false;
    };
    let mut characters = text.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if !first.is_ascii_uppercase()
        || text.starts_with('-')
        || text.contains(['=', ':', '<', '>', '[', ']'])
    {
        return false;
    }
    if text.contains('/') {
        return text.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || character.is_ascii_whitespace()
                || matches!(character, '/' | '-')
        });
    }
    characters.all(|character| {
        character.is_ascii_alphabetic() || character.is_ascii_whitespace() || character == '-'
    })
}

struct Audit<'a> {
    observed: Observed<'a>,
    rows: Vec<Value>,
    matched: BTreeSet<usize>,
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
                                    .is_some_and(|node| source_presentation_head(node))
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
            // Roff has no declaration-group construct. A contiguous run of
            // independently named IP heads can be a table of contents,
            // taxonomy, or several unrelated definitions with no shared
            // source body. It is therefore evidence worth preserving, but
            // cannot prove that lowering must have created one final group.
            // The reverse direction remains strict: every actual IR group
            // must still have an exact source-owner run. This avoids treating
            // ordinary presentation lists as false semantic regressions.
            "named-source-run-without-group"
        } else {
            // Without an actual final group, even a fully named native run
            // does not establish that all heads share one description. Keep
            // this source fact in the ledger, but do not promote it to a
            // product failure on inference alone.
            "named-source-run-without-group"
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
                // An unsigned numeric or bracketed parameter HEAD can split
                // a physical run before a separately proven declaration
                // suffix. EN04 can give such a HEAD a named Term fallback;
                // final names therefore cannot gate this *source* boundary.
                // Require the independent native text witness and the same
                // complete visible HEAD, not a damaged observed label or the
                // observed group's start/end. A missing owner still carries
                // its source obligation. A signed -1 is not unsigned numeric.
                let boundary = self
                    .observed
                    .owners
                    .get(
                        self.native_owners
                            .get(&(std::ptr::from_ref(child) as usize))
                            .unwrap_or(&(0, 0, usize::MAX)),
                    )
                    .and_then(|item| {
                        if bracket_head(self.observed.content, child, item) {
                            Some("parameter-only-head")
                        } else if unsigned_numeric_head(self.observed.content, child, item) {
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
    let mut observed = Observed {
        content: document.content(),
        owners: BTreeMap::new(),
        pointers: BTreeMap::new(),
        occurrences: BTreeMap::new(),
        list_owners: BTreeSet::new(),
        group_pointers: Vec::new(),
        groups: Vec::new(),
        invalid: 0,
    };
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
        group_index,
        native_owners,
        native_source_nodes,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tp_at_line(node: &Node, line: u32) -> Option<&Node> {
        (node.kind == NodeKind::Block
            && node.macro_name.as_deref() == Some("TP")
            && node.line == line)
            .then_some(node)
            .or_else(|| {
                node.children
                    .iter()
                    .find_map(|child| tp_at_line(child, line))
            })
    }

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

        // Fixed CVS man_macro.c::blk_imp retains each IP label; pre_IP in
        // man_term.c prints its first HEAD argument, while PD changes only
        // spacing. Independently recognizable options still form a valid
        // semantic group before we corrupt the final head.
        let source = b".TH PROBE 1\n.SH OPTIONS\n.IP \\fB--foo\\fR 4\n.PD 0\n.IP \\fB--high\\fR 4\n.IP \\fB--ss\\fR 4\n.PD\nSpatially Scalable\n";
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source)
            .unwrap();
        let mut document =
            mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
        assert!(violations(&profile(&native.document.root, &document)).is_empty());
        let atom = document
            .flow_mut()
            .unwrap()
            .content_store
            .atoms
            .iter_mut()
            .find(|atom| atom.kind.text() == Some("--foo"))
            .expect("source-proven first term");
        let mant_ir::ContentAtomKind::Text { text, .. } = &mut atom.kind else {
            panic!("text term atom");
        };
        *text = "422".to_owned();
        let Block::DefinitionList {
            items,
            declaration_groups,
            ..
        } = &mut document.flow_mut().unwrap().sections[0].blocks[0]
        else {
            panic!("definition list")
        };
        assert_eq!(declaration_groups[0].start_item, 0);
        assert_eq!(declaration_groups[0].end_item, 3);
        // Coupled corruption must not turn a real source declaration into a
        // numeric exception and then bless the shortened observed group.
        items[0].entry.as_mut().unwrap().names.clear();
        declaration_groups[0].start_item = 1;
        let changed = profile(&native.document.root, &document);
        assert!(!violations(&changed).is_empty(), "{changed}");
        assert_eq!(changed["unexpectedGroups"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn numeric_source_head_does_not_hide_an_independently_named_suffix_group() {
        // Reduced from ffmpeg-codecs(1)'s 422/high/ss profile list. Under
        // current conservative EN04 rules, generic high/ss terms do not by
        // themselves prove a shared description. The independently proven
        // --high/--ss options do. Fixed CVS man_term.c::pre_IP renders all
        // three physical heads, but does not provide a semantic group fact.
        let source = b".TH PROBE 1\n.SH OPTIONS\n.IP \\fB422\\fR 4\n.PD 0\n.IP \\fB--high\\fR 4\n.IP \\fB--ss\\fR 4\n.PD\nSpatially Scalable\n";
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

        let mut forged = document.clone();
        let Block::DefinitionList { items, .. } =
            &mut forged.flow_mut().unwrap().sections[0].blocks[0]
        else {
            panic!("definition list")
        };
        // A damaged owner with two terms, even if the second is invisible,
        // cannot use the native single-label exception to bless a suffix.
        items[0].terms.push(Vec::new());
        let split = profile(&native.document.root, &forged);
        assert_eq!(split["unexpectedGroups"].as_array().unwrap().len(), 1);

        for mutation in ["delete-group", "cross-unnamed-owner", "move-source-owner"] {
            let mut changed = document.clone();
            let Block::DefinitionList {
                items,
                declaration_groups,
                ..
            } = &mut changed.flow_mut().unwrap().sections[0].blocks[0]
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
            if mutation == "delete-group" {
                assert!(violations(&observed).is_empty(), "{mutation}: {observed}");
                assert!(
                    observed["sourceRuns"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|row| { row["reason"] == "named-source-run-without-group" })
                );
            } else {
                assert!(!violations(&observed).is_empty(), "{mutation}: {observed}");
            }
        }
    }

    #[test]
    fn signed_option_shaped_head_remains_in_the_source_run() {
        // Fixed CVS man_term.c::pre_IP renders the signed -1 head and the
        // later auto head as independent labels. Without an explicit Fl or
        // complete option declaration, EN04 keeps these generic Terms
        // ungrouped; source accounting must not misclassify -1 as unsigned.
        let source = b".TH PROBE 1\n.SH OPTIONS\n.IP \\fBseq_disp_ext\\fR 4\nEncoder configuration.\n.RS 4\n.IP \\fB\\-1\\fR 4\n.PD 0\n.IP \\fBauto\\fR 4\n.PD\nDecide automatically.\n.RE\n";
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source)
            .unwrap();
        let document =
            mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
        let valid = profile(&native.document.root, &document);
        assert_eq!(valid["observedGroups"], json!([]));
        assert!(violations(&valid).is_empty(), "{valid}");
        assert!(valid["sourceRuns"].as_array().unwrap().iter().any(|row| {
            row["physicalSources"] == json!([[6, 2, 0], [8, 2, 0]])
                && row["reason"] == "named-source-run-without-group"
        }));
        assert!(
            !valid["sourceRuns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["reason"] == "unsigned-numeric-head")
        );
    }

    #[test]
    fn generic_term_adjacency_does_not_prove_a_shared_description() {
        // Fixed CVS man_term.c::pre_IP renders all three separate labels and
        // only the final item's body. Neither blk_imp nor the formatter gives
        // generic foo/high/ss terms a shared semantic-description relation.
        let source = b".TH PROBE 1\n.SH OPTIONS\n.IP \\fBfoo\\fR 4\n.PD 0\n.IP \\fBhigh\\fR 4\n.IP \\fBss\\fR 4\n.PD\nSpatially Scalable\n";
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source)
            .unwrap();
        let document =
            mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
        let observed = profile(&native.document.root, &document);
        assert_eq!(observed["observedGroups"], json!([]), "{observed}");
        assert!(violations(&observed).is_empty(), "{observed}");
        assert!(
            observed["sourceRuns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| {
                    row["physicalSources"] == json!([[3, 2, 0], [5, 2, 0], [6, 2, 0]])
                        && row["reason"] == "named-source-run-without-group"
                })
        );
    }

    #[test]
    fn explicit_bracketed_literal_declaration_is_not_a_parameter_boundary() {
        // Fixed CVS mdoc_macro.c::blk_full and man_macro.c::blk_imp retain
        // each authored head. A Cm or B wrapper executes visible [foo] as a
        // literal declaration; unlike plain `[ argument ]`, it is not a
        // source-proven parameter-only head. In the third input the same-line
        // `[foo]` is a TP layout operand: pre_TP skips it and prints only the
        // following NODE_LINE B head.
        for source in [
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh COMMANDS\n.Bl -tag\n.It Cm [foo]\n.It Cm bar\nBody.\n.El\n"
                .as_slice(),
            b".TH PROBE 1\n.SH COMMANDS\n.TP\n.B [foo]\n.TP\n.B bar\nBody.\n",
            b".TH PROBE 1\n.SH COMMANDS\n.TP [foo]\n.B [foo]\n.TP\n.B bar\nBody.\n",
        ] {
            let native = libmandoc_rs::Parser::default()
                .parse_bytes("probe.1", source)
                .unwrap();
            let document =
                mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
            let observed = profile(&native.document.root, &document);
            assert_eq!(observed["observedGroups"].as_array().unwrap().len(), 1);
            assert!(violations(&observed).is_empty(), "{observed}");
            assert!(!observed["sourceRuns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["reason"] == "parameter-only-head"));
        }
    }

    #[test]
    fn plain_mdoc_parameter_head_preserves_the_independent_suffix_group() {
        // The exact input ran pinned CVS -Tutf8 before this assertion.
        // mdoc_macro.c::blk_full retains four authored It heads; mdoc_term.c
        // prints the bare [argument] as its own label. Unlike `Cm [foo]`, it
        // carries no semantic macro role and must not swallow the later Cm
        // declarations' independently proven shared description.
        let source = b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh COMMANDS\n.Bl -tag\n.It Cm first\n.It [argument]\n.It Cm second\n.It Cm third\nBody.\n.El\n";
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source)
            .unwrap();
        let document =
            mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
        let observed = profile(&native.document.root, &document);
        assert!(violations(&observed).is_empty(), "{observed}");
        assert_eq!(observed["observedGroups"].as_array().unwrap().len(), 1);
        assert!(
            observed["sourceRuns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| {
                    row["reason"] == "parameter-only-head"
                        && row["physicalSources"] == json!([[7, 2, 0]])
                })
        );
    }

    #[test]
    fn tp_width_operand_does_not_hide_a_later_plain_parameter_head() {
        // CVS man_term.c::pre_TP treats 4 as an invisible same-line width
        // and renders the following NODE_LINE text as the actual HEAD.
        // Audit this source boundary directly; semantic grouping of this
        // adjacent width form is a separate producer obligation.
        let source = b".TH PROBE 1\n.SH COMMANDS\n.TP\n.B first\n.TP 4\n\\fB      \\fP[ \\fIargument\\fP ]\n.TP\n.B second\n.TP\n.B third\nBody.\n";
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source)
            .unwrap();
        let document =
            mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
        let Block::DefinitionList { items, .. } = &document.flow().unwrap().sections[0].blocks[0]
        else {
            panic!("definition list")
        };
        let tp = tp_at_line(&native.document.root, 5).expect("second TP");
        assert_eq!(
            source_parameter_head(tp).as_deref(),
            Some("      [ argument ]")
        );
        assert!(bracket_head(document.content(), tp, &items[1]));
    }

    #[test]
    fn source_presentation_templates_use_the_executed_head_and_visible_escape_projection() {
        // Both exact inputs ran pinned CVS -Tutf8 first. In man_term.c,
        // pre_TP skips the same-line width and prints the next NODE_LINE;
        // term.c::term_word consumes the mdoc zero-width escape before the
        // visible slash. Neither control changes the authored source role.
        let man =
            b".TH PROBE 1\n.SH OPTIONS\n.TP 4\n\\fB-\\fR\\fImin-len\\fR\n.TP\n.B --long\nBody.\n";
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", man)
            .unwrap();
        let first = tp_at_line(&native.document.root, 3).expect("first TP");
        assert_eq!(source_head_text(first), Some(r"\fB-\fR\fImin-len\fR"));
        assert!(source_presentation_head(first));

        let mdoc = b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh COMMANDS\n.Bl -tag\n.It Xo\n.Pf \\&/ Ns Ar RE\n.Xc\n.It Cm second\n.It Cm third\nBody.\n.El\n";
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", mdoc)
            .unwrap();
        let document =
            mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), mdoc).unwrap();
        let observed = profile(&native.document.root, &document);
        assert!(violations(&observed).is_empty(), "{observed}");
        assert!(
            observed["sourceRuns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| {
                    row["reason"] == "source-run-semantic-subset"
                        && row["physicalSources"] == json!([[6, 2, 0], [9, 2, 0], [10, 2, 0]])
                })
        );
    }

    #[test]
    fn bounded_source_controls_preserve_one_native_parameter_boundary() {
        // Every exact input in this matrix ran pinned CVS -Tutf8 before the
        // assertions. roff_escape.c::roff_escape classifies fonts, named
        // glyphs and presentation controls; term.c::term_word executes them
        // before printing the same bracketed parameter head. The auditor
        // shares the production source projector, not its semantic decision.
        for (label, head) in [
            ("short-bold", r"\fB      \fP[ \fIargument\fP ]"),
            ("short-roman", r"\fR      \fP[ \fIargument\fP ]"),
            ("short-previous", r"\fP      \fP[ \fIargument\fP ]"),
            ("zero-width-then-font", r"\&\fB      \fP[ \fIargument\fP ]"),
            ("escaped-space", r"\       \fP[ \fIargument\fP ]"),
            ("named-font", r"\f[BI]      \fP[ \fIargument\fP ]"),
            ("two-byte-font", r"\f(BI      \fP[ \fIargument\fP ]"),
            ("nonbreaking-space", r"\~      \fP[ \fIargument\fP ]"),
            ("discretionary-zero-width", r"\%      \fP[ \fIargument\fP ]"),
            ("thin-zero-width", r"\fB      \fP[ \fIarg\|ument\fP ]"),
            ("narrow-zero-width", r"\fB      \fP[ \fIarg\^ument\fP ]"),
            ("numbered-space", r"\fB      \fP[ \fIarg\0ument\fP ]"),
            ("named-brackets", r"\[lB] argument \[rB]"),
            ("unicode-brackets", r"\[u005B] argument \[u005D]"),
            ("color-control", r"\m[red][ argument ]\m[]"),
            ("size-control", r"\s+1[ argument ]\s0"),
        ] {
            let source = format!(
                ".TH PROBE 1\n.SH COMMANDS\n.TP\n.B first\n.TP\n{head}\n.TP\n.B second\n.TP\n.B third\nBody.\n"
            );
            let native = libmandoc_rs::Parser::default()
                .parse_bytes("probe.1", source.as_bytes())
                .unwrap();
            let document =
                mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source.as_bytes())
                    .unwrap();
            let observed = profile(&native.document.root, &document);
            assert_eq!(
                observed["observedGroups"].as_array().unwrap().len(),
                1,
                "{label}: {observed}"
            );
            assert!(violations(&observed).is_empty(), "{label}: {observed}");
            assert!(
                source_parameter_head(tp_at_line(&native.document.root, 5).unwrap()).is_some(),
                "{label}"
            );
        }
        for head in [r"\f[BI      \fP[ \fIargument\fP ]"] {
            let source = format!(
                ".TH PROBE 1\n.SH COMMANDS\n.TP\n.B first\n.TP\n{head}\n.TP\n.B second\n.TP\n.B third\nBody.\n"
            );
            let native = libmandoc_rs::Parser::default()
                .parse_bytes("probe.1", source.as_bytes())
                .unwrap();
            assert!(
                source_parameter_head(tp_at_line(&native.document.root, 5).unwrap()).is_none(),
                "unsupported source escape: {head}"
            );
        }
    }

    #[test]
    fn semantic_subrun_can_follow_unaddressable_mdoc_templates() {
        // Reduced from the vi(1) search commands. The Ar-based /RE forms
        // are native presentation templates. Two explicit Cm commands then
        // provide the independent semantic evidence needed for the suffix
        // group. Fixed CVS mdoc_macro.c::blk_full closes each previous It;
        // mdoc_term.c::termp_it_pre renders all four distinct tag heads.
        let source = b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh COMMANDS\n.Bl -tag -width Ds\n.It Xo\n.Pf / Ns Ar RE\n.Xc\n.It Xo\n.Pf / Ns Ar RE Ns /\n.Xc\n.It Cm N\n.It Cm n\nSearch.\n.El\n";
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source)
            .unwrap();
        let document =
            mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
        let profile = profile(&native.document.root, &document);

        assert!(violations(&profile).is_empty(), "{profile}");
        assert!(profile["sourceRuns"].as_array().unwrap().iter().any(|row| {
            row["reason"] == "source-run-semantic-subset"
                && row["physicalSources"] == json!([[6, 2, 0], [9, 2, 0], [12, 2, 0], [13, 2, 0]])
                && row["observedGroup"] == 0
        }));
    }

    #[test]
    fn semantic_subrun_can_follow_source_proven_man_option_templates() {
        // Reduced from strings(1). The legacy `-min-len` heading is a
        // formatter-visible synopsis template, not an addressable option;
        // `-n` and `--bytes` are the independent names sharing its body.
        let source = b".TH PROBE 1\n.SH OPTIONS\n.IP \\fB\\-\\fR\\fImin\\-len\\fR 4\n.PD 0\n.IP \"\\fB\\-n\\fR \\fImin\\-len\\fR\" 4\n.IP \\fB\\-\\-bytes=\\fR\\fImin\\-len\\fR 4\n.PD\nDisplay strings.\n";
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source)
            .unwrap();
        let document =
            mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
        let valid = profile(&native.document.root, &document);
        assert!(violations(&valid).is_empty(), "{valid}");
        assert!(valid["sourceRuns"].as_array().unwrap().iter().any(|row| {
            row["reason"] == "source-run-semantic-subset"
                && row["physicalSources"] == json!([[3, 2, 0], [5, 2, 0], [6, 2, 0]])
        }));

        let mut corrupted = document;
        let Block::DefinitionList {
            declaration_groups, ..
        } = &mut corrupted.flow_mut().unwrap().sections[0].blocks[0]
        else {
            panic!("definition list")
        };
        declaration_groups.clear();
        let ungrouped = profile(&native.document.root, &corrupted);
        assert!(violations(&ungrouped).is_empty(), "{ungrouped}");
        assert!(
            ungrouped["sourceRuns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| { row["reason"] == "named-source-run-without-group" })
        );
    }

    #[test]
    fn semantic_subrun_can_follow_source_proven_man_search_templates() {
        // Reduced from the historic NetBSD ex(1) source. The RE form is a
        // visible edit-language template; N and n are the selectable commands
        // carrying its shared description.
        let source = b".TH PROBE 1\n.SH COMMANDS\n.TP\n.B \"?RE? [offset]<carriage-return>\"\n.TP\n.B N\n.TP\n.B n\nSearch forward or backward.\n";
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source)
            .unwrap();
        fn find_line(node: &Node, line: u32) -> Option<&Node> {
            (node.kind == NodeKind::Block && node.line == line)
                .then_some(node)
                .or_else(|| {
                    node.children
                        .iter()
                        .find_map(|child| find_line(child, line))
                })
        }
        assert!(native_man_search_template(
            find_line(&native.document.root, 3).expect("search template")
        ));
        let document =
            mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
        let valid = profile(&native.document.root, &document);
        assert!(violations(&valid).is_empty(), "{valid}");
        assert!(
            valid["sourceRuns"].as_array().unwrap().iter().any(|row| {
                row["reason"] == "source-run-retained"
                    && row["physicalSources"] == json!([[3, 2, 0], [5, 2, 0], [7, 2, 0]])
            }),
            "{valid}"
        );
    }

    #[test]
    fn plain_title_taxonomies_remain_ungrouped_census_evidence() {
        // Reduced from perltoc(1). Plain title-cased labels under a generic
        // heading are an index/taxonomy, not declarations sharing the final
        // item's prose. They remain visible individual terms, but must never
        // manufacture explanation context by adjacency alone.
        let source = b".TH PROBE 1\n.SH CONTENTS\n.IP Solution 4\n.PD 0\n.IP \"The Rest\" 4\n.IP Summary 4\n.IP Credits 4\n.PD\nContents.\n";
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source)
            .unwrap();
        let document =
            mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
        let valid = profile(&native.document.root, &document);
        assert!(violations(&valid).is_empty(), "{valid}");
        assert!(valid["observedGroups"].as_array().unwrap().is_empty());
        assert!(valid["sourceRuns"].as_array().unwrap().iter().any(|row| {
            row["reason"] == "named-source-run-without-group"
                && row["physicalSources"] == json!([[3, 2, 0], [5, 2, 0], [6, 2, 0], [7, 2, 0]])
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
            for block in &mut document.flow_mut().unwrap().sections[0].blocks {
                if let Block::DefinitionList {
                    declaration_groups, ..
                } = block
                {
                    declaration_groups.clear();
                }
            }
            let removed = profile(&native.document.root, &document);
            assert!(
                violations(&removed).is_empty(),
                "ungrouped source run is a census fact, not a false failure: {source}\n{removed}"
            );
            assert!(
                removed["sourceRuns"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| { row["reason"] == "named-source-run-without-group" })
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
        } = &mut document.flow_mut().unwrap().sections[0].blocks[0]
        else {
            panic!("definitions")
        };
        declaration_groups.remove(0);
        let ungrouped = profile(&native.document.root, &document);
        assert!(violations(&ungrouped).is_empty(), "{ungrouped}");
        assert!(
            ungrouped["sourceRuns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| { row["reason"] == "named-source-run-without-group" })
        );
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
        let mut forged = document.clone();
        let Block::DefinitionList { items, .. } =
            &mut forged.flow_mut().unwrap().sections[0].blocks[0]
        else {
            panic!("definition list")
        };
        items[1].terms.push(Vec::new());
        let split = profile(&parsed.document.root, &forged);
        assert_eq!(split["unexpectedGroups"].as_array().unwrap().len(), 1);
        let Block::DefinitionList {
            declaration_groups, ..
        } = &mut document.flow_mut().unwrap().sections[0].blocks[0]
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
    fn accounting_binds_actual_groups_and_records_ungrouped_runs() {
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
        } = &mut document.flow_mut().unwrap().sections[0].blocks[0]
        else {
            panic!()
        };
        declaration_groups.clear();
        let ungrouped = profile(&parsed.document.root, &document);
        assert!(violations(&ungrouped).is_empty(), "{ungrouped}");
        assert!(
            ungrouped["sourceRuns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| { row["reason"] == "named-source-run-without-group" })
        );
        let Block::DefinitionList {
            declaration_groups, ..
        } = &mut document.flow_mut().unwrap().sections[0].blocks[0]
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
