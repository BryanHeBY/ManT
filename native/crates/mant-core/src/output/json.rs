//! Serializes stable native contracts without a TypeScript shape conversion.

use mant_ast::{QueryBundle, QueryExcerpt, QueryOutline, QuerySearch, TldrCacheUpdate};

/// Serialize a query contract in compact or human-readable form.
///
/// # Errors
///
/// Propagates the unlikely JSON writer failure from [`serde_json`].
pub fn render_query_json(query: &QueryBundle, pretty: bool) -> Result<String, serde_json::Error> {
    render_json(query, pretty)
}

/// Serialize a complete query outline in compact or human-readable form.
///
/// # Errors
///
/// Propagates the unlikely JSON writer failure from [`serde_json`].
pub fn render_outline_json(
    outline: &QueryOutline,
    pretty: bool,
) -> Result<String, serde_json::Error> {
    render_json(outline, pretty)
}

/// Serialize selected query nodes in compact or human-readable form.
///
/// # Errors
///
/// Propagates the unlikely JSON writer failure from [`serde_json`].
pub fn render_excerpt_json(
    excerpt: &QueryExcerpt,
    pretty: bool,
) -> Result<String, serde_json::Error> {
    render_json(excerpt, pretty)
}

/// Serialize structure-aware search results in compact or human-readable form.
///
/// # Errors
///
/// Propagates the unlikely JSON writer failure from [`serde_json`].
pub fn render_search_json(search: &QuerySearch, pretty: bool) -> Result<String, serde_json::Error> {
    render_json(search, pretty)
}

/// Serialize the explicit tldr update result for a process boundary.
///
/// # Errors
///
/// Propagates the unlikely JSON writer failure from [`serde_json`].
pub fn render_update_json(
    update: &TldrCacheUpdate,
    pretty: bool,
) -> Result<String, serde_json::Error> {
    render_json(update, pretty)
}

fn render_json(value: &impl serde::Serialize, pretty: bool) -> Result<String, serde_json::Error> {
    if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
}

#[cfg(test)]
mod tests {
    use mant_ast::{QueryBundle, QuerySchema, TldrCacheAction, TldrCacheUpdate};

    use super::{render_query_json, render_update_json};

    #[test]
    fn compact_and_pretty_query_output_share_the_same_contract() {
        let query = QueryBundle {
            schema: QuerySchema::V2,
            topic: "ls".to_owned(),
            section: Some("1".to_owned()),
            manual: None,
            tldr: None,
        };
        let compact = render_query_json(&query, false).expect("compact JSON");
        let pretty = render_query_json(&query, true).expect("pretty JSON");

        assert_eq!(
            compact,
            r#"{"schema":"mant.query/v2","topic":"ls","section":"1"}"#
        );
        assert!(pretty.contains("\n  \"topic\": \"ls\","));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&compact).expect("compact value"),
            serde_json::from_str::<serde_json::Value>(&pretty).expect("pretty value")
        );
    }

    #[test]
    fn update_json_omits_absent_provider_fields() {
        let update = TldrCacheUpdate {
            action: TldrCacheAction::Cloned,
            cache_dir: Some("/cache/tldr".to_owned()),
            client: None,
            output: None,
            revision: Some("abc123".to_owned()),
        };

        assert_eq!(
            render_update_json(&update, false).expect("update JSON"),
            r#"{"action":"cloned","cacheDir":"/cache/tldr","revision":"abc123"}"#
        );
        assert!(
            render_update_json(&update, true)
                .expect("pretty update JSON")
                .contains("\n  \"cacheDir\": \"/cache/tldr\",")
        );
    }
}
