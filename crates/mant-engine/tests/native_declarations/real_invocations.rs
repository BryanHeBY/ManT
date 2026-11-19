//! Complete release manuals exercise invocation declarations and nested owners.
use std::path::Path;

use mant_ir::{EntryKind, ParameterKind, ResolvedContent};
use mant_protocol::{ContentSelector, EvidenceBasis, EvidenceClass};

use super::support::{check_head, explain, explanation_ranges, json_roundtrip, roundtrip};

fn fixture(name: &str) -> ResolvedContent {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real/native-invocations")
        .join(name);
    let resolved = ResolvedContent {
        document: Some(mant_loader::parse_manual_source(&path).unwrap()),
        address: None,
        label: name.into(),
        tldr: None,
    };
    let content = json_roundtrip::json_string_roundtrip(&resolved);
    assert_eq!(content.document, resolved.document);
    assert_eq!(
        mant_ir::validate_document(content.document.as_ref().unwrap()),
        []
    );
    content
}

fn direct_lines(content: &ResolvedContent, name: &str, expected: &[u32]) {
    let explanation = roundtrip(&explain(content, name));
    assert_eq!(
        explanation.counts.direct_entry.total,
        u32::try_from(expected.len()).unwrap(),
        "{name}"
    );
    let lines = explanation
        .evidence
        .iter()
        .filter(|hit| hit.class == EvidenceClass::DirectEntry)
        .map(|hit| hit.source.unwrap().line)
        .collect::<Vec<_>>();
    assert_eq!(lines, expected, "{name}");
    if !expected.is_empty() {
        assert!(
            explanation_ranges::assert_ranges(&explanation, name) > 0,
            "{name}"
        );
    }
}

fn check_owner(
    content: &ResolvedContent,
    line: u32,
    name: &str,
    form: &str,
    kind: EntryKind,
    body: &str,
) {
    let owner = super::definitions(content.document.as_ref().unwrap())
        .into_iter()
        .find(|item| item.source.is_some_and(|span| span.line == line))
        .expect("original definition");
    check_head(owner, form, &[name]);
    let name_lookup = explain(content, name);
    let named = name_lookup
        .evidence
        .iter()
        .find(|hit| hit.class == EvidenceClass::DirectEntry && hit.source == owner.source)
        .unwrap();
    assert_eq!(named.entry.as_ref().unwrap().kind, kind);
    assert!(
        named
            .bases
            .iter()
            .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
    );
    let form_lookup = roundtrip(&explain(content, form));
    let formed = form_lookup
        .evidence
        .iter()
        .find(|hit| hit.class == EvidenceClass::DirectEntry && hit.source == owner.source)
        .unwrap();
    assert_eq!(formed.outline.path(), named.outline.path());
    assert!(
        formed
            .bases
            .iter()
            .any(|basis| matches!(basis, EvidenceBasis::Form { .. }))
    );
    assert!(explanation_ranges::assert_ranges(&form_lookup, form) > 0);
    let excerpt =
        mant_query::select_excerpt(content, &[ContentSelector::path(formed.outline.path())])
            .unwrap();
    assert!(
        mant_render::render_excerpt_text(&excerpt).contains(body),
        "{line} {body}"
    );
}

#[test]
fn freebsd_dtrace_keeps_the_assignment_values_on_the_evaltime_owner() {
    // CVS tree reopens Cm at the middle delimiters while Sm off leaves
    // the complete assignment unspaced in both terminal devices.
    let content = fixture("freebsd-dtrace.1.gz");
    for value in ["preinit", "postinit", "main"] {
        direct_lines(&content, value, &[]);
    }
    direct_lines(&content, "evaltime", &[632]);
    check_owner(
        &content,
        632,
        "evaltime",
        "evaltime=exec|preinit|postinit|main",
        EntryKind::ConfigurationKey,
        "Process create mode.",
    );
}

#[test]
fn freebsd_zfs_allow_keeps_operand_alternatives_in_each_original_form() {
    // CVS places Ar alternatives and the nested optional group in the
    // extended HEAD belonging to the Nm/Cm invocation.
    let content = fixture("freebsd-zfs-allow.8.gz");
    direct_lines(&content, "volume", &[]);
    direct_lines(&content, "zfs allow", &[98, 120, 129, 290, 304]);
    check_owner(
        &content,
        129,
        "zfs allow",
        "zfs allow [-dl] -e|everyone perm|@setname[,perm|@setname]… filesystem|volume",
        EntryKind::Command,
        "Delegates ZFS administration permission",
    );
}

#[test]
fn freebsd_zfs_mount_keeps_the_flag_owner_independent_from_the_invocation() {
    // The two Fl R nodes have different CVS parents. One is in the
    // invocation HEAD and the other has its own It HEAD and BODY.
    let content = fixture("freebsd-zfs-mount.8.gz");
    direct_lines(&content, "-R", &[93]);
    direct_lines(&content, "zfs mount", &[56, 66]);
    check_owner(
        &content,
        56,
        "zfs mount",
        "zfs mount [-j]",
        EntryKind::Command,
        "Displays all ZFS file systems currently mounted.",
    );
    check_owner(
        &content,
        66,
        "zfs mount",
        "zfs mount [-Oflv] [-o options] -a|-R filesystem|filesystem",
        EntryKind::Command,
        "Mount ZFS filesystem on a path",
    );
    check_owner(
        &content,
        93,
        "-R",
        "-R",
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Option,
        },
        "Mount the specified filesystems along with all their children.",
    );
}

#[test]
fn freebsd_zfs_create_binds_both_original_command_forms() {
    // Fixed tree has separate Nm and Cm children at both extended HEADs.
    // termp_nm_pre() and termp_bold_pre() print that prefix before Op/Ar.
    let content = fixture("freebsd-zfs-create.8.gz");
    direct_lines(&content, "zfs create", &[55, 134]);
    for (line, form, body) in [
        (
            55,
            "zfs create [-Pnpuv] [-o property=value]… filesystem",
            "Creates a new ZFS file system.",
        ),
        (
            134,
            "zfs create [-ps] [-b blocksize] [-o property=value]… -V size volume",
            "Creates a volume of the given size.",
        ),
    ] {
        check_owner(&content, line, "zfs create", form, EntryKind::Command, body);
    }
}

#[test]
fn freebsd_zfs_send_and_redact_bind_each_original_command_form() {
    // The pinned HEADs execute the same Nm/Cm prefix before either the
    // extended option syntax or the standalone resume-token arguments.
    let content = fixture("freebsd-zfs-send.8.gz");
    direct_lines(&content, "zfs send", &[75, 319, 462, 557, 572]);
    direct_lines(&content, "zfs redact", &[590]);
    for (line, form, body) in [
        (
            75,
            "zfs send [-DLPVbcehnpsvw] [-R [-X dataset[,dataset]…]] [[-I|-i] snapshot] snapshot",
            "Creates a stream representation of the second snapshot",
        ),
        (
            319,
            "zfs send [-DLPVcenvw] [-i snapshot|bookmark] filesystem|volume|snapshot",
            "Generate a send stream, which may be of a filesystem",
        ),
        (
            462,
            "zfs send --redact redaction_bookmark [-DLPVcenpv] [-i snapshot|bookmark] snapshot",
            "Generate a redacted send stream.",
        ),
        (
            557,
            "zfs send [-PVenv] -t receive_resume_token",
            "Creates a send stream which resumes an interrupted receive.",
        ),
        (
            572,
            "zfs send [-PVnv] [-i snapshot|bookmark] -S filesystem",
            "Generate a send stream from a dataset that has been partially received.",
        ),
    ] {
        check_owner(&content, line, "zfs send", form, EntryKind::Command, body);
    }
    check_owner(
        &content,
        590,
        "zfs redact",
        "zfs redact snapshot redaction_bookmark redaction_snapshot…",
        EntryKind::Command,
        "Generate a new redaction bookmark.",
    );
}

#[test]
fn illumos_zfs_keeps_the_volume_term_and_compound_invocations_independent() {
    // CVS uses Sy for the documented object and Ar for the invocation
    // operands. The argument copies still belong to their command forms.
    let content = fixture("illumos-zfs.8");
    direct_lines(&content, "volume", &[357]);
    direct_lines(&content, "zfs allow", &[3970, 3980, 3989, 4133, 4147]);
    check_owner(
        &content,
        357,
        "volume",
        "volume",
        EntryKind::Term,
        "A logical volume exported as a raw or block device.",
    );
    check_owner(
        &content,
        3989,
        "zfs allow",
        "zfs allow [-dl] -e|everyone perm|@setname[,perm|@setname]... filesystem|volume",
        EntryKind::Command,
        "Delegates ZFS administration permission",
    );
}
