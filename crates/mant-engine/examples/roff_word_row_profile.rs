//! Stream public roff and Markdown observations for the finite word/row audit.

use std::io::{self, BufRead, BufWriter, Write};

use mant_ir::{ResolvedContent, validate_document};
use serde::Deserialize;
use serde_json::json;

#[path = "support/readback.rs"]
mod readback;
use readback::{Result, WireObservation, transport, visit_readbacks};

#[derive(Deserialize)]
struct Request {
    id: String,
    source: String,
}

fn main() -> Result<()> {
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
) -> Result<ResolvedContent> {
    let WireObservation {
        json: encoded,
        equivalent,
        content: transported,
    } = transport(content)?;
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

fn profile(request: &Request, output: &mut impl Write) -> Result<()> {
    let original = mant_loader::load_roff_bytes(request.source.as_bytes())?;
    let original = observe(output, &request.id, "roff", &original, None)?;
    visit_readbacks(&original, |route, readback, markdown| {
        observe(output, &request.id, route, readback, Some(markdown))
    })
}
