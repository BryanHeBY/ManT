//! Opaque environmental templates and concrete aliases in bsd.port.mk(5).
use super::{check_queries, explain, fixture};
use mant_ir::{EntryKind, EntryOwner, inline_plain_text};
use mant_protocol::{ContentSelector, EvidenceClass};

#[test]
fn port_installation_templates_keep_their_text_and_owner_without_naming_members() {
    // OpenBSD 7.9 retains both full templates within one native Ev
    // word. The comma-delimited architecture variables are distinct names.
    let query = fixture("openbsd-bsd.port.mk.5");
    let document = query.document.as_ref().unwrap();
    let items = super::super::definitions(document);
    for (line, form, description) in [
        (
            1253,
            "BSD_INSTALL_{PROGRAM,SCRIPT,DATA,MAN}[_DIR]",
            "Macros passed to make and configure invocations.",
        ),
        (
            2242,
            "INSTALL_{PROGRAM,SCRIPT,DATA,MAN}[_DIR]",
            "Macros to use to install a program, a script, data, or a man page",
        ),
    ] {
        let item = items
            .iter()
            .find(|item| item.source.is_some_and(|source| source.line == line))
            .unwrap();
        let owner = EntryOwner::Definition(item);
        assert_eq!(owner.facts().unwrap().kind, EntryKind::EnvironmentVariable);
        assert_eq!(owner.validated_names().unwrap(), Vec::<String>::new());
        assert_eq!(inline_plain_text(&item.terms[0]), form);
        let result = explain(&query, form);
        assert_eq!(result.counts.direct_entry.total, 1);
        let hit = result
            .evidence
            .iter()
            .find(|hit| hit.class == EvidenceClass::DirectEntry)
            .unwrap();
        assert_eq!(hit.source, item.source);
        let excerpt =
            mant_query::select_excerpt(&query, &[ContentSelector::path(hit.outline.path())])
                .unwrap();
        assert!(mant_render::render_excerpt_text(&excerpt).contains(description));
    }
    for member in ["SCRIPT", "DATA"] {
        assert_eq!(explain(&query, member).counts.direct_entry.total, 0);
    }
    check_queries(
        &query,
        1531,
        "CLANG_ARCHS, GCC4_ARCHS",
        &["CLANG_ARCHS", "GCC4_ARCHS"],
        "List of architectures using Clang or GCC 4.2.1 as the base compiler.",
    );
}
