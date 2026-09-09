//! Real host construction is lazy even when no fake service is injected.
use super::*;
use crate::{application, cli::run_with_host};
use mant_engine::LoadPolicy;
use mant_protocol::{
    CatalogQuery, InputFormat, QueryInput, QueryRequest, QueryView, RequestSchema,
};

fn invoke(host: &SystemHost, arguments: &[&str], input: &str) -> (u8, String, String) {
    let args = arguments
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect::<Vec<_>>();
    let mut output = Vec::new();
    let mut errors = Vec::new();
    let status = run_with_host(&args, &mut input.as_bytes(), &mut output, &mut errors, host);
    (
        status,
        String::from_utf8(output).unwrap(),
        String::from_utf8(errors).unwrap(),
    )
}

#[test]
fn offline_commands_and_invalid_inputs_do_not_capture_a_system_snapshot() {
    for (args, input, success) in [
        (vec!["--help"], "", true),
        (vec!["--schema", "request"], "", true),
        (vec!["--protocol-version"], "", true),
        (vec!["--unknown"], "", false),
        (
            vec!["tool", "--display", "tui", "--format", "json"],
            "",
            false,
        ),
        (vec!["--request-json"], "{", false),
        (
            vec!["--request-json"],
            r#"{"schema":"mant.request/v0.11","input":{"kind":"document","selector":"tool"},"view":{"kind":"excerpt","selectors":[]}}"#,
            false,
        ),
        (
            vec!["--input", "-", "--input-format", "markdown", "--manual"],
            "# Demo\n\nBody.",
            false,
        ),
        (
            vec![
                "--input",
                "-",
                "--input-format",
                "markdown",
                "--format",
                "json",
            ],
            "# Demo\n\nBody.",
            true,
        ),
    ] {
        let host = SystemHost::default();
        assert!(host.resolver.get().is_none());
        let (status, _, errors) = invoke(&host, &args, input);
        assert_eq!(status == 0, success, "{args:?}: {errors}");
        assert!(
            host.resolver.get().is_none(),
            "{args:?} captured source configuration"
        );
    }
}

#[test]
fn invalid_catalog_and_scope_requests_do_not_initialize_the_snapshot() {
    let host = SystemHost::default();
    assert!(
        host.discover(&CatalogQuery {
            pattern: Some("[".into()),
            syntax: mant_protocol::SearchSyntax::Regex,
            ..CatalogQuery::default()
        })
        .is_err()
    );
    assert!(host.resolver.get().is_none());
    assert!(
        host.resolve_scope(&mant_protocol::DocumentScope {
            documents: vec![],
            traversal: mant_protocol::DocumentTraversal::default(),
        })
        .is_err()
    );
    assert!(host.resolver.get().is_none());
}

#[test]
fn valid_complete_requests_reuse_one_host_snapshot_without_sharing_between_hosts() {
    // Packaged crate source provides an existing Markdown file without changing
    // process environment, registering a source, or writing a temporary fixture.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md");
    let mut request = QueryRequest {
        schema: RequestSchema::V0Dot11,
        input: QueryInput::File {
            path: path.to_string_lossy().into_owned(),
            format: InputFormat::Markdown,
        },
        view: QueryView::Full {},
    };
    let host = SystemHost::default();
    assert!(host.resolver.get().is_none());
    let full = application::execute_query(&request, LoadPolicy::Combined, &host).unwrap();
    assert!(matches!(full, QueryViewResult::Full(_)));
    let first = host
        .resolver
        .get()
        .expect("first valid request captures the snapshot");
    request.view = QueryView::Outline {
        entries: mant_protocol::EntryProjection::Summary,
        root: None,
        references: mant_protocol::ReferenceProjection::default(),
    };
    let outline = application::execute_query(&request, LoadPolicy::Combined, &host).unwrap();
    assert!(matches!(outline, QueryViewResult::Outline(_)));
    assert!(std::ptr::eq(first, host.resolver.get().unwrap()));

    let other = SystemHost::default();
    assert!(other.resolver.get().is_none());
    application::execute_query(&request, LoadPolicy::Combined, &other).unwrap();
    assert!(!std::ptr::eq(first, other.resolver.get().unwrap()));
}
