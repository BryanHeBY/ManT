//! Whole-page BSD option owners and the original native argument lists.
use super::{check_head, check_occurrences, check_queries, explain, fixture};
use mant_ir::{DefinitionItem, EntryKind, EntryOwner, ParameterKind, ResolvedContent};
use mant_protocol::EvidenceClass;

const OPTION: EntryKind = EntryKind::Parameter {
    parameter_kind: ParameterKind::Option,
};

#[test]
fn freebsd_ping_mode_arguments_remain_under_their_option() {
    // mdoc_macro.c::in_line closes Cm at the delimiter and reopens its
    // scope on time. The following It BODY still belongs to the -M option.
    let content = fixture("freebsd-ping.8.gz");
    let item = at_line(&content, 358);
    check_queries(
        &content,
        358,
        "-M mask | time",
        &["-M"],
        "Use ICMP_MASKREQ or ICMP_TSTAMP",
    );
    assert_eq!(EntryOwner::Definition(item).facts().unwrap().kind, OPTION);
    check_no_direct_owner(&content, 358, &["mask", "time"]);
}

#[test]
fn netbsd_patch_binds_each_proved_option_and_leaves_both_enumerations_visible() {
    let content = fixture("netbsd-patch.1");
    let item = at_line(&content, 282);
    check_queries(
        &content,
        282,
        "-V t | nil | never | none, --version-control t | nil | never | none",
        &["-V", "--version-control"],
        "Causes the next argument to be interpreted",
    );
    assert_eq!(EntryOwner::Definition(item).facts().unwrap().kind, OPTION);
    check_no_direct_owner(&content, 282, &["t", "nil", "never", "none"]);
}

#[test]
fn openbsd_openssl_pem_has_one_direct_owner_across_the_whole_page() {
    let content = fixture("openbsd-openssl.1");
    check_queries(&content, 6699, "pem", &["pem"], "Privacy Enhanced Mail");

    let argument_owner = at_line(&content, 260);
    check_head(argument_owner, "-inform der | pem | txt", &["-inform"]);
    assert_eq!(
        EntryOwner::Definition(argument_owner).facts().unwrap().kind,
        OPTION
    );
    let inform = explain(&content, "-inform");
    let direct = inform
        .evidence
        .iter()
        .find(|hit| hit.class == EvidenceClass::DirectEntry && hit.source == argument_owner.source)
        .unwrap();
    check_occurrences(direct, "-inform", std::slice::from_ref(&(0..7)));
    check_no_direct_owner(&content, 260, &["der", "pem", "txt"]);
    assert!(explain(&content, "pem").counts.entry_mention.total > 0);
}

fn at_line(content: &ResolvedContent, line: u32) -> &DefinitionItem {
    super::super::definitions(content.document.as_ref().unwrap())
        .into_iter()
        .find(|item| item.source.is_some_and(|source| source.line == line))
        .unwrap()
}

fn check_no_direct_owner(content: &ResolvedContent, line: u32, arguments: &[&str]) {
    for argument in arguments {
        let response = explain(content, argument);
        assert_eq!(
            response.counts.direct_entry.total,
            response.counts.direct_entry.returned
        );
        assert!(
            !response.evidence.iter().any(|hit| {
                hit.class == EvidenceClass::DirectEntry
                    && hit.source.is_some_and(|span| span.line == line)
            }),
            "{line}: {argument}"
        );
    }
}
