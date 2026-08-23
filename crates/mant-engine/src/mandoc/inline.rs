//! Lowers typed roff events and semantic mdoc macros into inline IR nodes.

use std::borrow::Cow;

use libmandoc_rs::{Node, NodeKind};
use mant_ir::Inline;

use crate::inline::{first_visible_character, has_printable_character, last_visible_character};
pub(crate) use crate::inline::{plain_text, terms_fit_inline};

mod source;
mod source_mdoc;

use source::roff_macro_arguments;
pub(super) use source_mdoc::lower_source_mdoc_request;

use super::{
    first_part_children,
    reference::trailing_sphinx_manual_reference,
    roff_escape::{RoffFont as Font, RoffInlineEvent, decode, visible_text},
};

pub(super) struct InlineBuilder {
    nodes: Vec<Inline>,
    boundary: PendingBoundary,
    spacing: SpacingMode,
    last_visible_character: Option<char>,
    has_printable_content: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingBoundary {
    Ordinary,
    Tight,
    Preserved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpacingMode {
    Enabled,
    Disabled,
}

impl SpacingMode {
    const fn enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }

    const fn from_enabled(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

impl From<bool> for SpacingMode {
    fn from(enabled: bool) -> Self {
        Self::from_enabled(enabled)
    }
}

/// Semantic boundary between two inline fragments in filled roff mode.
///
/// Roff distinguishes ordinary source wrapping from an input line whose first
/// text character is whitespace.  The former fills as a word boundary; the
/// latter starts a new output line.  Keeping that distinction here prevents
/// renderers from having to rediscover formatter semantics from flattened
/// text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FilledBoundary {
    SameLine,
    Word,
    LineBreak,
}

impl InlineBuilder {
    pub(super) const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            boundary: PendingBoundary::Ordinary,
            spacing: SpacingMode::Enabled,
            last_visible_character: None,
            has_printable_content: false,
        }
    }

    pub(super) const fn with_spacing(spacing_enabled: bool) -> Self {
        Self {
            nodes: Vec::new(),
            boundary: PendingBoundary::Ordinary,
            spacing: SpacingMode::from_enabled(spacing_enabled),
            last_visible_character: None,
            has_printable_content: false,
        }
    }

    pub(super) fn tighten_next_boundary(&mut self) {
        self.boundary = PendingBoundary::Tight;
    }

    pub(super) const fn has_tight_boundary(&self) -> bool {
        matches!(self.boundary, PendingBoundary::Tight)
    }

    pub(super) const fn spacing_enabled(&self) -> bool {
        self.spacing.enabled()
    }

    pub(super) fn set_spacing(&mut self, setting: &str) {
        let updated = updated_spacing(self.spacing.enabled(), setting);
        if updated == self.spacing.enabled() {
            return;
        }
        // `.Sm off` changes spacing *after* the request. If printable
        // content precedes the transition, retain its ordinary boundary to
        // the first following fragment, then concatenate subsequent macro
        // arguments until spacing is enabled again.
        self.boundary = match (updated, self.nodes.is_empty(), self.boundary) {
            (_, _, PendingBoundary::Tight) => PendingBoundary::Tight,
            (false, false, _) => PendingBoundary::Preserved,
            _ => PendingBoundary::Ordinary,
        };
        self.spacing = SpacingMode::from(updated);
    }

    /// Carry formatter state out of a nested structural wrapper.
    ///
    /// The nested builder has already applied the transition at its exact
    /// source position. The parent therefore inherits only the final state;
    /// replaying `set_spacing` here would invent a preserved boundary after a
    /// nested `Sm off` request.
    pub(super) fn inherit_spacing(&mut self, spacing_enabled: bool) {
        self.spacing = SpacingMode::from(spacing_enabled);
        if matches!(self.boundary, PendingBoundary::Preserved) {
            self.boundary = PendingBoundary::Ordinary;
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Preserve a formatter-requested line boundary without creating empty
    /// leading, repeated, or trailing rows around the paragraph.
    pub(super) fn hard_break(&mut self) {
        self.boundary = PendingBoundary::Ordinary;
        if self.has_printable_content && !matches!(self.nodes.last(), Some(Inline::LineBreak)) {
            self.nodes.push(Inline::LineBreak);
            self.last_visible_character = Some('\n');
        }
    }

    pub(super) fn append(&mut self, mut incoming: Vec<Inline>) {
        self.append_at_boundary(&mut incoming);
    }

    /// Append content using the formatter-level boundary selected by the
    /// block lowering pass.
    pub(super) fn append_filled(&mut self, incoming: Vec<Inline>, boundary: FilledBoundary) {
        match boundary {
            FilledBoundary::SameLine => self.append(incoming),
            FilledBoundary::Word => {
                let mut incoming = incoming;
                self.append_at_boundary(&mut incoming);
            }
            FilledBoundary::LineBreak => {
                self.hard_break();
                self.append(incoming);
            }
        }
    }

    fn append_at_boundary(&mut self, incoming: &mut Vec<Inline>) {
        if incoming.is_empty() {
            return;
        }
        let incoming_first = first_visible_character(incoming);
        let incoming_last = last_visible_character(incoming);
        let incoming_has_printable = has_printable_character(incoming);
        let add_space = needs_boundary_space(self.last_visible_character, incoming_first);
        let boundary = std::mem::replace(&mut self.boundary, PendingBoundary::Ordinary);
        if (self.spacing.enabled() || matches!(boundary, PendingBoundary::Preserved))
            && !matches!(boundary, PendingBoundary::Tight)
            && add_space
        {
            push_text(&mut self.nodes, " ".to_owned());
            self.last_visible_character = Some(' ');
            self.has_printable_content = true;
        }
        self.nodes.append(incoming);
        if incoming_last.is_some() {
            self.last_visible_character = incoming_last;
        }
        self.has_printable_content |= incoming_has_printable;
    }

    pub(super) fn finish(mut self) -> Vec<Inline> {
        while matches!(self.nodes.last(), Some(Inline::LineBreak)) {
            self.nodes.pop();
        }
        self.nodes
    }
}

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
        append_inline_node(&mut builder, node, default_name);
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
    if node.flags.delimiter_close {
        builder.tighten_next_boundary();
    }
    match node.macro_name.as_deref() {
        Some("Ns") => builder.tighten_next_boundary(),
        // `Pf` owns visible prefix text and suppresses only the boundary to
        // the following sibling. Treating it like the empty `Ns` request
        // silently discarded constructs such as `.Pf [\-]ddd Cm \&.`.
        Some("Pf") => {
            builder.append(lower_inline_node(
                node,
                default_name,
                builder.spacing_enabled(),
            ));
            builder.tighten_next_boundary();
        }
        // A roff break ends the current output line, not the paragraph.
        // `Pp` can also occur inside an extended mdoc definition head, where
        // it separates alternative terms without ending the owning item.
        // Keeping both inline lets the definition lowering retain that
        // distinction instead of concatenating the alternatives.
        Some("br" | "Pp") => builder.hard_break(),
        // Formatting requests carry control arguments such as `CW` and `R`.
        // `Es` likewise only changes the delimiters later `En` nodes use;
        // libmandoc resolves those delimiters onto each invocation. These
        // requests change formatter state and are never document text.
        // Verbatim regions already retain their semantics through
        // libmandoc's no-fill flag, so leaking these arguments would only
        // create phantom paragraphs around preformatted blocks.
        Some(
            "Es" | "Sm" | "PD" | "ad" | "fi" | "ft" | "hy" | "in" | "na" | "ne" | "nf" | "nh"
            | "nr" | "ta",
        ) => {}
        Some("Ap") => {
            builder.tighten_next_boundary();
            builder.append(vec![Inline::Text { value: "'".into() }]);
            builder.tighten_next_boundary();
        }
        _ => builder.append(lower_inline_node(
            node,
            default_name,
            builder.spacing_enabled(),
        )),
    }
    if node.flags.delimiter_open || node.flags.line_continuation || ends_with_no_space_control(node)
    {
        builder.tighten_next_boundary();
    }
}

/// Whether a nested inline scope leaves a no-space request for its next sibling.
///
/// libmandoc can keep `.Ns` as the final child of a styled macro while moving
/// the following text beside that macro. The inner builder sees the request,
/// but without this propagation its pending boundary would disappear when the
/// styled fragment is returned to the outer flow.
fn ends_with_no_space_control(node: &Node) -> bool {
    if matches!(node.macro_name.as_deref(), Some("Ns" | "Pf")) {
        return true;
    }
    node.children
        .iter()
        .rev()
        .find(|child| child.kind != NodeKind::Comment && !child.flags.no_print)
        .is_some_and(ends_with_no_space_control)
}

fn lower_inline_node(
    node: &Node,
    default_name: Option<&str>,
    spacing_enabled: bool,
) -> Vec<Inline> {
    if node.flags.no_print || node.kind == NodeKind::Comment {
        return Vec::new();
    }
    if node.kind == NodeKind::Text {
        return lower_text_node(node, Font::Regular);
    }
    if node.kind == NodeKind::Equation {
        return node
            .equation
            .as_deref()
            .map(visible_text)
            .filter(|value| !value.trim().is_empty())
            .map(|value| vec![Inline::Code { value }])
            .unwrap_or_default();
    }

    let macro_name = node.macro_name.as_deref();
    if macro_name == Some("Nm") && node.kind == NodeKind::Block {
        return lower_structural_name(node, default_name, spacing_enabled);
    }
    let children = inline_children(node);
    // man(7) alternating-font macros concatenate their arguments without
    // inserting spaces. Each argument switches to the next named font.
    let lowered = alternating_font_pair(macro_name).map_or_else(
        || lower_inline_nodes_with_spacing(children, default_name, spacing_enabled),
        |(first, second)| {
            lower_alternating_fonts(children, default_name, first, second, spacing_enabled)
        },
    );
    let anchor = navigation_anchor(node, &lowered);
    let mut output = lower_macro_inline(
        node,
        macro_name,
        children,
        lowered,
        default_name,
        spacing_enabled,
    );
    if let Some(anchor) = anchor {
        output.insert(0, anchor);
    }
    output
}

fn lower_macro_inline(
    node: &Node,
    macro_name: Option<&str>,
    children: &[Node],
    lowered: Vec<Inline>,
    default_name: Option<&str>,
    spacing_enabled: bool,
) -> Vec<Inline> {
    match macro_name {
        Some("Nm") => wrap_strong(if lowered.is_empty() {
            default_name.map_or_else(Vec::new, text_node)
        } else {
            lowered
        }),
        Some("Fl") => {
            // mdoc prepends one dash per `Fl` unconditionally: nested
            // `.Fl Fl acls` is the canonical spelling of `--acls`, and a bare
            // trailing `.Fl Fl` renders the `--` end-of-options marker.
            let mut content = vec![Inline::Text { value: "-".into() }];
            content.extend(lowered);
            wrap_strong(content)
        }
        Some("Cm" | "Ic" | "Sy" | "B" | "SB") => wrap_strong(lowered),
        Some("Ar" | "Pa" | "Em" | "Va" | "Vt" | "Ft" | "Fa" | "I") => wrap_emphasis(lowered),
        Some("Li") => vec![Inline::Code {
            value: plain_text(&lowered),
        }],
        Some("In") if !lowered.is_empty() => vec![Inline::Code {
            value: format!("#include <{}>", plain_text(&lowered)),
        }],
        Some("Xr" | "MR") => lower_manual_reference(children, default_name, spacing_enabled),
        Some("Lk") => lower_link(children, default_name, false, spacing_enabled),
        Some("Mt") => lower_link(children, default_name, true, spacing_enabled),
        Some("Bx") => lower_bsd_reference(node, lowered),
        // Keep the heading text as a private unresolved target until the
        // complete section tree is available. The document post-pass replaces
        // it with the stable Section::id or degrades it to ordinary text.
        Some("Sx") if !lowered.is_empty() => vec![Inline::Link {
            target: mant_ir::LinkTarget::Section {
                id: plain_text(&lowered).trim().into(),
            },
            title: None,
            children: lowered,
        }],
        Some("Nd") => {
            // `Nd` owns the separator between the name list and its one-line
            // description. This is formatter-generated punctuation rather
            // than a boundary between sibling source nodes, so spell the
            // required trailing space explicitly.
            let mut content = text_node("— ");
            content.extend(lowered);
            content
        }
        Some("Fn") => lower_function_element(node, default_name, spacing_enabled),
        Some("Fo") => lower_function_declaration(node, default_name, spacing_enabled),
        Some("Eo") => surround_fragments(
            lower_inline_nodes_with_spacing(
                first_part_children(node, NodeKind::Head),
                default_name,
                spacing_enabled,
            ),
            lowered,
            lower_inline_nodes_with_spacing(
                first_part_children(node, NodeKind::Tail),
                default_name,
                spacing_enabled,
            ),
        ),
        Some("En") => match node.enclosure.as_ref() {
            Some(enclosure) => surround(
                &visible_text(&enclosure.opening),
                lowered,
                &enclosure
                    .closing
                    .as_deref()
                    .map(visible_text)
                    .unwrap_or_default(),
            ),
            None => lowered,
        },
        Some(name) if enclosure_marks(name).is_some() => {
            let (opening, closing) = enclosure_marks(name).expect("matched enclosure macro");
            let mut content = surround(opening, lowered, closing);
            content.extend(trailing_enclosure_delimiters(
                node,
                default_name,
                spacing_enabled,
            ));
            content
        }
        _ => lowered,
    }
}

/// Retain punctuation that libmandoc moves behind an implicit enclosure body.
///
/// In input such as `.Pq phrase ;`, the semicolon is neither part of the
/// structural body nor the generated closing parenthesis. libmandoc keeps it
/// as a direct child and marks its validated delimiter role. Reading only the
/// body would silently turn `(phrase);` into `(phrase)`.
fn trailing_enclosure_delimiters(
    node: &Node,
    default_name: Option<&str>,
    spacing_enabled: bool,
) -> Vec<Inline> {
    node.children
        .iter()
        .filter(|child| child.flags.delimiter_close)
        .flat_map(|child| lower_inline_node(child, default_name, spacing_enabled))
        .collect()
}

/// Lower the block form of an mdoc `Nm` synopsis without losing its head.
///
/// In an extended `It Xo ... Xc` term, libmandoc places the command in the
/// `Nm` head and the following options in its body.  Treating that wrapper as
/// an ordinary inline `Nm` selects only the body, omits the command whenever
/// options exist, and wraps every option in the command's strong style.  An
/// empty body takes the opposite fallback path and lowers both structural
/// parts, duplicating the command.  Keep the two parts explicit instead:
/// style the head as the command and append the body with its own semantics.
fn lower_structural_name(
    node: &Node,
    default_name: Option<&str>,
    spacing_enabled: bool,
) -> Vec<Inline> {
    let head = lower_inline_nodes_with_spacing(
        first_part_children(node, NodeKind::Head),
        default_name,
        spacing_enabled,
    );
    let head = if head.is_empty() {
        default_name.map_or_else(Vec::new, text_node)
    } else {
        head
    };
    let body = lower_inline_nodes_with_spacing(
        first_part_children(node, NodeKind::Body),
        default_name,
        spacing_enabled,
    );
    let mut builder = InlineBuilder::with_spacing(spacing_enabled);
    builder.append(wrap_strong(head));
    builder.append(body);
    builder.finish()
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

    let mut declaration = vec![Inline::Strong { children: head }];
    declaration.push(Inline::Text { value: "(".into() });
    let mut has_argument = false;
    for argument in body {
        if argument.macro_name.as_deref() == Some("Fa") && inline_children(argument).len() > 1 {
            // One mdoc `Fa` invocation can declare several parameters. The
            // formatter owns the comma between those operands just as it
            // owns the comma between separate `Fa` invocations. Flatten the
            // semantic operands here instead of spacing the whole `Fa` node
            // as one argument. This is also required for function-pointer
            // declarations embedded outside SYNOPSIS, where libmandoc does
            // not set `synopsis_pretty` but `Fo` still owns a parameter list.
            // Validated closing delimiters remain attached to the preceding
            // parameter and never create a phantom one.
            for operand in inline_children(argument) {
                if operand.flags.delimiter_close {
                    declaration.extend(lower_inline_node(operand, default_name, spacing_enabled));
                    continue;
                }
                if has_argument {
                    declaration.push(Inline::Text { value: ", ".into() });
                }
                declaration.extend(wrap_emphasis(lower_inline_node(
                    operand,
                    default_name,
                    spacing_enabled,
                )));
                has_argument = true;
            }
        } else {
            if has_argument && !argument.flags.delimiter_close {
                declaration.push(Inline::Text { value: ", ".into() });
            }
            declaration.extend(lower_inline_node(argument, default_name, spacing_enabled));
            has_argument |= !argument.flags.delimiter_close;
        }
    }
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
        "Dq" | "Do" | "Qq" | "Qo" => Some(("“", "”")),
        "Sq" | "So" | "Ql" => Some(("‘", "’")),
        "Pq" | "Po" => Some(("(", ")")),
        "Brq" | "Bro" => Some(("{", "}")),
        "Aq" | "Ao" => Some(("<", ">")),
        _ => None,
    }
}

/// Convert libmandoc's validated deep-link marker into a zero-width IR node.
/// Explicit `.Tg` tags carry `node.tag`; automatically discovered tags fall
/// back to the same first visible word that libmandoc uses.
fn navigation_anchor(node: &Node, lowered: &[Inline]) -> Option<Inline> {
    if !node.flags.deep_link_target {
        return None;
    }
    let id = node.tag.as_deref().map(visible_text).or_else(|| {
        plain_text(lowered)
            .split_whitespace()
            .next()
            .map(ToOwned::to_owned)
    })?;
    (!id.is_empty()).then_some(Inline::Anchor { id: id.into() })
}

fn inline_children(node: &Node) -> &[Node] {
    let body = first_part_children(node, NodeKind::Body);
    if body.is_empty() {
        &node.children
    } else {
        body
    }
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

fn lower_link(
    children: &[Node],
    default_name: Option<&str>,
    email: bool,
    spacing_enabled: bool,
) -> Vec<Inline> {
    let Some(first) = children.first() else {
        return Vec::new();
    };
    let address = plain_text(&lower_inline_node(first, default_name, spacing_enabled));
    if address.is_empty() {
        return Vec::new();
    }
    let label = lower_inline_nodes_with_spacing(&children[1..], default_name, spacing_enabled);
    lower_external_link(address, label, email)
}

/// Build an mdoc external link without allowing punctuation to hide its target.
///
/// `Lk` and `Mt` accept ordinary trailing sentence punctuation as an argument.
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

fn lower_alternating_fonts(
    children: &[Node],
    default_name: Option<&str>,
    first: Font,
    second: Font,
    spacing_enabled: bool,
) -> Vec<Inline> {
    let mut output = Vec::new();
    for (index, child) in children.iter().enumerate() {
        let font = if index % 2 == 0 { first } else { second };
        // An alternating man(7) macro establishes the *initial* font for
        // each argument. Explicit `\\f` escapes inside that argument must
        // still be able to reset or replace it; wrapping an already-lowered
        // argument would incorrectly nest the outer font around the reset.
        let lowered = if child.kind == NodeKind::Text {
            lower_text_node(child, font)
        } else {
            apply_font(
                lower_inline_node(child, default_name, spacing_enabled),
                font,
            )
        };
        output.extend(lowered);
    }
    output
}

/// Lower a source request that libmandoc flattened while parsing a `tbl`
/// `T{ ... T}` cell. This shares the ordinary man(7) alternating-font model,
/// including tight argument concatenation and explicit `\\f` overrides.
pub(super) fn lower_source_alternating_fonts(
    macro_name: &str,
    source: &str,
) -> Option<Vec<Inline>> {
    let (first, second) = alternating_font_pair(Some(macro_name))?;
    let mut output = Vec::new();
    for (index, argument) in roff_macro_arguments(source).into_iter().enumerate() {
        let font = if index % 2 == 0 { first } else { second };
        output.extend(parse_roff_text_with_font(&argument, font, true));
    }
    Some(output)
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

fn apply_font(children: Vec<Inline>, font: Font) -> Vec<Inline> {
    match font {
        Font::Regular => children,
        Font::Strong => wrap_strong(children),
        Font::Emphasis => wrap_emphasis(children),
        Font::StrongEmphasis => wrap_strong(wrap_emphasis(children)),
        Font::Code | Font::CodeStrong | Font::CodeEmphasis => {
            let code = (!children.is_empty())
                .then(|| Inline::Code {
                    value: plain_text(&children),
                })
                .into_iter()
                .collect();
            match font {
                Font::Code => code,
                Font::CodeStrong => wrap_strong(code),
                Font::CodeEmphasis => wrap_emphasis(code),
                Font::Regular | Font::Strong | Font::Emphasis | Font::StrongEmphasis => {
                    unreachable!()
                }
            }
        }
    }
}

fn surround(open: &str, mut children: Vec<Inline>, close: &str) -> Vec<Inline> {
    let mut result = text_node(open);
    result.append(&mut children);
    result.extend(text_node(close));
    result
}

fn surround_fragments(
    mut opening: Vec<Inline>,
    mut children: Vec<Inline>,
    mut closing: Vec<Inline>,
) -> Vec<Inline> {
    opening.append(&mut children);
    opening.append(&mut closing);
    opening
}

fn text_node(value: &str) -> Vec<Inline> {
    vec![Inline::Text {
        value: value.to_owned(),
    }]
}

pub(super) fn parse_roff_text(source: &str) -> Vec<Inline> {
    parse_roff_text_with_font(source, Font::Regular, true)
}

/// Decode one roff text run using the font selected by its enclosing macro.
/// Explicit `\\f` escapes change `font` while the run is scanned, so a reset
/// to regular text remains visible even inside an alternating `.BI` argument.
fn parse_roff_text_with_font(
    source: &str,
    initial_font: Font,
    recognize_generated_references: bool,
) -> Vec<Inline> {
    let mut output = Vec::new();
    let mut buffer = String::new();
    let mut font = initial_font;
    let mut link: Option<String> = None;

    for event in decode(source) {
        match event {
            RoffInlineEvent::Text(value) => {
                buffer.push_str(&normalize_redundant_escaped_font(&value, font));
            }
            RoffInlineEvent::Font(next_font) => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                font = next_font;
            }
            RoffInlineEvent::Link(target) => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                link = target;
            }
            RoffInlineEvent::EmptyDestination => {
                if !recognize_generated_references
                    || !promote_sphinx_manual_reference(
                        &mut output,
                        &mut buffer,
                        font,
                        link.as_deref(),
                    )
                {
                    buffer.push_str("<>");
                }
            }
            RoffInlineEvent::LineBreak => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                if !matches!(output.last(), Some(Inline::LineBreak)) {
                    output.push(Inline::LineBreak);
                }
            }
            RoffInlineEvent::Presentation { .. } => {}
        }
    }
    flush_segment(&mut output, &mut buffer, font, link.as_deref());
    output
}

/// Some generated manuals wrap a link label in a font and then escape another
/// copy of that same font request as visible text. libmandoc correctly reports
/// the enclosing font, so remove only the redundant escaped request. Keeping
/// this conditional on the enclosing font preserves literal `\\f` examples in
/// formatter manuals and ordinary prose.
fn normalize_redundant_escaped_font(source: &str, font: Font) -> Cow<'_, str> {
    let opening = match font {
        Font::Strong => r"\fB",
        Font::Emphasis => r"\fI",
        Font::StrongEmphasis => r"\f[BI]",
        Font::Code => r"\fC",
        Font::CodeStrong => r"\f[CB]",
        Font::CodeEmphasis => r"\f[CI]",
        Font::Regular => return Cow::Borrowed(source),
    };
    if !source.contains(opening) {
        return Cow::Borrowed(source);
    }

    Cow::Owned(source.replace(opening, "").replace(r"\fR", ""))
}

/// Lower a text node after honoring a macro-provided default font. Nodes marked
/// non-printing by libmandoc are never allowed to escape through this shortcut.
fn lower_text_node(node: &Node, initial_font: Font) -> Vec<Inline> {
    if node.flags.no_print || node.kind == NodeKind::Comment {
        Vec::new()
    } else {
        parse_roff_text_with_font(
            node.text.as_deref().unwrap_or_default(),
            initial_font,
            !node.flags.no_fill,
        )
    }
}

fn promote_sphinx_manual_reference(
    output: &mut Vec<Inline>,
    buffer: &mut String,
    font: Font,
    external_link: Option<&str>,
) -> bool {
    if external_link.is_some() || matches!(font, Font::Code | Font::CodeStrong | Font::CodeEmphasis)
    {
        return false;
    }
    let Some(reference) = trailing_sphinx_manual_reference(buffer) else {
        return false;
    };
    let prefix = reference.prefix.to_owned();
    let display = reference.display.to_owned();
    let name = reference.name.to_owned();
    let manual_section = reference.manual_section.to_owned();
    *buffer = prefix;
    flush_segment(output, buffer, font, None);
    output.push(Inline::Link {
        target: mant_ir::LinkTarget::Manual {
            name,
            manual_section: Some(manual_section),
        },
        title: None,
        children: vec![styled_segment(display, font)],
    });
    true
}

fn flush_segment(output: &mut Vec<Inline>, buffer: &mut String, font: Font, link: Option<&str>) {
    if buffer.is_empty() {
        return;
    }
    let value = std::mem::take(buffer);
    let styled = styled_segment(value, font);
    if let Some(target) = link {
        output.push(Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: target.to_owned(),
            },
            title: None,
            children: vec![styled],
        });
    } else {
        output.push(styled);
    }
}

fn styled_segment(value: String, font: Font) -> Inline {
    match font {
        Font::Regular => Inline::Text { value },
        Font::Strong => Inline::Strong {
            children: vec![Inline::Text { value }],
        },
        Font::Emphasis => Inline::Emphasis {
            children: vec![Inline::Text { value }],
        },
        Font::StrongEmphasis => Inline::Strong {
            children: vec![Inline::Emphasis {
                children: vec![Inline::Text { value }],
            }],
        },
        Font::Code => Inline::Code { value },
        Font::CodeStrong => Inline::Strong {
            children: vec![Inline::Code { value }],
        },
        Font::CodeEmphasis => Inline::Emphasis {
            children: vec![Inline::Code { value }],
        },
    }
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
    use super::{
        FilledBoundary, Font, InlineBuilder, parse_roff_text, parse_roff_text_with_font, plain_text,
    };
    use mant_ir::Inline;

    #[test]
    fn inline_builder_tracks_nested_visible_boundaries_incrementally() {
        let mut builder = InlineBuilder::new();
        builder.append(vec![Inline::Anchor {
            id: "start".to_owned().into(),
        }]);
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
