//! Small upstream-contract examples; assert semantics as well as visible text.
use super::*;

fn links(document: &mant_ir::Document) -> Vec<(mant_ir::LinkTarget, String)> {
    struct Collector(Vec<(mant_ir::LinkTarget, String)>);
    impl<'ir> Visit<'ir> for Collector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Link {
                target, children, ..
            } = inline
            {
                self.0.push((target.clone(), inline_text(children)));
            }
            visit::walk_inline(self, inline);
        }
    }
    let mut collector = Collector(Vec::new());
    collector.visit_document(document);
    collector.0
}

#[test]
fn mt_retains_each_address_with_its_own_target() {
    for body in [
        ".Mt first@example.com second@example.com .",
        ".Mt first@example.com , second@example.com .",
        ".No Contact Mt first@example.com second@example.com .",
        ".TS\nl.\nT{\n.Mt first@example.com second@example.com .\nT}\n.TE",
    ] {
        let document = mdoc(body);
        let actual = links(&document);
        assert_eq!(actual.len(), 2, "{body}: {actual:?}");
        for ((target, label), address) in actual
            .iter()
            .zip(["first@example.com", "second@example.com"])
        {
            assert_eq!(label, address);
            assert_eq!(
                target,
                &mant_ir::LinkTarget::Email {
                    address: address.into()
                }
            );
        }
    }
}

fn mdoc(body: &str) -> mant_ir::Document {
    let source = format!(".Dd September 7, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n{body}\n");
    parse_manual_bytes(std::path::Path::new("probe.1"), source.as_bytes()).unwrap()
}

#[test]
fn fo_counts_operands_without_counting_controls_or_targets() {
    for (body, expected) in [
        (".Fa int size_t", "probe(int, size_t)"),
        (
            ".Fa \"const char *path\" \"int flags\"",
            "probe(const char *path, int flags)",
        ),
        (".Tg anchor\n.Fa int", "probe(int)"),
        (".Fa int\n.Sm off\n.Fa char", "probe(int, char)"),
        (".Fa int\n.Tg anchor\n.Fa char", "probe(int, char)"),
    ] {
        let document = mdoc(&format!(".Fo probe\n{body}\n.Fc"));
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{document:?}")
        };
        assert_eq!(inline_text(children), expected, "{body}");
        assert!(
            children
                .iter()
                .any(|node| matches!(node, Inline::Emphasis { .. })),
            "{children:?}"
        );
        if body.contains(".Tg") {
            assert!(anchor_ids(&document).iter().any(|id| id == "anchor"));
        }
    }
    let document = mdoc(".Fa int size_t");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("{document:?}")
    };
    assert_eq!(inline_text(children), "int size_t");
}
