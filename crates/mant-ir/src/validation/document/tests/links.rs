//! Existing links invariant contracts.
use super::*;

#[test]
fn validates_external_uri_structure() {
    for uri in [
        "https:relative",
        "https:///missing-host",
        "https://example.test:",
        "https://[::1",
        "https://[::1]:invalid",
        "https://%ZZ@example.test/path",
        "https://example.test/%ZZ",
        "https://user]name@example.test/path",
        "https://example%ZZ.test/path",
        "https://example..test/path",
        "https://例.example/path",
        "https://example.test/path#one#two",
        "mailto:",
        "mailto:?subject=x",
        "mailto:a..b@example.test",
        "mailto:.a@example.test",
        "mailto:a.@example.test",
        "mailto:user%ZZ@example.test",
        "mailto:%2Euser@example.test",
        "mailto:user%2E%2Ename@example.test",
        "mailto:user%40evil@example.test",
        "mailto:user%2Csecond@example.test",
        "mailto:%80@example.test",
        "mailto:%2Euser@example.test?subject=x",
        "mailto:user%2E%2Ename@example.test?subject=x",
        "mailto:user%40evil@example.test?subject=x",
        "mailto:%2Euser@example.test#fragment",
    ] {
        assert!(!is_valid_external_uri(uri), "accepted invalid URI {uri}");
    }
    for uri in [
        "https://example.test/path",
        "https://user@example.test:443/path",
        "https://user%40name@example.test/path",
        "https://[::1]:8443/path",
        "https://[::1]:8443/path?q=x#part",
        "https://service_name.example.test/path",
        "https://example.test./path",
        "https://ex%41mple.test/path",
        "https://xn--fsq.example/path",
        "mailto:user@example.test",
        "mailto:user@example.test?subject=hello",
        "mailto:user%25tag@example.test",
        "mailto:a%2Fb@example.test",
        "mailto:user@example.test,second@example.test",
        "mailto:user%252Etag@example.test",
    ] {
        assert!(is_valid_external_uri(uri), "rejected valid URI {uri}");
    }
}

#[test]
fn validates_email_and_mailto_round_trips() {
    for address in [
        "",
        "missing-domain",
        "@example.test",
        "docs@",
        ".docs@example.test",
        "docs.@example.test",
        "docs..team@example.test",
        "quoted\"name@example.test",
    ] {
        assert!(
            !is_valid_email_address(address),
            "accepted invalid email address {address}"
        );
    }
    for address in [
        "docs@example.test",
        "support@sub.example.test",
        "build+notifications@example.test",
    ] {
        assert!(
            is_valid_email_address(address),
            "rejected valid email address {address}"
        );
    }

    for (uri, address) in [
        ("mailto:docs@example.test", "docs@example.test"),
        ("MAILTO:user%25tag@example.test", "user%tag@example.test"),
        ("mailto:a%2Fb@example.test", "a/b@example.test"),
        (
            "mailto:user%252Etag@example.test",
            "user%2Etag@example.test",
        ),
    ] {
        let (_, remainder) = uri.split_once(':').expect("mailto URI has a scheme");
        let canonical_uri = format!("mailto:{remainder}");
        assert_eq!(
            email_address_from_mailto_uri(uri).as_deref(),
            Some(address),
            "failed to decode {uri}"
        );
        assert_eq!(
            mailto_uri_for_email_address(address).as_deref(),
            Some(canonical_uri.as_str()),
            "failed to serialize {address}"
        );
    }
    for uri in [
        "mailto:%2Euser@example.test",
        "mailto:user%2E%2Ename@example.test",
        "mailto:user%40evil@example.test",
        "mailto:user%2Csecond@example.test",
        "mailto:user@example.test,second@example.test",
        "mailto:user@example.test?subject=x",
        "mailto:user@example.test#fragment",
    ] {
        assert!(
            email_address_from_mailto_uri(uri).is_none(),
            "classified non-typed mailto URI {uri}"
        );
    }
}
