//! Compose semantic wrappers in output order, not by inspecting AST tails.
use super::font::coalesce_font_runs;
use super::links::{append_bsd_reference, append_link, append_mail_addresses};
use super::{
    Font, Inline, InlineBuilder, Node, NodeKind, append_include, append_inline_nodes,
    authored_section_phrase, first_part_children, inline_children, lower_equation_node,
    navigation_anchor,
};

pub(super) fn append(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    if node.macro_name.as_deref() == Some("Tg") {
        if let Some(anchor) = navigation_anchor(node) {
            builder.append(vec![anchor]);
        }
        return;
    }
    if node.flags.no_print || node.kind == NodeKind::Comment {
        return;
    }
    if append_structural_scope(builder, node, name) {
        return;
    }
    // Atomic reconstructions own their generated punctuation (references,
    // function declarations, etc.); their output cannot carry a guessed tail
    // effect. Transparent and styled scopes below share the caller's flow.
    if append_atomic(builder, node, name) {
        return;
    }
    if let Some(anchor) = navigation_anchor(node) {
        builder.append(vec![anchor]);
    }
    let posts = builder.scope_posts.clone();
    let mut sink = InlineContainerSink {
        builder,
        name,
        root_id: node.id,
    };
    if crate::mandoc::containers::drive(node, &posts, &mut sink) {
        return;
    }
    append_scope_children(builder, node, name);
}

fn append_structural_scope(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) -> bool {
    if node.kind == NodeKind::Block
        && node.macro_name.as_deref() == Some("Bl")
        && node.list_kind.is_some()
        && (node.list_kind == Some(libmandoc_rs::NormalizedListKind::Column)
            || builder.has_definition_list_execution())
    {
        if node.list_kind == Some(libmandoc_rs::NormalizedListKind::Column) {
            append_column_list(builder, node, name);
        } else {
            append_nested_list(builder, node, name);
        }
        return true;
    }
    if node.kind == NodeKind::Block
        && matches!(node.macro_name.as_deref(), Some("D1" | "Dl" | "Bd"))
    {
        if let Some(anchor) = navigation_anchor(node) {
            builder.append(vec![anchor]);
        }
        append_display(builder, node, name);
        return true;
    }
    false
}

fn append_scope_children(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    let children = inline_children(node);
    match node.macro_name.as_deref() {
        Some("Fn" | "Fo") => super::generated::function(builder, node, name),
        Some("Xr" | "MR") => super::generated::manual_reference(builder, node, name),
        Some("Nm") if node.kind == NodeKind::Block => {
            builder.with_font_scope(Font::Strong, |builder| {
                append_name(builder, first_part_children(node, NodeKind::Head), name);
            });
            append_inline_nodes(builder, first_part_children(node, NodeKind::Body), name);
        }
        Some("Nm") => {
            builder.with_font_scope(Font::Strong, |builder| append_name(builder, children, name));
        }
        Some("Fl") => builder.append_scope(
            |builder| {
                builder.with_font_scope(Font::Strong, |builder| {
                    builder.append_text("-");
                    builder
                        .with_prefix_join(|builder| append_inline_nodes(builder, children, name));
                });
            },
            coalesce_font_runs,
        ),
        // mdoc_term.c routes Cd and Fd through termp_fd_pre = termp_bold_pre
        // (dispatch lines 142/149): unconditional Strong in every section.
        Some("Cm" | "Ic" | "Sy" | "Ms" | "Cd" | "Fd") => {
            builder.with_font_scope(Font::Strong, |builder| {
                append_inline_nodes(builder, children, name);
            });
        }
        Some("Ar" | "Pa" | "Em" | "Va" | "Vt" | "Ft" | "Fa" | "Ad" | "Fr") => builder
            .with_font_scope(Font::Emphasis, |builder| {
                append_inline_nodes(builder, children, name);
            }),
        Some("No" | "Dv") => builder.with_font_scope(Font::Regular, |builder| {
            append_inline_nodes(builder, children, name);
        }),
        Some("Li") => builder.with_font_scope(Font::Code, |builder| {
            append_inline_nodes(builder, children, name);
        }),
        Some("Sx") => {
            let authored_target = authored_section_phrase(children, name);
            let previous_owner = builder.zero_advance.pending_native_owner();
            let annotated = builder.append_semantic_scope(
                node.id,
                |builder| {
                    builder.with_font_scope(Font::Emphasis, |builder| {
                        append_inline_nodes(builder, children, name);
                    });
                },
                |children| section_reference(authored_target.clone(), children),
            );
            builder.zero_advance.bind_pending_link(
                previous_owner,
                mant_ir::LinkTarget::Section {
                    id: authored_target.into(),
                },
                annotated,
            );
        }
        Some("Nd") => {
            // mdoc_term.c::termp_nd_pre() prints `\(en` (U+2013), not an em
            // dash. Its next term_word() supplies the separate automatic
            // separator; there is no authored blank in this generated word.
            builder.append_text(&super::catalog_glyph("en").to_string());
            append_inline_nodes(builder, children, name);
        }
        // mdoc_html.c::mdoc__x_pre() enriches these fields with typed
        // external targets while the visible terminal word is unchanged.
        Some("%U") => super::links::append_reference_field_link(builder, children, name, false),
        Some("%R") => super::links::append_reference_field_link(builder, children, name, true),
        Some("%T") if node.reference_quotes_title => append_quoted_title(builder, children, name),
        // mdoc_term.c maps these reference fields through termp_under_pre();
        // quoted %T takes the separate branch above when a journal is present.
        Some("%B" | "%I" | "%J" | "%T") => {
            builder.with_font_scope(Font::Emphasis, |builder| {
                append_inline_nodes(builder, children, name);
            });
        }
        _ => append_inline_nodes(builder, children, name),
    }
}

fn append_column_list(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    // Bl BLOCK pre owns an ordinary term_newln(); its HEAD is not printed
    // (mdoc_term.c::termp_bl_pre,1129-1136). Keep this execution in the
    // caller's state instead of treating a structural HEAD child as text.
    builder.no_fill_source_line();
    let in_head = builder.has_definition_head();
    let saved = builder.enter_nested_list_scope(true);
    let Some(body) = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Body)
    else {
        builder.exit_nested_list_scope(saved, false);
        return;
    };
    builder.begin_executed_node(body);
    let mut first_item = true;
    for item in &body.children {
        if item.kind != NodeKind::Block || item.macro_name.as_deref() != Some("It") {
            append_inline_nodes(builder, std::slice::from_ref(item), name);
            continue;
        }
        builder.begin_executed_node(item);
        builder.no_fill_source_line();
        // print_bvspace() stops at the enclosing non-item It ancestor. Only
        // the first column item adds this row; later column items suppress
        // inter-item vspace (mdoc_term.c:583-628).
        if first_item && !node.compact && in_head {
            builder.native_vertical_space(1);
        }
        first_item = false;
        if let Some(head) = item
            .children
            .iter()
            .find(|part| part.kind == NodeKind::Head)
        {
            builder.begin_executed_node(head);
            builder.enter_nested_column_part(true);
        }
        let parts: Vec<_> = item
            .children
            .iter()
            .filter(|part| part.kind == NodeKind::Body)
            .collect();
        for (index, part) in parts.iter().enumerate() {
            builder.begin_executed_node(part);
            builder.enter_nested_column_part(index + 1 == parts.len());
            let posts = builder.scope_posts.clone();
            posts.enter_body(part.id, builder.font.checkpoint());
            append_inline_nodes(builder, &part.children, name);
            if let Some(font) = posts.exit_body(part.id) {
                builder.font.pop_scope(font);
            }
            builder.finish_nested_column_part();
        }
    }
    // Bl BLOCK post calls term_newln() before resetting the default stops.
    builder.no_fill_source_line();
    super::display_tabs::exit_post(&mut builder.execution, node);
    builder.exit_nested_list_scope(saved, !first_item);
}

fn nested_list_distance(text: &str) -> crate::mandoc::layout::Distance {
    use crate::mandoc::layout::Distance;
    // a2width() treats an unscaled token as a printed width sample, not
    // as an implicit EN distance (mdoc_term.c:566-577). This reads a layout
    // fact; it never executes these sample escapes in the live formatter.
    if let Some((number, unit)) = text
        .trim()
        .split_at_checked(text.trim().len().saturating_sub(1))
        && matches!(
            unit,
            "n" | "m" | "u" | "c" | "f" | "i" | "M" | "P" | "v" | "p"
        )
        && number.parse::<f64>().is_ok()
    {
        return Distance::parse(text).unwrap_or_default();
    }
    let visible = super::plain_text(&super::parse_roff_text(text));
    Distance::cells(mant_ir::geometry::coordinate(
        mant_ir::geometry::text_width(&visible),
    ))
}

struct NestedListGeometry {
    kind: libmandoc_rs::NormalizedListKind,
    style: Option<libmandoc_rs::DefinitionListStyle>,
    head_width_units: usize,
    body_width_units: usize,
    offset_units: i32,
    flush_head: bool,
}

impl NestedListGeometry {
    fn from_node(node: &Node) -> Self {
        use crate::mandoc::layout::Distance;
        use libmandoc_rs::{DefinitionListStyle, NormalizedListKind};

        let kind = node.list_kind.unwrap_or(NormalizedListKind::Definition);
        let style = node.definition_list_style;
        let sized = matches!(
            kind,
            NormalizedListKind::Bullet | NormalizedListKind::Dash | NormalizedListKind::Ordered
        ) || matches!(
            style,
            Some(DefinitionListStyle::Tag | DefinitionListStyle::Hang)
        );
        let default_width = if matches!(
            style,
            Some(DefinitionListStyle::Tag | DefinitionListStyle::Hang)
        ) {
            8
        } else if sized {
            2
        } else {
            0
        };
        let width = node
            .width
            .as_deref()
            .map_or(Distance::cells(default_width), |width| {
                nested_list_distance(width).add(Distance::cells(2)).0
            });
        let head_width_units = width.nonnegative_basic_units();
        Self {
            kind,
            style,
            head_width_units,
            body_width_units: if sized { head_width_units } else { 0 },
            offset_units: node
                .offset
                .as_deref()
                .map_or(0, |offset| nested_list_distance(offset).basic_units()),
            flush_head: kind != NormalizedListKind::Plain
                && !matches!(
                    style,
                    Some(DefinitionListStyle::Inset | DefinitionListStyle::Diagnostic)
                ),
        }
    }
}

fn append_nested_list_head(
    builder: &mut InlineBuilder,
    head: &Node,
    geometry: &NestedListGeometry,
    ordinal: usize,
    name: Option<&str>,
) {
    use libmandoc_rs::{DefinitionListStyle, NormalizedListKind};

    let font = builder.font.checkpoint();
    match geometry.kind {
        NormalizedListKind::Bullet => {
            // termp_it_pre() feeds decoded \[bu] through term_word;
            // append_text accepts a decoded generated word.
            builder.with_font_scope(Font::Strong, |builder| builder.append_text("•"));
        }
        NormalizedListKind::Dash => {
            builder.with_font_scope(Font::Strong, |builder| builder.append_text("-"));
        }
        NormalizedListKind::Ordered => builder.append_text(&format!("{ordinal}.")),
        NormalizedListKind::Plain => {}
        _ if geometry.style == Some(DefinitionListStyle::Diagnostic) => {
            builder.with_font_scope(Font::Strong, |builder| {
                append_inline_nodes(builder, &head.children, name);
            });
        }
        _ => append_inline_nodes(builder, &head.children, name),
    }
    builder.font.pop_scope(font);
}

fn append_nested_list(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    use libmandoc_rs::DefinitionListStyle;

    // Bl BLOCK pre, It BLOCK pre, each HEAD/BODY post, and Bl BLOCK post
    // are native execution phases even when their output belongs to an
    // outer definition term (mdoc_term.c:583-963,1129-1151). Keep the same
    // builder, field buffer, and source-entry dispatcher for all children.
    builder.no_fill_source_line();
    let saved = builder.enter_nested_list_scope(false);
    let geometry = NestedListGeometry::from_node(node);
    let Some(body) = node
        .children
        .iter()
        .find(|part| part.kind == NodeKind::Body)
    else {
        builder.exit_nested_list_scope(saved, false);
        return;
    };
    builder.begin_executed_node(body);
    let mut item_count = 0usize;
    let mut previous_body_empty = false;
    for item in &body.children {
        if item.kind != NodeKind::Block || item.macro_name.as_deref() != Some("It") {
            append_inline_nodes(builder, std::slice::from_ref(item), name);
            continue;
        }
        builder.begin_executed_node(item);
        builder.no_fill_source_line();
        // First nested It reaches the enclosing non-item It ancestor. On
        // later items only bodyless diag suppresses inter-item vspace;
        // compact and negative .sp debt use the shared vertical executor.
        if !(node.compact
            || item_count > 0
                && geometry.style == Some(DefinitionListStyle::Diagnostic)
                && previous_body_empty)
        {
            builder.native_vertical_space(1);
        }
        item_count = item_count.saturating_add(1);
        let head = item
            .children
            .iter()
            .find(|part| part.kind == NodeKind::Head);
        let parts: Vec<_> = item
            .children
            .iter()
            .filter(|part| part.kind == NodeKind::Body)
            .collect();
        let body_empty = parts.iter().all(|part| part.children.is_empty());
        if let Some(head) = head {
            builder.begin_executed_node(head);
            builder.enter_nested_list_head(
                geometry.kind,
                geometry.style,
                geometry.head_width_units,
                geometry.offset_units,
                body_empty,
            );
            append_nested_list_head(builder, head, &geometry, item_count, name);
            builder.finish_nested_list_head(geometry.flush_head);
            builder.restore_nested_list_geometry(saved);
        }
        for part in parts {
            builder.begin_executed_node(part);
            let fixed_cells = match geometry.style {
                Some(DefinitionListStyle::Inset)
                    if head.is_some_and(|head| !head.children.is_empty()) =>
                {
                    1
                }
                Some(DefinitionListStyle::Diagnostic) => 2,
                _ => 0,
            };
            builder.enter_nested_list_body(
                saved,
                geometry.body_width_units,
                geometry.offset_units,
                fixed_cells,
            );
            let posts = builder.scope_posts.clone();
            posts.enter_body(part.id, builder.font.checkpoint());
            append_inline_nodes(builder, &part.children, name);
            if let Some(font) = posts.exit_body(part.id) {
                builder.font.pop_scope(font);
            }
            builder.no_fill_source_line();
            builder.restore_nested_list_geometry(saved);
        }
        previous_body_empty = body_empty;
    }
    builder.no_fill_source_line();
    super::display_tabs::exit_post(&mut builder.execution, node);
    builder.exit_nested_list_scope(saved, item_count > 0);
}

fn append_display(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    // D1/Dl pre and Bd's print_bvspace() both execute term_newln() before
    // changing tab configuration (mdoc_term.c:589,1328,1436). This is the
    // ordinary newline, not roff_pre_br(): temporary BRIND flags survive.
    builder.no_fill_source_line();
    if node.macro_name.as_deref() == Some("Bd") && !node.compact && builder.has_definition_head() {
        // print_bvspace() reaches the enclosing non-item It even when Bd
        // is its first HEAD child (mdoc_term.c:600-618). Its term_vspace()
        // is independent of roff .sp: keep BRIND/NOBREAK and consume any
        // negative vertical-space debt before asserting an output row.
        builder.native_vertical_space(1);
    }
    super::display_tabs::enter_pre(&mut builder.execution, node);
    let Some(body) = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Body && child.scope_end.is_none())
    else {
        return;
    };
    let posts = builder.scope_posts.clone();
    posts.enter_body(body.id, builder.font.checkpoint());
    if node.macro_name.as_deref() == Some("Bd") {
        posts.enter_display_fill(body.id, node.flags.no_fill);
    }
    builder.begin_executed_node(body);
    super::display_tabs::enter_pre(&mut builder.execution, body);
    append_inline_nodes(builder, &body.children, name);
    if let Some(saved) = posts.exit_body(body.id) {
        builder.font.pop_scope(saved);
    }
    if !posts.ended(body.id) {
        // Bd BODY post and D1/Dl BLOCK post call term_newln(), exactly
        // once for the original scope (mdoc_term.c:1131,1482).
        if node.macro_name.as_deref() == Some("Bd") {
            builder.finish_display_body(node.display_kind);
        } else {
            builder.no_fill_source_line();
        }
        posts.finish(body.id);
    }
    if node.macro_name.as_deref() == Some("Bd") {
        posts.exit_display_fill(body.id);
    }
}

struct InlineContainerSink<'a> {
    builder: &'a mut InlineBuilder,
    name: Option<&'a str>,
    root_id: u32,
}

impl<'node> crate::mandoc::containers::ContainerSink<'node> for InlineContainerSink<'_> {
    fn font(&mut self) -> &mut super::FontState {
        &mut self.builder.font
    }

    fn geometry_checkpoint(&self, node: &Node) -> Option<super::DefinitionGeometryCheckpoint> {
        self.builder.definition_geometry_checkpoint(node)
    }

    fn restore_geometry(&mut self, checkpoint: Option<super::DefinitionGeometryCheckpoint>) {
        self.builder.restore_definition_geometry(checkpoint);
    }

    fn restore_fill(&mut self, _fill: bool, marker: &Node) {
        // An explicit .Ed restores the original display at its source
        // marker, before subsequent children execute. Inline execution
        // derives their fill flags from AST nodes; its post still owes the
        // same term_newln() as a normal return (mdoc_term.c:1474-1486).
        // mdoc_endbody_alloc shares the original BODY norm, including its
        // display kind. The marker's post consumes that original scope.
        self.builder.finish_display_body(marker.display_kind);
    }

    fn source_node(&mut self, node: &'node Node, starts_line: bool) {
        // The caller entered the root before scope dispatch.  Direct Fo Fa
        // operands enter in function_argument(); all other nested source
        // wrappers use the same cursor as ordinary text nodes.
        if starts_line && node.id != self.root_id && node.macro_name.as_deref() != Some("Fa") {
            self.builder.begin_executed_node(node);
        }
    }

    fn event(&mut self, event: crate::mandoc::containers::Event<'node>) {
        use crate::mandoc::containers::Event;
        match event {
            Event::BeginNode(node) => self.builder.begin_executed_node(node),
            Event::Anchor(target, source) => {
                self.builder.append(vec![Inline::anchor_at(target, source)]);
            }
            Event::Break => {
                // roff_term_pre_ce()/pre_br() request term_newln(). A HANG
                // field can flush under TERMP_NOBREAK without ending the
                // device row, so execute the field transition first.
                self.builder.control_line_break();
            }
            Event::FlushLine => {
                self.builder
                    .append(vec![Inline::LineBreak { indent_columns: 0 }]);
            }
            Event::Children(nodes) => append_inline_nodes(self.builder, nodes, self.name),
            Event::Glyph(value) => self.builder.append_text(&value),
            Event::Tight => self.builder.tighten_next_boundary(),
            Event::Release => self.builder.release_next_boundary(),
            Event::EmptyWord => self.builder.execute_empty_word(),
            Event::EnterKeep => self.builder.enter_keep_words(),
            Event::ExitKeep => self.builder.exit_keep_words(),
            Event::FunctionArgument(argument, comma_after) => {
                super::generated::function_argument(self.builder, argument, comma_after, self.name);
            }
            _ => unreachable!("the shared container driver owns source and font events"),
        }
    }
}

fn append_quoted_title(builder: &mut InlineBuilder, children: &[Node], name: Option<&str>) {
    // post_rs() sets `norm.Rs.quote_T` after reordering fields. CVS
    // mdoc_html.c::mdoc__x_pre/post encloses only the title field.
    builder.append_text("“");
    builder.tighten_next_boundary();
    append_inline_nodes(builder, children, name);
    builder.tighten_next_boundary();
    builder.append_text("”");
}

/// Atomic syntax owns a semantic wrapper, but its visible components still
/// execute in the caller's formatter stream.  Keep that dispatch separate
/// from ordinary scope lowering so new atomic forms cannot accidentally
/// recreate an isolated zero-advance state.
fn append_atomic(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) -> bool {
    match node.macro_name.as_deref() {
        Some("In") => append_include(builder, node, name),
        Some("Bx") => append_bsd_reference(builder, node, name),
        Some("Lk") => {
            if let Some(anchor) = navigation_anchor(node) {
                builder.append(vec![anchor]);
            }
            append_link(builder, node, name);
        }
        Some("Mt") => {
            if let Some(anchor) = navigation_anchor(node) {
                builder.append(vec![anchor]);
            }
            append_mail_addresses(builder, node, name);
        }
        _ if node.kind == NodeKind::Equation => {
            builder.append(lower_equation_node(node));
        }
        _ => return false,
    }
    true
}

fn section_reference(authored_target: String, children: Vec<Inline>) -> Vec<Inline> {
    if children.is_empty() {
        return children;
    }
    vec![Inline::Link {
        target: mant_ir::LinkTarget::Section {
            id: authored_target.into(),
        },
        title: None,
        children,
    }]
}

fn append_name(builder: &mut InlineBuilder, nodes: &[Node], name: Option<&str>) {
    if nodes.is_empty() {
        if let Some(name) = name {
            builder.append_text(name);
        }
    } else {
        append_inline_nodes(builder, nodes, name);
    }
}
