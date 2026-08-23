#![no_main]

use libfuzzer_sys::fuzz_target;
use libmandoc_rs::{RenderFormat, Renderer};

mod query_pipeline;

fuzz_target!(|data: &[u8]| {
    if data.len() > query_pipeline::MAX_INPUT_BYTES {
        return;
    }
    for format in [RenderFormat::Ascii, RenderFormat::Utf8, RenderFormat::Html] {
        let _ = Renderer::new(format)
            .with_max_output_bytes(128 * 1024)
            .render_bytes("fuzz.1", data);
    }
    let Ok(query) = mant_engine::query_roff_bytes(data) else {
        return;
    };
    let pattern = String::from_utf8_lossy(data);
    query_pipeline::exercise(&query, &pattern);
});
