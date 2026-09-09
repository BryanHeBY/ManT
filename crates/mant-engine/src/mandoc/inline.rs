//! Lowers typed roff events and semantic mdoc macros into inline IR nodes.

use libmandoc_rs::{Node, NodeKind};
use mant_ir::Inline;

pub(crate) use crate::inline::{plain_text, terms_fit_inline};

mod flow;
mod font;
mod generated;
mod links;
pub(super) use links::lower_man_link;
use links::{lower_bsd_reference, lower_link, lower_mail_addresses};
mod scopes;
mod source_cursor;
pub(super) use flow::{FilledBoundary, FontState, InlineBuilder};
mod source;
mod source_fragment;

use font::lower_man_font_scope;
#[cfg(test)]
use font::parse_roff_text_with_font;
pub(super) use font::parse_roff_text_with_state;
pub(super) use font::{lower_inline_nodes_with_font_state, parse_roff_text};

pub(super) use source::roff_macro_arguments;
pub(super) use source_fragment::lower_source_fragment_with_formatter_state;

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
    append_inline_nodes(&mut builder, nodes, default_name);
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
        append_text_node(builder, node);
        return;
    }
    builder.begin_executed_node(node);
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
            builder.append_text("'");
            builder.tighten_next_boundary();
        }
        _ => scopes::append(builder, node, default_name),
    }
    // A bare Fl followed by a callable macro on the same source line owns an
    // external join. An explicit empty operand is different: it consumes the
    // prefix's operand position and must leave that sibling separate.
    if node.macro_name.as_deref() == Some("Fl")
        && node.children.is_empty()
        && next.is_some_and(|next| next.kind != NodeKind::Text && !next.flags.line_start)
    {
        builder.tighten_next_boundary();
    }
    if node.flags.delimiter_open || node.flags.line_continuation {
        builder.tighten_next_boundary();
    }
}

fn append_text_node(builder: &mut InlineBuilder, node: &Node) {
    builder.begin_executed_node(node);
    if node.flags.delimiter_close {
        builder.tighten_next_boundary();
    }
    let inlines = parse_roff_text_with_state(
        node.text.as_deref().unwrap_or_default(),
        &mut builder.font,
        !node.flags.no_fill,
    );
    // mdoc_term gives an empty text node a vertical row only when the text
    // itself begins an input line. An empty No/Em argument does not, whereas
    // a buffered zero-width glyph (for example \&) still occupies that row.
    let occupies_literal_row = !inlines.is_empty()
        || (node.flags.line_start && node.text.as_deref().is_some_and(str::is_empty))
        || decode(node.text.as_deref().unwrap_or_default())
            .iter()
            .any(|event| matches!(event, RoffInlineEvent::ZeroWidthGlyph));
    builder.append_word_with_literal_row(inlines, occupies_literal_row);
    if node.flags.delimiter_open || node.flags.line_continuation {
        builder.tighten_next_boundary();
    }
    builder.continue_source_line(super::blocks::ends_with_line_continuation(node));
}

/// Append sibling events without throwing away pending formatter effects.
pub(super) fn append_inline_nodes(
    builder: &mut InlineBuilder,
    nodes: &[Node],
    default_name: Option<&str>,
) {
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
            builder.append_text("and");
        }
        append_inline_node_with_next(builder, node, nodes.get(index + 1), default_name);
    }
}

/// Materialize an independent value only where a caller needs a complete
/// label/operand. Normal sibling and wrapper flow uses the shared builder.
fn lower_inline_node(
    node: &Node,
    default_name: Option<&str>,
    spacing_enabled: bool,
    font: &mut FontState,
) -> Vec<Inline> {
    let mut builder = InlineBuilder::with_spacing(spacing_enabled);
    builder.font = *font;
    append_inline_node(&mut builder, node, default_name);
    *font = builder.font;
    builder.finish()
}

/// Generated references/declarations consume their inner boundaries as part
/// of their own punctuation, rather than exporting an AST-tail approximation.
fn lower_atomic_node(
    node: &Node,
    default_name: Option<&str>,
    spacing_enabled: bool,
    font: &mut FontState,
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
            let saved = font.push_scope(if node.flags.synopsis_pretty && node.flags.line_start {
                Font::Strong
            } else {
                Font::Emphasis
            });
            let lowered =
                lower_inline_nodes_with_font_state(children, default_name, spacing_enabled, font);
            if node.flags.synopsis_pretty {
                font.select(Font::Strong);
            }
            font.pop_scope(saved);
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
        Some("Lk") => lower_link(children, default_name, spacing_enabled, font),
        Some("Mt") => lower_mail_addresses(children, default_name, spacing_enabled, font),
        Some("Bx") => lower_bsd_reference(
            node,
            lower_inline_nodes_with_font_state(children, default_name, spacing_enabled, font),
        ),
        _ => unreachable!("only generated references/declarations are atomic"),
    };
    if let Some(anchor) = navigation_anchor(node) {
        output.insert(0, anchor);
    }
    output
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

pub(super) fn enclosure_marks(name: &str) -> Option<(&'static str, &'static str)> {
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

pub(super) fn alternating_font_pair(macro_name: Option<&str>) -> Option<(Font, Font)> {
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
    fn font_scope_pop_preserves_previous_selection_and_spacing() {
        let mut builder = super::InlineBuilder::new();
        builder.font.select(super::Font::Emphasis);
        builder.font.select(super::Font::Strong);
        builder.with_font_scope(super::Font::Code, |builder| {
            builder.with_font_scope(super::Font::Regular, |builder| {
                builder.tighten_next_boundary();
            });
            assert_eq!(builder.font.current, super::Font::Code);
            assert_eq!(builder.font.previous, super::Font::Code);
        });
        assert_eq!(builder.font.current, super::Font::Strong);
        assert_eq!(builder.font.previous, super::Font::Code);
        assert!(builder.has_tight_boundary());
    }

    #[test]
    fn prefix_scope_distinguishes_invisible_operands_from_explicit_joins() {
        for explicit in [false, true] {
            let mut builder = super::InlineBuilder::new();
            builder.append(super::text_node("-"));
            builder.with_prefix_join(|builder| {
                builder.append(vec![super::Inline::anchor("operand")]);
                builder.append(super::text_node(""));
                if explicit {
                    builder.tighten_next_boundary();
                }
            });
            builder.append(super::text_node("next"));
            let output = builder.finish();
            assert_eq!(
                super::plain_text(&output),
                if explicit { "-next" } else { "- next" }
            );
            assert!(output.iter().any(
                |node| matches!(node, super::Inline::Anchor { id, .. } if id.as_str() == "operand")
            ));
        }
        let mut builder = super::InlineBuilder::new();
        builder.append(super::text_node("-"));
        builder.with_prefix_join(|builder| {
            builder.append(super::text_node("-"));
            builder.with_prefix_join(|builder| builder.append(super::text_node("")));
        });
        builder.append(super::text_node("next"));
        assert_eq!(super::plain_text(&builder.finish()), "-- next");
    }

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
    fn decoded_font_spellings_are_never_reinterpreted_as_controls() {
        let generated = parse_roff_text(r"\fB\\fBpackage.json\\fR config\fR");
        assert_eq!(plain_text(&generated), r"\fBpackage.json\fR config");
        assert!(matches!(generated.as_slice(), [Inline::Strong { .. }]));

        let emphasis = parse_roff_text(r"\fI\\fIvalue\\fR\fR");
        assert_eq!(plain_text(&emphasis), r"\fIvalue\fR");
        assert!(matches!(emphasis.as_slice(), [Inline::Emphasis { .. }]));

        let code = parse_roff_text(r"\fC\\fCvalue\\fR\fR");
        assert_eq!(plain_text(&code), r"\fCvalue\fR");
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
