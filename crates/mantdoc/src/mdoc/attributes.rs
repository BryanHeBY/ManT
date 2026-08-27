use super::{
    BTreeMap, BlockAttributes, DisplayKind, DocumentBuilder, ListWidthRule, MdocListMarker, NodeId,
    NodeKind, NormalizedFont, NormalizedListKind, Recovery, SourceSpan,
};

#[allow(clippy::too_many_lines)] // Mirrors mdoc's ordered list-option validation and recovery.
pub(super) fn list_attributes(
    builder: &DocumentBuilder,
    node: NodeId,
    post_validation_recoveries: &mut Vec<Recovery>,
) -> BlockAttributes {
    let arguments = builder
        .children(node)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let mut attributes = BlockAttributes {
        list_kind: Some(NormalizedListKind::Plain),
        list_type: "item",
        ..BlockAttributes::default()
    };
    // A list without an explicit type defaults to `-item`, which has no
    // normalized width.  Later type switches replace this policy just as
    // libmandoc's final list validator does.
    let mut width_rule = ListWidthRule::Drop;
    let mut selected_type = None::<&str>;
    let mut compact_seen = false;
    let mut offset_seen = false;
    let mut width_seen = false;
    let mut first_type_index = None;
    let mut last_width_argument = None;
    let mut index = 0;
    while let Some(argument) = arguments.get(index).copied() {
        let value = builder.node_text(argument).unwrap_or_default();
        let selected_list_type = matches!(
            value,
            "-bullet"
                | "-dash"
                | "-hyphen"
                | "-enum"
                | "-tag"
                | "-hang"
                | "-diag"
                | "-ohang"
                | "-inset"
                | "-column"
                | "-item"
        );
        let duplicate_list_type = selected_list_type && selected_type.is_some();
        if duplicate_list_type {
            post_validation_recoveries.push(Recovery::DuplicateListType {
                argument: match value {
                    "-bullet" => "-bullet",
                    "-dash" => "-dash",
                    "-hyphen" => "-hyphen",
                    "-enum" => "-enum",
                    "-tag" => "-tag",
                    "-hang" => "-hang",
                    "-diag" => "-diag",
                    "-ohang" => "-ohang",
                    "-inset" => "-inset",
                    "-column" => "-column",
                    "-item" => "-item",
                    _ => unreachable!("selected list type was matched above"),
                },
                location: builder.node_location(node),
            });
        }
        if selected_list_type && !duplicate_list_type {
            selected_type = Some(value);
            first_type_index = Some(index);
            attributes.list_type =
                list_type_name(value).expect("selected list type was matched above");
            attributes.terminal_hanging_list = value == "-hang";
            attributes.terminal_overhanging_list = value == "-ohang";
            attributes.terminal_inset_list = value == "-inset";
            attributes.terminal_diagnostic_list = value == "-diag";
            attributes.list_marker = match value {
                "-bullet" => Some(MdocListMarker::Bullet),
                "-dash" => Some(MdocListMarker::Dash),
                "-hyphen" => Some(MdocListMarker::Hyphen),
                "-enum" => Some(MdocListMarker::Enum),
                _ => None,
            };
        }
        attributes.list_kind = if duplicate_list_type {
            attributes.list_kind
        } else {
            match value {
                "-bullet" | "-dash" | "-hyphen" => {
                    width_rule = ListWidthRule::DefaultTwo;
                    Some(NormalizedListKind::Bullet)
                }
                "-enum" => {
                    width_rule = ListWidthRule::DefaultThree;
                    Some(NormalizedListKind::Ordered)
                }
                "-tag" => {
                    width_rule = ListWidthRule::DefaultSix;
                    Some(NormalizedListKind::Definition)
                }
                "-hang" => {
                    width_rule = ListWidthRule::Retain;
                    Some(NormalizedListKind::Definition)
                }
                "-diag" | "-ohang" | "-inset" => {
                    width_rule = ListWidthRule::Drop;
                    Some(NormalizedListKind::Definition)
                }
                "-column" => {
                    width_rule = ListWidthRule::Drop;
                    Some(NormalizedListKind::Column)
                }
                "-item" => {
                    width_rule = ListWidthRule::Drop;
                    Some(NormalizedListKind::Plain)
                }
                "-compact" => {
                    if compact_seen {
                        post_validation_recoveries.push(Recovery::DuplicateListArgument {
                            argument: "-compact".into(),
                            location: builder.node_location(argument),
                        });
                    }
                    compact_seen = true;
                    attributes.compact = true;
                    attributes.list_kind
                }
                "-offset" | "-width" => {
                    let option = value.trim_start_matches('-');
                    let value_argument = arguments.get(index + 1).copied().filter(|next| {
                        builder
                            .node_text(*next)
                            .is_some_and(|next| !is_list_option(next))
                    });
                    let seen = if value == "-offset" {
                        &mut offset_seen
                    } else {
                        &mut width_seen
                    };
                    if *seen {
                        let display = value_argument
                            .and_then(|next| builder.node_text(next))
                            .map_or_else(|| value.to_owned(), |next| format!("{value} {next}"));
                        post_validation_recoveries.push(Recovery::DuplicateListArgument {
                            argument: display.into_boxed_str(),
                            location: builder.node_location(argument),
                        });
                    }
                    *seen = true;
                    if value == "-width" {
                        last_width_argument = Some(argument);
                    }
                    if let Some(value_argument) = value_argument {
                        let normalized = builder
                            .node_text(value_argument)
                            .map(normalize_mdoc_layout_width);
                        if value == "-offset" {
                            attributes.offset = normalized;
                        } else {
                            attributes.width = normalized;
                        }
                    } else {
                        post_validation_recoveries.push(Recovery::EmptyListLayoutArgument {
                            option: if option == "offset" {
                                "offset"
                            } else {
                                "width"
                            },
                            location: builder.node_location(argument),
                        });
                        if value == "-width" {
                            attributes.width = Some("0n".to_owned());
                        }
                    }
                    attributes.list_kind
                }
                _ if value.starts_with('-') => {
                    post_validation_recoveries.push(Recovery::InvalidArguments {
                        message: format!("skipping excess arguments: Bl ... {value}").into(),
                        location: builder.node_location(argument),
                    });
                    attributes.list_kind
                }
                _ => attributes.list_kind,
            }
        };
        if matches!(value, "-offset" | "-width")
            && arguments.get(index + 1).is_some_and(|next| {
                builder
                    .node_text(*next)
                    .is_some_and(|next| !is_list_option(next))
            })
        {
            index += 1;
        }
        index += 1;
    }
    match first_type_index {
        Some(index) if index > 0 => {
            let first = arguments
                .first()
                .and_then(|argument| builder.node_text(*argument))
                .unwrap_or_default();
            post_validation_recoveries.push(Recovery::ListTypeNotFirst {
                argument: first.to_owned().into_boxed_str(),
                location: builder.node_location(node),
            });
        }
        None => {
            post_validation_recoveries.push(Recovery::MissingListType {
                location: builder.node_location(node),
            });
        }
        Some(_) => {}
    }
    match width_rule {
        ListWidthRule::Drop => {
            if attributes.width.is_some()
                && let Some(width) = last_width_argument
            {
                post_validation_recoveries.push(Recovery::SkippedListWidth {
                    list_type: attributes.list_type,
                    location: builder.node_location(width),
                });
            }
            attributes.width = None;
        }
        ListWidthRule::DefaultTwo if attributes.width.is_none() => {
            attributes.width = Some("2n".to_owned());
        }
        ListWidthRule::DefaultThree if attributes.width.is_none() => {
            attributes.width = Some("3n".to_owned());
        }
        ListWidthRule::DefaultSix if attributes.width.is_none() => {
            post_validation_recoveries.push(Recovery::MissingTagListWidth {
                location: builder.node_location(node),
            });
        }
        ListWidthRule::Retain
        | ListWidthRule::DefaultTwo
        | ListWidthRule::DefaultThree
        | ListWidthRule::DefaultSix => {}
    }
    if attributes.list_type == "column" {
        attributes.column_widths = column_declarations(builder, &arguments);
        attributes.column_count = Some(attributes.column_widths.len());
    }
    attributes
}

/// Retain the declaration phrases that mdoc associates with a column list.
///
/// libmandoc accepts further column labels after no-argument list options
/// such as `-compact`; only the single payload of `-width` and `-offset` is
/// excluded from the declaration.  Keeping this separate from generic option
/// validation avoids losing those labels when the public list Head is dropped.
pub(super) fn column_declarations(builder: &DocumentBuilder, arguments: &[NodeId]) -> Vec<String> {
    let mut declarations = Vec::new();
    let mut index = 0_usize;
    while let Some(argument) = arguments.get(index).copied() {
        let value = builder.node_text(argument).unwrap_or_default();
        if matches!(value, "-width" | "-offset") {
            index += 2;
            continue;
        }
        if !value.starts_with('-') || builder.node_argument_quoted(argument) {
            declarations.push(value.to_owned());
        }
        index += 1;
    }
    declarations
}

pub(super) fn is_list_option(value: &str) -> bool {
    matches!(
        value,
        "-bullet"
            | "-dash"
            | "-hyphen"
            | "-enum"
            | "-tag"
            | "-hang"
            | "-diag"
            | "-ohang"
            | "-inset"
            | "-column"
            | "-item"
            | "-compact"
            | "-offset"
            | "-width"
    )
}

pub(super) fn list_type_name(value: &str) -> Option<&'static str> {
    match value {
        "-bullet" => Some("bullet"),
        "-dash" => Some("dash"),
        "-hyphen" => Some("hyphen"),
        "-enum" => Some("enum"),
        "-tag" => Some("tag"),
        "-hang" => Some("hang"),
        "-diag" => Some("diag"),
        "-ohang" => Some("ohang"),
        "-inset" => Some("inset"),
        "-column" => Some("column"),
        "-item" => Some("item"),
        _ => None,
    }
}

/// mdoc normalizes macro names in `-width` and `-offset` to the fixed
/// terminal-cell width assigned by `mdoc_validate.c`; this is a normalized
/// public field, while layout-option tokens are structural input syntax.
pub(super) fn normalize_mdoc_layout_width(value: &str) -> String {
    let width = match value {
        "Ad" | "Ao" | "An" | "Aq" | "Ar" | "Bo" | "Bq" | "Cd" | "Dq" | "Dv" | "Eo" | "Fa"
        | "No" | "Pf" | "Po" | "Pq" | "Qo" | "So" | "Sq" | "Va" | "Vt" => Some(12),
        "Cm" | "Do" | "Em" | "Fl" | "Ic" | "Nm" | "Oo" | "Tn" | "Xr" => Some(10),
        "Er" => Some(17),
        "Ev" => Some(15),
        "Fo" | "Fn" | "Li" | "Ql" | "Sx" => Some(16),
        "Ds" | "Ms" | "Sy" => Some(6),
        "Op" => Some(14),
        "Pa" => Some(32),
        _ => None,
    };
    width.map_or_else(|| value.to_owned(), |width| format!("{width}n"))
}

pub(super) fn display_attributes(
    builder: &DocumentBuilder,
    node: NodeId,
    immediate_recoveries: &mut Vec<Recovery>,
    post_validation_recoveries: &mut Vec<Recovery>,
) -> BlockAttributes {
    let arguments = builder
        .children(node)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let mut attributes = BlockAttributes {
        ..BlockAttributes::default()
    };
    let mut index = 0;
    while let Some(argument) = arguments.get(index).copied() {
        let value = builder.node_text(argument).unwrap_or_default();
        match value {
            "-literal" | "-unfilled" | "-filled" | "-ragged" | "-centered" => {
                let display_kind = match value {
                    "-literal" | "-unfilled" => DisplayKind::Literal,
                    "-filled" | "-ragged" | "-centered" => DisplayKind::Filled,
                    _ => unreachable!("the display option was matched above"),
                };
                if attributes.display_kind.is_some() {
                    post_validation_recoveries.push(Recovery::DuplicateDisplayType {
                        argument: match value {
                            "-literal" => "literal",
                            "-unfilled" => "unfilled",
                            "-filled" => "filled",
                            "-ragged" => "ragged",
                            "-centered" => "centered",
                            _ => unreachable!("the display option was matched above"),
                        },
                        location: builder.node_location(node),
                    });
                } else {
                    attributes.display_kind = Some(display_kind);
                    attributes.literal_display = value == "-literal";
                    attributes.centered_display = value == "-centered";
                }
            }
            "-compact" => {
                if attributes.compact {
                    post_validation_recoveries.push(Recovery::DuplicateDisplayArgument {
                        argument: "-compact".into(),
                        location: builder.node_location(argument),
                    });
                }
                attributes.compact = true;
            }
            "-offset" => {
                let value = arguments
                    .get(index.saturating_add(1))
                    .and_then(|next| builder.node_text(*next))
                    .filter(|next| is_display_offset_value(next));
                let (offset, consumed) = if let Some(value) = value {
                    (Some(normalize_mdoc_layout_width(value)), true)
                } else {
                    post_validation_recoveries.push(Recovery::EmptyDisplayOffset {
                        location: builder.node_location(argument),
                    });
                    (None, false)
                };
                if let Some(offset) = offset {
                    if attributes.offset.is_some() {
                        post_validation_recoveries.push(Recovery::DuplicateDisplayArgument {
                            argument: format!("-offset {offset}").into(),
                            location: builder.node_location(argument),
                        });
                    }
                    attributes.offset = Some(offset);
                }
                index += usize::from(consumed);
            }
            "-file" => {
                post_validation_recoveries.push(Recovery::UnsupportedDisplayFile {
                    location: builder.node_location(node),
                });
                if arguments.get(index.saturating_add(1)).is_some() {
                    index += 1;
                }
            }
            _ if value.starts_with('-') => {
                immediate_recoveries.push(Recovery::InvalidArguments {
                    message: format!("skipping excess arguments: Bd ... {value}").into(),
                    location: builder.node_location(argument),
                });
                break;
            }
            _ => {}
        }
        index += 1;
    }
    if attributes.display_kind.is_none() {
        post_validation_recoveries.push(Recovery::MissingDisplayType {
            location: builder.node_location(node),
        });
        attributes.display_kind = Some(DisplayKind::Filled);
    }
    attributes
}

/// `-offset` accepts ordinary layout widths and signed numeric widths, but a
/// following named display option still starts a fresh option rather than
/// becoming its value.
pub(super) fn is_display_offset_value(value: &str) -> bool {
    !value.starts_with('-') || value.as_bytes().get(1).is_some_and(u8::is_ascii_digit)
}

pub(super) fn font_attributes(
    builder: &DocumentBuilder,
    node: NodeId,
    post_validation_recoveries: &mut Vec<Recovery>,
) -> BlockAttributes {
    let Some(first) = builder
        .children(node)
        .and_then(|arguments| arguments.first())
        .copied()
    else {
        post_validation_recoveries.push(Recovery::MissingFontType {
            location: builder.node_location(node),
        });
        return BlockAttributes::default();
    };
    let value = builder.node_text(first).unwrap_or_default();
    let arguments = builder.children(node).unwrap_or_default();
    let option_form = is_bf_option(value);
    let font = match value {
        "-emphasis" | "Em" => Some(NormalizedFont::Emphasis),
        "-literal" | "Li" => Some(NormalizedFont::Literal),
        "-symbolic" | "Sy" => Some(NormalizedFont::Symbolic),
        _ => {
            post_validation_recoveries.push(Recovery::UnknownFontType {
                argument: value.into(),
                location: builder.node_location(first),
            });
            None
        }
    };
    let excess = if option_form {
        arguments[1..]
            .iter()
            .copied()
            .find(|argument| !builder.node_text(*argument).is_some_and(is_bf_option))
    } else {
        arguments.get(1).copied()
    };
    if let Some(excess) = excess {
        post_validation_recoveries.push(Recovery::InvalidArguments {
            message: format!(
                "skipping excess arguments: Bf ... {}",
                builder.node_text(excess).unwrap_or_default()
            )
            .into(),
            location: builder.node_location(excess),
        });
    }
    BlockAttributes {
        font,
        ..BlockAttributes::default()
    }
}

pub(super) fn is_bf_option(value: &str) -> bool {
    matches!(value, "-emphasis" | "-literal" | "-symbolic")
}

pub(super) fn apply_attributes(
    builder: &mut DocumentBuilder,
    nodes: &[NodeId],
    attributes: &BlockAttributes,
) {
    for node in nodes {
        let _ = builder.set_node_list_kind(*node, attributes.list_kind);
        let _ = builder.set_node_list_marker(*node, attributes.list_marker);
        let _ = builder.set_node_column_widths(*node, attributes.column_widths.clone());
        let _ = builder.set_node_terminal_hanging_list(*node, attributes.terminal_hanging_list);
        let _ =
            builder.set_node_terminal_overhanging_list(*node, attributes.terminal_overhanging_list);
        let _ = builder.set_node_terminal_inset_list(*node, attributes.terminal_inset_list);
        let _ =
            builder.set_node_terminal_diagnostic_list(*node, attributes.terminal_diagnostic_list);
        let _ = builder.set_node_display_kind(*node, attributes.display_kind);
        let _ = builder.set_node_literal_display(*node, attributes.literal_display);
        let _ = builder.set_node_centered_display(*node, attributes.centered_display);
        let _ = builder.set_node_font(*node, attributes.font);
        let _ = builder.set_node_compact(*node, attributes.compact);
        if let Some(offset) = &attributes.offset {
            let _ = builder.set_node_offset(*node, offset.clone());
        }
        if let Some(width) = &attributes.width {
            let _ = builder.set_node_width(*node, width.clone());
        }
    }
}

pub(super) fn mark_subtree_no_fill(builder: &mut DocumentBuilder, root: NodeId) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if let Some(mut flags) = builder.node_flags(node) {
            flags.no_fill = true;
            let _ = builder.set_node_flags(node, flags);
        }
        // In mdoc literal displays, horizontal whitespace at the physical
        // line end is not part of the public text.  A whitespace-only line
        // therefore remains observable as an empty text node, while leading
        // indentation before a glyph is retained in the public AST.
        if builder.node_kind(node) == Some(NodeKind::Text)
            && let Some(normalized) = builder.node_text(node).and_then(|text| {
                let normalized = text.trim_end_matches([' ', '\t']);
                (normalized.len() != text.len()).then(|| normalized.to_owned())
            })
        {
            let _ = builder.text(node, normalized);
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().copied());
        }
    }
}

/// mdoc 填充文本不将物理行末空白发布到公开 AST；literal display 由其专用路径处理。
pub(super) fn trim_mdoc_filled_text_trailing_whitespace(
    builder: &mut DocumentBuilder,
    flat: &[NodeId],
) {
    for node in flat {
        if builder.node_kind(*node) != Some(NodeKind::Text)
            || builder.node_flags(*node).is_some_and(|flags| flags.no_fill)
        {
            continue;
        }
        let Some(normalized) = builder.node_text(*node).and_then(|text| {
            let normalized = text.trim_end_matches([' ', '\t']);
            (normalized.len() != text.len()).then(|| normalized.to_owned())
        }) else {
            continue;
        };
        let _ = builder.text(*node, normalized);
    }
}

/// Project the source-order `.nf`/`.fi` presentation state before structural
/// mdoc lowering moves scanner events under package blocks.  A display block
/// is its own fill-state boundary: `-unfilled` starts no-fill, `.fi` can turn
/// it off, and `.Ed` restores the state that preceded the display.  The
/// controlling request itself keeps its incoming state, while only `.nf`
/// arguments observe the new state.
pub(super) fn apply_presentation_flags(builder: &mut DocumentBuilder, flat: &[NodeId]) {
    let mut no_fill = false;
    let mut display_fill_restore = Vec::new();
    for node in flat {
        match builder.node_macro_name(*node) {
            Some("Bd") => {
                if no_fill {
                    mark_subtree_no_fill(builder, *node);
                }
                display_fill_restore.push(no_fill);
                no_fill = display_is_unfilled(builder, *node);
            }
            Some("Ed") => {
                if no_fill {
                    mark_subtree_no_fill(builder, *node);
                }
                if let Some(previous) = display_fill_restore.pop() {
                    no_fill = previous;
                }
            }
            Some("nf" | "fi") => {
                if no_fill {
                    mark_node_no_fill(builder, *node);
                }
                no_fill = builder.node_macro_name(*node) == Some("nf");
                if no_fill {
                    mark_children_no_fill(builder, *node);
                }
            }
            _ if no_fill => mark_subtree_no_fill(builder, *node),
            _ => {}
        }
    }
}

/// The first recognized display type owns fill state; later type options are
/// validation errors and cannot change the already selected public display.
pub(super) fn display_is_unfilled(builder: &DocumentBuilder, node: NodeId) -> bool {
    for argument in builder.children(node).into_iter().flatten() {
        match builder.node_text(*argument) {
            Some("-literal" | "-unfilled") => return true,
            Some("-filled" | "-ragged" | "-centered") => return false,
            _ => {}
        }
    }
    false
}

/// In filled mdoc input, a terminal `\c` followed by a blank physical line
/// recovers to ordinary text and omits the blank source event.  Literal
/// displays retain both nodes; their scanner flags are already established by
/// [`apply_presentation_flags`] when this runs.
pub(super) fn suppress_filled_c_blank_lines(
    builder: &mut DocumentBuilder,
    flat: &[NodeId],
) -> Vec<NodeId> {
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
        if builder.set_node_text(*text, value) {
            flags.line_continuation = false;
            let _ = builder.set_node_flags(*text, flags);
            suppressed.push(*blank);
        }
    }
    suppressed
}

/// Normalize physical blank source events to the semantic vertical-space
/// request used by mdoc in fill mode.  The scanner retains empty text so it
/// can preserve exact source positions; changing the existing arena record
/// rather than allocating a synthetic node retains that provenance and keeps
/// source-order validation deterministic.
pub(super) fn normalize_filled_blank_lines(
    builder: &mut DocumentBuilder,
    flat: &[NodeId],
    suppressed: &[NodeId],
) -> BTreeMap<NodeId, Recovery> {
    let mut recoveries = BTreeMap::new();
    for node in flat {
        if suppressed.contains(node)
            || builder.node_kind(*node) != Some(NodeKind::Text)
            || builder.node_text(*node) != Some("")
            || builder.node_flags(*node).is_none_or(|flags| flags.no_fill)
        {
            continue;
        }
        let location = blank_line_location(builder, *node);
        if builder.set_node_kind(*node, NodeKind::Element)
            && builder.macro_name(*node, "sp")
            && builder.clear_node_text(*node)
        {
            recoveries.insert(*node, Recovery::FilledBlankLine { location });
        }
    }
    recoveries
}

/// The scanner usually stores a blank physical line at its first source byte.
/// An execution-stage recovery may refine that position to the escape which
/// produced the semantic blank; retain that logical provenance when present.
pub(super) fn blank_line_location(builder: &DocumentBuilder, node: NodeId) -> Option<SourceSpan> {
    let mut location = builder.node_location(node)?;
    let position = builder.node_source_position(node)?;
    location.logical_start = Some(crate::SourcePosition {
        line: position.line,
        column: position.column,
    });
    Some(location)
}

pub(super) fn mark_children_no_fill(builder: &mut DocumentBuilder, root: NodeId) {
    let Some(children) = builder.children(root).map(<[NodeId]>::to_vec) else {
        return;
    };
    for child in children {
        mark_subtree_no_fill(builder, child);
    }
}

pub(super) fn mark_node_no_fill(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(mut flags) = builder.node_flags(node) else {
        return;
    };
    flags.no_fill = true;
    let _ = builder.set_node_flags(node, flags);
}
