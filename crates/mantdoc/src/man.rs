//! First structural pass for the traditional man(7) macro package.
//!
//! Roff execution deliberately emits a flat, source-ordered event stream.
//! This pass reorganizes those already-expanded nodes instead of rescanning
//! input bytes, so generated macro calls and resolver-owned source positions
//! retain the same arena records.  M4 grows this table incrementally; unknown
//! macros remain ordinary elements in the active body.

use crate::{MacroSet, NodeId, NodeKind, SourcePosition, SourceSpan, ast::DocumentBuilder};

mod driver;
mod metadata;
use metadata::{
    record_title_metadata, title_argument, title_argument_missing, title_date_argument,
    title_lowercase, title_missing_date, title_section_argument, title_section_missing,
    title_unparseable_date,
};
mod tags;
use tags::{append_to_active, make_block, mark_man_targets};
mod recovery;
pub(crate) use driver::structure;
pub(crate) use recovery::Recovery;

#[cfg(test)]
mod tests;

/// Bounded semantic-restructuring result consumed by the parser boundary.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct StructureOutcome {
    /// First source location whose `Head`/`Body` pair could not fit.
    pub(crate) node_limit_location: Option<SourceSpan>,
    /// Recoverable man scope findings, retained in source order.
    pub(crate) recoveries: Vec<Recovery>,
    /// No `.TH` request occurred, so a configured OS fallback must not be
    /// applied as if this were a titled man document.
    pub(crate) missing_title_control: bool,
}

fn remove_child(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    parent: NodeId,
    child: NodeId,
) {
    if parent == root {
        root_children.retain(|candidate| *candidate != child);
    } else if let Some(mut children) = builder.children(parent).map(<[NodeId]>::to_vec) {
        children.retain(|candidate| *candidate != child);
        let _ = builder.replace_children(parent, &children);
    }
}

/// Validate the finite argument surface of the man-ext optional-argument
/// macro without changing its ordinary inline AST projection.  Unlike `MT`,
/// `OP` retains all scanner children even after reporting a superfluous one.
fn validate_option_arguments(
    builder: &DocumentBuilder,
    node: NodeId,
    outcome: &mut StructureOutcome,
) {
    let arguments = builder.children(node).unwrap_or_default();
    if arguments.is_empty() {
        outcome.recoveries.push(Recovery::MissingOption {
            macro_name: "OP",
            location: builder.node_location(node),
        });
    } else if let Some(excess) = arguments.get(2).copied() {
        outcome.recoveries.push(Recovery::ExcessArguments {
            macro_name: "OP",
            argument: builder.node_text(excess).unwrap_or_default().into(),
            location: builder.node_location(excess),
        });
    }
}

/// Report and remove the first ignored argument of a fixed-arity man extension
/// macro.  `.PD` publishes only its accepted spacing argument in the AST.
fn validate_maximum_arguments(
    builder: &mut DocumentBuilder,
    node: NodeId,
    macro_name: &'static str,
    maximum: usize,
    outcome: &mut StructureOutcome,
) {
    let Some(arguments) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some(excess) = arguments.get(maximum).copied() else {
        return;
    };
    outcome.recoveries.push(Recovery::ExcessArguments {
        macro_name,
        argument: builder.node_text(excess).unwrap_or_default().into(),
        location: builder.node_location(excess),
    });
    let _ = builder.replace_children(node, &arguments[..maximum]);
}

/// Report the legacy summary for arguments ignored by zero-argument paragraph
/// controls. These controls retain their scanner children in the public tree;
/// their compatibility behavior is solely the source-order diagnostic.
fn validate_no_arguments(
    builder: &DocumentBuilder,
    node: NodeId,
    macro_name: &'static str,
    outcome: &mut StructureOutcome,
) {
    let Some(arguments) = builder.children(node) else {
        return;
    };
    let Some(first) = arguments.first().copied() else {
        return;
    };
    outcome.recoveries.push(Recovery::AllArguments {
        macro_name,
        first_argument: builder.node_text(first).unwrap_or_default().into(),
        has_more: arguments.len() > 1,
        location: builder.node_location(node),
    });
}

/// Roff's fill-mode requests accept no arguments.  Unlike the man paragraph
/// aliases, their entire raw tail is discarded and the native finding keeps
/// the complete spelling used by `MANDOCERR_ARG_SKIP`.
fn validate_and_discard_all_arguments(
    builder: &mut DocumentBuilder,
    node: NodeId,
    macro_name: &'static str,
    outcome: &mut StructureOutcome,
) {
    let Some(arguments) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some(first) = arguments.first().copied() else {
        return;
    };
    let arguments = arguments
        .iter()
        .filter_map(|argument| builder.node_text(*argument))
        .collect::<Vec<_>>()
        .join(" ");
    outcome.recoveries.push(Recovery::IgnoredArguments {
        macro_name,
        arguments: arguments.into(),
        location: builder.node_location(first),
    });
    let _ = builder.replace_children(node, &[]);
}

/// Parse `.RE`'s optional numeric target and preserve the legacy distinction
/// between a suffix on that number and a separately lexed argument.
fn re_target(
    builder: &DocumentBuilder,
    node: NodeId,
    outcome: &mut StructureOutcome,
) -> Option<usize> {
    let arguments = builder.children(node)?;
    let first = *arguments.first()?;
    let text = builder.node_text(first).unwrap_or_default();
    let prefix = text.bytes().take_while(u8::is_ascii_digit).count();
    if prefix < text.len() {
        outcome.recoveries.push(Recovery::ExcessArguments {
            macro_name: "RE",
            argument: text[prefix..].into(),
            location: argument_suffix_location(builder, first, prefix),
        });
    }
    for extra in &arguments[1..] {
        outcome.recoveries.push(Recovery::ExcessArguments {
            macro_name: "RE",
            argument: builder.node_text(*extra).unwrap_or_default().into(),
            location: builder.node_location(*extra),
        });
    }
    Some(text[..prefix].parse::<usize>().unwrap_or(0).max(1))
}

fn argument_suffix_location(
    builder: &DocumentBuilder,
    argument: NodeId,
    prefix: usize,
) -> Option<SourceSpan> {
    let mut location = builder.node_location(argument)?;
    let offset =
        u32::try_from(prefix + usize::from(builder.node_argument_quoted(argument))).ok()?;
    location.start = location.start.checked_add(offset)?;
    if let Some(position) = location.logical_start.as_mut() {
        position.column = position.column.checked_add(offset)?;
    }
    (location.start <= location.end).then_some(location)
}

fn blank_line_location(builder: &DocumentBuilder, node: NodeId) -> Option<SourceSpan> {
    let mut location = builder.node_location(node)?;
    let position = builder.node_source_position(node)?;
    location.logical_start = Some(SourcePosition {
        line: position.line,
        column: 1,
    });
    Some(location)
}

#[derive(Clone, Copy)]
struct ScopeFrame {
    open: NodeId,
    body: NodeId,
    resume_active: NodeId,
    resume_flow: NodeId,
}

enum IndentClose {
    Closed { frames: Vec<ScopeFrame> },
    NotOpen,
    FewerOpen { target: usize },
}

#[derive(Clone, Copy)]
struct ExplicitFrame {
    close: &'static str,
    open: NodeId,
    body: NodeId,
    resume_active: NodeId,
    resume_flow: NodeId,
}

fn normalize_pending_term_indent(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(argument) = builder
        .children(node)
        .and_then(|children| children.first())
        .copied()
    else {
        return;
    };
    let Some(value) = builder.node_text(argument) else {
        return;
    };
    if value.is_empty() || value.starts_with('+') || value.starts_with('-') {
        return;
    }
    let _ = builder.set_node_text(argument, format!("+{value}"));
}

#[derive(Clone, Copy, Debug)]
struct PendingHead {
    head: NodeId,
    body: Option<NodeId>,
    is_term: bool,
    block_parent: Option<(NodeId, NodeId)>,
}

impl PendingHead {
    const fn ordinary(block: NodeId, head: NodeId, body: NodeId, parent: NodeId) -> Self {
        Self {
            head,
            body: Some(body),
            is_term: false,
            block_parent: Some((block, parent)),
        }
    }

    const fn term(block: NodeId, head: NodeId, body: NodeId, parent: NodeId) -> Self {
        Self {
            head,
            body: Some(body),
            is_term: true,
            block_parent: Some((block, parent)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PendingElement {
    node: NodeId,
    parent: NodeId,
}

fn clear_pending(
    pending_head: &mut Option<PendingHead>,
    pending_element: &mut Option<PendingElement>,
) {
    *pending_head = None;
    *pending_element = None;
}

fn block_head_is_pending(builder: &DocumentBuilder, block: NodeId, name: &str) -> bool {
    matches!(name, "SH" | "SS")
        && builder
            .children(block)
            .and_then(|parts| parts.first())
            .and_then(|head| builder.children(*head))
            .is_some_and(<[NodeId]>::is_empty)
}

fn is_next_line_scoped_element(name: &str) -> bool {
    matches!(name, "SM" | "SB" | "R" | "B" | "I")
}

fn is_line_scoped_element(name: &str) -> bool {
    matches!(name, "SM" | "SB" | "R" | "B" | "I")
}

fn section_scope_name(builder: &DocumentBuilder, body: NodeId) -> &'static str {
    if builder.node_macro_name(body) == Some("SS") {
        "SS"
    } else {
        "SH"
    }
}

fn is_term_scope_breaker(name: &str) -> bool {
    matches!(
        name,
        "SH" | "SS"
            | "TP"
            | "TQ"
            | "IP"
            | "HP"
            | "LP"
            | "PP"
            | "P"
            | "RS"
            | "RE"
            | "UR"
            | "UE"
            | "MT"
            | "ME"
            | "SY"
            | "YS"
            | "TS"
    )
}

fn is_line_scope_breaker(name: &str) -> bool {
    is_term_scope_breaker(name)
}

fn line_scope_macro_name(builder: &DocumentBuilder, node: NodeId) -> Option<&'static str> {
    match builder.node_macro_name(node) {
        Some("B") => Some("B"),
        Some("I") => Some("I"),
        Some("R") => Some("R"),
        Some("SM") => Some("SM"),
        Some("SB") => Some("SB"),
        _ => None,
    }
}

/// Return line-scoped font Elements on the root-to-target path, excluding the
/// target itself. A nested empty font at EOF needs one validator finding for
/// each still-open outer scope, emitted from inner to outer.
fn line_scope_ancestors(builder: &DocumentBuilder, root: NodeId, target: NodeId) -> Vec<NodeId> {
    fn visit(
        builder: &DocumentBuilder,
        node: NodeId,
        target: NodeId,
        ancestors: &mut Vec<NodeId>,
    ) -> bool {
        if node == target {
            return true;
        }
        let scoped = line_scope_macro_name(builder, node).is_some();
        if scoped {
            ancestors.push(node);
        }
        if let Some(children) = builder.children(node) {
            for child in children {
                if visit(builder, *child, target, ancestors) {
                    return true;
                }
            }
        }
        if scoped {
            let _ = ancestors.pop();
        }
        false
    }

    let mut ancestors = Vec::new();
    if visit(builder, root, target, &mut ancestors) {
        ancestors
    } else {
        Vec::new()
    }
}

/// man(7) exposes an effective `\t` in a visible macro argument as a layout
/// tab, unlike the generic package AST escape projection.  The scanner keeps
/// a private copy-mode bit so an authored `\\t` remains literal text.
fn normalize_visible_macro_tabulation_escapes(builder: &mut DocumentBuilder, node: NodeId) {
    if !builder
        .node_macro_name(node)
        .is_some_and(is_visible_macro_with_tabulation_arguments)
    {
        return;
    }
    let Some(arguments) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    for argument in arguments {
        let value = decode_man_tabulation_escapes(builder, argument);
        if value != builder.node_text(argument).unwrap_or_default() {
            let _ = builder.set_node_text(argument, value);
        }
    }
}

fn is_visible_macro_with_tabulation_arguments(name: &str) -> bool {
    matches!(
        name,
        "B" | "I"
            | "R"
            | "SM"
            | "SB"
            | "BR"
            | "BI"
            | "IB"
            | "IR"
            | "RB"
            | "RI"
            | "IP"
            | "HP"
            | "TP"
            | "TQ"
    )
}

/// The scanner tokenizes arguments for safe roff expansion, whereas these
/// man inline macros retain one line argument in libmandoc's public AST.
/// Reusing the first token preserves its exact source position without a
/// second arena allocation.
fn coalesce_text_children(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(children) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some(&first) = children.first() else {
        return;
    };
    if children.len() == 1 {
        return;
    }
    let value = children
        .iter()
        .filter_map(|child| builder.node_text(*child))
        .collect::<Vec<_>>()
        .join(" ");
    if builder.text(first, value) {
        let _ = builder.replace_children(node, &[first]);
    }
}

/// `man_validate` treats tab-separated `.IP` tag words as one tag argument,
/// while retaining a later space-separated width as a second head child.  The
/// scanner deliberately tokenizes both separators so roff expansion remains
/// safe; reconstruct the package-level tag shape here before the temporary
/// separators are discarded.
fn coalesce_ip_tab_separated_tag(builder: &mut DocumentBuilder, head: NodeId) {
    let Some(children) = builder.children(head).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some(&first) = children.first() else {
        return;
    };

    let mut value = decode_man_tabulation_escapes(builder, first);
    let mut changed = value.as_str() != builder.node_text(first).unwrap_or_default();
    let mut last = 0;
    while last + 1 < children.len() && builder.node_separator_after(children[last]) == Some(b'\t') {
        last += 1;
    }
    for index in 1..=last {
        let width = builder.node_separator_width(children[index - 1]).max(1);
        value.extend(std::iter::repeat_n('\t', width as usize));
        value.push_str(&decode_man_tabulation_escapes(builder, children[index]));
        changed = true;
    }
    if !changed || !builder.set_node_text(first, value) {
        return;
    }
    if last == 0 {
        return;
    }
    let _ = builder.set_node_separator_after(first, builder.node_separator_after(children[last]));
    let _ = builder
        .set_node_separator_width(first, builder.node_separator_width(children[last]) as usize);

    let mut retained = Vec::with_capacity(children.len() - last);
    retained.push(first);
    retained.extend_from_slice(&children[(last + 1)..]);
    let _ = builder.replace_children(head, &retained);
}

fn decode_man_tabulation_escapes(builder: &DocumentBuilder, node: NodeId) -> String {
    let value = builder.node_text(node).unwrap_or_default();
    if builder.node_has_protected_tabulation_escape(node) {
        value.to_owned()
    } else {
        decode_unescaped_tabulation_escapes(value)
    }
}

/// Decode a tabulation escape only when it was not protected by a preceding
/// escaped backslash.  `man_validate` treats an effective `\t` in an `.IP`
/// tag as the same layout tabulation as a literal tab, but `\\t` remains
/// authored text.
fn decode_unescaped_tabulation_escapes(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = String::with_capacity(value.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\\' && bytes.get(cursor + 1) == Some(&b'\\') {
            decoded.push('\\');
            cursor += 2;
        } else if bytes[cursor] == b'\\' && bytes.get(cursor + 1) == Some(&b't') {
            decoded.push('\t');
            cursor += 2;
        } else {
            let Some(character) = value[cursor..].chars().next() else {
                break;
            };
            decoded.push(character);
            cursor += character.len_utf8();
        }
    }
    decoded
}

/// Attach the consecutive input lines owned by roff's `.ce` and `.rj`
/// requests before man block restructuring.  The roff parser records these
/// as Element children, even though the request itself is presentation-only.
/// A following macro aborts the pending request rather than becoming content.
fn attach_centered_input_lines(builder: &mut DocumentBuilder, flat: &[NodeId]) -> Vec<NodeId> {
    let mut attached = Vec::new();
    for (index, node) in flat.iter().copied().enumerate() {
        if !matches!(builder.node_macro_name(node), Some("ce" | "rj")) {
            continue;
        }
        let lines = builder
            .children(node)
            .and_then(|children| children.first())
            .and_then(|argument| builder.node_text(*argument))
            .and_then(|argument| argument.parse::<usize>().ok())
            .filter(|lines| *lines > 0)
            .unwrap_or(1);
        let mut text_nodes = Vec::with_capacity(lines);
        for candidate in flat.iter().copied().skip(index + 1) {
            if text_nodes.len() == lines || builder.node_kind(candidate) != Some(NodeKind::Text) {
                break;
            }
            text_nodes.push(candidate);
        }
        if text_nodes.is_empty() {
            continue;
        }
        let mut children = builder
            .children(node)
            .map(<[NodeId]>::to_vec)
            .unwrap_or_default();
        children.extend(text_nodes.iter().copied());
        if builder.replace_children(node, &children) {
            attached.extend(text_nodes);
        }
    }
    attached
}

fn close_indents(
    _builder: &DocumentBuilder,
    indents: &mut Vec<ScopeFrame>,
    active_body: &mut NodeId,
    flow_parent: &mut NodeId,
    target: Option<usize>,
) -> IndentClose {
    let levels = match target {
        None => 1,
        Some(target) if target > indents.len() => return IndentClose::FewerOpen { target },
        Some(target) => indents.len() + 1 - target,
    };
    let mut frames = Vec::with_capacity(levels);
    for _ in 0..levels {
        let Some(frame) = indents.pop() else {
            return IndentClose::NotOpen;
        };
        *active_body = frame.resume_active;
        *flow_parent = frame.resume_flow;
        frames.push(frame);
    }
    IndentClose::Closed { frames }
}

fn close_explicit(
    close: &str,
    explicit_blocks: &mut Vec<ExplicitFrame>,
    active_body: &mut NodeId,
    flow_parent: &mut NodeId,
) -> Option<ExplicitFrame> {
    let index = explicit_blocks
        .iter()
        .rposition(|frame| frame.close == close)?;
    let frame = explicit_blocks[index];
    explicit_blocks.truncate(index);
    *active_body = frame.resume_active;
    *flow_parent = frame.resume_flow;
    Some(frame)
}

/// Apply the man presentation flags that are stateful but do not affect roff
/// expansion.  This must run against source-order scanner events before they
/// are moved into blocks, otherwise a later `.fi` could accidentally change
/// the interpretation of text that preceded it in the arena.
fn apply_presentation_flags(builder: &mut DocumentBuilder, flat: &[NodeId]) {
    let mut no_fill = false;
    for node in flat {
        let macro_name = builder.node_macro_name(*node).map(str::to_owned);
        let presentation_toggle = matches!(macro_name.as_deref(), Some("nf" | "fi" | "EX" | "EE"));
        if presentation_toggle {
            // libmandoc records the controlling macro in the state that
            // preceded it, while the macro's arguments already observe its
            // new fill mode. For example, `.EX args` itself is filled but
            // its arguments are no-fill; `.EE args` is no-fill while its
            // arguments are filled. Preserve that distinction in the arena
            // rather than applying one flag to the entire scanner subtree.
            if no_fill {
                mark_node_no_fill(builder, *node);
            }
            no_fill = matches!(macro_name.as_deref(), Some("nf" | "EX"));
            if no_fill {
                mark_children_no_fill(builder, *node);
            }
        } else if no_fill {
            mark_subtree_no_fill(builder, *node);
        }
        mark_sentence_end(builder, *node);
    }
}

/// In filled man input, a final `\c` immediately before a blank physical
/// line is recovered as ordinary text without the control spelling; the
/// blank line itself is not published. In no-fill mode both source events
/// remain observable. Perform this after the presentation pass, when the
/// scanner-owned nodes carry their effective fill state but before they are
/// assigned to public man blocks.
fn suppress_filled_c_blank_lines(builder: &mut DocumentBuilder, flat: &[NodeId]) -> Vec<NodeId> {
    let mut suppressed = Vec::new();
    for pair in flat.windows(2) {
        let [text, blank] = pair else {
            continue;
        };
        if builder.node_kind(*text) != Some(NodeKind::Text)
            || builder.node_kind(*blank) != Some(NodeKind::Text)
            || builder.node_text(*blank) != Some("")
        {
            continue;
        }
        let Some(value) = builder.node_text(*text).map(str::to_owned) else {
            continue;
        };
        let Some(value) = value.strip_suffix("\\c") else {
            continue;
        };
        let Some(mut flags) = builder.node_flags(*text) else {
            continue;
        };
        if flags.no_fill || !flags.line_continuation || value.ends_with("\\z") {
            continue;
        }
        if builder.text(*text, value) {
            flags.line_continuation = false;
            let _ = builder.set_node_flags(*text, flags);
            suppressed.push(*blank);
        }
    }
    suppressed
}

fn mark_subtree_no_fill(builder: &mut DocumentBuilder, root: NodeId) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        mark_node_no_fill(builder, node);
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().copied());
        }
    }
}

#[allow(clippy::unnecessary_to_owned)] // The owned edge list ends the arena borrow before mutation.
fn mark_children_no_fill(builder: &mut DocumentBuilder, root: NodeId) {
    let Some(children) = builder.children(root) else {
        return;
    };
    // Copy the narrow edge slice before recursive flag mutation so the arena
    // does not carry an immutable borrow across its mutable traversal.
    let children = children.to_vec();
    for child in children {
        mark_subtree_no_fill(builder, child);
    }
}

fn mark_node_no_fill(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(mut flags) = builder.node_flags(node) else {
        return;
    };
    flags.no_fill = true;
    let _ = builder.set_node_flags(node, flags);
}

/// The source-order fill pass runs before man blocks have their `Head` and
/// `Body` nodes.  A no-fill mode therefore temporarily reaches a raw `TP` or
/// `IP` request and its lexical arguments.  In the public tree mandoc keeps
/// that state only on body flow, never on the structural nodes or on a term
/// head.  Normalize these staging-only flags after the tree is assembled.
fn clear_no_fill_from_man_structure(builder: &mut DocumentBuilder) {
    let mut pending = vec![(DocumentBuilder::root(), false)];
    while let Some((node, inside_head)) = pending.pop() {
        let kind = builder.node_kind(node);
        let head_context = inside_head || kind == Some(NodeKind::Head);
        if (inside_head
            || matches!(
                kind,
                Some(NodeKind::Block | NodeKind::Head | NodeKind::Body)
            ))
            && let Some(mut flags) = builder.node_flags(node)
        {
            flags.no_fill = false;
            let _ = builder.set_node_flags(node, flags);
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().rev().map(|child| (*child, head_context)));
        }
    }
}

/// A terminal dot in a man section title is heading text, not a filled-flow
/// sentence boundary.  The source-order presentation pass runs before Heads
/// exist, so normalize this package distinction after block construction.
fn clear_sentence_end_from_section_heads(builder: &mut DocumentBuilder) {
    let mut pending = vec![DocumentBuilder::root()];
    while let Some(node) = pending.pop() {
        let section_head = builder.node_kind(node) == Some(NodeKind::Head)
            && matches!(builder.node_macro_name(node), Some("SH" | "SS"));
        if section_head {
            if let Some(children) = builder.children(node).map(<[NodeId]>::to_vec) {
                for child in children {
                    if let Some(mut flags) = builder.node_flags(child) {
                        // A heading supplied as an omitted `.SH`/`.SS`
                        // argument is the next physical text line. It keeps
                        // ordinary text sentence state, unlike an authored
                        // same-line heading argument.
                        if !flags.line_start {
                            flags.sentence_end = false;
                            let _ = builder.set_node_flags(child, flags);
                        }
                    }
                }
            }
            continue;
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().rev().copied());
        }
    }
}

fn mark_sentence_end(builder: &mut DocumentBuilder, node: NodeId) {
    let candidate = builder
        .node_text(node)
        .is_some()
        .then_some(node)
        .or_else(|| {
            builder
                .children(node)
                .and_then(|children| children.last())
                .copied()
        });
    let Some(candidate) = candidate else {
        return;
    };
    let Some(text) = builder.node_text(candidate) else {
        return;
    };
    let terminal = text.trim_end_matches(['"', '\'', ')', ']', '}']);
    if !terminal.ends_with(['.', '!', '?']) {
        return;
    }
    let Some(mut flags) = builder.node_flags(candidate) else {
        return;
    };
    // Literal/no-fill lines preserve their physical presentation verbatim;
    // mandoc does not promote a terminal dot in that mode into sentence
    // punctuation for later layout decisions.
    if flags.no_fill {
        return;
    }
    flags.sentence_end = true;
    let _ = builder.set_node_flags(candidate, flags);
}

/// `man_validate` discards a leading non-empty `.PP` after a section heading
/// and relinks its body into the section. This preserves the parser's visible
/// tree without needing to move arena records or expose a special recovery
/// wrapper to consumers.
fn flatten_leading_section_paragraphs(
    builder: &mut DocumentBuilder,
    sections: &[NodeId],
    blockers: &[NodeId],
) {
    for section in sections {
        // `man_validate` only inspects the section's original first child.
        // If that was an ignored `br`/`sp`, removing it does not cause the
        // following PP to be reconsidered and flattened.
        if blockers.contains(section) {
            continue;
        }
        let Some(children) = builder.children(*section).map(<[NodeId]>::to_vec) else {
            continue;
        };
        let Some(paragraph) = children.first().copied() else {
            continue;
        };
        if builder.node_macro_name(paragraph) != Some("PP") {
            continue;
        }
        let Some(parts) = builder.children(paragraph) else {
            continue;
        };
        let Some(paragraph_body) = parts.get(1).copied() else {
            continue;
        };
        let Some(body_children) = builder.children(paragraph_body) else {
            continue;
        };
        if body_children.is_empty() {
            continue;
        }
        let mut replacement = body_children.to_vec();
        replacement.extend_from_slice(&children[1..]);
        let _ = builder.replace_children(*section, &replacement);
    }
}

/// Apply the roff-level paragraph-request checks that operate within a
/// completed man body.  This deliberately runs before section validation:
/// libmandoc reports local `br`/`sp` and PP-body recoveries in source order,
/// then validates enclosing section bodies afterwards.
fn validate_inline_paragraph_controls(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return;
    };
    for child in &children {
        validate_inline_paragraph_controls(builder, *child, recoveries);
    }

    let parent_is_paragraph_body = builder.node_kind(parent) == Some(NodeKind::Body)
        && builder.node_macro_name(parent) == Some("PP");
    let mut retained = builder
        .children(parent)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let mut index = 0;
    while index < retained.len() {
        let node = retained[index];
        // An empty completed tbl range lowers to the legacy closing-spacing
        // node. It is public `sp` syntax, but it is not an authored paragraph
        // control and must not receive a second man(7) recovery.
        if builder.node_preprocessor_opener(node) == Some("TS") {
            index += 1;
            continue;
        }
        let macro_name = builder.node_macro_name(node);
        let previous = index
            .checked_sub(1)
            .and_then(|previous| retained.get(previous).copied())
            .and_then(|previous| builder.node_macro_name(previous));
        // `node_macro_name` borrows the arena.  Retain only the three
        // paragraph-control names this validator recognises so recovery data
        // remains independent of that borrow (and intentionally static).
        let previous_control = match previous {
            Some("br") => Some("br"),
            Some("sp") => Some("sp"),
            Some("PP") => Some("PP"),
            _ => None,
        };
        match macro_name {
            Some("br") => {
                // `check_par()` treats only the first request inside a PP
                // body as following the paragraph macro.  Text before a
                // later `.br` is ordinary flow, not a PP-control sibling.
                let context =
                    previous_control.or((index == 0 && parent_is_paragraph_body).then_some("PP"));
                if let Some(context @ ("br" | "sp" | "PP")) = context {
                    recoveries.push(Recovery::ParagraphSkip {
                        macro_name: "br",
                        relation: "after",
                        context,
                        location: builder.node_location(node),
                    });
                    retained.remove(index);
                    continue;
                }
            }
            Some("sp") => {
                if previous_control == Some("br") {
                    let previous = retained[index - 1];
                    recoveries.push(Recovery::ParagraphSkip {
                        macro_name: "br",
                        relation: "before",
                        context: "sp",
                        location: builder.node_location(previous),
                    });
                    retained.remove(index - 1);
                    index = index.saturating_sub(1);
                    // A bare conditional opener contributes a generated
                    // `sp`.  Mandoc discards the preceding `br` during this
                    // recovery, but retains the resulting vertical space.
                    continue;
                }
                if index == 0 && parent_is_paragraph_body {
                    recoveries.push(Recovery::ParagraphSkip {
                        macro_name: "sp",
                        relation: "after",
                        context: "PP",
                        location: builder.node_location(node),
                    });
                    retained.remove(index);
                    continue;
                }
            }
            _ => {}
        }
        index += 1;
    }
    let _ = builder.replace_children(parent, &retained);
}

/// Validate a section body after its nested bodies.  Upstream keeps this
/// separate from the local roff validation above, which is why an initial
/// `sp` is diagnosed after all ordinary sibling-level paragraph recoveries.
fn validate_section_paragraph_controls(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    recoveries: &mut Vec<Recovery>,
    deferred: &mut Vec<(NodeId, Recovery)>,
    flatten_blockers: &mut Vec<NodeId>,
) {
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return;
    };
    for child in children {
        validate_section_paragraph_controls(builder, child, recoveries, deferred, flatten_blockers);
    }
    let Some(section_name) = (match builder.node_macro_name(parent) {
        Some("SH") => Some("SH"),
        Some("SS") => Some("SS"),
        _ => None,
    }) else {
        return;
    };
    if builder.node_kind(parent) != Some(NodeKind::Body) {
        return;
    }
    let mut remaining = Vec::with_capacity(deferred.len());
    for (owner, recovery) in std::mem::take(deferred) {
        if owner == parent {
            if matches!(
                &recovery,
                Recovery::ParagraphAfterSection {
                    macro_name: "br" | "sp",
                    ..
                }
            ) && !flatten_blockers.contains(&parent)
            {
                flatten_blockers.push(parent);
            }
            recoveries.push(recovery);
        } else {
            remaining.push((owner, recovery));
        }
    }
    *deferred = remaining;
    let Some(section_children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some(first) = section_children.first().copied() else {
        return;
    };
    if builder.node_preprocessor_opener(first) == Some("TS") {
        return;
    }
    if let Some(macro_name) = match builder.node_macro_name(first) {
        Some("br") => Some("br"),
        Some("sp") => Some("sp"),
        _ => None,
    } {
        recoveries.push(Recovery::ParagraphSkip {
            macro_name,
            relation: "after",
            context: section_name,
            location: builder.node_location(first),
        });
        if !flatten_blockers.contains(&parent) {
            flatten_blockers.push(parent);
        }
        let _ = builder.replace_children(parent, &section_children[1..]);
        return;
    }
    if let Some(last) = section_children.last().copied()
        && builder.node_macro_name(last) == Some("br")
    {
        // `man_validate` discards a terminal line break after recording this
        // post-validation recovery.
        recoveries.push(Recovery::ParagraphAtSectionEnd {
            macro_name: "br",
            section_name,
            location: builder.node_location(last),
        });
        let _ = builder.replace_children(parent, &section_children[..section_children.len() - 1]);
    }
}

fn paragraph_recovery_offset(recovery: &Recovery) -> u32 {
    match recovery {
        Recovery::EmptyParagraph { location, .. }
        | Recovery::ParagraphSkip { location, .. }
        | Recovery::ParagraphAtSectionEnd { location, .. } => location
            .as_ref()
            .map_or(u32::MAX, |location| location.start),
        _ => u32::MAX,
    }
}
