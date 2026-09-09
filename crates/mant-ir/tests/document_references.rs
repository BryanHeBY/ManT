use mant_ir::{DocumentReference, LinkTarget};
use serde_json::json;

#[test]
fn local_reference_subset_and_inline_targets_reject_unknown_fields() {
    for value in [
        json!({"kind":"document","name":"topic","fragment":"Mixed.Target"}),
        json!({"kind":"manual","name":"printf"}),
        json!({"kind":"manual","name":"printf","manualSection":"3"}),
    ] {
        assert!(serde_json::from_value::<DocumentReference>(value.clone()).is_ok());
        assert!(serde_json::from_value::<LinkTarget>(value.clone()).is_ok());
        let mut invalid = value;
        invalid["execute"] = json!(true);
        assert!(serde_json::from_value::<DocumentReference>(invalid.clone()).is_err());
        assert!(serde_json::from_value::<LinkTarget>(invalid).is_err());
    }
    for value in [
        json!({"kind":"external","uri":"https://example.com"}),
        json!({"kind":"email","address":"help@example.com"}),
        json!({"kind":"section","id":"topic"}),
    ] {
        assert!(serde_json::from_value::<DocumentReference>(value.clone()).is_err());
        assert!(serde_json::from_value::<LinkTarget>(value.clone()).is_ok());
        let mut invalid = value;
        invalid["execute"] = json!(true);
        assert!(serde_json::from_value::<LinkTarget>(invalid).is_err());
    }
}
