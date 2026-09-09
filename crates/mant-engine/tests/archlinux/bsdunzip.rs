//! Regressions from Arch Linux libarchive's `bsdunzip(1)` page.
use super::semantic_read;

use mant_engine::render_excerpt_markdown;
use mant_ir::{Block, EntryKind};

use crate::{
    common::{collect_sections, inline_text},
    fixtures::{archlinux_manual, archlinux_manual_query},
};

#[test]
fn distinct_option_heads_do_not_borrow_the_following_mdoc_description() {
    let document = archlinux_manual("bsdunzip");
    let mut sections = Vec::new();
    collect_sections(&document.sections, &mut sections);
    let description = sections
        .into_iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .expect("DESCRIPTION section");
    let items = description
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items),
            _ => None,
        })
        .expect("DESCRIPTION option list");
    let encoding = items
        .iter()
        .find(|item| {
            item.entry.as_ref().is_some_and(|identity| {
                identity.kind
                    == EntryKind::Parameter {
                        parameter_kind: mant_ir::ParameterKind::Option,
                    }
                    && identity.names == ["-O".to_owned()]
            })
        })
        .expect("-O must retain its own semantic definition");

    assert_eq!(
        encoding
            .terms
            .iter()
            .map(|term| inline_text(term))
            .collect::<Vec<_>>(),
        ["-O encoding"]
    );
    let query = archlinux_manual_query("bsdunzip");
    for selector in ["-I", "-O"] {
        let excerpt = semantic_read::semantic_excerpt(&query, &[selector])
            .unwrap_or_else(|error| panic!("explain {selector}: {error}"));
        assert_eq!(
            render_excerpt_markdown(&excerpt)
                .contains("Convert filenames from the specified encoding."),
            selector == "-O"
        );
    }
}
