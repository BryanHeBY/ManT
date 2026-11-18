//! Test native expectations against the parsed product and deliberate loss.

use mant_ir::{Document, Inline, LinkTarget, ResolvedContent, inline_plain_text, visit::VisitMut};

use super::parsed_structure;
use crate::{compare_structure, ir_profile};

const MAN_HEADER: &str = ".TH VALIDATE 1 \"November 18, 2026\"\n.SH DESCRIPTION\n";
const MDOC_HEADER: &str = concat!(
    ".Dd November 18, 2026\n.Dt VALIDATE 1\n.Os\n",
    ".Sh NAME\n.Nm validate\n.Nd address audit\n.Sh DESCRIPTION\n",
);

#[test]
fn link_obligations_keep_native_occurrences_and_operands() {
    // CVS mdoc_mt_pre visits every Mt operand. The Xr/MR and Lk/UR handlers
    // emit each distinct native node, even when macro expansion reuses its
    // source coordinate or two nodes have the same destination.
    for (head, body, property) in [
        (
            MDOC_HEADER,
            ".Mt first@example.test second@example.test\n",
            "email-links",
        ),
        (
            MDOC_HEADER,
            ".Mt team@example.test team@example.test\n",
            "email-links",
        ),
        (
            MDOC_HEADER,
            ".Mt first@example.test , second@example.test .\n",
            "email-links",
        ),
        (
            MDOC_HEADER,
            ".de LX\n.Mt first@example.test\n.Mt second@example.test\n..\n.LX\n",
            "email-links",
        ),
        (
            MDOC_HEADER,
            ".de LX\n.Xr ls 1\n.Xr ed 1\n..\n.LX\n",
            "manual-links",
        ),
        (
            MAN_HEADER,
            ".de LX\n.MR ls 1\n.MR ed 1\n..\n.LX\n",
            "manual-links",
        ),
        (
            MDOC_HEADER,
            ".de LX\n.Lk https://example.test FIRST\n.Lk https://example.test SECOND\n..\n.LX\n",
            "external-links",
        ),
        (
            MAN_HEADER,
            ".de LX\n.UR https://example.test\nFIRST\n.UE\n.UR https://example.test\nSECOND\n.UE\n..\n.LX\n",
            "external-links",
        ),
    ] {
        assert_address_loss(head, body, property, 2);
    }
}

#[test]
fn explicit_manual_and_reference_field_links_are_independent_obligations() {
    // man_MR_pre emits the first operand with its section, and mdoc__x_pre
    // emits %U as a URI and expands only the raw RFC number spelling in %R.
    for (head, body, property) in [
        (MAN_HEADER, ".MR ls 1\n", "manual-links"),
        (
            MDOC_HEADER,
            ".Rs\n.%T Citation\n.%U https://example.org/\n.Re\n",
            "external-links",
        ),
        (
            MDOC_HEADER,
            ".Rs\n.%T Citation\n.%U mailto\\&:user@example.test\n.Re\n",
            "email-links",
        ),
        (MDOC_HEADER, ".Rs\n.%R RFC 123\n.Re\n", "external-links"),
    ] {
        assert_address_loss(head, body, property, 1);
    }
    assert!(violations(MDOC_HEADER, ".Rs\n.%R RFC prologue\n.Re\n").is_empty());
}

#[test]
fn decoded_empty_addresses_allow_readable_labels_without_links() {
    // CVS HTML leaves an empty href (or bare mailto:) after encoding \&.
    // Those operands cannot support a typed destination in the reading IR.
    for (head, body, labels) in [
        (
            MDOC_HEADER,
            ".Mt \\&\n.Mt \\& second@example.test\n",
            &["second@example.test"][..],
        ),
        (MDOC_HEADER, ".Lk \\& LABEL\n", &["LABEL"][..]),
        (
            MAN_HEADER,
            ".UR \\&\nLABEL\n.UE\n.MT \\&\nCONTACT\n.ME\n",
            &["LABEL", "CONTACT"][..],
        ),
        (
            MDOC_HEADER,
            ".Rs\n.%T Citation\n.%U \\&\n.Re\n",
            &["Citation"][..],
        ),
    ] {
        let (expected, native, document) = parsed_structure(&[head, body].concat());
        let (observed, topology) = ir_profile(&document);
        assert!(
            compare_structure(&expected, &observed, &native, &topology).is_empty(),
            "{body}"
        );
        let email_count = usize::from(body.contains("second@example.test"));
        assert_eq!(
            (
                expected.external_links,
                expected.email_links,
                expected.unclassified_address_links
            ),
            (0, email_count, 0)
        );
        assert_eq!(
            (observed.external_links, observed.email_links),
            (0, email_count)
        );
        let visible = mant_render::render_query_text(&ResolvedContent {
            label: "fixture".to_owned(),
            address: None,
            document: Some(document),
            tldr: None,
        });
        for label in labels {
            assert!(visible.contains(label), "{body}: {visible}");
        }
    }
}

#[test]
fn a_valid_address_does_not_require_printable_label_text() {
    for (head, body, property) in [
        (
            MDOC_HEADER,
            ".Lk https://example.test \\&\n",
            "external-links",
        ),
        (
            MAN_HEADER,
            ".UR https://example.test\n\\&\n.UE\n",
            "external-links",
        ),
        (
            MDOC_HEADER,
            ".Lk mailto\\&:team@example.test LABEL\n",
            "email-links",
        ),
        (
            MAN_HEADER,
            ".UR mailto\\&:team@example.test\nLABEL\n.UE\n",
            "email-links",
        ),
    ] {
        assert_address_loss(head, body, property, 1);
    }
}

#[test]
fn recovered_operand_only_requires_a_link_in_the_combined_address_count() {
    let body = ".Lk https://example.test/\\[notaglyph] LABEL\n";
    let (expected, native, document) = parsed_structure(&[MDOC_HEADER, body].concat());
    assert_eq!(expected.unclassified_address_links, 1);
    assert_eq!((expected.external_links, expected.email_links), (0, 0));
    let (observed, topology) = ir_profile(&document);
    assert!(compare_structure(&expected, &observed, &native, &topology).is_empty());
    let serialized = serde_json::to_value(&expected).unwrap();
    assert!(serialized.get("unclassifiedAddressLinks").is_none());
    assert!(serialized.get("knownLinks").is_none());
    let mut missing = document;
    change_address(&mut missing, 0, None);
    let (observed, topology) = ir_profile(&missing);
    assert_eq!(
        compare_structure(&expected, &observed, &native, &topology),
        ["address-links: expected at least 1, observed 0"]
    );
}

#[test]
fn a_duplicate_cannot_hide_a_different_missing_destination() {
    let source = [MDOC_HEADER, ".Mt first@example.test second@example.test\n"].concat();
    let (expected, native, mut document) = parsed_structure(&source);
    change_address(
        &mut document,
        1,
        Some(LinkTarget::Email {
            address: "first@example.test".to_owned(),
        }),
    );
    let (observed, topology) = ir_profile(&document);
    assert_eq!(observed.email_links, 2);
    assert_eq!(
        compare_structure(&expected, &observed, &native, &topology),
        ["known-link-targets: expected at least 2, observed 1"]
    );
}

fn violations(header: &str, body: &str) -> Vec<String> {
    let (expected, native, document) = parsed_structure(&[header, body].concat());
    let (observed, topology) = ir_profile(&document);
    compare_structure(&expected, &observed, &native, &topology)
}

fn assert_address_loss(header: &str, body: &str, property: &str, occurrences: usize) {
    let (expected, native, document) = parsed_structure(&[header, body].concat());
    let (observed, topology) = ir_profile(&document);
    assert_eq!(
        expected.known_links.match_counts(&observed.known_links),
        (occurrences, occurrences),
        "{body}"
    );
    assert!(
        compare_structure(&expected, &observed, &native, &topology).is_empty(),
        "{body}"
    );
    for index in 0..occurrences {
        let mut missing = document.clone();
        change_address(&mut missing, index, None);
        let (observed, topology) = ir_profile(&missing);
        let violations = compare_structure(&expected, &observed, &native, &topology);
        assert_eq!(
            violations,
            [format!(
                "{property}: expected at least {occurrences}, observed {}",
                occurrences - 1
            )],
            "{body}, occurrence {index}"
        );
    }
}

fn change_address(document: &mut Document, index: usize, replacement: Option<LinkTarget>) {
    struct Change {
        wanted: usize,
        seen: usize,
        changed: bool,
        replacement: Option<LinkTarget>,
    }
    impl VisitMut for Change {
        fn visit_inline_mut(&mut self, inline: &mut Inline) {
            if let Inline::Link {
                target, children, ..
            } = inline
                && matches!(
                    target,
                    LinkTarget::External { .. }
                        | LinkTarget::Email { .. }
                        | LinkTarget::Manual { .. }
                )
            {
                let index = self.seen;
                self.seen += 1;
                if index == self.wanted {
                    if let Some(replacement) = self.replacement.take() {
                        *target = replacement;
                    } else {
                        *inline = Inline::Text {
                            value: inline_plain_text(children),
                        };
                    }
                    self.changed = true;
                }
            }
            mant_ir::visit::walk_inline_mut(self, inline);
        }
    }
    let mut change = Change {
        wanted: index,
        seen: 0,
        changed: false,
        replacement,
    };
    change.visit_document_mut(document);
    assert!(change.changed, "no address occurrence {index}");
}
