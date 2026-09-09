//! Original source cases shared by IR, CLI and TUI assertions.
use super::*;

fn query(body: &str) -> ResolvedContent {
    mant_engine::query_roff_bytes(
        format!(".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n{body}\n").as_bytes(),
    )
    .unwrap()
}

fn assert_rows(query: &ResolvedContent, first: &str, last: &str, blanks: usize) {
    for text in
        std::iter::once(mant_engine::render_query_text(query)).chain([40, 80, 120].map(|width| {
            DocumentView::new(query)
                .render(width)
                .text
                .lines
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        }))
    {
        let rows: Vec<_> = text.lines().map(str::trim).collect();
        let start = rows.iter().position(|row| *row == first).unwrap();
        let end = rows.iter().position(|row| *row == last).unwrap();
        assert_eq!(end - start - 1, blanks, "{text}");
        assert!(rows[start + 1..end].iter().all(|row| row.is_empty()));
    }
}

#[test]
fn display_pp_keeps_independent_space_and_post_gap_target() {
    for mode in ["literal", "unfilled"] {
        for (body, blanks) in [
            ("ALPHA\n.sp 1\n.Pp\nBETA", 2),
            ("ALPHA\\c\n.Pp\nBETA", 1),
            ("ALPHA\n.Pp\n.Pp\nBETA", 1),
            ("ALPHA\n.Pp\n.sp 1\nBETA", 1),
        ] {
            let query = query(&format!(".Bd -{mode} -compact\n{body}\n.Ed"));
            assert_rows(&query, "ALPHA", "BETA", blanks);
        }
        let query = query(&format!(
            ".Bd -{mode} -compact\nALPHA\n.sp 1\n.Tg Paragraph.Target\n.Pp\nBETA\n.Ed"
        ));
        assert_rows(&query, "ALPHA", "BETA", 2);
        let document = query.document.as_ref().unwrap();
        assert!(mant_ir::validate_document(document).is_empty());
        let beta = document.sections[0]
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Preformatted { children, .. }
                    if children.iter().any(|inline| {
                        matches!(inline,
                        Inline::Text { value } if value == "BETA")
                    }) =>
                {
                    Some(children)
                }
                _ => None,
            })
            .unwrap();
        assert!(beta.iter().any(|inline| matches!(inline,
            Inline::Anchor { fragment_aliases, owner_source: Some(_), .. }
            if fragment_aliases.iter().any(|alias| alias.as_str() == "Paragraph.Target")
        )));
        let initial = self::query(&format!(".Bd -{mode} -compact\n.Pp\nINITIAL\n.Ed"));
        assert_rows(&initial, "TEST", "INITIAL", 1);
    }
}

#[test]
fn invisible_native_siblings_are_not_confused_with_transparent_controls() {
    // CVS print_bvspace uses roff_node_prev, not emitted words. groff may
    // apply a different initial no-space policy for the invisible positives.
    for mode in ["literal", "unfilled", "filled"] {
        for (prefix, blanks) in [
            ("", 0),
            ("\\fB\n", 1),
            (".Bf -emphasis\n.Ef\n", 1),
            // Native validation deletes an empty Bk entirely.
            (".Bk -words\n.Ek\n", 0),
            (".ft B\n", 0),
            (".Sm off\n", 0),
            (".Tg Display.Target\n", 0),
            (".\\\" comment\n", 0),
            (".if 0 HIDDEN\n", 0),
        ] {
            for nested in [false, true] {
                let body = format!("{prefix}.Bd -{mode}\nBODY\n.Ed");
                let body = if nested {
                    format!(".Bd -literal -compact\n{body}\n.Ed")
                } else {
                    body
                };
                let query = query(&body);
                assert_rows(&query, "TEST", "BODY", blanks);
                assert!(mant_ir::validate_document(query.document.as_ref().unwrap()).is_empty());
            }
        }
        // Entering a wrapper is not an earlier sibling of its own first
        // child. Only consuming the complete empty wrapper sets a predecessor.
        let content = query(&format!(".Bf -emphasis\n.Bd -{mode}\nBODY\n.Ed\n.Ef"));
        assert_rows(&content, "TEST", "BODY", 0);
        let content = query(&format!(
            ".Bd -literal -compact\n.Bf -emphasis\n.Bd -{mode}\nBODY\n.Ed\n.Ef\n.Ed"
        ));
        assert_rows(&content, "TEST", "BODY", 0);
    }
}
