//! Independent, memory-only consumer of the codec's default public surface.

use mant_codec::{
    ResolvedContent, TldrPageLocation,
    encode::{
        MarkdownFragmentOptions, MarkdownNode, MarkdownOptions, render_addressable_markdown,
        render_blocks_fragment, render_markdown_with_options, render_sections_fragment,
    },
    parse_markdown, parse_tldr_command, parse_tldr_page,
};

fn exercise_public_codec() -> Result<(), Box<dyn std::error::Error>> {
    let source = "# Tool\n\nRoot café text.\n\n## Options\n\n<!-- mant:entries role=option case=sensitive -->\n- `--help`: Show help.\n";
    let parsed = parse_markdown(source, Some("unloaded/工具.md".to_owned()))?;
    assert!(parsed.document.diagnostics.is_empty());
    let tldr = parse_tldr_page(
        "# tool\n\n> Work with documents.\n\n- Read a document:\n\n`tool {{document}}`\n",
        TldrPageLocation {
            platform: "consumer-supplied".to_owned(),
            language: "und".to_owned(),
            source_path: "https://not-loaded.invalid/tool.md".to_owned(),
        },
    )?;
    assert_eq!(tldr.examples.len(), 1);
    assert_eq!(tldr.platform, "consumer-supplied");
    assert!(!parse_tldr_command("tool {{document}}").is_empty());
    let content = ResolvedContent {
        address: None,
        label: "tool".to_owned(),
        document: Some(parsed.document),
        tldr: Some(tldr),
    };
    let artifact = render_addressable_markdown(&content);
    assert_eq!(
        artifact.text(),
        render_markdown_with_options(&content, MarkdownOptions::ADDRESSABLE)
    );
    assert!(artifact.text().contains("Root café text."));
    assert!(artifact.text().contains("## TLDR"));
    assert!(artifact.section(usize::MAX).is_none());
    assert!(artifact.anchor_ranges().iter().all(|range| {
        artifact
            .text()
            .get(range.clone())
            .is_some_and(|text| text.starts_with("<a id="))
    }));
    let mut entry_count = 0;
    for mapped in artifact.nodes() {
        let text = artifact
            .text()
            .get(mapped.range())
            .expect("UTF-8 mapped range");
        if let MarkdownNode::DocumentEntry {
            names,
            owner,
            section,
            ..
        } = mapped.node()
        {
            entry_count += 1;
            assert_eq!(*names, ["--help"]);
            assert_eq!(owner.facts().unwrap().names, ["--help"]);
            assert!(text.contains("Show help."));
            let section = artifact.section(section.expect("entry section")).unwrap();
            assert_eq!(section.path().to_string(), "1");
            assert_eq!(section.parent(), None);
            assert_eq!(section.section().heading.plain_text(), "Options");
        }
    }
    assert_eq!(entry_count, 1);
    let document = content.document.as_ref().unwrap();
    let options = MarkdownFragmentOptions {
        preserve_anchors: true,
    };
    let mut fragments = render_blocks_fragment(&document.blocks, options);
    render_sections_fragment(&mut fragments, &document.sections, 2, options);
    let fragments = fragments.join("\n\n");
    assert!(fragments.contains("Root café text."));
    assert!(fragments.contains("--help"));
    assert!(!fragments.contains("mant:entries"));
    assert!(!fragments.contains("mant:entry"));
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    exercise_public_codec()
}

#[cfg(test)]
mod tests {
    #[test]
    fn standalone_markdown_tldr_and_encoding_need_no_native_or_host_surface() {
        super::exercise_public_codec().unwrap();
    }
}
