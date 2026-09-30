use super::{Font, Inline};

pub(super) fn flush_segment(
    output: &mut Vec<Inline>,
    buffer: &mut String,
    font: Font,
    link: Option<&str>,
) {
    if buffer.is_empty() {
        return;
    }
    let value = std::mem::take(buffer);
    output.push(styled_link(value, font, link));
}

pub(super) fn styled_link(value: String, font: Font, link: Option<&str>) -> Inline {
    let styled = styled_segment(value, font);
    if let Some(target) = link {
        Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: target.to_owned(),
            },
            title: None,
            children: vec![styled],
        }
    } else {
        styled
    }
}

pub(in crate::mandoc::inline) fn styled_segment(value: String, font: Font) -> Inline {
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

/// Keep a generated prefix and equally styled operands in a single visible
/// run, without wrapping font overrides in an additional, additive style.
pub(in crate::mandoc::inline) fn coalesce_font_runs(nodes: Vec<Inline>) -> Vec<Inline> {
    let mut output: Vec<Inline> = Vec::new();
    let mut owners = Vec::new();
    for node in nodes {
        if matches!(&node, Inline::Anchor { id, .. }
            if id.as_str().starts_with(super::super::flow::INTERNAL_FIELD_WORD))
        {
            owners.push(node);
            continue;
        }
        match (output.last_mut(), node) {
            (Some(Inline::Strong { children: previous }), Inline::Strong { mut children })
            | (Some(Inline::Emphasis { children: previous }), Inline::Emphasis { mut children }) => {
                // termp_fl_pre() keeps its generated prefix and children in
                // one font run. Private field owners are transparent to this
                // requested coalescing, but stay in source order inside the
                // run until native acceptance and rejection are complete.
                previous.append(&mut owners);
                previous.append(&mut children);
            }
            (Some(Inline::Text { value: previous }), Inline::Text { value })
                if owners.is_empty() =>
            {
                previous.push_str(&value);
            }
            (_, node) => {
                output.append(&mut owners);
                output.push(node);
            }
        }
    }
    output.append(&mut owners);
    output
}
