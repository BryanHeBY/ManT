//! Source-body text rendering and excerpt presentation with separate owners.

pub(super) mod blocks;
mod body;
mod excerpt;
mod flow;
#[cfg(test)]
mod tests;

pub(super) use body::render_located_blocks;
pub use body::{render_query_man, render_query_text, render_query_text_with};
pub use excerpt::{render_excerpt_text, render_excerpt_text_with};

#[cfg(test)]
struct PlainRenderer;

#[cfg(test)]
impl PlainRenderer {
    fn renderer() -> blocks::BlockRenderer<'static> {
        blocks::BlockRenderer {
            content: crate::test_content::content(),
            locations: None,
            names: None,
            decorate: &|_, text| text.to_owned(),
        }
    }

    // Keep the instance-based test helper API used throughout the block fixtures.
    #[allow(clippy::unused_self)]
    fn render_blocks(&self, blocks: &[mant_ir::Block], base_indent: i32) -> String {
        Self::renderer().render_blocks(blocks, base_indent)
    }

    #[allow(clippy::unused_self)]
    fn render_block_sequence(
        &self,
        blocks: &[mant_ir::Block],
        base_indent: i32,
        leading_gap: Option<usize>,
    ) -> String {
        Self::renderer().render_block_sequence(blocks, base_indent, leading_gap)
    }
}

#[cfg(test)]
const fn plain_renderer() -> PlainRenderer {
    PlainRenderer
}

fn indent_lines(value: &str, columns: usize) -> String {
    if columns == 0 {
        return value.to_owned();
    }
    let prefix = " ".repeat(columns);
    value
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn document_label(document: &str, section: Option<&str>) -> String {
    section.map_or_else(
        || document.to_owned(),
        |section| format!("{document}({section})"),
    )
}
