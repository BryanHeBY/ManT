//! Rebase owner and navigation ranges through list prefixes.
use super::{BlockSyntax, MappedText, TailSyntax};

impl MappedText {
    /// Apply the renderer's existing list prefixes while moving every boundary
    /// through the same line transform, including CRLF and empty continuation lines.
    pub(in crate::encode) fn prefix(self, marker: &str) -> Option<Self> {
        if self.text.trim().is_empty() {
            return None;
        }
        let continuation = " ".repeat(marker.chars().count());
        let mut output = String::new();
        let mut segments = Vec::new();
        let mut offset = 0;
        let mut navigation = self.navigation.map(|mut site| {
            site.continuation_columns += marker.chars().count();
            site
        });
        for (index, line) in self.text.lines().enumerate() {
            if index > 0 {
                output.push('\n');
            }
            if index == 0 {
                output.push_str(marker);
            } else if !line.is_empty() {
                output.push_str(&continuation);
            }
            if !self.owners.is_empty() {
                segments.push((offset, line.len(), output.len()));
            }
            if let (Some(site), Some(mapped)) = (self.navigation, &mut navigation) {
                for (position, destination) in [
                    (site.offset, &mut mapped.offset),
                    (site.tail, &mut mapped.tail),
                ] {
                    if position >= offset && position <= offset + line.len() {
                        *destination = output.len() + position - offset;
                    }
                }
            }
            output.push_str(line);
            offset += line.len();
            if self.text.as_bytes().get(offset) == Some(&b'\r') {
                offset += 1;
            }
            if self.text.as_bytes().get(offset) == Some(&b'\n') {
                offset += 1;
            }
        }
        let boundary = |position: usize| {
            let index = segments
                .partition_point(|(start, _, _)| *start <= position)
                .saturating_sub(1);
            let (start, length, mapped) = segments[index];
            mapped + position.saturating_sub(start).min(length)
        };
        let owners = self
            .owners
            .into_iter()
            .filter_map(|(owner, range)| {
                let range = boundary(range.start)..boundary(range.end);
                (range.start < range.end).then_some((owner, range))
            })
            .collect();
        Some(Self {
            text: output,
            owners,
            navigation,
            contribution: self.contribution,
            syntax: BlockSyntax::List {
                needs_blank: marker.ends_with(". ") && marker != "1. ",
            },
            tail: TailSyntax {
                columns: self.tail.columns.saturating_add(marker.chars().count()),
                ..self.tail
            },
            hard_rows: false,
        })
    }
}
