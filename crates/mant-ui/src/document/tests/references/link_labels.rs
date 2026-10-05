//! Clickable label extents remain separate from surrounding text.
use super::super::{DocumentView, LinkTarget, model};
use unicode_width::UnicodeWidthStr;

#[test]
fn roff_manual_name_link_excludes_surrounding_prose_after_wrapping() {
    // Keep packaged unit tests self-contained. The engine's repository-level
    // self_manual_authoring test separately checks the actual shipped labels.
    let source = "The following [man(7)](https://mandoc.bsd.lv/man/man.7.html) macros documented by mandoc have dedicated lowering behavior:";
    let query = mant_loader::load_markdown_text(source, None).unwrap();
    let view = DocumentView::new(&query);
    let target = LinkTarget::External(
        model::ExternalUri::parse("https://mandoc.bsd.lv/man/man.7.html").unwrap(),
    );
    for width in [4, 12, 40, 120] {
        let rendered = view.render(width);
        let mut clickable = String::new();
        for (row, line) in rendered.text.lines.iter().enumerate() {
            let mut column = 0;
            for character in line.to_string().chars() {
                if rendered.link_target_at(row, column) == Some(&target) {
                    clickable.push(character);
                }
                column += character.to_string().width();
            }
        }
        assert_eq!(clickable, "man(7)", "width={width}");
    }
}

#[test]
fn empty_man_link_bodies_use_clickable_head_text_in_markdown_and_tui() {
    // Both exact sources were checked with fixed CVS -Thtml/-Tascii/-Tlint.
    // man_html.c::man_UR_pre() uses HEAD as the anchor label only when BODY
    // has no child; the terminal still prints the generated target brackets.
    for (open, close, address, destination) in [
        ("UR", "UE", "https://example.com", "https://example.com"),
        ("MT", "ME", "user@example.com", "mailto:user@example.com"),
    ] {
        let source = format!(".TH TEST 1\n.SH DESCRIPTION\n.{open} {address}\n.{close}\n");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower empty BODY");
        let markdown = mant_codec::encode::render_markdown(&query);
        assert!(markdown.contains(address), "{open}: {markdown}");
        assert!(!markdown.contains("[]("), "{open}: {markdown}");
        assert!(
            markdown.contains(&format!("<{destination}>"))
                || markdown.contains(&format!("[{address}]({destination})")),
            "{open}: {markdown}"
        );

        let target = LinkTarget::External(model::ExternalUri::parse(destination).unwrap());
        let view = DocumentView::new(&query);
        for width in [12, 40, 120] {
            let rendered = view.render(width);
            let mut clickable = String::new();
            for (row, line) in rendered.text.lines.iter().enumerate() {
                let mut column = 0;
                for character in line.to_string().chars() {
                    if rendered.link_target_at(row, column) == Some(&target) {
                        clickable.push(character);
                    }
                    column += character.to_string().width();
                }
            }
            assert_eq!(clickable, address, "{open}, width={width}");
        }
    }
}
