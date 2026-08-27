use super::{
    DocumentBuilder, NodeFlags, NodeId, NodeKind, Recovery, SourceSpan, StructureOutcome,
    is_mdoc_closing_delimiter,
};

/// mdoc synthesizes the document name for an empty `.Nm`. This is an
/// AST-visible generated word, not a renderer convenience.
pub(super) fn insert_generated_nm_name(
    builder: &mut DocumentBuilder,
    source: NodeId,
    head: NodeId,
    max_nodes: usize,
) -> bool {
    if builder.node_count() >= max_nodes {
        return false;
    }
    let Some(name) = builder.metadata_mut().name.clone() else {
        return true;
    };
    let Some(text) = builder.push(head, NodeKind::Text) else {
        return false;
    };
    if !builder.text(text, name) || !builder.set_node_location(text, builder.node_location(source))
    {
        return false;
    }
    let Some(mut flags) = builder.node_flags(text) else {
        return false;
    };
    flags.generated = true;
    builder.set_node_flags(text, flags)
}

/// An empty `.Ar` exposes mdoc's generated default argument words.  They are
/// separate nodes in the owned AST (rather than one renderer-only string), so
/// canonical consumers can preserve their generated provenance.
pub(super) fn insert_generated_ar_default(
    builder: &mut DocumentBuilder,
    source: NodeId,
    parent: NodeId,
    max_nodes: usize,
) -> bool {
    const DEFAULT_WORDS: [&str; 2] = ["file", "..."];
    if builder.node_count().saturating_add(DEFAULT_WORDS.len()) > max_nodes {
        return false;
    }
    let synopsis_pretty = builder
        .node_flags(source)
        .is_some_and(|flags| flags.synopsis_pretty);
    let location = builder.node_location(source);
    for word in DEFAULT_WORDS {
        let Some(text) = builder.push(parent, NodeKind::Text) else {
            return false;
        };
        let flags = NodeFlags {
            generated: true,
            synopsis_pretty,
            ..NodeFlags::default()
        };
        if !builder.text(text, word)
            || !builder.set_node_location(text, location.clone())
            || !builder.set_node_flags(text, flags)
        {
            return false;
        }
    }
    true
}

/// Empty `.Mt` and `.Pa` elements use mandoc's generated nonbreaking-space
/// placeholder so following punctuation remains separated from prior prose.
pub(super) fn insert_generated_nonbreaking_default(
    builder: &mut DocumentBuilder,
    source: NodeId,
    max_nodes: usize,
) -> bool {
    if builder.node_count() >= max_nodes {
        return false;
    }
    let Some(text) = push_generated_text(builder, source, "~", false) else {
        return false;
    };
    if builder
        .node_flags(source)
        .is_some_and(|flags| flags.synopsis_pretty)
        && let Some(mut flags) = builder.node_flags(text)
    {
        flags.synopsis_pretty = true;
        return builder.set_node_flags(text, flags);
    }
    true
}

/// Apply compact-system-name generation to a parsed inline event list.  This
/// is needed after a partial block's Body re-enters inline parsing, which
/// bypasses the top-level source-order dispatcher.
pub(super) fn insert_generated_system_names(
    builder: &mut DocumentBuilder,
    events: &[NodeId],
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) {
    for event in events {
        let Some(macro_name) = builder.node_macro_name(*event).map(str::to_owned) else {
            continue;
        };
        if !insert_generated_system_name(builder, *event, &macro_name, max_nodes)
            && outcome.node_limit_location.is_none()
        {
            outcome.node_limit_location = builder.node_location(*event);
        }
    }
}

/// Allocate the default operating-system word published by mdoc's compact
/// system-name macros.  The generated child remains distinct from any
/// authored version/name argument, matching the legacy owned AST rather than
/// deferring the spelling to a renderer.
pub(super) fn insert_generated_system_name(
    builder: &mut DocumentBuilder,
    source: NodeId,
    macro_name: &str,
    max_nodes: usize,
) -> bool {
    if macro_name == "Bx" {
        return insert_generated_bx(builder, source, max_nodes);
    }
    let Some(name) = generated_system_name(macro_name) else {
        return true;
    };
    if builder.node_count() >= max_nodes {
        return false;
    }
    let existing_children = builder
        .children(source)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let Some(text) = builder.push(source, NodeKind::Text) else {
        return false;
    };
    let flags = NodeFlags {
        generated: true,
        ..NodeFlags::default()
    };
    if !(builder.text(text, name)
        && builder.set_node_location(text, builder.node_location(source))
        && builder.set_node_flags(text, flags))
    {
        return false;
    }
    // `post_xx()` constructs the generated operating-system word before an
    // optional authored version.
    let mut children = Vec::with_capacity(existing_children.len() + 1);
    children.push(text);
    children.extend(existing_children);
    builder.replace_children(source, &children)
}

/// Mirror mdoc's specialised `post_bx()` validation.
///
/// Unlike the other compact system-name macros, `Bx` makes its authored
/// version and the generated `BSD` word adjoining words, and a second
/// argument forms the generated `-` separator plus a title-cased BSD variant.
/// Keep these as distinct generated AST nodes: renderers and canonical
/// differential tests both consume their topology and provenance.
pub(super) fn insert_generated_bx(
    builder: &mut DocumentBuilder,
    source: NodeId,
    max_nodes: usize,
) -> bool {
    let existing_children = builder
        .children(source)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    // List-column restructuring may call the common system-name helper after
    // its body has already been normalised. Never publish a second synthetic
    // BSD sequence when that happens.
    if existing_children.iter().any(|child| {
        builder.node_text(*child) == Some("BSD")
            && builder
                .node_flags(*child)
                .is_some_and(|flags| flags.generated)
    }) {
        return true;
    }

    let additional_nodes = match existing_children.len() {
        0 => 1,
        1 => 2,
        _ => 5,
    };
    if builder.node_count().saturating_add(additional_nodes) > max_nodes {
        return false;
    }
    let location = builder.node_location(source);
    let Some(bsd) = push_generated_text_at(builder, source, "BSD", false, location.clone()) else {
        return false;
    };
    if existing_children.is_empty() {
        return builder.replace_children(source, &[bsd]);
    }

    let Some(before_bsd) = push_generated_element(builder, source, "Ns", location.clone()) else {
        return false;
    };
    let mut children = Vec::with_capacity(existing_children.len().saturating_add(5));
    children.push(existing_children[0]);
    children.push(before_bsd);
    children.push(bsd);

    if let Some(second_argument) = existing_children.get(1).copied() {
        let Some(before_dash) = push_generated_element(builder, source, "Ns", location.clone())
        else {
            return false;
        };
        let Some(dash) = push_generated_text_at(builder, source, "-", false, location.clone())
        else {
            return false;
        };
        let Some(before_variant) = push_generated_element(builder, source, "Ns", location) else {
            return false;
        };
        if let Some(value) = builder.node_text(second_argument) {
            let mut title_cased = value.as_bytes().to_vec();
            if let Some(first) = title_cased.first_mut() {
                first.make_ascii_uppercase();
            }
            let Ok(title_cased) = String::from_utf8(title_cased) else {
                return false;
            };
            if !builder.text(second_argument, title_cased) {
                return false;
            }
        }
        children.extend([before_dash, dash, before_variant, second_argument]);
        // The lexer currently exposes at most two Bx arguments, but retaining
        // a future scanner extension's tail is safer than silently dropping
        // user syntax before its own validator can classify it.
        children.extend(existing_children.into_iter().skip(2));
    }
    builder.replace_children(source, &children)
}

/// Match `append_delims()`'s quoted-delimiter EOS suppression after `.Bx`.
pub(super) fn clear_quoted_bx_trailing_delimiter_sentence_end(
    builder: &mut DocumentBuilder,
    candidate: Option<NodeId>,
) {
    let Some(candidate) = candidate else {
        return;
    };
    if !builder.node_argument_quoted(candidate)
        || !builder
            .node_text(candidate)
            .is_some_and(is_mdoc_closing_delimiter)
    {
        return;
    }
    let Some(mut flags) = builder.node_flags(candidate) else {
        return;
    };
    flags.sentence_end = false;
    let _ = builder.set_node_flags(candidate, flags);
}

/// A run of compact system-name requests remains a run of source elements
/// while its words are separated by ordinary whitespace.  A tab immediately
/// following the next spelling changes the legacy `Bl -column` parser state:
/// that spelling is then the current system macro's optional argument rather
/// than a new request.
pub(super) fn column_system_name_starts_next_element(
    builder: &DocumentBuilder,
    current: NodeId,
    next: NodeId,
) -> bool {
    builder
        .node_macro_name(current)
        .is_some_and(|name| generated_system_name(name).is_some())
        && builder
            .node_text(next)
            .is_some_and(|text| generated_system_name(text).is_some())
        && builder.node_separator_after(next) != Some(b'\t')
}

/// Expand mdoc's standard exit-status sentence.  A missing `-std` is a
/// recoverable validator omission: mandoc adds it, keeps authored words as
/// utility names, and publishes the normal generated sentence.
pub(super) fn expand_standard_exit_status(
    builder: &mut DocumentBuilder,
    source: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> bool {
    let Some(arguments) = builder.children(source).map(<[NodeId]>::to_vec) else {
        return true;
    };
    let names = if arguments
        .first()
        .is_some_and(|first| builder.node_text(*first) == Some("-std"))
    {
        &arguments[1..]
    } else {
        outcome.recoveries.push(Recovery::MissingStandardSelector {
            macro_name: "Ex",
            location: builder.node_location(source),
        });
        &arguments[..]
    };
    let generated_name_nodes = if names.is_empty() { 2 } else { names.len() };
    let required_nodes = 3_usize
        .saturating_add(generated_name_nodes)
        .saturating_add(names.len().saturating_sub(1));
    if builder.node_count().saturating_add(required_nodes) > max_nodes {
        return false;
    }

    let mut children = Vec::with_capacity(required_nodes);
    let Some(the) = push_generated_text(builder, source, "The", false) else {
        return false;
    };
    children.push(the);

    if names.is_empty() {
        if let Some(name) = builder.metadata_mut().name.clone() {
            let Some(name_element) = push_generated_element(builder, source, "Nm", None) else {
                return false;
            };
            let Some(name_text) = push_generated_text(builder, name_element, &name, false) else {
                return false;
            };
            if !builder.replace_children(name_element, &[name_text]) {
                return false;
            }
            children.push(name_element);
        } else {
            outcome.recoveries.push(Recovery::MissingExitName {
                location: builder.node_location(source),
            });
        }
    } else {
        for (index, name) in names.iter().enumerate() {
            if index > 0 {
                let separator = if index + 1 == names.len() { "and" } else { "," };
                let separator_location = if index + 1 == names.len() {
                    builder.node_location(*name)
                } else {
                    builder.node_location(names[index - 1])
                };
                let Some(separator) =
                    push_generated_text_at(builder, source, separator, false, separator_location)
                else {
                    return false;
                };
                children.push(separator);
            }
            let Some(name_element) =
                push_generated_element(builder, source, "Nm", builder.node_location(*name))
            else {
                return false;
            };
            if !builder.replace_children(name_element, &[*name]) {
                return false;
            }
            children.push(name_element);
        }
    }

    let utility_word = if names.len() > 1 {
        "utilities exit\\~0"
    } else {
        "utility exits\\~0"
    };
    let Some(result) = push_generated_text(builder, source, utility_word, false) else {
        return false;
    };
    children.push(result);
    let Some(outcome) = push_generated_text(
        builder,
        source,
        "on success, and\\~>0 if an error occurs.",
        true,
    ) else {
        return false;
    };
    children.push(outcome);
    builder.replace_children(source, &children)
}

/// Expand mdoc's standard return-value sentence.  A missing `-std` uses the
/// same recoverable defaulting rule as `Ex`; named entries become generated
/// `Fn` elements and the no-name form keeps its alternate introduction.
#[allow(clippy::too_many_lines)] // The two grammar-selected sentence forms share bounded allocation and source-order rules.
pub(super) fn expand_standard_return_value(
    builder: &mut DocumentBuilder,
    source: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> bool {
    let Some(arguments) = builder.children(source).map(<[NodeId]>::to_vec) else {
        return true;
    };
    let names = if arguments
        .first()
        .is_some_and(|first| builder.node_text(*first) == Some("-std"))
    {
        &arguments[1..]
    } else {
        outcome.recoveries.push(Recovery::MissingStandardSelector {
            macro_name: "Rv",
            location: builder.node_location(source),
        });
        &arguments[..]
    };
    let required_nodes = if names.is_empty() {
        5
    } else {
        7_usize
            .saturating_add(names.len())
            .saturating_add(names.len().saturating_sub(1))
    };
    if builder.node_count().saturating_add(required_nodes) > max_nodes {
        return false;
    }

    let mut children = Vec::with_capacity(required_nodes.saturating_sub(1));
    if names.is_empty() {
        let Some(success) = push_generated_text(
            builder,
            source,
            "Upon successful completion, the value\\~0 is returned;",
            false,
        ) else {
            return false;
        };
        children.push(success);
    } else {
        let Some(the) = push_generated_text(builder, source, "The", false) else {
            return false;
        };
        children.push(the);
        for (index, name) in names.iter().enumerate() {
            if index > 0 {
                let separator = if index + 1 == names.len() { "and" } else { "," };
                let separator_location = if index + 1 == names.len() {
                    builder.node_location(*name)
                } else {
                    builder.node_location(names[index - 1])
                };
                let Some(separator) =
                    push_generated_text_at(builder, source, separator, false, separator_location)
                else {
                    return false;
                };
                children.push(separator);
            }
            let Some(function) =
                push_generated_element(builder, source, "Fn", builder.node_location(*name))
            else {
                return false;
            };
            if !builder.replace_children(function, &[*name]) {
                return false;
            }
            children.push(function);
        }
        let returns = if names.len() > 1 {
            "functions return"
        } else {
            "function returns"
        };
        let Some(returns) = push_generated_text(builder, source, returns, false) else {
            return false;
        };
        children.push(returns);
        let Some(success) =
            push_generated_text(builder, source, "the value\\~0 if successful;", false)
        else {
            return false;
        };
        children.push(success);
    }

    let Some(otherwise) = push_generated_text(
        builder,
        source,
        "otherwise the value\\~\\-1 is returned and the global variable",
        false,
    ) else {
        return false;
    };
    children.push(otherwise);
    let Some(errno) = push_generated_element(builder, source, "Va", None) else {
        return false;
    };
    let Some(errno_text) = push_generated_text(builder, errno, "errno", false) else {
        return false;
    };
    if !builder.replace_children(errno, &[errno_text]) {
        return false;
    }
    children.push(errno);
    let Some(final_clause) =
        push_generated_text(builder, source, "is set to indicate the error.", true)
    else {
        return false;
    };
    children.push(final_clause);
    builder.replace_children(source, &children)
}

/// Allocate a generated text node at an mdoc macro's source location.
pub(super) fn push_generated_text(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    value: &str,
    sentence_end: bool,
) -> Option<NodeId> {
    push_generated_text_at(builder, parent, value, sentence_end, None)
}

/// Allocate a generated text node, optionally retaining the source position
/// of an authored list word that selected its generated connector.
pub(super) fn push_generated_text_at(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    value: &str,
    sentence_end: bool,
    location: Option<SourceSpan>,
) -> Option<NodeId> {
    let text = builder.push(parent, NodeKind::Text)?;
    let flags = NodeFlags {
        generated: true,
        sentence_end,
        ..NodeFlags::default()
    };
    (builder.text(text, value)
        && builder.set_node_location(text, location.or_else(|| builder.node_location(parent)))
        && builder.set_node_flags(text, flags))
    .then_some(text)
}

/// Allocate a generated, source-less-in-meaning element while retaining the
/// legacy source position used by mdoc's generated node projection.
pub(super) fn push_generated_element(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    macro_name: &str,
    location: Option<SourceSpan>,
) -> Option<NodeId> {
    let element = builder.push(parent, NodeKind::Element)?;
    let flags = NodeFlags {
        generated: true,
        ..NodeFlags::default()
    };
    (builder.macro_name(element, macro_name)
        && builder.set_node_location(element, location.or_else(|| builder.node_location(parent)))
        && builder.set_node_flags(element, flags))
    .then_some(element)
}

pub(super) fn is_legacy_roff_font(font: &[u8]) -> bool {
    matches!(
        font,
        b"C" | b"V"
            | b"B"
            | b"3"
            | b"I"
            | b"2"
            | b"P"
            | b"R"
            | b"1"
            | b"4"
            | b"BI"
            | b"CB"
            | b"CI"
            | b"CR"
            | b"CW"
            | b"VB"
            | b"VI"
    )
}

/// The compact system-name macros whose validator inserts a default word.
/// `Bx` uses the same generated-child rule when no BSD variant is authored;
/// its richer two-argument rewriting is retained as a separate follow-up.
pub(super) fn generated_system_name(macro_name: &str) -> Option<&'static str> {
    match macro_name {
        "Bsx" => Some("BSD/OS"),
        "Bx" => Some("BSD"),
        "Dx" => Some("DragonFly"),
        "Fx" => Some("FreeBSD"),
        "Nx" => Some("NetBSD"),
        "Ox" => Some("OpenBSD"),
        "Ux" => Some("UNIX"),
        _ => None,
    }
}

/// Reborrow a recognized compact system-name spelling as the static recovery
/// label required by the public diagnostic contract.
pub(super) fn system_macro_name(macro_name: &str) -> &'static str {
    match macro_name {
        "Bsx" => "Bsx",
        "Bx" => "Bx",
        "Dx" => "Dx",
        "Fx" => "Fx",
        "Nx" => "Nx",
        "Ox" => "Ox",
        "Ux" => "Ux",
        _ => unreachable!("only compact system-name macros reach this helper"),
    }
}
