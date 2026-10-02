//! Local, same-process native loading benchmark (not serialization or CLI time).
//! Run in release mode with a fixed gzip fixture; retain this executable when
//! comparing revisions. Decompression happens before the measured operation.
//! Optional `index`, `outline`, `explain`, `text`, `markdown`, or `search`
//! measures a post-load operation; `phase` reports separately bounded stages.
//! a third argument selects the explanation literal (default `-x`). For RSS,
//! run each mode in its own process: peak RSS includes the initial load.
use std::{fs::File, io::Read, path::Path};

#[path = "support/operation_measurement.rs"]
mod operation_measurement;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("expected gzip fixture path")?;
    let mut source = Vec::new();
    flate2::read::GzDecoder::new(File::open(&path)?).read_to_end(&mut source)?;
    let mode = std::env::args().nth(2).unwrap_or_else(|| "load".into());
    let query = std::env::args()
        .nth(3)
        .unwrap_or_else(|| if mode == "search" { "option" } else { "-x" }.into());
    let result = operation_measurement::measure(Path::new(&path), &source, &mode, &query)?;
    println!("{result}");
    Ok(())
}
