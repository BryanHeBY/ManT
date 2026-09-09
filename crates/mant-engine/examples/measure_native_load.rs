//! Local, same-process native loading benchmark (not serialization or CLI time).
//! Run in release mode with a fixed gzip fixture; retain this executable when
//! comparing revisions. Decompression happens before the measured operation.
//! Optional `index`, `outline`, or `explain` isolates a post-load operation;
//! a third argument selects the explanation literal (default `-x`). For RSS,
//! run each mode in its own process: peak RSS includes the initial load.
use std::{fs::File, io::Read, path::Path, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("expected gzip fixture path")?;
    let mut source = Vec::new();
    flate2::read::GzDecoder::new(File::open(&path)?).read_to_end(&mut source)?;
    let load = || mant_loader::parse_manual_bytes(Path::new(&path), &source);
    let mode = std::env::args().nth(2).unwrap_or_else(|| "load".into());
    let query = mant_protocol::ExplanationQuery {
        entry: std::env::args().nth(3).unwrap_or_else(|| "-x".into()),
        options: mant_protocol::ExplanationOptions::default(),
    };
    let content = mant_ir::ResolvedContent {
        label: path.clone(),
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
    let mut elapsed = Vec::new();
    for _ in 0..7 {
        let start = Instant::now();
        // Keep result destruction outside the measured interval, as for load.
        let value: Box<dyn std::any::Any> = match mode.as_str() {
            "load" => Box::new(load()?),
            "index" => Box::new(content.as_ref().expect("loaded content").semantic_index()),
            "outline" => Box::new(mant_query::build_outline(
                content.as_ref().expect("loaded content"),
            )?),
            "explain" => Box::new(mant_query::explain_query(
                content.as_ref().expect("loaded content"),
                &query,
            )?),
            _ => return Err("expected load, index, outline or explain".into()),
        };
        elapsed.push(start.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(&value);
    }
    println!(
        "{}",
        serde_json::json!({"operation": mode, "milliseconds": elapsed})
    );
    Ok(())
}
