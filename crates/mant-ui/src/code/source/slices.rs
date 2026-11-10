//! Collect one form's ordered slices without revisiting preceding inline leaves.

use super::{Markup, Source, SourceNode, source_node, walk};
use mant_ir::{EntryContentSlice, Inline, inline_scalar_len};
use std::ops::Range;

impl Source {
    /// Append already validated, non-overlapping slices of one original root.
    /// Return the root-relative scalar ranges in form order. A caller discards
    /// the complete form on failure, including any previously appended roots.
    pub(crate) fn append_slices(
        &mut self,
        nodes: &[Inline],
        slices: &[EntryContentSlice],
        limit: usize,
    ) -> Option<Vec<Range<usize>>> {
        let mut cursor = Slices::new(self, limit);
        if let [slice] = slices
            && slice.path.is_empty()
        {
            cursor.whole(nodes, Markup::default())?;
        } else {
            cursor.walk(nodes, slices, 0, Markup::default(), false)?;
        }
        Some(cursor.ranges)
    }
}

struct Slices<'a> {
    source: &'a mut Source,
    ranges: Vec<Range<usize>>,
    scalar: usize,
    limit: usize,
    #[cfg(test)]
    visited_siblings: usize,
}

impl<'a> Slices<'a> {
    fn new(source: &'a mut Source, limit: usize) -> Self {
        Self {
            source,
            ranges: Vec::new(),
            scalar: 0,
            limit,
            #[cfg(test)]
            visited_siblings: 0,
        }
    }

    fn walk(
        &mut self,
        nodes: &[Inline],
        mut slices: &[EntryContentSlice],
        depth: usize,
        markup: Markup,
        need_end: bool,
    ) -> Option<()> {
        for (index, node) in nodes.iter().enumerate() {
            let Some(next) = slices.first() else {
                if need_end {
                    self.scalar += inline_scalar_len(&nodes[index..]);
                }
                break;
            };
            let wanted = *next.path.get(depth)?;
            #[cfg(test)]
            {
                self.visited_siblings += 1;
            }
            if index < wanted {
                self.scalar += inline_scalar_len(std::slice::from_ref(node));
                continue;
            }
            if index != wanted {
                return None;
            }
            let count = slices.partition_point(|slice| slice.path.get(depth) == Some(&index));
            let (selected, remaining) = slices.split_at(count);
            self.node(
                node,
                selected,
                depth + 1,
                markup,
                need_end || !remaining.is_empty(),
            )?;
            slices = remaining;
        }
        slices.is_empty().then_some(())
    }

    fn node(
        &mut self,
        node: &Inline,
        slices: &[EntryContentSlice],
        depth: usize,
        markup: Markup,
        more_siblings: bool,
    ) -> Option<()> {
        if slices[0].path.len() > depth {
            let SourceNode::Container(children, markup) = source_node(node, markup) else {
                return None;
            };
            // The next slice may follow this wrapper. Account for its
            // unselected suffix once, in the same traversal.
            self.walk(children, slices, depth, markup, more_siblings)
        } else if slices[0].bytes.is_none() {
            self.whole(std::slice::from_ref(node), markup)
        } else {
            let SourceNode::Text(text, markup) = source_node(node, markup) else {
                return None;
            };
            self.bytes(text, slices, markup, more_siblings)
        }
    }

    // Preserve original leaf boundaries for form-level syntax evidence.
    // The ordinary prose collector can coalesce them without affecting its
    // complete-root contract.
    fn whole(&mut self, nodes: &[Inline], markup: Markup) -> Option<()> {
        let start = self.source.text.len();
        walk(nodes, markup, &mut |markup, text| {
            self.source.push_bounded(text, markup, self.limit, false)
        })
        .then_some(())?;
        self.record(self.source.text[start..].chars().count());
        Some(())
    }

    fn bytes(
        &mut self,
        text: &str,
        slices: &[EntryContentSlice],
        markup: Markup,
        need_end: bool,
    ) -> Option<()> {
        let mut byte = 0;
        for slice in slices {
            let range = slice.bytes.as_ref()?;
            self.scalar += text.get(byte..range.start)?.chars().count();
            let selected = text.get(range.clone())?;
            self.source
                .push_bounded(selected, markup, self.limit, false)
                .then_some(())?;
            self.record(selected.chars().count());
            byte = range.end;
        }
        if need_end {
            self.scalar += text.get(byte..)?.chars().count();
        }
        Some(())
    }

    fn record(&mut self, scalars: usize) {
        self.ranges.push(self.scalar..self.scalar + scalars);
        self.scalar += scalars;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::{EntryInlineRoot, LinkTarget};

    fn text(value: &str) -> Inline {
        Inline::Text {
            value: value.into(),
        }
    }

    fn slice(path: Vec<usize>, bytes: Option<Range<usize>>) -> EntryContentSlice {
        EntryContentSlice {
            root: EntryInlineRoot::Term { index: 0 },
            path,
            bytes,
        }
    }

    #[test]
    fn nested_sparse_slices_keep_original_scalar_positions_and_markup() {
        let nodes = [
            text("π"),
            Inline::Link {
                target: LinkTarget::External {
                    uri: "https://example.test".into(),
                },
                title: None,
                children: vec![
                    Inline::Emphasis {
                        children: vec![text("ä文xö"), text("hidden")],
                    },
                    text("z"),
                ],
            },
            Inline::line_break(),
            Inline::Code {
                value: "tail".into(),
            },
        ];
        let parts = [
            slice(vec![1, 0, 0], Some(2..5)),
            slice(vec![1, 0, 0], Some(6..8)),
            slice(vec![2], None),
            slice(vec![3], Some(1..4)),
        ];
        let mut source = Source::default();
        assert_eq!(
            source.append_slices(&nodes, &parts, 12),
            Some(vec![2..3, 4..5, 12..13, 14..17])
        );
        assert_eq!(source.text, "文ö\nail");
        assert_eq!(source.emphasis, [0..3, 3..5]);
        assert_eq!(source.code, std::iter::once(6..9).collect::<Vec<_>>());

        let mut bounded = Source::default();
        assert!(bounded.append_slices(&nodes, &parts, 8).is_none());
    }

    #[test]
    fn fragmented_form_walks_siblings_once_and_stops_at_last_selection() {
        let mut source = Source::default();
        let nodes = vec![text("名"); 4096];
        let selected = (0..1024)
            .step_by(2)
            .map(|index| slice(vec![index], Some(0..3)))
            .collect::<Vec<_>>();
        let mut cursor = Slices::new(&mut source, 1536);
        cursor
            .walk(&nodes, &selected, 0, Markup::default(), false)
            .unwrap();
        assert_eq!(cursor.visited_siblings, 1023);
        assert_eq!(cursor.ranges.len(), 512);
        assert_eq!(cursor.ranges.last(), Some(&(1022..1023)));
        assert_eq!(source.text, "名".repeat(512));
    }
}
