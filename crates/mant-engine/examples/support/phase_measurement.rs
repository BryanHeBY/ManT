//! Stage wall times from source-less owned lowering and an independent byte loader.
//! This is not a CPU profiler; independently measured medians are not additive.

use std::{error::Error, hint::black_box, path::Path, time::Instant};

use libmandoc_rs::{Compression, IncludePolicy, ParseOptions, Parser};
use serde_json::{Value, json};

pub(super) fn measure(path: &Path, source: &[u8]) -> Result<Value, Box<dyn Error>> {
    let parser = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    });
    // One complete byte-loader call before all timers, matching operation
    // initial-load policy. It is not a separate warmup for each phase.
    drop(mant_loader::parse_manual_bytes(path, source)?);
    let mut parsed = Vec::with_capacity(super::ROUNDS);
    let mut lowered = Vec::with_capacity(super::ROUNDS);
    let mut rendered = Vec::with_capacity(super::ROUNDS);
    let mut released = Vec::with_capacity(super::ROUNDS);
    let mut source_aware = Vec::with_capacity(super::ROUNDS);
    let mut output_bytes = Vec::with_capacity(super::ROUNDS);
    for _ in 0..super::ROUNDS {
        let start = Instant::now();
        let report = parser.parse_bytes(path, source)?;
        parsed.push(milliseconds(start));
        let start = Instant::now();
        let document = mant_codec::lower_mandoc_document(path, &report);
        lowered.push(milliseconds(start));
        let content = mant_ir::ResolvedContent {
            label: path.to_string_lossy().into_owned(),
            address: None,
            document: Some(document),
            tldr: None,
        };
        let start = Instant::now();
        let text = mant_render::render_query_man(&content);
        output_bytes.push(black_box(&text).len());
        rendered.push(milliseconds(start));
        let start = Instant::now();
        drop((report, content, text));
        released.push(milliseconds(start));
        let start = Instant::now();
        let loaded = mant_loader::parse_manual_bytes(path, source)?;
        source_aware.push(milliseconds(start));
        black_box(&loaded);
        drop(loaded);
    }
    Ok(json!({
        "schema": "mant.operation-measurement/v1",
        "operation": "phase",
        "rounds": super::ROUNDS,
        "phaseMilliseconds": {
            "parseOwned": parsed,
            "sourceLessLoweringAndRecognition": lowered,
            "nativeTextRender": rendered,
            "sourceLessDrop": released,
            "sourceAwareBytesLoad": source_aware
        },
        "outputBytes": output_bytes,
        "scope": {
            "source": "uncompressed bytes; gzip decode outside timers",
            "parseOwned": "native parse and owned transfer; Parser construction outside",
            "sourceLessLoweringAndRecognition": "public owned lowering without source text",
            "nativeTextRender": "String allocation and render, excluding destruction",
            "sourceLessDrop": "report, content and rendered String destruction",
            "sourceAwareBytesLoad": "independent parse_manual_bytes call; allocation inside, destruction outside; no file I/O",
            "initialLoad": "one byte-loader call before all timers",
            "combination": "phase medians are separate observations, not a production-call decomposition"
        }
    }))
}

fn milliseconds(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
