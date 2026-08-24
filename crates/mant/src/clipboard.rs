//! System clipboard integration for the interactive reader.

use mant_ui::{CopyFormat, CopyRequest, MAX_COPY_BYTES};

#[derive(Default)]
pub(super) struct SystemClipboard {
    clipboard: Option<arboard::Clipboard>,
}

impl SystemClipboard {
    pub(super) fn copy(&mut self, request: CopyRequest) -> Result<(), String> {
        let text = render_copy_request(request)?;
        if text.len() > MAX_COPY_BYTES {
            return Err("clipboard content exceeds the 4 MiB limit".to_owned());
        }
        let clipboard = match &mut self.clipboard {
            Some(clipboard) => clipboard,
            None => self.clipboard.insert(
                arboard::Clipboard::new()
                    .map_err(|error| format!("could not access the clipboard: {error}"))?,
            ),
        };
        clipboard
            .set_text(text)
            .map_err(|error| format!("could not copy to the clipboard: {error}"))
    }
}

fn render_copy_request(request: CopyRequest) -> Result<String, String> {
    match request {
        CopyRequest::Selection { text } => Ok(text),
        CopyRequest::Node {
            content,
            selector,
            format,
        } => {
            let excerpt = mant_engine::select_excerpt(content.as_ref(), &[selector])
                .map_err(|error| format!("could not select the current node: {error}"))?;
            Ok(match format {
                CopyFormat::Text => mant_engine::render_excerpt_text(&excerpt),
                CopyFormat::Markdown => mant_engine::render_excerpt_markdown(&excerpt),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mant_ir::ResolvedContent;
    use mant_protocol::NodeSelector;

    use super::{CopyFormat, CopyRequest, render_copy_request};

    #[test]
    fn requests_reuse_deterministic_semantic_renderers() {
        let content = Arc::new(
            mant_engine::query_markdown_text(
                "# Demo\n\n## Options\n\nUse `--help` for details.\n",
                Some("demo.md".to_owned()),
            )
            .expect("Markdown fixture"),
        );

        let text = render_copy_request(CopyRequest::Node {
            content: Arc::clone(&content),
            selector: NodeSelector::new("options"),
            format: CopyFormat::Text,
        })
        .expect("text node");
        let markdown = render_copy_request(CopyRequest::Node {
            content,
            selector: NodeSelector::new("options"),
            format: CopyFormat::Markdown,
        })
        .expect("Markdown node");

        assert!(text.contains("Options"));
        assert!(text.contains("--help"));
        assert!(markdown.contains("## Options"));
        assert!(markdown.contains("`--help`"));
    }

    #[test]
    fn visual_requests_are_not_reinterpreted() {
        let text = "rendered  text\nwithout Markdown reconstruction".to_owned();
        assert_eq!(
            render_copy_request(CopyRequest::Selection { text: text.clone() }).expect("selection"),
            text
        );
    }

    #[test]
    fn an_unknown_semantic_node_fails_before_clipboard_access() {
        let content = Arc::new(ResolvedContent {
            address: None,
            label: "empty".to_owned(),
            document: None,
            tldr: None,
        });
        let error = render_copy_request(CopyRequest::Node {
            content,
            selector: NodeSelector::new("missing"),
            format: CopyFormat::Text,
        })
        .expect_err("unknown node");

        assert!(error.starts_with("could not select the current node:"));
    }
}
