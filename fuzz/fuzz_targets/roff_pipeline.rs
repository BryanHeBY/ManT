#![no_main]

use libfuzzer_sys::fuzz_target;
use libmandoc_rs::{InputFormat, SourceBundle};

mod query_pipeline;

fuzz_target!(|data: &[u8]| {
    if data.len() > query_pipeline::MAX_INPUT_BYTES {
        return;
    }
    let mut bundle = SourceBundle::new();
    if bundle.insert("fuzz.1", data.to_vec()).is_err() {
        return;
    }
    let Ok(document) =
        mant_codec::annotated_fixed::project_annotated_manual("fuzz.1", &bundle, InputFormat::Auto)
    else {
        return;
    };
    let query = mant_ir::ResolvedContent {
        address: None,
        label: "fuzz(1)".into(),
        document: Some(document),
        tldr: None,
    };
    let pattern = String::from_utf8_lossy(data);
    query_pipeline::exercise(&query, &pattern);
});
