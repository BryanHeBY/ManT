//! Exercise the real JSON string boundary shared by document consumers.

use mant_ir::ResolvedContent;
use mant_protocol::QueryBundle;

pub fn json_string_roundtrip(content: &ResolvedContent) -> ResolvedContent {
    let original = QueryBundle::from(content);
    let encoded = serde_json::to_string(&original).unwrap();
    let decoded: QueryBundle = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        serde_json::to_value(original).unwrap(),
        serde_json::to_value(&decoded).unwrap(),
        "JSON string roundtrip"
    );
    decoded.into()
}
