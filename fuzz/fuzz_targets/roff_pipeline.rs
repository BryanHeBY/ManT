#![no_main]

use libfuzzer_sys::fuzz_target;

mod query_pipeline;

fuzz_target!(|data: &[u8]| {
    if data.len() > query_pipeline::MAX_INPUT_BYTES {
        return;
    }
    let Ok(query) = mant_core::query_roff_bytes(data) else {
        return;
    };
    let pattern = String::from_utf8_lossy(data);
    query_pipeline::exercise(&query, &pattern);
});
