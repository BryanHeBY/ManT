//! Local, same-process native loading benchmark (not serialization or CLI time).
//! Run in release mode with a fixed gzip fixture; retain this executable when
//! comparing revisions. Decompression happens before the measured operation.
use std::{fs::File, io::Read, path::Path, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("expected gzip fixture path")?;
    let mut source = Vec::new();
    flate2::read::GzDecoder::new(File::open(&path)?).read_to_end(&mut source)?;
    let load = || mant_engine::parse_manual_bytes(Path::new(&path), &source);
    drop(load()?);
    let mut elapsed = Vec::new();
    for _ in 0..7 {
        let start = Instant::now();
        let document = load()?;
        elapsed.push(start.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(&document);
    }
    println!("{}", serde_json::json!({"nativeLoadMilliseconds": elapsed}));
    Ok(())
}
