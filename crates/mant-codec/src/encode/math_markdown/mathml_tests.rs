//! Generated `MathML` crosses the same public Markdown entry points as LaTeX.

use super::*;
use crate::encode::{MarkdownFragmentOptions, MarkdownMath, render_inline_fragment};

fn options() -> MarkdownOptions {
    MarkdownOptions {
        math: MarkdownMath::Mathml,
        ..MarkdownOptions::default()
    }
}

fn literal(value: &str) -> Expr {
    Expr {
        atom_role: Role::Literal,
        ..atom(value)
    }
}

#[test]
fn documents_fragments_and_checked_exports_emit_inline_and_display_mathml() {
    let nodes = vec![inline(atom("x")), inline(atom("y"))];
    let content = query(vec![paragraph(nodes.clone()), display(Some(atom("z")), "")]);
    let artifact = crate::encode::render_addressable_markdown_with_options(&content, options());
    assert_eq!(artifact.text().matches("<math ").count(), 3);
    assert_eq!(artifact.text().matches("display=\"inline\"").count(), 2);
    assert_eq!(artifact.text().matches("display=\"block\"").count(), 1);
    assert_eq!(artifact.math_summary().encoded_formulas, 3);
    assert_eq!(artifact.math_summary().fallback_formulas, 0);
    assert_eq!(artifact.math_issues(), []);
    assert_eq!(math_events(artifact.text()), []);
    assert!(!artifact.text().contains("```"));
    let fragment = render_inline_fragment(
        &nodes,
        MarkdownFragmentOptions {
            math: MarkdownMath::Mathml,
            ..Default::default()
        },
    );
    assert_eq!(fragment.matches("<math ").count(), 2);
    assert!(fragment.contains("<mi>x</mi>"));
    let checked = render_math_markdown(&content, options(), MathPolicy::Strict).unwrap();
    assert_eq!(checked.markdown().text(), artifact.text());
    assert_eq!(checked.summary(), artifact.math_summary());
}

#[test]
fn mathematical_text_cannot_create_markdown_or_html_owners() {
    for value in [
        "a * b * c",
        "a _ b _ c",
        "`code`",
        "[x](https://example.test)",
        "![x](u)",
        "$x$",
        "a|b",
        "back\\slash",
        "<script> & &#42; &ast;",
        "a\nb\rc\td",
    ] {
        let fragment = render_inline_fragment(
            &[inline(literal(value))],
            MarkdownFragmentOptions {
                math: MarkdownMath::Mathml,
                ..Default::default()
            },
        );
        assert!(fragment.starts_with("<math "), "{value:?}: {fragment}");
        assert!(fragment.ends_with("</math>"));
        assert!(!fragment.contains('\n'));
        let mut recovered = String::new();
        for event in Parser::new_ext(&fragment, Options::ENABLE_MATH) {
            match event {
                Event::InlineHtml(tag) => assert!(!tag.contains("<script")),
                Event::Text(text) => recovered.push_str(&text),
                Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph) => {}
                other => panic!("unexpected event for {value:?}: {other:?}"),
            }
        }
        // Token blanks use NBSP so MathML retains their count and edges.
        assert_eq!(recovered, value.replace(' ', "\u{a0}"));
    }
    let markup = "<math><mi>&amp;ast; &lt; &#42; *</mi></math>";
    assert_eq!(
        super::super::mathml::markdown_fragment(markup).unwrap(),
        "<math><mi>&amp;ast; &lt; &#42; &#42;</mi></math>"
    );
}

#[test]
fn formula_styles_links_anchors_and_surrounding_markers_keep_their_owners() {
    let original = query(vec![paragraph(vec![
        Inline::Text {
            value: "*_[ ".into(),
        },
        Inline::anchor("formula"),
        Inline::Strong {
            children: vec![inline(literal("x*y"))],
        },
        Inline::Text {
            value: " and ".into(),
        },
        Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: "https://example.test/math".into(),
            },
            title: Some("math".into()),
            children: vec![Inline::Emphasis {
                children: vec![inline(literal("a_[b]"))],
            }],
        },
        Inline::Text {
            value: " ]_*".into(),
        },
    ])]);
    let before = original.clone();
    let artifact = crate::encode::render_addressable_markdown_with_options(
        &original,
        MarkdownOptions {
            preserve_anchors: true,
            ..options()
        },
    );
    let mut strong = 0;
    let mut emphasis = 0;
    let mut link = 0;
    let mut strong_text = String::new();
    let mut linked_text = String::new();
    for event in Parser::new(artifact.text()) {
        match event {
            Event::Start(Tag::Strong) => strong += 1,
            Event::End(TagEnd::Strong) => strong -= 1,
            Event::Start(Tag::Emphasis) => emphasis += 1,
            Event::End(TagEnd::Emphasis) => emphasis -= 1,
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) => {
                assert_eq!(dest_url.as_ref(), "https://example.test/math");
                assert_eq!(title.as_ref(), "math");
                link += 1;
            }
            Event::End(TagEnd::Link) => link -= 1,
            Event::Text(text) if strong > 0 => {
                assert_eq!(strong, 1);
                strong_text.push_str(&text);
            }
            Event::Text(text) if link > 0 => {
                assert_eq!((link, emphasis), (1, 1));
                linked_text.push_str(&text);
            }
            Event::Start(Tag::Image { .. }) => panic!("math did not author an image"),
            _ => {}
        }
    }
    assert_eq!(strong_text, "x*y", "{}", artifact.text());
    assert_eq!(linked_text, "a_[b]", "{}", artifact.text());
    assert_eq!(artifact.math_summary().encoded_formulas, 2);
    assert!(artifact.text().contains("id=\"formula\""));
    assert_eq!(original, before);
}

#[test]
fn display_cache_variants_share_owner_and_loss_accounting_in_both_orders() {
    for order in [[false, true], [true, false]] {
        let context = MathContext::export(true, MarkdownMath::Mathml);
        let expression = Expr {
            atom_role: Role::Unknown,
            ..atom("x")
        };
        for display in order {
            let markup = context.encoded(&expression, "x", display).unwrap();
            let mode = if display { "block" } else { "inline" };
            assert!(markup.contains(&format!("display=\"{mode}\"")));
        }
        let (issues, summary) = context.finish().unwrap();
        assert_eq!(summary.formulas, 1);
        assert_eq!(summary.encoded_formulas, 1);
        assert_eq!(summary.issues, 1);
        assert_eq!(issues.len(), 1);
    }
}

#[test]
fn contexts_invalid_scalars_and_markup_expansion_have_bounded_recovery() {
    let content = query(vec![
        Block::Preformatted {
            children: vec![inline(atom("x"))],
            language: None,
            inline_layout: mant_ir::InlineLayout::default(),
            layout: mant_ir::LayoutHint::default(),
            source: None,
        },
        display(None, "legacy"),
        paragraph(vec![inline(literal("\u{1}")), inline(atom("tail"))]),
    ]);
    let artifact = crate::encode::render_addressable_markdown_with_options(&content, options());
    assert_eq!(artifact.math_summary().formulas, 4);
    assert!(
        artifact
            .math_issues()
            .iter()
            .any(|i| i.kind == MarkdownMathIssueKind::LiteralContext)
    );
    assert!(
        artifact
            .math_issues()
            .iter()
            .any(|i| i.kind == MarkdownMathIssueKind::MissingStructure)
    );
    assert!(artifact.math_issues().iter().any(|i| matches!(
        i.kind,
        MarkdownMathIssueKind::Encoding(crate::math::MathLossKind::UnsupportedCharacter)
    )));
    assert!(artifact.text().contains("<mi>tail</mi>"));
    assert!(render_math_markdown(&content, options(), MathPolicy::Strict).is_err());
    let markup = format!(
        "<math><mi>{}</mi></math>",
        "*".repeat(crate::math::MAX_MATH_OUTPUT_BYTES / 4)
    );
    assert_eq!(
        super::super::mathml::markdown_fragment(&markup),
        Err(crate::math::MathError::LimitExceeded {
            limit: crate::math::MathLimit::OutputBytes
        })
    );
}
