#![no_main]

use libfuzzer_sys::fuzz_target;
use mant_core::TldrPageLocation;

fuzz_target!(|data: &str| {
    let location = TldrPageLocation {
        platform: "common".to_owned(),
        language: "en".to_owned(),
        source_path: "fuzz.md".to_owned(),
    };
    let _ = mant_core::parse_tldr_page(data, location);
    let _ = mant_core::parse_tldr_command(data);
});
