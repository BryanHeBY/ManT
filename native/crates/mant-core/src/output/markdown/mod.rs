//! Renders the native query contract as deterministic portable `CommonMark`.

mod blocks;
mod inline;

use mant_ast::{QueryBundle, Section, TldrCommandPart, TldrDocument};

use self::{blocks::render_blocks, inline::escape_text};

/// Render a complete query without a process-level trailing newline.
#[must_use]
pub fn render_markdown(query: &QueryBundle) -> String {
    let mut output = Vec::new();
    output.push(heading(1, &query.topic));

    if let Some(tldr) = &query.tldr {
        output.extend(render_tldr(tldr));
        if query.manual.is_some() {
            output.push("---".to_owned());
        }
    }

    if let Some(manual) = &query.manual {
        render_sections(&mut output, &manual.sections, 2);
    }
    output
        .into_iter()
        .filter(|block| !block.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim_end()
        .to_owned()
}

fn render_sections(output: &mut Vec<String>, sections: &[Section], depth: usize) {
    for section in sections {
        output.push(heading(depth, &section.title));
        output.extend(render_blocks(&section.blocks));
        render_sections(output, &section.children, depth.saturating_add(1));
    }
}

fn render_tldr(page: &TldrDocument) -> Vec<String> {
    let mut output = vec![heading(2, "TLDR")];
    output.extend(
        page.description
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| escape_text(line.trim())),
    );

    if let Some(value) = page.more_information.as_deref() {
        output.push(render_more_information(value));
    }
    if !page.examples.is_empty() {
        output.push(heading(3, "Examples"));
        for example in &page.examples {
            if !example.description.trim().is_empty() {
                output.push(format!("**{}**", escape_text(example.description.trim())));
            }
            if !example.command.is_empty() {
                let resolved = example
                    .command_parts
                    .iter()
                    .map(|part| match part {
                        TldrCommandPart::Text { value }
                        | TldrCommandPart::Placeholder { value } => value.as_str(),
                    })
                    .collect::<String>();
                output.push(inline::fenced_code(
                    if resolved.is_empty() {
                        &example.command
                    } else {
                        &resolved
                    },
                    Some("sh"),
                ));
            }
        }
    }
    output.push(format!(
        "*tldr-pages · CC BY 4.0 · {} · {}*",
        escape_text(&page.platform),
        escape_text(&page.language)
    ));
    output
}

fn render_more_information(value: &str) -> String {
    let value = value.trim();
    if value.starts_with("http://") || value.starts_with("https://") {
        let (url, punctuation) = value
            .strip_suffix('.')
            .map_or((value, ""), |url| (url, "."));
        if !url.chars().any(char::is_whitespace) && !url.contains(['<', '>']) {
            return format!("**More information:** <{url}>{punctuation}");
        }
    }
    format!("**More information:** {}", escape_text(value))
}

fn heading(depth: usize, title: &str) -> String {
    format!("{} {}", "#".repeat(depth.clamp(1, 6)), escape_text(title))
}

#[cfg(test)]
mod tests;
