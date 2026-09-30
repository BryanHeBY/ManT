use super::*;

#[test]
fn invisible_native_graph_preserves_following_accepted_field() {
    // Exact fixture ran through pinned pristine CVS -Tascii/-Tutf8/-Tlint.
    // term.c::term_fill() treats ASCII_NBRZW as graph despite zero width;
    // the second accepted pass ends its own physical row before Y prints.
    let item = review_definition_item(
        ".Bl -hang -width 4n\n.It Xo\n.No X\\p\n.No \"\\p\\&\"\n.No Y\n.Xc\n.No BodyWord\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "X\n\nY", "{item:#?}");
    assert!(
        matches!(&item.description[0], Block::Paragraph { children, .. }
        if inline_text(children) == "BodyWord"),
        "{item:#?}"
    );
}

#[test]
fn first_native_pass_rejection_drops_only_its_unprinted_field() {
    // Exact fixture ran through pinned pristine CVS -Tascii/-Tutf8/-Tlint.
    // term.c::term_flushln() stops on first nbr=0 and clears that buffer,
    // including a \z glyph already written by encode1(). BODY is a new field.
    let item = review_definition_item(
        ".Bl -hang -width 4n\n.It Xo\n.No \\p\n.No \\zY\n.No Z\n.Xc\n.No BodyWord\n.El\n",
    );
    assert!(
        item.terms.iter().all(|term| inline_text(term).is_empty()),
        "{item:#?}"
    );
    assert!(
        matches!(&item.description[0], Block::Paragraph { children, .. }
        if inline_text(children) == "BodyWord"),
        "{item:#?}"
    );
}

#[test]
fn head_rejection_cannot_revoke_a_body_after_a_real_flush() {
    // Exact fixture ran through pinned pristine CVS -Tascii/-Tutf8/-Tlint.
    // roff_term_pre_br() term_newln() consumes the old HEAD buffer. Rejected
    // HEAD bytes and the later BodyWord cannot share one rejection interval.
    let item = review_definition_item(
        ".Bl -inset\n.It Xo\n.No \"X\\p \\p Y\"\n.Xc\n.br\n.No BodyWord\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "X", "{item:#?}");
    assert!(
        item.description.iter().any(|block| matches!(block,
        Block::Paragraph { children, .. } if inline_text(children) == "\nBodyWord")),
        "{item:#?}"
    );
}

#[test]
fn overwritten_native_graphs_do_not_keep_a_rejected_owned_suffix() {
    // Exact CVS profiles retain the last overstrike glyph C, reject Z, and
    // retain BodyWord. encode1() keeps A/B as native graph but backspaces
    // their positions; source-cell acceptance cannot count them as IR glyphs.
    let item = review_definition_item(
        ".Bl -hang -width 4n\n.It Xo\n.No \"\\zA\\zBC \\p Z\"\n.Xc\n.No BodyWord\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "C", "{item:#?}");
    assert!(
        item.description.iter().any(|block| matches!(block,
        Block::Paragraph { children, .. } if inline_text(children) == "BodyWord")),
        "{item:#?}"
    );
    assert_no_private_field_markers(&item);
}

#[test]
fn explicit_flush_cannot_delete_a_prefix_already_accepted_by_an_earlier_pass() {
    // Exact CVS profiles keep X and BodyWord, reject Y. term_flushln()
    // consumes accepted passes before rejecting the remaining buffer; a
    // subsequent roff_term_pre_br() cannot revoke that committed prefix.
    let item = review_definition_item(
        ".Bl -hang -width 4n\n.It Xo\n.No \"X\\p \\p Y\"\n.br\n.Xc\n.No BodyWord\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "X", "{item:#?}");
    assert!(
        item.description.iter().any(|block| matches!(block,
        Block::Paragraph { children, .. } if inline_text(children) == "BodyWord")),
        "{item:#?}"
    );
    assert_no_private_field_markers(&item);
}

#[test]
fn native_acceptance_ranges_survive_link_and_style_wrappers() {
    // Exact CVS profiles keep X/Y on separate rows and reject Z, including
    // when Lk underlining owns both passes. Macro-generated URI spelling
    // executes after the label; typed Link identity is presentation metadata.
    let item = review_definition_item(
        ".Bl -hang -width 4n\n.It Xo\n.Lk https://example.org \"X\\p Y\" \"\\p Z\"\n.Xc\n.No BodyWord\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "X\nY", "{item:#?}");
    assert!(
        item.description.iter().any(|block| matches!(block,
        Block::Paragraph { children, .. } if inline_text(children) == "BodyWord")),
        "{item:#?}"
    );
    assert_no_private_field_markers(&item);
}

#[test]
fn hidden_source_operand_keeps_an_empty_native_owner_range() {
    // Exact CVS profiles retain X and URI, reject later Z. ManT compacts the
    // URI into navigation identity; that does not invalidate its native
    // buffer interval or permit a later rejected visible suffix to escape.
    let item = review_definition_item(
        ".Bl -hang -width 4n\n.It Xo\n.Lk \"https://example.org\\p \\p\" X\n.No Z\n.Xc\n.No BodyWord\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "X", "{item:#?}");
    assert!(
        item.description.iter().any(|block| matches!(block,
        Block::Paragraph { children, .. } if inline_text(children) == "BodyWord")),
        "{item:#?}"
    );
    assert_no_private_field_markers(&item);
}

fn assert_no_private_field_markers(item: &mant_ir::DefinitionItem) {
    fn check(nodes: &[Inline]) {
        for node in nodes {
            match node {
                Inline::Anchor { id, .. } => assert!(!id.as_str().starts_with('\0')),
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => check(children),
                _ => {}
            }
        }
    }
    for term in &item.terms {
        check(term);
    }
    for block in &item.description {
        if let Block::Paragraph { children, .. } | Block::Preformatted { children, .. } = block {
            check(children);
        }
    }
}
