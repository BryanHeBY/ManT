use super::*;

#[test]
fn inset_mid_word_marker_wipes_body_first_text() {
    // The exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. The inset
    // HEAD field flushes late (at the BODY word's NODE_LINE); its buffer
    // still holds the marker suffix, the generated separator, and the
    // BODY text, and the rejected pass wipes that whole buffer
    // (term.c:144-146 with 235): only the accepted prefix prints.
    let item = definition_item_from_source(
        ".Bl -inset\n.It Xo\n.No \"alpha \\p beta\"\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "alpha", "{item:#?}");
    let [
        Block::Paragraph { children, .. },
        Block::VerticalSpace { lines: 1, .. },
    ] = &item.description[..]
    else {
        panic!("expected only the rejected field's retained target: {item:#?}");
    };
    assert!(
        matches!(children.as_slice(), [Inline::Anchor { id, .. }] if id.as_str() == "tail"),
        "only navigation identity survives; no BODY text or row prints: {item:#?}"
    );
    assert!(
        !item.layout.inline_term(),
        "accepted prefix's row ended: {item:#?}"
    );
}

#[test]
fn inset_empty_operand_after_marker_breaks_when_the_body_consumes_its_field() {
    // The exact inset source passed fixed CVS -Tascii/-Tutf8/-Tlint.
    // Its empty No operand has no NODE_LINE and runs term_word(), not
    // term_vspace() (mdoc_term.c:354-378). The automatic blank after \p
    // is consumed only when the shared field reaches its BODY flush;
    // term_fill():287-306 then closes alpha before tail text. That real
    // boundary belongs to BODY output, not a predicted HEAD layout break.
    // The generated NBSP without an intervening empty operand is a graph
    // (term.c:347-350), so the armed-only case keeps its shared field.
    let inset = definition_item_from_source(
        ".Bl -inset\n.It Xo\n.No alpha\\p\n.No \"\"\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&inset.terms[0]), "alpha", "{inset:#?}");
    assert!(
        inset.layout.inline_term(),
        "HEAD leaves the raw field live: {inset:#?}"
    );
    let [Block::Paragraph { children, .. }] = &inset.description[..] else {
        panic!("expected shared BODY field output: {inset:#?}");
    };
    assert!(
        matches!(children.first(), Some(Inline::LineBreak { .. })),
        "BODY owns the executed marker boundary: {inset:#?}"
    );
    assert_eq!(inline_text(children), "\n tail text", "{inset:#?}");
    let diag = definition_item_from_source(
        ".Bl -diag\n.It Xo\n.No alpha\\p\n.No \"\"\n.Xc\n.No tail text\n.El\n",
    );
    assert!(
        diag.layout.inline_term(),
        "diag NOBREAK keeps the shared row: {diag:#?}"
    );
    let armed_only =
        definition_item_from_source(".Bl -inset\n.It Xo\n.No alpha\\p\n.Xc\n.No tail text\n.El\n");
    assert!(
        armed_only.layout.inline_term(),
        r"a trailing \p without the empty TEXT keeps the shared row: {armed_only:#?}"
    );
}

#[test]
fn inset_separate_marker_text_wipes_body_first_text() {
    // The exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. With the
    // marker in its own TEXT, the following word's separator meets the
    // armed field before any new graph: term.c:287-299 with 349-360 make
    // that pass reject, and the buffer wipe (term.c:144-146 with 235)
    // discards the suffix together with the run-in BODY text that still
    // shared the buffer. The native receipt decides this interval exactly
    // as it does for the single-TEXT case above; no BODY-wide latch remains.
    let item = definition_item_from_source(
        ".Bl -inset\n.It Xo\n.No one\n.No \\p\n.No two\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "one", "{item:#?}");
    let [
        Block::Paragraph { children, .. },
        Block::VerticalSpace { lines: 1, .. },
    ] = &item.description[..]
    else {
        panic!("expected only the rejected field's retained target: {item:#?}");
    };
    assert!(
        matches!(children.as_slice(), [Inline::Anchor { id, .. }] if id.as_str() == "tail"),
        "only navigation identity survives; no BODY text or row prints: {item:#?}"
    );
    assert!(
        !item.layout.inline_term(),
        "accepted prefix's row ended: {item:#?}"
    );
}

#[test]
fn diag_literal_head_marker_wipes_field_suffix() {
    // The exact source passed fixed CVS -Tascii/-Tutf8. Lint reports the
    // deliberate unmatched Xc: under
    // -diag the It HEAD does not parse extension blocks, so `.It Xo`
    // keeps the literal word "Xo" as its head (mdoc_macro.c:1112-1119)
    // and the extension's BODY words execute inside the still-open
    // NOBREAK field that termp_it_pre() configured before the HEAD
    // printed (mdoc_term.c:827-831) and that only the item's BODY post
    // term_newln() closes (mdoc_term.c:939-945). The marker's rejected
    // pass therefore wipes the unprinted suffix (term.c:144-146 with
    // 235): the accepted prefix prints on the head row and the joining
    // body text never does. CVS prints `Xo  alpha` followed by one blank
    // row: BRIND makes the rejected pass vfield=0, and trailspace=1
    // still executes the independent term_flushln()250-253 tail endline.
    let item = definition_item_from_source(
        ".Bl -diag\n.It Xo\n.No \"alpha \\p beta\"\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "Xo", "{item:#?}");
    assert!(
        item.layout.inline_term(),
        "diag NOBREAK keeps the head row: {item:#?}"
    );
    let [
        Block::Paragraph { children, .. },
        Block::VerticalSpace { lines: 1, .. },
    ] = &item.description[..]
    else {
        panic!("expected accepted run-in prefix and native tail row: {item:#?}");
    };
    assert_eq!(inline_text(children), "  alpha", "{item:#?}");
}

#[test]
fn cleared_no_break_field_wraps_hang_head_words_at_the_field_width() {
    // The exact source passed fixed CVS -Tascii. The `.sp` ran
    // term_vspace() then roff_term_pre_br(), which cleared TERMP_NOBREAK
    // (roff_term.c:71-78); the roff node returns before any flag restore
    // (mdoc_term.c:394-396), so the HEAD post flush fills the remaining
    // head words with vtarget=vfield (term.c:134-136): `after` and
    // `space` take separate rows at the list offset, and HANG keeps the
    // last row open for its body (term.c:250-253).
    let item = definition_item_from_source(
        ".Bl -hang -width 4n\n.It Xo\n.sp\n.No after space\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "\nafter\nspace", "{item:#?}");
    assert!(
        item.layout.inline_term(),
        "hang body stays on the last wrapped head row: {item:#?}"
    );
}

#[test]
fn no_fill_head_words_never_wrap_at_the_field_width() {
    // The exact source passed fixed CVS -Tascii. Even after `.nf` cleared
    // TERMP_NOBREAK through the shared roff_term_pre_br() dispatch
    // (roff_term.c:45-58), its NODE_NOFILL subtree prints under
    // TERMP_BRNEVER (mdoc_term.c:314-318): term_fill() runs with an
    // infinite target (term.c:143-144), so `after space` stays on one row
    // and only the BODY column follows.
    let item = definition_item_from_source(
        ".Bl -hang -width 4n\n.It Xo\n.nf\n.No after space\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "after space", "{item:#?}");
    assert!(
        !item.layout.inline_term(),
        "the fill-mode boundary closed the head row before BODY: {item:#?}"
    );
}

#[test]
fn tag_marker_split_head_closes_its_final_row() {
    // The exact sources passed fixed CVS -Tascii/-Tutf8/-Tlint. In-word
    // \p markers already flushed term_fill() passes, so the term_flushln()
    // tail rule (term.c:250-252) closes a NOBREAK-without-HANG (tag) row at
    // the final pass: BODY starts its own row even without a trailing
    // break. A HANG field keeps the shared row for its body.
    let tag = definition_item_from_source(
        ".Bl -tag -width 4n\n.It Xo\n.No \"x\\p y\\p z\"\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&tag.terms[0]), "x\ny\nz", "{tag:#?}");
    assert!(
        !tag.layout.inline_term(),
        "tag body must start its own row: {tag:#?}"
    );
    let hang = definition_item_from_source(
        ".Bl -hang -width 4n\n.It Xo\n.No \"x\\p y\\p z\"\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&hang.terms[0]), "x\ny\nz", "{hang:#?}");
    assert!(
        hang.layout.inline_term(),
        "hang body stays on the last row: {hang:#?}"
    );
}

#[test]
fn rejected_hang_link_field_does_not_reintroduce_a_wrapped_suffix() {
    // Exact HANG/TAG sources passed fixed CVS -Tascii/-Tutf8/-Tlint.
    // term.c::term_fill() returns nbr=0 when a \p is followed by the next
    // word's ordinary separator before any graph in that pass. A typed Lk
    // wrapper cannot turn the rejected suffix into a committed prefix.
    for style in ["hang", "tag"] {
        let item = definition_item_from_source(&format!(
            ".Bl -{style} -width 4n\n.It Xo\n.Lk https://example.com \\p QAXAQ\n.Xc\n.No BODY\n.El\n"
        ));
        assert!(
            !inline_text(&item.terms[0]).contains("QAXAQ"),
            "{style}: {item:#?}"
        );
        let Block::Paragraph { children, .. } = &item.description[0] else {
            panic!("expected BODY paragraph: {item:#?}");
        };
        assert_eq!(inline_text(children), "BODY");
    }
}

#[test]
fn diagnostic_xo_spelling_is_not_an_explicit_definition_head_scope() {
    // Exact -diag and -inset inputs passed fixed CVS -Ttree/-Tascii/-Tutf8/
    // -Tlint. mdoc_macro.c::blk_exp_close() breaks an intermediate It only
    // for an actual explicit block; -diag parses this Xo as literal TEXT.
    fn first_it_head(node: &libmandoc_rs::Node) -> Option<&libmandoc_rs::Node> {
        if node.kind == libmandoc_rs::NodeKind::Block && node.macro_token.as_deref() == Some("It") {
            return node
                .children
                .iter()
                .find(|child| child.kind == libmandoc_rs::NodeKind::Head);
        }
        node.children.iter().find_map(first_it_head)
    }
    let prefix =
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    for (list, head, expected_kind) in [
        ("diag", ".It Xo\n", libmandoc_rs::NodeKind::Text),
        (
            "inset",
            ".It Xo first\n.Xc\n",
            libmandoc_rs::NodeKind::Block,
        ),
    ] {
        let source = format!("{prefix}.Bl -{list}\n{head}.No BODY\n.El\n");
        let report = Parser::default()
            .parse_bytes("definition-head.1", source.as_bytes())
            .unwrap();
        let head = first_it_head(&report.document.root).expect("It HEAD");
        assert_eq!(head.children[0].kind, expected_kind, "{list}: {head:#?}");
        assert_eq!(
            head.children[0].flags.broken,
            list == "inset",
            "mdoc_macro.c::blk_exp_close() preserves native close evidence: {head:#?}"
        );
    }
}

#[test]
fn colon_breakpoint_wraps_the_head_row_like_the_reference() {
    // Fixed CVS -Tutf8: `\:` buffers ASCII_NBRZW on this device (chars.c:53
    // unicode column 0; term.c:631-632), a zero-width graph that can never
    // break a pass (term.c:340-349) and never prints (term.c:397). Every
    // operand therefore stays one row and BODY concatenates, however long
    // the tail: the wrapping shapes below are the ascii-device column
    // (ASCII_BREAK byte, term.c:287-300), kept for the -Tascii switch in
    // FieldCell::Breakpoint. All five rows verified against the pinned
    // reference: `X YYYYYZBODY` … `X YYYZBODY`.
    for (words, expected_rows) in [
        (r"YYYYY\:Z", 1),
        (r"YYYY\:ZZ", 1),
        (r"Y\:ZZZZ", 1),
        (r"YY\:ZZ", 1),
        (r"YYY\:Z", 1),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo X\n.br\n.No {words}\n.Xc\n.No BODY\n.El\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("colon-breakpoint-rows.1"),
            source.as_bytes(),
        )
        .unwrap();
        let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
            panic!("{words}: {document:#?}");
        };
        let row_breaks = items[0].terms[0]
            .iter()
            .filter(|node| matches!(node, Inline::LineBreak { .. }))
            .count();
        assert_eq!(row_breaks + 1, expected_rows, "{words}: {items:#?}");
    }
}
