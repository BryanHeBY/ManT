#![no_main]

use libfuzzer_sys::fuzz_target;
use mantdoc::{Compression, RenderFormat, Renderer, SourceName};

mod query_pipeline;

fuzz_target!(|data: &[u8]| {
    if data.len() > query_pipeline::MAX_INPUT_BYTES {
        return;
    }
    let name = SourceName::new("fuzz.1").expect("static source name is valid");
    for format in [RenderFormat::Ascii, RenderFormat::Utf8, RenderFormat::Html] {
        let _ = Renderer::new(format)
            .with_max_output_bytes(128 * 1024)
            .render_bytes(&name, data, Compression::Plain);
    }
    let Ok(query) = mant_engine::query_roff_bytes(data) else {
        return;
    };
    let pattern = String::from_utf8_lossy(data);
    query_pipeline::exercise(&query, &pattern);
});
