//! Regressions from Arch Linux libarchive's `bsdunzip(1)` page.

use mant_engine::{render_excerpt_markdown, select_explanation};
use mant_ir::{Block, DefinitionRole};

use crate::{
    common::{collect_sections, inline_text},
    fixtures::{archlinux_manual, archlinux_manual_query},
};

#[test]
fn distinct_option_heads_share_the_following_mdoc_description() {
    let document = archlinux_manual("bsdunzip");
    let mut sections = Vec::new();
    collect_sections(&document.sections, &mut sections);
    let description = sections
        .into_iter()
        .find(|section| section.title == "DESCRIPTION")
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
            item.identity.as_ref().is_some_and(|identity| {
                identity.role == DefinitionRole::Option
                    && identity.names == ["-I".to_owned(), "-O".to_owned()]
            })
        })
        .expect("-I and -O must form one semantic definition");

    assert_eq!(
        encoding
            .terms
            .iter()
            .map(|term| inline_text(term))
            .collect::<Vec<_>>(),
        ["-I encoding", "-O encoding"]
    );
    let query = archlinux_manual_query("bsdunzip");
    for selector in ["-I", "-O"] {
        let excerpt = select_explanation(&query, selector)
            .unwrap_or_else(|error| panic!("explain {selector}: {error}"));
        assert!(
            render_excerpt_markdown(&excerpt)
                .contains("Convert filenames from the specified encoding.")
        );
    }
}
