//! Native source selection is independent of optional command quick references.
use super::*;

#[test]
fn native_with_tldr_keeps_manual_selection_and_command_category_rules() {
    for section in ["1", "1p", "8", "8x", "3", "5", "7"] {
        let catalog = format!("manual/{section}/tool");
        for (selector, manual_section) in [
            ("tool", None),
            ("tool", Some(section)),
            (catalog.as_str(), None),
        ] {
            for cached in [
                Ok(Some(tldr())),
                Ok(None),
                Err("invalid cached page".into()),
            ] {
                let mut host = host(Ok(document(SourceFormat::Man, false, true)));
                host.locate.as_mut().unwrap().section = section.into();
                host.registered_document = Some("/data/tool.md".into());
                host.markdown = Ok(embedded_tldr_markdown());
                host.tldr = cached.clone();
                let spec = LoadSpec::Document {
                    selector,
                    source: None,
                    manual_section,
                };
                let result = super::super::load_with(spec, LoadPolicy::ManualWithTldr, &host)
                    .expect("native manual remains available");
                assert_eq!(
                    result.address,
                    Some(DocumentAddress::Manual {
                        name: "tool".into(),
                        manual_section: section.into(),
                    })
                );
                assert_eq!(result.document.unwrap().source.format, SourceFormat::Man);
                let command = matches!(section, "1" | "1p" | "8" | "8x");
                assert_eq!(
                    result.tldr,
                    if command { cached.ok().flatten() } else { None }
                );
                assert_eq!(
                    *host.calls.lock().unwrap(),
                    if command {
                        vec!["locate", "parse", "tldr"]
                    } else {
                        vec!["locate", "parse"]
                    },
                    "registered Markdown never replaces the native target"
                );
            }
        }
    }
}

#[test]
fn native_with_tldr_does_not_fall_back_when_the_manual_is_missing_or_invalid() {
    for failure in ["missing", "invalid", "unavailable"] {
        let mut host = host(Ok(document(SourceFormat::Man, false, true)));
        host.registered_document = Some("/data/tool.md".into());
        host.markdown = Ok(embedded_tldr_markdown());
        host.tldr = Ok(Some(tldr()));
        match failure {
            "missing" => host.locate = Err("missing native target".into()),
            "invalid" => host.direct = Err("invalid native target".into()),
            "unavailable" => host.native_available = false,
            _ => unreachable!(),
        }
        let error = load_request(&request(), LoadPolicy::ManualWithTldr, &host)
            .expect_err("optional content cannot replace the manual");
        let expected = match failure {
            "missing" => LoadError::ManualWithTldr {
                error: crate::ManualLoadError::NotFound {
                    name: "tool".into(),
                    detail: "missing native target".into(),
                },
                topic: "tool".into(),
            },
            "invalid" => LoadError::ManualWithTldr {
                error: crate::ManualLoadError::Parse {
                    name: "tool".into(),
                    detail: "invalid native target".into(),
                },
                topic: "tool".into(),
            },
            "unavailable" => LoadError::NativeBackendUnavailable { tldr_topic: None },
            _ => unreachable!(),
        };
        assert_eq!(error, expected, "cached tldr is only a hint on failure");
        let calls = host.calls.lock().unwrap();
        assert!(!calls.contains(&"markdown"));
        assert!(!calls.contains(&"name"));
        assert!(!calls.contains(&"fallback"));
    }
}

#[test]
fn native_policies_reject_registered_sources_and_direct_files_before_reads() {
    for policy in [LoadPolicy::ManualOnly, LoadPolicy::ManualWithTldr] {
        let host = host(Ok(document(SourceFormat::Man, false, true)));
        for spec in [
            LoadSpec::Document {
                selector: "tool",
                source: Some("source"),
                manual_section: None,
            },
            LoadSpec::Document {
                selector: "documents/tool",
                source: None,
                manual_section: None,
            },
            LoadSpec::File {
                path: "never-read.md",
                format: InputFormat::Markdown,
            },
        ] {
            assert!(super::super::load_with(spec, policy, &host).is_err());
            assert!(host.calls.lock().unwrap().is_empty());
        }
    }
}
