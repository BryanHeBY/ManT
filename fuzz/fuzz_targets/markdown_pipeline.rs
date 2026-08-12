#![no_main]

use libfuzzer_sys::fuzz_target;
use mant_core::query_markdown_text;

mod query_pipeline;

fuzz_target!(|data: &str| {
    if data.len() > query_pipeline::MAX_INPUT_BYTES {
        return;
    }
    let Ok(query) = query_markdown_text(data, None) else {
        return;
    };
    query_pipeline::exercise(&query, data);
});
