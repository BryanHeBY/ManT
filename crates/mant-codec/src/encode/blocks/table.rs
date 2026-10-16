//! Fence the accepted portable table rows and retain mapped owners.
use super::nonempty;
use crate::encode::{
    MarkdownOptions,
    inline::{fenced_code, html_anchor},
    mapped::{BlockSyntax, MappedText},
};
use mant_ir::visit::Visit;
use mant_ir::{Inline, TableRow};
use std::collections::HashSet;

pub(super) fn render_table(
    rows: &[TableRow],
    options: MarkdownOptions,
    track: bool,
) -> Option<MappedText> {
    // Empty data rows and native rules are authored structure, not fence
    // delimiters. Keep them in the same source-order portable row plan.
    let markers = if options.preserve_anchors {
        table_markers(rows)
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

/// Destinations are transparent regardless of a sibling glyph or container.
/// Keep them outside the fence; mapped insertion rebases the original owners.
fn table_markers(rows: &[TableRow]) -> String {
    #[derive(Default)]
    struct Markers<'a> {
        seen: HashSet<&'a str>,
        output: Vec<String>,
    }
    impl<'a> Visit<'a> for Markers<'a> {
        fn visit_inline(&mut self, inline: &'a Inline) {
            if let Inline::Anchor {
                id,
                fragment_aliases,
                ..
            } = inline
            {
                for target in std::iter::once(id.as_str())
                    .chain(fragment_aliases.iter().map(mant_ir::FragmentAlias::as_str))
                {
                    if self.seen.insert(target) {
                        self.output.push(html_anchor(target));
                    }
                }
            } else {
                mant_ir::visit::walk_inline(self, inline);
            }
        }
    }
    let mut markers = Markers::default();
    for row in rows {
        for cell in &row.cells {
            for block in &cell.blocks {
                markers.visit_block(block);
            }
        }
    }
    markers.output.join("\n")
}
