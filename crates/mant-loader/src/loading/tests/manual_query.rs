//! Source selection tests use index placeholders; no formatter is executed.

use super::*;
use crate::ManualLookupMode;

struct Fixture(PathBuf);

impl Fixture {
    fn new(label: &str, sections: &[&str]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "mant-loader-query-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        for section in sections {
            let directory = root.join(format!("man{section}"));
            std::fs::create_dir_all(&directory).expect("manual directory");
            std::fs::write(
                directory.join(format!("tool.{section}")),
                b"index placeholder",
            )
            .expect("manual index source");
        }
        Self(root)
    }

    fn host(&self, document: Result<Document, String>) -> StubHost {
        let mut host = host(document);
        host.manual_index = Some(crate::ManualIndex::from_roots(vec![self.0.clone()]));
        host
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove lookup fixture");
    }
}

#[test]
fn unresolved_major_queries_and_exact_catalog_addresses_keep_distinct_intent() {
    let fixture = Fixture::new("intent", &["1p"]);
    let host = fixture.host(Ok(document(SourceFormat::Man, false, true)));
    for _ in 0..2 {
        let missing = super::super::load_with(
            LoadSpec::Document {
                selector: "manual/1/tool",
                source: None,
                manual_section: None,
            },
            LoadPolicy::ManualOnly,
            &host,
        );
        assert!(matches!(
            missing,
            Err(LoadError::Manual(crate::ManualLoadError::NotFound { .. }))
        ));
        let selected = super::super::load_with(
            LoadSpec::Document {
                selector: "tool",
                source: None,
                manual_section: Some("1"),
            },
            LoadPolicy::ManualOnly,
            &host,
        )
        .expect("major query finds installed extension");
        assert_eq!(
            selected.address,
            Some(DocumentAddress::Manual {
                name: "tool".into(),
                manual_section: "1p".into(),
            })
        );
        assert_eq!(
            selected.document.unwrap().meta.manual_section.as_deref(),
            Some("1p")
        );
    }
    let requests = host.manual_requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.lookup_mode)
            .collect::<Vec<_>>(),
        [
            ManualLookupMode::Exact,
            ManualLookupMode::Compatible,
            ManualLookupMode::Exact,
            ManualLookupMode::Compatible,
        ]
    );
}

#[test]
fn selected_category_controls_metadata_and_tldr_for_every_native_policy() {
    for section in ["1p", "3p", "8x"] {
        let fixture = Fixture::new("selected-identity", &[section]);
        for policy in [
            LoadPolicy::Combined,
            LoadPolicy::ManualWithTldr,
            LoadPolicy::ManualOnly,
        ] {
            for (selector, category) in [
                ("tool".to_owned(), Some(&section[..1])),
                (format!("manual/{section}/tool"), None),
            ] {
                let mut host = fixture.host(Ok(document(SourceFormat::Man, false, true)));
                host.tldr = Ok(Some(tldr()));
                let result = super::super::load_with(
                    LoadSpec::Document {
                        selector: &selector,
                        source: None,
                        manual_section: category,
                    },
                    policy,
                    &host,
                )
                .expect("selected native manual");
                assert_eq!(
                    result.address,
                    Some(DocumentAddress::Manual {
                        name: "tool".into(),
                        manual_section: section.into(),
                    })
                );
                assert_eq!(
                    result.document.unwrap().meta.manual_section.as_deref(),
                    Some(section)
                );
                assert_eq!(
                    result.tldr.is_some(),
                    policy != LoadPolicy::ManualOnly && section != "3p"
                );
            }
        }
    }
}

#[test]
fn exact_category_wins_and_parse_or_empty_failure_never_retries_an_extension() {
    let fixture = Fixture::new("no-read-fallback", &["1", "1p"]);
    for direct in [
        Err("read or parse failed".into()),
        Ok(document(SourceFormat::Man, false, false)),
    ] {
        let host = fixture.host(direct);
        let failure = super::super::load_with(
            LoadSpec::Document {
                selector: "tool",
                source: None,
                manual_section: Some("1"),
            },
            LoadPolicy::ManualOnly,
            &host,
        )
        .expect_err("selected source failure is final");
        assert!(matches!(
            failure,
            LoadError::Manual(
                crate::ManualLoadError::Parse { .. } | crate::ManualLoadError::Empty { .. }
            )
        ));
        assert_eq!(*host.calls.lock().unwrap(), ["locate", "parse"]);
        let parsed = host.parsed_manuals.lock().unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].section, "1");
    }
}

#[test]
fn query_suffix_and_exact_catalog_names_do_not_retarget_aliases() {
    let fixture = Fixture::new("suffix-strict", &["1p"]);
    let mut host = fixture.host(Ok(document(SourceFormat::Man, false, true)));
    host.name_candidates = Some(vec!["missing".into(), "tool".into()]);
    for spec in [
        LoadSpec::Document {
            selector: "tool",
            source: None,
            manual_section: Some("1m"),
        },
        LoadSpec::Document {
            selector: "tool",
            source: None,
            manual_section: Some("3"),
        },
        LoadSpec::Document {
            selector: "manual/1p/missing",
            source: None,
            manual_section: None,
        },
    ] {
        assert!(matches!(
            super::super::load_with(spec, LoadPolicy::ManualOnly, &host),
            Err(LoadError::Manual(crate::ManualLoadError::NotFound { .. }))
        ));
    }
    assert!(host.parsed_manuals.lock().unwrap().is_empty());
    let last = host.manual_requests.lock().unwrap().last().unwrap().clone();
    assert_eq!(last, ManualRequest::new("missing", Some("1p".into())));
}
