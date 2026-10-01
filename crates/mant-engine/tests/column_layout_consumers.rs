//! Complete nested columns keep their owners through the actual JSON wire.

use mant_ir::{Block, ResolvedContent, visit::Visit};

const HEADER: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd test page\n.Sh DESCRIPTION\n";

#[derive(Default)]
struct Columns(usize);

impl<'ir> Visit<'ir> for Columns {
    fn visit_block(&mut self, block: &'ir Block) {
        if matches!(block, Block::Table { .. }) {
            self.0 += 1;
        }
        mant_ir::visit::walk_block(self, block);
    }
}

#[test]
fn nested_declared_columns_render_all_cells_after_json_in_plain_and_styled_views() {
    // Every exact input ran pristine CVS ASCII/UTF-8/HTML/lint first (depth
    // 0, 1, 2, 4, 8, 12, 16; no diagnostics). mdoc_term.c::termp_it_pre
    // establishes each column field; this depth remains structurally complete.
    // Renderer unit tests additionally count visits to reject double layout.
    for depth in [0, 1, 2, 4, 8, 12, 16] {
        let source = format!(
            "{HEADER}{}leaf\n{}",
            ".Bl -column \"xx\"\n.It head\n".repeat(depth),
            ".El\n".repeat(depth)
        );
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&loaded, false).unwrap();
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: ResolvedContent = restored.into();
        let mut columns = Columns::default();
        columns.visit_document(query.document.as_ref().unwrap());
        assert_eq!(columns.0, depth, "depth {depth}: complete column IR");

        let plain = mant_render::render_query_man(&query);
        let styled = mant_render::render_query_text_with(&query, |_, text| {
            format!("\u{1b}[1m{text}\u{1b}[0m")
        });
        for text in [&plain, &styled] {
            assert_eq!(text.matches("head").count(), depth, "depth {depth}");
            assert_eq!(text.matches("leaf").count(), 1, "depth {depth}");
        }
    }
}
