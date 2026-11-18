use std::{io::Read, path::Path};

use mant_ir::{EntryOwner, ResolvedContent};
use mant_protocol::{ContentSelector, EvidenceClass};

use super::support::{check_head, check_occurrences, explain, json_roundtrip};

#[path = "bsd_symbols/netbsd_bpf.rs"]
mod netbsd_bpf;

fn fixture(name: &str) -> ResolvedContent {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real/bsd-declarations")
        .join(name);
    json_roundtrip::json_string_roundtrip(&ResolvedContent {
        document: Some(mant_loader::parse_manual_source(&path).unwrap()),
        address: None,
        label: name.into(),
        tldr: None,
    })
}

#[test]
fn every_numbered_freebsd_errno_head_binds_its_symbol_at_the_original_line() {
    // CVS mdoc_macro.c::in_line keeps the ordinal and symbol inside one Er.
    // The separate Em description is still part of that same definition HEAD.
    let mut source = String::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("../../../../tests/fixtures/roff/real/bsd-declarations/freebsd-errno.2.gz")
            [..],
    )
    .read_to_string(&mut source)
    .unwrap();
    let content = fixture("freebsd-errno.2.gz");
    let items = super::definitions(content.document.as_ref().unwrap());
    let mut checked = 0;
    for (index, line) in source.lines().enumerate() {
        let Some(label) = line.strip_prefix(".It Er ") else {
            continue;
        };
        let mut words = label.split_whitespace();
        let (Some(number), Some(name)) = (words.next(), words.next()) else {
            continue;
        };
        if number == "0" || number.parse::<usize>().is_err() {
            continue;
        }
        let line = u32::try_from(index + 1).unwrap();
        let item = items
            .iter()
            .find(|item| item.source.is_some_and(|source| source.line == line))
            .expect("original error owner");
        let owner = EntryOwner::Definition(item);
        assert_eq!(owner.validated_names().unwrap(), [name], "line {line}");
        let form = mant_ir::inline_plain_text(&item.terms[0]);
        assert!(form.starts_with(&format!("{number} {name} ")), "{form}");
        checked += 1;
    }
    assert_eq!(checked, 95);
    check_queries(
        &content,
        379,
        "1 EPERM Operation not permitted.",
        &["EPERM"],
        "An attempt was made",
    );
}

#[test]
fn bsd_ioctl_constants_keep_types_out_of_names_and_parallel_subjects_in() {
    // CVS -Ttree keeps the unseparated int in the Dv invocation, splits
    // PPIGDATA and PPISDATA at stand-alone punctuation, and places the
    // syscons pointer argument in an independent Ar invocation.
    for (file, line, form, names, body) in [
        (
            "freebsd-ioctl.2.gz",
            84,
            "FIONREAD int",
            &["FIONREAD"][..],
            "Get the number of bytes",
        ),
        (
            "freebsd-ppi.4.gz",
            58,
            "PPIGDATA, PPISDATA",
            &["PPIGDATA", "PPISDATA"][..],
            "contents of the data register",
        ),
        (
            "dragonflybsd-sc.4.gz",
            515,
            "GIO_COLOR int *color",
            &["GIO_COLOR"][..],
            "Get the current text color.",
        ),
    ] {
        let content = fixture(file);
        check_queries(&content, line, form, names, body);
        let mentions = explain(&content, "int");
        assert_eq!(mentions.counts.direct_entry.total, 0, "{file}");
    }
}

#[test]
fn parenthesized_bsd_constant_notes_keep_their_original_names_and_owners() {
    // Pinned CVS mdoc_macro.c::in_line distinguishes the Dv subject
    // from the following Pq/Ic alias and from literal delimiters around Sy.
    let netgraph = fixture("freebsd-ng_deflate.4.gz");
    check_queries(
        &netgraph,
        83,
        "NGM_DEFLATE_CONFIG (config)",
        &["NGM_DEFLATE_CONFIG"],
        "This command resets and configures the node",
    );
    let acl = fixture("freebsd-archive_entry_acl.3.gz");
    check_queries(
        &acl,
        124,
        "ARCHIVE_ENTRY_ACL_READ (r)",
        &["ARCHIVE_ENTRY_ACL_READ"],
        "ARCHIVE_ENTRY_ACL_READ (r)",
    );
    check_queries(
        &acl,
        214,
        "ARCHIVE_ENTRY_ACL_READ_DATA (r)",
        &["ARCHIVE_ENTRY_ACL_READ_DATA"],
        "Read data (file).",
    );
    let items = super::definitions(acl.document.as_ref().unwrap());
    let read = items
        .iter()
        .find(|item| item.source.is_some_and(|source| source.line == 124))
        .unwrap();
    assert_eq!(read.description.len(), 0);
}

fn check_queries(content: &ResolvedContent, line: u32, form: &str, names: &[&str], body: &str) {
    let items = super::definitions(content.document.as_ref().unwrap());
    let item = items
        .iter()
        .find(|item| item.source.is_some_and(|source| source.line == line))
        .unwrap();
    check_head(item, form, names);
    let mut expected_path = None;
    for &name in names {
        let explanation = explain(content, name);
        assert_eq!(explanation.counts.direct_entry.total, 1, "{name}");
        let direct = explanation
            .evidence
            .iter()
            .find(|hit| hit.class == EvidenceClass::DirectEntry)
            .unwrap();
        assert_eq!(direct.source, item.source);
        let start = form.find(name).unwrap();
        let range = start..start + name.len();
        check_occurrences(direct, name, std::slice::from_ref(&range));
        let path = direct.outline.path();
        assert_eq!(
            path,
            expected_path
                .get_or_insert_with(|| path.to_owned())
                .as_str()
        );
        let by_path = mant_query::select_excerpt(content, &[ContentSelector::path(path)]).unwrap();
        assert_eq!(
            by_path,
            mant_query::select_excerpt(content, &[ContentSelector::id(direct.outline.node.id())])
                .unwrap(),
        );
        assert!(
            mant_render::render_excerpt_text(&by_path).contains(body),
            "{name}: {body}"
        );
    }
}
