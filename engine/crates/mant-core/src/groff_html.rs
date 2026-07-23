//! Parses man-db/groff HTML when source-level libmandoc reports unsupported roff.

use mant_ast::{
    Block, DefinitionItem, DocumentMeta, DocumentSchema, DocumentSource, Inline, LayoutHint,
    ListItem, ListKind, MantDocument, Producer, Section, SourceFormat,
};
use scraper::{ElementRef, Html, Selector};

/// Normalize one complete `man -Thtml` document into `ManT`'s stable AST.
#[must_use]
pub fn parse_groff_html(html: &str, source_path: Option<String>) -> MantDocument {
    let document = Html::parse_document(html);
    let mut sections = Vec::new();
    let mut next_id = 1;
    if let Ok(body_selector) = Selector::parse("body")
        && let Some(body) = document.select(&body_selector).next()
    {
        parse_body(body, &mut sections, &mut next_id);
    }

    let mut sections = nest_sections(sections);
    crate::definitions::identify_definitions(&mut sections, &std::collections::HashSet::new());
    MantDocument {
        schema: DocumentSchema::V2,
        producer: Producer {
            name: "mant".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            engine: None,
        },
        source: DocumentSource {
            format: SourceFormat::GroffHtml,
            path: source_path,
            renderer: Some("man -Thtml".to_owned()),
        },
        meta: DocumentMeta::default(),
        diagnostics: Vec::new(),
        sections,
    }
}

struct FlatSection {
    level: u8,
    section: Section,
}

fn parse_body(body: ElementRef<'_>, sections: &mut Vec<FlatSection>, next_id: &mut usize) {
    for child in body.children() {
        if let Some(text) = child.value().as_text() {
            let text = normalize_text(text.text.as_ref());
            if !text.is_empty()
                && let Some(current) = sections.last_mut()
            {
                current.section.blocks.push(paragraph(text, 0));
            }
            continue;
        }
        let Some(element) = ElementRef::wrap(child) else {
            continue;
        };
        let tag = element.value().name();
        if matches!(tag, "h1" | "hr" | "br") || is_toc_link(element) {
            continue;
        }
        if let Some(level) = heading_level(tag) {
            let title = normalize_text(&element.text().collect::<String>());
            if !title.is_empty() {
                sections.push(FlatSection {
                    level,
                    section: Section {
                        id: format!("groff-section-{}", *next_id),
                        title,
                        spacing_before_lines: u16::from(!sections.is_empty()),
                        blocks: Vec::new(),
                        children: Vec::new(),
                        source: None,
                    },
                });
                *next_id += 1;
            }
            continue;
        }
        let Some(current) = sections.last_mut() else {
            continue;
        };
        if tag == "table" {
            current.section.blocks.extend(parse_layout_table(element));
        } else if let Some(block) = parse_block(element, parse_indent(element)) {
            current.section.blocks.push(block);
        }
    }
}

fn is_toc_link(element: ElementRef<'_>) -> bool {
    element.value().name() == "a"
        && element
            .value()
            .attr("href")
            .is_some_and(|href| href.starts_with('#'))
}

fn heading_level(tag: &str) -> Option<u8> {
    match tag {
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

fn nest_sections(mut flat: Vec<FlatSection>) -> Vec<Section> {
    fn take_one(flat: &mut [FlatSection], index: &mut usize) -> Section {
        let level = flat[*index].level;
        let mut section = std::mem::replace(
            &mut flat[*index].section,
            Section {
                id: String::new(),
                title: String::new(),
                spacing_before_lines: 0,
                blocks: Vec::new(),
                children: Vec::new(),
                source: None,
            },
        );
        *index += 1;
        while *index < flat.len() && flat[*index].level > level {
            section.children.push(take_one(flat, index));
        }
        section
    }

    let mut roots = Vec::new();
    let mut index = 0;
    while index < flat.len() {
        roots.push(take_one(&mut flat, &mut index));
    }
    roots
}

fn parse_layout_table(table: ElementRef<'_>) -> Vec<Block> {
    let (Ok(row_selector), Ok(cell_selector)) = (Selector::parse("tr"), Selector::parse("td"))
    else {
        return Vec::new();
    };
    let mut blocks = Vec::new();
    for row in table.select(&row_selector) {
        let mut cumulative_width = 0;
        for cell in row.select(&cell_selector) {
            let indent = percent_to_columns(cumulative_width);
            for child in cell.children().filter_map(ElementRef::wrap) {
                if matches!(child.value().name(), "p" | "pre" | "ul" | "ol" | "dl")
                    && let Some(block) = parse_block(child, indent)
                {
                    blocks.push(block);
                }
            }
            cumulative_width += cell
                .value()
                .attr("width")
                .and_then(parse_percentage)
                .unwrap_or(0);
        }
    }
    blocks
}

fn parse_block(element: ElementRef<'_>, indent_columns: u16) -> Option<Block> {
    let layout = LayoutHint {
        indent_columns,
        ..LayoutHint::default()
    };
    match element.value().name() {
        "p" => {
            let children = parse_inline_children(element, false);
            if children.is_empty() {
                Some(Block::VerticalSpace {
                    lines: 1,
                    source: None,
                })
            } else {
                Some(Block::Paragraph {
                    children,
                    layout,
                    source: None,
                })
            }
        }
        "pre" => {
            let mut children = parse_inline_children(element, true);
            trim_pre_boundaries(&mut children);
            (!children.is_empty()).then_some(Block::Preformatted {
                children,
                language: None,
                layout,
                source: None,
            })
        }
        "ul" | "ol" => parse_list(element, layout),
        "dl" => parse_definition_list(element, layout),
        _ => {
            let children = parse_inline_children(element, false);
            (!children.is_empty()).then_some(Block::Paragraph {
                children,
                layout,
                source: None,
            })
        }
    }
}

fn parse_list(element: ElementRef<'_>, layout: LayoutHint) -> Option<Block> {
    let kind = if element.value().name() == "ol" {
        ListKind::Ordered
    } else {
        ListKind::Bullet
    };
    let start = (kind == ListKind::Ordered)
        .then(|| element.value().attr("start")?.parse().ok())
        .flatten();
    let items = element
        .children()
        .filter_map(ElementRef::wrap)
        .filter(|child| child.value().name() == "li")
        .filter_map(|item| {
            let mut blocks = item
                .children()
                .filter_map(ElementRef::wrap)
                .filter_map(|child| {
                    matches!(child.value().name(), "p" | "pre" | "ul" | "ol" | "dl")
                        .then(|| parse_block(child, 0))
                        .flatten()
                })
                .collect::<Vec<_>>();
            if blocks.is_empty() {
                let children = parse_inline_children(item, false);
                if !children.is_empty() {
                    blocks.push(Block::Paragraph {
                        children,
                        layout: LayoutHint::default(),
                        source: None,
                    });
                }
            }
            (!blocks.is_empty()).then_some(ListItem { blocks })
        })
        .collect::<Vec<_>>();

    (!items.is_empty()).then_some(Block::List {
        kind,
        start,
        compact: false,
        items,
        layout,
        source: None,
    })
}

fn parse_definition_list(element: ElementRef<'_>, layout: LayoutHint) -> Option<Block> {
    let mut items = Vec::new();
    let mut terms = Vec::new();

    for child in element.children().filter_map(ElementRef::wrap) {
        match child.value().name() {
            "dt" => {
                let term = parse_inline_children(child, false);
                if !term.is_empty() {
                    terms.push(term);
                }
            }
            "dd" => {
                let mut description = child
                    .children()
                    .filter_map(ElementRef::wrap)
                    .filter_map(|nested| {
                        matches!(nested.value().name(), "p" | "pre" | "ul" | "ol" | "dl")
                            .then(|| parse_block(nested, 0))
                            .flatten()
                    })
                    .collect::<Vec<_>>();
                if description.is_empty() {
                    let children = parse_inline_children(child, false);
                    if !children.is_empty() {
                        description.push(Block::Paragraph {
                            children,
                            layout: LayoutHint::default(),
                            source: None,
                        });
                    }
                }
                if !terms.is_empty() || !description.is_empty() {
                    items.push(DefinitionItem {
                        identity: None,
                        inline_term: crate::mandoc::inline::terms_fit_inline(
                            &terms,
                            crate::mandoc::inline::DEFAULT_INLINE_TERM_MAX_WIDTH,
                        ),
                        terms: std::mem::take(&mut terms),
                        description,
                        spacing_before_lines: None,
                    });
                }
            }
            _ => {}
        }
    }
    if !terms.is_empty() {
        items.push(DefinitionItem {
            identity: None,
            inline_term: crate::mandoc::inline::terms_fit_inline(
                &terms,
                crate::mandoc::inline::DEFAULT_INLINE_TERM_MAX_WIDTH,
            ),
            terms,
            description: Vec::new(),
            spacing_before_lines: None,
        });
    }

    (!items.is_empty()).then_some(Block::DefinitionList {
        items,
        compact: false,
        layout,
        source: None,
    })
}

fn parse_inline_children(element: ElementRef<'_>, preserve_newlines: bool) -> Vec<Inline> {
    let mut children = Vec::new();
    for child in element.children() {
        if let Some(text) = child.value().as_text() {
            let value = if preserve_newlines {
                normalize_pre_text(text.text.as_ref())
            } else {
                normalize_inline_text(text.text.as_ref())
            };
            if !value.is_empty() {
                children.push(Inline::Text { value });
            }
        } else if let Some(element) = ElementRef::wrap(child) {
            children.extend(parse_inline_element(element, preserve_newlines));
        }
    }
    children
}

fn parse_inline_element(element: ElementRef<'_>, preserve_newlines: bool) -> Vec<Inline> {
    let children = parse_inline_children(element, preserve_newlines);
    match element.value().name() {
        "br" => vec![Inline::LineBreak],
        "b" | "strong" => (!children.is_empty())
            .then_some(Inline::Strong { children })
            .into_iter()
            .collect(),
        "i" | "em" => (!children.is_empty())
            .then_some(Inline::Emphasis { children })
            .into_iter()
            .collect(),
        "code" | "tt" => {
            let value = inline_text(&children);
            (!value.is_empty())
                .then_some(Inline::Code { value })
                .into_iter()
                .collect()
        }
        "a" => element
            .value()
            .attr("href")
            .map_or(children.clone(), |target| {
                if children.is_empty() {
                    Vec::new()
                } else {
                    vec![Inline::ExternalLink {
                        uri: target.to_owned(),
                        title: element.value().attr("title").map(str::to_owned),
                        children,
                    }]
                }
            }),
        _ => children,
    }
}

fn parse_indent(element: ElementRef<'_>) -> u16 {
    element
        .value()
        .attr("style")
        .and_then(|style| {
            style.split(';').find_map(|declaration| {
                let (name, value) = declaration.split_once(':')?;
                (name.trim().eq_ignore_ascii_case("margin-left"))
                    .then(|| parse_percentage(value.trim()))
                    .flatten()
            })
        })
        .map_or(0, percent_to_columns)
}

fn parse_percentage(value: &str) -> Option<u16> {
    value.trim().strip_suffix('%')?.trim().parse().ok()
}

fn percent_to_columns(percent: u16) -> u16 {
    ((u32::from(percent) * 80 + 50) / 100)
        .try_into()
        .unwrap_or(u16::MAX)
}

fn paragraph(value: String, indent_columns: u16) -> Block {
    Block::Paragraph {
        children: vec![Inline::Text { value }],
        layout: LayoutHint {
            indent_columns,
            ..LayoutHint::default()
        },
        source: None,
    }
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Collapse HTML formatting whitespace while keeping one boundary space.
///
/// Inline nodes are parsed independently, so dropping the whitespace around a
/// `<b>` or `<i>` element would accidentally concatenate adjacent words.
fn normalize_inline_text(value: &str) -> String {
    let mut normalized = String::new();
    let mut pending_space = false;
    for character in value.chars() {
        if character.is_whitespace() {
            pending_space = true;
        } else {
            if pending_space {
                normalized.push(' ');
                pending_space = false;
            }
            normalized.push(character);
        }
    }
    if pending_space {
        normalized.push(' ');
    }
    normalized
}

fn normalize_pre_text(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

/// Groff normally formats `<pre>` on separate HTML source lines. Those source
/// delimiters are not part of the rendered manual and must not create blank
/// terminal rows. Interior newlines remain untouched.
fn trim_pre_boundaries(children: &mut Vec<Inline>) {
    if let Some(value) = first_text_mut(children) {
        *value = value.strip_prefix('\n').unwrap_or(value).to_owned();
    }
    if let Some(value) = last_text_mut(children) {
        *value = value.strip_suffix('\n').unwrap_or(value).to_owned();
    }
    prune_empty_inline(children);
}

fn first_text_mut(children: &mut [Inline]) -> Option<&mut String> {
    for child in children {
        match child {
            Inline::Text { value } => return Some(value),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => {
                if let Some(value) = first_text_mut(children) {
                    return Some(value);
                }
            }
            Inline::Code { .. } | Inline::Anchor { .. } | Inline::LineBreak => {}
        }
    }
    None
}

fn last_text_mut(children: &mut [Inline]) -> Option<&mut String> {
    for child in children.iter_mut().rev() {
        match child {
            Inline::Text { value } => return Some(value),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => {
                if let Some(value) = last_text_mut(children) {
                    return Some(value);
                }
            }
            Inline::Code { .. } | Inline::Anchor { .. } | Inline::LineBreak => {}
        }
    }
    None
}

fn prune_empty_inline(children: &mut Vec<Inline>) {
    for child in children.iter_mut() {
        match child {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => prune_empty_inline(children),
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Anchor { .. }
            | Inline::LineBreak => {}
        }
    }
    children.retain(|child| match child {
        Inline::Text { value } | Inline::Code { value } => !value.is_empty(),
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::ExternalLink { children, .. }
        | Inline::EmailLink { children, .. }
        | Inline::ManualReference { children, .. }
        | Inline::SectionReference { children, .. } => !children.is_empty(),
        Inline::Anchor { .. } | Inline::LineBreak => true,
    });
}

fn inline_text(children: &[Inline]) -> String {
    let mut value = String::new();
    for child in children {
        match child {
            Inline::Text { value: text } | Inline::Code { value: text } => value.push_str(text),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => value.push_str(&inline_text(children)),
            Inline::Anchor { .. } => {}
            Inline::LineBreak => value.push('\n'),
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use mant_ast::{Block, Inline, LayoutHint, SourceFormat};

    use super::{inline_text, parse_groff_html};

    #[test]
    fn parses_sections_indentation_and_inline_formatting() {
        let document = parse_groff_html(
            r##"<body>
              <h1>TEST(1)</h1><a href="#NAME">NAME</a><br><hr>
              <h2>NAME<a name="NAME"></a></h2>
              <p style="margin-left:9%">See <b>mant</b> and <i>friends</i>.</p>
              <h2>OPTIONS</h2><h3>Output</h3><p>details</p>
            </body>"##,
            None,
        );

        assert_eq!(document.source.format, SourceFormat::GroffHtml);
        assert_eq!(document.sections.len(), 2);
        assert_eq!(document.sections[0].title, "NAME");
        assert_eq!(document.sections[1].children[0].title, "Output");
        let Block::Paragraph {
            children, layout, ..
        } = &document.sections[0].blocks[0]
        else {
            panic!("expected paragraph");
        };
        assert_eq!(layout.indent_columns, 7);
        assert!(matches!(children[1], Inline::Strong { .. }));
        assert!(matches!(children[3], Inline::Emphasis { .. }));
        assert_eq!(inline_text(children), "See mant and friends.");
    }

    #[test]
    fn flattens_groff_layout_tables_with_cumulative_indentation() {
        let document = parse_groff_html(
            r#"<body><h2>OPTIONS</h2><table><tr>
              <td width="9%"></td><td width="3%"><p><b>-c</b></p></td>
              <td width="6%"></td><td width="82%"><p>sort by ctime</p></td>
            </tr></table></body>"#,
            None,
        );
        let blocks = &document.sections[0].blocks;
        let [Block::DefinitionList { items, layout, .. }] = blocks.as_slice() else {
            panic!("hanging groff table should become a semantic definition");
        };
        assert_eq!(layout.indent_columns, 7);
        assert!(
            items[0]
                .identity
                .as_ref()
                .is_some_and(|identity| { identity.names == ["-c"] })
        );
        assert!(matches!(
            items[0].description.as_slice(),
            [Block::Paragraph {
                layout: mant_ast::LayoutHint {
                    indent_columns: 3,
                    ..
                },
                ..
            }]
        ));
    }

    #[test]
    fn removes_only_html_source_boundaries_from_preformatted_content() {
        let document = parse_groff_html(
            "<body><h2>EXAMPLES</h2><pre>\n<b>git</b> one\ngit two\n</pre></body>",
            None,
        );
        let Block::Preformatted { children, .. } = &document.sections[0].blocks[0] else {
            panic!("expected preformatted block");
        };
        assert_eq!(inline_text(children), "git one\ngit two");
        assert!(matches!(children[0], Inline::Strong { .. }));
    }

    #[test]
    fn normalizes_inline_whitespace_and_preserves_breaks_through_transparent_tags() {
        let document = parse_groff_html(
            "<body><h2>TEXT</h2><p>alpha\n<font color=red><b>beta</b></font>\t<i>gamma</i><br>delta</p></body>",
            None,
        );
        let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
            panic!("expected paragraph");
        };

        assert_eq!(inline_text(children), "alpha beta gamma\ndelta");
        assert!(matches!(children[1], Inline::Strong { .. }));
        assert!(matches!(children[3], Inline::Emphasis { .. }));
        assert!(matches!(children[4], Inline::LineBreak));
    }

    #[test]
    fn parses_native_html_lists_and_definition_lists() {
        let document = parse_groff_html(
            r"<body><h2>OPTIONS</h2>
              <ul><li>first</li><li><p>second</p></li></ul>
              <dl><dt><b>-a</b></dt><dt><b>--all</b></dt>
                  <dd><p>Show all entries.</p><pre>ls -a</pre></dd></dl>
            </body>",
            None,
        );

        assert!(matches!(
            document.sections[0].blocks[0],
            Block::List { ref items, .. } if items.len() == 2
        ));
        let Block::DefinitionList { items, .. } = &document.sections[0].blocks[1] else {
            panic!("expected definition list");
        };
        assert_eq!(items[0].terms.len(), 2);
        assert_eq!(items[0].description.len(), 2);
        assert!(matches!(
            items[0].description[1],
            Block::Preformatted { .. }
        ));
    }

    #[test]
    fn ignores_empty_table_rows_and_restarts_widths_for_each_row() {
        let document = parse_groff_html(
            r#"<body><h2>TABLE</h2><table>
              <tr><td width="100%"></td></tr>
              <tr><td width="15%"></td><td width="85%"><p>twelve</p></td></tr>
              <tr><td width="25%"></td><td width="75%"><p>twenty</p></td></tr>
            </table></body>"#,
            None,
        );

        assert!(matches!(
            document.sections[0].blocks.as_slice(),
            [
                Block::Paragraph {
                    layout: LayoutHint {
                        indent_columns: 12,
                        ..
                    },
                    ..
                },
                Block::Paragraph {
                    layout: LayoutHint {
                        indent_columns: 20,
                        ..
                    },
                    ..
                }
            ]
        ));
    }

    #[test]
    fn excludes_groff_document_chrome_from_sections() {
        let document = parse_groff_html(
            r##"<body>
              <h1>LS(1)</h1><a href="#NAME">NAME</a><br><hr>
              generated renderer text
              <h2>NAME<a name="NAME"></a></h2><p>ls - list files</p>
              <h2>DESCRIPTION</h2><p>List directory contents.</p>
            </body>"##,
            Some("ls.html".to_owned()),
        );

        assert_eq!(
            document
                .sections
                .iter()
                .map(|section| section.title.as_str())
                .collect::<Vec<_>>(),
            ["NAME", "DESCRIPTION"]
        );
        assert_eq!(document.source.path.as_deref(), Some("ls.html"));
        assert_eq!(document.sections[0].blocks.len(), 1);
    }

    #[test]
    fn preserves_indentation_across_repeated_layout_table_rows() {
        let document = parse_groff_html(
            r#"<body><h2>DESCRIPTION</h2><table>
              <tr><td width="9%"></td><td width="91%"><p>first</p></td></tr>
              <tr><td width="9%"></td><td width="91%"><p>second</p></td></tr>
              <tr><td width="9%"></td><td width="91%"><p>third</p></td></tr>
            </table></body>"#,
            None,
        );
        let indented = document.sections[0]
            .blocks
            .iter()
            .filter(|block| {
                matches!(
                    block,
                    Block::Paragraph {
                        layout: mant_ast::LayoutHint {
                            indent_columns: 7,
                            ..
                        },
                        ..
                    }
                )
            })
            .count();
        assert_eq!(indented, 3);
    }
}
