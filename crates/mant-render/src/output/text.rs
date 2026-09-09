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
fn plain_renderer() -> blocks::BlockRenderer<'static> {
    blocks::BlockRenderer {
        locations: None,
        names: None,
        decorate: &|_, text| text.to_owned(),
    }
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
