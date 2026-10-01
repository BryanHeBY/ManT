//! Project accepted device-row origins at stable source-word scalar positions.

use std::collections::BTreeMap;

use super::{INTERNAL_FIELD_WORD, INTERNAL_ROW_ORIGIN, Inline};

pub(in crate::mandoc::inline::flow) fn project_row_origins(
    nodes: &mut Vec<Inline>,
    origins: &[(String, usize, usize)],
    output_start: usize,
) {
    if origins.is_empty() {
        return;
    }
    let mut positions = BTreeMap::<String, Vec<(usize, usize)>>::new();
    for (owner, scalar, origin) in origins {
        positions
            .entry(owner.clone())
            .or_default()
            .push((*scalar, *origin));
    }
    let mut cursor = OriginCursor {
        positions: &positions,
        owner: None,
        scalar: 0,
        next: 0,
        previous: BTreeMap::new(),
    };
    // Earlier fields are already committed. The captured current-owner
    // start predates its retirement; only that suffix is projected, using
    // stable words/scalars rather than indices for the receipt positions.
    let pending = nodes.split_off(output_start.min(nodes.len()));
    nodes.extend(cursor.project(pending));
}

struct OriginCursor<'a> {
    positions: &'a BTreeMap<String, Vec<(usize, usize)>>,
    owner: Option<String>,
    scalar: usize,
    next: usize,
    previous: BTreeMap<String, (usize, usize)>,
}

impl OriginCursor<'_> {
    fn project(&mut self, nodes: Vec<Inline>) -> Vec<Inline> {
        let mut output = Vec::with_capacity(nodes.len());
        for mut node in nodes {
            if let Inline::Anchor { id, .. } = &node
                && id.as_str().starts_with(INTERNAL_FIELD_WORD)
            {
                if let Some(owner) = self.owner.replace(id.as_str().to_owned()) {
                    self.previous.insert(owner, (self.scalar, self.next));
                }
                (self.scalar, self.next) =
                    self.previous.get(id.as_str()).copied().unwrap_or_default();
                output.push(node);
                self.emit_here(&mut output);
                continue;
            }
            match &mut node {
                Inline::Text { value } | Inline::Code { value } if !value.is_empty() => {
                    self.project_text(&node, &mut output);
                    continue;
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::PortableDisplay { children, .. }
                | Inline::Link { children, .. } => {
                    *children = self.project(std::mem::take(children));
                }
                _ => {}
            }
            output.push(node);
        }
        output
    }

    fn emit_here(&mut self, output: &mut Vec<Inline>) {
        let Some(positions) = self
            .owner
            .as_ref()
            .and_then(|owner| self.positions.get(owner))
        else {
            return;
        };
        while let Some((scalar, origin)) = positions.get(self.next) {
            if *scalar != self.scalar {
                break;
            }
            output.push(Inline::anchor(format!("{INTERNAL_ROW_ORIGIN}{origin}")));
            self.next += 1;
        }
    }

    fn project_text(&mut self, node: &Inline, output: &mut Vec<Inline>) {
        let (Inline::Text { value } | Inline::Code { value }) = node else {
            unreachable!("only word glyph projections advance the scalar cursor");
        };
        let mut piece = String::new();
        for character in value.chars() {
            if self
                .owner
                .as_ref()
                .and_then(|owner| self.positions.get(owner))
                .and_then(|positions| positions.get(self.next))
                .is_some_and(|(scalar, _)| *scalar == self.scalar)
            {
                append_piece(node, &mut piece, output);
                self.emit_here(output);
            }
            piece.push(character);
            self.scalar += 1;
        }
        append_piece(node, &mut piece, output);
        self.emit_here(output);
    }
}

fn append_piece(node: &Inline, piece: &mut String, output: &mut Vec<Inline>) {
    if piece.is_empty() {
        return;
    }
    let value = std::mem::take(piece);
    output.push(match node {
        Inline::Text { .. } => Inline::Text { value },
        Inline::Code { .. } => Inline::Code { value },
        _ => unreachable!("only Text and Code have scalar pieces"),
    });
}
