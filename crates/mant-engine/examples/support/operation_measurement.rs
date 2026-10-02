//! Seven same-process operation samples; serialization and destruction stay outside.

use std::{any::Any, error::Error, hint::black_box, path::Path, time::Instant};

use mant_ir::ResolvedContent;
use serde_json::{Value, json};

#[path = "phase_measurement.rs"]
mod phase_measurement;

const ROUNDS: usize = 7;

pub(super) fn measure(
    path: &Path,
    source: &[u8],
    mode: &str,
    literal: &str,
) -> Result<Value, Box<dyn Error>> {
    if mode == "phase" {
        return phase_measurement::measure(path, source);
    }
    if !matches!(
        mode,
        "load" | "index" | "outline" | "explain" | "text" | "markdown" | "search"
    ) {
        return Err(
            "expected load, index, outline, explain, text, markdown, search or phase".into(),
        );
    }
    let load = || mant_loader::parse_manual_bytes(path, source);
    let content = ResolvedContent {
        label: path.to_string_lossy().into_owned(),
        address: None,
        document: Some(load()?),
        tldr: None,
    };
    let content = if mode == "load" {
        drop(content);
        None
    } else {
        Some(content)
    };
    let explanation = mant_protocol::ExplanationQuery {
        entry: literal.to_owned(),
        options: mant_protocol::ExplanationOptions::default(),
    };
    let search: mant_protocol::SearchQuery = serde_json::from_value(json!({"pattern": literal}))?;
    let mut elapsed = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let start = Instant::now();
        let value: Box<dyn Any> = if mode == "load" {
            Box::new(load()?)
        } else {
            operation(
                content.as_ref().expect("loaded content"),
                mode,
                &explanation,
                &search,
            )?
        };
        elapsed.push(start.elapsed().as_secs_f64() * 1000.0);
        black_box(&value);
        drop(value);
    }
    Ok(json!({
        "schema": "mant.operation-measurement/v1",
        "operation": mode,
        "milliseconds": elapsed,
        "rounds": ROUNDS,
        "scope": {
            "source": "uncompressed bytes; gzip decode before initial load and timers",
            "initialLoad": "once before timers; dropped before load mode",
            "resultAllocation": "inside timer, including Box",
            "resultDestruction": "after timer",
            "serialization": "measurement array only, after all timers",
            "index": "rebuild every index operation; outline is Summary",
            "warmup": "no unmeasured target-query operation; retain all seven values"
        }
    }))
}

fn operation(
    content: &ResolvedContent,
    mode: &str,
    explanation: &mant_protocol::ExplanationQuery,
    search: &mant_protocol::SearchQuery,
) -> Result<Box<dyn Any>, Box<dyn Error>> {
    Ok(match mode {
        "index" => Box::new(content.semantic_index()),
        "outline" => Box::new(mant_query::build_outline(content)?),
        "explain" => Box::new(mant_query::explain_query(content, explanation)?),
        "text" => Box::new(mant_render::render_query_man(content)),
        "markdown" => Box::new(mant_codec::encode::render_markdown(content)),
        "search" => Box::new(mant_query::search_query(content, search)?),
        _ => unreachable!("validated operation"),
    })
}
