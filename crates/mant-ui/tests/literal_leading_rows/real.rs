//! Full real-page samples use the immutable corpus, never host man databases.
use crate::{ResolvedContent, literal_texts, restored};

#[test]
fn posix_shell_and_awk_examples_start_with_the_authored_command() {
    // Both exact gzip members were decompressed and run through pinned pristine
    // UTF-8 before adding these assertions (status 0, empty stderr). Their
    // EXAMPLES sources explicitly spell .nf, one empty TEXT, command, .fi.
    // This test applies the approved reading exception to that native row;
    // all original sources and license/provenance records remain immutable.
    for (name, commands) in [
        ("sh.1p.gz", vec!["sh -c \"cat myfile\"", "sh my_shell_cmds"]),
        ("awk.1p.gz", vec!["$3 > 5", "(NR % 10) == 0"]),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/roff/real/archlinux")
            .join(name);
        let mut document = mant_loader::parse_manual_source(&path).unwrap();
        let examples = document
            .sections
            .iter()
            .find(|section| section.heading.plain_text() == "EXAMPLES")
            .unwrap()
            .clone();
        document.blocks.clear();
        document.sections = vec![examples];
        let loaded = ResolvedContent {
            label: name.into(),
            address: None,
            document: Some(document),
            tldr: None,
        };
        let content = restored(&loaded);
        let literals = literal_texts(&content);
        for command in commands {
            let body = literals.iter().find(|body| body.contains(command)).unwrap();
            assert_eq!(
                body, command,
                "{name}: the complete code owner has no initial row"
            );
            for width in [80, 120] {
                let rendered = mant_ui::DocumentView::new(&content).render(width);
                assert_eq!(
                    rendered.search(command).len(),
                    1,
                    "{name}/{command}/{width}"
                );
            }
        }
        let markdown = mant_codec::encode::render_markdown(&content);
        let read = mant_loader::load_markdown_text(&markdown, None).unwrap();
        assert_eq!(literal_texts(&read), literals, "{name}");
    }
}
