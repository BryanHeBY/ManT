//! The complete NetBSD 10.1 page and its individual ioctl owners.
use super::{check_queries, explain, fixture};
use mant_ir::{EntryKind, EntryOwner};

// The fixed CVS tree keeps each type in a reopened Dv element of its
// invocation. The UTF-8 projection retains their complete forms and bodies.
const IOCTLS: &[(u32, &[&str], &str, &str)] = &[
    (
        104,
        &["BIOCGBLEN"],
        "BIOCGBLEN (u_int)",
        "Returns the required buffer length for reads",
    ),
    (
        108,
        &["BIOCSBLEN"],
        "BIOCSBLEN (u_int)",
        "Sets the buffer length for reads on",
    ),
    (
        119,
        &["BIOCGDLT"],
        "BIOCGDLT (u_int)",
        "Returns the type of the data link",
    ),
    (
        127,
        &["BIOCGDLTLIST"],
        "BIOCGDLTLIST (struct bpf_dltlist)",
        "Returns an array of the available types",
    ),
    (
        157,
        &["BIOCSDLT"],
        "BIOCSDLT (u_int)",
        "Changes the type of the data link",
    ),
    (
        176,
        &["BIOCGETIF"],
        "BIOCGETIF (struct ifreq)",
        "Returns the name of the hardware interface",
    ),
    (
        181,
        &["BIOCSETIF"],
        "BIOCSETIF (struct ifreq)",
        "Sets the hardware interface associated with the",
    ),
    (
        190,
        &["BIOCSRTIMEOUT", "BIOCGRTIMEOUT"],
        "BIOCSRTIMEOUT, BIOCGRTIMEOUT (struct timeval)",
        "Sets or gets the read timeout parameter.",
    ),
    (
        199,
        &["BIOCGSTATS"],
        "BIOCGSTATS (struct bpf_stat)",
        "Returns the following structure of packet statistics:",
    ),
    (
        223,
        &["BIOCIMMEDIATE"],
        "BIOCIMMEDIATE (u_int)",
        "Enables or disables",
    ),
    (
        239,
        &["BIOCSETF"],
        "BIOCSETF (struct bpf_program)",
        "Sets the filter program used by the",
    ),
    (
        264,
        &["BIOCSETWF"],
        "BIOCSETWF (struct bpf_program)",
        "Sets the write filter program used by",
    ),
    (
        270,
        &["BIOCVERSION"],
        "BIOCVERSION (struct bpf_version)",
        "Returns the major and minor version numbers",
    ),
    (
        295,
        &["BIOCSRSIG", "BIOCGRSIG"],
        "BIOCSRSIG, BIOCGRSIG (u_int)",
        "Sets or gets the receive signal.",
    ),
    (
        301,
        &["BIOCGHDRCMPLT", "BIOCSHDRCMPLT"],
        "BIOCGHDRCMPLT, BIOCSHDRCMPLT (u_int)",
        "Sets or gets the status of the",
    ),
    (
        310,
        &["BIOCGSEESENT", "BIOCSSEESENT"],
        "BIOCGSEESENT, BIOCSSEESENT (u_int)",
        "These commands are obsolete but left for",
    ),
    (
        339,
        &["BIOCFEEDBACK", "BIOCSFEEDBACK", "BIOCGFEEDBACK"],
        "BIOCFEEDBACK, BIOCSFEEDBACK, BIOCGFEEDBACK (u_int)",
        "Set (or get)",
    ),
    (
        363,
        &["FIONREAD"],
        "FIONREAD (int)",
        "Returns the number of bytes that are",
    ),
    (
        365,
        &["FIONBIO"],
        "FIONBIO (int)",
        "Set or clear non-blocking I/O.",
    ),
    (
        377,
        &["FIOASYNC"],
        "FIOASYNC (int)",
        "Enable or disable async I/O.",
    ),
    (
        389,
        &["FIOSETOWN", "FIOGETOWN"],
        "FIOSETOWN, FIOGETOWN (int)",
        "Set or get the process or process",
    ),
];

#[test]
fn every_annotated_ioctl_keeps_its_own_symbols_and_body() {
    let content = fixture("netbsd-bpf.4");
    let items = super::super::definitions(content.document.as_ref().unwrap());
    for &(line, names, form, body) in IOCTLS {
        let item = items
            .iter()
            .find(|item| item.source.is_some_and(|source| source.line == line))
            .unwrap();
        assert_eq!(
            EntryOwner::Definition(item).facts().unwrap().kind,
            EntryKind::Term
        );
        check_queries(&content, line, form, names, body);
    }
    assert_eq!(IOCTLS.len(), 21);
    assert_eq!(
        IOCTLS
            .iter()
            .map(|(_, names, _, _)| names.len())
            .sum::<usize>(),
        28
    );
    for argument in ["int", "u_int", "struct", "timeval", "bpf_program"] {
        assert_eq!(
            explain(&content, argument).counts.direct_entry.total,
            0,
            "{argument}"
        );
    }
    // The fixed UTF-8 projection also contains two prose references to
    // BIOCGBLEN outside its declaration, but none for FIONREAD.
    for (name, mentions) in [("BIOCGBLEN", 2), ("FIONREAD", 0)] {
        assert_eq!(
            explain(&content, name).counts.context_mention.total,
            mentions
        );
    }
}
