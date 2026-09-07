//! Resolve the same final-IR block paths emitted by the evidence walk.
use mant_ir::{Block, Document};

/// Resolve an explanation block/preview coordinate in the exact queried IR.
///
/// Paths start with `root` or `sections/sN[/sN...]`. Remaining zero-based
/// components select blocks (`bN`), list items (`iN`), definition descriptions
/// (`dN`) or table rows/cells (`rN/cN`). Malformed or stale paths return None.
/// Coordinates are local to this document snapshot, not durable identities.
#[must_use]
pub fn resolve_explanation_block<'a>(document: &'a Document, path: &str) -> Option<&'a Block> {
    let mut parts = path.split('/').peekable();
    let mut blocks = match parts.next()? {
        "root" => &document.blocks,
        "sections" => {
            let mut sections = &document.sections;
            let mut selected = None;
            while parts.peek().is_some_and(|p| p.starts_with('s')) {
                let index = index(parts.next()?, 's')?;
                let section = sections.get(index)?;
                sections = &section.children;
                selected = Some(&section.blocks);
            }
            selected?
        }
        _ => return None,
    };
    loop {
        let block = blocks.get(index(parts.next()?, 'b')?)?;
        if parts.peek().is_none() {
            return Some(block);
        }
        blocks = match block {
            Block::List { items, .. } => &items.get(index(parts.next()?, 'i')?)?.blocks,
            Block::DefinitionList { items, .. } => {
                &items.get(index(parts.next()?, 'd')?)?.description
            }
            Block::Table { rows, .. } => {
                &rows
                    .get(index(parts.next()?, 'r')?)?
                    .cells
                    .get(index(parts.next()?, 'c')?)?
                    .blocks
            }
            _ => return None,
        };
    }
}
fn index(value: &str, prefix: char) -> Option<usize> {
    let digits = value.strip_prefix(prefix)?;
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}
