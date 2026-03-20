use super::hash_map_output_key;
use serde_json::json;

#[test]
fn hash_map_output_key_hides_structured_presenter_params() {
    assert_eq!(
        hash_map_output_key("params", &json!({"view": "minimal"})),
        None
    );
    assert_eq!(hash_map_output_key("params", &json!(["minimal"])), None);
    assert_eq!(hash_map_output_key("title", &json!("title")), Some("title"));
}

#[test]
fn hash_map_output_key_preserves_scalar_params_values() {
    assert_eq!(
        hash_map_output_key("params", &json!("keep me")),
        Some("params")
    );
}
