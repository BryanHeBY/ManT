#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    if data.len() > 64 * 1024 {
        return;
    }
    let _ = mant_engine::parse_markdown(data, None);
});
