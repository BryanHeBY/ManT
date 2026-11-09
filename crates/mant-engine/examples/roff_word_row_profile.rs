//! Stream public roff and Markdown observations for the finite word/row audit.

use std::io::{self, BufRead, BufWriter, Write};

use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{ResolvedContent, validate_document};
use mant_protocol::QueryBundle;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct Request {
    id: String,
    source: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut output = BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let request: Request = serde_json::from_str(&line?)?;
        if let Err(error) = profile(&request, &mut output) {
            writeln!(
                output,
                "{}",
                json!({ "id": request.id, "error": error.to_string() })
            )?;
        }
        writeln!(output, "{}", json!({ "id": request.id, "end": true }))?;
    }
    output.flush()?;
    Ok(())
}

fn observe(
    output: &mut impl Write,
    id: &str,
    route: &str,
    content: &ResolvedContent,
    markdown: Option<&str>,
) -> Result<ResolvedContent, Box<dyn std::error::Error>> {
    let bundle = QueryBundle::from(content);
    let encoded = serde_json::to_string(&bundle)?;
    let decoded: QueryBundle = serde_json::from_str(&encoded)?;
    let equivalent = encoded == serde_json::to_string(&decoded)?;
    let transported: ResolvedContent = decoded.into();
    let plain = mant_render::render_query_man(&transported);
    let decorated = mant_render::render_query_text_with(&transported, |_, text| {
        format!("\x1b[1m{text}\x1b[0m")
    });
    let valid = transported
        .document
        .as_ref()
        .is_some_and(|document| validate_document(document).is_empty());
    writeln!(
        output,
        "{}",
        json!({
            "id": id, "route": route, "json": encoded, "jsonEquivalent": equivalent,
            "validDocument": valid, "plain": plain,
            "ansiEquivalent": decorated.replace("\x1b[1m", "").replace("\x1b[0m", "") == plain,
            "markdown": markdown,
        }),
    )?;
    Ok(transported)
}

fn profile(request: &Request, output: &mut impl Write) -> Result<(), Box<dyn std::error::Error>> {
    let original = mant_loader::load_roff_bytes(request.source.as_bytes())?;
    let original = observe(output, &request.id, "roff", &original, None)?;
    for (options, routes) in [
        (MarkdownOptions::default(), ["default-1", "default-2"]),
        (
            MarkdownOptions::ADDRESSABLE,
            ["addressable-1", "addressable-2"],
        ),
    ] {
        let mut current = original.clone();
        for route in routes {
            let markdown = render_markdown_with_options(&current, options);
            let readback = mant_loader::load_markdown_text(&markdown, None)?;
            current = observe(output, &request.id, route, &readback, Some(&markdown))?;
        }
    }
    Ok(())
}
