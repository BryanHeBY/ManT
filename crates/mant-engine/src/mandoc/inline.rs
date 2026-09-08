//! Lowers typed roff events and semantic mdoc macros into inline IR nodes.

use libmandoc_rs::{Node, NodeKind};
use mant_ir::Inline;

pub(crate) use crate::inline::{plain_text, terms_fit_inline};

mod flow;
mod font;
mod scopes;
pub(super) use flow::{FilledBoundary, FontState, InlineBuilder};
mod source;
mod source_fragment;

#[cfg(test)]
use font::parse_roff_text_with_font;
pub(super) use font::{lower_inline_nodes_with_font_state, parse_roff_text};
use font::{lower_man_font_scope, parse_roff_text_with_state};

pub(super) use source::roff_macro_arguments;
pub(super) use source_fragment::lower_source_fragment;

use super::{
    first_part_children,
    roff_escape::{RoffFont as Font, RoffInlineEvent, decode, visible_text},
};

pub(super) fn lower_inline_nodes(nodes: &[Node], default_name: Option<&str>) -> Vec<Inline> {
    lower_inline_nodes_with_spacing(nodes, default_name, true)
}

pub(super) fn lower_inline_nodes_with_spacing(
    nodes: &[Node],
    default_name: Option<&str>,
    spacing_enabled: bool,
) -> Vec<Inline> {
    let mut builder = InlineBuilder::with_spacing(spacing_enabled);
    for (index, node) in nodes.iter().enumerate() {
        if node.macro_name.as_deref() == Some("Sm") {
            let setting = plain_text(&lower_inline_nodes(&node.children, default_name));
            builder.set_spacing(setting.trim());
            continue;
        }
        // mandoc joins the final pair in a contiguous mdoc bibliography
        // author run with "and". The conjunction is formatter-generated, so
        // it is not a child of either `%A` node and must be restored while the
        // sibling context is still available.
        if node.macro_name.as_deref() == Some("%A")
            && index > 0
            && nodes[index - 1].macro_name.as_deref() == Some("%A")
            && nodes
                .get(index + 1)
                .is_none_or(|next| next.macro_name.as_deref() != Some("%A"))
        {
            builder.append(text_node("and"));
        }
        let spacing_before = builder.spacing_enabled();
        append_inline_node_with_next(&mut builder, node, nodes.get(index + 1), default_name);
        let spacing_after = spacing_after_node(node, spacing_before, default_name);
        builder.inherit_spacing(spacing_after);
    }
    builder.finish()
}

/// Apply one validated mdoc `Sm` state transition.
///
/// The same state machine is used for top-level filled flow and for nested
/// definition terms. Keeping it here prevents an `Sm` inside `Xo` from being
/// discarded merely because that subtree is lowered by an inline builder.
pub(super) fn updated_spacing(current: bool, setting: &str) -> bool {
    match setting {
        "on" => true,
        "off" => false,
        "" => !current,
        _ => current,
    }
}

pub(super) fn spacing_after_nodes(
    nodes: &[Node],
    mut spacing_enabled: bool,
    default_name: Option<&str>,
) -> bool {
    for node in nodes {
        spacing_enabled = spacing_after_node(node, spacing_enabled, default_name);
    }
    spacing_enabled
}

pub(super) fn spacing_after_node(
    node: &Node,
    spacing_enabled: bool,
    default_name: Option<&str>,
) -> bool {
    if node.macro_name.as_deref() == Some("Sm") {
        let setting = plain_text(&lower_inline_nodes(&node.children, default_name));
        return updated_spacing(spacing_enabled, setting.trim());
    }
    spacing_after_nodes(&node.children, spacing_enabled, default_name)
}

/// Lower one syntax node into an existing inline flow.
///
/// libmandoc classifies bare opening and closing delimiters during parsing.
/// Preserve those roles instead of re-inferring punctuation from visible
/// characters: literal displays can intentionally put spaces around the same
/// glyphs that ordinary prose uses as attached punctuation.
pub(super) fn append_inline_node(
    builder: &mut InlineBuilder,
    node: &Node,
    default_name: Option<&str>,
) {
    append_inline_node_with_next(builder, node, None, default_name);
}

pub(super) fn append_inline_node_with_next(
    builder: &mut InlineBuilder,
    node: &Node,
    next: Option<&Node>,
    default_name: Option<&str>,
) {
    if node.flags.no_print || node.kind == NodeKind::Comment {
        // Tg can recover an authored target even when native output is hidden.
        if node.macro_name.as_deref() == Some("Tg") {
            scopes::append(builder, node, default_name);
        }
        return;
    }
    if node.kind == NodeKind::Text && !node.flags.no_print {
        if node.flags.delimiter_close {
            builder.tighten_next_boundary();
        }
        let inlines = parse_roff_text_with_state(
            node.text.as_deref().unwrap_or_default(),
            &mut builder.font,
            !node.flags.no_fill,
        );
        builder.append(inlines);
        if node.flags.delimiter_open || node.flags.line_continuation {
            builder.tighten_next_boundary();
        }
        return;
    }
    if node.flags.delimiter_close {
        builder.tighten_next_boundary();
    }
    match node.macro_name.as_deref() {
        Some("B" | "I" | "SB" | "R" | "BI" | "BR" | "IB" | "IR" | "RB" | "RI" | "OP") => {
            let inlines = lower_man_font_scope(
                node,
                default_name,
                builder.spacing_enabled(),
                &mut builder.font,
            );
            builder.append(inlines);
        }
        Some("Ns") => {
            if !node.flags.line_start {
                builder.tighten_next_boundary();
            }
        }
        // `Pf` owns visible prefix text and suppresses only the boundary to
        // the following sibling. Treating it like the empty `Ns` request
        // silently discarded constructs such as `.Pf [\-]ddd Cm \&.`.
        Some("Pf") => {
            scopes::append(builder, node, default_name);
            if next.is_some_and(|next| !next.flags.line_start) {
                builder.tighten_next_boundary();
            }
        }
        // A roff break ends the current output line, not the paragraph.
        // `Pp` can also occur inside an extended mdoc definition head, where
        // it separates alternative terms without ending the owning item.
        // Keeping both inline lets the definition lowering retain that
        // distinction instead of concatenating the alternatives.
        Some("br") => builder.hard_break(),
        Some("Pp") => {
            // In no-fill displays libmandoc can move the automatic target of
            // a later semantic macro onto this paragraph break. The regular
            // block lowering path conserves structural Pp targets, but this
            // inline path must do so before replacing the node with a break.
            if let Some(target) = super::targets::raw_target(node) {
                builder.append(vec![Inline::anchor_at(target, super::source_span(node))]);
            }
            builder.hard_break();
        }
        // Formatting requests carry control arguments such as `CW` and `R`.
        // `Es` likewise only changes the delimiters later `En` nodes use;
        // libmandoc resolves those delimiters onto each invocation. These
        // requests change formatter state and are never document text.
        // Verbatim regions already retain their semantics through
        // libmandoc's no-fill flag, so leaking these arguments would only
        // create phantom paragraphs around preformatted blocks.
        Some("ft") => {
            let name = node
                .children
                .first()
                .and_then(|node| node.text.as_deref())
                .unwrap_or("P");
            if name == "P" {
                builder.font.restore();
            } else {
                builder.font.select(super::roff_escape::font(name));
            }
        }
        Some("SM") => {
            append_inline_nodes(builder, &node.children, default_name);
        }
        Some("Sm") => {
            let setting = plain_text(&lower_inline_nodes(&node.children, default_name));
            builder.set_spacing(setting.trim());
        }
        Some("Es" | "PD" | "ad" | "fi" | "hy" | "in" | "na" | "ne" | "nf" | "nh" | "nr" | "ta") => {
        }
        Some("Ap") => {
            builder.tighten_next_boundary();
            builder.append(vec![Inline::Text { value: "'".into() }]);
            builder.tighten_next_boundary();
        }
        _ => scopes::append(builder, node, default_name),
    }
    if node.flags.delimiter_open || node.flags.line_continuation {
        builder.tighten_next_boundary();
    }
}

/// Append sibling events without throwing away pending formatter effects.
fn append_inline_nodes(builder: &mut InlineBuilder, nodes: &[Node], default_name: Option<&str>) {
    for (index, node) in nodes.iter().enumerate() {
        append_inline_node_with_next(builder, node, nodes.get(index + 1), default_name);
    }
}

/// Materialize an independent value only where a caller needs a complete
/// label/operand. Normal sibling and wrapper flow uses the shared builder.
fn lower_inline_node(
    node: &Node,
    default_name: Option<&str>,
    spacing_enabled: bool,
) -> Vec<Inline> {
    let mut builder = InlineBuilder::with_spacing(spacing_enabled);
    append_inline_node(&mut builder, node, default_name);
    builder.finish()
}

/// Generated references/declarations consume their inner boundaries as part
/// of their own punctuation, rather than exporting an AST-tail approximation.
fn lower_atomic_node(
    node: &Node,
    default_name: Option<&str>,
    spacing_enabled: bool,
) -> Vec<Inline> {
    if node.kind == NodeKind::Equation {
        return node
            .equation
            .as_deref()
            .map(visible_text)
            .filter(|value| !value.trim().is_empty())
            .map(|value| vec![Inline::Code { value }])
            .unwrap_or_default();
    }
    let children = inline_children(node);
    let mut output = match node.macro_name.as_deref() {
        Some("In") => {
            let lowered = lower_inline_nodes_with_spacing(children, default_name, spacing_enabled);
            if lowered.is_empty() {
                Vec::new()
            } else {
                vec![Inline::Code {
                    value: format!(
                        "{}<{}>",
                        if node.flags.synopsis_pretty && node.flags.line_start {
                            "#include "
                        } else {
                            ""
                        },
                        plain_text(&lowered)
                    ),
                }]
            }
        }
        Some("Xr" | "MR") => lower_manual_reference(children, default_name, spacing_enabled),
        Some("Lk") => lower_link(children, default_name, spacing_enabled),
        Some("Mt") => lower_mail_addresses(children, default_name, spacing_enabled),
        Some("Bx") => lower_bsd_reference(
            node,
            lower_inline_nodes_with_spacing(children, default_name, spacing_enabled),
        ),
        Some("Fn") => lower_function_element(node, default_name, spacing_enabled),
        Some("Fo") => lower_function_declaration(node, default_name, spacing_enabled),
        _ => unreachable!("only generated references/declarations are atomic"),
    };
    if let Some(anchor) = navigation_anchor(node) {
        output.insert(0, anchor);
    }
    output
}

fn lower_function_element(
    node: &Node,
    default_name: Option<&str>,
    spacing_enabled: bool,
) -> Vec<Inline> {
    let Some((name, arguments)) = inline_children(node).split_first() else {
        return Vec::new();
    };
    let mut declaration = wrap_strong(lower_inline_node(name, default_name, spacing_enabled));
    declaration.push(Inline::Text { value: "(".into() });
    for (index, argument) in arguments.iter().enumerate() {
        if index > 0 {
            declaration.push(Inline::Text { value: ", ".into() });
        }
        declaration.extend(wrap_emphasis(lower_inline_node(
            argument,
            default_name,
            spacing_enabled,
        )));
    }
    declaration.push(Inline::Text {
        value: function_closing(node.flags.synopsis_pretty).into(),
    });
    declaration
}

fn lower_function_declaration(
    node: &Node,
    default_name: Option<&str>,
    spacing_enabled: bool,
) -> Vec<Inline> {
    let head = lower_inline_nodes_with_spacing(
        first_part_children(node, NodeKind::Head),
        default_name,
        spacing_enabled,
    );
    let body = first_part_children(node, NodeKind::Body);
    if head.is_empty() {
        return lower_inline_nodes_with_spacing(body, default_name, spacing_enabled);
    }

    // `Fo` stores an explicit `.Tg` on its head wrapper, while the visible
    // function declaration is lowered from the complete block.
    let target = super::targets::part_target_with_source(node, NodeKind::Head);
    let mut declaration = target
        .into_iter()
        .map(super::targets::OwnedTarget::into_inline)
        .collect::<Vec<_>>();
    declaration.push(Inline::Strong { children: head });
    declaration.push(Inline::Text { value: "(".into() });
    let mut has_argument = false;
    let mut arguments = InlineBuilder::with_spacing(spacing_enabled);
    for (index, argument) in body.iter().enumerate() {
        if argument.macro_name.as_deref() == Some("Fa") && !argument.flags.no_print {
            if let Some(anchor) = navigation_anchor(argument) {
                arguments.append(vec![anchor]);
            }
            // Fo owns one parameter per Fa operand; quoting, not guessing
            // C syntax or whitespace, determines a multi-word operand.
            for operand in inline_children(argument) {
                if operand.flags.delimiter_close {
                    append_inline_node(&mut arguments, operand, default_name);
                    continue;
                }
                if has_argument {
                    arguments.tighten_next_boundary();
                    arguments.append(text_node(", "));
                }
                arguments.append(wrap_emphasis(lower_inline_node(
                    operand,
                    default_name,
                    arguments.spacing_enabled(),
                )));
                has_argument = true;
            }
        } else {
            // Controls and zero-width targets keep their ordinary inline
            // effects and source position, but never consume a parameter.
            append_inline_node_with_next(
                &mut arguments,
                argument,
                body.get(index + 1),
                default_name,
            );
        }
    }
    declaration.extend(arguments.finish());
    let synopsis_pretty = node.flags.synopsis_pretty
        || node
            .children
            .iter()
            .any(|child| child.kind == NodeKind::Body && child.flags.synopsis_pretty);
    declaration.push(Inline::Text {
        value: function_closing(synopsis_pretty).into(),
    });
    declaration
}

const fn function_closing(synopsis_pretty: bool) -> &'static str {
    if synopsis_pretty { ");" } else { ")" }
}

/// Whether a semantic macro owns an inline enclosure body.
///
/// Both implicit forms such as `Aq` and explicit block forms such as
/// `Ao`/`Ac` arrive as one opener-owned subtree after libmandoc validation.
/// `Eo` carries its delimiters in structural head and tail nodes, while the
/// obsolete `En` carries the state resolved from the preceding `Es` request.
pub(super) fn is_enclosure_macro(macro_name: Option<&str>) -> bool {
    macro_name.is_some_and(|name| enclosure_marks(name).is_some())
        || matches!(macro_name, Some("Eo" | "En"))
}

fn enclosure_marks(name: &str) -> Option<(&'static str, &'static str)> {
    match name {
        "Op" | "Oo" | "Bq" | "Bo" => Some(("[", "]")),
        "Dq" | "Do" => Some(("“", "”")),
        "Qq" | "Qo" => Some(("\"", "\"")),
        "Sq" | "So" | "Ql" => Some(("‘", "’")),
        "Pq" | "Po" => Some(("(", ")")),
        "Brq" | "Bro" => Some(("{", "}")),
        "Aq" | "Ao" => Some(("<", ">")),
        _ => None,
    }
}

/// Convert libmandoc's validated deep-link marker into a zero-width IR node.
/// Explicit `.Tg` requests retain their authored argument; automatically
/// discovered tags fall back to libmandoc's first printable source token.
fn navigation_anchor(node: &Node) -> Option<Inline> {
    super::targets::raw_target(node).map(|id| Inline::anchor_at(id, super::source_span(node)))
}

fn inline_children(node: &Node) -> &[Node] {
    // A present empty body still owns the content. Falling back to Head and
    // Body wrappers would execute their enclosing macro a second time.
    node.children
        .iter()
        .find(|child| child.kind == NodeKind::Body)
        .map_or(&node.children, |body| &body.children)
}

fn lower_manual_reference(
    children: &[Node],
    default_name: Option<&str>,
    spacing_enabled: bool,
) -> Vec<Inline> {
    let Some(name_node) = children.first() else {
        return Vec::new();
    };
    let name = plain_text(&lower_inline_node(name_node, default_name, spacing_enabled));
    if name.is_empty() {
        return Vec::new();
    }
    let section = children
        .get(1)
        .map(|child| plain_text(&lower_inline_node(child, default_name, spacing_enabled)))
        .filter(|value| !value.is_empty());
    let display = section
        .as_ref()
        .map_or_else(|| name.clone(), |section| format!("{name}({section})"));
    let mut output = vec![Inline::Link {
        target: mant_ir::LinkTarget::Manual {
            name,
            manual_section: section,
        },
        title: None,
        children: text_node(&display),
    }];
    for child in children.iter().skip(2) {
        output.extend(lower_inline_node(child, default_name, spacing_enabled));
    }
    output
}

fn lower_link(children: &[Node], default_name: Option<&str>, spacing_enabled: bool) -> Vec<Inline> {
    let Some(first) = children.first() else {
        return Vec::new();
    };
    let address = plain_text(&lower_inline_node(first, default_name, spacing_enabled));
    if address.is_empty() {
        return Vec::new();
    }
    let label = lower_inline_nodes_with_spacing(&children[1..], default_name, spacing_enabled);
    lower_external_link(address, label, false)
}

/// Unlike Lk, Mt owns a sequence of addresses, not an address and a label.
fn lower_mail_addresses(
    children: &[Node],
    default_name: Option<&str>,
    spacing_enabled: bool,
) -> Vec<Inline> {
    let mut builder = InlineBuilder::with_spacing(spacing_enabled);
    for child in children {
        if child.flags.delimiter_close || child.flags.delimiter_open {
            append_inline_node(&mut builder, child, default_name);
            continue;
        }
        let address = plain_text(&lower_inline_node(child, default_name, spacing_enabled));
        if !address.is_empty() {
            builder.append(lower_external_link(address, Vec::new(), true));
        }
    }
    builder.finish()
}

/// Build an mdoc external link without allowing punctuation to hide its target.
///
/// `Lk` accepts ordinary trailing sentence punctuation as an argument.
/// It is not a descriptive label: a source spelling such as `.Lk URL .` must
/// render `URL.` rather than an otherwise invisible link whose only child is
/// `.`. The same policy is shared with the source fallback below.
fn lower_external_link(address: String, label: Vec<Inline>, email: bool) -> Vec<Inline> {
    let punctuation_only = is_source_closing_punctuation(&plain_text(&label));
    if punctuation_only {
        let children = text_node(&address);
        let target = external_link_target(address, email);
        let mut output = vec![Inline::Link {
            target,
            title: None,
            children,
        }];
        output.extend(label);
        return output;
    }
    let children = if label.is_empty() {
        text_node(&address)
    } else {
        label
    };
    vec![Inline::Link {
        target: external_link_target(address, email),
        title: None,
        children,
    }]
}

fn is_source_closing_punctuation(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            matches!(
                character,
                '.' | ',' | ':' | ';' | '!' | '?' | ')' | ']' | '}'
            )
        })
}

/// Lower the portable semantic forms of mdoc `Bx` from its authored arguments.
///
///
/// libmandoc appends a generated `BSD` node with `Ns` and intentionally leaves
/// lifecycle arguments as compact `-develBSD` text. The mdoc contract instead
/// gives the lifecycle forms descriptive meanings, while an ordinary version
/// and optional release render as `versionBSD release`. The raw AST flags make
/// this distinction explicit without reparsing source text or depending on a
/// particular formatter's generated nodes.
fn lower_bsd_reference(node: &Node, fallback: Vec<Inline>) -> Vec<Inline> {
    let mut authored = node
        .children
        .iter()
        .filter(|child| {
            child.kind == NodeKind::Text && !child.flags.generated && !child.flags.no_print
        })
        .filter_map(|child| child.text.as_deref())
        .map(visible_text)
        .filter(|value| !value.is_empty());
    let Some(first) = authored.next() else {
        return text_node("BSD");
    };
    let second = authored.next();
    if authored.next().is_some() {
        return fallback;
    }
    if second.is_none() {
        let lifecycle = match first.as_str() {
            "-alpha" => Some("BSD (currently in alpha test)"),
            "-beta" => Some("BSD (currently in beta test)"),
            "-devel" => Some("BSD (currently under development)"),
            _ => None,
        };
        if let Some(lifecycle) = lifecycle {
            return text_node(lifecycle);
        }
    }
    let mut value = format!("{first}BSD");
    if let Some(second) = second {
        value.push(' ');
        value.push_str(&second);
    }
    text_node(&value)
}

fn external_link_target(address: String, email: bool) -> mant_ir::LinkTarget {
    if email {
        mant_ir::LinkTarget::Email { address }
    } else {
        mant_ir::LinkTarget::External { uri: address }
    }
}

/// Lower GNU man-ext `.UR` and `.MT` blocks as one inline phrase.
///
/// The macros are structural in libmandoc's tree because their label occupies
/// a body, but they do not start a paragraph in man(7). A descriptive label
/// keeps the target visible after the link so text search and citation views
/// retain both pieces of source information.
pub(super) fn lower_man_link(
    node: &Node,
    default_name: Option<&str>,
    spacing_enabled: bool,
) -> Vec<Inline> {
    let target = plain_text(&lower_inline_nodes_with_spacing(
        first_part_children(node, NodeKind::Head),
        default_name,
        spacing_enabled,
    ));
    if target.is_empty() {
        return lower_inline_nodes_with_spacing(
            first_part_children(node, NodeKind::Body),
            default_name,
            spacing_enabled,
        );
    }

    let label = lower_inline_nodes_with_spacing(
        first_part_children(node, NodeKind::Body),
        default_name,
        spacing_enabled,
    );
    let has_label = !label.is_empty();
    let children = if has_label { label } else { text_node(&target) };
    let link_target = if node.macro_name.as_deref() == Some("MT") {
        mant_ir::LinkTarget::Email {
            address: target.clone(),
        }
    } else {
        mant_ir::LinkTarget::External {
            uri: target.clone(),
        }
    };
    let mut output = vec![Inline::Link {
        target: link_target,
        title: None,
        children,
    }];
    if has_label {
        output.push(Inline::Text {
            value: format!(" ⟨{target}⟩"),
        });
    }
    output.extend(lower_inline_nodes_with_spacing(
        first_part_children(node, NodeKind::Tail),
        default_name,
        spacing_enabled,
    ));
    output
}

fn wrap_strong(children: Vec<Inline>) -> Vec<Inline> {
    (!children.is_empty())
        .then_some(Inline::Strong { children })
        .into_iter()
        .collect()
}

fn wrap_emphasis(children: Vec<Inline>) -> Vec<Inline> {
    (!children.is_empty())
        .then_some(Inline::Emphasis { children })
        .into_iter()
        .collect()
}

fn alternating_font_pair(macro_name: Option<&str>) -> Option<(Font, Font)> {
    match macro_name {
        Some("BI") => Some((Font::Strong, Font::Emphasis)),
        Some("BR") => Some((Font::Strong, Font::Regular)),
        Some("IB") => Some((Font::Emphasis, Font::Strong)),
        Some("IR") => Some((Font::Emphasis, Font::Regular)),
        Some("RB") => Some((Font::Regular, Font::Strong)),
        Some("RI") => Some((Font::Regular, Font::Emphasis)),
        _ => None,
    }
}

fn surround(open: &str, mut children: Vec<Inline>, close: &str) -> Vec<Inline> {
    let mut result = text_node(open);
    result.append(&mut children);
    result.extend(text_node(close));
    result
}

fn text_node(value: &str) -> Vec<Inline> {
    vec![Inline::Text {
        value: value.to_owned(),
    }]
}

fn needs_boundary_space(left: Option<char>, right: Option<char>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if !left.is_whitespace() && !right.is_whitespace())
}

fn push_text(nodes: &mut Vec<Inline>, value: String) {
    if let Some(Inline::Text { value: previous }) = nodes.last_mut() {
        previous.push_str(&value);
    } else {
        nodes.push(Inline::Text { value });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn styled_scopes_preserve_pending_spacing_and_continuation() {
        for tight in [false, true] {
            let mut builder = super::InlineBuilder::new();
            builder.append(super::text_node("FIRST"));
            if tight {
                builder.tighten_next_boundary();
            }
            builder.append_scope(|builder| builder.set_spacing("off"), |nodes| nodes);
            builder.append_scope(
                |builder| builder.append(super::text_node("SECOND")),
                |children| vec![super::Inline::Emphasis { children }],
            );
            builder.append(super::text_node("THIRD"));
            assert_eq!(
                super::plain_text(&builder.finish()),
                if tight {
                    "FIRSTSECONDTHIRD"
                } else {
                    "FIRST SECONDTHIRD"
                }
            );
        }
    }

    use super::{
        FilledBoundary, Font, InlineBuilder, parse_roff_text, parse_roff_text_with_font, plain_text,
    };
    use mant_ir::Inline;

    #[test]
    fn inline_builder_tracks_nested_visible_boundaries_incrementally() {
        let mut builder = InlineBuilder::new();
        builder.append(vec![Inline::anchor("start")]);
        builder.append(vec![Inline::Strong {
            children: vec![Inline::Text {
                value: "first".to_owned(),
            }],
        }]);
        builder.append_filled(
            vec![Inline::Emphasis {
                children: vec![Inline::Text {
                    value: "second".to_owned(),
                }],
            }],
            FilledBoundary::Word,
        );
        builder.hard_break();
        builder.hard_break();
        builder.append(vec![Inline::Code {
            value: "third".to_owned(),
        }]);

        assert_eq!(plain_text(&builder.finish()), "first second\nthird");
    }

    #[test]
    fn decodes_fonts_hyphens_and_renderer_links() {
        let nodes =
            parse_roff_text("\\X'tty: link https://example.test'\\fB\\-h\\fR\\X'tty: link' FILE");

        assert_eq!(plain_text(&nodes), "-h FILE");
        assert!(matches!(
            nodes[0],
            Inline::Link {
                target: mant_ir::LinkTarget::External { .. },
                ..
            }
        ));
    }

    #[test]
    fn removes_roff_layout_escapes_without_hiding_literal_punctuation() {
        let source = r"[\|optional\|]\&.\|.\|. \||\|";

        assert_eq!(plain_text(&parse_roff_text(source)), "[optional]... |");
    }

    #[test]
    fn consumes_groff_colour_and_size_state_around_visible_text() {
        let source = r"The \m[blue]\fBGit User\(cqs Manual\fR\m[]\&\s-2\u[1]\d\s+2 has more detail";
        let nodes = parse_roff_text(source);

        assert_eq!(
            plain_text(&nodes),
            "The Git User's Manual[1] has more detail"
        );
        assert!(
            nodes
                .iter()
                .any(|node| matches!(node, Inline::Strong { .. }))
        );
    }

    #[test]
    fn preserves_pandoc_verbatim_font_styles() {
        let nodes = parse_roff_text(r"\f[V]code\f[R] \f[VB]bold\f[R] \f[VI]italic\f[R]");

        assert_eq!(plain_text(&nodes), "code bold italic");
        assert!(matches!(nodes.first(), Some(Inline::Code { value }) if value == "code"));
        assert!(nodes.iter().any(|node| matches!(
            node,
            Inline::Strong { children }
                if matches!(children.as_slice(), [Inline::Code { value }] if value == "bold")
        )));
        assert!(nodes.iter().any(|node| matches!(
            node,
            Inline::Emphasis { children }
                if matches!(children.as_slice(), [Inline::Code { value }] if value == "italic")
        )));
    }

    #[test]
    fn removes_redundant_escaped_font_requests_only_inside_the_same_font() {
        let generated = parse_roff_text(r"\fB\\fBpackage.json\\fR config\fR");
        assert_eq!(plain_text(&generated), "package.json config");
        assert!(matches!(generated.as_slice(), [Inline::Strong { .. }]));

        let emphasis = parse_roff_text(r"\fI\\fIvalue\\fR\fR");
        assert_eq!(plain_text(&emphasis), "value");
        assert!(matches!(emphasis.as_slice(), [Inline::Emphasis { .. }]));

        let code = parse_roff_text(r"\fC\\fCvalue\\fR\fR");
        assert_eq!(plain_text(&code), "value");
        assert!(matches!(code.as_slice(), [Inline::Code { .. }]));

        let literal = parse_roff_text(r"show \\fBbold\\fR markup");
        assert_eq!(plain_text(&literal), r"show \fBbold\fR markup");
    }

    #[test]
    fn promotes_only_evidenced_sphinx_manual_references() {
        let nodes = parse_roff_text(r"See btrfs\-subvolume(8) \%<> and btrfs(5) \%<> for details.");

        assert_eq!(
            plain_text(&nodes),
            "See btrfs-subvolume(8) and btrfs(5) for details."
        );
        let references = nodes
            .iter()
            .filter_map(|inline| match inline {
                Inline::Link {
                    target:
                        mant_ir::LinkTarget::Manual {
                            name,
                            manual_section: Some(manual_section),
                        },
                    ..
                } => Some((name.as_str(), manual_section.as_str())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(references, [("btrfs-subvolume", "8"), ("btrfs", "5")]);
    }

    #[test]
    fn preserves_empty_destinations_without_a_safe_reference() {
        for source in [
            r"literal \%<>",
            r"group(qgroup) \%<>",
            r"function(0) \%<>",
            r"/tmp/tool(1) \%<>",
            r"user@tool(1) \%<>",
            r"tool(1)\%<>",
        ] {
            assert!(
                plain_text(&parse_roff_text(source)).contains("<>"),
                "empty destination disappeared from {source:?}"
            );
        }
    }

    #[test]
    fn preserves_sphinx_shape_in_no_fill_and_code_content() {
        let no_fill = parse_roff_text_with_font(r"btrfs-subvolume(8) \%<>", Font::Regular, false);
        let code = parse_roff_text_with_font(r"btrfs-subvolume(8) \%<>", Font::Code, true);

        assert_eq!(plain_text(&no_fill), "btrfs-subvolume(8) <>");
        assert_eq!(plain_text(&code), "btrfs-subvolume(8) <>");
        assert!(!no_fill.iter().any(|inline| matches!(
            inline,
            Inline::Link {
                target: mant_ir::LinkTarget::Manual { .. },
                ..
            }
        )));
        assert!(!code.iter().any(|inline| matches!(
            inline,
            Inline::Link {
                target: mant_ir::LinkTarget::Manual { .. },
                ..
            }
        )));
    }
}
