//! Native man list marker evidence and adjacent IP list runs.

use super::super::{
    Block, DefinitionItem, ListKind, LoweringContext, Node, NodeKind, block_indent,
    first_part_children, layout_with_spacing, plain_text,
};
use libmandoc_rs::{MacroToken::Man, ManMacro};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IpMark {
    Star,
    Dash,
    NamedBullet,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc::blocks) struct IpRun {
    kind: ListKind,
    continues: bool,
}

/// CVS `man_html.c::list_continues()` inspects the first raw HEAD child of
/// adjacent IP blocks before deciding whether to open a UL. A single IP, a
/// mixed pair, or a styled mark remains a definition with its authored term.
pub(in crate::mandoc::blocks) fn adjacent_ip_run(nodes: &[Node], index: usize) -> Option<IpRun> {
    fn mark(node: &Node) -> Option<IpMark> {
        if node.kind != NodeKind::Block || node.macro_token.as_ref() != Some(&Man(ManMacro::Ip)) {
            return None;
        }
        match first_part_children(node, NodeKind::Head)
            .first()
            .and_then(|child| child.text.as_deref())?
        {
            "*" => Some(IpMark::Star),
            r"\-" => Some(IpMark::Dash),
            r"\(bu" | r"\[bu]" => Some(IpMark::NamedBullet),
            _ => None,
        }
    }
    let current = mark(nodes.get(index)?)?;
    let continues = nodes[..index]
        .iter()
        .rev()
        .find(|node| crate::mandoc::adjacency::is_logical_sibling(node))
        .and_then(mark)
        == Some(current);
    let followed =
        crate::mandoc::adjacency::next(&nodes[index + 1..]).and_then(mark) == Some(current);
    (continues || followed).then_some(IpRun {
        kind: if current == IpMark::Dash {
            ListKind::Dash
        } else {
            ListKind::Bullet
        },
        continues,
    })
}

/// Convert only a source-proven adjacent IP run to a list. A new run remains
/// separate even when the rendered glyph matches a prior run's list kind.
pub(super) fn append_ip_marked_list(
    output: &mut Vec<Block>,
    item: DefinitionItem,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: u16,
    source: Option<mant_ir::SourceSpan>,
    run: IpRun,
) {
    let list_item = super::ordered::spaced_man_list_item(item, 2, source, paragraph_distance);
    if run.continues
        && let Some(Block::List {
            kind,
            compact,
            items,
            ..
        }) = output
            .last_mut()
            .filter(|block| block_indent(block) == Some(indent_columns.relative_columns()))
        && *kind == run.kind
    {
        *compact = *compact && paragraph_distance == 0;
        items.push(list_item);
        return;
    }

    output.push(Block::List {
        kind: run.kind,
        compact: paragraph_distance == 0,
        items: vec![list_item],
        layout: layout_with_spacing(indent_columns, 0),
        source,
    });
}

/// Preserve literal tags without turning typographical marks into discovered
/// values or terms. Explicit bold/code marking is positive key-name evidence;
/// section names and the role of a containing option are not.
pub(super) fn record_mark_role(
    node: &Node,
    item: &DefinitionItem,
    context: &LoweringContext<'_>,
) -> bool {
    use crate::definitions::NativeHeadRole;
    use mant_ir::Inline;

    fn styled(inlines: &[Inline], in_style: bool) -> bool {
        inlines.iter().all(|inline| match inline {
            Inline::Anchor { .. } | Inline::Code { .. } | Inline::Equation { .. } => true,
            Inline::Text { value } => value.trim().is_empty() || in_style,
            Inline::Strong { children } => styled(children, true),
            Inline::Emphasis { children } | Inline::Link { children, .. } => {
                styled(children, in_style)
            }
            Inline::LineBreak { .. } => false,
        })
    }
    let [term] = item.terms.as_slice() else {
        return false;
    };
    let text = plain_text(term);
    // A named roff bullet is an authored DT in a singleton IP/TP, but still
    // only a presentation mark unless its source explicitly styles the term.
    // Require both its complete visible spelling and native escape evidence:
    // a literal Unicode bullet need not have the same source role.
    if text.trim() == "•"
        && super::super::definition::visible_definition_head(node)
            .iter()
            .any(contains_named_bullet_escape)
    {
        let role = if styled(term, false) {
            NativeHeadRole::LiteralTerm
        } else {
            NativeHeadRole::Presentation
        };
        context.native_heads.borrow_mut().record(item, role);
        return true;
    }
    let mut chars = text.trim().chars();
    let Some(mark) = chars.next() else {
        return false;
    };
    if chars.next().is_some() || !mark.is_ascii() || (mark.is_ascii_alphanumeric() && mark != 'o') {
        return false;
    }
    // An explicitly styled dash in a TP/TQ head is a complete shell operand
    // (for example, `set -`). An IP head uses the same visible DT for a
    // literal punctuation key, so retain that separate source role.
    // man_html.c::man_IP_pre() renders both spellings as authored tags.
    if mark == '-' && styled(term, false) && node.macro_token.as_ref() != Some(&Man(ManMacro::Ip)) {
        context
            .native_heads
            .borrow_mut()
            .record(item, NativeHeadRole::Operand);
        return true;
    }
    let role = if styled(term, false) {
        NativeHeadRole::LiteralTerm
    } else {
        NativeHeadRole::Presentation
    };
    context.native_heads.borrow_mut().record(item, role);
    true
}

fn contains_named_bullet_escape(node: &Node) -> bool {
    node.text
        .as_ref()
        .is_some_and(|text| text.contains(r"\(bu") || text.contains(r"\[bu]"))
        || node.children.iter().any(contains_named_bullet_escape)
}

#[cfg(test)]
mod tests {
    use super::{Block, plain_text};

    #[test]
    fn ip_literal_marks_and_styled_keys_are_not_inferred_bullets() {
        for (mark, expected) in [
            ("*", "*"),
            ("o", "o"),
            ("#", "#"),
            ("=", "="),
            (r"\e", "\\"),
            ("^", "^"),
            ("$", "$"),
            ("+", "+"),
            ("-", "-"),
            (r"\fB*\fP", "*"),
            ("•", "•"),
        ] {
            let source = format!(".TH MARK 1\n.SH DESCRIPTION\n.IP \"{mark}\" 4\nBODY\n");
            let document = crate::mandoc::parse_plain_manual(
                std::path::Path::new("mark.1"),
                source.as_bytes(),
            )
            .unwrap();
            let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
                panic!("literal {mark:?} must retain its authored tag");
            };
            assert_eq!(plain_text(&items[0].terms[0]), expected);
        }
    }

    #[test]
    fn singleton_ip_bullet_stays_an_authored_definition_term() {
        // CVS man_html.c::list_continues() needs an adjacent compatible IP
        // before man_IP_pre() opens a UL; these exact inputs all yield DL.
        for (mark, literal_term) in [
            (r"\(bu", false),
            (r"\[bu]", false),
            (r"\fB\[bu]\fP", true),
            (r"\ \(bu", false),
        ] {
            let source = format!(".TH MARK 1\n.SH DESCRIPTION\n.IP \"{mark}\" 4\nBODY\n");
            let document = crate::mandoc::parse_plain_manual(
                std::path::Path::new("mark.1"),
                source.as_bytes(),
            )
            .unwrap();
            let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
                panic!("singleton {mark:?} must keep its authored tag");
            };
            assert_eq!(items.len(), 1);
            assert!(plain_text(&items[0].terms[0]).contains('•'));
            assert_eq!(items[0].entry.is_some(), literal_term, "{mark:?}");
        }
    }

    #[test]
    fn adjacent_ip_runs_follow_native_head_marker_identity() {
        // CVS man_html.c::list_continues() recognizes raw adjacent IP HEADs:
        // two escaped dashes form Bl-dash, star and named-bullet runs form
        // separate Bl-bullet lists, and mixed marks stay independent DLs.
        for (source, expected) in [
            (
                ".TH MARK 1\n.SH DESCRIPTION\n.IP \\- 2\nfirst\n.IP \\- 2\nsecond\n",
                vec![("dash", 2)],
            ),
            (
                ".TH MARK 1\n.SH DESCRIPTION\n.IP \\(bu 2\nFIRST\n.IP \\[bu] 2\nSECOND\n",
                vec![("bullet", 2)],
            ),
            (
                ".TH MARK 1\n.SH DESCRIPTION\n.IP * 2\nONE\n.IP * 2\nTWO\n.IP \\(bu 2\nTHREE\n.IP \\(bu 2\nFOUR\n",
                vec![("bullet", 2), ("bullet", 2)],
            ),
        ] {
            let document = crate::mandoc::parse_plain_manual(
                std::path::Path::new("ip-runs.1"),
                source.as_bytes(),
            )
            .expect("lower adjacent IP run");
            let actual = document.sections[0]
                .blocks
                .iter()
                .map(|block| match block {
                    Block::List { kind, items, .. } => (
                        match kind {
                            mant_ir::ListKind::Bullet => "bullet",
                            mant_ir::ListKind::Dash => "dash",
                            _ => "other",
                        },
                        items.len(),
                    ),
                    _ => ("definition", 0),
                })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{source}");
        }
    }
}
