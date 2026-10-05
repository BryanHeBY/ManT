//! Fence the accepted portable table rows and retain mapped owners.
use super::nonempty;
use crate::encode::{
    MarkdownOptions,
    inline::{fenced_code, preformatted_anchor_markers},
    mapped::{BlockSyntax, MappedText},
};
use mant_ir::{Block, TableRow};

pub(super) fn render_table(
    rows: &[TableRow],
    options: MarkdownOptions,
    track: bool,
) -> Option<MappedText> {
    // Empty data rows and native rules are authored structure, not fence
    // delimiters. Keep them in the same source-order portable row plan.
    let markers = if options.preserve_anchors {
        rows.iter()
            .filter(|row| mant_ir::table_row_is_navigation_only(row))
            .flat_map(|row| &row.cells)
            .flat_map(|cell| &cell.blocks)
            .filter_map(|block| match block {
                Block::Paragraph { children, .. } => Some(preformatted_anchor_markers(children)),
                _ => None,
            })
            .collect::<String>()
    } else {
        String::new()
    };
    let rows = crate::encode::table_projection::rows(rows, track);
    if rows.is_empty() {
        return nonempty(markers).map(|mut markers| {
            markers.contribution.rows = false;
            markers.syntax_site(BlockSyntax::Phrasing)
        });
    }
    Some({
        let mut body = MappedText::join(rows, "\n");
        let fenced = fenced_code(&body.text, None);
        let prefix = fenced.find('\n').expect("fence header") + 1;
        for (_, range) in &mut body.owners {
            range.start += prefix;
            range.end += prefix;
        }
        body.text = fenced;
        // Accepted data rows remain physical even when every cell is empty.
        // Their fence is structure, unlike a navigation-only row filtered above.
        body.contribution.rows = true;
        if !markers.is_empty() {
            body.insert(0, &format!("{markers}\n\n"));
        }
        body.syntax_site(if markers.is_empty() {
            BlockSyntax::Fence
        } else {
            BlockSyntax::Phrasing
        })
        .tail_grammar(BlockSyntax::Fence, false)
    })
}
