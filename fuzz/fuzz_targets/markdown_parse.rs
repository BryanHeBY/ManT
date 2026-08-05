#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    let _ = mant_core::parse_markdown(data, None);
});
